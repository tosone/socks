use std::net::Ipv4Addr;
use std::process::Command;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::helper;
use crate::sslocal::TUN_ADDRESS;

const TUN_IPV4: &str = "10.255.0.1";

#[derive(Debug, Clone)]
pub struct RouteSnapshot {
    pub gateway: String,
    pub interface: String,
    pub dns: Option<DnsSnapshot>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DnsSnapshot {
    pub service: String,
    pub servers: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppliedRoutes {
    pub tun_name: String,
    pub server_ip: String,
    pub dns: Option<DnsSnapshot>,
}

pub fn current_default_route() -> AppResult<RouteSnapshot> {
    let output = Command::new("route")
        .args(["-n", "get", "default"])
        .output()
        .map_err(|err| AppError::msg(format!("Failed to read the default route: {err}")))?;
    if !output.status.success() {
        return Err(AppError::msg(stderr_or(
            &output,
            "Failed to read the default route",
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut gateway = None;
    let mut interface = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("gateway:") {
            gateway = Some(rest.trim().to_string());
        }
        if let Some(rest) = line.strip_prefix("interface:") {
            interface = Some(rest.trim().to_string());
        }
    }
    match (gateway, interface) {
        (Some(gateway), Some(interface)) => {
            let dns = current_dns_snapshot(&interface).ok();
            Ok(RouteSnapshot {
                gateway,
                interface,
                dns,
            })
        }
        _ => Err(AppError::msg(
            "Could not parse the default gateway or interface",
        )),
    }
}

pub async fn wait_for_tun_name(timeout: Duration) -> AppResult<String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(name) = find_tun_by_address(TUN_IPV4)? {
            return Ok(name);
        }
        if Instant::now() >= deadline {
            return Err(AppError::msg(format!(
                "Timed out waiting for the TUN interface (expected {TUN_ADDRESS})"
            )));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

pub fn apply_global_routes(
    snapshot: &RouteSnapshot,
    tun_name: &str,
    server_ip: &str,
) -> AppResult<AppliedRoutes> {
    helper::apply_routes(tun_name, server_ip, &snapshot.gateway)?;
    if let Some(dns) = snapshot.dns.as_ref() {
        if let Err(err) = helper::apply_dns(&dns.service) {
            let _ = helper::delete_routes(tun_name, server_ip);
            return Err(err);
        }
    }
    Ok(AppliedRoutes {
        tun_name: tun_name.to_string(),
        server_ip: server_ip.to_string(),
        dns: snapshot.dns.clone(),
    })
}

pub fn restore_routes(applied: &AppliedRoutes) -> AppResult<()> {
    // Best-effort: still try even if some routes were already removed.
    if let Some(dns) = applied.dns.as_ref() {
        let _ = helper::restore_dns(&dns.service, &dns.servers);
    }
    let _ = helper::delete_routes(&applied.tun_name, &applied.server_ip);
    Ok(())
}

fn current_dns_snapshot(interface: &str) -> AppResult<DnsSnapshot> {
    let service = service_for_interface(interface)?;
    let output = Command::new("networksetup")
        .args(["-getdnsservers", &service])
        .output()
        .map_err(|err| AppError::msg(format!("Failed to read DNS servers: {err}")))?;
    if !output.status.success() {
        return Err(AppError::msg(stderr_or(
            &output,
            "Failed to read DNS servers",
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let servers = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("There aren't any DNS Servers"))
        .map(str::to_string)
        .collect();
    Ok(DnsSnapshot { service, servers })
}

fn service_for_interface(interface: &str) -> AppResult<String> {
    let output = Command::new("networksetup")
        .arg("-listallhardwareports")
        .output()
        .map_err(|err| AppError::msg(format!("Failed to list network services: {err}")))?;
    if !output.status.success() {
        return Err(AppError::msg(stderr_or(
            &output,
            "Failed to list network services",
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut current_service = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Hardware Port:") {
            current_service = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("Device:") {
            if rest.trim() == interface {
                if let Some(service) = current_service {
                    return Ok(service);
                }
            }
        }
    }
    Err(AppError::msg(format!(
        "Could not find a network service for interface {interface}"
    )))
}

fn find_tun_by_address(addr: &str) -> AppResult<Option<String>> {
    let output = Command::new("ifconfig")
        .output()
        .map_err(|err| AppError::msg(format!("Failed to run ifconfig: {err}")))?;
    if !output.status.success() {
        return Err(AppError::msg(stderr_or(&output, "ifconfig failed")));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut current: Option<String> = None;
    for line in text.lines() {
        if !line.starts_with('\t') && !line.starts_with(' ') {
            current = line.split(':').next().map(str::to_string);
            continue;
        }
        if line.contains("inet ") && line.contains(addr) {
            if let Some(name) = current {
                return Ok(Some(name));
            }
        }
    }
    let _ = addr.parse::<Ipv4Addr>();
    Ok(None)
}

fn stderr_or(output: &std::process::Output, fallback: &str) -> String {
    let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if err.is_empty() {
        fallback.to_string()
    } else {
        err
    }
}
