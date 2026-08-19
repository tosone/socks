use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::error::{AppError, AppResult};

const PROXY_INSTALL_COMMANDS: &[&str] = &[
    "echo Preparing remote environment",
    "echo Installing server dependencies",
    "echo Writing service configuration",
    "echo Installer placeholder completed",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallerRunInput {
    pub ip: String,
    pub port: u16,
    pub user: String,
    pub private_key_path: String,
    pub password: String,
    pub proxy_server_ip: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallerRunResult {
    pub exit_status: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallerRunEvent {
    stream: InstallerLogStream,
    data: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
enum InstallerLogStream {
    Stdout,
    System,
}

pub async fn run_sample(app: AppHandle, input: InstallerRunInput) -> AppResult<InstallerRunResult> {
    if input.ip.trim().is_empty() {
        return Err(AppError::msg("IP is required."));
    }
    if input.user.trim().is_empty() {
        return Err(AppError::msg("User is required."));
    }
    if input.private_key_path.trim().is_empty() && input.password.is_empty() {
        return Err(AppError::msg("Private key or password is required."));
    }
    if input.proxy_server_ip.trim().is_empty() {
        return Err(AppError::msg("Proxy server IP is required."));
    }

    emit_log(
        &app,
        InstallerLogStream::System,
        "Starting installer placeholder\n",
    );

    let mut lines = vec![
        format!(
            "echo Connecting to {}:{} as {}\n",
            input.ip.trim(),
            input.port,
            input.user.trim()
        ),
        format!(
            "echo Configuring Shadowsocks proxy for {}\n",
            input.proxy_server_ip.trim()
        ),
    ];
    lines.extend(
        PROXY_INSTALL_COMMANDS
            .iter()
            .map(|command| format!("{command}\n")),
    );

    for line in lines {
        emit_log(&app, InstallerLogStream::Stdout, line);
        tokio::time::sleep(Duration::from_millis(220)).await;
    }

    Ok(InstallerRunResult {
        exit_status: Some(0),
    })
}

fn emit_log(app: &AppHandle, stream: InstallerLogStream, data: impl Into<String>) {
    let _ = app.emit(
        "installer-run",
        InstallerRunEvent {
            stream,
            data: data.into(),
        },
    );
}
