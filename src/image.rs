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

#[derive(Debug, Clone, Copy)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
    pub format: &'static str,
    pub orientation: u8,
}

impl DecodedImage {
    pub fn into_oriented(self, orientation: u8) -> RenderResult<Self> {
        let (rgba, width, height) = orient_rgba(self.rgba, self.width, self.height, orientation);
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    pub fn into_pixmap(mut self) -> RenderResult<Pixmap> {
        let size = IntSize::from_wh(self.width, self.height)
            .ok_or_else(|| RenderError::internal("invalid decoded image size"))?;
        premultiply(&mut self.rgba);
        Pixmap::from_vec(self.rgba, size)
            .ok_or_else(|| RenderError::internal("failed to create pixmap from decoded image"))
    }
}

fn validate_encoded(data: &[u8]) -> RenderResult<()> {
    if data.is_empty() {
        return Err(RenderError::image("image data is empty"));
    }
    if data.len() > MAX_ENCODED_BYTES {
        return Err(RenderError::image_too_large(format!(
            "encoded image is {} bytes, max is {MAX_ENCODED_BYTES}",
            data.len()
        )));
    }
    Ok(())
}

fn is_png(data: &[u8]) -> bool {
    data.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a])
}

fn is_jpeg(data: &[u8]) -> bool {
    data.starts_with(&[0xff, 0xd8, 0xff])
}

fn is_webp(data: &[u8]) -> bool {
    data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP"
}

pub fn decode_image_raw(data: &[u8], limits: ImageLimits) -> RenderResult<(DecodedImage, u8)> {
    validate_encoded(data)?;

    if is_png(data) {
        return Ok((decode_png(data, limits)?, 1));
    }
    if is_jpeg(data) {
        return decode_jpeg(data, limits);
    }
    if is_webp(data) {
        return Ok((decode_webp(data, limits)?, 1));
    }

    Err(RenderError::image(
        "unsupported image format; expected PNG, JPEG or WebP",
    ))
}

pub fn decode_image(data: &[u8], limits: ImageLimits) -> RenderResult<DecodedImage> {
    let (image, orientation) = decode_image_raw(data, limits)?;
    image.into_oriented(orientation)
}

pub fn probe_image(data: &[u8]) -> RenderResult<ImageInfo> {
    validate_encoded(data)?;

    if is_png(data) {
        let (width, height) = probe_png(data)?;
        return Ok(ImageInfo {
            width,
            height,
            format: "png",
            orientation: 1,
        });
    }
    if is_jpeg(data) {
        let (width, height, orientation) = probe_jpeg(data)?;
        return Ok(ImageInfo {
            width,
            height,
            format: "jpeg",
            orientation,
        });
    }
    if is_webp(data) {
        let (width, height) = probe_webp(data)?;
        return Ok(ImageInfo {
            width,
            height,
            format: "webp",
            orientation: 1,
        });
    }

    Err(RenderError::image(
        "unsupported image format; expected PNG, JPEG or WebP",
    ))
}

fn probe_png(data: &[u8]) -> RenderResult<(u32, u32)> {
    use zune_png::PngDecoder;
    use zune_png::zune_core::bytestream::ZCursor;
    use zune_png::zune_core::options::DecoderOptions;

    let options = DecoderOptions::default();
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
    if width == 0 || height == 0 {
        return Err(RenderError::image("image has zero dimensions"));
    }
    Ok((width, height))
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

fn probe_jpeg(data: &[u8]) -> RenderResult<(u32, u32, u8)> {
    use zune_jpeg::JpegDecoder;
    use zune_jpeg::zune_core::bytestream::ZCursor;
    use zune_jpeg::zune_core::options::DecoderOptions;

    let options = DecoderOptions::default();
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(data), options);
    decoder
        .decode_headers()
        .map_err(|error| RenderError::image(format!("JPEG header error: {error}")))?;

    let info = decoder
        .info()
        .ok_or_else(|| RenderError::image("JPEG header has no dimensions"))?;
    let width = u32::from(info.width);
    let height = u32::from(info.height);
    if width == 0 || height == 0 {
        return Err(RenderError::image("image has zero dimensions"));
    }
    let orientation = decoder
        .exif()
        .map_or(1, |exif| parse_exif_orientation(exif));
    Ok(if swaps_dimensions(orientation) {
        (height, width, orientation)
    } else {
        (width, height, orientation)
    })
}

fn decode_jpeg(data: &[u8], limits: ImageLimits) -> RenderResult<(DecodedImage, u8)> {
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
    let orientation = decoder
        .exif()
        .map_or(1, |exif| parse_exif_orientation(exif));

    let rgba = decoder
        .decode()
        .map_err(|error| RenderError::image(format!("JPEG decode error: {error}")))?;
    if rgba.len() != width as usize * height as usize * 4 {
        return Err(RenderError::image(
            "JPEG decoder returned unexpected data size",
        ));
    }

    Ok((
        DecodedImage {
            width,
            height,
            rgba,
        },
        orientation,
    ))
}

