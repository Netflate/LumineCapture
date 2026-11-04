//! realistic stitching pages: flat areas, text, pictures, window chrome and
//! things floating over the page. The stitched image must hold the page and nothing that
//! floats over it, except in the screens at either end, which nothing else ever showed: there
//! it is as a frame with it, not pieced together from several.

mod common;

use common::{Screen, page_row, shot, smooth};
use shot_image::Image;
use shot_stitch::{Config, Status, Stitcher};

fn wrong_pixels(screen: Screen, positions: &[i32], image: &shot_image::Image) -> usize {
    let base = *positions.iter().min().unwrap();
    let (left, width) = (screen.page_columns().start, screen.page_columns().len());
    let edge = screen.height as i32;
    (edge..image.height() - edge)
        .map(|y| {
            let page_y = y - screen.top as i32 + base;
            let want = page_row(page_y, width);
            let got = &image.row(y)[left..];
            // a playing video is right in any of its frames
            (0..width)
                .filter(|&x| got[x] != want[x] && !screen.plays(page_y, x))
                .count()
        })
        .sum()
}
/// Ensures that static elements (like window chrome or tooltips) at the very top
/// and bottom of the final image are preserved from a single edge frame
/// as floating tooltips, since they are deleted from all frames
/// but has to be returned
fn wrong_ends(screen: Screen, positions: &[i32], image: &Image) -> usize {
    let base = *positions.iter().min().unwrap();
    let height = screen.height as i32;
    let page = screen.page_columns();
    let wrong = |rows: std::ops::Range<i32>| {
        // where the screen at that end was, by the image, which may fall a row or two short
        let at = base + rows.start;
        let frames: Vec<Image> = (0..positions.len())
            .map(|n| shot(screen, at, n as u32))
            .collect();
        let mut wrong = 0;
        for y in rows.clone() {
            let page_y = y - screen.top as i32 + base;
            let want = page_row(page_y, page.len());
            let got = image.row(y);
            wrong += (0..screen.width)
                .filter(|&x| x < screen.sidebar || page.contains(&x))
                .filter(|&x| {
                    let shown = if x < screen.sidebar {
                        got[x] == 0xff1e1e1e
                    } else {
                        let x = x - page.start;
                        got[page.start + x] == want[x] || screen.plays(page_y, x)
                    };
                    !shown && frames.iter().all(|f| f.row(y - rows.start)[x] != got[x])
                })
                .count();
        }
        wrong
    };
    let length = image.height();
    wrong(0..height.min(length)) + wrong((length - height).max(0)..length)
}

fn check(screen: Screen, positions: &[i32], tolerance: usize) {
    let mut stitch = Stitcher::new(Config::default()).unwrap();
    let mut lost = 0;
    for (n, &at) in positions.iter().enumerate() {
        let result = stitch.push(shot(screen, at, n as u32)).unwrap();
        lost += usize::from(result.status == Status::NoMatch);
    }
    let image = stitch.full_image().unwrap().unwrap();
    assert_eq!(lost, 0, "frames that found no place");
    let (min, max) = (
        positions.iter().min().unwrap(),
        positions.iter().max().unwrap(),
    );
    let expected = max - min + screen.height as i32;
    assert!(
        (expected - image.height()).abs() < Config::default().min_append,
        "height {} instead of {expected}",
        image.height()
    );
    let wrong = wrong_pixels(screen, positions, &image);
    assert!(wrong <= tolerance, "{wrong} wrong pixels");
    let ends = wrong_ends(screen, positions, &image);
    assert_eq!(ends, 0, "pixels at the ends pieced from several frames");
}

/// Tests a chat app where the entire screen is static except for the scrolling message column,
/// forcing the stitcher to rely solely on that narrow column to track movement.
#[test]
fn a_chat_app_on_the_whole_monitor() {
    for burst in [150, 300, 600, 900] {
        let down = smooth(4000, burst);
        let up: Vec<i32> = down.iter().map(|at| 4000 - at).collect();
        check(Screen::chat_app(), &down, 0);
        check(Screen::chat_app(), &up, 0);
    }
}

/// Flicks up: the page just shown is right under the sticky header, where the last frame had
/// the header. Taken for the page under a float, half the screen ends up under floats in a
/// few frames, and what is left to compare fits anywhere.
#[test]
fn fast_flicks_up_under_a_sticky_header() {
    for (screen, bursts) in [
        (Screen::monitor(), &[600, 900][..]),
        (Screen::region(), &[600, 900]),
        (Screen::busy(), &[600]),
    ] {
        for &burst in bursts {
            let up: Vec<i32> = smooth(4000, burst).iter().map(|at| 4000 - at).collect();
            check(screen, &up, 0);
        }
    }
}

