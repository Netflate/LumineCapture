use wayland_client::{
    QueueHandle, 
    protocol::{
        wl_buffer, wl_shm
    },
};
use crate::backend::wayland::utils::state::OverlayState;
use nix::sys::memfd::{MFdFlags, memfd_create};
use std::ffi::CStr;
use std::os::fd::AsFd;
use nix::unistd::ftruncate;


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
    let buffer = pool.create_buffer(0, width as i32, height as i32, stride as i32,
        wl_shm::Format::Argb8888, qh, ());
    pool.destroy();

    Ok(ShmBuffer { buffer, mmap: mmap, _fd: fd })
}



impl ShmBuffer {
    pub fn write_pixels(&mut self, pixels : &[u8]) {
        self.mmap[..pixels.len()].copy_from_slice(pixels);
    }
}