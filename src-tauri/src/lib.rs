use std::io::{Write,};
use std::sync::Mutex;
use std::fs::read_to_string;
use std::env;
use tauri::{Builder, Manager, ipc::Channel, State, Emitter};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use anyhow::anyhow;
use thiserror;
use vte;

const CMD_HISTORY: &str = "rust_shell_history.txt";  //match src/shell.rs
const OSC133: [u8; 3] = *b"133";
const PROMPT_START: [u8; 1] = *b"A";
const PROMPT_END: [u8; 1] = *b"B";
const CMD_OUTPUT_START: [u8; 1] = *b"C";
const CMD_END: [u8; 1] = *b"D";

struct Pty {
    writer: Box<dyn Write + Send>, //write handle to master pty, from take_writer()
    master: Box<dyn MasterPty + Send>, //master end of PtyPair
}
type PtyState = Mutex<Pty>;

struct PtyParser<'a> {
    shell_state: &'a [u8],
    app: tauri::AppHandle,
    output: Vec<u8>,
}

impl PtyParser<'_> {
    fn new(app_: tauri::AppHandle) -> Self {
        Self { shell_state: &CMD_END, app: app_, output: Vec::new() }
    }
}

impl vte::Perform for PtyParser<'_> {
    //got full osc seq
    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) { 
        if _params.is_empty() {return;}
        println!("[osc dispatch] params: {:?}", _params);
        if _params[0] == OSC133 {
            match _params[1] {
                val if val == PROMPT_START => {
                    self.shell_state = &PROMPT_START;
                    if _params.len() != 3 { return; }
                    let prompt = std::str::from_utf8(_params[2]).unwrap();
                    self.app.emit("prompt_continue", prompt).unwrap(); 
                },
                val if val == PROMPT_END => {
                    if self.shell_state != &PROMPT_START {
                        println!("ERR: invalid shell state transition");
                    }
                    self.shell_state = &PROMPT_END;
                    println!("PROMPT_END!!");
                },
                val if val == CMD_OUTPUT_START => {
                    if self.shell_state != CMD_END && self.shell_state != PROMPT_END {
                        println!("ERR: invalid shell state transition");
                    }
                    self.shell_state = &CMD_OUTPUT_START;
                    self.app.emit("output_start", ()).unwrap();
                },
                val if val == &CMD_END => {
                    if self.shell_state != CMD_OUTPUT_START {
                        println!("ERR: invalid shell state transition");
                    }
                    self.shell_state = &CMD_END;
                },
                _ => println!("ERR: invalid OSC133 sequence"),
            }
            println!("shell state: {:?}", self.shell_state);
        } 
    }
    //got full csi seq (e.g. enter fullscreen)
    fn csi_dispatch(
            &mut self,
            _params: &vte::Params,
            _intermediates: &[u8],
            _ignore: bool,
            _action: char,
        ) {
        //println!("[csi dispatch] params: {:?}, intermed: {:?}, action: {:?}", _params, _intermediates, _action);
    }

}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("pty init error: {0}")]
    Pty(#[from] anyhow::Error),

    #[error("pty io error: {0}")]
    PtyIO(#[from] std::io::Error),
}

// must manually implement serde::Serialize for Error enum
impl serde::Serialize for Error {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: serde::ser::Serializer,
  {
    serializer.serialize_str(self.to_string().as_ref())
  }
}

#[tauri::command]
fn get_editor_hist() -> Option<Vec<String>> { //returning None here is still a resolved js promise
    match env::home_dir() {
        Some(path) => {
            match read_to_string(&path.join(CMD_HISTORY)) {
                Ok(hist) => {
                    let normalized = hist.replace("\\r\\n", "\\n");
                    let mut line_iter = normalized.split('\n');
                    let _ = line_iter.next(); //skip the header
                    let mut hist = Vec::with_capacity(line_iter.size_hint().0);
                    while let Some(line) = line_iter.next() {
                        hist.push(line.replace("\\n", "\n"));
                    }
                    Some(hist)
                },
                Err(_) => None,
            }
        },
        None => None,
    }
}

#[tauri::command] 
fn pty_write(pty_: State<'_, PtyState>, cli_input: &str) -> Result<(), Error> {
    let mut pty = pty_.lock().unwrap();
    //will continuously call write until all of cli_input written; else err
    pty.writer.write_all(cli_input.as_bytes())
        .map_err(|e| Error::PtyIO(e))
}

#[tauri::command]
// pty_channel is initialized from frontend, passed here. frontend calls pty_read only once 
// and polls waiting for output on pty_channel
fn pty_read(app_handle: tauri::AppHandle, pty_: State<'_, PtyState>, pty_channel: Channel<Vec<u8>>) -> Result<(), Error> {
    let pty = pty_.lock().unwrap();
    let mut reader = pty.master.try_clone_reader()
        .map_err(|e| Error::Pty(anyhow!("{}", e)))?;
    let app = app_handle.clone();
    std::thread::spawn(move || {
        let mut buf = vec![0; 2048];
        let mut pty_parser = PtyParser::new(app);
        let mut statemachine = vte::Parser::new();
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break, //read EOF: pty slave child process (rust_shell) died early
                Ok(n) => {
                    for i in 0..n {
                        let should_forward = pty_parser.shell_state == CMD_OUTPUT_START;
                        statemachine.advance(&mut pty_parser, &buf[i..(i+1)]);
                        if should_forward {
                            pty_parser.output.push(buf[i]);
                        }
                    }
                    //send along channel
                    //channel must have owned data, not reference to data, else compiler complains
                    if !pty_parser.output.is_empty() {
                        println!("pty_parser.output: {:?}", std::str::from_utf8(&pty_parser.output).unwrap());
                        let _ = pty_channel.send(pty_parser.output);
                        pty_parser.output = Vec::new();
                    }
                },
                Err(_) => break,
            }
        }
    });

    Ok(())
}

fn init_shell() -> Result<Pty, Error> {
    let pty_pair = native_pty_system().openpty(PtySize {
        rows: 30,
        cols: 160,
        pixel_width: 0,
        pixel_height: 0,
    }).map_err(|e| Error::Pty(anyhow!("{}", e)))?;

    //TODO: replace with release binary, maybe use tauri sidecar
    let cmd = CommandBuilder::new("/Users/aris/projects/learnRust/rust_shell/target/debug/rust_shell");
    let _ = pty_pair.slave.spawn_command(cmd)
        .map_err(|e| Error::Pty(anyhow!("{}", e)))?;

    let pty_writer = pty_pair.master.take_writer()
        .map_err(|e| Error::Pty(anyhow!("{}", e)))?;
    
    Ok(Pty {
        writer: pty_writer,
        master: pty_pair.master,
    })
}


#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    Builder::default()
        .setup(|app| {
            //autogen'd boilerplate
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            //create pty and store in app state
            let ptystate = Mutex::new(init_shell()?);
            app.manage(ptystate);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![pty_write, pty_read, get_editor_hist])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
