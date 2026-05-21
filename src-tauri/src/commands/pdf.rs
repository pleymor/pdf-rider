use std::collections::{HashMap, HashSet};

use lopdf::{Document, IncrementalDocument, Object, ObjectId};
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
    // Fast-path: nothing to apply → byte-perfect copy. Avoids any PDF rewrite.
    if annotations.is_empty() && rotation_delta == 0 && form_fields.is_empty() {
        let prev_meta_empty = Document::load(&input_path)
            .map(|d| {
                let m = writer::load_meta(&d);
                m.annotations.is_empty() && m.stream_ids.is_empty()
            })
            .unwrap_or(true);
        if prev_meta_empty {
            std::fs::copy(&input_path, &output_path).map_err(|e| e.to_string())?;
            return Ok(());
        }
    }

    // We MUST go through `IncrementalDocument` rather than `Document::load +
    // Document::save`. The full-rewrite save in lopdf 0.39 drops streams and
    // loses indirect /Length objects on PDFs that use object streams (iLovePDF
    // / Acrobat output), producing files whose content streams render as
    // garbage. The incremental path preserves the input bytes verbatim and
    // only appends our modifications.
    let mut inc = IncrementalDocument::load(&input_path).map_err(|e| e.to_string())?;

    // Apply rotation to every page's Rotate entry before burning annotations.
    if rotation_delta != 0 {
        let page_ids: Vec<ObjectId> = inc
            .get_prev_documents()
            .get_pages()
            .values()
            .copied()
            .collect();
        for page_id in page_ids {
            inc.opt_clone_object_to_new_document(page_id)
                .map_err(|e| e.to_string())?;
            if let Ok(Object::Dictionary(dict)) = inc.new_document.get_object_mut(page_id) {
                let current = dict
                    .get(b"Rotate")
                    .ok()
                    .and_then(|o| o.as_i64().ok())
                    .unwrap_or(0);
                let new_rotate = (current + rotation_delta).rem_euclid(360);
                dict.set(b"Rotate", Object::Integer(new_rotate));
            }
        }
    }

    let mut meta = writer::load_meta(inc.get_prev_documents());

    // Group annotations by page
    let mut by_page: HashMap<u32, Vec<Annotation>> = HashMap::new();
    for ann in &annotations {
        by_page.entry(ann.page()).or_default().push(ann.clone());
    }

    let pages = inc.get_prev_documents().get_pages();
    let mut new_stream_ids: HashMap<u32, [u32; 2]> = HashMap::new();

    // Write (or update) annotation streams for each page that has annotations.
    // Page + existing stream must be cloned into new_document before the helper
    // mutates them, otherwise get_object_mut would miss them.
    for (&page_num, anns) in &by_page {
        let page_id = pages
            .get(&page_num)
            .copied()
            .ok_or_else(|| format!("page {page_num} not found in document"))?;

        let existing = meta.stream_ids.get(&page_num).map(|arr| (arr[0], arr[1] as u16));

        inc.opt_clone_object_to_new_document(page_id)
            .map_err(|e| e.to_string())?;
        inline_page_resources(&mut inc, page_id)?;
        if let Some(sid) = existing {
            inc.opt_clone_object_to_new_document(sid)
                .map_err(|e| e.to_string())?;
        }

        if let Some(sid) =
            writer::write_annotations_for_page(&mut inc.new_document, page_id, anns, existing)?
        {
            new_stream_ids.insert(page_num, [sid.0, sid.1 as u32]);
        }
    }

    // For pages that previously had annotations but now have none, empty their streams.
    for (&page_num, arr) in &meta.stream_ids {
        if !by_page.contains_key(&page_num) {
            let sid = (arr[0], arr[1] as u16);
            inc.opt_clone_object_to_new_document(sid)
                .map_err(|e| e.to_string())?;
            if let Ok(Object::Stream(s)) = inc.new_document.get_object_mut(sid) {
                s.content = vec![];
            }
        }
    }

    meta.annotations = annotations;
    meta.stream_ids = new_stream_ids;

    // Catalog must live in new_document for save_meta (CCAnnot) and
    // write_form_fields (AcroForm) to mutate it.
    let cat_id = inc
        .get_prev_documents()
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|e| format!("No catalog: {e}"))?;
    inc.opt_clone_object_to_new_document(cat_id)
        .map_err(|e| e.to_string())?;
    writer::save_meta(&mut inc.new_document, &meta)?;

    if !form_fields.is_empty() {
        clone_acroform_tree(&mut inc, cat_id).map_err(|e| e.to_string())?;
        writer::write_form_fields(&mut inc.new_document, &form_fields)?;
    }

    inc.save(&output_path).map_err(|e| e.to_string())?;
    Ok(())
}

