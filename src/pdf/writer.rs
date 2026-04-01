use std::collections::HashMap;

use base64::Engine;
use image::ImageReader;
use lopdf::{dictionary, Document, Object, ObjectId, Stream, StringFormat};

use super::models::{
    Annotation, CircleAnnotation, RectAnnotation, SignatureAnnotation, TextAlignment,
    TextAnnotation,
};

// ── Annotation metadata (JSON + stream-ID tracking) ──────────────────────────

use serde::{Deserialize, Serialize};

/// Stored in the PDF catalog under `CCAnnot`.
/// Holds the editable annotation data AND the object IDs of the burned content
/// streams so they can be updated in-place on re-save (preventing accumulation).
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AnnotationMeta {
    pub annotations: Vec<Annotation>,
    /// Per-page annotation stream object IDs: page_num → [object_id, generation].
    pub stream_ids: HashMap<u32, [u32; 2]>,
}

fn catalog_id(doc: &Document) -> Result<ObjectId, String> {
    doc.trailer
        .get(b"Root")
        .map_err(|_| "No Root in trailer".to_string())?
        .as_reference()
        .map_err(|_| "Root is not a reference".to_string())
}

/// Read annotation metadata from the `CCAnnot` catalog entry.
/// Returns a default (empty) meta if the entry is absent or unreadable.
pub fn load_meta(doc: &Document) -> AnnotationMeta {
    let Ok(cat_id) = catalog_id(doc) else { return AnnotationMeta::default() };
    let Ok(cat)    = doc.get_object(cat_id) else { return AnnotationMeta::default() };
    let Ok(dict)   = cat.as_dict() else { return AnnotationMeta::default() };

    let Ok(Object::Reference(meta_ref)) = dict.get(b"CCAnnot") else {
        return AnnotationMeta::default();
    };
    let Ok(obj) = doc.get_object(*meta_ref) else { return AnnotationMeta::default() };
    let Object::Stream(stream) = obj else { return AnnotationMeta::default() };
    let Ok(json) = String::from_utf8(stream.content.clone()) else {
        return AnnotationMeta::default();
    };
    serde_json::from_str(&json).unwrap_or_default()
}

/// Serialize `meta` and store it in the PDF catalog under `CCAnnot`.
pub fn save_meta(doc: &mut Document, meta: &AnnotationMeta) -> Result<(), String> {
    let json      = serde_json::to_string(meta).map_err(|e| e.to_string())?;
    let stream    = Stream::new(dictionary! {}, json.into_bytes());
    let stream_id = doc.add_object(Object::Stream(stream));

    let cat_id  = catalog_id(doc)?;
    let catalog = doc.get_object_mut(cat_id).map_err(|e| e.to_string())?;
    if let Object::Dictionary(d) = catalog {
        d.set("CCAnnot", Object::Reference(stream_id));
    }
    Ok(())
}

// ── PDF operator builders ─────────────────────────────────────────────────────

/// Append rect stroke operators to `buf`.
pub fn write_rect(buf: &mut Vec<u8>, ann: &RectAnnotation) {
    let ops = format!(
        "q\n{:.4} {:.4} {:.4} RG\n{:.4} w\n{:.4} {:.4} {:.4} {:.4} re\nS\nQ\n",
        ann.color.r_f(),
        ann.color.g_f(),
        ann.color.b_f(),
        ann.stroke_width,
        ann.x,
        ann.y,
        ann.width,
        ann.height,
    );
    buf.extend_from_slice(ops.as_bytes());
}