#[test]
fn whole_monitor_with_chrome_sidebar_and_dock() {
    check(Screen::monitor(), &smooth(4000, 300), 0);
}

#[test]
fn region_with_a_dock_and_a_link_tooltip() {
    check(Screen::region(), &smooth(3000, 250), 0);
}

#[test]
fn translucent_plaque_and_button_in_the_middle() {
    for burst in [90, 250] {
        check(Screen::busy(), &smooth(3000, burst), 0);
    }
}

/// Flicks of 600 rows show every row in some six frames, with a translucent plaque, a dock, a
/// blinking tooltip and a button over them. A cell that no frame showed uncovered can keep a
/// covered view: under a uniform plaque the dimmed page moves along with the page, and only
/// its edges tell it apart.
#[test]
fn everything_at_once_with_fast_flicks() {
    check(Screen::busy(), &smooth(3000, 600), 64);
}

#[test]
fn scrolling_up_prepends_without_floats() {
    let positions: Vec<i32> = smooth(3000, 250).iter().map(|at| 3000 - at).collect();
    check(Screen::busy(), &positions, 0);
}

#[test]
fn back_and_forth() {
    let mut positions = smooth(2000, 250);
    positions.extend(smooth(1200, 200).iter().map(|at| 2000 - at));
    positions.extend(smooth(2500, 300).iter().map(|at| 800 + at));
    check(Screen::busy(), &positions, 0);
}

/// A playing video takes around a third of the screen
/// With floats over the page as well, some of them stay next to it
/// right next to a video they can't be told apart from a translucent plaque over the page.
#[test]
fn a_playing_video_in_the_page() {
    let screen = Screen {
        video: Some((150, 1100, 700, 320)),
        dock: None,
        tooltip: None,
        ..Screen::region()
    };
    check(screen, &smooth(3000, 250), 0);
}

/// A list that never scrolls, as tall as the screen, like the chats of a messenger: the page
/// beside it is stitched all the same.
#[test]
fn a_list_that_stays_put_beside_the_page() {
    let down = smooth(3000, 250);
    let up: Vec<i32> = down.iter().map(|at| 3000 - at).collect();
    for positions in [down, smooth(3000, 600), up] {
        check(Screen::messenger(), &positions, 0);
    }
}

fn timing(screen: Screen, end: i32) {
    let mut stitch = Stitcher::new(Config::default()).unwrap();
    let mut times = Vec::new();
    for (n, &at) in smooth(end, 300).iter().enumerate() {
        let frame = shot(screen, at, n as u32);
        let start = std::time::Instant::now();
        stitch.push(frame).unwrap();
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    println!(
        "{}x{}, {} px long: median {:.1} ms, p90 {:.1} ms, max {:.1} ms per frame",
        screen.width,
        screen.height,
        stitch.dimensions().1,
        times[times.len() / 2],
        times[times.len() * 9 / 10],
        times[times.len() - 1]
    );
}

#[test]
#[ignore = "timing, by hand: cargo test --release -p shot-stitch --test realistic -- --ignored --nocapture"]
fn timing_whole_monitor() {
    timing(Screen::monitor(), 4000);
}

#[test]
#[ignore = "timing, by hand: cargo test --release -p shot-stitch --test realistic -- --ignored --nocapture"]
fn timing_long_page() {
    timing(Screen::busy(), 40000);
}

/// Back to somewhere far up the page in one go: out of reach of the last frame, so the place
/// is looked for in all the stored rows, where a page that repeats itself fits in more than
/// one. Found exactly or not at all, more importantly never put in the wrong place;
/// and found as a rule, by the pixels where the signatures fit in several places.
#[test]
fn a_jump_far_back_lands_exactly_or_not_at_all() {
    let screen = Screen {
        dock: None,
        tooltip: None,
        ..Screen::region()
    };
    let positions = smooth(3000, 250);
    let mut found = 0;
    for back in [500, 1111, 1777, 2222] {
        let mut stitch = Stitcher::new(Config::default()).unwrap();
        for (n, &at) in positions.iter().enumerate() {
            stitch.push(shot(screen, at, n as u32)).unwrap();
        }
        let result = stitch.push(shot(screen, back, 0)).unwrap();
        match result.status {
            Status::NoMatch => {}
            Status::NoProgress => {
                assert_eq!(result.position, back, "jump to {back}");
                found += 1;
            }
            status => panic!("jump to {back}: {status:?} at {}", result.position),
        }
        let image = stitch.full_image().unwrap().unwrap();
        assert_eq!(
            wrong_pixels(screen, &positions, &image),
            0,
            "jump to {back}"
        );
    }
    assert!(found >= 3, "only {found} of 4 jumps found");
}
