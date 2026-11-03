use shot_image::Image;
use shot_stitch::{Axis, Config, Edge, Status, Stitcher};

fn frame(origin: i32, width: i32, height: i32) -> Image {
    let mut pixels = Vec::new();
    for y in origin..origin + height {
        for x in 0..width {
            let hash = (y as u32).wrapping_mul(2_654_435_761).rotate_left(7)
                ^ (x as u32).wrapping_mul(2246822519);
            pixels.push(0xff000000 | (hash & 0x00ffffff));
        }
    }
    Image::from_pixels(width, height, pixels).unwrap()
}

fn config() -> Config {
    Config {
        min_overlap: 32,
        accept_diff: 0.5,
        min_append: 10,
    }
}

#[test]
fn forward_revisit_and_prepend_keep_exact_content() {
    let mut stitch = Stitcher::new(config()).unwrap();
    assert_eq!(
        stitch.push(frame(100, 80, 180)).unwrap().status,
        Status::FirstFrame
    );
    let result = stitch.push(frame(145, 80, 180)).unwrap();
    assert_eq!(
        (result.status, result.added, result.edge),
        (Status::Appended, 45, Edge::End)
    );
    let result = stitch.push(frame(110, 80, 180)).unwrap();
    assert_eq!((result.status, result.position), (Status::NoProgress, 10));
    assert_eq!(
        stitch.push(frame(100, 80, 180)).unwrap().status,
        Status::NoProgress
    );
    let result = stitch.push(frame(60, 80, 180)).unwrap();
    assert_eq!(
        (result.status, result.added, result.edge),
        (Status::Appended, 40, Edge::Start)
    );
    assert_eq!(stitch.full_image().unwrap().unwrap(), frame(60, 80, 265));
    assert_eq!(stitch.stats().frame_count, 3);
}

#[test]
fn pending_motion_accumulates_against_original_anchor() {
    let mut stitch = Stitcher::new(config()).unwrap();
    stitch.push(frame(0, 80, 180)).unwrap();
    for origin in [3, 6, 9] {
        assert_eq!(
            stitch.push(frame(origin, 80, 180)).unwrap().status,
            Status::NoProgress
        );
    }
    let result = stitch.push(frame(12, 80, 180)).unwrap();
    assert_eq!((result.status, result.added), (Status::Appended, 12));
    assert_eq!(stitch.full_image().unwrap().unwrap(), frame(0, 80, 192));
}

#[test]
fn horizontal_stitch_and_axis_lock() {
    let mut stitch = Stitcher::new(config()).unwrap();
    assert!(stitch.set_axis(Axis::Horizontal).unwrap());
    stitch.push(frame(0, 64, 180).transpose().unwrap()).unwrap();
    assert!(!stitch.axis_locked());
    assert_eq!(
        stitch
            .push(frame(40, 64, 180).transpose().unwrap())
            .unwrap()
            .status,
        Status::Appended
    );
    assert!(!stitch.set_axis(Axis::Vertical).unwrap());
    assert_eq!(
        stitch.full_image().unwrap().unwrap(),
        frame(0, 64, 220).transpose().unwrap()
    );
}

#[test]
fn seed_axis_switch_preserves_image_and_cache() {
    let mut stitch = Stitcher::new(config()).unwrap();
    let seed = frame(0, 64, 180);
    stitch.push(seed.clone()).unwrap();
    assert!(stitch.set_axis(Axis::Horizontal).unwrap());
    assert_eq!(stitch.full_image().unwrap(), Some(seed.clone()));
    let revision = stitch.revision();
    assert_eq!(stitch.full_image().unwrap(), Some(seed));
    assert_eq!(revision, stitch.revision());
}

#[test]
fn rejected_frames_do_not_replace_anchor() {
    let mut stitch = Stitcher::new(config()).unwrap();
    stitch.push(frame(0, 80, 180)).unwrap();
    assert_eq!(
        stitch.push(frame(0, 81, 180)).unwrap().status,
        Status::NoMatch
    );
    assert_eq!(
        stitch.push(frame(10000, 80, 180)).unwrap().status,
        Status::NoMatch
    );
    assert_eq!(
        stitch.push(frame(40, 80, 180)).unwrap().status,
        Status::Appended
    );
    assert_eq!(stitch.full_image().unwrap().unwrap(), frame(0, 80, 220));
    let mut blank = Stitcher::new(config()).unwrap();
    let image = Image::from_pixels(80, 180, vec![0xffffffff; 80 * 180]).unwrap();
    blank.push(image.clone()).unwrap();
    assert_eq!(blank.push(image).unwrap().status, Status::NoProgress);
}

