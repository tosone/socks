use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use russh::client;
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg};
use russh::{ChannelMsg, Disconnect};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::net::ToSocketAddrs;

use crate::error::{AppError, AppResult};
use crate::password;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshRunInput {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_mode: SshAuthMode,
    pub private_key_path: Option<String>,
    pub password: Option<String>,
    pub service_password: String,
    pub method: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SshAuthMode {
    Key,
    Password,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshRunResult {
    pub exit_status: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SshRunEvent {
    stream: SshLogStream,
    data: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
enum SshLogStream {
    Stdout,
    Stderr,
    System,
}

struct Client;

impl client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

struct Session {
    session: client::Handle<Client>,
}

impl Session {
    async fn connect<A: ToSocketAddrs>(
        input: &SshRunInput,
        addrs: A,
        app: &AppHandle,
    ) -> AppResult<Self> {
        let username = username(input);
        let config = Arc::new(client::Config {
            inactivity_timeout: Some(Duration::from_secs(15)),
            ..Default::default()
        });
        let mut session = client::connect(config, addrs, Client)
            .await
            .map_err(|err| AppError::msg(format!("Failed to connect SSH server: {err}")))?;

        match input.auth_mode {
            SshAuthMode::Key => {
                let key_path = input
                    .private_key_path
                    .as_deref()
                    .map(str::trim)
                    .filter(|path| !path.is_empty())
                    .ok_or_else(|| AppError::msg("Private key path is required."))?;
                emit_log(
                    app,
                    SshLogStream::System,
                    format!("Using key: {key_path}\n"),
                );
                let key_pair = load_secret_key(expand_home(key_path), None)
                    .map_err(|err| AppError::msg(format!("Failed to load private key: {err}")))?;
                let auth_res = session
                    .authenticate_publickey(
                        username,
                        PrivateKeyWithHashAlg::new(
                            Arc::new(key_pair),
                            session
                                .best_supported_rsa_hash()
                                .await
                                .map_err(|err| {
                                    AppError::msg(format!("Failed to negotiate RSA hash: {err}"))
                                })?
                                .flatten(),
                        ),
                    )
                    .await
                    .map_err(|err| {
                        AppError::msg(format!("Public key authentication failed: {err}"))
                    })?;
                if !auth_res.success() {
                    return Err(AppError::msg("Public key authentication was rejected."));
                }
            }
            SshAuthMode::Password => {
                let password = input
                    .password
                    .as_deref()
                    .ok_or_else(|| AppError::msg("Password is required."))?;
                let auth_res = session
                    .authenticate_password(username, password)
                    .await
                    .map_err(|err| {
                        AppError::msg(format!("Password authentication failed: {err}"))
                    })?;
                if !auth_res.success() {
                    return Err(AppError::msg("Password authentication was rejected."));
                }
            }
        }

        Ok(Self { session })
    }

    async fn call(&mut self, command: &str, app: &AppHandle) -> AppResult<(Option<u32>, String)> {
        let mut channel = self
            .session
            .channel_open_session()
            .await
            .map_err(|err| AppError::msg(format!("Failed to open SSH channel: {err}")))?;
        channel
            .request_pty(false, "xterm-256color", 100, 30, 0, 0, &[])
            .await
            .map_err(|err| AppError::msg(format!("Failed to request SSH PTY: {err}")))?;
        channel
            .exec(true, command)
            .await
            .map_err(|err| AppError::msg(format!("Failed to execute SSH command: {err}")))?;

        let mut exit_status = None;
        let mut stdout = String::new();
        while let Some(msg) = channel.wait().await {
            match msg {
                ChannelMsg::Data { data } => {
                    let text = String::from_utf8_lossy(&data);
                    stdout.push_str(&text);
                    emit_log(app, SshLogStream::Stdout, text);
                }
                ChannelMsg::ExtendedData { data, .. } => {
                    emit_log(app, SshLogStream::Stderr, String::from_utf8_lossy(&data));
                }
                ChannelMsg::ExitStatus { exit_status: code } => {
                    exit_status = Some(code);
                }
                ChannelMsg::ExitSignal {
                    signal_name,
                    error_message,
                    ..
                } => {
                    emit_log(
                        app,
                        SshLogStream::Stderr,
                        format!("Process exited by signal {signal_name:?}: {error_message}\n"),
                    );
                }
                ChannelMsg::Close => break,
                _ => {}
            }
        }
        Ok((exit_status, stdout))
    }

    async fn close(&mut self) {
        let _ = self
            .session
            .disconnect(Disconnect::ByApplication, "", "English")
            .await;
    }
}

pub async fn run_sample(app: AppHandle, input: SshRunInput) -> AppResult<SshRunResult> {
    if input.host.trim().is_empty() {
        return run_local_sample(&app).await;
    }
    if input.service_password.is_empty() {
        return Err(AppError::msg("Service password is required."));
    }
    if input.method.trim().is_empty() {
        return Err(AppError::msg("Encryption method is required."));
    }

    let host = input.host.trim().to_string();
    let port = input.port;
    emit_log(
        &app,
        SshLogStream::System,
        format!(
            "\x1b[36mConnecting\x1b[0m {host}:{port} as {}\n",
            username(&input)
        ),
    );
    let mut session = Session::connect(&input, (host.as_str(), port), &app).await?;
    let command = install_command(&input)?;
    emit_log(
        &app,
        SshLogStream::System,
        format!("\x1b[32mConnected\x1b[0m. Running installer.\n$ {command}\n"),
    );
    let (exit_status, _) = session.call(&command, &app).await?;
    session.close().await;
    Ok(SshRunResult { exit_status })
}

async fn run_local_sample(app: &AppHandle) -> AppResult<SshRunResult> {
    let lines = [
        "\x1b[36mNo host provided; rendering local SSH sample.\x1b[0m\n",
        "Connecting 192.0.2.10:22 as root\n",
        "\x1b[32mConnected\x1b[0m. Running sample command.\n",
        "$ printf '\\033[32mconnected\\033[0m\\n'; uname -a; id; pwd\n",
        "\x1b[32mconnected\x1b[0m\n",
        "Darwin sample-host 25.0.0 arm64\n",
        "uid=0(root) gid=0(root) groups=0(root)\n",
        "/root\n",
    ];

    for line in lines {
        emit_log(app, SshLogStream::System, line);
        tokio::time::sleep(Duration::from_millis(180)).await;
    }
    Ok(SshRunResult {
        exit_status: Some(0),
    })
}

fn install_command(input: &SshRunInput) -> AppResult<String> {
    let password = password::normalize_for_method(input.method.trim(), &input.service_password)?;
    let password = shell_quote(&password);
    let method = shell_quote(input.method.trim());

    Ok(format!(
        r#"set -eu
if ! command -v docker >/dev/null 2>&1; then
  curl -fsSL https://get.docker.com | sh
fi
sudo docker pull ghcr.io/tosone/socks:latest
sudo docker rm -f socks-server >/dev/null 2>&1 || true
sudo docker run -d --name socks-server --restart unless-stopped -p 443:443/tcp -e SS_PASSWORD={password} -e SS_METHOD={method} ghcr.io/tosone/socks:latest
sudo docker ps --filter name=socks-server"#
    ))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn username(input: &SshRunInput) -> String {
    let username = input.username.trim();
    if username.is_empty() {
        "root".to_string()
    } else {
        username.to_string()
    }
}

fn emit_log(app: &AppHandle, stream: SshLogStream, data: impl Into<String>) {
    let _ = app.emit(
        "ssh-run",
        SshRunEvent {
            stream,
            data: data.into(),
        },
    );
}

fn expand_home(path: &str) -> PathBuf {
    if path == "~" {
        return std::env::var_os("HOME").map_or_else(|| PathBuf::from(path), PathBuf::from);
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}
