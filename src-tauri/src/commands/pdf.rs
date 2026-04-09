use std::collections::{HashMap, HashSet};

use lopdf::Document;
use serde::{Deserialize, Serialize};

use crate::pdf::{models::Annotation, writer, writer::FormFieldValue};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PageOperation {
    pub page: u32,
    pub rotation: i64,
    pub delete: bool,
}

/// Returns the total number of pages in the given PDF file.
#[tauri::command]
pub fn get_page_count(file_path: String) -> Result<u32, String> {
    let doc = Document::load(&file_path).map_err(|e| e.to_string())?;
    Ok(doc.get_pages().len() as u32)
}

/// Burns `annotations` into the PDF content streams (visible in all viewers)
/// and also stores them as editable JSON metadata so they can be re-loaded.
///
/// On re-save, existing annotation streams are updated in-place rather than
/// appended, so repeated saves never accumulate stale burn layers.
#[tauri::command]
pub fn save_annotated_pdf(
    input_path: String,
    output_path: String,
    annotations: Vec<Annotation>,
    rotation_delta: i64,
    form_fields: Vec<FormFieldValue>,
) -> Result<(), String> {
    let mut doc = Document::load(&input_path).map_err(|e| e.to_string())?;

    // Apply rotation to every page's Rotate entry before burning annotations.
    if rotation_delta != 0 {
        let page_ids: Vec<(u32, u16)> = doc.get_pages().values().copied().collect();
        for page_id in page_ids {
            if let Ok(lopdf::Object::Dictionary(ref mut dict)) = doc.get_object_mut(page_id) {
                let current = dict.get(b"Rotate")
                    .ok()
                    .and_then(|o| o.as_i64().ok())
                    .unwrap_or(0);
                let new_rotate = (current + rotation_delta).rem_euclid(360);
                dict.set(b"Rotate", lopdf::Object::Integer(new_rotate));
            }
        }
    }

    let mut meta = writer::load_meta(&doc);

    // Group annotations by page
    let mut by_page: HashMap<u32, Vec<Annotation>> = HashMap::new();
    for ann in &annotations {
        by_page.entry(ann.page()).or_default().push(ann.clone());
    }

    let pages = doc.get_pages();
    let mut new_stream_ids: HashMap<u32, [u32; 2]> = HashMap::new();

    // Write (or update) annotation streams for each page that has annotations
    for (&page_num, anns) in &by_page {
        let page_id = pages
            .get(&page_num)
            .copied()
            .ok_or_else(|| format!("page {page_num} not found in document"))?;

        let existing = meta.stream_ids.get(&page_num).map(|arr| (arr[0], arr[1] as u16));

        if let Some(sid) = writer::write_annotations_for_page(&mut doc, page_id, anns, existing)? {
            new_stream_ids.insert(page_num, [sid.0, sid.1 as u32]);
        }
    }

    // For pages that previously had annotations but now have none, empty their streams
    for (&page_num, arr) in &meta.stream_ids {
        if !by_page.contains_key(&page_num) {
            let sid = (arr[0], arr[1] as u16);
            if let Ok(obj) = doc.get_object_mut(sid) {
                if let lopdf::Object::Stream(s) = obj {
                    s.content = vec![];
                }
            }
        }
    }

    meta.annotations = annotations;
    meta.stream_ids  = new_stream_ids;
    writer::save_meta(&mut doc, &meta)?;

    // Write form field values into the PDF's AcroForm structure
    writer::write_form_fields(&mut doc, &form_fields)?;

    doc.save(&output_path).map_err(|e| e.to_string())?;
    Ok(())
}

