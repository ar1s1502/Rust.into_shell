use std::io::{Write,};
use std::sync::Mutex;
use tauri::{Builder, Manager, ipc::Channel, State};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use anyhow::anyhow;
use thiserror;

struct Pty {
    writer: Box<dyn Write + Send>, //write handle to master pty, from take_writer()
    master: Box<dyn MasterPty + Send>, //master end of PtyPair
}

type PtyState = Mutex<Pty>;

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
fn pty_write(pty_: State<'_, PtyState>, cli_input: &str) -> Result<(), Error> {
    let mut pty = pty_.lock().unwrap();
    //will continuously call write until all of cli_input written; else err
    pty.writer.write_all(cli_input.as_bytes())
        .map_err(|e| Error::PtyIO(e))
}

#[tauri::command]
// pty_channel is initialized from frontend, passed here. frontend calls pty_read only once 
// and polls waiting for output on pty_channel
fn pty_read(pty_: State<'_, PtyState>, pty_channel: Channel<Vec<u8>>) -> Result<(), Error> {
    let pty = pty_.lock().unwrap();
    let mut reader = pty.master.try_clone_reader()
        .map_err(|e| Error::Pty(anyhow!("{}", e)))?;

    std::thread::spawn(move || {
        let mut buf = vec![0; 2048];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break, //read EOF: pty slave child process (rust_shell) died early
                Ok(n) => {
                    //send along channel
                    //channel must have owned data, not reference to data, else compiler complains
                    let output = buf[..n].to_vec();
                    let _ = pty_channel.send(output);
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
        .invoke_handler(tauri::generate_handler![pty_write, pty_read])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
