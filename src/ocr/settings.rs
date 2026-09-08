/// Rules for initializing the OCR engine. From a temporary config file 
/// (`~/.config/LumineCapture/ocr-engine`) using key-value format prior to a global config:
///   mode = daemon | at-launch | on-demand
///   device = auto | cpu | gpu
///   daemon_idle = 900   (idle timeout in seconds; 0 disables auto-exit)

/// see when and why each mode is chosen in [crate::ocr] top comments
use std::path::{Path, PathBuf};
use std::time::Duration;

use log::warn;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// build up when ocr tool is selected, dies with the overlay
    OnDemand,
    /// build up with every programm launch, dies with the overlay
    AtLaunch,
    /// ready engine in resident background process `--ocr-daemon`
    Daemon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    Auto,
    Cpu,
    Gpu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineSettings {
    pub mode: Mode,
    pub device: Device,
    /// `None` disables the idle auto-exit behavior for the daemon.
    pub daemon_idle: Option<Duration>,
}

// (feature = "ocr-gpu")
pub const GPU_BUILD: bool = cfg!(feature = "ocr-gpu");

impl Default for EngineSettings {
    fn default() -> Self {
        Self {
            mode: Mode::OnDemand,
            device: Device::Auto,
            daemon_idle: Some(Duration::from_secs(900)),
        }
    }
}

impl EngineSettings {
    pub fn load() -> Self {
        let Some(path) = path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text),
            Err(_) => {
                write_template(&path);
                Self::default()
            }
        }
    }

    pub fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or_default().trim();
            if line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                warn!("ocr: ignoring setting line without '=': {line}");
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            let understood = match key {
                "mode" => parse_mode(value).map(|mode| settings.mode = mode).is_some(),
                "device" => parse_device(value)
                    .map(|device| settings.device = device)
                    .is_some(),
                "daemon_idle" => value
                    .parse::<u64>()
                    .map(|secs| settings.daemon_idle = (secs > 0).then(|| Duration::from_secs(secs)))
                    .is_ok(),
                _ => true,
            };
            if !understood {
                warn!("ocr: ignoring setting {key} = {value}");
            }
        }
        settings
    }

    /// Resolves the actual execution mode.
    ///
    /// daemon is only beneficial with GPU acceleration. Therefore, if GPU access
    /// is unavailable, `daemon` mode silently falls back to `on-demand`. Explicitly setting
    /// `device = cpu` bypasses this fallback to allow daemon debugging.
    pub fn resolve(&self, daemon_possible: bool, gpu_possible: bool) -> Mode {
        match self.mode {
            Mode::Daemon if !daemon_possible => Mode::OnDemand,
            Mode::Daemon if self.device == Device::Cpu || gpu_possible => Mode::Daemon,
            Mode::Daemon => Mode::OnDemand,
            mode => mode,
        }
    }
}

fn parse_mode(value: &str) -> Option<Mode> {
    match value {
        "on-demand" => Some(Mode::OnDemand),
        "at-launch" => Some(Mode::AtLaunch),
        "daemon" => Some(Mode::Daemon),
        _ => None,
    }
}

fn parse_device(value: &str) -> Option<Device> {
    match value {
        "auto" => Some(Device::Auto),
        "cpu" => Some(Device::Cpu),
        "gpu" => Some(Device::Gpu),
        _ => None,
    }
}

pub const TEMPLATE: &str = "\
# How LumineCapture prepares the OCR engine.
# Delete this file to get these defaults back.

# on-demand  build the engine when OCR is used, inside this process (default)
# at-launch  build it at every launch, inside this process
# daemon     keep a ready engine in a background process, reused by later launches
mode = on-demand

# Only the daemon uses this. A GPU engine reads text about twice as fast, but holds
# video memory for as long as the daemon lives, so nothing runs on the GPU unless
# you ask for the daemon. auto measures this machine once and keeps the faster one.
# The GPU needs libwebgpu_dawn.so (14 MB); the daemon downloads it the first time.
# auto | cpu | gpu
device = auto

# Seconds the daemon may sit unused before it exits. 0 lets it stay forever.
daemon_idle = 900
";

fn write_template(path: &Path) {
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(path, TEMPLATE));
    if let Err(e) = written {
        warn!("ocr: cannot write {}: {e}", path.display());
    }
}

fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("LumineCapture").join("ocr-engine"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_gives_defaults() {
        assert_eq!(EngineSettings::parse(""), EngineSettings::default());
        assert_eq!(EngineSettings::parse("\n  \n# only a comment\n"), EngineSettings::default());
    }

    #[test]
    fn reads_every_key() {
        let settings = EngineSettings::parse("mode = at-launch\ndevice=cpu\ndaemon_idle = 60\n");
        assert_eq!(settings.mode, Mode::AtLaunch);
        assert_eq!(settings.device, Device::Cpu);
        assert_eq!(settings.daemon_idle, Some(Duration::from_secs(60)));
    }

    #[test]
    fn zero_idle_means_never() {
        assert_eq!(EngineSettings::parse("daemon_idle=0").daemon_idle, None);
    }

    #[test]
    fn comments_whitespace_and_unknown_keys_are_ignored() {
        let settings = EngineSettings::parse("  mode = on-demand   # trailing\nfuture_key = 1\n\tdevice\t=\tgpu");
        assert_eq!(settings.mode, Mode::OnDemand);
        assert_eq!(settings.device, Device::Gpu);
    }

    #[test]
    fn bad_values_keep_defaults() {
        let settings = EngineSettings::parse("mode = turbo\ndevice = npu\ndaemon_idle = -5\ngarbage line");
        assert_eq!(settings, EngineSettings::default());
    }

    #[test]
    fn later_lines_win() {
        assert_eq!(EngineSettings::parse("mode=daemon\nmode=on-demand").mode, Mode::OnDemand);
    }

    #[test]
    fn daemon_needs_a_gpu_unless_cpu_is_forced() {
        let daemon = EngineSettings { mode: Mode::Daemon, ..EngineSettings::default() };
        assert_eq!(daemon.resolve(true, true), Mode::Daemon);
        assert_eq!(daemon.resolve(true, false), Mode::OnDemand);
        assert_eq!(daemon.resolve(false, true), Mode::OnDemand);

        let gpu = EngineSettings { device: Device::Gpu, ..daemon };
        assert_eq!(gpu.resolve(true, false), Mode::OnDemand);

        let cpu = EngineSettings { device: Device::Cpu, ..daemon };
        assert_eq!(cpu.resolve(true, false), Mode::Daemon);
        assert_eq!(cpu.resolve(false, false), Mode::OnDemand);
    }

    #[test]
    fn local_modes_are_kept() {
        for mode in [Mode::OnDemand, Mode::AtLaunch] {
            let settings = EngineSettings { mode, ..EngineSettings::default() };
            assert_eq!(settings.resolve(false, false), mode);
            assert_eq!(settings.resolve(true, true), mode);
        }
    }

    #[test]
    fn nothing_runs_in_the_background_by_default() {
        let settings = EngineSettings::default();
        assert_eq!(settings.mode, Mode::OnDemand);
        assert_eq!(settings.resolve(true, true), Mode::OnDemand);
    }

    #[test]
    fn the_template_reads_back_as_the_defaults() {
        assert_eq!(EngineSettings::parse(TEMPLATE), EngineSettings::default());
    }
}