fn probe_webp(data: &[u8]) -> RenderResult<(u32, u32)> {
    use image_webp::WebPDecoder;

    let decoder = WebPDecoder::new(Cursor::new(data))
        .map_err(|error| RenderError::image(format!("WebP header error: {error}")))?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 {
        return Err(RenderError::image("image has zero dimensions"));
    }
    Ok((width, height))
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

fn swap_u16(data: &[u8], offset: usize, little_endian: bool) -> Option<u16> {
    let bytes = data.get(offset..offset + 2)?;
    Some(if little_endian {
        u16::from_le_bytes([bytes[0], bytes[1]])
    } else {
        u16::from_be_bytes([bytes[0], bytes[1]])
    })
}

fn swap_u32(data: &[u8], offset: usize, little_endian: bool) -> Option<u32> {
    let bytes = data.get(offset..offset + 4)?;
    Some(if little_endian {
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    } else {
        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    })
}

pub fn parse_exif_orientation(data: &[u8]) -> u8 {
    let little_endian = match data.get(0..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return 1,
    };
    if swap_u16(data, 2, little_endian) != Some(42) {
        return 1;
    }
    let Some(ifd_offset) = swap_u32(data, 4, little_endian).map(|value| value as usize) else {
        return 1;
    };
    let Some(count) = swap_u16(data, ifd_offset, little_endian) else {
        return 1;
    };
    for index in 0..usize::from(count) {
        let entry = ifd_offset + 2 + index * 12;
        if swap_u16(data, entry, little_endian) != Some(0x0112) {
            continue;
        }
        let kind = swap_u16(data, entry + 2, little_endian);
        let value = match kind {
            Some(3) => swap_u16(data, entry + 8, little_endian).map(|value| value as u32),
            Some(4) => swap_u32(data, entry + 8, little_endian),
            _ => None,
        };
        return match value {
            Some(value @ 1..=8) => value as u8,
            _ => 1,
        };
    }
    1
}

pub(crate) fn swaps_dimensions(orientation: u8) -> bool {
    matches!(orientation, 5..=8)
}

pub(crate) fn orient_rgba(
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    orientation: u8,
) -> (Vec<u8>, u32, u32) {
    if !(2..=8).contains(&orientation) {
        return (rgba, width, height);
    }
    let (out_width, out_height) = if swaps_dimensions(orientation) {
        (height, width)
    } else {
        (width, height)
    };
    let mut output = vec![0_u8; rgba.len()];
    let source_stride = width as usize * 4;
    let target_stride = out_width as usize * 4;

    for y in 0..height as usize {
        let row = y * source_stride;
        for x in 0..width as usize {
            let (target_x, target_y) = match orientation {
                2 => (width as usize - 1 - x, y),
                3 => (width as usize - 1 - x, height as usize - 1 - y),
                4 => (x, height as usize - 1 - y),
                5 => (y, x),
                6 => (height as usize - 1 - y, x),
                7 => (height as usize - 1 - y, width as usize - 1 - x),
                8 => (y, width as usize - 1 - x),
                _ => (x, y),
            };
            let source = row + x * 4;
            let target = target_y * target_stride + target_x * 4;
            output[target..target + 4].copy_from_slice(&rgba[source..source + 4]);
        }
    }

    (output, out_width, out_height)
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

    #[test]
    fn reads_exif_orientation() {
        let mut data = Vec::new();
        data.extend_from_slice(b"MM");
        data.extend_from_slice(&42_u16.to_be_bytes());
        data.extend_from_slice(&8_u32.to_be_bytes());
        data.extend_from_slice(&1_u16.to_be_bytes());
        data.extend_from_slice(&0x0112_u16.to_be_bytes());
        data.extend_from_slice(&3_u16.to_be_bytes());
        data.extend_from_slice(&1_u32.to_be_bytes());
        data.extend_from_slice(&6_u16.to_be_bytes());
        data.extend_from_slice(&0_u16.to_be_bytes());
        data.extend_from_slice(&0_u32.to_be_bytes());
        assert_eq!(parse_exif_orientation(&data), 6);
    }

    #[test]
    fn orients_pixels() {
        let pixels = vec![1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255, 4, 0, 0, 255];
        let (rotated, width, height) = orient_rgba(pixels, 2, 2, 6);
        assert_eq!((width, height), (2, 2));
        assert_eq!(
            rotated,
            vec![3, 0, 0, 255, 1, 0, 0, 255, 4, 0, 0, 255, 2, 0, 0, 255]
        );
    }
}
