use pdfium_render::prelude::*;

/// A link with its bounding box in canvas pixel coordinates and URL.
#[derive(Clone, Debug)]
pub struct LinkBox {
    pub url: String,
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

/// Find a link at the given canvas pixel coordinates (with tolerance).
pub fn link_at(links: &[LinkBox], x: f32, y: f32) -> Option<&LinkBox> {
    let tol = 8.0;
    links.iter().find(|l| x >= l.left - tol && x <= l.right + tol && y >= l.top - tol && y <= l.bottom + tol)
}

/// Extract links from PDF page annotations (real PDF link annotations).
pub fn extract_page_links(
    pdfium: &Pdfium,
    path: &std::path::Path,
    page_index: u16,
    scale: f32,
    page_height_pt: f32,
) -> Vec<LinkBox> {
    let Ok(doc) = pdfium.load_pdf_from_file(path, None) else { return Vec::new() };
    let Ok(page) = doc.pages().get(page_index) else { return Vec::new() };

    let mut links = Vec::new();
    for ann in page.annotations().iter() {
        if ann.annotation_type() != PdfPageAnnotationType::Link { continue; }
        if let Some(_link_ann) = ann.as_link_annotation() {
            // pdfium-render 0.8 doesn't expose uri() — skip for now
        }
    }
    links
}

/// Detect URLs in extracted text by scanning for http/https patterns.
/// Handles URLs that span across multiple lines by working on all chars sorted
/// in reading order (top-to-bottom, left-to-right).
pub fn detect_text_urls(
    chars: &[super::text_selection::CharBox],
    _scale: f32,
) -> Vec<LinkBox> {
    if chars.is_empty() { return Vec::new(); }

    // Build a flat sorted list of all chars in reading order
    let mut indexed: Vec<(usize, &super::text_selection::CharBox)> =
        chars.iter().enumerate().collect();

    indexed.sort_by(|a, b| {
        let ay = (a.1.top + a.1.bottom) / 2.0;
        let by = (b.1.top + b.1.bottom) / 2.0;
        let line_h = (a.1.bottom - a.1.top).max(b.1.bottom - b.1.top);
        if (ay - by).abs() > line_h * 0.4 {
            ay.partial_cmp(&by).unwrap()
        } else {
            a.1.left.partial_cmp(&b.1.left).unwrap()
        }
    });

    // Build char vector, inserting spaces at line breaks so URLs don't bleed across lines
    let mut all_chars: Vec<char> = Vec::with_capacity(indexed.len() * 2);
    let mut all_indexed: Vec<(usize, &super::text_selection::CharBox)> = Vec::with_capacity(indexed.len() * 2);
    for (i, &(orig_i, cb)) in indexed.iter().enumerate() {
        // Detect line break: if previous char is on a different line, insert space
        if i > 0 {
            let prev_cy = (indexed[i - 1].1.top + indexed[i - 1].1.bottom) / 2.0;
            let cur_cy = (cb.top + cb.bottom) / 2.0;
            let line_h = cb.bottom - cb.top;
            if line_h > 0.0 && (cur_cy - prev_cy).abs() > line_h * 0.4 {
                // Line break — check if prev char ends a word (not a hyphen continuation)
                let prev_char = all_chars.last().copied().unwrap_or(' ');
                if prev_char != ' ' && prev_char != '-' {
                    all_chars.push(' ');
                    // Dummy entry for the space (reuse prev char's box)
                    all_indexed.push(indexed[i - 1]);
                }
            }
        }
        all_chars.push(cb.ch);
        all_indexed.push((orig_i, cb));
    }
    let indexed = all_indexed;

    let mut links = Vec::new();
    let mut search_start = 0;

    loop {
        if search_start >= all_chars.len() { break; }
        let remaining: String = all_chars[search_start..].iter().collect();
        let http_pos = remaining.find("http://").or_else(|| remaining.find("https://"));
        let Some(byte_pos) = http_pos else { break };

        let char_pos = remaining[..byte_pos].chars().count();
        let abs_start = search_start + char_pos;

        // Find end of URL
        let mut end_pos = abs_start;
        for i in abs_start..all_chars.len() {
            let c = all_chars[i];
            if c == ' ' || c == '>' || c == ')' || c == ']' || c == '"' || c == '\t' {
                break;
            }
            end_pos = i + 1;
        }

        if end_pos > abs_start + 7 && end_pos <= indexed.len() {
            let url: String = all_chars[abs_start..end_pos].iter().collect();

            // Build one LinkBox per line of the URL
            let mut line_start = abs_start;
            let mut i = abs_start + 1;
            while i <= end_pos {
                let at_end = i == end_pos;
                let new_line = if !at_end {
                    let prev_cy = (indexed[i - 1].1.top + indexed[i - 1].1.bottom) / 2.0;
                    let cur_cy = (indexed[i].1.top + indexed[i].1.bottom) / 2.0;
                    let line_h = indexed[i].1.bottom - indexed[i].1.top;
                    (cur_cy - prev_cy).abs() > line_h * 0.4
                } else {
                    false
                };

                if at_end || new_line {
                    let seg_end = if at_end { end_pos - 1 } else { i - 1 };
                    if seg_end >= line_start && line_start < indexed.len() && seg_end < indexed.len() {
                        let first = indexed[line_start].1;
                        let last = indexed[seg_end].1;
                        links.push(LinkBox {
                            url: url.clone(),
                            left: first.left,
                            top: first.top.min(last.top),
                            right: last.right,
                            bottom: first.bottom.max(last.bottom),
                        });
                    }
                    line_start = i;
                }
                i += 1;
            }
        }
        search_start = end_pos;
    }

    links
}
