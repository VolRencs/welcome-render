use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tiny_skia::{
    Color, FillRule, FilterQuality, Mask, PathBuilder, Pixmap, PixmapPaint, Rect, Transform,
};

use crate::cache::PixmapCache;
use crate::color::ColorU8;
use crate::error::{RenderError, RenderResult};
use crate::font::FontStore;
use crate::image::{DecodedImage, ImageLimits, decode_image};
use crate::normalize::OutputFormat;
use crate::png_encode::{encode_rgb_jpeg_pixmap, encode_rgb_png_pixmap};
use crate::scene::{EllipseData, SceneData};
use crate::text::TextEngine;

const DESIGN_WIDTH: u32 = 900;
const DESIGN_HEIGHT: u32 = 480;
const DEFAULT_CACHE_BYTES: usize = 64 * 1024 * 1024;
const MAX_TEXTS: usize = 16;

#[derive(Debug, Clone, Copy)]
pub struct RendererOptions {
    pub system_fonts: bool,
    pub cache_bytes: usize,
    pub limits: ImageLimits,
}

impl Default for RendererOptions {
    fn default() -> Self {
        Self {
            system_fonts: true,
            cache_bytes: DEFAULT_CACHE_BYTES,
            limits: ImageLimits::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RendererStats {
    pub renders: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub cache_bytes: u64,
    pub cache_entries: u64,
    pub background_failures: u64,
    pub avatar_failures: u64,
}

pub struct RendererInner {
    fonts: FontStore,
    text: TextEngine,
    cache: PixmapCache,
    limits: ImageLimits,
    renders: AtomicU64,
    background_failures: AtomicU64,
    avatar_failures: AtomicU64,
}

impl RendererInner {
    pub fn new(
        regular: Vec<u8>,
        bold: Option<Vec<u8>>,
        fallback: Vec<Vec<u8>>,
        options: RendererOptions,
    ) -> RenderResult<Self> {
        let fonts = FontStore::new(regular, bold, fallback, options.system_fonts)?;
        Ok(Self {
            fonts,
            text: TextEngine::new(),
            cache: PixmapCache::new(options.cache_bytes),
            limits: options.limits,
            renders: AtomicU64::new(0),
            background_failures: AtomicU64::new(0),
            avatar_failures: AtomicU64::new(0),
        })
    }

    pub fn render(&self, scene: &SceneData) -> RenderResult<Vec<u8>> {
        self.renders.fetch_add(1, Ordering::Relaxed);

        if scene.texts.len() > MAX_TEXTS {
            return Err(RenderError::invalid(format!(
                "at most {MAX_TEXTS} text layers are supported, got {}",
                scene.texts.len()
            )));
        }

        // When the caller knows the canvas size, a warm cache hit must not decode
        // the background at all. Without explicit dimensions we have to decode it
        // to discover the size.
        let (width, height, mut background, mut background_failed, mut background_error) =
            match (scene.width, scene.height) {
                (Some(width), Some(height)) => (width, height, None, false, None),
                (None, None) => {
                    let (decoded, failed, error) = match scene.background.as_deref() {
                        Some(data) => match decode_image(data, self.limits) {
                            Ok(background) => (Some(background), false, None),
                            Err(error) => (None, true, Some(error)),
                        },
                        None => (None, false, None),
                    };
                    let dimensions = decoded
                        .as_ref()
                        .map_or((DESIGN_WIDTH, DESIGN_HEIGHT), |background| {
                            (background.width, background.height)
                        });
                    (dimensions.0, dimensions.1, decoded, failed, error)
                }
                _ => {
                    return Err(RenderError::invalid(
                        "width and height must be provided together",
                    ));
                }
            };

        if width == 0 || height == 0 {
            return Err(RenderError::invalid("canvas dimensions must be positive"));
        }
        let (width, height) = fit_to_limits(
            width,
            height,
            self.limits.max_dimension,
            self.limits.max_pixels,
        );

        if background_failed {
            self.background_failures.fetch_add(1, Ordering::Relaxed);
            if scene.strict_background {
                return Err(strict_background_error(scene, background_error));
            }
        }

        let (overlay_color, overlay_opacity) = overlay_params(scene)?;
        let cache_key = base_cache_key(scene, width, height, overlay_color, overlay_opacity);

        let cached = cache_key.as_ref().and_then(|key| self.cache.get(key));

        let base = match cached {
            Some(pixmap) => pixmap,
            None => {
                if background.is_none()
                    && !background_failed
                    && let Some(data) = scene.background.as_deref()
                {
                    match decode_image(data, self.limits) {
                        Ok(decoded) => background = Some(decoded),
                        Err(error) => {
                            background_failed = true;
                            background_error = Some(error);
                            self.background_failures.fetch_add(1, Ordering::Relaxed);
                            if scene.strict_background {
                                return Err(strict_background_error(scene, background_error));
                            }
                        }
                    }
                }
                let built = Arc::new(self.build_base(
                    width,
                    height,
                    background,
                    overlay_color,
                    overlay_opacity,
                )?);
                if let Some(key) = cache_key
                    && !background_failed
                {
                    self.cache.insert_arc(key, Arc::clone(&built));
                }
                built
            }
        };

        let mut canvas = (*base).clone();

        if let Some(avatar) = scene.avatar.as_deref()
            && let Some(ellipse) = scene.avatar_ellipse
            && self.draw_avatar(&mut canvas, avatar, ellipse).is_err()
        {
            self.avatar_failures.fetch_add(1, Ordering::Relaxed);
        }

        for text in &scene.texts {
            self.text.draw_text(&self.fonts, &mut canvas, text)?;
        }

        match output_format(scene)? {
            OutputFormat::Png => encode_rgb_png_pixmap(&canvas),
            OutputFormat::Jpeg => encode_rgb_jpeg_pixmap(&canvas, output_quality(scene)?),
        }
    }

    pub fn clear_cache(&self) -> u64 {
        self.cache.clear()
    }

    #[must_use]
    pub fn stats(&self) -> RendererStats {
        let cache = self.cache.stats();
        RendererStats {
            renders: self.renders.load(Ordering::Relaxed),
            cache_hits: cache.hits,
            cache_misses: cache.misses,
            cache_bytes: cache.bytes,
            cache_entries: cache.entries,
            background_failures: self.background_failures.load(Ordering::Relaxed),
            avatar_failures: self.avatar_failures.load(Ordering::Relaxed),
        }
    }

    fn build_base(
        &self,
        width: u32,
        height: u32,
        background: Option<DecodedImage>,
        overlay_color: ColorU8,
        overlay_opacity: f32,
    ) -> RenderResult<Pixmap> {
        let mut canvas = Pixmap::new(width, height)
            .ok_or_else(|| RenderError::internal("failed to allocate base canvas"))?;

        match background {
            Some(background) if background.width == width && background.height == height => {
                composite_background_exact(&mut canvas, &background);
            }
            Some(background) => {
                let background_width = background.width;
                let background_height = background.height;
                canvas.fill(Color::from_rgba8(
                    ColorU8::BASE.r,
                    ColorU8::BASE.g,
                    ColorU8::BASE.b,
                    255,
                ));
                let source = background.into_pixmap()?;
                let scale_x = width as f32 / background_width as f32;
                let scale_y = height as f32 / background_height as f32;
                let scale = scale_x.max(scale_y);
                let draw_width = background_width as f32 * scale;
                let draw_height = background_height as f32 * scale;
                let dx = (width as f32 - draw_width) / 2.0;
                let dy = (height as f32 - draw_height) / 2.0;

                let paint = PixmapPaint {
                    quality: FilterQuality::Bilinear,
                    ..Default::default()
                };
                canvas.draw_pixmap(
                    0,
                    0,
                    source.as_ref(),
                    &paint,
                    Transform::from_row(scale, 0.0, 0.0, scale, dx, dy),
                    None,
                );
            }
            None => fill_gradient(&mut canvas),
        }

        apply_overlay(&mut canvas, overlay_color, overlay_opacity);
        Ok(canvas)
    }

    fn draw_avatar(
        &self,
        canvas: &mut Pixmap,
        data: &[u8],
        ellipse: EllipseData,
    ) -> RenderResult<()> {
        if !ellipse.cx.is_finite()
            || !ellipse.cy.is_finite()
            || !ellipse.rx.is_finite()
            || !ellipse.ry.is_finite()
            || ellipse.rx <= 0.0
            || ellipse.ry <= 0.0
        {
            return Err(RenderError::invalid(
                "avatar ellipse must have finite positive radii",
            ));
        }

        let left = ellipse.cx - ellipse.rx;
        let top = ellipse.cy - ellipse.ry;
        let right = ellipse.cx + ellipse.rx;
        let bottom = ellipse.cy + ellipse.ry;
        if right <= 0.0
            || bottom <= 0.0
            || left >= canvas.width() as f32
            || top >= canvas.height() as f32
        {
            return Ok(());
        }

        let target_width = (ellipse.rx * 2.0).ceil();
        let target_height = (ellipse.ry * 2.0).ceil();
        if !target_width.is_finite()
            || !target_height.is_finite()
            || target_width < 1.0
            || target_height < 1.0
        {
            return Err(RenderError::invalid("avatar ellipse is too small"));
        }

        let target_width = target_width as u32;
        let target_height = target_height as u32;
        if target_width > self.limits.max_dimension || target_height > self.limits.max_dimension {
            return Err(RenderError::image_too_large(format!(
                "avatar target is {target_width}x{target_height}, max dimension is {}",
                self.limits.max_dimension
            )));
        }
        if u64::from(target_width) * u64::from(target_height) > self.limits.max_pixels {
            return Err(RenderError::image_too_large("avatar target is too large"));
        }

        let image = decode_image(data, self.limits)?;
        let image_width = image.width;
        let image_height = image.height;
        let source = image.into_pixmap()?;
        let mut avatar = Pixmap::new(target_width, target_height)
            .ok_or_else(|| RenderError::internal("failed to allocate avatar canvas"))?;

        let target_w = target_width as f32;
        let target_h = target_height as f32;
        let scale = (target_w / image_width as f32).max(target_h / image_height as f32);
        let draw_width = image_width as f32 * scale;
        let draw_height = image_height as f32 * scale;
        let dx = (target_w - draw_width) / 2.0;
        let dy = (target_h - draw_height) / 2.0;

        let paint = PixmapPaint {
            quality: FilterQuality::Bilinear,
            ..Default::default()
        };
        avatar.draw_pixmap(
            0,
            0,
            source.as_ref(),
            &paint,
            Transform::from_row(scale, 0.0, 0.0, scale, dx, dy),
            None,
        );

        let oval = Rect::from_ltrb(0.0, 0.0, target_w, target_h)
            .ok_or_else(|| RenderError::internal("invalid avatar oval"))?;
        let path = PathBuilder::from_oval(oval)
            .ok_or_else(|| RenderError::internal("failed to build avatar ellipse"))?;
        let mut mask = Mask::new(target_width, target_height)
            .ok_or_else(|| RenderError::internal("failed to allocate avatar mask"))?;
        mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
        avatar.apply_mask(&mask);

        let x = ellipse.cx - target_w / 2.0;
        let y = ellipse.cy - target_h / 2.0;
        let draw_paint = PixmapPaint {
            quality: FilterQuality::Bilinear,
            ..Default::default()
        };
        canvas.draw_pixmap(
            0,
            0,
            avatar.as_ref(),
            &draw_paint,
            Transform::from_translate(x, y),
            None,
        );

        Ok(())
    }
}

fn fit_to_limits(width: u32, height: u32, max_dimension: u32, max_pixels: u64) -> (u32, u32) {
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

fn overlay_params(scene: &SceneData) -> RenderResult<(ColorU8, f32)> {
    match &scene.overlay {
        Some(overlay) => {
            let color = ColorU8::parse(&overlay.color)?;
            if !overlay.opacity.is_finite() {
                return Err(RenderError::invalid("overlay opacity must be finite"));
            }
            Ok((color, overlay.opacity.clamp(0.0, 1.0)))
        }
        None => Ok((ColorU8::BLACK, 0.25)),
    }
}

fn base_cache_key(
    scene: &SceneData,
    width: u32,
    height: u32,
    overlay_color: ColorU8,
    overlay_opacity: f32,
) -> Option<String> {
    let suffix = format!(
        "{width}x{height}|{}|{}",
        overlay_color.to_hex(),
        overlay_opacity.to_bits()
    );
    match (&scene.background, &scene.background_key) {
        (None, _) => Some(format!("gradient|{suffix}")),
        (Some(data), Some(key)) => Some(format!("{key}|{}|{suffix}", data.len())),
        (Some(_), None) => None,
    }
}

fn strict_background_error(scene: &SceneData, error: Option<RenderError>) -> RenderError {
    let key = scene.background_key.as_deref().unwrap_or("unnamed");
    let detail = error.map_or_else(|| "unknown error".to_owned(), |error| error.to_string());
    RenderError::image(format!("background {key}: {detail}"))
}

fn output_format(scene: &SceneData) -> RenderResult<OutputFormat> {
    match scene.format.as_deref() {
        None | Some("png") => Ok(OutputFormat::Png),
        Some("jpeg" | "jpg") => Ok(OutputFormat::Jpeg),
        Some(other) => Err(RenderError::invalid(format!(
            "unsupported output format {other:?}"
        ))),
    }
}

fn output_quality(scene: &SceneData) -> RenderResult<u8> {
    match scene.quality {
        None => Ok(90),
        Some(quality) if (1..=100).contains(&quality) => Ok(quality as u8),
        Some(quality) => Err(RenderError::invalid(format!(
            "quality must be in 1..=100, got {quality}"
        ))),
    }
}

fn composite_background_exact(canvas: &mut Pixmap, background: &DecodedImage) {
    let data = canvas.data_mut();
    let base = ColorU8::BASE;
    for (source, target) in background
        .rgba
        .chunks_exact(4)
        .zip(data.chunks_exact_mut(4))
    {
        let alpha = u32::from(source[3]);
        if alpha == 255 {
            target.copy_from_slice(source);
            continue;
        }
        if alpha == 0 {
            target.copy_from_slice(&[base.r, base.g, base.b, 255]);
            continue;
        }
        let inverse = 255 - alpha;
        target[0] =
            ((u32::from(source[0]) * alpha + u32::from(base.r) * inverse + 127) / 255) as u8;
        target[1] =
            ((u32::from(source[1]) * alpha + u32::from(base.g) * inverse + 127) / 255) as u8;
        target[2] =
            ((u32::from(source[2]) * alpha + u32::from(base.b) * inverse + 127) / 255) as u8;
        target[3] = 255;
    }
}

fn fill_gradient(canvas: &mut Pixmap) {
    let width = canvas.width();
    let height = canvas.height();
    let data = canvas.data_mut();
    let start = ColorU8::BASE;
    let end = ColorU8::GRADIENT_END;

    let mut row = Vec::with_capacity(width as usize * 4);
    for x in 0..width {
        let t = if width <= 1 {
            0.0
        } else {
            x as f32 / (width - 1) as f32
        };
        row.push(lerp_u8(start.r, end.r, t));
        row.push(lerp_u8(start.g, end.g, t));
        row.push(lerp_u8(start.b, end.b, t));
        row.push(255);
    }

    for y in 0..height {
        let offset = y as usize * width as usize * 4;
        data[offset..offset + row.len()].copy_from_slice(&row);
    }
}

fn apply_overlay(canvas: &mut Pixmap, color: ColorU8, opacity: f32) {
    let alpha = (opacity * f32::from(color.a)).round() as u32;
    if alpha == 0 {
        return;
    }
    let inverse = 255 - alpha;

    if color.r == 0 && color.g == 0 && color.b == 0 {
        for pixel in canvas.data_mut().chunks_exact_mut(4) {
            pixel[0] = ((u32::from(pixel[0]) * inverse + 127) / 255) as u8;
            pixel[1] = ((u32::from(pixel[1]) * inverse + 127) / 255) as u8;
            pixel[2] = ((u32::from(pixel[2]) * inverse + 127) / 255) as u8;
            pixel[3] = 255;
        }
        return;
    }

    for pixel in canvas.data_mut().chunks_exact_mut(4) {
        pixel[0] = ((u32::from(pixel[0]) * inverse + u32::from(color.r) * alpha + 127) / 255) as u8;
        pixel[1] = ((u32::from(pixel[1]) * inverse + u32::from(color.g) * alpha + 127) / 255) as u8;
        pixel[2] = ((u32::from(pixel[2]) * inverse + u32::from(color.b) * alpha + 127) / 255) as u8;
        pixel[3] = 255;
    }
}

fn lerp_u8(from: u8, to: u8, t: f32) -> u8 {
    (f32::from(from) + (f32::from(to) - f32::from(from)) * t).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_canvas_that_fits() {
        assert_eq!(fit_to_limits(900, 480, 4096, 16_777_216), (900, 480));
        assert_eq!(fit_to_limits(0, 10, 4096, 16_777_216), (1, 1));
    }

    #[test]
    fn clamps_by_dimension() {
        assert_eq!(fit_to_limits(1920, 1080, 320, u64::MAX), (320, 180));
        assert_eq!(fit_to_limits(8000, 4000, 4096, u64::MAX), (4096, 2048));
    }

    #[test]
    fn clamps_by_pixel_budget() {
        assert_eq!(fit_to_limits(20, 20, 4096, 100), (10, 10));
        let (width, height) = fit_to_limits(8000, 4000, 4096, 1_000_000);
        assert!(u64::from(width) * u64::from(height) <= 1_000_000);
    }

    #[test]
    fn gradient_row_is_opaque() {
        let mut pixmap = Pixmap::new(2, 1).unwrap();
        fill_gradient(&mut pixmap);
        assert_eq!(pixmap.data()[3], 255);
        assert_eq!(pixmap.data()[7], 255);
    }
}
