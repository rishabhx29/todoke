//! Todoke desktop app — Tauri 2 shell over the todoke-core engine.
//!
//! Layout: `state` (long-lived engine state), `dto` (serde payloads),
//! `commands` (the IPC bridge). Identity honors `TODOKE_DATA_DIR` exactly
//! like the CLI so tests and portable use behave the same.

mod commands;
mod dto;
mod state;

use state::{EngineMutex, EngineState, data_dir};
use tauri::Manager;
use todoke_core::identity::DeviceIdentity;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let (identity, identity_error) = match data_dir() {
                Ok(dir) => match DeviceIdentity::load_or_create(&dir) {
                    Ok(id) => (Some(id), None),
                    Err(e) => (None, Some(format!("identity: {e}"))),
                },
                Err(e) => (None, Some(e)),
            };
            let data_dir = data_dir().unwrap_or_else(|_| std::env::temp_dir().join("todoke"));
            app.manage(EngineMutex::new(EngineState::new(
                identity,
                identity_error,
                data_dir,
            )));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_identity,
            commands::set_device_name,
            commands::list_devices,
            commands::send_files,
            commands::cancel_transfer,
            commands::receive_start,
            commands::receive_stop,
            commands::receive_status,
            commands::get_receive_target,
            commands::set_receive_target,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Todoke");
}
