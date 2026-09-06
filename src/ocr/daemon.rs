// Persistent OCR daemon keeping the recognition model ready in memory between overlay sessions.
// Enforces a single instance per user via `flock`, bound to a UNIX socket in `XDG_RUNTIME_DIR`.
// Exits automatically on idle timeout, explicit `Shutdown`, binary mismatch/upgrade, or engine failure.
// When unavailable, OCR falls back to executing in-process within the overlay.
//
// protocol - Socket wire format and framing primitives
// server   - Daemon service daemon entry point (`--ocr-daemon serve | status | stop`)
// client   - `OcrBackend` client wrapper that delegates to the daemon with automatic local fallback

pub mod client;
pub mod protocol;
pub mod server;
#[cfg(test)]
mod tests;

use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nix::fcntl::{Flock, FlockArg};

use super::models::ModelFiles;
use super::paddle_backend::PaddleBackend;
use super::settings::{Device, EngineSettings, GPU_BUILD, Mode};
use protocol::EngineState;

#[derive(Debug, Clone)]
pub struct Paths {
    pub dir: PathBuf,
    pub socket: PathBuf,
    pub lock: PathBuf,
    pub strikes: PathBuf,
}

impl Paths {
    pub fn from_env() -> Option<Self> {
        let dir = std::env::var_os("XDG_RUNTIME_DIR").filter(|dir| !dir.is_empty())?;
        Some(Self::in_dir(PathBuf::from(dir).join("LumineCapture")))
    }

    pub fn in_dir(dir: PathBuf) -> Self {
        Self {
            socket: dir.join("ocr.sock"),
            lock: dir.join("ocr.lock"),
            strikes: dir.join("ocr.strikes"),
            dir,
        }
    }
}

/// Application version along with binary size and modification timestamp
/// rebuilding or updating changes the ID.
pub fn build_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        let stamp = std::env::current_exe()
            .and_then(fs::metadata)
            .map(|meta| {
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos());
                format!("{}-{mtime}", meta.len())
            })
            .unwrap_or_default();
        format!("{}-{stamp}", env!("CARGO_PKG_VERSION"))
    })
}

pub fn resolve_mode(settings: &EngineSettings) -> Mode {
    let paths = Paths::from_env();
    let ruled_out = paths
        .as_ref()
        .is_some_and(|paths| gpu_ruled_out(&read_strikes(paths), build_id()));
    let mode = settings.resolve(paths.is_some(), GPU_BUILD && !ruled_out);

    if mode == settings.mode {
        eprintln!("ocr: engine mode = {mode:?} (device {:?})", settings.device);
    } else {
        let why = if paths.is_none() {
            "no XDG_RUNTIME_DIR"
        } else if ruled_out {
            "this build already failed to run this engine on a GPU before"
        } else {
            "this build has no GPU support yet"
        };
        eprintln!(
            "ocr: engine mode = {mode:?} (config wants {:?}, but {why} -- \
             set device = cpu in ~/.config/LumineCapture/ocr-engine to force a CPU daemon)",
            settings.mode
        );
    }
    mode
}

pub fn model_name(files: &ModelFiles) -> String {
    files.recognizer.to_string_lossy().into_owned()
}

