// ── input region helpers ─────────────────────────────────────────────────────
// Input regions may only be updated while the surface is mapped; committing
// before or after unmap causes the compositor to drop connection on invalid sizes.

use crate::backend::InputHole;
use smithay_client_toolkit::compositor::{CompositorState, Region};
use wayland_client::protocol::wl_surface;

pub fn apply_input_hole(
    compositor: &CompositorState,
    surface: &wl_surface::WlSurface,
    (width, height): (i32, i32),
    hole: InputHole,
) {
    let Ok(region) = Region::new(compositor) else {
        return;
    };
    region.add(0, 0, width, height);
    let (x, y, w, h) = hole.hole;
    region.subtract(x, y, w, h);
    if let Some((x, y, w, h)) = hole.keep {
        region.add(x, y, w, h);
    }
    surface.set_input_region(Some(region.wl_region()));
}
