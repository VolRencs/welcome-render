use tiny_skia::Pixmap;

use crate::error::{RenderError, RenderResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Png,
    Jpeg,
}

impl OutputFormat {
    pub fn parse(value: &str) -> RenderResult<Self> {
        match value {
            "png" => Ok(Self::Png),
            "jpeg" | "jpg" => Ok(Self::Jpeg),
            _ => Err(RenderError::invalid(format!(
                "unsupported output format {value:?}"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
        }
    }
}

pub fn parse_quality(value: u32) -> RenderResult<u8> {
    if (1..=100).contains(&value) {
        Ok(value as u8)
    } else {
        Err(RenderError::invalid(format!(
            "quality must be in 1..=100, got {value}"
        )))
    }
}

/// Converts straight RGBA8 pixels to tightly packed RGB8 pixels.
pub fn rgba_to_rgb(rgba: &[u8]) -> Vec<u8> {
    let (pixels, rest) = rgba.as_chunks::<4>();
    debug_assert!(rest.is_empty(), "RGBA buffer must be pixel aligned");
    let mut rgb = Vec::with_capacity(pixels.len() * 3);
    for pixel in pixels {
        rgb.extend_from_slice(&pixel[0..3]);
    }
    rgb
}

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
fn pixmap_to_rgb(pixmap: &Pixmap) -> Vec<u8> {
    let data = pixmap.data();
    let (pixels, rest) = data.as_chunks::<4>();
    debug_assert!(rest.is_empty(), "pixmap data must be pixel aligned");
    debug_assert!(
        pixels.iter().all(|pixel| pixel[3] == 255),
        "renderer canvas must stay opaque"
    );
    let mut rgb = Vec::with_capacity(pixels.len() * 3);
    for pixel in pixels {
        rgb.extend_from_slice(&pixel[0..3]);
    }
    rgb
}

pub fn encode_rgb_png_pixmap(pixmap: &Pixmap) -> RenderResult<Vec<u8>> {
    encode_rgb_png(pixmap.width(), pixmap.height(), &pixmap_to_rgb(pixmap))
}

pub fn encode_rgb_jpeg_pixmap(pixmap: &Pixmap, quality: u8) -> RenderResult<Vec<u8>> {
    encode_rgb_jpeg(
        pixmap.width(),
        pixmap.height(),
        &pixmap_to_rgb(pixmap),
        quality,
    )
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

    #[test]
    fn parses_quality_range() {
        assert_eq!(parse_quality(1).unwrap(), 1);
        assert_eq!(parse_quality(100).unwrap(), 100);
        assert!(parse_quality(0).is_err());
        assert!(parse_quality(101).is_err());
    }

    #[test]
    fn converts_rgba_to_rgb() {
        assert_eq!(
            rgba_to_rgb(&[1, 2, 3, 255, 4, 5, 6, 128]),
            vec![1, 2, 3, 4, 5, 6]
        );
    }
}