/// Append circle stroke operators (4-arc Bézier) to `buf`.
pub fn write_circle(buf: &mut Vec<u8>, ann: &CircleAnnotation) {
    // Bézier approximation constant κ ≈ 0.5523
    const K: f64 = 0.5523;
    let cx = ann.x + ann.width / 2.0;
    let cy = ann.y + ann.height / 2.0;
    let rx = ann.width / 2.0;
    let ry = ann.height / 2.0;

    let ops = format!(
        concat!(
            "q\n",
            "{:.4} {:.4} {:.4} RG\n",
            "{:.4} w\n",
            // Move to top centre
            "{:.4} {:.4} m\n",
            // Arc 1: top → right
            "{:.4} {:.4} {:.4} {:.4} {:.4} {:.4} c\n",
            // Arc 2: right → bottom
            "{:.4} {:.4} {:.4} {:.4} {:.4} {:.4} c\n",
            // Arc 3: bottom → left
            "{:.4} {:.4} {:.4} {:.4} {:.4} {:.4} c\n",
            // Arc 4: left → top
            "{:.4} {:.4} {:.4} {:.4} {:.4} {:.4} c\n",
            "S\nQ\n"
        ),
        ann.color.r_f(), ann.color.g_f(), ann.color.b_f(),
        ann.stroke_width,
        // Move to top
        cx, cy + ry,
        // Arc 1
        cx + K * rx, cy + ry,   cx + rx, cy + K * ry,   cx + rx, cy,
        // Arc 2
        cx + rx, cy - K * ry,   cx + K * rx, cy - ry,   cx, cy - ry,
        // Arc 3
        cx - K * rx, cy - ry,   cx - rx, cy - K * ry,   cx - rx, cy,
        // Arc 4
        cx - rx, cy + K * ry,   cx - K * rx, cy + ry,   cx, cy + ry,
    );
    buf.extend_from_slice(ops.as_bytes());
}

/// Select the PDF standard Type1 font name for the given style flags.
fn font_name(bold: bool, italic: bool) -> &'static str {
    match (bold, italic) {
        (true, true) => "Helvetica-BoldOblique",
        (true, false) => "Helvetica-Bold",
        (false, true) => "Helvetica-Oblique",
        _ => "Helvetica",
    }
}

/// Short resource key for a font name (no hyphens, fits in name dict).
fn font_key(bold: bool, italic: bool) -> &'static str {
    match (bold, italic) {
        (true, true) => "HelvBO",
        (true, false) => "HelvB",
        (false, true) => "HelvO",
        _ => "Helv",
    }
}

/// Escape a string for use inside PDF literal string parentheses.
fn pdf_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out
}

/// Split `text` on explicit `\n` then word-wrap each paragraph to `max_width`,
/// using `char_w` as the average character width estimate.
fn wrap_text(text: &str, max_width: f64, char_w: f64) -> Vec<String> {
    let mut result = Vec::new();
    for para in text.split('\n') {
        if para.is_empty() {
            result.push(String::new());
            continue;
        }
        let mut line = String::new();
        let mut line_w = 0.0_f64;
        for word in para.split(' ') {
            let word_w = word.len() as f64 * char_w;
            // Width if we append this word (with a leading space if line is non-empty)
            let candidate_w = if line.is_empty() { word_w } else { line_w + char_w + word_w };
            if !line.is_empty() && candidate_w > max_width {
                result.push(line.clone());
                line = word.to_string();
                line_w = word_w;
            } else {
                if !line.is_empty() { line.push(' '); line_w += char_w; }
                line.push_str(word);
                line_w += word_w;
            }
        }
        result.push(line);
    }
    result
}

/// Append text operators to `buf`.
/// Returns the resource key and base font name so the caller can register it.
pub fn write_text(buf: &mut Vec<u8>, ann: &TextAnnotation) -> (&'static str, &'static str) {
    let fkey  = font_key(ann.bold, ann.italic);
    let fname = font_name(ann.bold, ann.italic);
    // Helvetica average glyph width ≈ 0.5 × font size
    let char_w  = ann.font_size * 0.5;
    let line_h  = ann.font_size * 1.2;
    let lines   = wrap_text(&ann.content, ann.width, char_w);

    let mut ops = format!(
        "q\n{:.4} {:.4} {:.4} rg\nBT\n/{} {:.4} Tf\n",
        ann.color.r_f(), ann.color.g_f(), ann.color.b_f(),
        fkey, ann.font_size,
    );

    // Collect underline segments to draw after ET (outside text object)
    let mut underlines: Vec<(f64, f64, f64)> = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let line_w = line.len() as f64 * char_w;
        let x_off = match ann.alignment {
            TextAlignment::Left   => 0.0,
            TextAlignment::Center => ((ann.width - line_w) / 2.0).max(0.0),
            TextAlignment::Right  => (ann.width - line_w).max(0.0),
        };
        let x = ann.x + x_off;
        let y = ann.y - i as f64 * line_h;

        // 1 0 0 1 x y Tm = absolute text position
        ops.push_str(&format!("1 0 0 1 {:.4} {:.4} Tm\n({}) Tj\n", x, y, pdf_escape(line)));

        if ann.underline {
            underlines.push((x, y - ann.font_size * 0.12, line_w.min(ann.width)));
        }
    }

    ops.push_str("ET\n");

    if !underlines.is_empty() {
        ops.push_str(&format!(
            "{:.4} {:.4} {:.4} RG\n{:.4} w\n",
            ann.color.r_f(), ann.color.g_f(), ann.color.b_f(),
            ann.font_size * 0.05,
        ));
        for (x, ul_y, ul_len) in underlines {
            ops.push_str(&format!("{:.4} {:.4} m\n{:.4} {:.4} l\nS\n", x, ul_y, x + ul_len, ul_y));
        }
    }

    ops.push_str("Q\n");
    buf.extend_from_slice(ops.as_bytes());
    (fkey, fname)
}

