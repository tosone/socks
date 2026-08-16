use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use std::{env, fs};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

pub const HELPER_LABEL: &str = "com.tosone.socks.helper";
pub const HELPER_SOCKET: &str = "/var/run/com.tosone.socks.helper.sock";
const HELPER_BIN: &str = "/Library/PrivilegedHelperTools/com.tosone.socks.helper";
const HELPER_PLIST: &str = "/Library/LaunchDaemons/com.tosone.socks.helper.plist";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum HelperRequest {
    Ping,
    Route {
        action: RouteAction,
        tun: String,
        #[serde(rename = "serverIp")]
        server_ip: String,
        gateway: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RouteAction {
    Add,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelperResponse {
    pub ok: bool,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperStatus {
    pub installed: bool,
}

pub fn helper_status() -> HelperStatus {
    HelperStatus {
        installed: ping_helper().is_ok(),
    }
}

pub fn install_helper() -> AppResult<HelperStatus> {
    let source = current_app_exe()?;
    if !source.exists() {
        return Err(AppError::msg(format!(
            "Could not find the app executable: {}",
            source.display()
        )));
    }

    let tmp = env::temp_dir().join("com.tosone.socks.helper-install");
    fs::create_dir_all(&tmp)?;
    let plist_src = tmp.join("helper.plist");
    let script_src = tmp.join("install.sh");
    fs::write(&plist_src, helper_plist())?;
    let script = format!(
        r#"#!/bin/bash
set -euo pipefail
mkdir -p /Library/PrivilegedHelperTools
install -o root -g wheel -m 0755 {src} {dst}
install -o root -g wheel -m 0644 {plist_src} {plist}
if launchctl print system/{label} >/dev/null 2>&1; then
  launchctl bootout system/{label} || true
fi
if ! launchctl bootstrap system {plist} >/dev/null 2>&1; then
  launchctl unload {plist} >/dev/null 2>&1 || true
  launchctl load -w {plist}
fi
launchctl enable system/{label} >/dev/null 2>&1 || true
launchctl kickstart -k system/{label} >/dev/null 2>&1 || true
"#,
        src = sh_single_quote(&source.to_string_lossy()),
        dst = HELPER_BIN,
        plist_src = sh_single_quote(&plist_src.to_string_lossy()),
        plist = HELPER_PLIST,
        label = HELPER_LABEL,
    );
    fs::write(&script_src, script)?;

    run_osascript(&format!(
        "/bin/bash {}",
        sh_single_quote(&script_src.to_string_lossy())
    ))?;
    wait_for_helper(Duration::from_secs(8))?;
    Ok(helper_status())
}

pub fn uninstall_helper() -> AppResult<HelperStatus> {
    let script = format!(
        r#"set +e
launchctl bootout system/{label} >/dev/null 2>&1
launchctl unload {plist} >/dev/null 2>&1
rm -f {plist} {bin} {sock}
"#,
        label = HELPER_LABEL,
        plist = HELPER_PLIST,
        bin = HELPER_BIN,
        sock = HELPER_SOCKET,
    );
    run_osascript(&script)?;
    Ok(helper_status())
}

pub fn apply_routes(tun: &str, server_ip: &str, gateway: &str) -> AppResult<()> {
    send_request(&HelperRequest::Route {
        action: RouteAction::Add,
        tun: tun.to_string(),
        server_ip: server_ip.to_string(),
        gateway: Some(gateway.to_string()),
    })
}

pub fn delete_routes(tun: &str, server_ip: &str) -> AppResult<()> {
    send_request(&HelperRequest::Route {
        action: RouteAction::Delete,
        tun: tun.to_string(),
        server_ip: server_ip.to_string(),
        gateway: None,
    })
}

pub fn run_helper() {
    if let Err(err) = helper_main() {
        eprintln!("socks helper: {err}");
        std::process::exit(1);
    }
}

fn helper_main() -> AppResult<()> {
    let _ = std::fs::remove_file(HELPER_SOCKET);
    let listener = std::os::unix::net::UnixListener::bind(HELPER_SOCKET)
        .map_err(|err| AppError::msg(format!("Failed to bind helper socket: {err}")))?;
    let _ = Command::new("/usr/sbin/chown")
        .args(["root:staff", HELPER_SOCKET])
        .status();
    let _ = Command::new("/bin/chmod")
        .args(["660", HELPER_SOCKET])
        .status();

    for incoming in listener.incoming() {
        let Ok(mut stream) = incoming else {
            continue;
        };
        if let Err(err) = handle_client(&mut stream) {
            let _ = write_response(
                &mut stream,
                &HelperResponse {
                    ok: false,
                    error: Some(err.to_string()),
                },
            );
        }
    }
    Ok(())
}

fn handle_client(stream: &mut UnixStream) -> AppResult<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(5))).ok();

    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let request: HelperRequest = serde_json::from_str(line.trim())
        .map_err(|err| AppError::msg(format!("Invalid helper request: {err}")))?;
    match request {
        HelperRequest::Ping => write_response(
            stream,
            &HelperResponse {
                ok: true,
                error: None,
            },
        ),
        HelperRequest::Route {
            action,
            tun,
            server_ip,
            gateway,
        } => {
            apply_route_command(action, &tun, &server_ip, gateway.as_deref())?;
            write_response(
                stream,
                &HelperResponse {
                    ok: true,
                    error: None,
                },
            )
        }
    }
}

fn apply_route_command(
    action: RouteAction,
    tun: &str,
    server_ip: &str,
    gateway: Option<&str>,
) -> AppResult<()> {
    if !valid_utun(tun) {
        return Err(AppError::msg("Invalid TUN interface name"));
    }
    if !valid_ipv4(server_ip) {
        return Err(AppError::msg("Invalid server IP"));
    }

    match action {
        RouteAction::Add => {
            let gateway = gateway.ok_or_else(|| AppError::msg("Missing gateway"))?;
            if !valid_ipv4(gateway) {
                return Err(AppError::msg("Invalid gateway IP"));
            }
            run_route(&["-n", "add", "-host", server_ip, gateway])?;
            run_route(&["-n", "add", "-net", "0.0.0.0/1", "-interface", tun])?;
            run_route(&["-n", "add", "-net", "128.0.0.0/1", "-interface", tun])?;
        }
        RouteAction::Delete => {
            let _ = run_route(&["-n", "delete", "-net", "0.0.0.0/1", "-interface", tun]);
            let _ = run_route(&["-n", "delete", "-net", "128.0.0.0/1", "-interface", tun]);
            let _ = run_route(&["-n", "delete", "-host", server_ip]);
        }
    }
    Ok(())
}

fn run_route(args: &[&str]) -> AppResult<()> {
    let output = Command::new("/sbin/route")
        .args(args)
        .output()
        .map_err(|err| AppError::msg(format!("Failed to run route: {err}")))?;
    if output.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if err.is_empty() {
        Err(AppError::msg("route command failed"))
    } else {
        Err(AppError::msg(err))
    }
}

fn write_response(stream: &mut UnixStream, response: &HelperResponse) -> AppResult<()> {
    let line = serde_json::to_string(response)?;
    stream.write_all(line.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

fn ping_helper() -> AppResult<()> {
    send_request(&HelperRequest::Ping)
}

fn send_request(request: &HelperRequest) -> AppResult<()> {
    let mut stream = UnixStream::connect(HELPER_SOCKET).map_err(|_| {
        AppError::msg(
            "Helper is not running. Install it first so later connects do not ask for a password.",
        )
    })?;
    stream.set_read_timeout(Some(Duration::from_secs(8))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(8))).ok();
    let line = serde_json::to_string(request)?;
    stream.write_all(line.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut response_line = String::new();
    reader.read_line(&mut response_line)?;
    let response: HelperResponse = serde_json::from_str(response_line.trim())
        .map_err(|err| AppError::msg(format!("Invalid helper response: {err}")))?;
    if response.ok {
        Ok(())
    } else {
        Err(AppError::msg(
            response
                .error
                .unwrap_or_else(|| "Helper request failed".to_string()),
        ))
    }
}

fn wait_for_helper(timeout: Duration) -> AppResult<()> {
    let deadline = Instant::now() + timeout;
    let mut last = AppError::msg("Helper did not start in time");
    while Instant::now() < deadline {
        match ping_helper() {
            Ok(()) => return Ok(()),
            Err(err) => last = err,
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(last)
}

fn current_app_exe() -> AppResult<PathBuf> {
    std::env::current_exe()
        .map_err(|err| AppError::msg(format!("Could not locate the current executable: {err}")))
}

fn helper_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{bin}</string>
        <string>helper</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/var/log/com.tosone.socks.helper.log</string>
    <key>StandardErrorPath</key>
    <string>/var/log/com.tosone.socks.helper.log</string>
</dict>
</plist>
"#,
        label = HELPER_LABEL,
        bin = HELPER_BIN,
    )
}

fn run_osascript(shell: &str) -> AppResult<()> {
    let script = format!(
        "do shell script {} with administrator privileges",
        applescript_string(shell)
    );
    let output = Command::new("osascript")
        .args(["-e", &script])
        .output()
        .map_err(|err| {
            AppError::msg(format!("Failed to request administrator privileges: {err}"))
        })?;
    if output.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if err.is_empty() {
        Err(AppError::msg(
            "Administrator authorization failed or was cancelled",
        ))
    } else {
        Err(AppError::msg(err))
    }
}

fn applescript_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn sh_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub fn valid_utun(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("utun") else {
        return false;
    };
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

pub fn valid_ipv4(value: &str) -> bool {
    value.parse::<std::net::Ipv4Addr>().is_ok()
}

#[allow(dead_code)]
pub fn helper_bin_path() -> &'static Path {
    Path::new(HELPER_BIN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_utun_names() {
        assert!(valid_utun("utun0"));
        assert!(valid_utun("utun12"));
        assert!(!valid_utun("en0"));
        assert!(!valid_utun("utun"));
        assert!(!valid_utun("utunX"));
    }

    #[test]
    fn accepts_ipv4() {
        assert!(valid_ipv4("1.2.3.4"));
        assert!(!valid_ipv4("example.com"));
        assert!(!valid_ipv4("::1"));
    }
}
