//! Size-based rolling file logger for the data plane.
//!
//! The extension runs inside a sandboxed process and its `log` output used to go
//! nowhere: the `log` crate only emits records once a logger is installed, and
//! nothing installed one. This module installs a logger that writes every level
//! to `<dir>/socks.log`, rotating once a file passes 20 MiB and keeping at most
//! four files in total (`socks.log`, `.1`, `.2`, `.3`, newest first).

use std::{
    ffi::CStr,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};

use log::{Level, LevelFilter, Log, Metadata, Record};

/// Rotate once the current file reaches this size.
const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
/// Number of files kept in total, the current one included.
const MAX_FILES: usize = 4;
/// Name of the file currently being written.
const CURRENT_FILE: &str = "socks.log";

/// The logger can only be installed once per process, so keep a handle around.
/// A later `socks_core_start` (the network-change restart) must be able to
/// reconfigure it rather than silently lose logging.
static INSTANCE: OnceLock<&'static FileLogger> = OnceLock::new();

/// Install the rolling file logger, or reconfigure an already installed one.
pub fn install(dir: &Path, level: LevelFilter) -> std::io::Result<()> {
    if let Some(logger) = INSTANCE.get() {
        logger.reconfigure(dir, level);
        return Ok(());
    }

    let logger: &'static FileLogger = Box::leak(Box::new(FileLogger::new(dir, level)?));
    // Only fails when another logger is already installed, which is not fatal.
    let _ = log::set_logger(logger);
    log::set_max_level(level);
    let _ = INSTANCE.set(logger);
    Ok(())
}

struct FileLogger {
    level: AtomicUsize,
    dir: Mutex<PathBuf>,
    file: Mutex<Option<RollingFile>>,
}

impl FileLogger {
    fn new(dir: &Path, level: LevelFilter) -> std::io::Result<Self> {
        let file = RollingFile::open(dir, MAX_FILE_BYTES, MAX_FILES)?;
        Ok(Self {
            level: AtomicUsize::new(level as usize),
            dir: Mutex::new(dir.to_path_buf()),
            file: Mutex::new(Some(file)),
        })
    }

    fn reconfigure(&self, dir: &Path, level: LevelFilter) {
        self.level.store(level as usize, Ordering::Relaxed);
        log::set_max_level(level);

        // A poisoned lock must not take logging down with it.
        let mut current_dir = self.dir.lock().unwrap_or_else(|err| err.into_inner());
        if current_dir.as_path() == dir {
            return;
        }

        match RollingFile::open(dir, MAX_FILE_BYTES, MAX_FILES) {
            Ok(file) => {
                *self.file.lock().unwrap_or_else(|err| err.into_inner()) = Some(file);
                *current_dir = dir.to_path_buf();
            }
            Err(err) => {
                log::warn!("failed to move the data plane log to {}: {err}", dir.display());
            }
        }
    }
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() as usize <= self.level.load(Ordering::Relaxed)
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let mut line = String::with_capacity(128);
        push_timestamp(&mut line);
        line.push(' ');
        line.push_str(level_tag(record.level()));
        line.push(' ');
        line.push_str(record.target());
        line.push(' ');
        line.push_str(&record.args().to_string());
        line.push('\n');

        let mut guard = self.file.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(file) = guard.as_mut() {
            file.write_line(&line);
        }
    }

    fn flush(&self) {
        let mut guard = self.file.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(file) = guard.as_mut() {
            let _ = file.file.flush();
        }
    }
}

struct RollingFile {
    dir: PathBuf,
    file: File,
    written: u64,
    max_file_bytes: u64,
    max_files: usize,
}

impl RollingFile {
    fn open(dir: &Path, max_file_bytes: u64, max_files: usize) -> std::io::Result<Self> {
        fs::create_dir_all(dir)?;
        let file = append(dir.join(CURRENT_FILE))?;
        let written = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        Ok(Self {
            dir: dir.to_path_buf(),
            file,
            written,
            max_file_bytes,
            max_files,
        })
    }

    fn write_line(&mut self, line: &str) {
        if self.file.write_all(line.as_bytes()).is_err() {
            return;
        }
        // Flush per line: the extension can be killed at any moment, and a lost
        // tail is exactly what we need while debugging a crash.
        let _ = self.file.flush();
        self.written += line.len() as u64;

        if self.written >= self.max_file_bytes {
            self.rotate();
        }
    }