/// `write_annotations_for_page`'s `add_font_to_page` / `add_xobject_to_page`
/// helpers assume `/Resources` (and its `/Font`, `/XObject` sub-dicts) live
/// inline on the page. Real-world PDFs frequently use indirect references
/// instead (e.g. `/Resources 100 0 R`), causing them to bail with
/// "Resources is not an inline dict".
///
/// We materialise the referenced dictionaries onto the page in `new_document`
/// (page already cloned by the caller) by cloning their contents from
/// `prev_documents`. The originals stay untouched, so any other page that
/// shared them keeps working.
fn inline_page_resources(
    inc: &mut IncrementalDocument,
    page_id: ObjectId,
) -> Result<(), String> {
    // Step 1: inline /Resources itself if it's a reference.
    let resources_ref: Option<ObjectId> = {
        let dict = inc
            .new_document
            .get_object(page_id)
            .and_then(Object::as_dict)
            .map_err(|e| e.to_string())?;
        match dict.get(b"Resources") {
            Ok(Object::Reference(id)) => Some(*id),
            _ => None,
        }
    };
    if let Some(res_id) = resources_ref {
        let clone = inc
            .get_prev_documents()
            .get_object(res_id)
            .map_err(|e| e.to_string())?
            .clone();
        let page = inc
            .new_document
            .get_object_mut(page_id)
            .and_then(Object::as_dict_mut)
            .map_err(|e| e.to_string())?;
        page.set(b"Resources", clone);
    }

    // Step 2: inline /Resources/Font and /Resources/XObject if either is a reference.
    let (font_ref, xobj_ref): (Option<ObjectId>, Option<ObjectId>) = {
        let dict = inc
            .new_document
            .get_object(page_id)
            .and_then(Object::as_dict)
            .map_err(|e| e.to_string())?;
        let res = match dict.get(b"Resources") {
            Ok(Object::Dictionary(d)) => d,
            _ => return Ok(()),
        };
        let f = match res.get(b"Font") {
            Ok(Object::Reference(id)) => Some(*id),
            _ => None,
        };
        let x = match res.get(b"XObject") {
            Ok(Object::Reference(id)) => Some(*id),
            _ => None,
        };
        (f, x)
    };
    for (key, maybe_ref) in [(&b"Font"[..], font_ref), (&b"XObject"[..], xobj_ref)] {
        let Some(sub_id) = maybe_ref else { continue };
        let clone = inc
            .get_prev_documents()
            .get_object(sub_id)
            .map_err(|e| e.to_string())?
            .clone();
        let page = inc
            .new_document
            .get_object_mut(page_id)
            .and_then(Object::as_dict_mut)
            .map_err(|e| e.to_string())?;
        if let Ok(Object::Dictionary(res)) = page.get_mut(b"Resources") {
            res.set(key.to_vec(), clone);
        }
    }
    Ok(())
}

