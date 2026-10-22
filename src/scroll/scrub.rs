// Frame border artifact scrubber: removes residual selection border pixels.
// Ported from Mark Shot's scrubCaptureFrameArtifacts.

const SCAN_PX: usize = 10;
const COLOR_DISTANCE: u32 = 70;
const MIN_HITS: usize = 12;
const HIT_PERCENT: usize = 35;

const ARTIFACT_COLORS: [(u32, u32, u32); 2] = [(45, 212, 191), (250, 204, 21)];

fn is_artifact_pixel(pixel: u32) -> bool {
    let (r, g, b) = ((pixel >> 16) & 255, (pixel >> 8) & 255, pixel & 255);
    ARTIFACT_COLORS.iter().any(|&(cr, cg, cb)| {
        let (dr, dg, db) = (
            r as i32 - cr as i32,
            g as i32 - cg as i32,
            b as i32 - cb as i32,
        );
        (dr * dr + dg * dg + db * db) as u32 <= COLOR_DISTANCE * COLOR_DISTANCE
    })
}

fn row_is_artifact(pixels: &[u32], width: usize, y: usize) -> bool {
    let row = &pixels[y * width..(y + 1) * width];
    let hits = row.iter().filter(|p| is_artifact_pixel(**p)).count();
    hits >= MIN_HITS.max(width * HIT_PERCENT / 100)
}

fn column_is_artifact(pixels: &[u32], width: usize, height: usize, x: usize) -> bool {
    let hits = (0..height)
        .filter(|y| is_artifact_pixel(pixels[y * width + x]))
        .count();
    hits >= MIN_HITS.max(height * HIT_PERCENT / 100)
}

fn replacement_row(pixels: &[u32], width: usize, height: usize, top: bool) -> Option<usize> {
    if top {
        for y in SCAN_PX.min(height - 1)..height {
            if !row_is_artifact(pixels, width, y) {
                return Some(y);
            }
        }
        None
    } else {
        for y in (0..=height.saturating_sub(SCAN_PX + 1)).rev() {
            if !row_is_artifact(pixels, width, y) {
                return Some(y);
            }
        }
        None
    }
}

fn replacement_column(pixels: &[u32], width: usize, height: usize, left: bool) -> Option<usize> {
    if left {
        for x in SCAN_PX.min(width - 1)..width {
            if !column_is_artifact(pixels, width, height, x) {
                return Some(x);
            }
        }
        None
    } else {
        for x in (0..=width.saturating_sub(SCAN_PX + 1)).rev() {
            if !column_is_artifact(pixels, width, height, x) {
                return Some(x);
            }
        }
        None
    }
}

fn copy_row(pixels: &mut [u32], width: usize, dst: usize, src: usize) {
    if dst == src {
        return;
    }
    let from = src * width;
    let to = dst * width;
    for x in 0..width {
        pixels[to + x] = pixels[from + x];
    }
}

fn copy_column(pixels: &mut [u32], width: usize, height: usize, dst: usize, src: usize) {
    if dst == src {
        return;
    }
    for y in 0..height {
        pixels[y * width + dst] = pixels[y * width + src];
    }
}

/// Scrubs selection rectangle border artifacts around pixels, returning number of cleaned edges (0..=4).
pub fn apply(pixels: &mut [u32], width: i32, height: i32) -> u32 {
    let (w, h) = (width.max(0) as usize, height.max(0) as usize);
    if w == 0 || h == 0 || pixels.len() < w * h {
        return 0;
    }
    let mut scrubbed = 0;

    // top and bottom
    let scan_rows = SCAN_PX.min(h);
    if let Some(replacement) = replacement_row(pixels, w, h, true) {
        let mut changed = false;
        for y in 0..scan_rows {
            if row_is_artifact(pixels, w, y) {
                copy_row(pixels, w, y, replacement);
                changed = true;
            }
        }
        scrubbed += changed as u32;
    }
    if let Some(replacement) = replacement_row(pixels, w, h, false) {
        let mut changed = false;
        for y in h - scan_rows..h {
            if row_is_artifact(pixels, w, y) {
                copy_row(pixels, w, y, replacement);
                changed = true;
            }
        }
        scrubbed += changed as u32;
    }

    // left and right
    let scan_columns = SCAN_PX.min(w);
    if let Some(replacement) = replacement_column(pixels, w, h, true) {
        let mut changed = false;
        for x in 0..scan_columns {
            if column_is_artifact(pixels, w, h, x) {
                copy_column(pixels, w, h, x, replacement);
                changed = true;
            }
        }
        scrubbed += changed as u32;
    }
    if let Some(replacement) = replacement_column(pixels, w, h, false) {
        let mut changed = false;
        for x in w - scan_columns..w {
            if column_is_artifact(pixels, w, h, x) {
                copy_column(pixels, w, h, x, replacement);
                changed = true;
            }
        }
        scrubbed += changed as u32;
    }

    scrubbed
}
