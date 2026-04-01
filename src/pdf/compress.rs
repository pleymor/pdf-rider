use image::{ExtendedColorType, ImageEncoder};
use lopdf::{Document, Object};

/// Quality presets that map to JPEG re-encoding targets.
#[derive(Debug, Clone, Copy)]
pub enum CompressionLevel {
    /// Smallest file, suitable for on-screen reading (JPEG q=25).
    Screen,
    /// Balanced size and quality, suitable for e-readers and sharing (JPEG q=55).
    Ebook,
    /// Good reproduction quality with moderate reduction (JPEG q=80).
    Print,
}

impl CompressionLevel {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "screen" => Some(Self::Screen),
            "ebook"  => Some(Self::Ebook),
            "print"  => Some(Self::Print),
            _        => None,
        }
    }

    fn jpeg_quality(self) -> u8 {
        match self {
            Self::Screen => 25,
            Self::Ebook  => 55,
            Self::Print  => 80,
        }
    }
}

/// Returns `true` if `obj` is a PDF Name equal to `name`, or if it is an Array
/// whose first element is that Name (common for single-filter streams).
fn has_filter(obj: &Object, name: &[u8]) -> bool {
    match obj {
        Object::Name(n) => n.as_slice() == name,
        Object::Array(arr) => arr
            .first()
            .map(|first| matches!(first, Object::Name(n) if n.as_slice() == name))
            .unwrap_or(false),
        _ => false,
    }
}

/// Attempt to re-encode a JPEG byte slice at `quality`.
/// Returns `Some(new_bytes)` only when the result is strictly smaller.
fn recompress_jpeg(content: &[u8], quality: u8) -> Option<Vec<u8>> {
    // Decode the JPEG using the image crate's auto-format detection.
    let img = image::ImageReader::new(std::io::Cursor::new(content))
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;

    // Normalise to RGB8 (JPEG does not support alpha; grayscale inflates
    // slightly but stays correct and simplifies the code path).
    let rgb = img.to_rgb8();
    let (w, h) = rgb.dimensions();

    let mut output = std::io::Cursor::new(Vec::<u8>::new());
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, quality);
    encoder
        .write_image(rgb.as_raw(), w, h, ExtendedColorType::Rgb8)
        .ok()?;

    let new_bytes = output.into_inner();

    // Only accept the result if it actually reduces file size.
    if new_bytes.len() < content.len() {
        Some(new_bytes)
    } else {
        None
    }
}

/// Iterates all objects in `doc`, finds DCTDecode image XObjects without
/// transparency (no `/SMask`), and re-encodes them at the target quality.
///
/// Returns `(original_image_bytes, compressed_image_bytes)` so the caller
/// can report savings to the user.
pub fn compress_images(doc: &mut Document, level: CompressionLevel) -> (u64, u64) {
    let quality = level.jpeg_quality();

    // Collect IDs first to avoid a simultaneous borrow of `doc.objects`.
    let ids: Vec<_> = doc.objects.keys().copied().collect();

    let mut original_total: u64 = 0;
    let mut compressed_total: u64 = 0;

    for id in ids {
        let Some(Object::Stream(stream)) = doc.objects.get_mut(&id) else {
            continue;
        };

        // Must be an Image XObject.
        let is_image = stream
            .dict
            .get(b"Subtype")
            .ok()
            .map(|o| matches!(o, Object::Name(n) if n.as_slice() == b"Image"))
            .unwrap_or(false);
        if !is_image {
            continue;
        }

        // Must use DCTDecode (JPEG) as its filter.
        let is_dct = stream
            .dict
            .get(b"Filter")
            .ok()
            .map(|o| has_filter(o, b"DCTDecode"))
            .unwrap_or(false);
        if !is_dct {
            continue;
        }

        // Skip images with an alpha channel — converting them would destroy the
        // transparency mask stored in the companion SMask XObject.
        let has_smask = stream.dict.get(b"SMask").is_ok();
        if has_smask {
            continue;
        }

        let original_size = stream.content.len() as u64;
        original_total += original_size;

        if let Some(new_bytes) = recompress_jpeg(&stream.content, quality) {
            compressed_total += new_bytes.len() as u64;
            stream.content = new_bytes;
        } else {
            // Not compressible at this level — count as unchanged.
            compressed_total += original_size;
        }
    }

    (original_total, compressed_total)
}

