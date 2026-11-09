use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rustybuzz::{Direction, UnicodeBuffer, shape};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};
use unicode_bidi::BidiInfo;

use crate::color::ColorU8;
use crate::error::{RenderError, RenderResult};
use crate::font::FontStore;
use crate::scene::TextSpecData;

const MAX_TEXT_CHARS: usize = 4096;
const MAX_LAYOUT_CACHE: usize = 256;

#[derive(Debug, Clone)]
struct ShapedGlyph {
    /// Index into the font chain used for this text run.
    face: usize,
    glyph_id: u16,
    /// size / units_per_em for the face that produced this glyph.
    scale: f32,
    x_offset: f32,
    y_offset: f32,
    x_advance: f32,
}

#[derive(Debug, Clone, Default)]
struct TextLayout {
    glyphs: Vec<ShapedGlyph>,
    width: f32,
}

#[derive(Debug, Hash, PartialEq, Eq)]
struct TextKey {
    text: String,
    size_bits: u32,
    bold: bool,
}

pub struct TextEngine {
    cache: Mutex<HashMap<TextKey, Arc<TextLayout>>>,
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEngine {
    #[must_use]
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn draw_text(
        &self,
        fonts: &FontStore,
        canvas: &mut Pixmap,
        spec: &TextSpecData,
    ) -> RenderResult<()> {
        if !spec.x.is_finite() || !spec.y.is_finite() {
            return Err(RenderError::invalid("text position must be finite"));
        }
        let layout = self.layout(fonts, &spec.text, spec.size, spec.bold)?;
        if layout.glyphs.is_empty() {
            return Ok(());
        }
        let color = ColorU8::parse(&spec.color)?;
        if color.a == 0 {
            return Ok(());
        }

        let mut builder =
            PathBuilder::with_capacity(layout.glyphs.len() * 4, layout.glyphs.len() * 8);
        let mut pen_x = spec.x - layout.width / 2.0;

        for glyph in &layout.glyphs {
            let face_id = fonts.face_id(glyph.face, spec.bold);
            let scale = glyph.scale;
            let tx = pen_x + glyph.x_offset;
            let ty = spec.y - glyph.y_offset;

            fonts
                .database()
                .with_face_data(face_id, |data, face_index| {
                    if let Ok(face) = ttf_parser::Face::parse(data, face_index) {
                        let mut outline = OutlineToPath {
                            builder: &mut builder,
                            scale,
                            tx,
                            ty,
                        };
                        let _ =
                            face.outline_glyph(ttf_parser::GlyphId(glyph.glyph_id), &mut outline);
                    }
                });

            pen_x += glyph.x_advance;
        }

        let Some(path) = builder.finish() else {
            return Ok(());
        };

        let mut paint = Paint {
            anti_alias: true,
            ..Default::default()
        };
        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
        canvas.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
        Ok(())
    }

    fn layout(
        &self,
        fonts: &FontStore,
        text: &str,
        size: f32,
        bold: bool,
    ) -> RenderResult<Arc<TextLayout>> {
        if !size.is_finite() || size <= 0.0 || size > 4096.0 {
            return Err(RenderError::invalid(format!(
                "text size must be in (0, 4096], got {size}"
            )));
        }

        let normalized = normalize_text(text);
        if normalized.chars().count() > MAX_TEXT_CHARS {
            return Err(RenderError::invalid(format!(
                "text is longer than {MAX_TEXT_CHARS} characters"
            )));
        }

        let key = TextKey {
            text: normalized.clone(),
            size_bits: size.to_bits(),
            bold,
        };

        {
            let cache = self
                .cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(layout) = cache.get(&key) {
                return Ok(Arc::clone(layout));
            }
        }

        let layout = Arc::new(self.build_layout(fonts, &normalized, size, bold)?);
        {
            let mut cache = self
                .cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if cache.len() >= MAX_LAYOUT_CACHE {
                cache.clear();
            }
            cache.insert(key, Arc::clone(&layout));
        }
        Ok(layout)
    }

    fn build_layout(
        &self,
        fonts: &FontStore,
        text: &str,
        size: f32,
        bold: bool,
    ) -> RenderResult<TextLayout> {
        if text.is_empty() {
            return Ok(TextLayout::default());
        }

        let mut glyphs = Vec::new();
        // SVG text has an LTR base direction unless `direction` is set. Matching
        // that keeps mixed RTL/LTR lines consistent with the browser preview.
        let bidi = BidiInfo::new(text, Some(unicode_bidi::Level::ltr()));

        for paragraph in &bidi.paragraphs {
            let (_, runs) = bidi.visual_runs(paragraph, paragraph.range.clone());
            for run in runs {
                if run.start >= run.end || run.end > text.len() {
                    continue;
                }
                let rtl = bidi
                    .levels
                    .get(run.start)
                    .copied()
                    .is_some_and(|level| level.is_rtl());
                self.shape_run(fonts, &text[run], rtl, size, bold, &mut glyphs)?;
            }
        }

        let width: f32 = glyphs.iter().map(|glyph| glyph.x_advance).sum();
        let (glyphs, width) = if width < 0.0 {
            let mut reversed = glyphs;
            reversed.reverse();
            for glyph in &mut reversed {
                glyph.x_advance = -glyph.x_advance;
            }
            (reversed, -width)
        } else {
            (glyphs, width)
        };

        Ok(TextLayout { glyphs, width })
    }

    fn shape_run(
        &self,
        fonts: &FontStore,
        text: &str,
        rtl: bool,
        size: f32,
        bold: bool,
        output: &mut Vec<ShapedGlyph>,
    ) -> RenderResult<()> {
        if text.is_empty() {
            return Ok(());
        }

        let mut segments: Vec<(usize, String)> = Vec::new();
        for ch in text.chars() {
            let face = fonts.face_for_char(ch, bold);
            if let Some((current_face, buffer)) = segments.last_mut()
                && *current_face == face
            {
                buffer.push(ch);
                continue;
            }
            let mut buffer = String::new();
            buffer.push(ch);
            segments.push((face, buffer));
        }

        if rtl {
            segments.reverse();
        }

        for (face, segment) in segments {
            let glyphs = self.shape_segment(fonts, &segment, face, size, bold, rtl)?;
            output.extend(glyphs);
        }

        Ok(())
    }

    fn shape_segment(
        &self,
        fonts: &FontStore,
        text: &str,
        face_index: usize,
        size: f32,
        bold: bool,
        rtl: bool,
    ) -> RenderResult<Vec<ShapedGlyph>> {
        let face_id = fonts.face_id(face_index, bold);
        let result = fonts
            .database()
            .with_face_data(face_id, |data, index| -> RenderResult<Vec<ShapedGlyph>> {
                let face = rustybuzz::Face::from_slice(data, index)
                    .ok_or_else(|| RenderError::font("failed to parse shaped font face"))?;
                let units_per_em = face.units_per_em();
                if units_per_em <= 0 {
                    return Err(RenderError::font("font has invalid units per em"));
                }
                let scale = size / units_per_em as f32;

                let mut buffer = UnicodeBuffer::new();
                buffer.push_str(text);
                buffer.guess_segment_properties();
                buffer.set_direction(if rtl {
                    Direction::RightToLeft
                } else {
                    Direction::LeftToRight
                });

                let shaped = shape(&face, &[], buffer);
                let infos = shaped.glyph_infos();
                let positions = shaped.glyph_positions();
                let mut glyphs = Vec::with_capacity(infos.len());

                for (info, position) in infos.iter().zip(positions) {
                    let glyph_id = u16::try_from(info.glyph_id).map_err(|_| {
                        RenderError::font("font produced a glyph id larger than u16")
                    })?;
                    glyphs.push(ShapedGlyph {
                        face: face_index,
                        glyph_id,
                        scale,
                        x_offset: position.x_offset as f32 * scale,
                        y_offset: position.y_offset as f32 * scale,
                        x_advance: position.x_advance as f32 * scale,
                    });
                }

                Ok(glyphs)
            })
            .ok_or_else(|| RenderError::font("failed to load shaped font face"))??;

        Ok(result)
    }
}

fn normalize_text(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    for word in text.split_whitespace() {
        if !output.is_empty() {
            output.push(' ');
        }
        output.push_str(word);
    }
    output
}

struct OutlineToPath<'a> {
    builder: &'a mut PathBuilder,
    scale: f32,
    tx: f32,
    ty: f32,
}

impl ttf_parser::OutlineBuilder for OutlineToPath<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.builder
            .move_to(self.tx + x * self.scale, self.ty - y * self.scale);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.builder
            .line_to(self.tx + x * self.scale, self.ty - y * self.scale);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.builder.quad_to(
            self.tx + x1 * self.scale,
            self.ty - y1 * self.scale,
            self.tx + x * self.scale,
            self.ty - y * self.scale,
        );
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.builder.cubic_to(
            self.tx + x1 * self.scale,
            self.ty - y1 * self.scale,
            self.tx + x2 * self.scale,
            self.ty - y2 * self.scale,
            self.tx + x * self.scale,
            self.ty - y * self.scale,
        );
    }

    fn close(&mut self) {
        self.builder.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_whitespace() {
        assert_eq!(normalize_text("  hello\n\tworld  "), "hello world");
        assert_eq!(normalize_text(""), "");
    }
}
