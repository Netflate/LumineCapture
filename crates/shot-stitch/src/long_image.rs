use crate::profile::Signature;
use shot_image::buffer::allocate;
use shot_image::{Image, ImageError, MAX_PIXELS};

#[derive(Default)]
pub(crate) struct LongImage {
    pixels: Vec<u32>,
    width: i32,
    top: usize,
    used: usize,
    pub cols: Signature,
    pub revision: u64,
}

impl LongImage {
    pub fn reset(&mut self, seed: &Image, cols: Signature) -> Result<(), ImageError> {
        let mut pixels = allocate(seed.pixels().len())?;
        pixels.copy_from_slice(seed.pixels());
        *self = Self {
            pixels,
            width: seed.width(),
            used: seed.height() as usize,
            cols,
            revision: self.revision.wrapping_add(1),
            ..Self::default()
        };
        Ok(())
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.used as i32
    }

    pub fn row(&self, y: i32) -> &[u32] {
        assert!(y >= 0 && (y as usize) < self.used);
        let start = (self.top + y as usize) * self.width as usize;
        &self.pixels[start..start + self.width as usize]
    }

    pub fn image(&self) -> Result<Image, ImageError> {
        let width = self.width as usize;
        let mut data = Vec::new();
        data.try_reserve_exact(self.used * width)
            .map_err(|_| ImageError::Allocation)?;
        let start = self.top * width;
        data.extend_from_slice(&self.pixels[start..start + self.used * width]);
        Image::from_pixels(self.width, self.height(), data)
    }

    /// Reserves space; bottom growth avoids copying the existing image.
    fn ensure_room(&mut self, grow_top: usize, grow_bottom: usize) -> Result<(), ImageError> {
        let width = self.width as usize;
        let target = self
            .used
            .checked_add(grow_top)
            .and_then(|n| n.checked_add(grow_bottom))
            .ok_or(ImageError::Dimensions)?;
        let limit = MAX_PIXELS / (width + crate::profile::BAND_COUNT);
        if target > limit {
            return Err(ImageError::Dimensions);
        }
        if self.top < grow_top {
            let reserve = grow_top + (target / 8).max(256);
            let mut grown = Vec::new();
            grown
                .try_reserve_exact((reserve + self.used + grow_bottom) * width)
                .map_err(|_| ImageError::Allocation)?;
            grown.resize(reserve * width, 0);
            let start = self.top * width;
            grown.extend_from_slice(&self.pixels[start..start + self.used * width]);
            self.pixels = grown;
            self.top = reserve;
        }
        let needed = self.top + self.used + grow_bottom;
        if needed * width > self.pixels.capacity() {
            let rows = (needed + needed / 2)
                .max(needed + 256)
                .min(self.top + limit);
            self.pixels
                .try_reserve_exact(rows * width - self.pixels.len())
                .map_err(|_| ImageError::Allocation)?;
        }
        Ok(())
    }

    pub fn append(
        &mut self,
        frame: &Image,
        cols: &Signature,
        rows: i32,
        trim: i32,
    ) -> Result<(), ImageError> {
        let rows = rows.clamp(0, frame.height().min(cols.len() as i32)) as usize;
        let trim = trim.clamp(0, (self.used as i32 - 1).max(0)) as usize;
        self.cols
            .try_reserve(rows)
            .map_err(|_| ImageError::Allocation)?;
        self.ensure_room(0, rows.saturating_sub(trim))?;
        self.used -= trim;
        self.cols.truncate(self.used);
        self.pixels
            .truncate((self.top + self.used) * self.width as usize);
        for i in 0..rows {
            let y = frame.height() as usize - rows + i;
            self.pixels.extend_from_slice(frame.row(y as i32));
            self.cols.push(cols[y]);
        }
        self.used += rows;
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }

    pub fn prepend(
        &mut self,
        frame: &Image,
        cols: &Signature,
        rows: i32,
        trim: i32,
    ) -> Result<(), ImageError> {
        let rows = rows.clamp(0, frame.height().min(cols.len() as i32)) as usize;
        let trim = trim.clamp(0, (self.used as i32 - 1).max(0)) as usize;
        let mut merged = Vec::new();
        merged
            .try_reserve_exact(rows + self.used - trim)
            .map_err(|_| ImageError::Allocation)?;
        merged.extend_from_slice(&cols[..rows]);
        merged.extend_from_slice(&self.cols[trim..]);
        self.ensure_room(rows.saturating_sub(trim), 0)?;
        self.top += trim;
        self.used -= trim;
        let width = self.width as usize;
        for i in 0..rows {
            let start = (self.top - rows + i) * width;
            self.pixels[start..start + width].copy_from_slice(frame.row(i as i32));
        }
        self.top -= rows;
        self.used += rows;
        self.cols = merged;
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
}
