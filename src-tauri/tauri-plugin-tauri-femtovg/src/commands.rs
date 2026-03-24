use tauri::{AppHandle, command, Runtime};

use crate::models::*;
use crate::Result;
use crate::TauriFemtovgExt;

#[command]
pub(crate) async fn ping<R: Runtime>(
    app: AppHandle<R>,
    payload: PingRequest,
) -> Result<PingResponse> {
    app.tauri_femtovg().ping(payload)
}
