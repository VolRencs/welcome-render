mod cache;
mod color;
mod error;
mod font;
mod image;
mod normalize;
mod png_encode;
mod render;
mod scene;
mod text;

use std::sync::Arc;

use napi::bindgen_prelude::{AsyncTask, Buffer, Env, Task};
use napi_derive::napi;

use crate::color::ColorU8;
use crate::error::RenderError;
use crate::image::{ImageInfo, ImageLimits, probe_image as probe_image_data};
use crate::normalize::{NormalizeOptions, OutputFormat, normalize_image as normalize_image_data};
use crate::render::{RendererInner, RendererOptions};
use crate::scene::{EllipseData, OverlayData, SceneData, TextSpecData};

#[napi(object)]
pub struct NativeFonts {
    pub regular: Buffer,
    pub bold: Option<Buffer>,
    pub fallback: Option<Vec<Buffer>>,
}

#[napi(object)]
pub struct NativeRendererOptions {
    pub system_fonts: Option<bool>,
    pub cache_bytes: Option<u32>,
    pub max_dimension: Option<u32>,
    pub max_pixels: Option<u32>,
}

impl From<NativeRendererOptions> for RendererOptions {
    fn from(value: NativeRendererOptions) -> Self {
        let defaults = Self::default();
        let default_max_pixels = u32::try_from(defaults.limits.max_pixels).unwrap_or(u32::MAX);
        Self {
            system_fonts: value.system_fonts.unwrap_or(defaults.system_fonts),
            cache_bytes: value
                .cache_bytes
                .map_or(defaults.cache_bytes, |bytes| bytes as usize),
            limits: ImageLimits {
                max_dimension: value
                    .max_dimension
                    .unwrap_or(defaults.limits.max_dimension)
                    .max(1),
                max_pixels: u64::from(value.max_pixels.unwrap_or(default_max_pixels).max(1)),
            },
        }
    }
}

#[napi(object)]
pub struct NativeNormalizeOptions {
    pub max_dimension: Option<u32>,
    pub max_pixels: Option<u32>,
    pub format: Option<String>,
    pub quality: Option<u32>,
    pub background: Option<String>,
}

#[napi(object)]
pub struct NativeNormalizedImage {
    pub data: Buffer,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub original_width: u32,
    pub original_height: u32,
    pub orientation: u32,
}

#[napi(object)]
pub struct NativeImageInfo {
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub orientation: u32,
}

#[napi(object)]
pub struct NativeOverlay {
    pub color: String,
    pub opacity: f64,
}

#[napi(object)]
pub struct NativeEllipse {
    pub cx: f64,
    pub cy: f64,
    pub rx: f64,
    pub ry: f64,
}

#[napi(object)]
pub struct NativeText {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub size: f64,
    pub color: String,
    pub weight: Option<u32>,
}

#[napi(object)]
pub struct NativeScene {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub background: Option<Buffer>,
    pub background_key: Option<String>,
    pub overlay: Option<NativeOverlay>,
    pub avatar: Option<Buffer>,
    pub avatar_ellipse: Option<NativeEllipse>,
    pub texts: Vec<NativeText>,
    pub format: Option<String>,
    pub quality: Option<u32>,
    pub strict_background: Option<bool>,
}

impl NativeScene {
    fn into_scene(self) -> SceneData {
        SceneData {
            width: self.width,
            height: self.height,
            background: self.background.map(|buffer| buffer.to_vec()),
            background_key: self.background_key,
            overlay: self.overlay.map(|overlay| OverlayData {
                color: overlay.color,
                opacity: overlay.opacity as f32,
            }),
            avatar: self.avatar.map(|buffer| buffer.to_vec()),
            avatar_ellipse: self.avatar_ellipse.map(|ellipse| EllipseData {
                cx: ellipse.cx as f32,
                cy: ellipse.cy as f32,
                rx: ellipse.rx as f32,
                ry: ellipse.ry as f32,
            }),
            texts: self
                .texts
                .into_iter()
                .map(|text| TextSpecData {
                    text: text.text,
                    x: text.x as f32,
                    y: text.y as f32,
                    size: text.size as f32,
                    color: text.color,
                    bold: text.weight.unwrap_or(400) >= 600,
                })
                .collect(),
            format: self.format,
            quality: self.quality,
            strict_background: self.strict_background.unwrap_or(false),
        }
    }
}

#[napi(object)]
pub struct NativeStats {
    pub renders: f64,
    pub cache_hits: f64,
    pub cache_misses: f64,
    pub cache_bytes: f64,
    pub cache_entries: f64,
    pub background_failures: f64,
    pub avatar_failures: f64,
}

