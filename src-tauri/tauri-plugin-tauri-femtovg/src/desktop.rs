use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::models::*;

pub fn init<R: Runtime, C: DeserializeOwned>(
  app: &AppHandle<R>,
  _api: PluginApi<R, C>,
) -> crate::Result<TauriFemtovg<R>> {
  Ok(TauriFemtovg(app.clone()))
}

/// Access to the tauri-femtovg APIs.
pub struct TauriFemtovg<R: Runtime>(AppHandle<R>);

impl<R: Runtime> TauriFemtovg<R> {
  pub fn ping(&self, payload: PingRequest) -> crate::Result<PingResponse> {
    Ok(PingResponse {
      value: payload.value,
    })
  }
}
