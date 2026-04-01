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

    pub fn get_mut_for_page(&mut self, page: u32) -> Option<&mut Vec<Annotation>> {
        self.pages.get_mut(&page)
    }

    pub fn remove(&mut self, page: u32, idx: usize) -> Option<Annotation> {
        if let Some(anns) = self.pages.get_mut(&page) {
            if idx < anns.len() {
                return Some(anns.remove(idx));
            }
        }
        None
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
