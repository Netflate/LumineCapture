// Scrolling capture: frame cropping, preview thumbnails, panel and session state.

pub mod frame;
pub mod panel;
pub mod preview;
pub mod session;
pub mod state;

#[cfg(test)]
mod frame_tests;
#[cfg(test)]
mod panel_tests;
#[cfg(test)]
mod session_tests;
#[cfg(test)]
mod state_tests;

pub use session::ScrollSession;
pub use state::Scroll;

#[cfg(test)]
mod overlay_tests;
