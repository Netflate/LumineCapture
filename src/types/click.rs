use std::time::{Duration, Instant};
use crate::types::color_popover::ColorField;

pub const DOUBLE_CLICK_MS: u64 = 400;
pub const DOUBLE_CLICK_DIST: f32 = 6.0;

/// A double-click is counted only if all of:
/// 1. Both clicks occur on the exact same target 
/// 2. Time elapsed between the two clicks is less than `DOUBLE_CLICK_MS`.
/// 3. Distance between the two clicks is within `DOUBLE_CLICK_DIST`.
#[derive(Debug, Default)]
pub struct DoubleClickTracker<T> {
    last: Option<(Instant, T, (f32, f32))>,
}

impl<T: PartialEq + Copy> DoubleClickTracker<T> {
    pub fn new() -> Self {
        Self { last: None }
    }

    /// Registers a single click on a `target` at a specific position `pos`.
    ///
    /// The `pos` coordinates can be either local or global. The tracker only 
    /// calculates relative distance, so the coordinate system doesn't matter 
    /// as long as the caller is consistent.
    ///
    /// # Returns
    /// * `true` if this click completes a valid double-click sequence.
    /// * `false` if it is the first click, took too long, moved too far, 
    ///   or was on a different target.
    pub fn register(&mut self, target: T, pos: (f32, f32)) -> bool {
        let now = Instant::now();
        let is_double = self.last.is_some_and(|(t, prev_target, prev_pos)| {
            prev_target == target
                && now.duration_since(t) < Duration::from_millis(DOUBLE_CLICK_MS)
                && dist(prev_pos, pos) <= DOUBLE_CLICK_DIST
        });
        
        self.last = Some((now, target, pos));
        
        is_double
    }

    /// Manually clears the tracked click state.
    ///
    /// Useful when the application context changes
    /// to prevent accidental cross-context double-clicks.
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.last = None;
    }
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    (dx * dx + dy * dy).sqrt()
}

/// Identifiers for interactive objects that support double-click tracking.
///
/// This serves as the target type `T` for the `DoubleClickTracker`, allowing 
/// the system to distinguish between clicks on different UI elements or annotations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickTarget {
    SettingsWidget(usize),
    TextAnnotation(u64),
    ColorField(ColorField),
}