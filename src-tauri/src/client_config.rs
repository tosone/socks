//! Standard Shadowsocks client configuration for the Rust data plane.
//!
//! The extension hands this JSON to `socks_core_start` verbatim; the core owns
//! cipher validation and AEAD-2022 key derivation.

use serde_json::json;

use crate::error::AppResult;
use crate::profiles::Profile;

/// Serialise `profile` into the standard Shadowsocks config shape.
pub fn client_config(profile: &Profile) -> AppResult<String> {
    let value = json!({
        "server": profile.server,
        "server_port": profile.port,
        "method": profile.method,
        "password": profile.password,
    });
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(method: &str, password: &str) -> Profile {
        Profile {
            id: "id".into(),
            name: "name".into(),
            server: "example.com".into(),
            port: 8388,
            password: password.into(),
            method: method.into(),
            created_at: 0,
        }
    }

    #[test]
    fn builds_standard_shadowsocks_config() {
        let config = client_config(&profile("aes-256-gcm", "pass")).unwrap();
        let value: serde_json::Value = serde_json::from_str(&config).unwrap();

        assert_eq!(value["server"], "example.com");
        assert_eq!(value["server_port"], 8388);
        assert_eq!(value["method"], "aes-256-gcm");
        assert_eq!(value["password"], "pass");
    }

    #[test]
    fn passes_through_aead_2022_password() {
        // The core derives the key; the app must not transform it.
        let config = client_config(&profile("2022-blake3-aes-128-gcm", "short-password")).unwrap();
        let value: serde_json::Value = serde_json::from_str(&config).unwrap();

        assert_eq!(value["password"], "short-password");
    }
}
