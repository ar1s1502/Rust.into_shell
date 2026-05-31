use portable_pty::{CommandBuilder, native_pty_system, PtySize, PtySystem};
use anyhow::anyhow;
use thiserror;

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("pty init error: {0}")]
    Pty(#[from] anyhow::Error),
}

// we must manually implement serde::Serialize for Error 
impl serde::Serialize for Error {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: serde::ser::Serializer,
  {
    serializer.serialize_str(self.to_string().as_ref())
  }
}

#[tauri::command]
fn init_shell() -> Result<(), Error> {
    let mut pair = native_pty_system().openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }).map_err(|e| Error::Pty(anyhow!("{}", e)))?;

    let cmd = CommandBuilder::new("../target/debug/rust_shell");
    let child = pair.slave.spawn_command(cmd)
        .map_err(|e| Error::Pty(anyhow!("{}", e)))?;

    let pty_reader = pair.master.try_clone_reader() //read output from pty
        .map_err(|e| Error::Pty(anyhow!("{}", e)))?; 
    let pty_writer = pair.master.take_writer()
        .map_err(|e| Error::Pty(anyhow!("{}", e)))?;
    
    Ok(())

}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![init_shell, ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
