#![no_std]
#![allow(clippy::new_without_default)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

mod font;
mod page;
mod parser;
mod stream;
mod text;
mod token;

pub use page::PageContent;
pub use parser::{parse_pdf, PdfObj};

#[derive(Debug, Clone)]
pub enum PdfError {
    ParseError(String),
    DecompressionError(String),
}

impl fmt::Display for PdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PdfError::ParseError(msg) => write!(f, "Parse error: {msg}"),
            PdfError::DecompressionError(msg) => write!(f, "Decompression error: {msg}"),
        }
    }
}

pub fn extract_text(pdf_bytes: Vec<u8>) -> Result<Vec<String>, PdfError> {
    let (pages, objects) = parse_pdf(&pdf_bytes)?;
    extract_text_from_document(&pages, &objects).map_err(PdfError::ParseError)
}

pub fn extract_text_from_document(
    pages: &[PageContent],
    objects: &BTreeMap<(u32, u16), PdfObj>,
) -> Result<Vec<String>, String> {
    let mut results = Vec::new();

    for page in pages {
        let text = extract_text_from_page(page, objects);
        results.push(text);
    }

    Ok(results)
}

pub fn extract_text_from_page(
    page: &PageContent,
    objects: &BTreeMap<(u32, u16), PdfObj>,
) -> String {
    text::extract_text_from_page_content(page, objects)
}
