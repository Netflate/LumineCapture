use crate::backend::wayland::utils::state::OverlayState;
use nix::sys::memfd::{MFdFlags, memfd_create};
use nix::unistd::ftruncate;
use std::ffi::CStr;
use std::os::fd::AsFd;
use wayland_client::{
    QueueHandle,
    protocol::{wl_buffer, wl_shm},
};

pub struct ShmBuffer {
    pub buffer: wl_buffer::WlBuffer,
    pub mmap: memmap2::MmapMut,
    _fd: std::os::fd::OwnedFd,
}

pub fn create_shm_buffer(
    shm: &wl_shm::WlShm,
    qh: &QueueHandle<OverlayState>,
    width: u32,
    height: u32,
) -> Result<ShmBuffer, Box<dyn std::error::Error>> {
    let stride = width * 4;
    let size = (stride * height) as usize;

    let fd = memfd_create(
        CStr::from_bytes_with_nul(b"lumine-shm\0")?,
        MFdFlags::empty(),
    )?;
    ftruncate(&fd, size as i64)?;
    let mmap = unsafe { memmap2::MmapMut::map_mut(&fd)? };

    let pool = shm.create_pool(fd.as_fd(), size as i32, qh, ());
    let buffer = pool.create_buffer(
        0,
        width as i32,
        height as i32,
        stride as i32,
        wl_shm::Format::Argb8888,
        qh,
        (),
    );
    pool.destroy();

    Ok(ShmBuffer {
        buffer,
        mmap: mmap,
        _fd: fd,
    })
}

impl ShmBuffer {
    pub fn write_pixels(&mut self, pixels: &[u8]) {
        self.mmap[..pixels.len()].copy_from_slice(pixels);
    }

    pub fn write_pixels_rect(&mut self, pixels: &[u8], width: u32, rect: (u32, u32, u32, u32)) {
        let (x, y, w, h) = rect;
        if w == 0 || h == 0 {
            return;
        }

        let stride = (width * 4) as usize;
        let row_bytes = (w * 4) as usize;
        let src = pixels;
        let dst = &mut self.mmap;

        for row in 0..h {
            let sy = (y + row) as usize;
            let sx = x as usize;
            let off = sy * stride + sx * 4;

            dst[off..off + row_bytes].copy_from_slice(&src[off..off + row_bytes]);
        }
    }
}
