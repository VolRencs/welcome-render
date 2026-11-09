use std::io::Cursor;

use tiny_skia::{IntSize, Pixmap};

use crate::error::{RenderError, RenderResult};

const MAX_ENCODED_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct ImageLimits {
    pub max_dimension: u32,
    pub max_pixels: u64,
}

impl Default for ImageLimits {
    fn default() -> Self {
        Self {
            max_dimension: 4096,
            max_pixels: 4096 * 4096,
        }
    }
}

impl ImageLimits {
    pub fn check(self, width: u32, height: u32) -> RenderResult<()> {
        if width == 0 || height == 0 {
            return Err(RenderError::image("image has zero dimensions"));
        }
        if width > self.max_dimension || height > self.max_dimension {
            return Err(RenderError::image_too_large(format!(
                "image is {width}x{height}, max dimension is {}",
                self.max_dimension
            )));
        }
        if u64::from(width) * u64::from(height) > self.max_pixels {
            return Err(RenderError::image_too_large(format!(
                "image is {width}x{height}, max pixel count is {}",
                self.max_pixels
            )));
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    /// Straight (not premultiplied) RGBA8.
    pub rgba: Vec<u8>,
}

impl DecodedImage {
    pub fn to_pixmap(&self) -> RenderResult<Pixmap> {
        let size = IntSize::from_wh(self.width, self.height)
            .ok_or_else(|| RenderError::internal("invalid decoded image size"))?;
        let mut rgba = self.rgba.clone();
        premultiply(&mut rgba);
        Pixmap::from_vec(rgba, size)
            .ok_or_else(|| RenderError::internal("failed to create pixmap from decoded image"))
    }
}

pub fn decode_image(data: &[u8], limits: ImageLimits) -> RenderResult<DecodedImage> {
    if data.is_empty() {
        return Err(RenderError::image("image data is empty"));
    }
    if data.len() > MAX_ENCODED_BYTES {
        return Err(RenderError::image_too_large(format!(
            "encoded image is {} bytes, max is {MAX_ENCODED_BYTES}",
            data.len()
        )));
    }

    if data.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return decode_png(data, limits);
    }
    if data.starts_with(&[0xff, 0xd8, 0xff]) {
        return decode_jpeg(data, limits);
    }
    if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        return decode_webp(data, limits);
    }

    Err(RenderError::image(
        "unsupported image format; expected PNG, JPEG or WebP",
    ))
}

fn decode_png(data: &[u8], limits: ImageLimits) -> RenderResult<DecodedImage> {
    use zune_png::PngDecoder;
    use zune_png::zune_core::bytestream::ZCursor;
    use zune_png::zune_core::colorspace::ColorSpace;
    use zune_png::zune_core::options::DecoderOptions;
    use zune_png::zune_core::result::DecodingResult;

    let options = DecoderOptions::default()
        .png_set_strip_to_8bit(true)
        .png_set_add_alpha_channel(true);
    let mut decoder = PngDecoder::new_with_options(ZCursor::new(data), options);
    decoder
        .decode_headers()
        .map_err(|error| RenderError::image(format!("PNG header error: {error}")))?;

    let (width, height) = decoder
        .dimensions()
        .ok_or_else(|| RenderError::image("PNG header has no dimensions"))?;
    let width = u32::try_from(width)
        .map_err(|_| RenderError::image_too_large("PNG width does not fit u32"))?;
    let height = u32::try_from(height)
        .map_err(|_| RenderError::image_too_large("PNG height does not fit u32"))?;
    limits.check(width, height)?;

    let colorspace = decoder
        .colorspace()
        .ok_or_else(|| RenderError::image("PNG header has no colorspace"))?;

    let decoded = decoder
        .decode()
        .map_err(|error| RenderError::image(format!("PNG decode error: {error}")))?;
    let raw = match decoded {
        DecodingResult::U8(raw) => raw,
        _ => return Err(RenderError::image("unexpected PNG decode depth")),
    };

    let rgba = match colorspace {
        ColorSpace::RGBA => raw,
        ColorSpace::RGB => rgb_to_rgba(raw),
        ColorSpace::Luma => luma_to_rgba(raw),
        ColorSpace::LumaA => luma_alpha_to_rgba(raw),
        ColorSpace::BGR => bgr_to_rgba(raw),
        ColorSpace::BGRA => bgra_to_rgba(raw),
        ColorSpace::ARGB => argb_to_rgba(raw),
        other => {
            return Err(RenderError::image(format!(
                "unsupported PNG colorspace: {other:?}"
            )));
        }
    };

    if rgba.len() != width as usize * height as usize * 4 {
        return Err(RenderError::image(
            "PNG decoder returned unexpected data size",
        ));
    }

    Ok(DecodedImage {
        width,
        height,
        rgba,
    })
}

