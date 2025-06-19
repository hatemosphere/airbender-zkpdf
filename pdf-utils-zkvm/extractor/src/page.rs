use crate::font::PdfFont;
use crate::parser::PdfObj;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Debug, Clone)]
pub struct PageContent {
    pub content_streams: Vec<Vec<u8>>,
    pub fonts: BTreeMap<String, PdfFont>,
    pub resources: BTreeMap<String, PdfObj>,
}

impl PageContent {
    pub fn new() -> Self {
        Self {
            content_streams: Vec::new(),
            fonts: BTreeMap::new(),
            resources: BTreeMap::new(),
        }
    }
}
