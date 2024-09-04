extern crate ffmpeg_the_third as ffmpeg;

use ffmpeg::format::Pixel;

#[derive(derive_more::Deref, derive_more::DerefMut)]
pub struct Rescaler(ffmpeg::software::scaling::Context);

unsafe impl std::marker::Send for Rescaler {}

pub fn rgba_rescaler_for_frame(frame: &ffmpeg::util::frame::Video) -> Rescaler {
    Rescaler(
        ffmpeg::software::scaling::Context::get(
            frame.format(),
            frame.width(),
            frame.height(),
            Pixel::RGB24,
            frame.width(),
            frame.height(),
            ffmpeg::software::scaling::Flags::BILINEAR,
        )
        .unwrap(),
    )
}

pub fn video_frame_to_pixel_buffer(
    frame: &ffmpeg::util::frame::Video,
) -> slint::SharedPixelBuffer<slint::Rgb8Pixel> {
    let mut pixel_buffer =
        slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(frame.width(), frame.height());
    let ffmpeg_line_iter = frame.data(0).chunks_exact(frame.stride(0));
    let slint_pixel_line_iter = pixel_buffer
        .make_mut_bytes()
        .chunks_mut(frame.width() as usize * std::mem::size_of::<slint::Rgb8Pixel>());
    for (source_line, dest_line) in ffmpeg_line_iter.zip(slint_pixel_line_iter) {
        dest_line.copy_from_slice(&source_line[..dest_line.len()])
    }
    pixel_buffer
}
