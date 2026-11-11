// Runs PP-OCRv5 mobile models via ONNX Runtime using the `oar-ocr` crate.
//
// Highlights:
// - Uses the model picked in the language list (see `models`), e.g. Cyrillic supports Cyrillic, English, digits, and punctuation.
// - Processes image crops one by one. Batching or multi-threading is avoided
//   because it slows down execution and wastes memory cache.
// - Uses direct low-level API access to get exact character positions (`return_word_box`)
//   for precise sub-line text selection.
// - Logs performance metrics (`ocr: timing`) to stderr. (temporary).

use log::debug;
use std::time::Instant;

use oar_ocr::core::config::{OrtExecutionProvider, OrtSessionConfig};
use oar_ocr::core::traits::task::ImageTaskInput;
use oar_ocr::core::traits::{AdapterBuilder, OrtConfigurable};
use oar_ocr::domain::adapters::TextRecognitionAdapterBuilder;
use oar_ocr::domain::tasks::{TextDetectionConfig, TextRecognitionConfig, TextRecognitionTask};
use oar_ocr::predictors::{TaskPredictorCore, TextDetectionPredictor};
use oar_ocr::processors::{LimitType, Point};
use oar_ocr::utils::get_rotate_crop_image;
use tiny_skia::Rect;

use crate::config::OcrFilter;

use super::layout;
use super::lookalikes;
use super::models::ModelFiles;
use super::{OcrBackend, OcrError, OcrImage, OcrLine, OcrText};

type Recognizer = TaskPredictorCore<TextRecognitionTask>;

/// not so sure how its going to work on low res or high res monitors
/// (TODO)
pub struct PaddleBackend {
    detector: TextDetectionPredictor,
    recognizer: Recognizer,
}

impl PaddleBackend {
    pub fn new(files: &ModelFiles) -> Result<Self, OcrError> {
        Self::build(files, false)
    }

    pub fn on_gpu(files: &ModelFiles) -> Result<Self, OcrError> {
        Self::build(files, true)
    }

    fn build(files: &ModelFiles, gpu: bool) -> Result<Self, OcrError> {
        let total = Instant::now();
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let mut providers = Vec::new();
        if gpu {
            super::dawn::load()?;
            providers.push(OrtExecutionProvider::WebGPU);
        }
        // Explicitly configuring the CPU Execution Provider disables ORT's arena allocator.
        // With the arena allocator enabled, scanning a 1920x2160 frame retains +-1.1 GB resident memory indefinitely;
        // without it, memory drops to ~145 MB, at the cost of a ~5-10% latency penalty per scan.
        providers.push(OrtExecutionProvider::CPU);
        let mut ort = OrtSessionConfig::new()
            .with_intra_threads(threads)
            .with_execution_providers(providers)
            .add_config_entry("session.intra_op.allow_spinning", "0");
        if gpu {
            // WebGPU keeps buffers sized for the largest image
            // Without cache it stays under +-130 MB, with only a +-5% slowdown on scans.
            for kind in ["storage", "uniform", "queryResolve", "default"] {
                ort = ort.add_config_entry(
                    format!("ep.webgpuexecutionprovider.{kind}BufferCacheMode"),
                    "disabled",
                );
            }
        }

        let stage = Instant::now();
        let detect = &crate::config::get().ocr.detect;
        let detector = TextDetectionPredictor::builder()
            .with_config(TextDetectionConfig {
                score_threshold: detect.score_threshold,
                box_threshold: detect.box_threshold,
                unclip_ratio: detect.unclip_ratio,
                max_candidates: detect.max_candidates,
                // longest side the detector sees; the stock 960 halves a 1080p grab and loses small UI text
                limit_side_len: Some(detect.max_side),
                limit_type: Some(LimitType::Max),
                max_side_len: Some(4096),
            })
            .with_ort_config(ort.clone())
            .build(files.detector.as_path())?;
        let detector_ms = ms(stage);

        let stage = Instant::now();
        let dict: Vec<String> = std::fs::read_to_string(&files.dict)?
            .lines()
            .map(str::to_owned)
            .collect();
        let rec_config = TextRecognitionConfig {
            score_threshold: 0.0,
        };
        let rec_adapter = TextRecognitionAdapterBuilder::new()
            .with_config(rec_config.clone())
            .character_dict(dict)
            .return_word_box(true) // per-character x positions
            .with_ort_config(ort.clone())
            .build(files.recognizer.as_path())?;
        let recognizer = TaskPredictorCore::new(
            Box::new(rec_adapter),
            TextRecognitionTask::new(rec_config.clone()),
            rec_config,
        );
        let recognizer_ms = ms(stage);

        debug!(
            "ocr: engine ready in {:.0}ms  (detector {:.0} | recognizer {:.0})",
            ms(total),
            detector_ms,
            recognizer_ms,
        );

        Ok(Self {
            detector,
            recognizer,
        })
    }
}

