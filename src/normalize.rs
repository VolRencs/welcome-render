use crate::color::ColorU8;
use crate::error::{RenderError, RenderResult};
use crate::image::{ImageLimits, decode_image_raw, orient_rgba, swaps_dimensions};
use crate::png_encode::{encode_rgb_jpeg, encode_rgb_png};

const INPUT_MAX_DIMENSION: u32 = 16_384;
const INPUT_MAX_PIXELS: u64 = 64 * 1024 * 1024;

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

#[derive(Debug, Clone, Copy)]
pub struct NormalizeOptions {
    pub max_dimension: u32,
    pub max_pixels: u64,
    pub format: OutputFormat,
    pub quality: u8,
    pub background: ColorU8,
}

impl Default for NormalizeOptions {
    fn default() -> Self {
        Self {
            max_dimension: 2560,
            max_pixels: 4_000_000,
            format: OutputFormat::Jpeg,
            quality: 88,
            background: ColorU8::BASE,
        }
    }
}

#[derive(Debug)]
pub struct NormalizedImage {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub format: &'static str,
    pub original_width: u32,
    pub original_height: u32,
    pub orientation: u8,
}

pub fn normalize_image(data: &[u8], options: NormalizeOptions) -> RenderResult<NormalizedImage> {
    let (decoded, orientation) = decode_image_raw(
        data,
        ImageLimits {
            max_dimension: INPUT_MAX_DIMENSION,
            max_pixels: INPUT_MAX_PIXELS,
        },
    )?;

    let original_width = decoded.width;
    let original_height = decoded.height;
    let (oriented_width, oriented_height) = if swaps_dimensions(orientation) {
        (original_height, original_width)
    } else {
        (original_width, original_height)
    };
    let (width, height) = fit_dimensions(
        oriented_width,
        oriented_height,
        options.max_dimension,
        options.max_pixels,
    );
    let (resize_width, resize_height) = if swaps_dimensions(orientation) {
        (height, width)
    } else {
        (width, height)
    };

    let mut rgba = decoded.rgba;
    flatten_alpha(&mut rgba, options.background);
    let resized = resize_rgba(
        &rgba,
        original_width,
        original_height,
        resize_width,
        resize_height,
    );
    drop(rgba);
    let (oriented, width, height) = orient_rgba(resized, resize_width, resize_height, orientation);
    let rgb = rgba_to_rgb(&oriented);
    drop(oriented);
    let encoded = match options.format {
        OutputFormat::Png => encode_rgb_png(width, height, &rgb)?,
        OutputFormat::Jpeg => encode_rgb_jpeg(width, height, &rgb, options.quality)?,
    };

    Ok(NormalizedImage {
        data: encoded,
        width,
        height,
        format: options.format.as_str(),
        original_width,
        original_height,
        orientation,
    })
}

fn fit_dimensions(width: u32, height: u32, max_dimension: u32, max_pixels: u64) -> (u32, u32) {
    if width == 0 || height == 0 {
        return (1, 1);
    }
    let max_dimension = u64::from(max_dimension.max(1));
    let max_pixels = max_pixels.max(1);
    let pixels = u64::from(width) * u64::from(height);

    if u64::from(width) <= max_dimension
        && u64::from(height) <= max_dimension
        && pixels <= max_pixels
    {
        return (width, height);
    }

    let mut scale = 1.0_f64;
    scale = scale.min(max_dimension as f64 / f64::from(width));
    scale = scale.min(max_dimension as f64 / f64::from(height));
    scale = scale.min((max_pixels as f64 / pixels as f64).sqrt());

    let mut out_width = ((f64::from(width) * scale).floor() as u32).max(1);
    let mut out_height = ((f64::from(height) * scale).floor() as u32).max(1);

    while u64::from(out_width) * u64::from(out_height) > max_pixels {
        if out_width >= out_height && out_width > 1 {
            out_width -= 1;
        } else if out_height > 1 {
            out_height -= 1;
        } else {
            break;
        }
    }

    (out_width, out_height)
}

fn flatten_alpha(rgba: &mut [u8], background: ColorU8) {
    for pixel in rgba.chunks_exact_mut(4) {
        let alpha = u32::from(pixel[3]);
        if alpha == 255 {
            continue;
        }
        let inverse = 255 - alpha;
        pixel[0] =
            ((u32::from(pixel[0]) * alpha + u32::from(background.r) * inverse + 127) / 255) as u8;
        pixel[1] =
            ((u32::from(pixel[1]) * alpha + u32::from(background.g) * inverse + 127) / 255) as u8;
        pixel[2] =
            ((u32::from(pixel[2]) * alpha + u32::from(background.b) * inverse + 127) / 255) as u8;
        pixel[3] = 255;
    }
}

fn resize_rgba(
    source: &[u8],
    source_width: u32,
    source_height: u32,
    target_width: u32,
    target_height: u32,
) -> Vec<u8> {
    if source_width == target_width && source_height == target_height {
        return source.to_vec();
    }
    if target_width >= source_width && target_height >= source_height {
        return source.to_vec();
    }

    let source_width_usize = source_width as usize;
    let source_height_usize = source_height as usize;
    let target_width_usize = target_width as usize;
    let target_height_usize = target_height as usize;
    let mut output = vec![0_u8; target_width_usize * target_height_usize * 4];

    for target_y in 0..target_height_usize {
        let source_y0 = target_y * source_height_usize / target_height_usize;
        let source_y1 = ((target_y + 1) * source_height_usize)
            .div_ceil(target_height_usize)
            .max(source_y0 + 1)
            .min(source_height_usize);
        for target_x in 0..target_width_usize {
            let source_x0 = target_x * source_width_usize / target_width_usize;
            let source_x1 = ((target_x + 1) * source_width_usize)
                .div_ceil(target_width_usize)
                .max(source_x0 + 1)
                .min(source_width_usize);

            let mut red = 0_u64;
            let mut green = 0_u64;
            let mut blue = 0_u64;
            let mut count = 0_u64;
            for source_y in source_y0..source_y1 {
                let row = source_y * source_width_usize * 4;
                for source_x in source_x0..source_x1 {
                    let index = row + source_x * 4;
                    red += u64::from(source[index]);
                    green += u64::from(source[index + 1]);
                    blue += u64::from(source[index + 2]);
                    count += 1;
                }
            }

            let target = (target_y * target_width_usize + target_x) * 4;
            output[target] = ((red + count / 2) / count) as u8;
            output[target + 1] = ((green + count / 2) / count) as u8;
            output[target + 2] = ((blue + count / 2) / count) as u8;
            output[target + 3] = 255;
        }
    }

    output
}

fn rgba_to_rgb(rgba: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    for pixel in rgba.chunks_exact(4) {
        rgb.extend_from_slice(&pixel[0..3]);
    }
    rgb
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_dimensions() {
        assert_eq!(fit_dimensions(900, 480, 2560, 4_000_000), (900, 480));
        assert_eq!(fit_dimensions(6000, 4000, 2560, 4_000_000), (2449, 1632));
        assert_eq!(fit_dimensions(2560, 2560, 2560, 4_000_000), (2000, 2000));
    }

    #[test]
    fn resizes_without_upscaling() {
        let source = vec![255_u8; 4 * 4 * 4];
        assert_eq!(resize_rgba(&source, 4, 4, 4, 4), source);
        assert_eq!(resize_rgba(&source, 4, 4, 8, 8), source);
    }
}