/// Pre-clone every `/AcroForm` field dict so `write_form_fields` can `get_object_mut`
/// freely on `inc.new_document` without crossing back into `prev_documents`.
fn clone_acroform_tree(inc: &mut IncrementalDocument, cat_id: ObjectId) -> lopdf::Result<()> {
    let acroform_id = {
        let dict = inc.get_prev_documents().get_object(cat_id).and_then(Object::as_dict)?;
        match dict.get(b"AcroForm") {
            Ok(Object::Reference(id)) => *id,
            // Inline AcroForm dict: write_form_fields will promote it itself once the
            // catalog is in new_document.
            _ => return Ok(()),
        }
    };
    inc.opt_clone_object_to_new_document(acroform_id)?;

    let field_refs: Vec<ObjectId> = {
        let dict = inc
            .get_prev_documents()
            .get_object(acroform_id)
            .and_then(Object::as_dict)?;
        match dict.get(b"Fields") {
            Ok(Object::Array(arr)) => arr.iter().filter_map(|o| o.as_reference().ok()).collect(),
            _ => return Ok(()),
        }
    };

    let mut stack: Vec<ObjectId> = field_refs;
    let mut seen: HashSet<ObjectId> = HashSet::new();
    while let Some(field_id) = stack.pop() {
        if !seen.insert(field_id) {
            continue;
        }
        inc.opt_clone_object_to_new_document(field_id)?;
        let kids_refs: Vec<ObjectId> = inc
            .get_prev_documents()
            .get_object(field_id)
            .and_then(Object::as_dict)
            .ok()
            .and_then(|d| d.get(b"Kids").ok())
            .and_then(|o| match o {
                Object::Array(arr) => Some(arr.iter().filter_map(|o| o.as_reference().ok()).collect()),
                _ => None,
            })
            .unwrap_or_default();
        for kid in kids_refs {
            stack.push(kid);
        }
    }
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
    let meta_empty = Document::load(&input_path)
        .map(|d| writer::load_meta(&d).stream_ids.is_empty())
        .unwrap_or(true);
    if meta_empty {
        // No burn streams to clear; just copy bytes verbatim and avoid lopdf's
        // corruption-prone full rewrite.
        std::fs::copy(&input_path, &output_path).map_err(|e| e.to_string())?;
        return Ok(false);
    }

    let mut inc = IncrementalDocument::load(&input_path).map_err(|e| e.to_string())?;
    let meta = writer::load_meta(inc.get_prev_documents());
    for arr in meta.stream_ids.values() {
        let sid = (arr[0], arr[1] as u16);
        inc.opt_clone_object_to_new_document(sid)
            .map_err(|e| e.to_string())?;
        if let Ok(Object::Stream(s)) = inc.new_document.get_object_mut(sid) {
            s.content = vec![];
        }
    }
    inc.save(&output_path).map_err(|e| e.to_string())?;
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
///
/// Implementation notes
/// --------------------
/// We **MUST NOT** round-trip the document through `Document::load` + `save`.
/// lopdf 0.39's full-save path corrupts content streams on PDFs that use object
/// streams (`/ObjStm`) or indirect-reference `/Length` entries (typical of
/// iLovePDF/Acrobat output): some streams disappear, others lose their length
/// reference, and the resulting file renders as garbage in Chrome, Edge, and
/// pdf.js. Pure structural fixes (flattening the page tree, etc.) don't help —
/// the corruption is at the save layer.
///
/// Instead, we use `IncrementalDocument` which preserves the input bytes
/// verbatim and only appends an incremental update with the modified objects.
/// Page deletion in this model means: clone the deleted page's parent
/// `/Pages` dict into the new revision, drop the kid ref, decrement `/Count`,
/// walk up the parent chain decrementing each ancestor's `/Count`. Rotations
/// clone the leaf page dict and bump its `/Rotate`. The deleted page objects
/// stay in the original bytes but become unreferenced, so PDF viewers ignore
/// them.
#[tauri::command]
pub fn modify_pages(
    input_path: String,
    output_path: String,
    operations: Vec<PageOperation>,
) -> Result<(), String> {
    let mut inc = IncrementalDocument::load(&input_path).map_err(|e| e.to_string())?;

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

    let pages = inc.get_prev_documents().get_pages();

    // Rotations: clone the leaf, update /Rotate.
    for (&page_num, &rot_delta) in &rotation_map {
        let Some(&page_id) = pages.get(&page_num) else { continue };
        inc.opt_clone_object_to_new_document(page_id)
            .map_err(|e| e.to_string())?;
        if let Ok(Object::Dictionary(dict)) = inc.new_document.get_object_mut(page_id) {
            let current = dict
                .get(b"Rotate")
                .ok()
                .and_then(|o| o.as_i64().ok())
                .unwrap_or(0);
            let new_rotate = (current + rot_delta).rem_euclid(360);
            dict.set(b"Rotate", Object::Integer(new_rotate));
        }
    }

    // Deletions: drop the page from its parent's /Kids, then decrement /Count
    // up the chain. We do this per deleted page so multi-page deletes within
    // the same intermediate node accumulate correctly.
    for &page_num in &delete_set {
        let Some(&page_id) = pages.get(&page_num) else { continue };

        let direct_parent = inc
            .get_prev_documents()
            .get_object(page_id)
            .and_then(Object::as_dict)
            .and_then(|d| d.get(b"Parent"))
            .and_then(Object::as_reference)
            .map_err(|e| format!("page {page_num} has no /Parent: {e}"))?;

        inc.opt_clone_object_to_new_document(direct_parent)
            .map_err(|e| e.to_string())?;
        if let Ok(Object::Dictionary(dict)) = inc.new_document.get_object_mut(direct_parent) {
            if let Ok(Object::Array(kids)) = dict.get_mut(b"Kids") {
                kids.retain(|k| k.as_reference().ok() != Some(page_id));
            }
            let count = dict
                .get(b"Count")
                .and_then(Object::as_i64)
                .unwrap_or(0);
            dict.set(b"Count", Object::Integer(count - 1));
        }

        // Walk the rest of the parent chain decrementing /Count.
        let mut cursor = direct_parent;
        let mut depth = 0;
        loop {
            depth += 1;
            if depth > 64 {
                break;
            }
            let next = inc
                .get_prev_documents()
                .get_object(cursor)
                .and_then(Object::as_dict)
                .and_then(|d| d.get(b"Parent"))
                .and_then(Object::as_reference);
            let Ok(next_id) = next else { break };
            if next_id == cursor {
                break;
            }
            inc.opt_clone_object_to_new_document(next_id)
                .map_err(|e| e.to_string())?;
            if let Ok(Object::Dictionary(dict)) = inc.new_document.get_object_mut(next_id) {
                let count = dict
                    .get(b"Count")
                    .and_then(Object::as_i64)
                    .unwrap_or(0);
                dict.set(b"Count", Object::Integer(count - 1));
            }
            cursor = next_id;
        }
    }

    // Update CCAnnot metadata so editable annotations follow the page renumber.
    let mut meta = writer::load_meta(inc.get_prev_documents());
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

    // save_meta needs the catalog in new_document so it can set /CCAnnot on it.
    let cat_id = inc
        .get_prev_documents()
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|e| format!("No catalog: {e}"))?;
    inc.opt_clone_object_to_new_document(cat_id)
        .map_err(|e| e.to_string())?;
    writer::save_meta(&mut inc.new_document, &meta)?;

    inc.save(&output_path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{dictionary, ObjectId};

    /// Build a PDF whose page tree mirrors iLovePDF/Acrobat output: a root
    /// /Pages node with several intermediate /Pages nodes, each holding several
    /// leaf /Page entries. This is the structure lopdf 0.39's delete_pages
    /// corrupts.
    fn build_nested_pdf(pages_per_group: usize, groups: usize) -> (Document, Vec<ObjectId>) {
        let mut doc = Document::new();
        doc.version = "1.7".to_string();

        // Allocate the root /Pages first so leaves can reference its (eventual) id
        // via /Parent. We'll fill in its /Kids after building intermediates.
        let root_id = doc.add_object(dictionary! {
            b"Type" => Object::Name(b"Pages".to_vec()),
            b"Kids" => Object::Array(vec![]),
            b"Count" => Object::Integer(0),
        });

        let mut intermediate_ids = Vec::new();
        let mut leaf_ids = Vec::new();

        for _ in 0..groups {
            let mid_id = doc.add_object(dictionary! {
                b"Type" => Object::Name(b"Pages".to_vec()),
                b"Parent" => Object::Reference(root_id),
                b"Kids" => Object::Array(vec![]),
                b"Count" => Object::Integer(0),
            });
            let mut kids_for_mid = Vec::new();
            for _ in 0..pages_per_group {
                let leaf_id = doc.add_object(dictionary! {
                    b"Type" => Object::Name(b"Page".to_vec()),
                    b"Parent" => Object::Reference(mid_id),
                    b"MediaBox" => Object::Array(vec![
                        Object::Integer(0), Object::Integer(0),
                        Object::Integer(612), Object::Integer(792),
                    ]),
                    b"Resources" => Object::Dictionary(lopdf::Dictionary::new()),
                });
                kids_for_mid.push(Object::Reference(leaf_id));
                leaf_ids.push(leaf_id);
            }
            if let Ok(Object::Dictionary(d)) = doc.get_object_mut(mid_id) {
                d.set(b"Kids".to_vec(), Object::Array(kids_for_mid));
                d.set(b"Count".to_vec(), Object::Integer(pages_per_group as i64));
            }
            intermediate_ids.push(mid_id);
        }

        let root_kids: Vec<Object> = intermediate_ids
            .iter()
            .copied()
            .map(Object::Reference)
            .collect();
        let total = (pages_per_group * groups) as i64;
        if let Ok(Object::Dictionary(d)) = doc.get_object_mut(root_id) {
            d.set(b"Kids".to_vec(), Object::Array(root_kids));
            d.set(b"Count".to_vec(), Object::Integer(total));
        }

        let cat_id = doc.add_object(dictionary! {
            b"Type" => Object::Name(b"Catalog".to_vec()),
            b"Pages" => Object::Reference(root_id),
        });
        doc.trailer.set(b"Root", Object::Reference(cat_id));

        (doc, leaf_ids)
    }

    /// Regression: deleting page 1 from a multi-level page tree (iLovePDF
    /// layout: root → 4 intermediates of 20 pages) used to corrupt the tree
    /// because lopdf's `delete_pages` doesn't scrub the deleted ref from every
    /// intermediate /Kids. The saved file would end up with the deleted page
    /// still referenced and other intermediates with empty Kids/Count=0.
    #[test]
    fn delete_first_page_of_multi_level_tree() {
        let (mut doc, _leaves) = build_nested_pdf(20, 4);
        assert_eq!(doc.get_pages().len(), 80);

        let input = std::env::temp_dir().join("pdf-rider-test-multilevel-in.pdf");
        let output = std::env::temp_dir().join("pdf-rider-test-multilevel-out.pdf");
        doc.save(&input).unwrap();

        modify_pages(
            input.to_string_lossy().into_owned(),
            output.to_string_lossy().into_owned(),
            vec![PageOperation { page: 1, rotation: 0, delete: true }],
        )
        .unwrap();

        let saved = Document::load(&output).unwrap();
        assert_eq!(
            saved.get_pages().len(),
            79,
            "saved file should have 79 pages after deleting page 1"
        );

        // No /Pages node should still reference 80 entries, and the catalog's
        // root /Pages must report Count=79.
        let cat_id = saved
            .trailer
            .get(b"Root")
            .and_then(|o| o.as_reference())
            .unwrap();
        let root_pages_id = saved
            .get_object(cat_id)
            .and_then(Object::as_dict)
            .and_then(|d| d.get(b"Pages"))
            .and_then(Object::as_reference)
            .unwrap();
        let root = saved
            .get_object(root_pages_id)
            .and_then(Object::as_dict)
            .unwrap();
        let root_count = root.get(b"Count").and_then(Object::as_i64).unwrap();
        assert_eq!(root_count, 79, "root /Pages Count must be 79");

        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&output);
    }

    /// split_pdf produces one PDF per range with the expected page count
    /// and reuses existing filenames safely via `-2` suffixes.
    #[test]
    fn split_pdf_produces_one_file_per_range() {
        let (mut doc, _) = build_nested_pdf(5, 2); // 10 pages
        let dir = std::env::temp_dir().join(format!(
            "pdf-rider-split-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("in.pdf");
        doc.save(&input).unwrap();

        let ranges = vec![
            SplitRange { start: 1, end: 3 },
            SplitRange { start: 4, end: 4 },
            SplitRange { start: 5, end: 10 },
        ];
        let created = split_pdf(
            input.to_string_lossy().into_owned(),
            dir.to_string_lossy().into_owned(),
            "doc".to_string(),
            ranges,
        )
        .unwrap();

        assert_eq!(created.len(), 3);
        assert!(created[0].ends_with("doc-pages-1-3.pdf"));
        assert!(created[1].ends_with("doc-page-4.pdf"));
        assert!(created[2].ends_with("doc-pages-5-10.pdf"));

        let counts: Vec<usize> = created
            .iter()
            .map(|p| Document::load(p).unwrap().get_pages().len())
            .collect();
        assert_eq!(counts, vec![3, 1, 6]);

        // Re-running the same split should append `-2` suffixes to avoid collision.
        let created2 = split_pdf(
            input.to_string_lossy().into_owned(),
            dir.to_string_lossy().into_owned(),
            "doc".to_string(),
            vec![SplitRange { start: 1, end: 3 }],
        )
        .unwrap();
        assert!(created2[0].ends_with("doc-pages-1-3-2.pdf"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn split_pdf_rejects_out_of_range() {
        let (mut doc, _) = build_nested_pdf(3, 1); // 3 pages
        let dir = std::env::temp_dir().join(format!(
            "pdf-rider-split-oob-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("in.pdf");
        doc.save(&input).unwrap();

        let err = split_pdf(
            input.to_string_lossy().into_owned(),
            dir.to_string_lossy().into_owned(),
            "doc".to_string(),
            vec![SplitRange { start: 1, end: 99 }],
        )
        .unwrap_err();
        assert!(err.contains("Invalid range"), "got: {err}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Deleting a contiguous slice from the middle: 3 pages out of 20 across
    /// two intermediates must shift annotation pages correctly and leave the
    /// remaining 17 pages readable.
    #[test]
    fn delete_pages_across_intermediates() {
        let (mut doc, _) = build_nested_pdf(5, 4);
        assert_eq!(doc.get_pages().len(), 20);

        let input = std::env::temp_dir().join("pdf-rider-test-cross-in.pdf");
        let output = std::env::temp_dir().join("pdf-rider-test-cross-out.pdf");
        doc.save(&input).unwrap();

        let ops: Vec<PageOperation> = (1..=20)
            .map(|p| PageOperation {
                page: p,
                rotation: 0,
                delete: matches!(p, 5 | 6 | 7),
            })
            .collect();

        modify_pages(
            input.to_string_lossy().into_owned(),
            output.to_string_lossy().into_owned(),
            ops,
        )
        .unwrap();

        let saved = Document::load(&output).unwrap();
        assert_eq!(saved.get_pages().len(), 17);

        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&output);
    }
}

/// Extracts selected pages from a PDF into a new file, preserving vector text.
///
/// Implementation note: we MUST NOT round-trip through `Document::save`. lopdf
/// 0.39's full-rewrite path corrupts content streams on PDFs that use object
/// streams (`/ObjStm`) or indirect-reference `/Length` entries (typical of
/// Acrobat / iLovePDF / Word output) — text comes out garbled. We use
/// `IncrementalDocument` instead, which preserves the input bytes verbatim and
/// drops unwanted pages by mutating only the page-tree metadata in an
/// incremental update. The dropped page objects remain in the bytes but become
/// unreferenced, so viewers ignore them.
#[tauri::command]
pub fn extract_pdf_pages(
    input_path: String,
    output_path: String,
    pages: Vec<u32>,
) -> Result<(), String> {
    let keep: HashSet<u32> = pages.into_iter().collect();
    extract_pages_via_incremental(&input_path, &output_path, &keep)
}

/// Keeps only the pages in `keep` (1-based). Uses `IncrementalDocument` so
/// content streams stay byte-perfect. The file size is dominated by the
/// original bytes — unreferenced page objects stay in the file as orphans.
fn extract_pages_via_incremental(
    input_path: &str,
    output_path: &str,
    keep: &HashSet<u32>,
) -> Result<(), String> {
    let mut inc = IncrementalDocument::load(input_path).map_err(|e| e.to_string())?;
    let total = inc.get_prev_documents().get_pages().len() as u32;

    let keeping_all = total > 0
        && keep.len() as u32 >= total
        && (1..=total).all(|p| keep.contains(&p));
    if keeping_all {
        // Byte-perfect copy — no incremental update needed.
        std::fs::copy(input_path, output_path).map_err(|e| e.to_string())?;
        return Ok(());
    }

    let delete_set: HashSet<u32> = (1..=total).filter(|p| !keep.contains(p)).collect();
    apply_page_deletions(&mut inc, &delete_set)?;
    inc.save(output_path).map_err(|e| e.to_string())?;
    Ok(())
}

/// Drops `delete_set` pages from `inc` by walking each page up to its parent,
/// removing it from /Kids, and decrementing /Count along the chain. Also
/// updates the `CCAnnot` catalog entry so editable annotations follow the page
/// renumber.
///
/// This is the same algorithm `modify_pages` uses for deletions, factored out
/// so split / extract can reuse it.
fn apply_page_deletions(
    inc: &mut IncrementalDocument,
    delete_set: &HashSet<u32>,
) -> Result<(), String> {
    if delete_set.is_empty() {
        return Ok(());
    }

    let pages = inc.get_prev_documents().get_pages();

    for &page_num in delete_set {
        let Some(&page_id) = pages.get(&page_num) else { continue };

        let direct_parent = inc
            .get_prev_documents()
            .get_object(page_id)
            .and_then(Object::as_dict)
            .and_then(|d| d.get(b"Parent"))
            .and_then(Object::as_reference)
            .map_err(|e| format!("page {page_num} has no /Parent: {e}"))?;

        inc.opt_clone_object_to_new_document(direct_parent)
            .map_err(|e| e.to_string())?;
        if let Ok(Object::Dictionary(dict)) = inc.new_document.get_object_mut(direct_parent) {
            if let Ok(Object::Array(kids)) = dict.get_mut(b"Kids") {
                kids.retain(|k| k.as_reference().ok() != Some(page_id));
            }
            let count = dict.get(b"Count").and_then(Object::as_i64).unwrap_or(0);
            dict.set(b"Count", Object::Integer(count - 1));
        }

        // Walk up the parent chain decrementing /Count.
        let mut cursor = direct_parent;
        let mut depth = 0;
        loop {
            depth += 1;
            if depth > 64 {
                break;
            }
            let next = inc
                .get_prev_documents()
                .get_object(cursor)
                .and_then(Object::as_dict)
                .and_then(|d| d.get(b"Parent"))
                .and_then(Object::as_reference);
            let Ok(next_id) = next else { break };
            if next_id == cursor {
                break;
            }
            inc.opt_clone_object_to_new_document(next_id)
                .map_err(|e| e.to_string())?;
            if let Ok(Object::Dictionary(dict)) = inc.new_document.get_object_mut(next_id) {
                let count = dict.get(b"Count").and_then(Object::as_i64).unwrap_or(0);
                dict.set(b"Count", Object::Integer(count - 1));
            }
            cursor = next_id;
        }
    }

    // Update CCAnnot metadata so editable annotations follow the page renumber.
    let mut meta = writer::load_meta(inc.get_prev_documents());
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

    let cat_id = inc
        .get_prev_documents()
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|e| format!("No catalog: {e}"))?;
    inc.opt_clone_object_to_new_document(cat_id)
        .map_err(|e| e.to_string())?;
    writer::save_meta(&mut inc.new_document, &meta)?;

    Ok(())
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct SplitRange {
    /// 1-based inclusive
    pub start: u32,
    /// 1-based inclusive
    pub end: u32,
}

/// Splits a PDF into multiple files, one per range. Each output file contains
/// the pages [start, end] (inclusive, 1-based) from `input_path`.
///
/// Output filenames: `{base_name}-pages-{start}-{end}.pdf` (or
/// `{base_name}-page-{n}.pdf` when start == end). If a file already exists,
/// `-N` is appended before the extension. Returns the list of created paths.
#[tauri::command]
pub fn split_pdf(
    input_path: String,
    output_dir: String,
    base_name: String,
    ranges: Vec<SplitRange>,
) -> Result<Vec<String>, String> {
    if ranges.is_empty() {
        return Err("No ranges to split".to_string());
    }

    let total = Document::load(&input_path)
        .map_err(|e| e.to_string())?
        .get_pages()
        .len() as u32;

    let out_dir = std::path::PathBuf::from(&output_dir);
    if !out_dir.is_dir() {
        return Err(format!("Output directory not found: {output_dir}"));
    }

    let safe_base = sanitize_filename(&base_name);
    let mut created: Vec<String> = Vec::with_capacity(ranges.len());

    for range in &ranges {
        if range.start == 0 || range.end < range.start || range.end > total {
            return Err(format!(
                "Invalid range {}-{} (document has {total} page{})",
                range.start,
                range.end,
                if total == 1 { "" } else { "s" },
            ));
        }

        let stem = if range.start == range.end {
            format!("{safe_base}-page-{}", range.start)
        } else {
            format!("{safe_base}-pages-{}-{}", range.start, range.end)
        };
        let target = unique_path(&out_dir, &stem, "pdf");

        let keep: HashSet<u32> = (range.start..=range.end).collect();
        extract_pages_via_incremental(
            &input_path,
            target.to_string_lossy().as_ref(),
            &keep,
        )?;
        created.push(target.to_string_lossy().into_owned());
    }

    Ok(created)
}

/// Strips characters that are illegal in filenames on Windows/macOS/Linux.
fn sanitize_filename(name: &str) -> String {
    let trimmed = name.trim().trim_end_matches('.');
    let cleaned: String = trimmed
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    if cleaned.is_empty() {
        "document".to_string()
    } else {
        cleaned
    }
}

/// Returns `{dir}/{stem}.{ext}`, appending `-N` to the stem until a free path is found.
fn unique_path(dir: &std::path::Path, stem: &str, ext: &str) -> std::path::PathBuf {
    let candidate = dir.join(format!("{stem}.{ext}"));
    if !candidate.exists() {
        return candidate;
    }
    for n in 2u32..=9999 {
        let p = dir.join(format!("{stem}-{n}.{ext}"));
        if !p.exists() {
            return p;
        }
    }
    dir.join(format!("{stem}-conflict.{ext}"))
}
