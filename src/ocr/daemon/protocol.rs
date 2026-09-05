// UNIX socket protocol for the OCR daemon.
// Framing structure: u32 LE length (tag + body) | u8 tag | body.
// Numeric fields are little-endian; strings and byte buffers are prefixed with a u32 length.
// The initial `Hello` and `Welcome` handshakes (carrying `proto` and `build` identifiers) must retain their layout:
// they allow cross-version process recognition, triggering out-of-date daemons to step down.

use std::ffi::OsStr;
use std::fmt;
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use tiny_skia::Rect;

use crate::ocr::models::ModelFiles;
use crate::ocr::{OcrImage, OcrLine, OcrText};

pub const PROTO: u16 = 1;
/// `Recognize` frame payload
///  full desktop screen capture spanning multiple display monitors.
pub const MAX_FRAME: usize = 256 * 1024 * 1024;
/// All remaining frame types.
pub const MAX_SMALL: usize = 16 * 1024 * 1024;
pub const MAX_LINES: usize = 100_000;
const IMAGE_HEAD: usize = 16;

mod tag {
    pub const HELLO: u8 = 1;
    pub const WELCOME: u8 = 2;
    pub const SHUTDOWN: u8 = 3;
    pub const OK: u8 = 4;
    pub const ERROR: u8 = 5;
    pub const LOAD: u8 = 6;
    pub const RECOGNIZE: u8 = 7;
    pub const LINES: u8 = 8;
    pub const STATUS: u8 = 9;
    pub const STATUS_REPLY: u8 = 10;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineState {
    Idle,
    Loading { model: String },
    Ready { device: String, model: String },
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Welcome {
    pub proto: u16,
    pub build: String,
    pub state: EngineState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub pid: u32,
    pub uptime_secs: u64,
    pub state: EngineState,
    pub served: u64,
    pub last_ms: u32,
    pub rss_kb: u64,
    pub idle_left_secs: Option<u64>,
}

pub enum Request {
    Hello { proto: u16, build: String },
    Load(ModelFiles),
    Recognize(OcrImage),
    Status,
    Shutdown,
}

/// Recognition request payload
/// image buffer is borrowed so it can be processed locally if daemon RPC fails.
#[derive(Clone, Copy)]
pub enum Outgoing<'a> {
    Hello(&'a str),
    Load(&'a ModelFiles),
    Recognize(&'a OcrImage),
    Status,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    Welcome(Welcome),
    Ok,
    Error(String),
    Lines(OcrText),
    Status(Status),
}

#[derive(Debug)]
pub enum ProtoError {
    Io(io::Error),
    TooLarge(usize),
    Malformed(&'static str),
    UnknownTag(u8),
}

impl fmt::Display for ProtoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtoError::Io(e) => write!(f, "{e}"),
            ProtoError::TooLarge(len) => write!(f, "frame of {len} bytes is too large"),
            ProtoError::Malformed(what) => write!(f, "malformed {what}"),
            ProtoError::UnknownTag(tag) => write!(f, "unknown message {tag}"),
        }
    }
}

impl std::error::Error for ProtoError {}

impl From<io::Error> for ProtoError {
    fn from(e: io::Error) -> Self {
        ProtoError::Io(e)
    }
}

pub fn write_request(w: &mut impl Write, request: Outgoing) -> io::Result<()> {
    let mut body = Body::default();
    match request {
        Outgoing::Hello(build) => {
            body.u16(PROTO);
            body.str(build);
            body.send(w, tag::HELLO)
        }
        Outgoing::Load(files) => {
            body.path(&files.detector);
            body.path(&files.recognizer);
            body.path(&files.dict);
            body.send(w, tag::LOAD)
        }
        Outgoing::Recognize(image) => write_image(w, image),
        Outgoing::Status => body.send(w, tag::STATUS),
        Outgoing::Shutdown => body.send(w, tag::SHUTDOWN),
    }
}

pub fn read_request(r: &mut impl Read) -> Result<Request, ProtoError> {
    let (tag, len) = read_head(r)?;
    if tag == tag::RECOGNIZE {
        return read_image(r, len).map(Request::Recognize);
    }
    let body = read_small(r, len)?;
    let mut c = Cursor { buf: &body };
    let request = match tag {
        tag::HELLO => {
            return Ok(Request::Hello {
                proto: c.u16()?,
                build: c.string()?,
            });
        }
        tag::LOAD => Request::Load(ModelFiles {
            detector: c.path()?,
            recognizer: c.path()?,
            dict: c.path()?,
        }),
        tag::STATUS => Request::Status,
        tag::SHUTDOWN => Request::Shutdown,
        other => return Err(ProtoError::UnknownTag(other)),
    };
    c.finish()?;
    Ok(request)
}

pub fn write_reply(w: &mut impl Write, reply: &Reply) -> io::Result<()> {
    let mut body = Body::default();
    let tag = match reply {
        Reply::Welcome(welcome) => {
            body.u16(welcome.proto);
            body.str(&welcome.build);
            body.state(&welcome.state);
            tag::WELCOME
        }
        Reply::Ok => tag::OK,
        Reply::Error(message) => {
            body.str(message);
            tag::ERROR
        }
        Reply::Lines(text) => {
            if text.lines.len() > MAX_LINES {
                return Err(invalid("too many lines to send"));
            }
            body.u32(text.lines.len() as u32);
            for line in &text.lines {
                body.str(&line.text);
                let b = line.bounds;
                for v in [b.left(), b.top(), b.right(), b.bottom()] {
                    body.f32(v);
                }
                body.u32(line.char_x.len() as u32);
                for &x in &line.char_x {
                    body.f32(x);
                }
            }
            tag::LINES
        }
        Reply::Status(status) => {
            body.u32(status.pid);
            body.u64(status.uptime_secs);
            body.state(&status.state);
            body.u64(status.served);
            body.u32(status.last_ms);
            body.u64(status.rss_kb);
            match status.idle_left_secs {
                Some(secs) => {
                    body.u8(1);
                    body.u64(secs);
                }
                None => body.u8(0),
            }
            tag::STATUS_REPLY
        }
    };
    body.send(w, tag)
}

pub fn read_reply(r: &mut impl Read) -> Result<Reply, ProtoError> {
    let (tag, len) = read_head(r)?;
    let body = read_small(r, len)?;
    let mut c = Cursor { buf: &body };
    let reply = match tag {
        tag::WELCOME => {
            let proto = c.u16()?;
            let build = c.string()?;
            if proto != PROTO {
                // Skip parsing state from mismatched versions. Daemon instance will be replaced regardless.

                return Ok(Reply::Welcome(Welcome {
                    proto,
                    build,
                    state: EngineState::Idle,
                }));
            }
            Reply::Welcome(Welcome {
                proto,
                build,
                state: c.state()?,
            })
        }
        tag::OK => Reply::Ok,
        tag::ERROR => Reply::Error(c.string()?),
        tag::LINES => Reply::Lines(c.lines()?),
        tag::STATUS_REPLY => Reply::Status(Status {
            pid: c.u32()?,
            uptime_secs: c.u64()?,
            state: c.state()?,
            served: c.u64()?,
            last_ms: c.u32()?,
            rss_kb: c.u64()?,
            idle_left_secs: match c.u8()? {
                0 => None,
                1 => Some(c.u64()?),
                _ => return Err(ProtoError::Malformed("status")),
            },
        }),
        other => return Err(ProtoError::UnknownTag(other)),
    };
    c.finish()?;
    Ok(reply)
}

fn write_image(w: &mut impl Write, image: &OcrImage) -> io::Result<()> {
    let pixels = image_len(image.width, image.height)
        .filter(|&n| n == image.rgb.len())
        .ok_or_else(|| invalid("image buffer does not match its size"))?;
    let len = 1 + IMAGE_HEAD + pixels;
    if len > MAX_FRAME {
        return Err(invalid("image is too large to send"));
    }
    let mut head = Vec::with_capacity(5 + IMAGE_HEAD);
    head.extend_from_slice(&(len as u32).to_le_bytes());
    head.push(tag::RECOGNIZE);
    head.extend_from_slice(&image.width.to_le_bytes());
    head.extend_from_slice(&image.height.to_le_bytes());
    head.extend_from_slice(&image.origin.0.to_le_bytes());
    head.extend_from_slice(&image.origin.1.to_le_bytes());
    // pixels written directly, avoiding intermediate buffer copies.
    w.write_all(&head)?;
    w.write_all(&image.rgb)?;
    w.flush()
}

fn read_image(r: &mut impl Read, len: usize) -> Result<OcrImage, ProtoError> {
    if len < IMAGE_HEAD {
        return Err(ProtoError::Malformed("image"));
    }
    let mut head = [0u8; IMAGE_HEAD];
    r.read_exact(&mut head)?;
    let mut c = Cursor { buf: &head };
    let (width, height) = (c.u32()?, c.u32()?);
    let origin = (c.f32()?, c.f32()?);
    let pixels = image_len(width, height)
        .filter(|&n| n == len - IMAGE_HEAD)
        .ok_or(ProtoError::Malformed("image size"))?;
    let mut rgb = vec![0u8; pixels];
    r.read_exact(&mut rgb)?;
    Ok(OcrImage {
        rgb,
        width,
        height,
        origin,
    })
}

/// RGB8 image byte buffer.
/// returns `None` if empty or if dimension size overflows.
fn image_len(width: u32, height: u32) -> Option<usize> {
    (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(3)
        .filter(|&n| n > 0)
}

fn read_head(r: &mut impl Read) -> Result<(u8, usize), ProtoError> {
    let mut head = [0u8; 5];
    r.read_exact(&mut head)?;
    let len = u32::from_le_bytes([head[0], head[1], head[2], head[3]]) as usize;
    if len == 0 {
        return Err(ProtoError::Malformed("frame"));
    }
    if len > MAX_FRAME {
        return Err(ProtoError::TooLarge(len));
    }
    Ok((head[4], len - 1))
}

fn read_small(r: &mut impl Read, len: usize) -> Result<Vec<u8>, ProtoError> {
    if len + 1 > MAX_SMALL {
        return Err(ProtoError::TooLarge(len + 1));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    Ok(body)
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[derive(Default)]
struct Body(Vec<u8>);

impl Body {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }

    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn bytes(&mut self, v: &[u8]) {
        self.u32(u32::try_from(v.len()).unwrap_or(u32::MAX));
        self.0.extend_from_slice(v);
    }

    fn str(&mut self, v: &str) {
        self.bytes(v.as_bytes());
    }

    fn path(&mut self, v: &Path) {
        self.bytes(v.as_os_str().as_bytes());
    }

    fn state(&mut self, state: &EngineState) {
        match state {
            EngineState::Idle => self.u8(0),
            EngineState::Loading { model } => {
                self.u8(1);
                self.str(model);
            }
            EngineState::Ready { device, model } => {
                self.u8(2);
                self.str(device);
                self.str(model);
            }
            EngineState::Failed(message) => {
                self.u8(3);
                self.str(message);
            }
        }
    }

    fn send(self, w: &mut impl Write, tag: u8) -> io::Result<()> {
        let len = 1 + self.0.len();
        if len > MAX_SMALL {
            return Err(invalid("message is too large to send"));
        }
        let mut frame = Vec::with_capacity(4 + len);
        frame.extend_from_slice(&(len as u32).to_le_bytes());
        frame.push(tag);
        frame.extend_from_slice(&self.0);
        w.write_all(&frame)?;
        w.flush()
    }
}

struct Cursor<'a> {
    buf: &'a [u8],
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], ProtoError> {
        if self.buf.len() < n {
            return Err(ProtoError::Malformed("truncated message"));
        }
        let (head, rest) = self.buf.split_at(n);
        self.buf = rest;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], ProtoError> {
        Ok(self.take(N)?.try_into().expect("take returned N bytes"))
    }

    fn u8(&mut self) -> Result<u8, ProtoError> {
        Ok(self.array::<1>()?[0])
    }

    fn u16(&mut self) -> Result<u16, ProtoError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    fn u32(&mut self) -> Result<u32, ProtoError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, ProtoError> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    fn f32(&mut self) -> Result<f32, ProtoError> {
        let v = f32::from_le_bytes(self.array()?);
        if v.is_finite() {
            Ok(v)
        } else {
            Err(ProtoError::Malformed("number"))
        }
    }

    fn bytes(&mut self) -> Result<&'a [u8], ProtoError> {
        let len = self.u32()? as usize;
        self.take(len)
    }

    fn string(&mut self) -> Result<String, ProtoError> {
        std::str::from_utf8(self.bytes()?)
            .map(str::to_owned)
            .map_err(|_| ProtoError::Malformed("text"))
    }

    fn path(&mut self) -> Result<PathBuf, ProtoError> {
        Ok(PathBuf::from(OsStr::from_bytes(self.bytes()?)))
    }

    fn state(&mut self) -> Result<EngineState, ProtoError> {
        Ok(match self.u8()? {
            0 => EngineState::Idle,
            1 => EngineState::Loading {
                model: self.string()?,
            },
            2 => EngineState::Ready {
                device: self.string()?,
                model: self.string()?,
            },
            3 => EngineState::Failed(self.string()?),
            _ => return Err(ProtoError::Malformed("engine state")),
        })
    }

    fn lines(&mut self) -> Result<OcrText, ProtoError> {
        let count = self.u32()? as usize;
        if count > MAX_LINES {
            return Err(ProtoError::Malformed("line count"));
        }
        let mut lines = Vec::with_capacity(count.min(self.buf.len() / 24));
        for _ in 0..count {
            let text = self.string()?;
            let (l, t, r, b) = (self.f32()?, self.f32()?, self.f32()?, self.f32()?);
            let bounds = Rect::from_ltrb(l, t, r, b).ok_or(ProtoError::Malformed("line bounds"))?;
            let n = self.u32()? as usize;
            if n > self.buf.len() / 4 {
                return Err(ProtoError::Malformed("truncated message"));
            }
            let char_x = (0..n).map(|_| self.f32()).collect::<Result<Vec<_>, _>>()?;
            lines.push(OcrLine {
                text,
                bounds,
                char_x,
            });
        }
        Ok(OcrText { lines })
    }

    fn finish(&self) -> Result<(), ProtoError> {
        if self.buf.is_empty() {
            Ok(())
        } else {
            Err(ProtoError::Malformed("trailing bytes"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocr::testing;

    fn frame(tag: u8, body: &[u8]) -> Vec<u8> {
        let mut out = ((body.len() + 1) as u32).to_le_bytes().to_vec();
        out.push(tag);
        out.extend_from_slice(body);
        out
    }

    fn encode_request(request: Outgoing) -> Vec<u8> {
        let mut out = Vec::new();
        write_request(&mut out, request).unwrap();
        out
    }

    fn encode_reply(reply: &Reply) -> Vec<u8> {
        let mut out = Vec::new();
        write_reply(&mut out, reply).unwrap();
        out
    }

    fn roundtrip(reply: Reply) {
        let bytes = encode_reply(&reply);
        assert_eq!(read_reply(&mut bytes.as_slice()).unwrap(), reply);
    }

    fn malformed(result: Result<Reply, ProtoError>) -> bool {
        matches!(result, Err(ProtoError::Malformed(_)))
    }

    #[test]
    fn requests_roundtrip() {
        let bytes = encode_request(Outgoing::Hello("1.0-abc"));
        match read_request(&mut bytes.as_slice()).unwrap() {
            Request::Hello { proto, build } => {
                assert_eq!(proto, PROTO);
                assert_eq!(build, "1.0-abc");
            }
            _ => panic!("expected hello"),
        }

        let files = testing::files("кириллица");
        let bytes = encode_request(Outgoing::Load(&files));
        match read_request(&mut bytes.as_slice()).unwrap() {
            Request::Load(read) => assert_eq!(read, files),
            _ => panic!("expected load"),
        }

        let image = testing::image(7, 5);
        let bytes = encode_request(Outgoing::Recognize(&image));
        match read_request(&mut bytes.as_slice()).unwrap() {
            Request::Recognize(read) => {
                assert_eq!((read.width, read.height, read.origin), (7, 5, image.origin));
                assert_eq!(read.rgb, image.rgb);
            }
            _ => panic!("expected recognize"),
        }

        let bytes = encode_request(Outgoing::Status);
        assert!(matches!(read_request(&mut bytes.as_slice()), Ok(Request::Status)));
        let bytes = encode_request(Outgoing::Shutdown);
        assert!(matches!(read_request(&mut bytes.as_slice()), Ok(Request::Shutdown)));
    }

    #[test]
    fn non_utf8_paths_survive() {
        let files = ModelFiles {
            detector: PathBuf::from(OsStr::from_bytes(b"/m/\xff\xfe.onnx")),
            recognizer: PathBuf::from("/m/r.onnx"),
            dict: PathBuf::from("/m/d.txt"),
        };
        let bytes = encode_request(Outgoing::Load(&files));
        match read_request(&mut bytes.as_slice()).unwrap() {
            Request::Load(read) => assert_eq!(read, files),
            _ => panic!("expected load"),
        }
    }

    #[test]
    fn replies_roundtrip() {
        for state in [
            EngineState::Idle,
            EngineState::Loading { model: "m".into() },
            EngineState::Ready {
                device: "cpu".into(),
                model: "/models/cyrillic.onnx".into(),
            },
            EngineState::Failed("no such file".into()),
        ] {
            roundtrip(Reply::Welcome(Welcome {
                proto: PROTO,
                build: "b".into(),
                state: state.clone(),
            }));
            for idle_left_secs in [None, Some(42)] {
                roundtrip(Reply::Status(Status {
                    pid: 123,
                    uptime_secs: 9,
                    state: state.clone(),
                    served: 7,
                    last_ms: 314,
                    rss_kb: 64_000,
                    idle_left_secs,
                }));
            }
        }
        roundtrip(Reply::Ok);
        roundtrip(Reply::Error("ошибка".into()));
        roundtrip(Reply::Lines(OcrText::default()));
        roundtrip(Reply::Lines(testing::labelled("привет", &testing::image(30, 10))));
    }

    #[test]
    fn several_frames_read_back_in_order() {
        let mut bytes = encode_reply(&Reply::Ok);
        bytes.extend(encode_reply(&Reply::Error("x".into())));
        let mut r = bytes.as_slice();
        assert_eq!(read_reply(&mut r).unwrap(), Reply::Ok);
        assert_eq!(read_reply(&mut r).unwrap(), Reply::Error("x".into()));
        assert!(matches!(read_reply(&mut r), Err(ProtoError::Io(e)) if e.kind() == io::ErrorKind::UnexpectedEof));
    }

    #[test]
    fn truncated_frames_are_errors_not_panics() {
        let full = encode_reply(&Reply::Lines(testing::labelled("abc", &testing::image(4, 4))));
        for cut in 0..full.len() {
            assert!(read_reply(&mut &full[..cut]).is_err(), "cut at {cut}");
        }
        let full = encode_request(Outgoing::Recognize(&testing::image(3, 3)));
        for cut in 0..full.len() {
            assert!(read_request(&mut &full[..cut]).is_err(), "cut at {cut}");
        }
    }

    #[test]
    fn oversized_frames_are_refused_before_allocating() {
        let mut huge = ((MAX_FRAME + 1) as u32).to_le_bytes().to_vec();
        huge.push(tag::OK);
        assert!(matches!(read_reply(&mut huge.as_slice()), Err(ProtoError::TooLarge(_))));

        let mut big_small = ((MAX_SMALL + 1) as u32).to_le_bytes().to_vec();
        big_small.push(tag::STATUS);
        assert!(matches!(read_request(&mut big_small.as_slice()), Err(ProtoError::TooLarge(_))));

        // (?#) HTTP-клиент стучится в сокет
        assert!(read_request(&mut b"GET / HTTP/1.1\r\n\r\n".as_slice()).is_err());
    }

    #[test]
    fn empty_frame_and_unknown_tags() {
        let zero = 0u32.to_le_bytes();
        assert!(matches!(read_reply(&mut zero.as_slice()), Err(ProtoError::Malformed(_)) | Err(ProtoError::Io(_))));
        let mut zero = zero.to_vec();
        zero.push(0);
        assert!(matches!(read_reply(&mut zero.as_slice()), Err(ProtoError::Malformed(_))));

        assert!(matches!(read_reply(&mut frame(200, &[]).as_slice()), Err(ProtoError::UnknownTag(200))));
        assert!(matches!(read_request(&mut frame(200, &[]).as_slice()), Err(ProtoError::UnknownTag(200))));
        // A reply is not mistaken for a request and vice versa
        assert!(matches!(read_request(&mut frame(tag::OK, &[]).as_slice()), Err(ProtoError::UnknownTag(_))));
        assert!(matches!(read_reply(&mut frame(tag::STATUS, &[]).as_slice()), Err(ProtoError::UnknownTag(_))));
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        assert!(malformed(read_reply(&mut frame(tag::OK, &[1]).as_slice())));
        assert!(matches!(read_request(&mut frame(tag::STATUS, &[1]).as_slice()), Err(ProtoError::Malformed(_))));
    }

    #[test]
    fn hello_from_a_newer_client_may_carry_more() {
        let mut body = Body::default();
        body.u16(PROTO + 1);
        body.str("future");
        body.u64(99);
        let mut bytes = Vec::new();
        body.send(&mut bytes, tag::HELLO).unwrap();
        match read_request(&mut bytes.as_slice()).unwrap() {
            Request::Hello { proto, build } => assert_eq!((proto, build.as_str()), (PROTO + 1, "future")),
            _ => panic!("expected hello"),
        }
    }

    #[test]
    fn welcome_from_another_version_is_still_readable() {
        let mut body = Body::default();
        body.u16(PROTO + 7);
        body.str("old");
        body.u8(250); // state unknown to this version
        body.u64(1);
        let mut bytes = Vec::new();
        body.send(&mut bytes, tag::WELCOME).unwrap();
        match read_reply(&mut bytes.as_slice()).unwrap() {
            Reply::Welcome(w) => assert_eq!((w.proto, w.build.as_str()), (PROTO + 7, "old")),
            other => panic!("expected welcome, got {other:?}"),
        }
    }

    #[test]
    fn bad_image_sizes() {
        let image_frame = |w: u32, h: u32, pixels: usize| {
            let mut body = Vec::new();
            body.extend_from_slice(&w.to_le_bytes());
            body.extend_from_slice(&h.to_le_bytes());
            body.extend_from_slice(&0f32.to_le_bytes());
            body.extend_from_slice(&0f32.to_le_bytes());
            body.extend(std::iter::repeat_n(0u8, pixels));
            frame(tag::RECOGNIZE, &body)
        };
        let is_malformed = |bytes: Vec<u8>| matches!(read_request(&mut bytes.as_slice()), Err(ProtoError::Malformed(_)));

        assert!(is_malformed(image_frame(2, 2, 11)));
        assert!(is_malformed(image_frame(2, 2, 13)));
        assert!(is_malformed(image_frame(0, 5, 0)));
        // `w * h * 3` can overflow and must not wrap around into an unexpectedly small buffer size.
        assert!(is_malformed(image_frame(u32::MAX, u32::MAX, 12)));
        assert!(matches!(read_request(&mut frame(tag::RECOGNIZE, &[0; 3]).as_slice()), Err(ProtoError::Malformed(_))));

        let mut nan_origin = image_frame(1, 1, 3);
        nan_origin[13..17].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(is_malformed(nan_origin));
    }

    #[test]
    fn mismatched_image_is_not_sent() {
        let mut image = testing::image(4, 4);
        image.rgb.pop();
        let mut out = Vec::new();
        assert!(write_request(&mut out, Outgoing::Recognize(&image)).is_err());
        assert!(out.is_empty());
    }

    #[test]
    fn lines_are_validated() {
        let line = |text: &[u8], bounds: [f32; 4], char_x: &[f32], declared: Option<u32>| {
            let mut body = Body::default();
            body.u32(1);
            body.bytes(text);
            for v in bounds {
                body.f32(v);
            }
            body.u32(declared.unwrap_or(char_x.len() as u32));
            for &x in char_x {
                body.f32(x);
            }
            let mut bytes = Vec::new();
            body.send(&mut bytes, tag::LINES).unwrap();
            read_reply(&mut bytes.as_slice())
        };
        let ok = [0.0, 0.0, 10.0, 5.0];
        assert!(line(b"ab", ok, &[0.0, 5.0, 10.0], None).is_ok());
        assert!(malformed(line(b"\xc3\x28", ok, &[0.0, 5.0, 10.0], None)));
        assert!(malformed(line(b"ab", [0.0, 0.0, f32::INFINITY, 5.0], &[0.0], None)));
        assert!(malformed(line(b"ab", [10.0, 0.0, 0.0, 5.0], &[0.0], None)));
        assert!(malformed(line(b"ab", ok, &[0.0, f32::NAN, 10.0], None)));
        assert!(malformed(line(b"ab", ok, &[0.0], Some(u32::MAX))));

        let mut body = Body::default();
        body.u32(u32::MAX);
        let mut bytes = Vec::new();
        body.send(&mut bytes, tag::LINES).unwrap();
        assert!(malformed(read_reply(&mut bytes.as_slice())));
    }

    #[test]
    fn bad_state_and_status_flags() {
        let mut body = Body::default();
        body.u16(PROTO);
        body.str("b");
        body.u8(9);
        let mut bytes = Vec::new();
        body.send(&mut bytes, tag::WELCOME).unwrap();
        assert!(malformed(read_reply(&mut bytes.as_slice())));

        let status = Reply::Status(Status {
            pid: 1,
            uptime_secs: 1,
            state: EngineState::Idle,
            served: 0,
            last_ms: 0,
            rss_kb: 0,
            idle_left_secs: None,
        });
        let mut bytes = encode_reply(&status);
        *bytes.last_mut().unwrap() = 2;
        assert!(malformed(read_reply(&mut bytes.as_slice())));
    }

    #[test]
    fn too_many_lines_are_not_sent() {
        let one = testing::labelled("a", &testing::image(2, 2)).lines.remove(0);
        let text = OcrText {
            lines: vec![one; MAX_LINES + 1],
        };
        assert!(write_reply(&mut Vec::new(), &Reply::Lines(text)).is_err());
    }
}