/// Creates a copy of the PDF with annotation content streams emptied
/// (the CCAnnot metadata is preserved). Returns true if any streams were cleared.
/// Used to produce a "display" PDF that pdf.js can render without showing
/// burned annotation content — the overlay draws all annotations from memory.
#[tauri::command]
pub fn strip_annotation_streams(
    input_path: String,
    output_path: String,
) -> Result<bool, String> {
    let mut doc = Document::load(&input_path).map_err(|e| e.to_string())?;
    let meta = writer::load_meta(&doc);
    if meta.stream_ids.is_empty() {
        doc.save(&output_path).map_err(|e| e.to_string())?;
        return Ok(false);
    }
    writer::clear_annotation_streams(&mut doc, &meta)?;
    doc.save(&output_path).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Reads editable annotations from the PDF's `CCAnnot` catalog entry.
/// Returns an empty array if the file has no stored annotations.
#[tauri::command]
pub fn read_annotations(file_path: String) -> Result<Vec<Annotation>, String> {
    let doc = Document::load(&file_path).map_err(|e| e.to_string())?;
    Ok(writer::load_meta(&doc).annotations)
}

/// Applies per-page rotation and deletion to a PDF.
#[tauri::command]
pub fn modify_pages(
    input_path: String,
    output_path: String,
    operations: Vec<PageOperation>,
) -> Result<(), String> {
    let mut doc = Document::load(&input_path).map_err(|e| e.to_string())?;

    let rotation_map: HashMap<u32, i64> = operations
        .iter()
        .filter(|op| !op.delete && op.rotation != 0)
        .map(|op| (op.page, op.rotation))
        .collect();

    let delete_set: HashSet<u32> = operations
        .iter()
        .filter(|op| op.delete)
        .map(|op| op.page)
        .collect();

    let pages = doc.get_pages();
    for (&page_num, &page_id) in &pages {
        if let Some(&rot_delta) = rotation_map.get(&page_num) {
            if let Ok(lopdf::Object::Dictionary(ref mut dict)) = doc.get_object_mut(page_id) {
                let current = dict
                    .get(b"Rotate")
                    .ok()
                    .and_then(|o| o.as_i64().ok())
                    .unwrap_or(0);
                let new_rotate = (current + rot_delta).rem_euclid(360);
                dict.set(b"Rotate", lopdf::Object::Integer(new_rotate));
            }
        }
    }

    if !delete_set.is_empty() {
        let mut to_delete: Vec<u32> = delete_set.iter().copied().collect();
        to_delete.sort();
        doc.delete_pages(&to_delete);
    }

    let mut meta = writer::load_meta(&doc);

    if !delete_set.is_empty() {
        let sorted_deleted: Vec<u32> = {
            let mut v: Vec<u32> = delete_set.iter().copied().collect();
            v.sort();
            v
        };

        meta.annotations.retain(|ann| !delete_set.contains(&ann.page()));
        for ann in &mut meta.annotations {
            let old_page = ann.page();
            let shift = sorted_deleted.iter().filter(|&&d| d < old_page).count() as u32;
            ann.set_page(old_page - shift);
        }

        let old_ids = std::mem::take(&mut meta.stream_ids);
        for (page_num, ids) in old_ids {
            if delete_set.contains(&page_num) {
                continue;
            }
            let shift = sorted_deleted.iter().filter(|&&d| d < page_num).count() as u32;
            meta.stream_ids.insert(page_num - shift, ids);
        }
    }

    writer::save_meta(&mut doc, &meta)?;
    doc.save(&output_path).map_err(|e| e.to_string())?;
    Ok(())
}

/// Extracts selected pages from a PDF into a new file, preserving vector text.
#[tauri::command]
pub fn extract_pdf_pages(
    input_path: String,
    output_path: String,
    pages: Vec<u32>,
) -> Result<(), String> {
    let mut doc = Document::load(&input_path).map_err(|e| e.to_string())?;

    let all_pages = doc.get_pages();
    let total = all_pages.len() as u32;

    if pages.len() as u32 >= total {
        doc.save(&output_path).map_err(|e| e.to_string())?;
        return Ok(());
    }

    let pages_to_keep: std::collections::HashSet<u32> = pages.into_iter().collect();

    let mut pages_to_remove: Vec<(u32, u16)> = Vec::new();
    for (&page_num, &page_id) in &all_pages {
        if !pages_to_keep.contains(&page_num) {
            pages_to_remove.push(page_id);
        }
    }

    let catalog_id = doc.trailer.get(b"Root")
        .and_then(|o| o.as_reference())
        .map_err(|e| format!("No catalog: {e}"))?;
    let pages_id = doc.get_object(catalog_id)
        .and_then(|o| o.as_dict())
        .and_then(|d| d.get(b"Pages"))
        .and_then(|o| o.as_reference())
        .map_err(|e| format!("No Pages: {e}"))?;

    let remove_set: std::collections::HashSet<(u32, u16)> = pages_to_remove.iter().copied().collect();

    if let Ok(lopdf::Object::Dictionary(ref mut pages_dict)) = doc.get_object_mut(pages_id) {
        if let Ok(lopdf::Object::Array(ref kids)) = pages_dict.get(b"Kids") {
            let new_kids: Vec<lopdf::Object> = kids.iter()
                .filter(|kid| {
                    if let Ok(id) = kid.as_reference() {
                        !remove_set.contains(&id)
                    } else {
                        true
                    }
                })
                .cloned()
                .collect();
            let count = new_kids.len() as i64;
            pages_dict.set(b"Kids", lopdf::Object::Array(new_kids));
            pages_dict.set(b"Count", lopdf::Object::Integer(count));
        }
    }

    doc.prune_objects();
    doc.save(&output_path).map_err(|e| e.to_string())?;
    Ok(())
}
