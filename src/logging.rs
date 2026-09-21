// ── Logging ───────────────────────────────────────────────────────────────────
//
// Every process this binary spawns (overlay, pin, clipboard helper, OCR daemon)
// writes to this one file
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use log::{Level, LevelFilter, Log, Metadata, Record};

/// default logging level, console doesn't need anythung besides warnings
const DEFAULT_STDERR_LEVEL: LevelFilter = LevelFilter::Warn;
const DEFAULT_DAEMON_STDERR_LEVEL: LevelFilter = LevelFilter::Info;
const DEFAULT_FILE_LEVEL: LevelFilter = LevelFilter::Info;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Process {
    Overlay,
    Pin,
    Clipboard,
    OcrDaemon,
}

impl Process {
    fn tag(self) -> &'static str {
        match self {
            Process::Overlay => "lumine",
            Process::Pin => "pin",
            Process::Clipboard => "clipboard",
            Process::OcrDaemon => "ocr-daemon",
        }
    }
}

struct Logger {
    tag: &'static str,
    pid: u32,
    stderr_level: LevelFilter,
    file_level: LevelFilter,
    /// dependencies log through the same facade (zbus alone narrates every d-bus command)
    deps: bool,
    file: Option<Mutex<File>>,
}

/// Starts logging for this process. `LUMINE_LOG` (error|warn|info|debug|trace)
pub fn init(process: Process) {
    let asked = std::env::var("LUMINE_LOG")
        .ok()
        .and_then(|v| v.trim().parse::<LevelFilter>().ok())
        .or_else(|| {
            crate::config::get()
                .log
                .level
                .as_deref()
                .and_then(|v| v.trim().parse::<LevelFilter>().ok())
        });

    // the daemon's stderr is already redirected into the log file
    // so it logs everything there through stderr and keeps no sink
    let daemon = process == Process::OcrDaemon;
    let stderr_level = asked.unwrap_or(if daemon {
        DEFAULT_DAEMON_STDERR_LEVEL
    } else {
        DEFAULT_STDERR_LEVEL
    });
    let file_level = if daemon {
        LevelFilter::Off
    } else {
        asked.unwrap_or(DEFAULT_FILE_LEVEL)
    };

    let file = (file_level != LevelFilter::Off)
        .then(open_log_file)
        .flatten()
        .map(Mutex::new);

    let logger = Logger {
        tag: process.tag(),
        pid: std::process::id(),
        stderr_level,
        file_level,
        deps: std::env::var_os("LUMINE_LOG_DEPS").is_some(),
        file,
    };

    let max = stderr_level.max(file_level);
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(max);
        log_panics();
    }
    if process == Process::Overlay {
        crate::config::log_warnings();
    }
}

pub fn log_file_path() -> Option<PathBuf> {
    Some(dirs::state_dir()?.join("LumineCapture").join("lumine.log"))
}

/// Opens the log for appending, keeping one older file around.
pub fn open_log_file() -> Option<File> {
    let path = log_file_path()?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).ok()?;
    }

    let limit = crate::config::get().log.max_file_size_mb * 1024 * 1024;
    if fs::metadata(&path).is_ok_and(|meta| meta.len() > limit) {
        let _ = fs::rename(&path, path.with_extension("log.1"));
    }

    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()
}

fn log_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panic: {info}");
        previous(info);
    }));
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.stderr_level.max(self.file_level) && self.ours(metadata.target())
    }

    fn log(&self, record: &Record) {
        if !self.ours(record.target()) {
            return;
        }
        let level = record.level();
        let to_stderr = level <= self.stderr_level;
        let to_file = level <= self.file_level;
        if !to_stderr && !to_file {
            return;
        }

        let line = self.format(record);
        if to_stderr {
            let _ = std::io::stderr().write_all(line.as_bytes());
        }
        if to_file
            && let Some(file) = &self.file
            && let Ok(mut file) = file.lock()
        {
            let _ = file.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        let _ = std::io::stderr().flush();
        if let Some(file) = &self.file
            && let Ok(mut file) = file.lock()
        {
            let _ = file.flush();
        }
    }
}

impl Logger {
    fn ours(&self, target: &str) -> bool {
        self.deps || target.starts_with(env!("CARGO_CRATE_NAME"))
    }

    fn format(&self, record: &Record) -> String {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let level = record.level();
        let where_ = match level {
            Level::Debug | Level::Trace => format!(" {}", record.target()),
            _ => String::new(),
        };
        format!(
            "{now} {level:5} [{} {}]{where_} {}\n",
            self.tag,
            self.pid,
            record.args()
        )
    }
}
