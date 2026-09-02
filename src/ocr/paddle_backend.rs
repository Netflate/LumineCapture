// Runs PP-OCRv5 mobile models via ONNX Runtime using the `oar-ocr` crate.
//
// Highlights:
// - Uses temporary for now Cyrillic model that supports Cyrillic, English, digits, and punctuation.
// - Processes image crops one by one. Batching or multi-threading is avoided 
//   because it slows down execution and wastes memory cache.
// - Uses direct low-level API access to get exact character positions (`return_word_box`) 
//   for precise sub-line text selection.
// - Logs performance metrics (`ocr: timing`) to stderr. (temporary).

use std::time::Instant;

use oar_ocr::core::config::OrtSessionConfig;
use oar_ocr::core::traits::{AdapterBuilder, OrtConfigurable};
use oar_ocr::core::traits::task::ImageTaskInput;
use oar_ocr::domain::adapters::TextRecognitionAdapterBuilder;
use oar_ocr::domain::tasks::{TextDetectionConfig, TextRecognitionConfig, TextRecognitionTask};
use oar_ocr::predictors::{TaskPredictorCore, TextDetectionPredictor};
use oar_ocr::processors::{LimitType, Point};
use oar_ocr::utils::get_rotate_crop_image;
use tiny_skia::Rect;

use super::layout;
use super::{OcrBackend, OcrError, OcrImage, OcrLine, OcrText};

type Recognizer = TaskPredictorCore<TextRecognitionTask>;

// todo HARDCODE
static DETECTION_MODEL: &[u8] = include_bytes!("../../assets/models/pp-ocrv5_mobile_det.onnx");
static RECOGNITION_MODEL: &[u8] =
    include_bytes!("../../assets/models/cyrillic_pp-ocrv5_mobile_rec.onnx");
static RECOGNITION_DICT: &str = include_str!("../../assets/models/ppocrv5_cyrillic_dict.txt");

/// Longest image side the detector sees; larger inputs are downscaled first.
/// The stock default of 960 halves a 1080p grab and loses small UI text.
const DETECT_LIMIT_SIDE_LEN: u32 = 1920;
/// not so sure how its going to work on low res or high res monitors
/// (TODO)
pub struct PaddleBackend {
    detector: TextDetectionPredictor,
    recognizer: Recognizer,
}