impl PaddleBackend {
    fn detect(
        &self,
        img: &image::RgbImage,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<Vec<Point>>, OcrError> {
        let (w, h) = img.dimensions();
        let (wide, plan) = plan_strips(w, h, crate::config::get().ocr.detect.max_side);
        if let [_] = plan[..] {
            let detected = self.detector.predict(vec![img.clone()])?;
            if cancelled() {
                return Err(super::CANCELLED.into());
            }
            let regions = detected.detections.into_iter().next().unwrap_or_default();
            return Ok(regions.into_iter().map(|r| r.bbox.points).collect());
        }

        let (max_x, max_y) = (w.saturating_sub(1) as f32, h.saturating_sub(1) as f32);
        let mut quads = Vec::new();
        for strip in plan {
            let (x, y, sw, sh) = if wide {
                (strip.start, 0, strip.end - strip.start, h)
            } else {
                (0, strip.start, w, strip.end - strip.start)
            };
            let piece = image::imageops::crop_imm(img, x, y, sw, sh).to_image();
            let (piece, (left, top)) = pad_for_detection(piece);
            let detected = self.detector.predict(vec![piece])?;
            if cancelled() {
                return Err(super::CANCELLED.into());
            }
            let regions = detected.detections.into_iter().next().unwrap_or_default();
            let (dx, dy) = (x as f32 - left as f32, y as f32 - top as f32);
            for region in regions {
                let pts: Vec<Point> = region
                    .bbox
                    .points
                    .iter()
                    .map(|p| Point::new((p.x + dx).clamp(0.0, max_x), (p.y + dy).clamp(0.0, max_y)))
                    .collect();
                let Some(bounds) = quad_bounds(&pts) else {
                    continue;
                };
                let center = if wide {
                    bounds.left() + bounds.width() / 2.0
                } else {
                    bounds.top() + bounds.height() / 2.0
                };
                if center >= strip.own_start as f32 && center < strip.own_end as f32 {
                    quads.push(pts);
                }
            }
        }
        Ok(quads)
    }
}

/// what neighbouring strips share, so a line cut by one's edge is whole in the other
const STRIP_OVERLAP: u32 = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Strip {
    start: u32,
    end: u32,
    own_start: u32,
    own_end: u32,
}

/// up to twice `max_side` the detector's own shrinking is kept, one strip
fn plan_strips(w: u32, h: u32, max_side: u32) -> (bool, Vec<Strip>) {
    let wide = w > h;
    let (long, short) = if wide { (w, h) } else { (h, w) };
    if long <= max_side.saturating_mul(2) {
        return (wide, strips(long, long, STRIP_OVERLAP));
    }
    (wide, strips(long, max_side.max(short), STRIP_OVERLAP))
}

fn strips(long: u32, limit: u32, overlap: u32) -> Vec<Strip> {
    let limit = limit.max(1);
    if long <= limit {
        return vec![Strip {
            start: 0,
            end: long,
            own_start: 0,
            own_end: long,
        }];
    }
    let step = limit - overlap.min(limit / 2);
    let mut starts = vec![0u32];
    while let Some(&last) = starts.last()
        && last + limit < long
    {
        starts.push((last + step).min(long - limit));
    }
    let mut out: Vec<Strip> = starts
        .iter()
        .map(|&start| Strip {
            start,
            end: start + limit,
            own_start: 0,
            own_end: long,
        })
        .collect();
    for i in 1..out.len() {
        let cut = (out[i].start + out[i - 1].end) / 2;
        out[i - 1].own_end = cut;
        out[i].own_start = cut;
    }
    out
}

fn quad_bounds(pts: &[Point]) -> Option<Rect> {
    if pts.len() != 4 {
        return None;
    }
    let (mut l, mut t, mut r, mut b) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in pts {
        l = l.min(p.x);
        t = t.min(p.y);
        r = r.max(p.x);
        b = b.max(p.y);
    }
    Rect::from_ltrb(l, t, r, b)
}

impl OcrBackend for PaddleBackend {
    fn recognize(
        &self,
        image: OcrImage,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<OcrText, OcrError> {
        let total = Instant::now();

        let stage = Instant::now();
        let (width, height, (ox, oy)) = (image.width, image.height, image.origin);
        // By value, so the full-screen buffer isn't copied.
        let img = image::RgbImage::from_raw(width, height, image.rgb)
            .ok_or("ocr: RGB buffer does not match its dimensions")?;
        let area = Rect::from_xywh(ox, oy, width as f32, height as f32);
        // adding pad for image otherwise on small images where text is close to border
        // some text will be missing, if not all
        let (img, (left, top)) = pad_for_detection(img);
        // real positinos of text, since image could be changed
        let (ox, oy) = (ox - left as f32, oy - top as f32);
        let decode_ms = ms(stage);

        let stage = Instant::now();
        // `predict` takes ownership and the adapter unwraps it, so this clone
        // is unavoidable: the pixels are needed again to cut the line crops.
        let quads = self.detect(&img, cancelled)?;
        let detect_ms = ms(stage);

        // Boxes in image coordinates, keeping each quad for the rotated crop.
        let stage = Instant::now();
        let mut boxes: Vec<Rect> = Vec::new();
        let quads: Vec<Vec<Point>> = quads
            .into_iter()
            .filter_map(|pts| {
                boxes.push(quad_bounds(&pts)?);
                Some(pts)
            })
            .collect();
        if boxes.is_empty() {
            return Ok(OcrText::default());
        }

        // Specks of window chrome come back as detections. Dropping them here,
        // by size alone, keeps them out of the merged crops below.
        let body_height = median_height(&boxes);
        let keep: Vec<usize> = (0..boxes.len())
            .filter(|&i| !is_speck(&boxes[i], body_height))
            .collect();

        // One crop per visual line rather than per detection box: the detector
        // splits a line wherever the spacing widens, and each extra call costs
        // a fixed ~9ms on top of the per-pixel work.
        let rows = layout::row_groups(
            &keep.iter().map(|&i| boxes[i]).collect::<Vec<_>>(),
            |a, b| has_rule(&img, a, b),
        );
        let mut rects: Vec<Rect> = Vec::with_capacity(rows.len());
        let mut crops: Vec<image::RgbImage> = Vec::with_capacity(rows.len());
        for row in &rows {
            let members: Vec<usize> = row.iter().map(|&r| keep[r]).collect();
            let crop = if let [only] = members[..] {
                // A lone box may be skewed, so go through the quad.
                match get_rotate_crop_image(&img, &quads[only]) {
                    Ok(crop) => crop,
                    Err(_) => continue,
                }
            } else {
                match axis_aligned_crop(&img, &union_of(&members, &boxes)) {
                    Some(crop) => crop,
                    None => continue,
                }
            };
            let merged = union_of(&members, &boxes);
            let Some(global) = Rect::from_ltrb(
                merged.left() + ox,
                merged.top() + oy,
                merged.right() + ox,
                merged.bottom() + oy,
            ) else {
                continue;
            };
            rects.push(global);
            crops.push(crop);
        }
        let crop_ms = ms(stage);
        if crops.is_empty() {
            return Ok(OcrText::default());
        }

        let stage = Instant::now();
        let mut reads: Vec<Read> = Vec::with_capacity(crops.len());
        for crop in &crops {
            if cancelled() {
                return Err(super::CANCELLED.into());
            }
            reads.push(recognize_one(&self.recognizer, crop));
        }
        let recognize_ms = ms(stage);

        let stage = Instant::now();
        let mut lines: Vec<OcrLine> = rects
            .into_iter()
            .zip(reads)
            .filter(|(bounds, read)| !is_noise(bounds, read, body_height))
            .map(|(bounds, read)| {
                let text = if crate::config::get().ocr.lookalikes {
                    lookalikes::fix(&read.text)
                } else {
                    read.text
                };
                let char_x = char_boundaries(&bounds, &text, &read.positions);
                OcrLine {
                    text,
                    bounds,
                    char_x,
                }
            })
            // recognizer's positions are fractions of the unclipped crop
            // this is why its on purpose after char_boundaries
            .filter_map(|line| match area {
                Some(area) => clip_line(line, area),
                None => Some(line),
            })
            .collect();
        lines.sort_by(|a, b| {
            a.bounds
                .top()
                .partial_cmp(&b.bounds.top())
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(
                    a.bounds
                        .left()
                        .partial_cmp(&b.bounds.left())
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
        });
        let sort_ms = ms(stage);

        debug!(
            "ocr: timing {}x{}px -> {} lines in {:.0}ms  (decode {:.0} | detect {:.0} | \
             crop {:.0} [{} boxes -> {} crops] | recognize {:.0} | sort {:.1})",
            width,
            height,
            lines.len(),
            ms(total),
            decode_ms,
            detect_ms,
            crop_ms,
            boxes.len(),
            crops.len(),
            recognize_ms,
            sort_ms,
        );

        Ok(OcrText { lines })
    }
}

/// adds padding around text
/// since oar-ocr rounds each side to the nearest multiple of 32 on its own, so
/// small crops get squashed or stretched, causing the detector to lose text closer
/// to edge.
///
/// Big regions are skipped
fn pad_for_detection(img: image::RgbImage) -> (image::RgbImage, (u32, u32)) {
    // not sure if it needs to be configurable
    // but i will leave it as it is at least for testing (with or without pad (pad=0))
    let detect = &crate::config::get().ocr.detect;
    let (w, h) = img.dimensions();
    let grow = |n: u32| (u64::from(n) + 2 * u64::from(detect.pad)).div_ceil(32) * 32;
    let (pw, ph) = (grow(w), grow(h));
    if detect.pad == 0 || w == 0 || h == 0 || pw.max(ph) > u64::from(detect.max_side) {
        return (img, (0, 0));
    }
    let (pw, ph) = (pw as u32, ph as u32);
    let (left, top) = ((pw - w) / 2, (ph - h) / 2);
    let mut out = image::RgbImage::from_pixel(pw, ph, border_median(&img));
    image::imageops::replace(&mut out, &img, left.into(), top.into());
    (out, (left, top))
}

/// Practically a copy of oar-ocr's `calculate_smart_padding_color`,
/// but without its +-16 away from black and white
fn border_median(img: &image::RgbImage) -> image::Rgb<u8> {
    let (w, h) = img.dimensions();
    let ring = (0..w)
        .flat_map(|x| [(x, 0), (x, h - 1)])
        .chain((1..h.saturating_sub(1)).flat_map(|y| [(0, y), (w - 1, y)]));
    let mut channels: [Vec<u8>; 3] = Default::default();
    for (x, y) in ring {
        for (values, &c) in channels.iter_mut().zip(&img.get_pixel(x, y).0) {
            values.push(c);
        }
    }
    image::Rgb(channels.map(|mut values| {
        let mid = values.len() / 2;
        *values.select_nth_unstable(mid).1
    }))
}

/// Detector boxes are wider than the text, so near the selection's edge
/// they stick out into the padding. Cut them back to the selection.
///
/// A line with nothing left inside is dropped
fn clip_line(mut line: OcrLine, area: Rect) -> Option<OcrLine> {
    line.bounds = line
        .bounds
        .intersect(&area)
        .filter(|b| b.width() > 0.0 && b.height() > 0.0)?;
    for x in &mut line.char_x {
        *x = x.clamp(line.bounds.left(), line.bounds.right());
    }
    Some(line)
}

/// One crop's recognition result.
#[derive(Default)]
struct Read {
    text: String,
    /// Per-character position, 0..1 across the crop's own width.
    positions: Vec<f32>,
    confidence: f32,
}

/// Milliseconds since `start`, for the timing line.
fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn recognize_one(recognizer: &Recognizer, crop: &image::RgbImage) -> Read {
    let Ok(mut out) = recognizer.predict(ImageTaskInput::new(vec![crop.clone()])) else {
        return Read::default();
    };
    let steps = out.sequence_lengths.first().copied().unwrap_or(0);
    let positions = out.char_positions.drain(..).next().unwrap_or_default();
    Read {
        text: out.texts.drain(..).next().unwrap_or_default(),
        positions: to_crop_space(positions, steps, crop.width(), crop.height()),
        confidence: out.scores.first().copied().unwrap_or(0.0),
    }
}

// Adjusts character positions returned by the OCR model.
//
// recognizer scales image crops to 48px high and pads them to a fixed width.
// Because of this extra padding, character coordinates can get squished
// to the left, especially on short lines.
const REC_HEIGHT: f32 = 48.0;
const REC_WIDTH: f32 = 320.0;
const REC_MAX_WIDTH: f32 = 3200.0;

/// Padded-tensor positions -> fractions of the crop itself.
fn to_crop_space(mut positions: Vec<f32>, steps: usize, w: u32, h: u32) -> Vec<f32> {
    let ratio = w as f32 / h.max(1) as f32;
    let tensor_w = (REC_HEIGHT * ratio.max(REC_WIDTH / REC_HEIGHT))
        .floor()
        .min(REC_MAX_WIDTH);
    let content_w = (REC_HEIGHT * ratio).ceil().min(tensor_w).max(1.0);

    let half_step = if steps > 0 { 0.5 / steps as f32 } else { 0.0 };
    for p in &mut positions {
        *p = ((*p + half_step) * tensor_w / content_w).clamp(0.0, 1.0);
    }
    positions
}

fn filter() -> &'static OcrFilter {
    &crate::config::get().ocr.filter
}

/// A detection far smaller than the body text in both directions, which is
/// what visual noise looks like. The recognizer cannot answer "not text" and
/// would return its best single-character guess for it.
fn is_speck(bounds: &Rect, body_height: f32) -> bool {
    let f = filter();
    if bounds.height() < f.speck_size * body_height && bounds.width() < f.speck_size * body_height {
        return true;
    }
    // A detection this much taller than the body text, and no wider than it is
    // tall, is an icon or a logo rather than a line of text.
    bounds.height() > f.glyph_height * body_height
        && bounds.width() < f.glyph_aspect * bounds.height()
}

/// Whether a read should be dropped instead of becoming a line.
fn is_noise(bounds: &Rect, read: &Read, body_height: f32) -> bool {
    let f = filter();
    if read.text.trim().is_empty() || read.confidence < f.min_confidence {
        return true;
    }
    read.text.chars().count() <= 1
        && (bounds.height() < f.single_char_height * body_height
            || read.confidence < f.single_char_confidence)
}

// Checks for a vertical ruling line in the gap between two boxes on the same line.
// Separated boxes are split into distinct crops for OCR processing.
//
// rule_contrast: minimum brightness difference between a line pixel and the background.
// rule_coverage: minimum fraction of a segment that must be such pixels.
// rule_reach: vertical search past a text line, as a fraction of its height.
// Cell borders usually extend beyond text boundaries, unlike pipe characters (`|`).
fn has_rule(img: &image::RgbImage, a: &Rect, b: &Rect) -> bool {
    let f = filter();
    let (top, bottom) = (a.top().min(b.top()), a.bottom().max(b.bottom()));
    let reach = (bottom - top) * f.rule_reach;
    let clamp_y = |y: f32| (y.max(0.0) as u32).min(img.height());
    let x0 = a.right().ceil().max(0.0) as u32;
    let x1 = (b.left().floor().max(0.0) as u32).min(img.width());
    let (above, y0, y1, below) = (
        clamp_y(top - reach),
        clamp_y(top),
        clamp_y(bottom),
        clamp_y(bottom + reach),
    );
    if x1 <= x0 || y1 <= y0 {
        return false;
    }

    let luma = |x: u32, y: u32| {
        let [r, g, b] = img.get_pixel(x, y).0;
        (i32::from(r) * 299 + i32::from(g) * 587 + i32::from(b) * 114) / 1000
    };
    let mut values: Vec<i32> = (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .map(|(x, y)| luma(x, y))
        .collect();
    let mid = values.len() / 2;
    let background = *values.select_nth_unstable(mid).1;

    let covered = |x: u32, from: u32, to: u32| {
        let needed = ((to - from) as f32 * f.rule_coverage).ceil() as usize;
        to > from
            && (from..to)
                .filter(|&y| (luma(x, y) - background).abs() > f.rule_contrast)
                .count()
                >= needed
    };
    (x0..x1).any(|x| covered(x, y0, y1) && (covered(x, above, y0) || covered(x, y1, below)))
}

/// Smallest rectangle covering the listed boxes.
fn union_of(members: &[usize], boxes: &[Rect]) -> Rect {
    let (mut l, mut t, mut r, mut b) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for &i in members {
        l = l.min(boxes[i].left());
        t = t.min(boxes[i].top());
        r = r.max(boxes[i].right());
        b = b.max(boxes[i].bottom());
    }
    Rect::from_ltrb(l, t, r, b).unwrap_or(boxes[members[0]])
}

/// Cut `rect` out of `img`, clamped to the image.
fn axis_aligned_crop(img: &image::RgbImage, rect: &Rect) -> Option<image::RgbImage> {
    let x = rect.left().floor().max(0.0) as u32;
    let y = rect.top().floor().max(0.0) as u32;
    let right = (rect.right().ceil() as u32).min(img.width());
    let bottom = (rect.bottom().ceil() as u32).min(img.height());
    if right <= x || bottom <= y {
        return None;
    }
    Some(image::imageops::crop_imm(img, x, y, right - x, bottom - y).to_image())
}

/// Median detection-box height, used as the body text size.
fn median_height(rects: &[Rect]) -> f32 {
    if rects.is_empty() {
        return 1.0;
    }
    let mut heights: Vec<f32> = rects.iter().map(|r| r.height()).collect();
    heights.sort_by(f32::total_cmp);
    heights[heights.len() / 2].max(1.0)
}
/// Returns X coordinates for character boundaries (character count + 1 values),
/// sorted left to right within `bounds`.
///
/// Handles proportional spacing: boundaries follow both the recognizer's positions and
/// character widths (exp an 'i' next to a 'W' won't take up half the space).
fn char_boundaries(bounds: &Rect, text: &str, norm: &[f32]) -> Vec<f32> {
    let widths: Vec<f32> = text.chars().map(advance).collect();
    let n = widths.len();
    let (left, right) = (bounds.left(), bounds.right());
    if n == 0 {
        return vec![left, right];
    }

    let at = |k: usize| left + bounds.width() * norm[k].clamp(0.0, 1.0);
    let total: f32 = widths.iter().sum();
    let span = if norm.len() == n && n > 1 {
        at(n - 1) - at(0)
    } else {
        0.0
    };
    if span <= 0.0 || total <= 0.0 {
        return modelled(left, right, &widths);
    }
    let unit = span / (total - 0.5 * (widths[0] + widths[n - 1])).max(f32::EPSILON);

    let mut system = Tridiagonal::new(n + 1);
    for (k, &w) in widths.iter().enumerate() {
        if w == 0.0 {
            system.add(k, (-1.0, 1.0), 0.0, MARK_WEIGHT);
            continue;
        }
        system.add(k, (0.5, 0.5), at(k), 1.0);
        system.add(k, (-1.0, 1.0), w * unit, WIDTH_WEIGHT);
    }

    let mut xs = system.solve();
    for x in &mut xs {
        *x = x.clamp(left, right);
    }
    // Now line edges are anchored to the box edge, simple as that,
    // instead of calculating them from character positions, since the
    // position of the first letter in large text often broke due to delay
    xs[0] = left;
    xs[n] = right;
    for k in 0..n {
        xs[k + 1] = xs[k + 1].max(xs[k]);
    }
    xs
}
/// Relative weight of estimated character width compared to the recognizer's position output.
const WIDTH_WEIGHT: f32 = 1.0;

/// High constraint weight enforcing near-zero width for combining diacritical marks.
const MARK_WEIGHT: f32 = 50.0;

// Tridiagonal normal equations where each constraint links two adjacent character boundaries.
struct Tridiagonal {
    diag: Vec<f32>,
    off: Vec<f32>,
    rhs: Vec<f32>,
}

impl Tridiagonal {
    fn new(len: usize) -> Self {
        Self {
            diag: vec![0.0; len],
            off: vec![0.0; len.saturating_sub(1)],
            rhs: vec![0.0; len],
        }
    }

    fn add(&mut self, k: usize, (u, v): (f32, f32), target: f32, weight: f32) {
        self.diag[k] += weight * u * u;
        self.diag[k + 1] += weight * v * v;
        self.off[k] += weight * u * v;
        self.rhs[k] += weight * u * target;
        self.rhs[k + 1] += weight * v * target;
    }

    fn solve(self) -> Vec<f32> {
        let len = self.diag.len();
        let mut c = vec![0.0f32; len];
        let mut d = vec![0.0f32; len];
        for i in 0..len {
            let (sub, cp, dp) = if i > 0 {
                (self.off[i - 1], c[i - 1], d[i - 1])
            } else {
                (0.0, 0.0, 0.0)
            };
            let denom = (self.diag[i] - sub * cp).max(1e-9);
            c[i] = self.off.get(i).map_or(0.0, |off| off / denom);
            d[i] = (self.rhs[i] - sub * dp) / denom;
        }
        let mut xs = vec![0.0f32; len];
        for i in (0..len).rev() {
            let next = if i + 1 < len { c[i] * xs[i + 1] } else { 0.0 };
            xs[i] = d[i] - next;
        }
        xs
    }
}

/// Fallback positioning based solely on estimated character widths.
/// Used for single characters or when OCR positions are missing.
fn modelled(left: f32, right: f32, widths: &[f32]) -> Vec<f32> {
    let unit = (right - left) / widths.iter().sum::<f32>().max(f32::EPSILON);
    let mut xs = Vec::with_capacity(widths.len() + 1);
    let mut x = left;
    xs.push(left);
    for w in widths {
        x += w * unit;
        xs.push(x);
    }
    *xs.last_mut().unwrap() = right;
    xs
}

/// Returns estimated character width relative to a standard lowercase letter.
fn advance(c: char) -> f32 {
    match c {
        'i' | 'j' | 'l' | 'I' | '|' | '.' | ',' | ':' | ';' | '!' | '\'' | '`' => 0.4,
        ' ' | 'f' | 'r' | 't' | '(' | ')' | '[' | ']' | '{' | '}' | '-' | '"' | '/' | '\\' => 0.6,
        'm' | 'w' | 'M' | 'W' | 'ш' | 'щ' | 'ж' | 'ы' | 'ю' | 'ф' => 1.5,
        'Ш' | 'Щ' | 'Ж' | 'Ы' | 'Ю' | 'Ф' | '—' | '№' => 1.6,
        _ if super::is_mark(c) => 0.0,
        _ if c as u32 >= 0x2E80 => 2.0,
        _ if c.is_uppercase() => 1.2,
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outer_boundaries_sit_on_the_box_edges() {
        let bounds = Rect::from_ltrb(100.0, 0.0, 610.0, 115.0).unwrap();
        let xs = char_boundaries(&bounds, "MONDAY", &[0.2, 0.35, 0.45, 0.6, 0.75, 0.9]);
        assert_eq!(xs.len(), 7);
        assert_eq!((xs[0], xs[6]), (100.0, 610.0));
        assert!(xs.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn small_images_are_padded_to_multiples_of_32() {
        let mut img = image::RgbImage::from_pixel(100, 20, image::Rgb([200, 210, 220]));
        // this pixel is used to check the image lands at (left, top) unchanged
        // since even with pads change it stay the same
        img.put_pixel(50, 10, image::Rgb([0, 0, 0]));
        let (out, (left, top)) = pad_for_detection(img);
        assert_eq!((out.width() % 32, out.height() % 32), (0, 0));
        assert!(left >= 16 && top >= 16);
        assert_eq!(out.get_pixel(left + 50, top + 10).0, [0, 0, 0]);
        assert_eq!(out.get_pixel(0, 0).0, [200, 210, 220]);
    }

    #[test]
    fn images_near_max_side_are_not_padded() {
        let (out, offset) = pad_for_detection(image::RgbImage::new(1900, 100));
        assert_eq!((out.dimensions(), offset), ((1900, 100), (0, 0)));
    }

    fn check_strips(long: u32, limit: u32, overlap: u32) -> Vec<Strip> {
        let plan = strips(long, limit, overlap);
        assert_eq!((plan[0].start, plan[0].own_start), (0, 0));
        assert_eq!(
            (plan.last().unwrap().end, plan.last().unwrap().own_end),
            (long, long)
        );
        for s in &plan {
            assert!(s.end - s.start <= limit);
            assert!(s.start <= s.own_start && s.own_start < s.own_end && s.own_end <= s.end);
        }
        for pair in plan.windows(2) {
            assert_eq!(pair[0].own_end, pair[1].own_start);
            assert!(pair[1].start < pair[0].end);
        }
        plan
    }

    #[test]
    fn short_pictures_get_one_strip() {
        let plan = check_strips(1920, 1920, 256);
        assert_eq!(plan.len(), 1);
        assert_eq!((plan[0].start, plan[0].end), (0, 1920));
        assert_eq!(check_strips(500, 1920, 256).len(), 1);
    }

    #[test]
    fn strips_cover_and_partition_the_axis() {
        for (long, limit) in [
            (1921, 1920),
            (2560, 1920),
            (6000, 1920),
            (6000, 1000),
            (12160, 1920),
            (3841, 2160),
            (10, 4),
            (7, 2),
        ] {
            let plan = check_strips(long, limit, 256);
            assert!(plan.len() > 1);
            assert!(plan.iter().all(|s| s.end - s.start == limit));
            let mut owned = vec![0u32; long as usize];
            for s in &plan {
                for i in s.own_start..s.own_end {
                    owned[i as usize] += 1;
                }
            }
            assert!(owned.iter().all(|&n| n == 1), "{long} by {limit}");
        }
    }

    #[test]
    fn owned_parts_split_overlaps_in_the_middle() {
        let plan = strips(2560, 1920, 256);
        assert_eq!(plan.len(), 2);
        assert_eq!((plan[1].start, plan[0].end), (640, 1920));
        assert_eq!(plan[0].own_end, 1280);
    }

    #[test]
    fn lines_near_a_cut_sit_whole_in_the_owner() {
        let plan = strips(6000, 1920, 256);
        for pair in plan.windows(2) {
            let cut = pair[0].own_end;
            assert!(cut - pair[1].start >= 128 && pair[0].end - cut >= 128);
        }
    }

    #[test]
    fn strips_follow_the_long_side() {
        let (wide, plan) = plan_strips(1000, 6000, 1920);
        assert!(!wide);
        assert_eq!(plan.last().unwrap().end, 6000);
        let (wide, plan) = plan_strips(6000, 1000, 1920);
        assert!(wide);
        assert_eq!(plan.last().unwrap().end, 6000);
        let (_, plan) = plan_strips(3840, 9000, 1920);
        assert!(plan.iter().all(|s| s.end - s.start == 3840));
    }

    #[test]
    fn up_to_twice_max_side_is_one_pass() {
        for (w, h) in [
            (1920, 1080),
            (1080, 1920),
            (2560, 1440),
            (3840, 2160),
            (1040, 3040),
            (3840, 3840),
        ] {
            assert_eq!(plan_strips(w, h, 1920).1.len(), 1, "{w}x{h}");
        }
        for (w, h) in [(5120, 1440), (1040, 6080), (1920, 4320), (1040, 3841)] {
            assert!(plan_strips(w, h, 1920).1.len() > 1, "{w}x{h}");
        }
    }

    #[test]
    fn quad_bounds_wants_four_points() {
        let p = |x, y| Point::new(x, y);
        let quad = [p(1.0, 2.0), p(9.0, 2.0), p(9.0, 5.0), p(1.0, 5.0)];
        assert_eq!(quad_bounds(&quad), Rect::from_ltrb(1.0, 2.0, 9.0, 5.0));
        assert!(quad_bounds(&quad[..3]).is_none());
    }

    #[test]
    fn lines_are_cut_to_the_scanned_area() {
        let line = OcrLine {
            text: "ab".into(),
            bounds: Rect::from_ltrb(-3.0, 5.0, 40.0, 30.0).unwrap(),
            char_x: vec![-3.0, 20.0, 40.0],
        };
        let area = Rect::from_ltrb(0.0, 0.0, 30.0, 25.0).unwrap();
        let cut = clip_line(line.clone(), area).unwrap();
        assert_eq!(cut.bounds, Rect::from_ltrb(0.0, 5.0, 30.0, 25.0).unwrap());
        assert_eq!(cut.char_x, vec![0.0, 20.0, 30.0]);
        let beside = Rect::from_ltrb(50.0, 0.0, 60.0, 10.0).unwrap();
        let touching = Rect::from_ltrb(0.0, 0.0, 30.0, 5.0).unwrap();
        assert!(clip_line(line.clone(), beside).is_none());
        assert!(clip_line(line, touching).is_none());
    }
}
