//! Combines several PDF files into a single new one.
//!
//! Each input is loaded with lopdf, its object ids are shifted past the ids
//! already used by the output, and its objects are moved over as they are:
//! content streams, fonts, images and annotations are never decoded or
//! re-encoded. Only the document-level structures are rebuilt:
//!
//! - the page tree: one flat `/Pages` node listing every page in order. The
//!   attributes a page inherits from its old tree (`/Resources`, `/MediaBox`,
//!   `/CropBox`, `/Rotate`) are pinned on the page first, so sizes and
//!   orientation do not change;
//! - bookmarks: the top-level items of each file follow each other under one
//!   outline root;
//! - form fields: one `/AcroForm` listing every file's fields; a top-level
//!   field whose name is already taken gets a `_2`, `_3`… suffix, so fields of
//!   different files never share a value;
//! - named destinations (and the links and bookmarks using them), optional
//!   content groups, and PDF Rider's editable annotations (`CCAnnot`), whose
//!   page numbers and stream ids follow the new numbering.
//!
//! Document info, XMP metadata and viewer preferences come from the first
//! file. Encrypted inputs are refused: their content would have to be
//! decrypted and written out unprotected.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId};

use super::writer::{self, AnnotationMeta};

/// Page attributes a page may inherit from its ancestors in the page tree.
const INHERITABLE: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];

/// Catalog entries carried over from the first file.
const FIRST_CATALOG_KEYS: [&[u8]; 6] = [
    b"Metadata",
    b"ViewerPreferences",
    b"PageLayout",
    b"PageMode",
    b"Lang",
    b"OpenAction",
];

/// Merges `input_paths` (in that order; a file may appear several times)
/// into a new PDF written to `output_path`, which may be one of the inputs.
/// Returns the page count of the merged file. Nothing is written on error.
pub fn merge_files(input_paths: &[String], output_path: &str) -> Result<u32, String> {
    if input_paths.len() < 2 {
        return Err("Select at least two PDF files to merge.".to_string());
    }
    let inputs = input_paths
        .iter()
        .map(|p| load_input(p))
        .collect::<Result<Vec<_>, _>>()?;
    let mut merged = merge_documents(inputs)?;
    let page_count = merged.get_pages().len() as u32;

    // Write next to the target, then move it into place, so a failure never
    // leaves a half-written file — or a truncated input when the user saves
    // over one of the files being merged.
    let tmp = format!("{output_path}.pdf-rider-merge.tmp");
    let written = merged
        .save(&tmp)
        .map(drop)
        .and_then(|_| std::fs::rename(&tmp, output_path));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "Could not write {}: {e}. Check that the folder is writable and the file is not open in another program.",
            display_name(output_path)
        ));
    }
    Ok(page_count)
}

/// Checks that `path` can be merged and returns its page count.
pub fn check_input(path: &str) -> Result<u32, String> {
    load_input(path).map(|i| i.doc.get_pages().len() as u32)
}

struct MergeInput {
    doc: Document,
}

fn display_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

fn encrypted_error(name: &str) -> String {
    format!(
        "{name} is encrypted (password-protected or with usage restrictions), so it cannot be merged. Remove the protection in the app that created it, then try again."
    )
}

fn load_input(path: &str) -> Result<MergeInput, String> {
    let name = display_name(path);
    let bytes = std::fs::read(path).map_err(|e| format!("{name} could not be read: {e}."))?;
    let doc = match Document::load_mem(&bytes) {
        Ok(doc) => doc,
        Err(lopdf::Error::InvalidPassword | lopdf::Error::Decryption(_)) => {
            return Err(encrypted_error(&name))
        }
        Err(e) => return Err(format!("{name} is not a valid PDF or is damaged ({e}).")),
    };
    if doc.is_encrypted() || doc.was_encrypted() {
        return Err(encrypted_error(&name));
    }
    if doc.get_pages().is_empty() {
        return Err(format!("{name} has no pages."));
    }
    Ok(MergeInput { doc })
}

/// What the merge has gathered from the files appended so far.
#[derive(Default)]
struct MergeState {
    kids: Vec<Object>,
    page_offset: u32,
    meta: AnnotationMeta,
    outline_items: Vec<ObjectId>,
    outline_count: i64,
    acroform: Option<Dictionary>,
    fields: Vec<Object>,
    field_names: HashSet<Vec<u8>>,
    /// `/Names /Dests` name tree entries (string keys), sorted.
    tree_dests: BTreeMap<Vec<u8>, Object>,
    /// Catalog `/Dests` entries (name keys).
    dict_dests: BTreeMap<Vec<u8>, Object>,
    ocgs: Vec<Object>,
    oc_config: Option<Dictionary>,
    oc_order: Vec<Object>,
    oc_on: Vec<Object>,
    oc_off: Vec<Object>,
    first_catalog: Option<Dictionary>,
    info: Option<Object>,
}

fn merge_documents(inputs: Vec<MergeInput>) -> Result<Document, String> {
    let version = inputs
        .iter()
        .map(|i| i.doc.version.clone())
        .max()
        .unwrap_or_else(|| "1.7".to_string());
    let mut out = Document::with_version(version);
    let pages_id = out.new_object_id();
    let mut st = MergeState::default();
    for input in inputs {
        append(&mut out, &mut st, pages_id, input.doc)?;
    }
    finish(&mut out, st, pages_id)?;
    Ok(out)
}

/// Resolves one level of indirection.
fn deref<'a>(doc: &'a Document, obj: &'a Object) -> &'a Object {
    match obj {
        Object::Reference(id) => doc.get_object(*id).unwrap_or(obj),
        o => o,
    }
}

