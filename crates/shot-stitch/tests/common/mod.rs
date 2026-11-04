//! A synthetic test desktop simulating a realistic web page scrolling beneath fixed ui and floating panels.
//! Used to stress-test stitching algorithms against repeating text patterns and static overlays,
//! ensuring they don't break when content moves under sticky headers or floating elements.
#![allow(dead_code)]

use shot_image::Image;

pub const WHITE: u32 = 0xffffffff;
pub const BLACK: u32 = 0xff000000;

fn hash(a: u32, b: u32) -> u32 {
    let mut h = a.wrapping_mul(0x9e37_79b1) ^ b.wrapping_mul(0x85eb_ca77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h
}

fn gray(level: u32) -> u32 {
    0xff000000 | level << 16 | level << 8 | level
}

/// Words of a text line: dark strokes on `background`, one glyph every 9 pixels.
fn text_row(row: &mut [u32], y_in_line: i32, seed: u32, x0: usize, x1: usize, ink: u32) {
    if !(2..14).contains(&y_in_line) {
        return;
    }
    let mut x = x0;
    let mut word = 0;
    while x < x1 {
        let len = 3 + (hash(seed, word) % 7) as usize;
        for g in 0..len {
            let glyph = hash(seed ^ 0x55, word * 16 + g as u32);
            for gx in 0..7 {
                let px = x + g * 9 + gx;
                if px >= x1 {
                    break;
                }
                // a few strokes per glyph, some rows of it only
                if glyph >> (gx * 3 % 29) & 1 == 1 && hash(glyph, y_in_line as u32) % 3 != 0 {
                    row[px] = ink;
                }
            }
        }
        x += len * 9 + 12;
        word += 1;
    }
}

/// What the scrollable part of the page shows at y row:
/// generated web layout consisting of a dark hero section with titles and a picture,
/// followed by an infinite repeating feed of sections with paragraphs of text, cards with
/// gradients or sub-text, vertical borders, and section dividers.
pub fn page_row(y: i32, width: usize) -> Vec<u32> {
    let mut row = vec![WHITE; width];
    let left = width / 7;
    let right = width - width / 7;
    // a dark hero, then white sections of text and cards
    if y < 760 {
        row.fill(BLACK);
        if (140..330).contains(&y) {
            text_row(
                &mut row,
                (y - 140) % 38,
                (y - 140) as u32 / 38,
                left,
                width / 2,
                WHITE,
            );
        }
        if (90..600).contains(&y) {
            // picture: smooth gradients with some detail
            for (x, p) in row.iter_mut().enumerate().take(right).skip(width / 2 + 40) {
                let v =
                    ((x as i32 * 3 + y * 2) % 200) as u32 + (hash(x as u32 / 6, y as u32 / 6) % 40);
                *p = 0xff000000 | (v / 2) << 16 | v << 8 | (255 - v);
            }
        }
        if (690..700).contains(&y) {
            for (x, p) in row.iter_mut().enumerate().take(right).skip(left) {
                *p = gray(if (x / 220) % 2 == 0 { 70 } else { 50 });
            }
        }
        return row;
    }
    let y = y - 760;
    let section = y / 700;
    let inner = y % 700;
    if inner < 80 {
        // blank space between sections
        return row;
    }
    if inner < 120 {
        text_row(
            &mut row,
            (inner - 80) / 2,
            1000 + section as u32,
            left,
            width / 2,
            gray(20),
        );
        return row;
    }
    // paragraphs on the left, cards on the right
    if inner < 600 {
        text_row(
            &mut row,
            (inner - 120) % 38,
            (section * 100 + (inner - 120) / 38) as u32,
            left,
            width * 2 / 5,
            gray(40),
        );
        let card = (width * 9 / 20, right);
        if (140..360).contains(&inner) {
            for (x, p) in row.iter_mut().enumerate().take(card.1).skip(card.0) {
                let v = ((x as i32 + inner * 3 + section * 50) % 256) as u32;
                *p = 0xff000000
                    | v << 16
                    | (255 - v) << 8
                    | (v / 2 + hash(x as u32 / 5, (y / 5) as u32) % 30);
            }
        } else if (380..560).contains(&inner) {
            text_row(
                &mut row,
                (inner - 380) % 26,
                (section * 77 + inner / 26) as u32,
                card.0 + 20,
                card.1 - 20,
                gray(30),
            );
        }
        // card borders: a vertical line that stays put under scrolling
        if (130..570).contains(&inner) {
            row[card.0] = gray(210);
            row[card.1 - 1] = gray(210);
        }
    } else if inner == 650 {
        row[left..right].fill(gray(225));
    }
    row
}

#[derive(Clone, Copy)]
pub struct Screen {
    pub width: usize,
    pub height: usize,
    /// rows like system bar, browser chrome and the sticky header of the site
    pub top: usize,
    /// columns of a sidebar left of the page
    pub sidebar: usize,
    /// a floating dock (x, y, w, h)
    pub dock: Option<(usize, usize, usize, usize)>,
    /// preview link browser shows in the bottom-left corner, in odd frames
    pub tooltip: Option<(usize, usize)>,
    /// a transparent plaque the page shows through (x, y, w, h)
    pub glass: Option<(usize, usize, usize, usize)>,
    /// an opaque round button, like a chat bubble (x, y, size)
    pub button: Option<(usize, usize, usize)>,
    /// video in the page, playing: x, page row, w, h
    pub video: Option<(usize, i32, usize, i32)>,
    /// columns of a list that stays put right of the sidebar, like discord
    pub list: usize,
    /// columns of another one at the right edge, like the members
    pub members: usize,
    /// rows of a bar under the page that stays put, like message box
    pub footer: usize,
}

impl Screen {
    /// Whether the page shows the video at its row `y` and column `x`, which looks different
    /// in every frame.
    pub fn plays(&self, y: i32, x: usize) -> bool {
        self.video.is_some_and(|(vx, vy, vw, vh)| {
            (vy..vy + vh).contains(&y) && (vx..vx + vw).contains(&x)
        })
    }
}

impl Screen {
    /// the whole monitor, like the second screenshot
    pub fn monitor() -> Self {
        Self {
            width: 1920,
            height: 1080,
            top: 147,
            sidebar: 55,
            dock: Some((757, 1025, 406, 47)),
            tooltip: None,
            glass: None,
            button: None,
            video: None,
            list: 0,
            members: 0,
            footer: 0,
        }
    }

    /// a region over the page only: the site header and a dock at the bottom in it
    pub fn region() -> Self {
        Self {
            width: 1240,
            height: 900,
            top: 46,
            sidebar: 0,
            dock: Some((470, 845, 406, 47)),
            tooltip: Some((32, 870)),
            glass: None,
            button: None,
            video: None,
            list: 0,
            members: 0,
            footer: 0,
        }
    }

    /// a messenger: the chats on the left stay put, the messages scroll
    pub fn messenger() -> Self {
        Self {
            dock: None,
            tooltip: None,
            list: 300,
            ..Self::region()
        }
    }

    /// the whole monitor over a chat app: servers, channels and members stay put around a
    /// column of messages, the box to type in under it, the dock over that, a button and a
    /// link over the messages
    pub fn chat_app() -> Self {
        Self {
            top: 130,
            sidebar: 80,
            list: 400,
            members: 300,
            footer: 80,
            dock: Some((780, 1028, 360, 44)),
            // a button to jump to the latest messages (like in telegram or discord), a link under the point
            button: Some((1540, 900, 48)),
            tooltip: Some((490, 975)),
            ..Self::monitor()
        }
    }

    /// Columns of the page on screen.
    pub fn page_columns(&self) -> std::ops::Range<usize> {
        self.sidebar + self.list..self.width - self.members
    }

    /// everything at once, floats in the middle of the screen too
    pub fn busy() -> Self {
        Self {
            glass: Some((300, 380, 520, 90)),
            button: Some((1130, 520, 64)),
            ..Self::region()
        }
    }
}

fn blend(under: u32, over: u32, alpha: u32) -> u32 {
    let mix = |s: u32| {
        let (a, b) = ((under >> s) & 255, (over >> s) & 255);
        (a * (255 - alpha) + b * alpha) / 255
    };
    0xff000000 | mix(16) << 16 | mix(8) << 8 | mix(0)
}

/// Row `y` of a list of chats: an avatar, a name and a line of the last message for every
/// chat, a border on its right. It doesn't scroll with the page.
fn list_row(row: &mut [u32], y: usize) {
    if row.is_empty() {
        return;
    }
    let width = row.len();
    row.fill(gray(24));
    let (chat, inner) = (y / 64, (y % 64) as i32);
    if (12..52).contains(&inner) {
        let color = 0xff000000 | hash(chat as u32, 77) & 0xffffff;
        for p in &mut row[12..52.min(width)] {
            *p = color;
        }
    }
    text_row(
        row,
        inner - 14,
        300 + chat as u32,
        64,
        width * 3 / 4,
        gray(225),
    );
    text_row(
        row,
        inner - 36,
        600 + chat as u32,
        64,
        width - 16,
        gray(140),
    );
    row[width - 1] = gray(60);
}

/// The screen with the page scrolled by `scroll`, the `n`th frame.
pub fn shot(screen: Screen, scroll: i32, n: u32) -> Image {
    let Screen {
        width,
        height,
        top,
        sidebar,
        ..
    } = screen;
    let mut pixels = Vec::with_capacity(width * height);
    let page = screen.page_columns();
    for y in 0..height {
        let mut row = vec![WHITE; width];
        if y < top {
            // chrome: a darker bar with labels, the last rows the site header
            let shade = if y < top.saturating_sub(42) { 45 } else { 250 };
            row.fill(gray(shade));
            let ink = if shade < 128 { gray(220) } else { gray(30) };
            text_row(
                &mut row,
                (y % 42) as i32 - 12,
                9000 + (y / 42) as u32,
                width / 6,
                width * 5 / 6,
                ink,
            );
            // a clock that changes now and then
            if y < 30 {
                text_row(
                    &mut row,
                    y as i32 - 8,
                    7000 + n / 40,
                    width / 2 - 60,
                    width / 2 + 60,
                    ink,
                );
            }
        } else {
            row[..sidebar].fill(gray(30));
            list_row(&mut row[sidebar..page.start], y - top);
            list_row(&mut row[page.end..], y - top + 2000);
            if y >= height - screen.footer {
                // a box to type in, with a hint in it
                row[page.clone()].fill(gray(48));
                text_row(
                    &mut row,
                    (y - (height - screen.footer)) as i32 - 30,
                    4444,
                    page.start + 60,
                    page.start + 300,
                    gray(150),
                );
            } else {
                let page_y = scroll + (y - top) as i32;
                let mut content = page_row(page_y, page.len());
                for (x, p) in content.iter_mut().enumerate() {
                    if screen.plays(page_y, x) {
                        *p = 0xff000000 | hash(hash(x as u32, page_y as u32), n) & 0xffffff;
                    }
                }
                row[page.clone()].copy_from_slice(&content);
            }
        }
        pixels.extend_from_slice(&row);
    }
    let mut image = pixels;
    if let Some((dx, dy, dw, dh)) = screen.dock {
        // a translucent dock with icons, rounded corners and a soft shadow
        const SHADOW: usize = 8;
        for y in dy.saturating_sub(SHADOW)..(dy + dh + SHADOW).min(height) {
            for x in dx.saturating_sub(SHADOW)..(dx + dw + SHADOW).min(width) {
                let p = &mut image[y * width + x];
                let ox = (dx as i32 - x as i32)
                    .max(x as i32 - (dx + dw - 1) as i32)
                    .max(0);
                let oy = (dy as i32 - y as i32)
                    .max(y as i32 - (dy + dh - 1) as i32)
                    .max(0);
                let outside = ((ox * ox + oy * oy) as f32).sqrt();
                if outside > 0.0 {
                    let alpha = (60.0 * (1.0 - outside / SHADOW as f32)).max(0.0) as u32;
                    *p = blend(*p, BLACK, alpha);
                    continue;
                }
                let icon = (x - dx) % 48;
                let inside_icon = (8..40).contains(&icon) && (8..39).contains(&(y - dy));
                *p = if inside_icon {
                    let v = hash(
                        (x - dx) as u32 / 48,
                        ((x - dx) % 48 * 31 + (y - dy)) as u32 / 5,
                    );
                    0xff000000 | (v & 0xffffff)
                } else {
                    blend(*p, gray(40), 200)
                };
            }
        }
    }
    if let Some((gx, gy, gw, gh)) = screen.glass {
        // frosted: half white over whatever scrolls under it, a label on top
        for y in gy..gy + gh {
            let row = &mut image[y * width..(y + 1) * width];
            for p in &mut row[gx..gx + gw] {
                *p = blend(*p, WHITE, 150);
            }
            text_row(
                row,
                (y - gy) as i32 - 30,
                5151,
                gx + 20,
                gx + gw - 20,
                gray(10),
            );
        }
    }
    if let Some((bx, by, size)) = screen.button {
        let r = size as f32 / 2.0;
        for y in by..by + size {
            for x in bx..bx + size {
                let (dx, dy) = (
                    x as f32 - bx as f32 - r + 0.5,
                    y as f32 - by as f32 - r + 0.5,
                );
                let d = (dx * dx + dy * dy).sqrt();
                let p = &mut image[y * width + x];
                if d < r - 1.0 {
                    *p = if (dx.abs() < 12.0 && dy.abs() < 3.0)
                        || (dx.abs() < 3.0 && dy.abs() < 12.0)
                    {
                        WHITE
                    } else {
                        0xff2f7de1
                    };
                } else if d < r {
                    // an antialiased rim
                    *p = blend(*p, 0xff2f7de1, ((r - d) * 255.0) as u32);
                }
            }
        }
    }
    if let Some((tx, ty)) = screen.tooltip
        && n % 3 != 0
    {
        for y in ty..(ty + 22).min(height) {
            let row = &mut image[y * width..(y + 1) * width];
            row[tx..tx + 600].fill(gray(25));
            text_row(row, (y - ty) as i32 - 3, 4242, tx + 6, tx + 594, gray(230));
        }
    }
    Image::from_pixels(width as i32, height as i32, image).unwrap()
}

/// Scroll positions of a wheel with smooth scrolling: bursts that ease out.
pub fn smooth(end: i32, burst: i32) -> Vec<i32> {
    let mut out = vec![0];
    let mut at = 0;
    while at < end {
        let target = (at + burst).min(end);
        let start = at;
        for step in 1..=6 {
            let t = step as f32 / 6.0;
            let eased = 1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t);
            at = start + ((target - start) as f32 * eased).round() as i32;
            out.push(at);
        }
        // a pause between flicks: the same frame a few times
        out.push(at);
        out.push(at);
    }
    out
}
