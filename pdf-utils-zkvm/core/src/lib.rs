#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

pub use extractor_zkvm::{extract_text, PdfError};
pub use signature_validator_zkvm::{verify_pdf_signature, SignatureAlgorithm};
pub use pdf_logger::{Logger, NullLogger, set_logger, log_debug};

pub struct PdfValidationResult {
    pub signature_valid: bool,
    pub text_pages: Vec<String>,
}

pub fn validate_and_extract_pdf(pdf_bytes: &[u8]) -> Result<PdfValidationResult, String> {
    // Verify signature
    let signature_valid = verify_pdf_signature(pdf_bytes)?;
    
    // Extract text
    let text_pages = extract_text(pdf_bytes.to_vec())
        .map_err(|e| alloc::format!("Text extraction failed: {}", e))?;
    
    Ok(PdfValidationResult {
        signature_valid,
        text_pages,
    })
}