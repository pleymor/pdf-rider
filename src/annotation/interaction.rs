use crate::pdf::models::*;

/// Active drawing tool.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
}

/// State machine for annotation interaction.
pub struct InteractionState {
    pub tool: Tool,
    // Drawing
    pub drawing: bool,
    pub start_page: u32,
    pub start_x: f32,
    pub start_y: f32,
    // Selection
    pub selected_page: u32,
    pub selected_idx: Option<usize>,
    // Dragging (move)
    pub dragging: bool,
    pub drag_start_x: f32,
    pub drag_start_y: f32,
    pub drag_orig_pdf_x: f64,
    pub drag_orig_pdf_y: f64,
    // Style defaults
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
            selected_page: 0,
            selected_idx: None,
            dragging: false,
            drag_start_x: 0.0,
            drag_start_y: 0.0,
            drag_orig_pdf_x: 0.0,
            drag_orig_pdf_y: 0.0,
            color: RgbColor { r: 255, g: 0, b: 0 },
            stroke_width: 2.0,
            font_size: 14.0,
        }
    }
}

fn canvas_to_pdf(canvas_x: f32, canvas_y: f32, scale: f32, page_height_pt: f32) -> (f64, f64) {
    let pdf_x = canvas_x as f64 / scale as f64;
    let pdf_y = page_height_pt as f64 - canvas_y as f64 / scale as f64;
    (pdf_x, pdf_y)
}

const MIN_SIZE_PT: f64 = 10.0;

impl InteractionState {
    pub fn clear_selection(&mut self) {
        self.selected_idx = None;
        self.selected_page = 0;
        self.dragging = false;
    }

    pub fn pointer_down(&mut self, page: u32, x: f32, y: f32) {
        if self.tool == Tool::Select {
            return;
        }
        self.drawing = true;
        self.start_page = page;
        self.start_x = x;
        self.start_y = y;
    }

    /// Start dragging the selected annotation.
    pub fn start_drag(&mut self, x: f32, y: f32, ann: &Annotation) {
        self.dragging = true;
        self.drag_start_x = x;
        self.drag_start_y = y;
        let (ox, oy) = ann_pdf_origin(ann);
        self.drag_orig_pdf_x = ox;
        self.drag_orig_pdf_y = oy;
    }

    /// Apply drag delta to annotation. Returns true if moved.
    pub fn apply_drag(
        &self,
        x: f32,
        y: f32,
        scale: f32,
        ann: &mut Annotation,
    ) -> bool {
        if !self.dragging { return false; }
        let dx = (x - self.drag_start_x) as f64 / scale as f64;
        let dy = -(y - self.drag_start_y) as f64 / scale as f64; // Y is flipped
        set_ann_origin(ann, self.drag_orig_pdf_x + dx, self.drag_orig_pdf_y + dy);
        true
    }

    pub fn end_drag(&mut self) {
        self.dragging = false;
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

/// Get the PDF origin (x, y) of an annotation.
fn ann_pdf_origin(ann: &Annotation) -> (f64, f64) {
    match ann {
        Annotation::Rect(r) => (r.x, r.y),
        Annotation::Circle(c) => (c.x, c.y),
        Annotation::Text(t) => (t.x, t.y),
        Annotation::Signature(s) => (s.x, s.y),
    }
}

/// Set the PDF origin (x, y) of an annotation.
pub fn set_ann_origin(ann: &mut Annotation, x: f64, y: f64) {
    match ann {
        Annotation::Rect(r) => { r.x = x; r.y = y; }
        Annotation::Circle(c) => { c.x = x; c.y = y; }
        Annotation::Text(t) => { t.x = x; t.y = y; }
        Annotation::Signature(s) => { s.x = x; s.y = y; }
    }
}
