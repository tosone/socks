//! Standard Shadowsocks configuration for the data plane.

use std::str::FromStr;

use base64::Engine as _;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use shadowsocks::config::{Mode, ServerAddr, ServerConfig};
use shadowsocks::crypto::CipherKind;

/// Configuration handed to [`crate::socks_core_start`] as JSON.
///
/// This is the standard Shadowsocks shape (`server`, `server_port`, `method`,
/// `password`), plus a few tunnel-only knobs. It intentionally does **not** use
/// Outline's transport config format.
#[derive(Debug, Clone, Deserialize)]
pub struct StartConfig {
    pub server: String,
    pub server_port: u16,
    pub method: String,
    pub password: String,

    /// `tcp_only`, `udp_only` or `tcp_and_udp` (default).
    #[serde(default)]
    pub mode: Option<String>,

    /// UDP association idle timeout, in seconds.
    #[serde(default)]
    pub udp_timeout: Option<u64>,

    /// Maximum number of simultaneous UDP associations.
    #[serde(default)]
    pub udp_max_associations: Option<usize>,

    /// SIP003 plugin name (not supported yet; logged and ignored).
    #[serde(default)]
    pub plugin: Option<String>,

    /// SIP003 plugin options (not supported yet; logged and ignored).
    #[serde(default)]
    pub plugin_opts: Option<String>,

    /// Level for the rolling data plane log: `error`, `warn`, `info`,
    /// `debug` (default), `trace` or `off`.
    ///
    /// Debug is the default because the interesting per-connection detail
    /// ("created TCP connection for ...") lives there; the rolling writer keeps
    /// the volume bounded.
    #[serde(default)]
    pub log_level: Option<String>,
}

impl StartConfig {
    pub fn mode(&self) -> Mode {
        match self.mode.as_deref() {
            Some("tcp_only") => Mode::TcpOnly,
            Some("udp_only") => Mode::UdpOnly,
            Some("tcp_and_udp") | None => Mode::TcpAndUdp,
            Some(other) => {
                log::warn!("unknown mode {other:?}, falling back to tcp_and_udp");
                Mode::TcpAndUdp
            }
        }
    }

    /// Level for the rolling data plane log.
    pub fn log_level_filter(&self) -> log::LevelFilter {
        match self.log_level.as_deref().map(str::trim) {
            Some("error") => log::LevelFilter::Error,
            Some("warn") | Some("warning") => log::LevelFilter::Warn,
            Some("info") => log::LevelFilter::Info,
            Some("trace") => log::LevelFilter::Trace,
            Some("off") | Some("none") => log::LevelFilter::Off,
            _ => log::LevelFilter::Debug,
        }
    }

    /// Build a `ServerConfig`, normalising the password for AEAD-2022 ciphers.
    pub fn server_config(&self) -> Result<ServerConfig, String> {
        let method_name = self.method.trim();
        let method = CipherKind::from_str(method_name)
            .map_err(|_| format!("unsupported encryption method: {}", self.method))?;

        if self.server.trim().is_empty() {
            return Err("server is required".to_owned());
        }
        if self.server_port == 0 {
            return Err("server_port is required".to_owned());
        }

        if let Some(plugin) = self.plugin.as_deref().filter(|p| !p.trim().is_empty()) {
            log::warn!(
                "SIP003 plugins are not supported yet; ignoring plugin {plugin:?} with options {:?}",
                self.plugin_opts.as_deref().unwrap_or_default(),
            );
        }

        let password = normalize_password(method, &self.password)?;
        let addr = parse_server_addr(&self.server, self.server_port)?;

        ServerConfig::new(addr, password, method).map_err(|err| format!("invalid server config: {err}"))
    }
}

/// AEAD-2022 ciphers require a base64 key of exactly `key_len` bytes.
///
/// When the user typed a passphrase instead, derive the key the same way the
/// Shadowsocks 2022 spec (and `sslocal`) does: SHA-256, truncated to `key_len`.
pub fn normalize_password(method: CipherKind, password: &str) -> Result<String, String> {
    if !method.is_aead_2022() {
        return Ok(password.to_owned());
    }

    let key_len = method.key_len();
    if decodes_to_len(password, key_len) {
        return Ok(password.to_owned());
    }

    if password.is_empty() {
        return Err("password is required".to_owned());
    }

    let digest = Sha256::digest(password.as_bytes());
    Ok(base64::engine::general_purpose::STANDARD.encode(&digest[..key_len]))
}

fn decodes_to_len(value: &str, len: usize) -> bool {
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .is_ok_and(|decoded| decoded.len() == len)
}

fn parse_server_addr(host: &str, port: u16) -> Result<ServerAddr, String> {
    let host = host.trim();
    let candidate = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    ServerAddr::from_str(&candidate).map_err(|err| format!("invalid server address: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(method: &str, password: &str) -> StartConfig {
        StartConfig {
            server: "1.2.3.4".to_owned(),
            server_port: 8388,
            method: method.to_owned(),
            password: password.to_owned(),
            mode: None,
            udp_timeout: None,
            udp_max_associations: None,
            plugin: None,
            plugin_opts: None,
            log_level: None,
        }
    }

    #[test]
    fn accepts_standard_shadowsocks_config() {
        let server = config("aes-256-gcm", "pass").server_config().unwrap();
        assert_eq!(server.addr().port(), 8388);
    }

    #[test]
    fn derives_aead_2022_key_from_passphrase() {
        let server = config("2022-blake3-aes-128-gcm", "short-password")
            .server_config()
            .unwrap();
        // Base64 of the first 16 bytes of SHA-256("short-password").
        assert_eq!(server.password(), "I/LHNXUdcAZorb1hq7+G/g==");
    }

    #[test]
    fn keeps_existing_aead_2022_key() {
        let key = "I/LHNXUdcAZorb1hq7+G/ughttcM/YommCY66Oo3dN8=";
        let server = config("2022-blake3-chacha20-poly1305", key)
            .server_config()
            .unwrap();
        assert_eq!(server.password(), key);
    }

    #[test]
    fn brackets_ipv6_server_addresses() {
        let addr = parse_server_addr("2001:db8::1", 8388).unwrap();
        assert_eq!(addr.to_string(), "[2001:db8::1]:8388");
    }

    #[test]
    fn rejects_unknown_cipher() {
        assert!(config("not-a-cipher", "pass").server_config().is_err());
    }
}
