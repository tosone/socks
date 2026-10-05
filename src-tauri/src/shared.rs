//! Access to the App Group container shared with the NetworkExtension.

use std::path::PathBuf;

use crate::error::AppResult;

/// Path of the container shared with the extension.
///
/// `None` when the App Group is unavailable, in which case shared state (traffic
/// stats and the data plane log) cannot be read and callers fall back to local
/// state.
pub fn container_path() -> AppResult<Option<PathBuf>> {
    platform::container_path()
}

#[cfg(target_os = "macos")]
mod platform {
    use std::{
        ffi::CStr,
        os::raw::{c_char, c_int},
        path::PathBuf,
    };

    use crate::error::AppResult;

    /// Generous upper bound for a container path.
    const BUFFER_LEN: usize = 4096;

    extern "C" {
        fn socks_shared_container_path(buffer: *mut c_char, buffer_len: usize) -> c_int;
    }

    pub fn container_path() -> AppResult<Option<PathBuf>> {
        let mut buffer = vec![0 as c_char; BUFFER_LEN];
        // SAFETY: the helper only writes up to `buffer_len` bytes and always
        // NUL-terminates what it writes.
        let status = unsafe { socks_shared_container_path(buffer.as_mut_ptr(), buffer.len()) };
        if status != 0 {
            return Ok(None);
        }

        let path = unsafe { CStr::from_ptr(buffer.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        if path.is_empty() {
            return Ok(None);
        }
        Ok(Some(PathBuf::from(path)))
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use std::path::PathBuf;

    use crate::error::AppResult;

    pub fn container_path() -> AppResult<Option<PathBuf>> {
        Ok(None)
    }
}
