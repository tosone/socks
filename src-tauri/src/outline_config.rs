use crate::error::AppResult;
use crate::password;
use crate::profiles::Profile;

pub fn transport_config(profile: &Profile) -> AppResult<String> {
    let secret = password::normalize_for_method(&profile.method, &profile.password)?;
    Ok(format!(
        "transport:\n  $type: shadowsocks\n  endpoint: {}\n  cipher: {}\n  secret: {}\n",
        yaml_string(&format!("{}:{}", profile.server, profile.port)),
        yaml_string(&profile.method),
        yaml_string(&secret),
    ))
}

fn yaml_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
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
    fn builds_shadowsocks_transport_config() {
        let config = transport_config(&profile("aes-256-gcm", "pass")).unwrap();

        assert_eq!(
            config,
            "transport:\n  $type: shadowsocks\n  endpoint: \"example.com:8388\"\n  cipher: \"aes-256-gcm\"\n  secret: \"pass\"\n"
        );
    }

    #[test]
    fn normalizes_2022_secret_for_outline_transport() {
        let config =
            transport_config(&profile("2022-blake3-aes-128-gcm", "short-password")).unwrap();

        assert!(config.contains("secret: \"I/LHNXUdcAZorb1hq7+G/g==\""));
    }
}