impl PaddleBackend {
    pub fn new() -> Result<Self, OcrError> {
        let total = Instant::now();
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let ort = OrtSessionConfig::new().with_intra_threads(threads);

        let stage = Instant::now();
        let detector = TextDetectionPredictor::builder()
            .with_config(TextDetectionConfig {
                score_threshold: 0.3,
                box_threshold: 0.6,
                unclip_ratio: 1.5,
                max_candidates: 1000,
                limit_side_len: Some(DETECT_LIMIT_SIDE_LEN),
                limit_type: Some(LimitType::Max),
                max_side_len: Some(4096),
            })
            .with_ort_config(ort.clone())
            .build(DETECTION_MODEL)?;
        let detector_ms = ms(stage);

        let stage = Instant::now();
        let dict: Vec<String> = RECOGNITION_DICT.lines().map(str::to_owned).collect();
        let rec_config = TextRecognitionConfig {
            score_threshold: 0.0,
        };
        let rec_adapter = TextRecognitionAdapterBuilder::new()
            .with_config(rec_config.clone())
            .character_dict(dict)
            .return_word_box(true) // per-character x positions
            .with_ort_config(ort.clone())
            .build(RECOGNITION_MODEL)?;
        let recognizer = TaskPredictorCore::new(
            Box::new(rec_adapter),
            TextRecognitionTask::new(rec_config.clone()),
            rec_config,
        );
        let recognizer_ms = ms(stage);

        eprintln!(
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

impl OcrBackend for PaddleBackend {
    fn recognize(&self, image: OcrImage) -> Result<OcrText, OcrError> {
        let total = Instant::now();

        let stage = Instant::now();
        let (width, height, (ox, oy)) = (image.width, image.height, image.origin);
        // By value, so the full-screen buffer isn't copied.
        let img = image::RgbImage::from_raw(width, height, image.rgb)
            .ok_or("ocr: RGB buffer does not match its dimensions")?;
        let decode_ms = ms(stage);

        let stage = Instant::now();
        // `predict` takes ownership and the adapter unwraps it, so this clone
        // is unavoidable: the pixels are needed again to cut the line crops.
        let detected = self.detector.predict(vec![img.clone()])?;
        let detect_ms = ms(stage);
        let Some(regions) = detected.detections.into_iter().next() else {
            return Ok(OcrText::default());
        };

        // Boxes in image coordinates, keeping each quad for the rotated crop.
        let stage = Instant::now();
        let mut quads: Vec<Vec<Point>> = Vec::new();
        let mut boxes: Vec<Rect> = Vec::new();
        for region in regions {
            let pts = region.bbox.points;
            if pts.len() != 4 {
                continue;
            }
            let (mut l, mut t, mut r, mut b) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for p in &pts {
                l = l.min(p.x);
                t = t.min(p.y);
                r = r.max(p.x);
                b = b.max(p.y);
            }
            let Some(rect) = Rect::from_ltrb(l, t, r, b) else {
                continue;
            };
            quads.push(pts);
            boxes.push(rect);
        }
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
        let rows = layout::row_groups(&keep.iter().map(|&i| boxes[i]).collect::<Vec<_>>());
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
        let reads: Vec<Read> = crops
            .iter()
            .map(|crop| recognize_one(&self.recognizer, crop))
            .collect();
        let recognize_ms = ms(stage);

        let stage = Instant::now();
        let mut lines: Vec<OcrLine> = rects
            .into_iter()
            .zip(reads)
            .filter(|(bounds, read)| !is_noise(bounds, read, body_height))
            .map(|(bounds, read)| {
                let char_x = char_boundaries(&bounds, &read.text, &read.positions);
                OcrLine {
                    text: read.text,
                    bounds,
                    char_x,
                }
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

        eprintln!(
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

/// Minimum confidence for a one-character read to be believed.
const SPECK_CONFIDENCE: f32 = 0.6;

/// Minimum confidence for any read to be believed.
const MIN_CONFIDENCE: f32 = 0.55;

/// A detection this much taller than the body text, and no wider than it is
/// tall, is an icon or a logo rather than a line of text.
const GLYPH_HEIGHT: f32 = 1.8;
const GLYPH_ASPECT: f32 = 1.2;

/// A detection far smaller than the body text in both directions, which is
/// what visual noise looks like. The recognizer cannot answer "not text" and
/// would return its best single-character guess for it.
fn is_speck(bounds: &Rect, body_height: f32) -> bool {
    if bounds.height() < 0.5 * body_height && bounds.width() < 0.5 * body_height {
        return true;
    }
    bounds.height() > GLYPH_HEIGHT * body_height
        && bounds.width() < GLYPH_ASPECT * bounds.height()
}

/// Whether a read should be dropped instead of becoming a line.
fn is_noise(bounds: &Rect, read: &Read, body_height: f32) -> bool {
    if read.text.trim().is_empty() || read.confidence < MIN_CONFIDENCE {
        return true;
    }
    read.text.chars().count() <= 1
        && (bounds.height() < 0.6 * body_height || read.confidence < SPECK_CONFIDENCE)
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
/// Handles proportional spacing so boundaries split gaps based on character widths
/// (exp an 'i' next to a 'W' won't take up half the space).
fn char_boundaries(bounds: &Rect, text: &str, norm: &[f32]) -> Vec<f32> {
    let widths: Vec<f32> = text.chars().map(advance).collect();
    let n = widths.len();
    let (left, right) = (bounds.left(), bounds.right());
    if n == 0 {
        return vec![left, right];
    }

    let at = |k: usize| left + bounds.width() * norm[k].clamp(0.0, 1.0);
    let span = if norm.len() == n { at(n - 1) - at(0) } else { 0.0 };
    if n == 1 || span <= 0.0 {
        return modelled(left, right, &widths);
    }

    // OCR positions measure center-to-center, leaving half of the first 
    // and half of the last character uncovered.
    let total: f32 = widths.iter().sum();
    let unit = span / (total - 0.5 * (widths[0] + widths[n - 1])).max(f32::EPSILON);

    let mut xs = Vec::with_capacity(n + 1);
    xs.push(at(0) - 0.5 * widths[0] * unit);
    for k in 1..n {
        let share = widths[k - 1] / (widths[k - 1] + widths[k]);
        xs.push(at(k - 1) + (at(k) - at(k - 1)) * share);
    }
    xs.push(at(n - 1) + 0.5 * widths[n - 1] * unit);

    // OCR positions can drift left on dense text. Re-center everything 
    // relative to the bounding box.
    let slide = 0.5 * (left + right - xs[0] - xs[n]);
    for x in &mut xs {
        *x += slide;
    }

    // Ensure narrow characters have a minimum width so boundaries don't overlap.
    const MIN_SHARE: f32 = 0.6;
    xs[0] = xs[0].max(left);
    for k in 0..n {
        xs[k + 1] = xs[k + 1].max(xs[k] + MIN_SHARE * widths[k] * unit);
    }

    // If expanding minimum widths pushed text past the right boundary, 
    // scale down proportionally to fit the box.
    let (start, grown) = (xs[0], xs[n] - xs[0]);
    if xs[n] > right && grown > 0.0 {
        let squeeze = (right - start) / grown;
        for x in &mut xs[1..] {
            *x = start + (*x - start) * squeeze;
        }
    }
    xs
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
        _ if c as u32 >= 0x2E80 => 2.0, 
        _ if c.is_uppercase() => 1.2,
        _ => 1.0,
    }
}