use std::io::{BufRead, BufReader, Write};
use std::net::IpAddr;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use std::{env, fs};

use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

use crate::error::{AppError, AppResult};
use crate::macos_route::DnsSnapshot;
use crate::profile::Profile;
use crate::sslocal::{self, DNS_RELAY_PORT};

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
    Dns {
        action: RouteAction,
        service: String,
        servers: Vec<String>,
        #[serde(rename = "relayPort")]
        relay_port: Option<u16>,
    },
    Start {
        profile: Profile,
        #[serde(rename = "outboundInterface")]
        outbound_interface: String,
        #[serde(rename = "serverIp")]
        server_ip: String,
        gateway: String,
        dns: Option<DnsSnapshot>,
        #[serde(rename = "localDnsIp")]
        local_dns_ip: String,
        #[serde(rename = "bundledPluginDir")]
        bundled_plugin_dir: Option<String>,
        #[serde(rename = "aclPath")]
        acl_path: Option<String>,
    },
    Stop,
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
    #[serde(default, rename = "tunName", skip_serializing_if = "Option::is_none")]
    pub tun_name: Option<String>,
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

pub struct StartRuntimeInput {
    pub profile: Profile,
    pub outbound_interface: String,
    pub server_ip: String,
    pub gateway: String,
    pub dns: Option<DnsSnapshot>,
    pub local_dns_ip: IpAddr,
    pub bundled_plugin_dir: Option<PathBuf>,
    pub acl_path: Option<PathBuf>,
}

pub fn start_runtime(input: StartRuntimeInput) -> AppResult<String> {
    let response = send_request_raw(&HelperRequest::Start {
        profile: input.profile,
        outbound_interface: input.outbound_interface,
        server_ip: input.server_ip,
        gateway: input.gateway,
        dns: input.dns,
        local_dns_ip: input.local_dns_ip.to_string(),
        bundled_plugin_dir: input
            .bundled_plugin_dir
            .map(|path| path.to_string_lossy().into_owned()),
        acl_path: input
            .acl_path
            .map(|path| path.to_string_lossy().into_owned()),
    })
    .map_err(|err| {
        let message = err.to_string();
        if message.contains("Invalid helper request") || message.contains("unknown variant") {
            AppError::msg(
                "Installed helper is outdated. Reinstall helper once, then connect again.",
            )
        } else {
            AppError::msg(message)
        }
    })?;
    response
        .tun_name
        .ok_or_else(|| AppError::msg("Helper did not return a TUN interface name"))
}

pub fn stop_runtime() -> AppResult<()> {
    send_request(&HelperRequest::Stop)
}

pub fn run_helper() {
    if let Err(err) = helper_main() {
        eprintln!("socks helper: {err}");
        std::process::exit(1);
    }
}

struct HelperRuntime {
    runtime: tokio::runtime::Runtime,
    session: Option<HelperSession>,
}

struct HelperSession {
    profile_id: String,
    tun_name: String,
    routes: Option<HelperAppliedRoutes>,
    server_task: JoinHandle<()>,
}

#[derive(Clone)]
struct HelperAppliedRoutes {
    tun_name: String,
    server_ip: String,
    dns: Option<DnsSnapshot>,
}

