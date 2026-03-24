use tauri::{
  plugin::{Builder, TauriPlugin},
  Manager, Runtime,
};

pub use models::*;

#[cfg(desktop)]
mod desktop;
#[cfg(mobile)]
mod mobile;

mod commands;
mod error;
mod models;

pub use error::{Error, Result};

#[cfg(desktop)]
use desktop::TauriFemtovg;
#[cfg(mobile)]
use mobile::TauriFemtovg;

/// Extensions to [`tauri::App`], [`tauri::AppHandle`] and [`tauri::Window`] to access the tauri-femtovg APIs.
pub trait TauriFemtovgExt<R: Runtime> {
  fn tauri_femtovg(&self) -> &TauriFemtovg<R>;
}

impl<R: Runtime, T: Manager<R>> crate::TauriFemtovgExt<R> for T {
  fn tauri_femtovg(&self) -> &TauriFemtovg<R> {
    self.state::<TauriFemtovg<R>>().inner()
  }
}

/// Initializes the plugin.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
  Builder::new("tauri-femtovg")
    .invoke_handler(tauri::generate_handler![commands::ping])
    .setup(|app, api| {
      #[cfg(mobile)]
      let tauri_femtovg = mobile::init(app, api)?;
      #[cfg(desktop)]
      let tauri_femtovg = desktop::init(app, api)?;
      app.manage(tauri_femtovg);
      Ok(())
    })
    .build()
}
