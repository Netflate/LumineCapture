use image::{ExtendedColorType, ImageEncoder};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::error::Error;

const WEBP_MAX: u32 = 16383;
const AVIF_SPEED: u8 = 8;

type Encoded<'a> = (Cow<'a, [u8]>, FileFormat);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileFormat {
    Png,
    Webp,
    Avif,
    #[serde(alias = "jpg")]
    Jpeg,
}

impl FileFormat {
    pub fn ext(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Webp => "webp",
            Self::Avif => "avif",
            Self::Jpeg => "jpg",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Jpeg => "jpeg",
            other => other.ext(),
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Webp => "image/webp",
            Self::Avif => "image/avif",
            Self::Jpeg => "image/jpeg",
        }
    }

    pub fn parse(s: &str) -> Result<Self, &'static str> {
        match s.to_ascii_lowercase().as_str() {
            "png" => Ok(Self::Png),
            "webp" => Ok(Self::Webp),
            "avif" => Ok(Self::Avif),
            "jpeg" | "jpg" => Ok(Self::Jpeg),
            _ => Err("expected png, webp, avif or jpeg"),
        }
    }
}

fn quality(q: u8) -> u8 {
    q.clamp(1, 100)
}

/// format actually written: PNG instead of WebP when the image is too big for it
/// which will be the case for scrolling picture
pub fn encode(png: &[u8], format: FileFormat, q: u8) -> Result<Encoded<'_>, Box<dyn Error>> {
    if format == FileFormat::Png {
        return Ok((Cow::Borrowed(png), format));
    }
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png)?;
    let (w, h) = (img.width(), img.height());
    let q = quality(q);
    let mut out = Vec::new();
    match format {
        FileFormat::Png => unreachable!(),
        FileFormat::Webp => {
            if w > WEBP_MAX || h > WEBP_MAX {
                log::warn!("{w}x{h} is too big for WebP, saving PNG");
                return Ok((Cow::Borrowed(png), FileFormat::Png));
            }
            image::codecs::webp::WebPEncoder::new_lossless(&mut out).write_image(
                img.to_rgba8().as_raw(),
                w,
                h,
                ExtendedColorType::Rgba8,
            )?;
        }
        FileFormat::Avif => {
            let pixels: Vec<ravif::RGBA8> = img
                .to_rgba8()
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| ravif::RGBA8::new(p[0], p[1], p[2], p[3]))
                .collect();
            let enc = ravif::Encoder::new()
                .with_quality(q as f32)
                .with_alpha_quality(q as f32)
                .with_speed(AVIF_SPEED);
            let img = ravif::Img::new(pixels.as_slice(), w as usize, h as usize);
            out = enc.encode_rgba(img)?.avif_file;
        }
        FileFormat::Jpeg => {
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, q).write_image(
                img.to_rgb8().as_raw(),
                w,
                h,
                ExtendedColorType::Rgb8,
            )?;
        }
    }
    Ok((Cow::Owned(out), format))
}

pub fn for_clipboard(png: &[u8]) -> Option<(FileFormat, Vec<u8>)> {
    let save = &crate::config::get().save;
    if save.clipboard_format == FileFormat::Png {
        return None;
    }
    match encode(png, save.clipboard_format, save.quality) {
        Ok((data, format)) if format == save.clipboard_format => Some((format, data.into_owned())),
        Ok(_) => None,
        Err(e) => {
            log::warn!("can't encode the clipboard image: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FileFormat, encode, for_clipboard, quality};

    #[test]
    fn mime_names_the_format() {
        assert_eq!(FileFormat::Png.mime(), "image/png");
        assert_eq!(FileFormat::Webp.mime(), "image/webp");
        assert_eq!(FileFormat::Avif.mime(), "image/avif");
        assert_eq!(FileFormat::Jpeg.mime(), "image/jpeg");
    }

    #[test]
    fn clipboard_gets_no_extra_for_png() {
        assert!(for_clipboard(&png(8, 8)).is_none());
    }

    #[test]
    fn parse_takes_the_names_and_jpg() {
        for (name, format) in [
            ("png", FileFormat::Png),
            ("webp", FileFormat::Webp),
            ("avif", FileFormat::Avif),
            ("jpeg", FileFormat::Jpeg),
            ("jpg", FileFormat::Jpeg),
            ("JPG", FileFormat::Jpeg),
        ] {
            assert_eq!(FileFormat::parse(name), Ok(format));
            assert_eq!(FileFormat::parse(format.name()), Ok(format));
        }
        assert!(FileFormat::parse("bmp").is_err());
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(w, h, |x, y| {
            image::Rgba([(x * 7) as u8, (y * 5) as u8, 90, 255])
        });
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    fn size(data: &[u8]) -> (u32, u32) {
        let img = image::load_from_memory(data).unwrap();
        (img.width(), img.height())
    }

    #[test]
    fn png_is_written_unchanged() {
        let src = png(40, 30);
        let (out, format) = encode(&src, FileFormat::Png, 90).unwrap();
        assert_eq!((out.as_ref(), format), (src.as_slice(), FileFormat::Png));
    }

    #[test]
    fn webp_and_jpeg_decode_back() {
        let src = png(40, 30);
        for f in [FileFormat::Webp, FileFormat::Jpeg] {
            let (out, format) = encode(&src, f, 90).unwrap();
            assert_eq!(format, f);
            assert_eq!(size(&out), (40, 30));
        }
    }

    #[test]
    fn avif_has_an_avif_ftyp_box() {
        let src = png(40, 30);
        let (out, format) = encode(&src, FileFormat::Avif, 90).unwrap();
        assert_eq!(format, FileFormat::Avif);
        assert_eq!(&out[4..12], b"ftypavif");
    }

    #[test]
    fn webp_too_big_falls_back_to_png() {
        let src = png(16384, 1);
        let (out, format) = encode(&src, FileFormat::Webp, 90).unwrap();
        assert_eq!((out.as_ref(), format), (src.as_slice(), FileFormat::Png));
    }

    #[test]
    fn quality_is_clamped() {
        assert_eq!((quality(0), quality(50), quality(200)), (1, 50, 100));
    }
}
