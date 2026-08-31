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
struct Read {
    text: String,
    /// Per-character position, normalized 0..1 across the crop width.
    positions: Vec<f32>,
    confidence: f32,
}

/// Milliseconds since `start`, for the timing line.
fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn recognize_one(recognizer: &Recognizer, crop: &image::RgbImage) -> Read {
    match recognizer.predict(ImageTaskInput::new(vec![crop.clone()])) {
        Ok(mut out) => Read {
            text: out.texts.drain(..).next().unwrap_or_default(),
            positions: out.char_positions.drain(..).next().unwrap_or_default(),
            confidence: out.scores.first().copied().unwrap_or(0.0),
        },
        Err(_) => Read {
            text: String::new(),
            positions: Vec::new(),
            confidence: 0.0,
        },
    }
}

/// Minimum confidence for a one-character read to be believed.
const SPECK_CONFIDENCE: f32 = 0.6;

/// A detection far smaller than the body text in both directions, which is
/// what visual noise looks like. The recognizer cannot answer "not text" and
/// would return its best single-character guess for it.
fn is_speck(bounds: &Rect, body_height: f32) -> bool {
    bounds.height() < 0.5 * body_height && bounds.width() < 0.5 * body_height
}

/// Whether a read should be dropped instead of becoming a line.
fn is_noise(bounds: &Rect, read: &Read, body_height: f32) -> bool {
    if read.text.trim().is_empty() {
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

// the X position of every character boundary, including the ends.
// Total values = character count + 1 (sorted left to right).
//
// `norm` holds normalized character positions (0.0 to 1.0).
// Boundaries inside the text are placed midway between neighboring characters.
fn char_boundaries(bounds: &Rect, text: &str, norm: &[f32]) -> Vec<f32> {
    let n = text.chars().count();
    let (left, width) = (bounds.left(), bounds.width());
    if n == 0 {
        return vec![left, bounds.right()];
    }
    let at = |k: usize| left + width * norm.get(k).copied().unwrap_or(1.0).clamp(0.0, 1.0);

    let mut xs = Vec::with_capacity(n + 1);
    xs.push(left);
    for k in 1..n {
        xs.push(0.5 * (at(k - 1) + at(k)));
    }
    xs.push(bounds.right());
    for i in 1..xs.len() {
        if xs[i] < xs[i - 1] {
            xs[i] = xs[i - 1];
        }
    }
    xs
}