/// Apply FlateDecode to all streams not already compressed.
/// Uses lopdf's built-in Document::compress().
pub fn compress_streams(doc: &mut Document) {
    doc.compress();
}

/// Remove metadata: Info dictionary, XMP Metadata stream, page thumbnails.
pub fn strip_metadata(doc: &mut Document) {
    // 1. Remove /Info object and trailer entry
    let info_id = doc.trailer.get(b"Info").and_then(|o| o.as_reference()).ok();
    if let Some(id) = info_id {
        doc.objects.remove(&id);
    }
    doc.trailer.remove(b"Info");

    // 2. Remove /Metadata stream from catalog (two passes to avoid borrow conflicts)
    let catalog_id = doc.trailer.get(b"Root").and_then(|o| o.as_reference()).ok();
    if let Some(cid) = catalog_id {
        let meta_id = doc
            .objects
            .get(&cid)
            .and_then(|o| o.as_dict().ok())
            .and_then(|d| d.get(b"Metadata").ok())
            .and_then(|m| m.as_reference().ok());
        if let Some(mid) = meta_id {
            doc.objects.remove(&mid);
            if let Some(obj) = doc.objects.get_mut(&cid) {
                if let Ok(dict) = obj.as_dict_mut() {
                    dict.remove(b"Metadata");
                }
            }
        }
    }

    // 3. Remove /Thumb from every page dictionary
    let page_ids: Vec<_> = doc.get_pages().into_values().collect();
    for page_id in page_ids {
        if let Some(obj) = doc.objects.get_mut(&page_id) {
            if let Ok(dict) = obj.as_dict_mut() {
                dict.remove(b"Thumb");
            }
        }
    }
}

/// Remove unreferenced objects and empty streams.
pub fn prune_dead_objects(doc: &mut Document) {
    doc.prune_objects();
    doc.delete_zero_length_streams();
}

/// Remove the AcroForm and all widget annotations from the document.
/// Static page content (labels, layout) is preserved; interactive form
/// fields and any values stored only in their appearance streams are lost.
pub fn flatten_forms(doc: &mut Document) {
    // Remove /AcroForm from catalog
    let catalog_id = doc.trailer.get(b"Root").and_then(|o| o.as_reference()).ok();
    if let Some(cid) = catalog_id {
        let acroform_id = doc
            .objects
            .get(&cid)
            .and_then(|o| o.as_dict().ok())
            .and_then(|d| d.get(b"AcroForm").ok())
            .and_then(|o| o.as_reference().ok());
        if let Some(obj) = doc.objects.get_mut(&cid) {
            if let Ok(dict) = obj.as_dict_mut() {
                dict.remove(b"AcroForm");
            }
        }
        if let Some(afid) = acroform_id {
            doc.objects.remove(&afid);
        }
    }

    // Remove widget annotations from every page
    let page_ids: Vec<_> = doc.get_pages().into_values().collect();
    for page_id in page_ids {
        let annot_ids: Vec<_> = doc
            .objects
            .get(&page_id)
            .and_then(|o| o.as_dict().ok())
            .and_then(|d| d.get(b"Annots").ok())
            .and_then(|o| o.as_array().ok())
            .map(|arr| arr.iter().filter_map(|o| o.as_reference().ok()).collect())
            .unwrap_or_default();

        if annot_ids.is_empty() {
            continue;
        }

        let mut keep: Vec<_> = Vec::new();
        for &id in &annot_ids {
            let is_widget = doc
                .objects
                .get(&id)
                .and_then(|o| o.as_dict().ok())
                .and_then(|d| d.get(b"Subtype").ok())
                .map(|o| matches!(o, Object::Name(n) if n.as_slice() == b"Widget"))
                .unwrap_or(false);
            if is_widget {
                doc.objects.remove(&id);
            } else {
                keep.push(id);
            }
        }

        if let Some(obj) = doc.objects.get_mut(&page_id) {
            if let Ok(dict) = obj.as_dict_mut() {
                if keep.is_empty() {
                    dict.remove(b"Annots");
                } else {
                    dict.set(
                        b"Annots",
                        Object::Array(keep.iter().map(|&id| Object::Reference(id)).collect()),
                    );
                }
            }
        }
    }
}
