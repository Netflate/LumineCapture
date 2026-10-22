use crate::scroll::scrub;

const ARTIFACT: u32 = 0xff2d_d4bf;

fn canvas(width: usize, height: usize) -> Vec<u32> {
    (0..height)
        .flat_map(|y| {
            (0..width).map(move |x| 0xff00_0000 | ((y as u32 & 0xff) << 8) | (x as u32 & 0xff))
        })
        .collect()
}

#[test]
fn plain_image_is_left_untouched() {
    let (w, h) = (40, 40);
    let original = canvas(w, h);
    let mut pixels = original.clone();
    assert_eq!(scrub::apply(&mut pixels, w as i32, h as i32), 0);
    assert_eq!(pixels, original, "clean image must not change");
}

#[test]
fn top_artifact_row_is_replaced_by_content_below() {
    let (w, h) = (40, 40);
    let mut pixels = canvas(w, h);
    for y in 0..2 {
        for x in 0..w {
            pixels[y * w + x] = ARTIFACT;
        }
    }

    let expected = pixels[10 * w..11 * w].to_vec();
    assert_eq!(scrub::apply(&mut pixels, w as i32, h as i32), 1);
    assert_eq!(pixels[0..w], expected[..], "top row not replaced");
    assert_eq!(pixels[w..2 * w], expected[..], "second row not replaced");
}

#[test]
fn left_artifact_column_is_replaced_by_content_to_the_right() {
    let (w, h) = (40, 40);
    let mut pixels = canvas(w, h);
    for x in 0..2 {
        for y in 0..h {
            pixels[y * w + x] = ARTIFACT;
        }
    }

    let expected: Vec<u32> = (0..h).map(|y| pixels[y * w + 10]).collect();
    assert_eq!(scrub::apply(&mut pixels, w as i32, h as i32), 1);
    for (y, want) in expected.iter().enumerate() {
        assert_eq!(pixels[y * w], *want, "left column not replaced at y={y}");
        assert_eq!(
            pixels[y * w + 1],
            *want,
            "second column not replaced at y={y}"
        );
    }
}

#[test]
fn a_single_colored_row_is_not_treated_as_an_artifact() {
    let (w, h) = (40, 40);
    let mut pixels = canvas(w, h);
    for pixel in pixels.iter_mut().take(5) {
        *pixel = ARTIFACT;
    }
    let original = pixels.clone();
    assert_eq!(scrub::apply(&mut pixels, w as i32, h as i32), 0);
    assert_eq!(pixels, original);
}

#[test]
fn degenerate_sizes_do_not_panic() {
    let mut empty: Vec<u32> = Vec::new();
    assert_eq!(scrub::apply(&mut empty, 0, 0), 0);

    let mut single = vec![ARTIFACT];
    assert_eq!(scrub::apply(&mut single, 1, 1), 0);
}