fn deref_dict<'a>(doc: &'a Document, obj: Option<&'a Object>) -> Option<&'a Dictionary> {
    obj.and_then(|o| deref(doc, o).as_dict().ok())
}

fn deref_array(doc: &Document, obj: Option<&Object>) -> Vec<Object> {
    obj.and_then(|o| deref(doc, o).as_array().ok().cloned())
        .unwrap_or_default()
}

fn append(out: &mut Document, st: &mut MergeState, pages_id: ObjectId, mut doc: Document) -> Result<(), String> {
    let meta = writer::load_meta(&doc);
    let shift = out.max_id;
    shift_ids(&mut doc, shift);

    let catalog_id = doc
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|e| format!("no document catalog: {e}"))?;
    let catalog = doc.get_dictionary(catalog_id).map_err(|e| e.to_string())?.clone();
    let pages = doc.get_pages();
    let first = st.first_catalog.is_none();

    // Objects of the old document structure, rebuilt in `finish`.
    let mut skip: HashSet<ObjectId> = HashSet::from([catalog_id]);
    for key in [&b"CCAnnot"[..], b"Outlines", b"AcroForm", b"OCProperties"] {
        if let Ok(Object::Reference(id)) = catalog.get(key) {
            skip.insert(*id);
        }
    }
    if !first {
        if let Ok(Object::Reference(id)) = doc.trailer.get(b"Info") {
            skip.insert(*id);
        }
    }

    collect_named_dests(&mut doc, st, &catalog);

    for &page_id in pages.values() {
        let inherited = inherited_attributes(&doc, page_id);
        let page = doc
            .get_object_mut(page_id)
            .and_then(Object::as_dict_mut)
            .map_err(|e| format!("page object {page_id:?}: {e}"))?;
        for (key, value) in inherited {
            if !page.has(&key) {
                page.set(key, value);
            }
        }
        page.set("Parent", pages_id);
        st.kids.push(Object::Reference(page_id));
    }

    collect_outlines(&doc, st, &catalog);
    collect_form(&mut doc, st, &catalog);
    collect_optional_content(&doc, st, &catalog);

    for mut ann in meta.annotations {
        ann.set_page(ann.page() + st.page_offset);
        st.meta.annotations.push(ann);
    }
    for (page, [id, gen]) in meta.stream_ids {
        st.meta.stream_ids.insert(page + st.page_offset, [id + shift, gen]);
    }

    if first {
        st.first_catalog = Some(catalog);
        st.info = doc.trailer.get(b"Info").ok().cloned();
    }

    for (id, obj) in std::mem::take(&mut doc.objects) {
        if skip.contains(&id) || is_structural(&obj) {
            continue;
        }
        out.max_id = out.max_id.max(id.0);
        out.objects.insert(id, obj);
    }
    out.max_id = out.max_id.max(doc.max_id);
    st.page_offset += pages.len() as u32;
    Ok(())
}

/// Page-tree nodes and file-structure objects of an input file.
fn is_structural(obj: &Object) -> bool {
    match obj {
        Object::Dictionary(d) => d.has_type(b"Pages") || d.has(b"Linearized"),
        Object::Stream(s) => s.dict.has_type(b"ObjStm") || s.dict.has_type(b"XRef"),
        _ => false,
    }
}

/// Adds `shift` to every object id of `doc`, and to every reference to one.
fn shift_ids(doc: &mut Document, shift: u32) {
    fn shift_refs(obj: &mut Object, shift: u32) {
        match obj {
            Object::Reference(id) => id.0 += shift,
            Object::Array(items) => items.iter_mut().for_each(|o| shift_refs(o, shift)),
            Object::Dictionary(d) => d.iter_mut().for_each(|(_, o)| shift_refs(o, shift)),
            Object::Stream(s) => s.dict.iter_mut().for_each(|(_, o)| shift_refs(o, shift)),
            _ => {}
        }
    }
    let objects = std::mem::take(&mut doc.objects);
    doc.objects = objects
        .into_iter()
        .map(|((id, gen), mut obj)| {
            shift_refs(&mut obj, shift);
            ((id + shift, gen), obj)
        })
        .collect();
    doc.trailer.iter_mut().for_each(|(_, o)| shift_refs(o, shift));
    doc.max_id += shift;
}

/// The inheritable attributes `page_id` does not set itself, taken from its
/// nearest ancestor that does.
fn inherited_attributes(doc: &Document, page_id: ObjectId) -> Vec<(Vec<u8>, Object)> {
    let Ok(page) = doc.get_dictionary(page_id) else { return vec![] };
    let mut missing: Vec<&[u8]> = INHERITABLE.iter().copied().filter(|k| !page.has(k)).collect();
    let mut found = vec![];
    let mut parent = page.get(b"Parent").and_then(Object::as_reference).ok();
    let mut seen = HashSet::from([page_id]);
    while let Some(id) = parent {
        if missing.is_empty() || !seen.insert(id) {
            break;
        }
        let Ok(node) = doc.get_dictionary(id) else { break };
        missing.retain(|key| match node.get(key) {
            Ok(value) => {
                found.push((key.to_vec(), value.clone()));
                false
            }
            Err(_) => true,
        });
        parent = node.get(b"Parent").and_then(Object::as_reference).ok();
    }
    found
}