impl HelperRuntime {
    fn new() -> AppResult<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("socks-helper-runtime")
            .build()
            .map_err(|err| AppError::msg(format!("Failed to create helper runtime: {err}")))?;
        Ok(Self {
            runtime,
            session: None,
        })
    }

    fn start(&mut self, input: StartRuntimeInput) -> AppResult<String> {
        self.stop()?;
        let bundled_plugin_dir = input.bundled_plugin_dir.as_deref();
        let acl_path = input.acl_path.as_deref();
        let config = sslocal::build_server_config(
            &input.profile,
            Some(input.outbound_interface.clone()),
            bundled_plugin_dir,
            input.local_dns_ip,
            acl_path,
        )?;
        let runtime = self.runtime.block_on(sslocal::start_local(config))?;
        let profile_id = input.profile.id.clone();
        let server_task = self.runtime.spawn(async move {
            if let Err(err) = runtime.server.run().await {
                eprintln!("[socks-helper] sslocal stopped profile_id={profile_id}: {err:?}");
            }
        });
        let tun_name =
            match self
                .runtime
                .block_on(crate::macos_route::wait_for_tun_name(Duration::from_secs(
                    8,
                ))) {
                Ok(name) => name,
                Err(err) => {
                    server_task.abort();
                    return Err(err);
                }
            };
        let routes = match apply_runtime_routes(
            &tun_name,
            &input.server_ip,
            &input.gateway,
            input.dns.clone(),
        ) {
            Ok(routes) => routes,
            Err(err) => {
                server_task.abort();
                return Err(err);
            }
        };
        self.session = Some(HelperSession {
            profile_id: input.profile.id,
            tun_name: tun_name.clone(),
            routes: Some(routes),
            server_task,
        });
        Ok(tun_name)
    }

    fn stop(&mut self) -> AppResult<()> {
        if let Some(mut session) = self.session.take() {
            eprintln!(
                "[socks-helper] stopping runtime profile_id={} tun={}",
                session.profile_id, session.tun_name
            );
            session.server_task.abort();
            if let Some(routes) = session.routes.take() {
                restore_runtime_routes(&routes);
            }
        }
        Ok(())
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

    let mut runtime = HelperRuntime::new()?;
    for incoming in listener.incoming() {
        let Ok(mut stream) = incoming else {
            continue;
        };
        if let Err(err) = handle_client(&mut stream, &mut runtime) {
            let _ = write_response(
                &mut stream,
                &HelperResponse {
                    ok: false,
                    error: Some(err.to_string()),
                    tun_name: None,
                },
            );
        }
    }
    Ok(())
}

fn handle_client(stream: &mut UnixStream, runtime: &mut HelperRuntime) -> AppResult<()> {
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
                tun_name: None,
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
                    tun_name: None,
                },
            )
        }
        HelperRequest::Dns {
            action,
            service,
            servers,
            relay_port,
        } => {
            apply_dns_command(action, &service, &servers, relay_port)?;
            write_response(
                stream,
                &HelperResponse {
                    ok: true,
                    error: None,
                    tun_name: None,
                },
            )
        }
        HelperRequest::Start {
            profile,
            outbound_interface,
            server_ip,
            gateway,
            dns,
            local_dns_ip,
            bundled_plugin_dir,
            acl_path,
        } => {
            let local_dns_ip = local_dns_ip
                .parse::<IpAddr>()
                .map_err(|err| AppError::msg(format!("Invalid local DNS IP: {err}")))?;
            let tun_name = runtime.start(StartRuntimeInput {
                profile,
                outbound_interface,
                server_ip,
                gateway,
                dns,
                local_dns_ip,
                bundled_plugin_dir: bundled_plugin_dir.map(PathBuf::from),
                acl_path: acl_path.map(PathBuf::from),
            })?;
            write_response(
                stream,
                &HelperResponse {
                    ok: true,
                    error: None,
                    tun_name: Some(tun_name),
                },
            )
        }
        HelperRequest::Stop => {
            runtime.stop()?;
            write_response(
                stream,
                &HelperResponse {
                    ok: true,
                    error: None,
                    tun_name: None,
                },
            )
        }
    }
}

fn apply_runtime_routes(
    tun_name: &str,
    server_ip: &str,
    gateway: &str,
    dns: Option<DnsSnapshot>,
) -> AppResult<HelperAppliedRoutes> {
    apply_route_command(RouteAction::Add, tun_name, server_ip, Some(gateway))?;
    if let Some(dns_snapshot) = dns.as_ref() {
        if let Err(err) = apply_dns_command(
            RouteAction::Add,
            &dns_snapshot.service,
            &[],
            Some(DNS_RELAY_PORT),
        ) {
            let _ = apply_route_command(RouteAction::Delete, tun_name, server_ip, None);
            return Err(err);
        }
    }
    Ok(HelperAppliedRoutes {
        tun_name: tun_name.to_string(),
        server_ip: server_ip.to_string(),
        dns,
    })
}

