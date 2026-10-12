pub mod daemon;
pub mod ext_data_control;

use std::time::Duration;

/// overlay re-execs itself with these args. `cli` on the other side reads them
pub const DAEMON_ARG: &str = "--clipboard-daemon";
pub const TEXT_ARG: &str = "text";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    Clipboard,
    Primary,
}

impl Selection {
    pub fn arg(self) -> &'static str {
        match self {
            Self::Clipboard => "clipboard",
            Self::Primary => "primary",
        }
    }

    pub fn from_arg(arg: &str) -> Option<Self> {
        [Self::Clipboard, Self::Primary]
            .into_iter()
            .find(|s| s.arg() == arg)
    }
}

/// how long the parent waits for the helper to claim the selection
pub const READY_TIMEOUT: Duration = Duration::from_secs(3);