/// Returns `base`, or `base` followed by `_2`, `_3`… — the first that `taken`
/// rejects not. UTF-16 text strings get a UTF-16 suffix.
fn unique_name(base: &[u8], taken: impl Fn(&[u8]) -> bool) -> Vec<u8> {
    if !taken(base) {
        return base.to_vec();
    }
    let utf16 = base.starts_with(&[0xFE, 0xFF]);
    (2u32..)
        .map(|n| {
            let suffix = format!("_{n}");
            let mut name = base.to_vec();
            if utf16 {
                name.extend(suffix.encode_utf16().flat_map(u16::to_be_bytes));
            } else {
                name.extend(suffix.bytes());
            }
            name
        })
        .find(|name| !taken(name))
        .expect("an unused suffix exists")
}

/// Gathers the named destinations of `doc` into `st`, renaming those whose
/// name an earlier file already uses (and every use of them in `doc`).
fn collect_named_dests(doc: &mut Document, st: &mut MergeState, catalog: &Dictionary) {
    let mut tree = vec![];
    if let Some(names) = deref_dict(doc, catalog.get(b"Names").ok()) {
        if let Ok(root) = names.get(b"Dests") {
            flatten_name_tree(doc, root, &mut tree, 0);
        }
    }
    let dict: Vec<(Vec<u8>, Object)> = deref_dict(doc, catalog.get(b"Dests").ok())
        .map(|d| d.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();

    let mut string_renames = HashMap::new();
    for (key, value) in tree {
        let name = unique_name(&key, |n| st.tree_dests.contains_key(n));
        if name != key {
            string_renames.insert(key, name.clone());
        }
        st.tree_dests.insert(name, value);
    }
    let mut name_renames = HashMap::new();
    for (key, value) in dict {
        let name = unique_name(&key, |n| st.dict_dests.contains_key(n));
        if name != key {
            name_renames.insert(key, name.clone());
        }
        st.dict_dests.insert(name, value);
    }
    if string_renames.is_empty() && name_renames.is_empty() {
        return;
    }

    // Links (/Dest, GoTo /D) and bookmarks refer to named destinations by
    // string (name tree) or by name (catalog /Dests).
    let rename = |value: &mut Object| match value {
        Object::String(s, _) => {
            if let Some(new) = string_renames.get(s.as_slice()) {
                *s = new.clone();
            }
        }
        Object::Name(n) => {
            if let Some(new) = name_renames.get(n.as_slice()) {
                *n = new.clone();
            }
        }
        _ => {}
    };
    for obj in doc.objects.values_mut() {
        let Object::Dictionary(d) = obj else { continue };
        if let Ok(dest) = d.get_mut(b"Dest") {
            rename(dest);
        }
        if let Ok(Object::Dictionary(action)) = d.get_mut(b"A") {
            if let Ok(dest) = action.get_mut(b"D") {
                rename(dest);
            }
        }
        if d.get(b"S").and_then(Object::as_name).ok() == Some(b"GoTo".as_slice()) {
            if let Ok(dest) = d.get_mut(b"D") {
                rename(dest);
            }
        }
    }
}

fn flatten_name_tree(doc: &Document, node: &Object, out: &mut Vec<(Vec<u8>, Object)>, depth: u32) {
    let Ok(node) = deref(doc, node).as_dict() else { return };
    if depth > 32 {
        return;
    }
    let names = deref_array(doc, node.get(b"Names").ok());
    for pair in names.chunks(2) {
        if let [Object::String(key, _), value] = pair {
            out.push((key.clone(), value.clone()));
        }
    }
    for kid in deref_array(doc, node.get(b"Kids").ok()) {
        flatten_name_tree(doc, &kid, out, depth + 1);
    }
}

fn collect_outlines(doc: &Document, st: &mut MergeState, catalog: &Dictionary) {
    let Some(root) = deref_dict(doc, catalog.get(b"Outlines").ok()) else { return };
    let mut seen = HashSet::new();
    let mut cur = root.get(b"First").and_then(Object::as_reference).ok();
    let mut items = 0;
    while let Some(id) = cur {
        if !seen.insert(id) {
            break;
        }
        let Ok(item) = doc.get_dictionary(id) else { break };
        st.outline_items.push(id);
        items += 1;
        cur = item.get(b"Next").and_then(Object::as_reference).ok();
    }
    st.outline_count += root
        .get(b"Count")
        .and_then(Object::as_i64)
        .map(i64::abs)
        .unwrap_or(items);
}

/// Inlines `dict[key]` and, one level down, its dictionary values (`/DR`
/// and its `/Font`), so they can be merged entry by entry.
fn inline_resources(doc: &Document, dict: &Dictionary) -> Dictionary {
    let mut out = Dictionary::new();
    for (key, value) in dict.iter() {
        match deref(doc, value) {
            Object::Dictionary(d) => out.set(key.clone(), Object::Dictionary(d.clone())),
            _ => out.set(key.clone(), value.clone()),
        }
    }
    out
}

fn collect_form(doc: &mut Document, st: &mut MergeState, catalog: &Dictionary) {
    let Some(form) = deref_dict(doc, catalog.get(b"AcroForm").ok()).cloned() else { return };
    let fields = deref_array(doc, form.get(b"Fields").ok());
    for field in fields {
        if let Ok(id) = field.as_reference() {
            if let Ok(Object::Dictionary(d)) = doc.get_object_mut(id) {
                if let Ok(Object::String(name, _)) = d.get_mut(b"T") {
                    let unique = unique_name(name, |n| st.field_names.contains(n));
                    st.field_names.insert(unique.clone());
                    *name = unique;
                }
            }
        }
        st.fields.push(field);
    }

    let dr = deref_dict(doc, form.get(b"DR").ok()).map(|d| inline_resources(doc, d));
    let merged = st.acroform.get_or_insert_with(Dictionary::new);
    for (key, value) in form.iter() {
        if key.as_slice() != b"Fields" && key.as_slice() != b"DR" && !merged.has(key) {
            merged.set(key.clone(), value.clone());
        }
    }
    if form.get(b"NeedAppearances").and_then(Object::as_bool).unwrap_or(false) {
        merged.set("NeedAppearances", true);
    }
    let Some(dr) = dr else { return };
    if !merged.has(b"DR") {
        merged.set("DR", Dictionary::new());
    }
    let Ok(Object::Dictionary(merged_dr)) = merged.get_mut(b"DR") else { return };
    for (key, value) in dr.iter() {
        match (merged_dr.get_mut(key), value) {
            (Ok(Object::Dictionary(existing)), Object::Dictionary(new)) => {
                for (k, v) in new.iter() {
                    if !existing.has(k) {
                        existing.set(k.clone(), v.clone());
                    }
                }
            }
            (Ok(_), _) => {}
            (Err(_), _) => merged_dr.set(key.clone(), value.clone()),
        }
    }
}

fn collect_optional_content(doc: &Document, st: &mut MergeState, catalog: &Dictionary) {
    let Some(props) = deref_dict(doc, catalog.get(b"OCProperties").ok()) else { return };
    st.ocgs.extend(deref_array(doc, props.get(b"OCGs").ok()));
    let Some(config) = deref_dict(doc, props.get(b"D").ok()) else { return };
    st.oc_order.extend(deref_array(doc, config.get(b"Order").ok()));
    st.oc_on.extend(deref_array(doc, config.get(b"ON").ok()));
    st.oc_off.extend(deref_array(doc, config.get(b"OFF").ok()));
    if st.oc_config.is_none() {
        let mut config = config.clone();
        for key in [&b"Order"[..], b"ON", b"OFF"] {
            config.remove(key);
        }
        st.oc_config = Some(config);
    }
}

fn finish(out: &mut Document, st: MergeState, pages_id: ObjectId) -> Result<(), String> {
    let count = st.kids.len() as i64;
    out.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => st.kids, "Count" => count }),
    );

    let mut catalog = dictionary! { "Type" => "Catalog", "Pages" => pages_id };
    if let Some(first) = &st.first_catalog {
        for key in FIRST_CATALOG_KEYS {
            if let Ok(value) = first.get(key) {
                catalog.set(key, value.clone());
            }
        }
    }

    if let (Some(&first), Some(&last)) = (st.outline_items.first(), st.outline_items.last()) {
        let root = out.add_object(dictionary! {
            "Type" => "Outlines", "First" => first, "Last" => last, "Count" => st.outline_count,
        });
        let items = &st.outline_items;
        for (i, &id) in items.iter().enumerate() {
            if let Ok(Object::Dictionary(item)) = out.get_object_mut(id) {
                item.set("Parent", root);
                match i.checked_sub(1).map(|p| items[p]) {
                    Some(prev) => item.set("Prev", prev),
                    None => drop(item.remove(b"Prev")),
                }
                match items.get(i + 1) {
                    Some(&next) => item.set("Next", next),
                    None => drop(item.remove(b"Next")),
                }
            }
        }
        catalog.set("Outlines", root);
    }

    if let Some(mut form) = st.acroform {
        form.set("Fields", st.fields);
        catalog.set("AcroForm", form);
    }

    if !st.tree_dests.is_empty() {
        let names: Vec<Object> = st
            .tree_dests
            .into_iter()
            .flat_map(|(k, v)| [Object::String(k, lopdf::StringFormat::Literal), v])
            .collect();
        let tree = out.add_object(dictionary! { "Names" => names });
        catalog.set("Names", dictionary! { "Dests" => tree });
    }
    if !st.dict_dests.is_empty() {
        let mut dests = Dictionary::new();
        for (k, v) in st.dict_dests {
            dests.set(k, v);
        }
        let id = out.add_object(dests);
        catalog.set("Dests", id);
    }

    if !st.ocgs.is_empty() {
        let mut config = st.oc_config.unwrap_or_default();
        for (key, values) in [("Order", st.oc_order), ("ON", st.oc_on), ("OFF", st.oc_off)] {
            if !values.is_empty() {
                config.set(key, values);
            }
        }
        catalog.set("OCProperties", dictionary! { "OCGs" => st.ocgs, "D" => config });
    }

    let catalog_id = out.add_object(catalog);
    out.trailer.set("Root", catalog_id);
    if let Some(info) = st.info {
        out.trailer.set("Info", info);
    }
    if !st.meta.annotations.is_empty() || !st.meta.stream_ids.is_empty() {
        writer::save_meta(out, &st.meta)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::models::{Annotation, RectAnnotation, RgbColor};
    use crate::pdf::writer::{self, AnnotationMeta};
    use lopdf::{dictionary, Object, ObjectId, Stream, StringFormat};
    use std::collections::HashMap;
    use std::path::PathBuf;

    const A4: [i64; 4] = [0, 0, 595, 842];
    const LETTER_LANDSCAPE: [i64; 4] = [0, 0, 792, 612];
    const A5: [i64; 4] = [0, 0, 420, 595];

    /// A scratch directory removed when dropped.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "pdf-rider-merge-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
        fn path(&self, name: &str) -> String {
            self.0.join(name).to_string_lossy().into_owned()
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn content_for(marker: &str) -> Vec<u8> {
        format!("BT /F1 12 Tf 72 72 Td ({marker}) Tj ET").into_bytes()
    }

    fn arr(r: [i64; 4]) -> Object {
        Object::Array(r.iter().map(|&v| Object::Integer(v)).collect())
    }

    /// Builds a document whose page tree is root /Pages → one intermediate
    /// /Pages per group → leaf pages. `/MediaBox`, `/Rotate` and `/Resources`
    /// are set on the intermediate node (inherited by its pages) unless a page
    /// sets its own `MediaBox`.
    struct Group {
        media: [i64; 4],
        rotate: i64,
        markers: Vec<&'static str>,
    }

    fn build_doc(groups: &[Group]) -> (Document, Vec<ObjectId>) {
        let mut doc = Document::with_version("1.7");
        let root_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let mut root_kids = vec![];
        let mut leaves = vec![];
        let mut total = 0;
        for g in groups {
            let mid_id = doc.new_object_id();
            let mut kids = vec![];
            for m in &g.markers {
                let content = doc.add_object(Stream::new(dictionary! {}, content_for(m)));
                let page = doc.add_object(dictionary! {
                    "Type" => "Page",
                    "Parent" => mid_id,
                    "Contents" => content,
                });
                kids.push(Object::Reference(page));
                leaves.push(page);
            }
            total += kids.len() as i64;
            doc.objects.insert(
                mid_id,
                Object::Dictionary(dictionary! {
                    "Type" => "Pages",
                    "Parent" => root_id,
                    "Count" => kids.len() as i64,
                    "Kids" => kids,
                    "MediaBox" => arr(g.media),
                    "Rotate" => g.rotate,
                    "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
                }),
            );
            root_kids.push(Object::Reference(mid_id));
        }
        doc.objects.insert(
            root_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => root_kids, "Count" => total,
            }),
        );
        let cat = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => root_id });
        doc.trailer.set("Root", cat);
        (doc, leaves)
    }

    fn simple(media: [i64; 4], rotate: i64, markers: &[&'static str]) -> (Document, Vec<ObjectId>) {
        build_doc(&[Group { media, rotate, markers: markers.to_vec() }])
    }

    fn catalog_mut(doc: &mut Document) -> &mut lopdf::Dictionary {
        let id = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
        doc.get_object_mut(id).unwrap().as_dict_mut().unwrap()
    }

    fn catalog(doc: &Document) -> &lopdf::Dictionary {
        doc.catalog().unwrap()
    }

    fn save(doc: &mut Document, path: &str) {
        doc.save(path).unwrap();
    }

    /// The marker text of each page, in page order.
    fn markers(doc: &Document) -> Vec<String> {
        doc.get_pages()
            .values()
            .map(|&id| {
                let bytes = doc.get_page_content(id).unwrap();
                let s = String::from_utf8_lossy(&bytes).into_owned();
                let start = s.find('(').unwrap() + 1;
                let end = s.find(')').unwrap();
                s[start..end].to_string()
            })
            .collect()
    }

    fn page_attr(doc: &Document, page: ObjectId, key: &[u8]) -> Object {
        let d = doc.get_dictionary(page).unwrap();
        let v = d.get(key).unwrap_or_else(|_| panic!("page has no direct {}", String::from_utf8_lossy(key)));
        match v {
            Object::Reference(id) => doc.get_object(*id).unwrap().clone(),
            o => o.clone(),
        }
    }

    fn ints(o: &Object) -> Vec<i64> {
        o.as_array().unwrap().iter().map(|v| v.as_float().unwrap() as i64).collect()
    }

    fn text(o: &Object) -> String {
        String::from_utf8_lossy(o.as_str().unwrap()).into_owned()
    }

    #[test]
    fn merges_documents_of_different_sizes_in_order() {
        let dir = TempDir::new("sizes");
        let (mut a, _) = simple(A4, 0, &["A1", "A2"]);
        let (mut b, _) = simple(LETTER_LANDSCAPE, 90, &["B1"]);
        let (mut c, _) = build_doc(&[
            Group { media: A5, rotate: 0, markers: vec!["C1", "C2"] },
            Group { media: A5, rotate: 180, markers: vec!["C3"] },
        ]);
        let (pa, pb, pc) = (dir.path("a.pdf"), dir.path("b.pdf"), dir.path("c.pdf"));
        save(&mut a, &pa);
        save(&mut b, &pb);
        save(&mut c, &pc);
        let out = dir.path("merged.pdf");

        let count = merge_files(&[pa, pb, pc], &out).unwrap();
        assert_eq!(count, 6);

        let merged = Document::load(&out).unwrap();
        assert_eq!(markers(&merged), ["A1", "A2", "B1", "C1", "C2", "C3"]);

        let pages: Vec<ObjectId> = merged.get_pages().values().copied().collect();
        let sizes: Vec<Vec<i64>> = pages.iter().map(|&p| ints(&page_attr(&merged, p, b"MediaBox"))).collect();
        assert_eq!(sizes, [A4.to_vec(), A4.to_vec(), LETTER_LANDSCAPE.to_vec(), A5.to_vec(), A5.to_vec(), A5.to_vec()]);
        let rotations: Vec<i64> = pages.iter().map(|&p| page_attr(&merged, p, b"Rotate").as_i64().unwrap()).collect();
        assert_eq!(rotations, [0, 0, 90, 0, 0, 180]);
        // Inherited /Resources were pinned on each page, so the font still resolves.
        for &p in &pages {
            let res = page_attr(&merged, p, b"Resources");
            assert!(res.as_dict().unwrap().get(b"Font").is_ok());
        }
    }

    #[test]
    fn follows_the_given_order() {
        let dir = TempDir::new("order");
        let (mut a, _) = simple(A4, 0, &["A1", "A2"]);
        let (mut b, _) = simple(A5, 0, &["B1"]);
        let (pa, pb) = (dir.path("a.pdf"), dir.path("b.pdf"));
        save(&mut a, &pa);
        save(&mut b, &pb);
        let out = dir.path("merged.pdf");

        merge_files(&[pb.clone(), pa.clone(), pb], &out).unwrap();
        let merged = Document::load(&out).unwrap();
        assert_eq!(markers(&merged), ["B1", "A1", "A2", "B1"]);
    }

    #[test]
    fn may_overwrite_one_of_its_inputs() {
        let dir = TempDir::new("overwrite");
        let (mut a, _) = simple(A4, 0, &["A1"]);
        let (mut b, _) = simple(A5, 0, &["B1"]);
        let (pa, pb) = (dir.path("a.pdf"), dir.path("b.pdf"));
        save(&mut a, &pa);
        save(&mut b, &pb);

        merge_files(&[pa.clone(), pb], &pa).unwrap();
        assert_eq!(markers(&Document::load(&pa).unwrap()), ["A1", "B1"]);
    }

    /// Content streams with an indirect /Length, stored in a file that uses
    /// object streams and a cross-reference stream (Acrobat / iLovePDF style),
    /// come out byte-identical.
    #[test]
    fn keeps_content_of_object_stream_pdfs_intact() {
        let dir = TempDir::new("objstm");
        let (mut a, leaves) = simple(A4, 0, &["A1", "A2"]);
        for &leaf in &leaves {
            let cid = a.get_dictionary(leaf).unwrap().get(b"Contents").unwrap().as_reference().unwrap();
            let len = a.get_object(cid).unwrap().as_stream().unwrap().content.len() as i64;
            let len_id = a.add_object(Object::Integer(len));
            let s = a.get_object_mut(cid).unwrap().as_stream_mut().unwrap();
            s.dict.set("Length", Object::Reference(len_id));
        }
        let pa = dir.path("a.pdf");
        let mut file = std::fs::File::create(&pa).unwrap();
        a.save_modern(&mut file).unwrap();
        drop(file);
        assert!(std::fs::read(&pa).unwrap().windows(6).any(|w| w == b"ObjStm"));
        let (mut b, _) = simple(A5, 0, &["B1"]);
        let pb = dir.path("b.pdf");
        save(&mut b, &pb);
        let out = dir.path("merged.pdf");

        merge_files(&[pa, pb], &out).unwrap();
        let merged = Document::load(&out).unwrap();
        assert_eq!(markers(&merged), ["A1", "A2", "B1"]);
        let first = *merged.get_pages().get(&1).unwrap();
        assert_eq!(merged.get_page_content(first).unwrap(), content_for("A1"));
    }

    #[test]
    fn rejects_fewer_than_two_files() {
        let dir = TempDir::new("one");
        let (mut a, _) = simple(A4, 0, &["A1"]);
        let pa = dir.path("a.pdf");
        save(&mut a, &pa);
        let err = merge_files(&[pa], &dir.path("out.pdf")).unwrap_err();
        assert!(err.contains("at least two"), "got: {err}");
    }

    #[test]
    fn reports_an_invalid_file_by_name() {
        let dir = TempDir::new("invalid");
        let (mut a, _) = simple(A4, 0, &["A1"]);
        let pa = dir.path("a.pdf");
        save(&mut a, &pa);
        let bad = dir.path("notes.pdf");
        std::fs::write(&bad, b"this is not a PDF").unwrap();
        let out = dir.path("out.pdf");

        let err = merge_files(&[pa, bad.clone()], &out).unwrap_err();
        assert!(err.contains("notes.pdf"), "got: {err}");
        assert!(err.contains("not a valid PDF"), "got: {err}");
        assert!(!std::path::Path::new(&out).exists(), "no output on failure");

        let err = check_input(&bad).unwrap_err();
        assert!(err.contains("notes.pdf") && err.contains("not a valid PDF"), "got: {err}");
    }

    #[test]
    fn reports_a_missing_file_by_name() {
        let dir = TempDir::new("missing");
        let err = check_input(&dir.path("gone.pdf")).unwrap_err();
        assert!(err.contains("gone.pdf"), "got: {err}");
    }

    fn encrypt(doc: &mut Document, user_password: &str) {
        use lopdf::{EncryptionState, EncryptionVersion, Permissions};
        doc.trailer.set(
            "ID",
            Object::Array(vec![
                Object::String(b"0123456789abcdef".to_vec(), StringFormat::Hexadecimal),
                Object::String(b"0123456789abcdef".to_vec(), StringFormat::Hexadecimal),
            ]),
        );
        let version = EncryptionVersion::V2 {
            document: doc,
            owner_password: "owner",
            user_password,
            key_length: 128,
            permissions: Permissions::PRINTABLE,
        };
        let state = EncryptionState::try_from(version).unwrap();
        doc.encrypt(&state).unwrap();
    }

    #[test]
    fn reports_encrypted_files_by_name() {
        let dir = TempDir::new("encrypted");
        let (mut a, _) = simple(A4, 0, &["A1"]);
        let pa = dir.path("a.pdf");
        save(&mut a, &pa);

        // Needs a password to open.
        let (mut locked, _) = simple(A4, 0, &["L1"]);
        encrypt(&mut locked, "secret");
        let pl = dir.path("locked.pdf");
        save(&mut locked, &pl);
        // Opens without a password but carries usage restrictions.
        let (mut restricted, _) = simple(A4, 0, &["R1"]);
        encrypt(&mut restricted, "");
        let pr = dir.path("restricted.pdf");
        save(&mut restricted, &pr);

        for (path, name) in [(&pl, "locked.pdf"), (&pr, "restricted.pdf")] {
            let err = merge_files(&[pa.clone(), path.clone()], &dir.path("out.pdf")).unwrap_err();
            assert!(err.contains(name), "got: {err}");
            assert!(err.contains("encrypted"), "got: {err}");
            assert!(check_input(path).unwrap_err().contains("encrypted"));
        }
    }

    #[test]
    fn check_input_returns_the_page_count() {
        let dir = TempDir::new("check");
        let (mut a, _) = simple(A4, 0, &["A1", "A2", "A3"]);
        let pa = dir.path("a.pdf");
        save(&mut a, &pa);
        assert_eq!(check_input(&pa).unwrap(), 3);
    }

    fn rect_on(page: u32) -> Annotation {
        Annotation::Rect(RectAnnotation {
            page,
            x: 10.0,
            y: 10.0,
            width: 50.0,
            height: 20.0,
            color: RgbColor { r: 255, g: 0, b: 0 },
            stroke_width: 1.0,
        })
    }

    /// Adds a PDF Rider editable annotation on `page`, with its burned stream.
    fn add_editable_annotation(doc: &mut Document, page: u32) -> ObjectId {
        let sid = doc.add_object(Stream::new(dictionary! {}, b"% burned".to_vec()));
        let meta = AnnotationMeta {
            annotations: vec![rect_on(page)],
            stream_ids: HashMap::from([(page, [sid.0, sid.1 as u32])]),
        };
        writer::save_meta(doc, &meta).unwrap();
        sid
    }

    #[test]
    fn carries_editable_annotations_over_with_renumbered_pages() {
        let dir = TempDir::new("annots");
        let (mut a, _) = simple(A4, 0, &["A1", "A2"]);
        add_editable_annotation(&mut a, 2);
        let (mut b, _) = simple(A5, 0, &["B1"]);
        add_editable_annotation(&mut b, 1);
        let (pa, pb) = (dir.path("a.pdf"), dir.path("b.pdf"));
        save(&mut a, &pa);
        save(&mut b, &pb);
        let out = dir.path("merged.pdf");

        merge_files(&[pa, pb], &out).unwrap();
        let merged = Document::load(&out).unwrap();
        let meta = writer::load_meta(&merged);
        let pages: Vec<u32> = meta.annotations.iter().map(|a| a.page()).collect();
        assert_eq!(pages, [2, 3]);
        let mut stream_pages: Vec<u32> = meta.stream_ids.keys().copied().collect();
        stream_pages.sort();
        assert_eq!(stream_pages, [2, 3]);
        for [id, gen] in meta.stream_ids.values() {
            let s = merged.get_object((*id, *gen as u16)).unwrap().as_stream().unwrap();
            assert_eq!(s.content, b"% burned");
        }
    }

    /// Adds a one-item outline pointing at `page`.
    fn add_outline(doc: &mut Document, title: &str, page: ObjectId) {
        let root = doc.new_object_id();
        let item = doc.add_object(dictionary! {
            "Title" => Object::string_literal(title),
            "Parent" => root,
            "Dest" => vec![Object::Reference(page), "Fit".into()],
        });
        doc.objects.insert(
            root,
            Object::Dictionary(dictionary! {
                "Type" => "Outlines", "First" => item, "Last" => item, "Count" => 1,
            }),
        );
        catalog_mut(doc).set("Outlines", root);
    }

    #[test]
    fn concatenates_bookmarks() {
        let dir = TempDir::new("outlines");
        let (mut a, la) = simple(A4, 0, &["A1", "A2"]);
        add_outline(&mut a, "Chapter A", la[1]);
        let (mut b, _) = simple(A5, 0, &["B1"]);
        let (mut c, lc) = simple(A5, 0, &["C1"]);
        add_outline(&mut c, "Chapter C", lc[0]);
        let (pa, pb, pc) = (dir.path("a.pdf"), dir.path("b.pdf"), dir.path("c.pdf"));
        save(&mut a, &pa);
        save(&mut b, &pb);
        save(&mut c, &pc);
        let out = dir.path("merged.pdf");

        merge_files(&[pa, pb, pc], &out).unwrap();
        let merged = Document::load(&out).unwrap();
        let pages = merged.get_pages();
        let root_id = catalog(&merged).get(b"Outlines").unwrap().as_reference().unwrap();
        let root = merged.get_dictionary(root_id).unwrap();
        assert_eq!(root.get(b"Count").unwrap().as_i64().unwrap(), 2);

        let mut titles = vec![];
        let mut cur = root.get(b"First").unwrap().as_reference().ok();
        while let Some(id) = cur {
            let item = merged.get_dictionary(id).unwrap();
            assert_eq!(item.get(b"Parent").unwrap().as_reference().unwrap(), root_id);
            let dest_page = item.get(b"Dest").unwrap().as_array().unwrap()[0].as_reference().unwrap();
            let page_num = pages.iter().find(|(_, &id)| id == dest_page).map(|(n, _)| *n).unwrap();
            titles.push((text(item.get(b"Title").unwrap()), page_num));
            cur = item.get(b"Next").and_then(Object::as_reference).ok();
        }
        assert_eq!(titles, [("Chapter A".to_string(), 2), ("Chapter C".to_string(), 4)]);
    }

    /// Adds a text field named `name` with its widget on `page`.
    fn add_field(doc: &mut Document, name: &str, page: ObjectId) {
        let font = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let field = doc.add_object(dictionary! {
            "FT" => "Tx",
            "T" => Object::string_literal(name),
            "V" => Object::string_literal(format!("value of {name}")),
            "Type" => "Annot",
            "Subtype" => "Widget",
            "Rect" => vec![10.into(), 10.into(), 100.into(), 30.into()],
            "P" => page,
        });
        doc.get_object_mut(page).unwrap().as_dict_mut().unwrap()
            .set("Annots", vec![Object::Reference(field)]);
        let form = doc.add_object(dictionary! {
            "Fields" => vec![Object::Reference(field)],
            "DA" => Object::string_literal("/Helv 0 Tf 0 g"),
            "DR" => dictionary! { "Font" => dictionary! { "Helv" => font } },
        });
        catalog_mut(doc).set("AcroForm", form);
    }

    #[test]
    fn merges_form_fields_and_renames_clashing_names() {
        let dir = TempDir::new("fields");
        let (mut a, la) = simple(A4, 0, &["A1"]);
        add_field(&mut a, "name", la[0]);
        let (mut b, lb) = simple(A5, 0, &["B1"]);
        add_field(&mut b, "name", lb[0]);
        let (pa, pb) = (dir.path("a.pdf"), dir.path("b.pdf"));
        save(&mut a, &pa);
        save(&mut b, &pb);
        let out = dir.path("merged.pdf");

        merge_files(&[pa, pb], &out).unwrap();
        let merged = Document::load(&out).unwrap();
        let form = match catalog(&merged).get(b"AcroForm").unwrap() {
            Object::Reference(id) => merged.get_dictionary(*id).unwrap(),
            o => o.as_dict().unwrap(),
        };
        let names: Vec<(String, String)> = form.get(b"Fields").unwrap().as_array().unwrap().iter()
            .map(|f| {
                let d = merged.get_dictionary(f.as_reference().unwrap()).unwrap();
                (text(d.get(b"T").unwrap()), text(d.get(b"V").unwrap()))
            })
            .collect();
        assert_eq!(names, [
            ("name".to_string(), "value of name".to_string()),
            ("name_2".to_string(), "value of name".to_string()),
        ]);
        let fonts = form.get(b"DR").unwrap().as_dict().unwrap().get(b"Font").unwrap();
        let fonts = match fonts { Object::Reference(id) => merged.get_dictionary(*id).unwrap(), o => o.as_dict().unwrap() };
        assert!(fonts.get(b"Helv").is_ok());
    }

    /// Adds a named destination `name` → `page`, and a link on `page` to it.
    fn add_named_dest_and_link(doc: &mut Document, name: &str, page: ObjectId) {
        let dests = doc.add_object(dictionary! {
            "Names" => vec![
                Object::string_literal(name),
                vec![Object::Reference(page), "Fit".into()].into(),
            ],
        });
        catalog_mut(doc).set("Names", dictionary! { "Dests" => dests });
        let link = doc.add_object(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Link",
            "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()],
            "Dest" => Object::string_literal(name),
        });
        doc.get_object_mut(page).unwrap().as_dict_mut().unwrap()
            .set("Annots", vec![Object::Reference(link)]);
    }

    fn named_dests(doc: &Document) -> Vec<(String, ObjectId)> {
        let names = catalog(doc).get(b"Names").unwrap();
        let names = match names { Object::Reference(id) => doc.get_dictionary(*id).unwrap(), o => o.as_dict().unwrap() };
        let tree = doc.get_dictionary(names.get(b"Dests").unwrap().as_reference().unwrap()).unwrap();
        tree.get(b"Names").unwrap().as_array().unwrap()
            .chunks(2)
            .map(|kv| (text(&kv[0]), kv[1].as_array().unwrap()[0].as_reference().unwrap()))
            .collect()
    }

    #[test]
    fn keeps_named_destinations_and_renames_clashes() {
        let dir = TempDir::new("dests");
        let (mut a, la) = simple(A4, 0, &["A1"]);
        add_named_dest_and_link(&mut a, "page.1", la[0]);
        let (mut b, lb) = simple(A5, 0, &["B1"]);
        add_named_dest_and_link(&mut b, "page.1", lb[0]);
        let (pa, pb) = (dir.path("a.pdf"), dir.path("b.pdf"));
        save(&mut a, &pa);
        save(&mut b, &pb);
        let out = dir.path("merged.pdf");

        merge_files(&[pa, pb], &out).unwrap();
        let merged = Document::load(&out).unwrap();
        let pages = merged.get_pages();
        let dests = named_dests(&merged);
        assert_eq!(dests, [("page.1".to_string(), pages[&1]), ("page.1_2".to_string(), pages[&2])]);
        // The link on page 2 follows the renamed destination.
        let annots = merged.get_dictionary(pages[&2]).unwrap().get(b"Annots").unwrap().as_array().unwrap();
        let link = merged.get_dictionary(annots[0].as_reference().unwrap()).unwrap();
        assert_eq!(text(link.get(b"Dest").unwrap()), "page.1_2");
    }

    #[test]
    fn takes_document_info_from_the_first_file() {
        let dir = TempDir::new("info");
        let (mut a, _) = simple(A4, 0, &["A1"]);
        let info = a.add_object(dictionary! { "Title" => Object::string_literal("First") });
        a.trailer.set("Info", info);
        let (mut b, _) = simple(A5, 0, &["B1"]);
        let info = b.add_object(dictionary! { "Title" => Object::string_literal("Second") });
        b.trailer.set("Info", info);
        let (pa, pb) = (dir.path("a.pdf"), dir.path("b.pdf"));
        save(&mut a, &pa);
        save(&mut b, &pb);
        let out = dir.path("merged.pdf");

        merge_files(&[pa, pb], &out).unwrap();
        let merged = Document::load(&out).unwrap();
        let info = merged.get_dictionary(merged.trailer.get(b"Info").unwrap().as_reference().unwrap()).unwrap();
        assert_eq!(text(info.get(b"Title").unwrap()), "First");
    }
}
