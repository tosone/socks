use std::str::FromStr;

use base64::Engine as _;
use sha2::{Digest, Sha256};
use shadowsocks::crypto::CipherKind;

use crate::error::{AppError, AppResult};

pub fn normalize_for_method(method_name: &str, password: &str) -> AppResult<String> {
    let method = CipherKind::from_str(method_name)
        .map_err(|_| AppError::msg(format!("Unknown encryption method: {method_name}")))?;

    if !method.is_aead_2022() {
        return Ok(password.to_string());
    }

    let key_len = method.key_len();
    if decodes_to_len(password, key_len) {
        return Ok(password.to_string());
    }

    let digest = Sha256::digest(password.as_bytes());
    Ok(base64::engine::general_purpose::STANDARD.encode(&digest[..key_len]))
}

fn decodes_to_len(password: &str, key_len: usize) -> bool {
    base64::engine::general_purpose::STANDARD
        .decode(password)
        .is_ok_and(|decoded| decoded.len() == key_len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_non_2022_password_unchanged() {
        let password = normalize_for_method("aes-256-gcm", "short-password").unwrap();

        assert_eq!(password, "short-password");
    }

    #[test]
    fn derives_2022_password_as_base64_key() {
        let password =
            normalize_for_method("2022-blake3-chacha20-poly1305", "short-password").unwrap();

        assert_eq!(password, "I/LHNXUdcAZorb1hq7+G/ughttcM/YommCY66Oo3dN8=");
    }

    #[test]
    fn derives_2022_aes_128_password_as_16_byte_key() {
        let password = normalize_for_method("2022-blake3-aes-128-gcm", "short-password").unwrap();

        assert_eq!(password, "I/LHNXUdcAZorb1hq7+G/g==");
    }

    #[test]
    fn keeps_existing_2022_base64_key_unchanged() {
        let key = "I/LHNXUdcAZorb1hq7+G/ughttcM/YommCY66Oo3dN8=";
        let password = normalize_for_method("2022-blake3-chacha20-poly1305", key).unwrap();

        assert_eq!(password, key);
    }
}