fn decode_jpeg(data: &[u8], limits: ImageLimits) -> RenderResult<DecodedImage> {
    use zune_jpeg::JpegDecoder;
    use zune_jpeg::zune_core::bytestream::ZCursor;
    use zune_jpeg::zune_core::colorspace::ColorSpace;
    use zune_jpeg::zune_core::options::DecoderOptions;

    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(data), options);
    decoder
        .decode_headers()
        .map_err(|error| RenderError::image(format!("JPEG header error: {error}")))?;

    let info = decoder
        .info()
        .ok_or_else(|| RenderError::image("JPEG header has no dimensions"))?;
    let width = u32::from(info.width);
    let height = u32::from(info.height);
    limits.check(width, height)?;

    let rgba = decoder
        .decode()
        .map_err(|error| RenderError::image(format!("JPEG decode error: {error}")))?;
    if rgba.len() != width as usize * height as usize * 4 {
        return Err(RenderError::image(
            "JPEG decoder returned unexpected data size",
        ));
    }

    Ok(DecodedImage {
        width,
        height,
        rgba,
    })
}

fn decode_webp(data: &[u8], limits: ImageLimits) -> RenderResult<DecodedImage> {
    use image_webp::WebPDecoder;

    let mut decoder = WebPDecoder::new(Cursor::new(data))
        .map_err(|error| RenderError::image(format!("WebP header error: {error}")))?;

    let (width, height) = decoder.dimensions();
    limits.check(width, height)?;
    decoder.set_memory_limit(width as usize * height as usize * 4);

    let size = decoder
        .output_buffer_size()
        .ok_or_else(|| RenderError::image("WebP image is too large"))?;
    let mut buffer = vec![0_u8; size];
    decoder
        .read_image(&mut buffer)
        .map_err(|error| RenderError::image(format!("WebP decode error: {error}")))?;

    let rgba = if decoder.has_alpha() {
        buffer
    } else {
        rgb_to_rgba(buffer)
    };

    if rgba.len() != width as usize * height as usize * 4 {
        return Err(RenderError::image(
            "WebP decoder returned unexpected data size",
        ));
    }

    Ok(DecodedImage {
        width,
        height,
        rgba,
    })
}

fn premultiply(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
        let alpha = u32::from(pixel[3]);
        if alpha == 255 {
            continue;
        }
        pixel[0] = ((u32::from(pixel[0]) * alpha + 127) / 255) as u8;
        pixel[1] = ((u32::from(pixel[1]) * alpha + 127) / 255) as u8;
        pixel[2] = ((u32::from(pixel[2]) * alpha + 127) / 255) as u8;
    }
}

fn rgb_to_rgba(rgb: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgb.len() / 3 * 4);
    for pixel in rgb.chunks_exact(3) {
        out.extend_from_slice(pixel);
        out.push(255);
    }
    out
}

fn bgr_to_rgba(bgr: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(bgr.len() / 3 * 4);
    for pixel in bgr.chunks_exact(3) {
        out.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
    }
    out
}

fn bgra_to_rgba(bgra: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(bgra.len());
    for pixel in bgra.chunks_exact(4) {
        out.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
    }
    out
}

fn argb_to_rgba(argb: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(argb.len());
    for pixel in argb.chunks_exact(4) {
        out.extend_from_slice(&[pixel[1], pixel[2], pixel[3], pixel[0]]);
    }
    out
}

fn luma_to_rgba(luma: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(luma.len() * 4);
    for value in luma {
        out.extend_from_slice(&[value, value, value, 255]);
    }
    out
}

fn luma_alpha_to_rgba(luma_alpha: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(luma_alpha.len() / 2 * 4);
    for pixel in luma_alpha.chunks_exact(2) {
        out.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_data() {
        assert!(decode_image(&[], ImageLimits::default()).is_err());
    }

    #[test]
    fn rejects_unknown_format() {
        assert!(decode_image(b"definitely not an image", ImageLimits::default()).is_err());
    }

    #[test]
    fn limits_are_enforced() {
        let limits = ImageLimits {
            max_dimension: 100,
            max_pixels: 100 * 100,
        };
        assert!(limits.check(101, 10).is_err());
        assert!(limits.check(100, 101).is_err());
        assert!(limits.check(0, 10).is_err());
        assert!(limits.check(100, 100).is_ok());
    }
}
