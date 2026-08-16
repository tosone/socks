use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, AppResult};

pub const CIPHERS: &[&str] = &[
    "aes-128-gcm",
    "aes-256-gcm",
    "chacha20-ietf-poly1305",
    "aes-128-ccm",
    "aes-256-ccm",
    "aes-128-gcm-siv",
    "aes-256-gcm-siv",
    "xchacha20-ietf-poly1305",
    "sm4-gcm",
    "sm4-ccm",
    "2022-blake3-aes-128-gcm",
    "2022-blake3-aes-256-gcm",
    "2022-blake3-chacha20-poly1305",
    "2022-blake3-chacha8-poly1305",
    "chacha20-ietf",
    "aes-128-ctr",
    "aes-192-ctr",
    "aes-256-ctr",
    "aes-128-cfb",
    "aes-192-cfb",
    "aes-256-cfb",
    "aes-128-cfb1",
    "aes-192-cfb1",
    "aes-256-cfb1",
    "aes-128-cfb8",
    "aes-192-cfb8",
    "aes-256-cfb8",
    "aes-128-ofb",
    "aes-192-ofb",
    "aes-256-ofb",
    "camellia-128-ctr",
    "camellia-192-ctr",
    "camellia-256-ctr",
    "camellia-128-cfb",
    "camellia-192-cfb",
    "camellia-256-cfb",
    "camellia-128-cfb1",
    "camellia-192-cfb1",
    "camellia-256-cfb1",
    "camellia-128-cfb8",
    "camellia-192-cfb8",
    "camellia-256-cfb8",
    "camellia-128-ofb",
    "camellia-192-ofb",
    "camellia-256-ofb",
    "rc4-md5",
    "rc4",
    "table",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub server: String,
    pub port: u16,
    pub password: String,
    pub method: String,
    pub plugin: Option<String>,
    pub plugin_opts: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInput {
    pub name: String,
    pub server: String,
    pub port: u16,
    pub password: String,
    pub method: String,
    pub plugin: Option<String>,
    pub plugin_opts: Option<String>,
}

impl ProfileInput {
    pub fn validate(&self) -> AppResult<()> {
        if self.name.trim().is_empty() {
            return Err(AppError::msg("Name is required"));
        }
        if self.server.trim().is_empty() {
            return Err(AppError::msg("Server is required"));
        }
        if self.port == 0 {
            return Err(AppError::msg("Invalid port"));
        }
        if self.password.is_empty() {
            return Err(AppError::msg("Password is required"));
        }
        if !CIPHERS.contains(&self.method.as_str()) {
            return Err(AppError::msg(format!(
                "Unsupported encryption method: {}",
                self.method
            )));
        }
        Ok(())
    }
}

pub fn store_path(data_dir: &Path) -> PathBuf {
    data_dir.join("profiles.json")
}

pub fn load_profiles(data_dir: &Path) -> AppResult<Vec<Profile>> {
    let path = store_path(data_dir);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(path)?;
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_str(&raw)?)
}

pub fn save_profiles(data_dir: &Path, profiles: &[Profile]) -> AppResult<()> {
    fs::create_dir_all(data_dir)?;
    let path = store_path(data_dir);
    let raw = serde_json::to_string_pretty(profiles)?;
    fs::write(path, raw)?;
    Ok(())
}

pub fn create_profile(input: ProfileInput) -> AppResult<Profile> {
    input.validate()?;
    Ok(Profile {
        id: Uuid::new_v4().to_string(),
        name: input.name.trim().to_string(),
        server: input.server.trim().to_string(),
        port: input.port,
        password: input.password,
        method: input.method,
        plugin: empty_to_none(input.plugin),
        plugin_opts: empty_to_none(input.plugin_opts),
        created_at: now_secs(),
    })
}

pub fn apply_update(profile: &mut Profile, input: ProfileInput) -> AppResult<()> {
    input.validate()?;
    profile.name = input.name.trim().to_string();
    profile.server = input.server.trim().to_string();
    profile.port = input.port;
    profile.password = input.password;
    profile.method = input.method;
    profile.plugin = empty_to_none(input.plugin);
    profile.plugin_opts = empty_to_none(input.plugin_opts);
    Ok(())
}

fn empty_to_none(value: Option<String>) -> Option<String> {
    value.and_then(|v| {
        let trimmed = v.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_name() {
        let input = ProfileInput {
            name: "  ".into(),
            server: "1.1.1.1".into(),
            port: 8388,
            password: "pw".into(),
            method: "aes-256-gcm".into(),
            plugin: None,
            plugin_opts: None,
        };
        assert!(input.validate().is_err());
    }
}
