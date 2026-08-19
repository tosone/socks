use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::error::{AppError, AppResult};

const PROXY_INSTALL_COMMAND_TEMPLATE: &str = r#"set -eu
if ! command -v docker >/dev/null 2>&1; then
  curl -fsSL https://get.docker.com | sh -s -- --mirror Aliyun
fi
sudo docker pull ghcr.io/tosone/socks-proxy:latest
sudo docker rm -f socks-proxy >/dev/null 2>&1 || true
sudo docker run -d --name socks-proxy --restart unless-stopped -p 39036:39036/tcp -p 39036:39036/udp -e SHADOWSOCKS_SERVER={proxy_server_ip} -e SHADOWSOCKS_PORT=39036 ghcr.io/tosone/socks-proxy:latest
sudo docker ps --filter name=socks-proxy"#;

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

    let command = install_command(&input);
    let lines = [
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
        command,
    ];

    for line in lines {
        emit_log(&app, InstallerLogStream::Stdout, line);
        tokio::time::sleep(Duration::from_millis(220)).await;
    }

    Ok(InstallerRunResult {
        exit_status: Some(0),
    })
}

fn install_command(input: &InstallerRunInput) -> String {
    PROXY_INSTALL_COMMAND_TEMPLATE.replace(
        "{proxy_server_ip}",
        &shell_quote(input.proxy_server_ip.trim()),
    )
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
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
