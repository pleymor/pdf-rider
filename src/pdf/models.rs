use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RgbColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl RgbColor {
    /// Normalize channel to 0.0–1.0 for PDF operators.
    pub fn r_f(&self) -> f64 { self.r as f64 / 255.0 }
    pub fn g_f(&self) -> f64 { self.g as f64 / 255.0 }
    pub fn b_f(&self) -> f64 { self.b as f64 / 255.0 }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum TextAlignment {
    Left,
    Center,
    Right,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RectAnnotation {
    pub page: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub color: RgbColor,
    pub stroke_width: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CircleAnnotation {
    pub page: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub color: RgbColor,
    pub stroke_width: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TextAnnotation {
    pub page: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub content: String,
    pub color: RgbColor,
    pub font_size: f64,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub alignment: TextAlignment,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SignatureAnnotation {
    pub page: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Base64-encoded PNG (no data: URI prefix).
    pub image_data: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Annotation {
    Rect(RectAnnotation),
    Circle(CircleAnnotation),
    Text(TextAnnotation),
    Signature(SignatureAnnotation),
}

impl Annotation {
    pub fn page(&self) -> u32 {
        match self {
            Annotation::Rect(a) => a.page,
            Annotation::Circle(a) => a.page,
            Annotation::Text(a) => a.page,
            Annotation::Signature(a) => a.page,
        }
    }
}
