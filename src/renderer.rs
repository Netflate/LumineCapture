use crate::types::CapturedFrame;


pub fn create_first_frame(mut frame: CapturedFrame) -> CapturedFrame{
    let intensity = 0.3;
    for pixel in frame.pixels.chunks_exact_mut(4) {
        pixel[0] = (pixel[0] as f32 * intensity) as u8;
        pixel[1] = (pixel[1] as f32 * intensity) as u8;
        pixel[2] = (pixel[2] as f32 * intensity) as u8;
    }

    frame
}


// fn fill_pixels(
//     frame: &CapturedFrame,
//     screen_width: u32, 
//     screen_height: u32) -> Vec<u8> {
//     let size = (screen_width * screen_height * 4) as usize;
//     let mut buf = vec![0u8; size];

//     let offset_x = (screen_width - frame.width) / 2;
//     let offset_y = (screen_height - frame.height) / 2;

//     for row in 0..frame.height {
//         let src_start = (row * frame.width * 4) as usize;
//         let src_end = src_start + (frame.width * 4) as usize;

//         let dst_start = ((offset_y + row) * screen_width * 4 + offset_x * 4) as usize;
//         let dst_end = dst_start + (frame.width * 4) as usize;

//         buf[dst_start..dst_end].copy_from_slice(&frame.pixels[src_start..src_end]);
//     }

//     buf
// }