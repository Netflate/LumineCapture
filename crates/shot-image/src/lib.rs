//! Integer ARGB pixel buffer operations without GUI framework dependencies.

#![forbid(unsafe_code)]

pub mod buffer;

pub use buffer::{Image, ImageError, ImageView, MAX_PIXELS};