#[test]
fn config_and_session_isolation() {
    assert!(
        Stitcher::new(Config {
            min_overlap: i32::MAX,
            ..config()
        })
        .is_err()
    );
    assert!(
        Stitcher::new(Config {
            min_overlap: 0,
            ..config()
        })
        .is_err()
    );
    assert!(
        Stitcher::new(Config {
            accept_diff: f32::NAN,
            ..config()
        })
        .is_err()
    );
    let mut first = Stitcher::new(config()).unwrap();
    let second = Stitcher::new(config()).unwrap();
    first.push(frame(0, 80, 180)).unwrap();
    assert_eq!(second.stats().frame_count, 0);
}

/// A floating fixed element
fn pinned(origin: i32, (x, y, w, h): (i32, i32, i32, i32)) -> Image {
    let page = frame(origin, 80, 180);
    let mut pixels = page.pixels().to_vec();
    for row in y..y + h {
        for col in x..x + w {
            pixels[(row * 80 + col) as usize] = if (row + col) % 3 == 0 {
                0xffffffff
            } else {
                0xff3a6ea5
            };
        }
    }
    Image::from_pixels(80, 180, pixels).unwrap()
}

fn scroll_past(float: (i32, i32, i32, i32)) {
    let mut stitch = Stitcher::new(Config::default()).unwrap();
    let last = 200;
    for origin in (0..=last).step_by(20) {
        stitch.push(pinned(origin, float)).unwrap();
    }
    let result = stitch.full_image().unwrap().unwrap();
    let page = frame(0, 80, last + 180);
    assert_eq!(result.height(), page.height());
    let (_, y, _, h) = float;
    let kept = last + y..last + y + h;
    for row in 0..result.height() {
        if !kept.contains(&row) {
            assert_eq!(result.row(row), page.row(row), "row {row}");
        }
    }
}

/// like "jump to bottom"
fn above_footer(origin: i32) -> Image {
    let page = frame(origin, 80, 400);
    let mut pixels = page.pixels().to_vec();
    for row in 334..400 {
        for col in 0..80 {
            pixels[(row * 80 + col) as usize] = if row >= 360 {
                0xff202020 | ((row * 7 + col * 3) as u32 & 0x3f)
            } else if (30..70).contains(&col) {
                if (row + col) % 3 == 0 {
                    0xffffffff
                } else {
                    0xff3a6ea5
                }
            } else {
                pixels[(row * 80 + col) as usize]
            };
        }
    }
    Image::from_pixels(80, 400, pixels).unwrap()
}

#[test]
fn button_at_the_bottom_stays_only_in_the_last_screen() {
    scroll_past((50, 140, 20, 20));
}

#[test]
fn tab_in_the_middle_of_the_side_stays_only_in_the_last_screen() {
    scroll_past((68, 70, 12, 40));
}

#[test]
fn plaque_right_above_a_sticky_footer_stays_only_in_the_last_screen() {
    let mut stitch = Stitcher::new(Config::default()).unwrap();
    let last = 400;
    for origin in (0..=last).step_by(26) {
        stitch.push(above_footer(origin)).unwrap();
    }
    let result = stitch.full_image().unwrap().unwrap();
    let height = result.height();
    let end = (0..=last).step_by(26).last().unwrap();
    assert_eq!(height, end + 400);
    // with a step of exactly one plaque the rows right above the last one are only ever
    // seen next to it, so a sliver of the previous plaque may stay in the last screen
    let page = frame(0, 80, end + 360);
    for row in 0..height - 66 - 26 {
        assert_eq!(result.row(row), page.row(row), "row {row}");
    }
}

#[test]
fn a_jump_far_from_the_last_step_is_found_to_the_pixel() {
    // noise fits at one offset only, every third offset of a coarse search misses it
    for jump in [200, 201, 202, 203] {
        let mut stitch = Stitcher::new(config()).unwrap();
        stitch.push(frame(0, 80, 400)).unwrap();
        let result = stitch.push(frame(jump, 80, 400)).unwrap();
        assert_eq!(
            (result.status, result.added),
            (Status::Appended, jump),
            "jump {jump}"
        );
        assert_eq!(
            stitch.full_image().unwrap().unwrap(),
            frame(0, 80, 400 + jump)
        );
    }
}
