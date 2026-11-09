#[derive(Debug, Clone)]
pub struct OverlayData {
    pub color: String,
    pub opacity: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct EllipseData {
    pub cx: f32,
    pub cy: f32,
    pub rx: f32,
    pub ry: f32,
}

#[derive(Debug, Clone)]
pub struct TextSpecData {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub color: String,
    pub bold: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SceneData {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub background: Option<Vec<u8>>,
    pub background_key: Option<String>,
    pub overlay: Option<OverlayData>,
    pub avatar: Option<Vec<u8>>,
    pub avatar_ellipse: Option<EllipseData>,
    pub texts: Vec<TextSpecData>,
}
