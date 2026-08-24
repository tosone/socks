use crate::error::{AppError, AppResult};
use crate::profiles::Profile;

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::{CStr, CString};
    use std::os::raw::{c_char, c_int};

    use super::*;

    extern "C" {
        fn socks_packet_tunnel_start(
            tunnel_id: *const c_char,
            name: *const c_char,
            transport_config: *const c_char,
            error_buffer: *mut c_char,
            error_buffer_len: usize,
        ) -> c_int;
        fn socks_packet_tunnel_stop(
            tunnel_id: *const c_char,
            error_buffer: *mut c_char,
            error_buffer_len: usize,
        ) -> c_int;
    }

    pub fn start(profile: &Profile, transport_config: &str) -> AppResult<()> {
        let tunnel_id = c_string("tunnel id", &profile.id)?;
        let name = c_string("tunnel name", &profile.name)?;
        let transport_config = c_string("transport config", transport_config)?;
        let mut error = vec![0 as c_char; 2048];
        let status = unsafe {
            socks_packet_tunnel_start(
                tunnel_id.as_ptr(),
                name.as_ptr(),
                transport_config.as_ptr(),
                error.as_mut_ptr(),
                error.len(),
            )
        };
        result(status, &error, "Failed to start packet tunnel")
    }

    pub fn stop(tunnel_id: &str) -> AppResult<()> {
        let tunnel_id = c_string("tunnel id", tunnel_id)?;
        let mut error = vec![0 as c_char; 2048];
        let status = unsafe {
            socks_packet_tunnel_stop(tunnel_id.as_ptr(), error.as_mut_ptr(), error.len())
        };
        result(status, &error, "Failed to stop packet tunnel")
    }

    fn c_string(label: &str, value: &str) -> AppResult<CString> {
        CString::new(value).map_err(|_| AppError::msg(format!("{label} cannot contain NUL bytes")))
    }

    fn result(status: c_int, error: &[c_char], fallback: &str) -> AppResult<()> {
        if status == 0 {
            return Ok(());
        }
        let message = unsafe { CStr::from_ptr(error.as_ptr()) }
            .to_string_lossy()
            .trim()
            .to_string();
        if message.is_empty() {
            Err(AppError::msg(fallback))
        } else {
            Err(AppError::msg(message))
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::*;

    pub fn start(_profile: &Profile, _transport_config: &str) -> AppResult<()> {
        Err(AppError::msg(
            "Packet Tunnel is only available on macOS and iOS.",
        ))
    }

    pub fn stop(_tunnel_id: &str) -> AppResult<()> {
        Ok(())
    }
}

pub fn start(profile: &Profile, transport_config: &str) -> AppResult<()> {
    platform::start(profile, transport_config)
}

pub fn stop(tunnel_id: &str) -> AppResult<()> {
    platform::stop(tunnel_id)
}
