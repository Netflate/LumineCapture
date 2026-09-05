/// Rules for initializing the OCR engine. From a temporary config file 
/// (`~/.config/LumineCapture/ocr-engine`) using key-value format prior to a global config:
///   mode = daemon | at-launch | on-demand
///   device = auto | cpu | gpu
///   daemon_idle = 900   (idle timeout in seconds; 0 disables auto-exit)

/// see when and why each mode is chosen in [crate::ocr] top comments
use std::path::PathBuf;
use std::time::Duration;

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
pub struct EngineSettings {
    pub mode: Mode,
    /// `None` disables the idle auto-exit behavior for the daemon.
    pub daemon_idle: Option<Duration>,
}

impl Default for EngineSettings {
    fn default() -> Self {
        Self {
            mode: Mode::Daemon,
            daemon_idle: Some(Duration::from_secs(900)),
        }
    }
}

impl EngineSettings {
    pub fn load() -> Self {
        path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map_or_else(Self::default, |text| Self::parse(&text))
    }

    pub fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or_default().trim();
            if line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                eprintln!("ocr: ignoring setting line without '=': {line}");
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            let understood = match key {
                "mode" => parse_mode(value).map(|mode| settings.mode = mode).is_some(),
                "daemon_idle" => value
                    .parse::<u64>()
                    .map(|secs| settings.daemon_idle = (secs > 0).then(|| Duration::from_secs(secs)))
                    .is_ok(),
                _ => true,
            };
            if !understood {
                eprintln!("ocr: ignoring setting {key} = {value}");
            }
        }
        settings
    }

    pub fn resolve(&self, daemon_possible: bool) -> Mode {
        match self.mode {
            Mode::Daemon if !daemon_possible => Mode::OnDemand,
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
        let settings = EngineSettings::parse("mode = at-launch\ndaemon_idle = 60\n");
        assert_eq!(settings.mode, Mode::AtLaunch);
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
    fn daemon_needs_a_runtime_dir() {
        let daemon = EngineSettings::default();
        assert_eq!(daemon.resolve(true), Mode::Daemon);
        assert_eq!(daemon.resolve(false), Mode::OnDemand);
    }

    #[test]
    fn local_modes_are_kept() {
        for mode in [Mode::OnDemand, Mode::AtLaunch] {
            let settings = EngineSettings { mode, ..EngineSettings::default() };
            assert_eq!(settings.resolve(false), mode);
            assert_eq!(settings.resolve(true), mode);
        }
    }
}