/// Decode base64 PNG, embed as an image XObject with SMask for transparency,
/// append `Do` operators to `buf`.
/// Returns `(xobj_name, xobj_id, smask_id)` so the caller can add page resources.
pub fn write_image(
    buf: &mut Vec<u8>,
    ann: &SignatureAnnotation,
    doc: &mut Document,
) -> Result<(String, ObjectId, Option<ObjectId>), String> {
    // Decode base64 (strip data URI prefix if present)
    let b64 = ann
        .image_data
        .splitn(2, ',')
        .last()
        .unwrap_or(&ann.image_data);
    let png_bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("base64 decode error: {e}"))?;

    // Decode image → RGBA
    let img = ImageReader::new(std::io::Cursor::new(&png_bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?
        .to_rgba8();

    let (w, h) = img.dimensions();
    let mut rgb_data: Vec<u8> = Vec::with_capacity((w * h * 3) as usize);
    let mut alpha_data: Vec<u8> = Vec::with_capacity((w * h) as usize);

    for pixel in img.pixels() {
        rgb_data.push(pixel[0]);
        rgb_data.push(pixel[1]);
        rgb_data.push(pixel[2]);
        alpha_data.push(pixel[3]);
    }

    // Create SMask (alpha channel XObject)
    let smask_stream = Stream::new(
        dictionary! {
            "Type" => Object::Name(b"XObject".to_vec()),
            "Subtype" => Object::Name(b"Image".to_vec()),
            "Width" => Object::Integer(w as i64),
            "Height" => Object::Integer(h as i64),
            "ColorSpace" => Object::Name(b"DeviceGray".to_vec()),
            "BitsPerComponent" => Object::Integer(8),
        },
        alpha_data,
    );
    let smask_id = doc.add_object(Object::Stream(smask_stream));

    // Create main Image XObject
    let img_stream = Stream::new(
        dictionary! {
            "Type" => Object::Name(b"XObject".to_vec()),
            "Subtype" => Object::Name(b"Image".to_vec()),
            "Width" => Object::Integer(w as i64),
            "Height" => Object::Integer(h as i64),
            "ColorSpace" => Object::Name(b"DeviceRGB".to_vec()),
            "BitsPerComponent" => Object::Integer(8),
            "SMask" => Object::Reference(smask_id),
        },
        rgb_data,
    );
    let img_id = doc.add_object(Object::Stream(img_stream));

    // A unique resource name for this image
    static IMG_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = IMG_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let xobj_name = format!("Sig{n}");

    // Build transformation matrix: scale to (width, height), translate to (x, y)
    let ops = format!(
        "q\n{:.4} 0 0 {:.4} {:.4} {:.4} cm\n/{} Do\nQ\n",
        ann.width, ann.height, ann.x, ann.y, xobj_name,
    );
    buf.extend_from_slice(ops.as_bytes());

    Ok((xobj_name, img_id, Some(smask_id)))
}

// ── Resource helpers ──────────────────────────────────────────────────────────

/// Add a standard Type1 font resource to a page (or its Resources dict).
/// `fkey` is the resource alias (e.g. "Helv"), `base_font` is the PDF font name.
fn add_font_to_page(
    doc: &mut Document,
    page_id: ObjectId,
    fkey: &str,
    base_font: &str,
) -> Result<(), String> {
    // Build a minimal font dict object
    let font_obj = dictionary! {
        "Type" => Object::Name(b"Font".to_vec()),
        "Subtype" => Object::Name(b"Type1".to_vec()),
        "BaseFont" => Object::Name(base_font.as_bytes().to_vec()),
    };
    let font_id = doc.add_object(Object::Dictionary(font_obj));

    // Mutably access the page dict
    let page = doc
        .get_object_mut(page_id)
        .map_err(|e| e.to_string())?;
    let page_dict = match page {
        Object::Dictionary(d) => d,
        _ => return Err("page is not a dictionary".into()),
    };

    // Ensure Resources entry exists as an inline dict
    if !page_dict.has(b"Resources") {
        page_dict.set("Resources", Object::Dictionary(lopdf::Dictionary::new()));
    }

    // Navigate into Resources.Font
    let resources = page_dict
        .get_mut(b"Resources")
        .map_err(|e| e.to_string())?;
    let res_dict = match resources {
        Object::Dictionary(d) => d,
        _ => return Err("Resources is not an inline dict (inherited); cannot modify".into()),
    };

    if !res_dict.has(b"Font") {
        res_dict.set("Font", Object::Dictionary(lopdf::Dictionary::new()));
    }

    let font_dict = res_dict
        .get_mut(b"Font")
        .map_err(|e| e.to_string())?;
    if let Object::Dictionary(d) = font_dict {
        d.set(fkey.as_bytes().to_vec(), Object::Reference(font_id));
    }

    Ok(())
}

/// Add an image XObject reference to the page's Resources.XObject dict.
fn add_xobject_to_page(
    doc: &mut Document,
    page_id: ObjectId,
    name: &str,
    xobj_id: ObjectId,
) -> Result<(), String> {
    let page = doc
        .get_object_mut(page_id)
        .map_err(|e| e.to_string())?;
    let page_dict = match page {
        Object::Dictionary(d) => d,
        _ => return Err("page is not a dictionary".into()),
    };

    if !page_dict.has(b"Resources") {
        page_dict.set("Resources", Object::Dictionary(lopdf::Dictionary::new()));
    }

    let resources = page_dict
        .get_mut(b"Resources")
        .map_err(|e| e.to_string())?;
    let res_dict = match resources {
        Object::Dictionary(d) => d,
        _ => return Err("Resources is not an inline dict".into()),
    };

    if !res_dict.has(b"XObject") {
        res_dict.set("XObject", Object::Dictionary(lopdf::Dictionary::new()));
    }

    let xobj_dict = res_dict
        .get_mut(b"XObject")
        .map_err(|e| e.to_string())?;
    if let Object::Dictionary(d) = xobj_dict {
        d.set(name.as_bytes().to_vec(), Object::Reference(xobj_id));
    }

    Ok(())
}

/// Append `new_ops` as an additional content stream to the page.
/// Returns the `ObjectId` of the newly created stream.
fn append_content_stream(
    doc: &mut Document,
    page_id: ObjectId,
    new_ops: Vec<u8>,
) -> Result<ObjectId, String> {
    let new_stream = Stream::new(dictionary! {}, new_ops);
    let new_id = doc.add_object(Object::Stream(new_stream));

    // Read the current Contents value (clone to avoid borrow conflict)
    let existing: Vec<Object> = {
        let page = doc.get_object(page_id).map_err(|e| e.to_string())?;
        let dict = page.as_dict().map_err(|e| e.to_string())?;
        match dict.get(b"Contents") {
            Ok(Object::Reference(id)) => vec![Object::Reference(*id)],
            Ok(Object::Array(arr)) => arr.clone(),
            _ => vec![],
        }
    };

    let mut refs = existing;
    refs.push(Object::Reference(new_id));

    let page = doc
        .get_object_mut(page_id)
        .map_err(|e| e.to_string())?;
    if let Object::Dictionary(d) = page {
        d.set("Contents", Object::Array(refs));
    }

    Ok(new_id)
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Write all annotations for a single page into the document.
///
/// If `existing_stream_id` is `Some`, the operators are written into that
/// existing stream object in-place (prevents cumulative burn layers on re-save).
/// Otherwise a new content stream is appended.
///
/// Returns the `ObjectId` of the stream that was written, or `None` if there
/// were no annotations and no existing stream to clear.
pub fn write_annotations_for_page(
    doc: &mut Document,
    page_id: ObjectId,
    annotations: &[Annotation],
    existing_stream_id: Option<ObjectId>,
) -> Result<Option<ObjectId>, String> {
    let mut ops: Vec<u8> = Vec::new();
    let mut fonts_needed: HashMap<&'static str, &'static str> = HashMap::new();
    let mut xobjects: Vec<(String, ObjectId)> = Vec::new();

    for ann in annotations {
        match ann {
            Annotation::Rect(r) => write_rect(&mut ops, r),
            Annotation::Circle(c) => write_circle(&mut ops, c),
            Annotation::Text(t) => {
                let (fkey, fname) = write_text(&mut ops, t);
                fonts_needed.insert(fkey, fname);
            }
            Annotation::Signature(s) => {
                let (name, img_id, _smask_id) = write_image(&mut ops, s, doc)?;
                xobjects.push((name, img_id));
            }
        }
    }

    if ops.is_empty() {
        // If we had a previous stream, empty it so stale operators are gone
        if let Some(sid) = existing_stream_id {
            let obj = doc.get_object_mut(sid).map_err(|e| e.to_string())?;
            if let Object::Stream(s) = obj { s.content = vec![]; }
            return Ok(Some(sid));
        }
        return Ok(None);
    }

    // Register font resources
    for (fkey, fname) in &fonts_needed {
        add_font_to_page(doc, page_id, fkey, fname)?;
    }

    // Register image XObject resources
    for (name, xobj_id) in xobjects {
        add_xobject_to_page(doc, page_id, &name, xobj_id)?;
    }

    // Update existing stream in-place, or append a new one
    if let Some(sid) = existing_stream_id {
        let obj = doc.get_object_mut(sid).map_err(|e| e.to_string())?;
        if let Object::Stream(s) = obj {
            s.content = ops;
            return Ok(Some(sid));
        }
    }
    let new_id = append_content_stream(doc, page_id, ops)?;
    Ok(Some(new_id))
}

// ── AcroForm field writing ────────────────────────────────────────────────────

/// A form field name/value pair sent from the frontend.
#[derive(serde::Deserialize, Clone)]
pub struct FormFieldValue {
    pub name: String,
    /// Plain text for Tx/Ch, "true"/"false" for checkboxes, export value for radios.
    pub value: String,
}

/// Write user-supplied form field values into the document's AcroForm structure.
/// Sets `/NeedAppearances true` so viewers regenerate visual appearances.
/// Silently succeeds if the document has no AcroForm or `fields` is empty.
pub fn write_form_fields(doc: &mut Document, fields: &[FormFieldValue]) -> Result<(), String> {
    if fields.is_empty() {
        return Ok(());
    }

    let values_map: HashMap<String, String> = fields
        .iter()
        .map(|f| (f.name.clone(), f.value.clone()))
        .collect();

    let cat_id = catalog_id(doc)?;

    // Locate the AcroForm — only handles the common case of a referenced dict
    let acroform_id = {
        let cat  = doc.get_object(cat_id).map_err(|e| e.to_string())?;
        let dict = cat.as_dict().map_err(|e| e.to_string())?;
        match dict.get(b"AcroForm") {
            Ok(Object::Reference(id)) => *id,
            _ => return Ok(()), // no AcroForm or inline dict — skip silently
        }
    };

    // Set NeedAppearances = true so viewers regenerate field appearances
    {
        let acroform = doc.get_object_mut(acroform_id).map_err(|e| e.to_string())?;
        if let Object::Dictionary(d) = acroform {
            d.set(b"NeedAppearances", Object::Boolean(true));
        }
    }

    // Collect the top-level Fields array (cloned to release the borrow)
    let field_refs: Vec<Object> = {
        let acroform = doc.get_object(acroform_id).map_err(|e| e.to_string())?;
        let dict     = acroform.as_dict().map_err(|e| e.to_string())?;
        match dict.get(b"Fields") {
            Ok(Object::Array(arr)) => arr.clone(),
            _ => return Ok(()),
        }
    };

    // First pass (immutable): collect what needs to be updated
    let updates = collect_field_updates(doc, &field_refs, "", "", &values_map)?;

    // Second pass (mutable): apply the updates
    for (id, ft, value, on_state, set_as) in updates {
        let obj = doc.get_object_mut(id).map_err(|e| e.to_string())?;
        if let Object::Dictionary(d) = obj {
            match ft.as_str() {
                "Tx" | "Ch" => {
                    d.set(b"V", Object::String(value.into_bytes(), StringFormat::Literal));
                }
                "Btn" => {
                    let (v_bytes, as_bytes) = match value.as_str() {
                        "true"  => (on_state.clone(), on_state),
                        "false" | "" => (b"Off".to_vec(), b"Off".to_vec()),
                        export_val => {
                            let b = export_val.as_bytes().to_vec();
                            (b.clone(), b)
                        }
                    };
                    d.set(b"V", Object::Name(v_bytes));
                    if set_as {
                        d.set(b"AS", Object::Name(as_bytes));
                    }
                }
                _ => {
                    d.set(b"V", Object::String(value.into_bytes(), StringFormat::Literal));
                }
            }
        }
    }

    Ok(())
}

/// Recursively traverse the AcroForm field tree, collecting updates.
/// Returns `Vec<(ObjectId, field_type, new_value, on_state_name, set_as_flag)>`.
#[allow(clippy::type_complexity)]
fn collect_field_updates(
    doc: &Document,
    refs: &[Object],
    parent_name: &str,
    inherited_ft: &str,
    values: &HashMap<String, String>,
) -> Result<Vec<(ObjectId, String, String, Vec<u8>, bool)>, String> {
    let mut result = Vec::new();

    for obj_ref in refs {
        let id = match obj_ref {
            Object::Reference(id) => *id,
            _ => continue,
        };

        let obj  = match doc.get_object(id) { Ok(o) => o, Err(_) => continue };
        let dict = match obj.as_dict()       { Ok(d) => d, Err(_) => continue };

        // Partial field name (/T)
        let partial: String = dict.get(b"T")
            .ok()
            .and_then(|o| match o {
                Object::String(s, _) => String::from_utf8(s.clone()).ok(),
                _ => None,
            })
            .unwrap_or_default();

        let full_name = if parent_name.is_empty() {
            partial.clone()
        } else if partial.is_empty() {
            parent_name.to_string()
        } else {
            format!("{}.{}", parent_name, partial)
        };

        // Field type — inherit from parent if not set on this node
        let ft: String = dict.get(b"FT")
            .ok()
            .and_then(|o| if let Object::Name(n) = o { String::from_utf8(n.clone()).ok() } else { None })
            .unwrap_or_else(|| inherited_ft.to_string());

        let has_kids = dict.has(b"Kids");

        // If this node has a /T and we have a value for it, schedule an update
        if !partial.is_empty() {
            let maybe_val = values.get(&full_name)
                .or_else(|| values.get(&partial))
                .cloned();
            if let Some(val) = maybe_val {
                let on_state = field_on_state_name(dict);
                // Only set /AS on terminal widget nodes (checkbox merged with field)
                let set_as = !has_kids && ft == "Btn";
                result.push((id, ft.clone(), val, on_state, set_as));
            }
        }

        // Recurse into Kids
        if has_kids {
            let kids: Vec<Object> = match dict.get(b"Kids") {
                Ok(Object::Array(arr)) => arr.clone(),
                _ => continue,
            };
            let child_results = collect_field_updates(doc, &kids, &full_name, &ft, values)?;
            result.extend(child_results);
        }
    }

    Ok(result)
}

/// Determine the on-state name for a checkbox/radio from its appearance dict.
/// Falls back to `b"Yes"` if no appearance dictionary is present.
fn field_on_state_name(dict: &lopdf::Dictionary) -> Vec<u8> {
    if let Ok(Object::Dictionary(ap)) = dict.get(b"AP") {
        if let Ok(Object::Dictionary(n_dict)) = ap.get(b"N") {
            for (key, _) in n_dict.iter() {
                if key.as_slice() != b"Off" {
                    return key.clone();
                }
            }
        }
    }
    b"Yes".to_vec()
}

/// Clear the content streams referenced by `meta.stream_ids`.
/// Produces a "display" version of the PDF where annotation rendering
/// is owned entirely by the overlay, avoiding stale pdf.js burns.
pub fn clear_annotation_streams(doc: &mut Document, meta: &AnnotationMeta) -> Result<(), String> {
    for arr in meta.stream_ids.values() {
        let sid = (arr[0], arr[1] as u16);
        if let Ok(obj) = doc.get_object_mut(sid) {
            if let Object::Stream(s) = obj {
                s.content = vec![];
            }
        }
    }
    Ok(())
}

// ── Tests (TDD — must fail before implementation) ────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::models::{RgbColor, TextAlignment};

    fn red() -> RgbColor { RgbColor { r: 255, g: 0, b: 0 } }
    fn black() -> RgbColor { RgbColor { r: 0, g: 0, b: 0 } }

    #[test]
    fn test_write_rect_contains_operators() {
        let ann = RectAnnotation {
            page: 1,
            x: 100.0, y: 200.0,
            width: 50.0, height: 30.0,
            color: red(),
            stroke_width: 1.5,
        };
        let mut buf = Vec::new();
        write_rect(&mut buf, &ann);
        let s = String::from_utf8(buf).unwrap();

        assert!(s.contains("q"), "missing save state");
        assert!(s.contains("Q"), "missing restore state");
        assert!(s.contains("re"), "missing rect operator");
        assert!(s.contains("\nS\n"), "missing stroke operator");
        assert!(s.contains("RG"), "missing stroke colour");
        // Red component should be 1.0000
        assert!(s.contains("1.0000"), "red channel not 1.0");
    }

    #[test]
    fn test_write_circle_contains_bezier() {
        let ann = CircleAnnotation {
            page: 1,
            x: 100.0, y: 100.0,
            width: 60.0, height: 60.0,
            color: black(),
            stroke_width: 1.0,
        };
        let mut buf = Vec::new();
        write_circle(&mut buf, &ann);
        let s = String::from_utf8(buf).unwrap();

        assert!(s.contains(" c\n"), "missing bezier curve operator");
        assert!(s.contains(" m\n"), "missing moveto operator");
        assert!(s.contains("\nS\n"), "missing stroke operator");
        assert!(s.contains("RG"), "missing stroke colour");
    }

    #[test]
    fn test_write_text_contains_text_operators() {
        let ann = TextAnnotation {
            page: 1,
            x: 50.0, y: 300.0,
            width: 200.0,
            content: "Hello".to_string(),
            color: black(),
            font_size: 12.0,
            bold: false,
            italic: false,
            underline: false,
            alignment: TextAlignment::Left,
        };
        let mut buf = Vec::new();
        let (fkey, fname) = write_text(&mut buf, &ann);
        let s = String::from_utf8(buf).unwrap();

        assert!(s.contains("BT"), "missing begin text");
        assert!(s.contains("ET"), "missing end text");
        assert!(s.contains("Tf"), "missing font operator");
        assert!(s.contains("Tj"), "missing text draw operator");
        assert!(s.contains("Hello"), "text content missing");
        assert_eq!(fkey, "Helv");
        assert_eq!(fname, "Helvetica");
    }

    #[test]
    fn test_write_text_bold_italic_selects_correct_font() {
        let ann = TextAnnotation {
            page: 1,
            x: 0.0, y: 0.0,
            width: 100.0,
            content: "Bold".to_string(),
            color: black(),
            font_size: 10.0,
            bold: true,
            italic: true,
            underline: false,
            alignment: TextAlignment::Center,
        };
        let mut buf = Vec::new();
        let (fkey, fname) = write_text(&mut buf, &ann);
        assert_eq!(fkey, "HelvBO");
        assert_eq!(fname, "Helvetica-BoldOblique");
    }

    #[test]
    fn test_write_image_embeds_xobject() {
        // Create a 2×2 solid red PNG in memory
        use image::{DynamicImage, ImageBuffer, Rgba};
        let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(2, 2);
        for p in img.pixels_mut() {
            *p = Rgba([255, 0, 0, 255]);
        }
        let mut png_buf: Vec<u8> = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut png_buf),
                image::ImageFormat::Png,
            )
            .unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png_buf);

        let ann = SignatureAnnotation {
            page: 1,
            x: 10.0, y: 10.0,
            width: 50.0, height: 20.0,
            image_data: b64,
        };

        let mut doc = Document::new();
        let mut ops_buf: Vec<u8> = Vec::new();
        let (name, _img_id, _smask) = write_image(&mut ops_buf, &ann, &mut doc).unwrap();

        let s = String::from_utf8(ops_buf).unwrap();
        assert!(s.contains("Do"), "missing XObject Do operator");
        assert!(s.contains(" cm\n"), "missing transformation matrix");
        assert!(s.contains(&name), "resource name not in content stream");
    }
}
