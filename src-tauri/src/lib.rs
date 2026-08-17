mod error;
mod helper;
mod macos_route;
mod profile;
mod session;
mod ssh;
mod sslocal;

use std::{collections::HashMap, fs};

use error::AppResult;
use helper::HelperStatus;
use profile::{Profile, ProfileInput, CIPHERS};
use session::{AppState, RuntimeStatus, TrafficTotals};
use ssh::{SshRunInput, SshRunResult};
use tauri::{Manager, RunEvent};

#[tauri::command]
async fn list_profiles(state: tauri::State<'_, AppState>) -> AppResult<Vec<Profile>> {
    Ok(state.list_profiles().await)
}

#[tauri::command]
async fn list_traffic_totals(
    state: tauri::State<'_, AppState>,
) -> AppResult<HashMap<String, TrafficTotals>> {
    Ok(state.list_traffic_totals().await)
}

#[tauri::command]
async fn create_profile(
    state: tauri::State<'_, AppState>,
    input: ProfileInput,
) -> AppResult<Profile> {
    state.create_profile(input).await
}

#[tauri::command]
async fn update_profile(
    state: tauri::State<'_, AppState>,
    id: String,
    input: ProfileInput,
) -> AppResult<Profile> {
    state.update_profile(&id, input).await
}

#[tauri::command]
async fn delete_profile(state: tauri::State<'_, AppState>, id: String) -> AppResult<()> {
    state.delete_profile(&id).await
}

#[tauri::command]
fn list_ciphers() -> Vec<String> {
    CIPHERS.iter().map(|s| s.to_string()).collect()
}

#[tauri::command]
async fn connect(state: tauri::State<'_, AppState>, id: String) -> AppResult<RuntimeStatus> {
    state.connect(&id).await
}

#[tauri::command]
async fn disconnect(state: tauri::State<'_, AppState>) -> AppResult<RuntimeStatus> {
    state.disconnect().await
}

#[tauri::command]
async fn runtime_status(state: tauri::State<'_, AppState>) -> AppResult<RuntimeStatus> {
    Ok(state.runtime_status().await)
}

#[tauri::command]
fn helper_status(state: tauri::State<'_, AppState>) -> HelperStatus {
    state.helper_status()
}

#[tauri::command]
fn install_helper(state: tauri::State<'_, AppState>) -> AppResult<HelperStatus> {
    state.install_helper()
}

#[tauri::command]
async fn uninstall_helper(state: tauri::State<'_, AppState>) -> AppResult<HelperStatus> {
    state.uninstall_helper().await
}

#[tauri::command]
async fn run_ssh_sample(app: tauri::AppHandle, input: SshRunInput) -> AppResult<SshRunResult> {
    ssh::run_sample(app, input).await
}

pub fn run_helper() {
    helper::run_helper();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            fs::create_dir_all(&data_dir)?;
            let state = AppState::load(data_dir, app.handle().clone())?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_profiles,
            list_traffic_totals,
            create_profile,
            update_profile,
            delete_profile,
            list_ciphers,
            connect,
            disconnect,
            runtime_status,
            helper_status,
            install_helper,
            uninstall_helper,
            run_ssh_sample
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            let state = app.state::<AppState>();
            tauri::async_runtime::block_on(state.shutdown());
        }
    });
}