    /// `socks.log` becomes `socks.log.1`, `.1` becomes `.2`, and the file at
    /// `max_files - 1` is discarded.
    fn rotate(&mut self) {
        let oldest = self.max_files.saturating_sub(1);
        let _ = fs::remove_file(self.rotated_path(oldest));
        for index in (1..oldest).rev() {
            let _ = fs::rename(self.rotated_path(index), self.rotated_path(index + 1));
        }
        let _ = fs::rename(self.dir.join(CURRENT_FILE), self.rotated_path(1));

        match append(self.dir.join(CURRENT_FILE)) {
            Ok(file) => {
                self.file = file;
                self.written = 0;
            }
            Err(err) => {
                // Keep writing to the old handle rather than lose logging.
                log::error!("failed to open a new log file: {err}");
            }
        }
    }

    fn rotated_path(&self, index: usize) -> PathBuf {
        self.dir.join(format!("{CURRENT_FILE}.{index}"))
    }
}

fn append(path: PathBuf) -> std::io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn level_tag(level: Level) -> &'static str {
    match level {
        Level::Error => "ERROR",
        Level::Warn => "WARN ",
        Level::Info => "INFO ",
        Level::Debug => "DEBUG",
        Level::Trace => "TRACE",
    }
}

/// Appends `YYYY-MM-DD HH:MM:SS.mmm` in local time.
fn push_timestamp(out: &mut String) {
    // SAFETY: every call is checked; on failure a placeholder is written so a
    // logging problem can never panic the data plane.
    unsafe {
        let mut now = std::mem::zeroed::<libc::timeval>();
        if libc::gettimeofday(&mut now, std::ptr::null_mut()) != 0 {
            out.push_str("---------- --:--:--.---");
            return;
        }

        let mut local = std::mem::zeroed::<libc::tm>();
        if libc::localtime_r(&now.tv_sec, &mut local).is_null() {
            out.push_str("---------- --:--:--.---");
            return;
        }

        const FORMAT: &std::ffi::CStr = c"%Y-%m-%d %H:%M:%S";
        let mut buffer = [0 as libc::c_char; 32];
        let len = libc::strftime(buffer.as_mut_ptr(), buffer.len(), FORMAT.as_ptr(), &local);
        if len == 0 {
            out.push_str("---------- --:--:--.---");
            return;
        }

        out.push_str(&CStr::from_ptr(buffer.as_ptr()).to_string_lossy());
        let _ = std::fmt::Write::write_fmt(out, format_args!(".{:03}", now.tv_usec / 1000));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "socks-core-log-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn file_names(dir: &Path) -> Vec<String> {
        let mut names = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    #[test]
    fn rotates_once_the_file_size_limit_is_reached() {
        let dir = temp_dir("rotate");
        let mut file = RollingFile::open(&dir, 10, MAX_FILES).unwrap();

        // Each 6-byte line pushes a 10-byte file over the limit every other write.
        for _ in 0..40 {
            file.write_line("aaaaa\n");
        }

        let names = file_names(&dir);
        assert_eq!(
            names,
            vec!["socks.log", "socks.log.1", "socks.log.2", "socks.log.3"],
            "expected the current file plus three rotations, got {names:?}"
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn keeps_writing_to_the_current_file_below_the_limit() {
        let dir = temp_dir("no-rotate");
        let mut file = RollingFile::open(&dir, 1024, MAX_FILES).unwrap();

        file.write_line("one\n");
        file.write_line("two\n");

        assert_eq!(file_names(&dir), vec!["socks.log"]);
        assert_eq!(fs::read_to_string(dir.join(CURRENT_FILE)).unwrap(), "one\ntwo\n");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn timestamp_has_the_expected_shape() {
        let mut line = String::new();
        push_timestamp(&mut line);
        // "2026-10-05 20:31:07.123"
        assert_eq!(line.len(), 23, "unexpected timestamp {line:?}");
        assert_eq!(&line[4..5], "-");
        assert_eq!(&line[10..11], " ");
        assert_eq!(&line[19..20], ".");
    }
}
