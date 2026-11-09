use tiny_skia::Pixmap;

use crate::error::{RenderError, RenderResult};

pub fn encode_rgb_png(width: u32, height: u32, rgb: &[u8]) -> RenderResult<Vec<u8>> {
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Balanced);
        let mut writer = encoder
            .write_header()
            .map_err(|error| RenderError::encode(format!("PNG header: {error}")))?;
        writer
            .write_image_data(rgb)
            .map_err(|error| RenderError::encode(format!("PNG data: {error}")))?;
        writer
            .finish()
            .map_err(|error| RenderError::encode(format!("PNG finish: {error}")))?;
    }

    Ok(output)
}

pub fn encode_rgb_jpeg(width: u32, height: u32, rgb: &[u8], quality: u8) -> RenderResult<Vec<u8>> {
    let width =
        u16::try_from(width).map_err(|_| RenderError::jpeg("JPEG width does not fit u16"))?;
    let height =
        u16::try_from(height).map_err(|_| RenderError::jpeg("JPEG height does not fit u16"))?;
    let mut output = Vec::new();
    let encoder = jpeg_encoder::Encoder::new(&mut output, quality);
    encoder
        .encode(rgb, width, height, jpeg_encoder::ColorType::Rgb)
        .map_err(|error| RenderError::jpeg(format!("JPEG encode: {error}")))?;
    Ok(output)
}

/// Encodes an opaque premultiplied RGBA pixmap as an RGB PNG.
///
/// The renderer always composites onto an opaque base, so the alpha channel is
/// redundant. Dropping it makes the encoder cheaper and the file smaller.
pub fn encode_rgb_png_pixmap(pixmap: &Pixmap) -> RenderResult<Vec<u8>> {
    let width = pixmap.width();
    let height = pixmap.height();
    let data = pixmap.data();

    let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
    for pixel in data.chunks_exact(4) {
        debug_assert_eq!(pixel[3], 255, "renderer canvas must stay opaque");
        rgb.extend_from_slice(&pixel[0..3]);
    }

    encode_rgb_png(width, height, &rgb)
}

pub fn encode_rgb_jpeg_pixmap(pixmap: &Pixmap, quality: u8) -> RenderResult<Vec<u8>> {
    let width = pixmap.width();
    let height = pixmap.height();
    let data = pixmap.data();

    let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
    for pixel in data.chunks_exact(4) {
        debug_assert_eq!(pixel[3], 255, "renderer canvas must stay opaque");
        rgb.extend_from_slice(&pixel[0..3]);
    }

    encode_rgb_jpeg(width, height, &rgb, quality)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::{Color, Pixmap};

    #[test]
    fn encodes_opaque_rgb_png() {
        let mut pixmap = Pixmap::new(2, 2).unwrap();
        pixmap.fill(Color::from_rgba8(10, 20, 30, 255));
        let png = encode_rgb_png_pixmap(&pixmap).unwrap();
        assert_eq!(
            &png[0..8],
            &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
        );
        assert_eq!(png[25], 2, "expected PNG color type RGB");
    }

    #[test]
    fn encodes_rgb_jpeg() {
        let rgb = vec![10_u8; 2 * 2 * 3];
        let jpeg = encode_rgb_jpeg(2, 2, &rgb, 90).unwrap();
        assert_eq!(&jpeg[0..3], &[0xff, 0xd8, 0xff]);
    }
}
