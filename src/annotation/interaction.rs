use crate::pdf::models::*;

/// Active drawing tool.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Rect,
    Circle,
    Text,
}

impl Tool {
    pub fn from_str(s: &str) -> Self {
        match s {
            "rect" => Tool::Rect,
            "circle" => Tool::Circle,
            "text" => Tool::Text,
            _ => Tool::Select,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Tool::Select => "select",
            Tool::Rect => "rect",
            Tool::Circle => "circle",
            Tool::Text => "text",
        }
    }
}

/// State machine for annotation drawing.
pub struct InteractionState {
    pub tool: Tool,
    pub drawing: bool,
    pub start_page: u32,
    pub start_x: f32, // canvas pixels
    pub start_y: f32,
    pub color: RgbColor,
    pub stroke_width: f64,
    pub font_size: f64,
}

impl Default for InteractionState {
    fn default() -> Self {
        Self {
            tool: Tool::Select,
            drawing: false,
            start_page: 0,
            start_x: 0.0,
            start_y: 0.0,
            color: RgbColor { r: 255, g: 0, b: 0 },
            stroke_width: 2.0,
            font_size: 14.0,
        }
    }
}

/// Convert canvas pixel coords to PDF coords (bottom-left origin, Y-up).
fn canvas_to_pdf(canvas_x: f32, canvas_y: f32, scale: f32, page_height_pt: f32) -> (f64, f64) {
    let pdf_x = canvas_x as f64 / scale as f64;
    let pdf_y = page_height_pt as f64 - canvas_y as f64 / scale as f64;
    (pdf_x, pdf_y)
}

const MIN_SIZE_PT: f64 = 10.0;

impl InteractionState {
    pub fn pointer_down(&mut self, page: u32, x: f32, y: f32) {
        if self.tool == Tool::Select {
            return; // selection handled separately later
        }
        self.drawing = true;
        self.start_page = page;
        self.start_x = x;
        self.start_y = y;
    }

    /// Finish drawing and return a new annotation, or None if too small.
    pub fn pointer_up(
        &mut self,
        page: u32,
        x: f32,
        y: f32,
        scale: f32,
        page_height_pt: f32,
    ) -> Option<Annotation> {
        if !self.drawing || page != self.start_page {
            self.drawing = false;
            return None;
        }
        self.drawing = false;

        let (pdf_x1, pdf_y1) = canvas_to_pdf(self.start_x, self.start_y, scale, page_height_pt);
        let (pdf_x2, pdf_y2) = canvas_to_pdf(x, y, scale, page_height_pt);

        let left = pdf_x1.min(pdf_x2);
        let right = pdf_x1.max(pdf_x2);
        let bottom = pdf_y1.min(pdf_y2);
        let top = pdf_y1.max(pdf_y2);
        let w = right - left;
        let h = top - bottom;

        match self.tool {
            Tool::Rect => {
                if w < MIN_SIZE_PT || h < MIN_SIZE_PT { return None; }
                Some(Annotation::Rect(RectAnnotation {
                    page,
                    x: left,
                    y: bottom,
                    width: w,
                    height: h,
                    color: self.color.clone(),
                    stroke_width: self.stroke_width,
                }))
            }
            Tool::Circle => {
                if w < MIN_SIZE_PT || h < MIN_SIZE_PT { return None; }
                Some(Annotation::Circle(CircleAnnotation {
                    page,
                    x: left,
                    y: bottom,
                    width: w,
                    height: h,
                    color: self.color.clone(),
                    stroke_width: self.stroke_width,
                }))
            }
            Tool::Text => {
                if w < MIN_SIZE_PT { return None; }
                Some(Annotation::Text(TextAnnotation {
                    page,
                    x: left,
                    y: top,
                    width: w,
                    content: "Text".to_string(),
                    color: self.color.clone(),
                    font_size: self.font_size,
                    bold: false,
                    italic: false,
                    underline: false,
                    alignment: TextAlignment::Left,
                }))
            }
            Tool::Select => None,
        }
    }
}
