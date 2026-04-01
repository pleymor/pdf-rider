use crate::pdf::models::Annotation;
use std::collections::HashMap;

/// In-memory store of annotations grouped by page number (1-indexed).
#[derive(Default)]
pub struct AnnotationStore {
    pages: HashMap<u32, Vec<Annotation>>,
}

impl AnnotationStore {
    pub fn clear(&mut self) {
        self.pages.clear();
    }

    pub fn load(&mut self, annotations: Vec<Annotation>) {
        self.pages.clear();
        for ann in annotations {
            let page = ann.page();
            self.pages.entry(page).or_default().push(ann);
        }
    }

    pub fn add(&mut self, ann: Annotation) {
        let page = ann.page();
        self.pages.entry(page).or_default().push(ann);
    }

    pub fn get_for_page(&self, page: u32) -> &[Annotation] {
        self.pages.get(&page).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn all(&self) -> Vec<Annotation> {
        let mut result = Vec::new();
        for anns in self.pages.values() {
            result.extend(anns.iter().cloned());
        }
        result
    }

    pub fn is_empty(&self) -> bool {
        self.pages.values().all(|v| v.is_empty())
    }
}