pub fn engine_factory(device: Device) -> server::Factory {
    Arc::new(move |files: &ModelFiles| match device {
        Device::Cpu => PaddleBackend::new(files)
            .map(|backend| server::Engine {
                backend: Box::new(backend),
                device: "cpu".into(),
            })
            .map_err(|e| server::BuildError::Failed(e.to_string())),
        Device::Auto | Device::Gpu => Err(server::BuildError::NoGpu),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strike {
    NoGpu,
    Failed,
}

const STRIKE_LIMIT: usize = 3;
const STRIKE_WINDOW_SECS: u64 = 600;
const STRIKES_KEPT: usize = 8;

pub fn record_strike(paths: &Paths, build: &str, strike: Strike) {
    let old = read_strikes(paths);
    let mut lines: Vec<&str> = old.lines().collect();
    let word = match strike {
        Strike::NoGpu => "no-gpu",
        Strike::Failed => "failed",
    };
    let line = format!("{} {word} {build}", unix_now());
    lines.push(&line);
    let text = lines[lines.len().saturating_sub(STRIKES_KEPT)..].join("\n") + "\n";
    if let Err(e) = fs::create_dir_all(&paths.dir).and_then(|()| fs::write(&paths.strikes, text)) {
        eprintln!("ocr: cannot record a daemon failure: {e}");
    }
}

pub fn read_strikes(paths: &Paths) -> String {
    fs::read_to_string(&paths.strikes).unwrap_or_default()
}

/// Do not spawn the daemo. Cause build lacks GPU support, or the daemon has crashed 3 times within 10 minutes.
pub fn daemon_blocked(strikes: &str, build: &str, now: u64) -> bool {
    let mut failures = 0;
    for (time, strike) in strikes_of(strikes, build) {
        match strike {
            Strike::NoGpu => return true,
            Strike::Failed if now.saturating_sub(time) < STRIKE_WINDOW_SECS => failures += 1,
            Strike::Failed => {}
        }
    }
    failures >= STRIKE_LIMIT
}

pub fn gpu_ruled_out(strikes: &str, build: &str) -> bool {
    strikes_of(strikes, build).any(|(_, strike)| strike == Strike::NoGpu)
}

fn strikes_of<'a>(strikes: &'a str, build: &'a str) -> impl Iterator<Item = (u64, Strike)> + 'a {
    strikes.lines().filter_map(move |line| {
        let mut parts = line.splitn(3, ' ');
        let time = parts.next()?.parse().ok()?;
        let strike = match parts.next()? {
            "no-gpu" => Strike::NoGpu,
            "failed" => Strike::Failed,
            _ => return None,
        };
        (parts.next()? == build).then_some((time, strike))
    })
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Lock is free implies daemon is inactive 
/// kernel automatically releases flock upon process termination.
pub fn lock_free(path: &Path) -> bool {
    match OpenOptions::new().read(true).open(path) {
        Ok(file) => Flock::lock(file, FlockArg::LockExclusiveNonblock).is_ok(),
        Err(_) => true,
    }
}

pub fn wait_lock_free(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if lock_free(path) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub fn lock_file_pid(paths: &Paths) -> Option<i32> {
    fs::read_to_string(&paths.lock).ok()?.trim().parse().ok()
}

pub fn spawn_daemon() -> io::Result<()> {
    let mut command = Command::new(std::env::current_exe()?);
    command.args(["--ocr-daemon", "serve"]);
    spawn_detached(command, log_path().as_deref())
}

const CLOSE_RANGE_CLOEXEC: u32 = 4;
const LOG_LIMIT: u64 = 512 * 1024;

pub fn spawn_detached(mut command: Command, log: Option<&Path>) -> io::Result<()> {
    let log = log.and_then(|path| {
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        // An unheld lock indicates the daemon is inactive; 
        // kernel automatically releases the `flock` upon process termination.
        let file = OpenOptions::new().create(true).append(true).open(path).ok()?;
        if file.metadata().is_ok_and(|meta| meta.len() > LOG_LIMIT) {
            let _ = file.set_len(0);
        }
        Some(file)
    });
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log.map_or_else(Stdio::null, Stdio::from));
    unsafe {
        command.pre_exec(|| {
            // Spawns a new process session so terminal signals (e.g., `SIGHUP`) do not affect the daemon.
            nix::unistd::setsid().map_err(io::Error::from)?;
            // Ensures overlay file descriptors without `CLOEXEC` (Wayland, PipeWire, portal handles) close automatically on `exec`.
            nix::libc::syscall(nix::libc::SYS_close_range, 3u32, u32::MAX, CLOSE_RANGE_CLOEXEC);
            Ok(())
        });
    }
    let mut child = command.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn log_path() -> Option<PathBuf> {
    Some(dirs::state_dir()?.join("LumineCapture").join("ocr-daemon.log"))
}

pub fn cli(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let paths = Paths::from_env().ok_or("XDG_RUNTIME_DIR is not set, the OCR daemon has nowhere to live")?;
    match args.next().as_deref() {
        None | Some("serve") => serve(paths),
        Some("status") => status(&paths),
        Some("stop") => stop(&paths),
        Some(other) => Err(format!("unknown command `{other}`, expected serve | status | stop").into()),
    }
}

fn serve(paths: Paths) -> Result<(), Box<dyn Error>> {
    let settings = EngineSettings::load();
    let config = server::Config::new(
        paths,
        build_id().to_owned(),
        settings.daemon_idle,
        engine_factory(settings.device),
    );
    server::serve(config)?;
    Ok(())
}

fn status(paths: &Paths) -> Result<(), Box<dyn Error>> {
    let Some(status) = client::status(paths) else {
        if lock_free(&paths.lock) {
            println!("ocr daemon: not running");
            std::process::exit(3);
        }
        let pid = lock_file_pid(paths).map_or("?".to_owned(), |pid| pid.to_string());
        println!("ocr daemon: pid {pid} holds the lock but does not answer (try --ocr-daemon stop)");
        std::process::exit(4);
    };
    let engine = match &status.state {
        EngineState::Idle => "idle, no model requested yet".to_owned(),
        EngineState::Loading { model } => format!("loading {}", file_name(model)),
        EngineState::Ready { device, model } => format!("ready on {device}, {}", file_name(model)),
        EngineState::Failed(e) => format!("failed: {e}"),
        EngineState::NoGpu => "no GPU available, leaving (OCR runs in the overlay instead)".to_owned(),
    };
    println!("ocr daemon: running, pid {}, up {}", status.pid, human(status.uptime_secs));
    println!("engine:     {engine}");
    println!("served:     {} scan(s), last took {} ms", status.served, status.last_ms);
    println!("memory:     {} MB", status.rss_kb / 1024);
    match status.idle_left_secs {
        Some(secs) => println!("idle exit:  in {}", human(secs)),
        None => println!("idle exit:  never"),
    }
    println!("socket:     {}", paths.socket.display());
    if let Some(log) = log_path() {
        println!("log:        {}", log.display());
    }
    Ok(())
}

fn stop(paths: &Paths) -> Result<(), Box<dyn Error>> {
    if client::stop(paths, Duration::from_secs(2), &client::kill_hard)? {
        println!("ocr daemon: stopped");
    } else {
        println!("ocr daemon: not running");
    }
    Ok(())
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn human(secs: u64) -> String {
    match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}m {}s", secs / 60, secs % 60),
        _ => format!("{}h {}m", secs / 3600, secs % 3600 / 60),
    }
}