fn parse_normalize_options(
    options: Option<NativeNormalizeOptions>,
) -> napi::Result<NormalizeOptions> {
    let defaults = NormalizeOptions::default();
    let options = match options {
        Some(options) => options,
        None => return Ok(defaults),
    };
    let format = match options.format {
        Some(format) => OutputFormat::parse(&format).map_err(napi::Error::from)?,
        None => defaults.format,
    };
    let quality = match options.quality {
        Some(quality) if (1..=100).contains(&quality) => quality as u8,
        Some(quality) => {
            return Err(
                RenderError::invalid(format!("quality must be in 1..=100, got {quality}"))
                    .into_napi(),
            );
        }
        None => defaults.quality,
    };
    let background = match options.background {
        Some(background) => ColorU8::parse(&background).map_err(napi::Error::from)?,
        None => defaults.background,
    };
    Ok(NormalizeOptions {
        max_dimension: options
            .max_dimension
            .unwrap_or(defaults.max_dimension)
            .max(1),
        max_pixels: u64::from(
            options
                .max_pixels
                .unwrap_or(defaults.max_pixels as u32)
                .max(1),
        ),
        format,
        quality,
        background,
    })
}

#[napi]
pub fn probe_image(data: Buffer) -> napi::Result<NativeImageInfo> {
    let info = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| probe_image_data(&data)))
        .map_err(|_| RenderError::internal("image probe panicked").into_napi())?
        .map_err(napi::Error::from)?;
    Ok(image_info_to_native(info))
}

#[napi(ts_return_type = "Promise<NativeNormalizedImage>")]
pub fn normalize_image(
    data: Buffer,
    options: Option<NativeNormalizeOptions>,
) -> napi::Result<AsyncTask<NormalizeTask>> {
    let options = parse_normalize_options(options)?;
    Ok(AsyncTask::new(NormalizeTask {
        data: data.to_vec(),
        options,
    }))
}

fn image_info_to_native(info: ImageInfo) -> NativeImageInfo {
    NativeImageInfo {
        width: info.width,
        height: info.height,
        format: info.format.to_owned(),
        orientation: u32::from(info.orientation),
    }
}

#[napi(js_name = "NativeWelcomeRenderer")]
pub struct NativeWelcomeRenderer {
    inner: Arc<RendererInner>,
}

#[napi]
impl NativeWelcomeRenderer {
    #[napi(constructor)]
    pub fn new(fonts: NativeFonts, options: Option<NativeRendererOptions>) -> napi::Result<Self> {
        let options = options.map_or_else(RendererOptions::default, Into::into);
        let fallback = fonts
            .fallback
            .unwrap_or_default()
            .into_iter()
            .map(|buffer| buffer.to_vec())
            .collect();

        let inner = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            RendererInner::new(
                fonts.regular.to_vec(),
                fonts.bold.map(|buffer| buffer.to_vec()),
                fallback,
                options,
            )
        }))
        .map_err(|_| RenderError::internal("renderer initialization panicked").into_napi())?
        .map_err(napi::Error::from)?;

        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    #[napi]
    pub fn render(&self, scene: NativeScene) -> napi::Result<Buffer> {
        let scene = scene.into_scene();
        let output = guarded_render(&self.inner, &scene)?;
        Ok(Buffer::from(output))
    }

    #[napi(ts_return_type = "Promise<Buffer>")]
    pub fn render_async(&self, scene: NativeScene) -> AsyncTask<RenderTask> {
        AsyncTask::new(RenderTask {
            inner: Arc::clone(&self.inner),
            scene: scene.into_scene(),
        })
    }

    #[napi]
    pub fn clear_cache(&self) -> u32 {
        self.inner.clear_cache().min(u64::from(u32::MAX)) as u32
    }

    #[napi(getter)]
    pub fn stats(&self) -> NativeStats {
        let stats = self.inner.stats();
        NativeStats {
            renders: stats.renders as f64,
            cache_hits: stats.cache_hits as f64,
            cache_misses: stats.cache_misses as f64,
            cache_bytes: stats.cache_bytes as f64,
            cache_entries: stats.cache_entries as f64,
            background_failures: stats.background_failures as f64,
            avatar_failures: stats.avatar_failures as f64,
        }
    }

    #[napi]
    pub fn dispose(&self) {
        self.inner.clear_cache();
    }
}

fn guarded_render(inner: &RendererInner, scene: &SceneData) -> napi::Result<Vec<u8>> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| inner.render(scene))) {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(error)) => Err(error.into_napi()),
        Err(_) => Err(RenderError::internal("render panicked").into_napi()),
    }
}

pub struct RenderTask {
    inner: Arc<RendererInner>,
    scene: SceneData,
}

#[napi]
impl Task for RenderTask {
    type Output = Vec<u8>;
    type JsValue = Buffer;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        guarded_render(&self.inner, &self.scene)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(Buffer::from(output))
    }
}

pub struct NormalizeTask {
    data: Vec<u8>,
    options: NormalizeOptions,
}

#[napi]
impl Task for NormalizeTask {
    type Output = crate::normalize::NormalizedImage;
    type JsValue = NativeNormalizedImage;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            normalize_image_data(&self.data, self.options)
        }))
        .map_err(|_| RenderError::internal("image normalization panicked").into_napi())?
        .map_err(napi::Error::from)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(NativeNormalizedImage {
            data: Buffer::from(output.data),
            width: output.width,
            height: output.height,
            format: output.format.to_owned(),
            original_width: output.original_width,
            original_height: output.original_height,
            orientation: u32::from(output.orientation),
        })
    }
}