fn restore_runtime_routes(routes: &HelperAppliedRoutes) {
    if let Some(dns) = routes.dns.as_ref() {
        let _ = apply_dns_command(RouteAction::Delete, &dns.service, &dns.servers, None);
    }
    let _ = apply_route_command(
        RouteAction::Delete,
        &routes.tun_name,
        &routes.server_ip,
        None,
    );
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
            let result = (|| {
                run_route(&["-n", "add", "-host", server_ip, gateway])?;
                run_route(&["-n", "add", "-net", "0.0.0.0/1", "-interface", tun])?;
                run_route(&["-n", "add", "-net", "128.0.0.0/1", "-interface", tun])?;
                Ok(())
            })();
            if let Err(err) = result {
                let _ = run_route(&["-n", "delete", "-net", "0.0.0.0/1", "-interface", tun]);
                let _ = run_route(&["-n", "delete", "-net", "128.0.0.0/1", "-interface", tun]);
                let _ = run_route(&["-n", "delete", "-host", server_ip]);
                return Err(err);
            }
        }
        RouteAction::Delete => {
            let _ = run_route(&["-n", "delete", "-net", "0.0.0.0/1", "-interface", tun]);
            let _ = run_route(&["-n", "delete", "-net", "128.0.0.0/1", "-interface", tun]);
            let _ = run_route(&["-n", "delete", "-host", server_ip]);
        }
    }
    Ok(())
}

fn apply_dns_command(
    action: RouteAction,
    service: &str,
    servers: &[String],
    relay_port: Option<u16>,
) -> AppResult<()> {
    if service.trim().is_empty() {
        return Err(AppError::msg("Missing network service"));
    }

    match action {
        RouteAction::Add => {
            let relay_port = relay_port.ok_or_else(|| AppError::msg("Missing DNS relay port"))?;
            apply_pf_dns_redirect(relay_port)?;
            if let Err(err) = run_networksetup(&["-setdnsservers", service, "127.0.0.1"]) {
                let _ = clear_pf_dns_redirect();
                return Err(err);
            }
        }
        RouteAction::Delete => {
            let result = if servers.is_empty() {
                run_networksetup(&["-setdnsservers", service, "Empty"])
            } else {
                let mut args = vec!["-setdnsservers", service];
                args.extend(servers.iter().map(String::as_str));
                run_networksetup(&args)
            };
            let clear_result = clear_pf_dns_redirect();
            result?;
            clear_result?;
        }
    }
    Ok(())
}

fn apply_pf_dns_redirect(relay_port: u16) -> AppResult<()> {
    if relay_port == 0 {
        return Err(AppError::msg("Invalid DNS relay port"));
    }
    let rules = format!(
        "rdr pass on lo0 inet proto tcp from any to 127.0.0.1 port 53 -> 127.0.0.1 port {relay_port}\n\
         rdr pass on lo0 inet proto udp from any to 127.0.0.1 port 53 -> 127.0.0.1 port {relay_port}\n"
    );
    let _ = Command::new("/sbin/pfctl").arg("-E").output();
    let mut child = Command::new("/sbin/pfctl")
        .args(["-a", "com.apple/socks", "-f", "-"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| AppError::msg(format!("Failed to run pfctl: {err}")))?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(rules.as_bytes())?;
    }
    let output = child
        .wait_with_output()
        .map_err(|err| AppError::msg(format!("Failed to wait for pfctl: {err}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(AppError::msg(stderr_or(&output, "pfctl failed")))
    }
}

fn clear_pf_dns_redirect() -> AppResult<()> {
    let output = Command::new("/sbin/pfctl")
        .args(["-a", "com.apple/socks", "-F", "all"])
        .output()
        .map_err(|err| AppError::msg(format!("Failed to run pfctl: {err}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(AppError::msg(stderr_or(&output, "pfctl failed")))
    }
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

fn run_networksetup(args: &[&str]) -> AppResult<()> {
    let output = Command::new("/usr/sbin/networksetup")
        .args(args)
        .output()
        .map_err(|err| AppError::msg(format!("Failed to run networksetup: {err}")))?;
    if output.status.success() {
        return Ok(());
    }
    Err(AppError::msg(stderr_or(&output, "networksetup failed")))
}

fn stderr_or(output: &std::process::Output, fallback: &str) -> String {
    let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if err.is_empty() {
        fallback.to_string()
    } else {
        err
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
    send_request_raw(request).map(|_| ())
}

fn send_request_raw(request: &HelperRequest) -> AppResult<HelperResponse> {
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
        Ok(response)
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
