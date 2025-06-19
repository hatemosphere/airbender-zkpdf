#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;

pub(crate) fn get_signature_der(pdf_bytes: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    #[cfg(feature = "debug")]
    pdf_logger::debug_log!("Looking for signature in PDF of {} bytes", pdf_bytes.len());
    
    let byte_range = extract_byte_range(pdf_bytes)?;
    
    #[cfg(feature = "debug")]
    pdf_logger::debug_log!("Found ByteRange: [{} {} {} {}]", 
        byte_range.offset1, byte_range.length1, 
        byte_range.offset2, byte_range.length2);
    
    let signed_data = extract_signed_data(pdf_bytes, &byte_range)?;
    let signature_hex = extract_signature_hex(pdf_bytes, &byte_range)?;
    
    #[cfg(feature = "debug")]
    pdf_logger::debug_log!("Signature hex length: {}", signature_hex.len());
    
    let signature_der = hex_to_bytes_internal(&signature_hex)?;

    Ok((signature_der, signed_data))
}

#[derive(Debug)]
struct ByteRange {
    offset1: usize,
    length1: usize,
    offset2: usize,
    length2: usize,
}


fn extract_byte_range(pdf_bytes: &[u8]) -> Result<ByteRange, String> {
    let byte_range_pattern = b"/ByteRange";
    
    #[cfg(feature = "debug")]
    {
        pdf_logger::debug_log!("Searching for /ByteRange in PDF...");
        // Print first 200 bytes for debugging
        if pdf_bytes.len() > 200 {
            if let Ok(preview) = core::str::from_utf8(&pdf_bytes[0..200]) {
                pdf_logger::debug_log!("PDF preview: {}", preview);
            }
        }
    }
    
    let byte_range_pos = find_pattern_internal(pdf_bytes, byte_range_pattern)
        .ok_or_else(|| String::from("ByteRange not found"))?;

    let start = byte_range_pos + byte_range_pattern.len();
    let bracket_start = find_byte_internal(pdf_bytes, b'[', start)
        .ok_or_else(|| String::from("ByteRange opening bracket not found"))?;
    let bracket_end = find_byte_internal(pdf_bytes, b']', bracket_start)
        .ok_or_else(|| String::from("ByteRange closing bracket not found"))?;

    let byte_range_str = core::str::from_utf8(&pdf_bytes[bracket_start + 1..bracket_end])
        .map_err(|_| String::from("Invalid UTF-8 in ByteRange"))?;

    let parts: Vec<&str> = byte_range_str.split_whitespace().collect();
    if parts.len() != 4 {
        return Err(String::from("ByteRange should have exactly 4 values"));
    }

    Ok(ByteRange {
        offset1: parse_usize(parts[0])?,
        length1: parse_usize(parts[1])?,
        offset2: parse_usize(parts[2])?,
        length2: parse_usize(parts[3])?,
    })
}

fn extract_signed_data(pdf_bytes: &[u8], byte_range: &ByteRange) -> Result<Vec<u8>, String> {
    let mut signed_data = Vec::new();

    let end1 = byte_range.offset1 + byte_range.length1;
    if end1 > pdf_bytes.len() {
        return Err(String::from("First ByteRange segment out of bounds"));
    }
    signed_data.extend_from_slice(&pdf_bytes[byte_range.offset1..end1]);

    let end2 = byte_range.offset2 + byte_range.length2;
    if end2 > pdf_bytes.len() {
        return Err(String::from("Second ByteRange segment out of bounds"));
    }
    signed_data.extend_from_slice(&pdf_bytes[byte_range.offset2..end2]);

    Ok(signed_data)
}

fn extract_signature_hex(pdf_bytes: &[u8], byte_range: &ByteRange) -> Result<String, String> {
    let sig_start = byte_range.offset1 + byte_range.length1;
    let sig_end = byte_range.offset2;

    if sig_start >= sig_end || sig_end > pdf_bytes.len() {
        return Err(String::from("Invalid signature position"));
    }

    // Instead of searching in the signature range, search after the ByteRange
    // In many PDFs, /Contents appears before /ByteRange
    let contents_pattern = b"/Contents";
    
    // First try to find /Contents after the ByteRange
    let byte_range_pattern = b"/ByteRange";
    let byte_range_pos = find_pattern_internal(pdf_bytes, byte_range_pattern)
        .ok_or_else(|| String::from("ByteRange not found"))?;
    
    // Search for /Contents starting from before the ByteRange position
    let search_start = if byte_range_pos > 500 { byte_range_pos - 500 } else { 0 };
    let contents_pos = find_pattern_internal(&pdf_bytes[search_start..], contents_pattern)
        .map(|pos| search_start + pos)
        .ok_or_else(|| String::from("/Contents not found near ByteRange"))?;

    let hex_start = find_byte_internal(pdf_bytes, b'<', contents_pos + contents_pattern.len())
        .ok_or_else(|| String::from("Signature hex start not found"))?
        + 1;
    let hex_end = find_byte_internal(pdf_bytes, b'>', hex_start)
        .ok_or_else(|| String::from("Signature hex end not found"))?;

    let hex_str = core::str::from_utf8(&pdf_bytes[hex_start..hex_end])
        .map_err(|_| String::from("Invalid UTF-8 in signature hex"))?;

    Ok(String::from(hex_str))
}

#[cfg(test)]
pub fn hex_to_bytes(hex_str: &str) -> Result<Vec<u8>, String> {
    hex_to_bytes_internal(hex_str)
}

fn hex_to_bytes_internal(hex_str: &str) -> Result<Vec<u8>, String> {
    let hex_str = hex_str.trim();
    let hex_str = if hex_str.len() % 2 == 1 {
        let mut padded = String::with_capacity(hex_str.len() + 1);
        padded.push('0');
        padded.push_str(hex_str);
        padded
    } else {
        String::from(hex_str)
    };

    hex::decode(&hex_str).map_err(|e| alloc::format!("Failed to decode hex: {:?}", e))
}

#[cfg(test)]
pub fn find_pattern(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    find_pattern_internal(haystack, needle)
}

fn find_pattern_internal(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
pub fn find_byte(haystack: &[u8], needle: u8, start: usize) -> Option<usize> {
    find_byte_internal(haystack, needle, start)
}

fn find_byte_internal(haystack: &[u8], needle: u8, start: usize) -> Option<usize> {
    haystack[start..].iter().position(|&b| b == needle).map(|pos| start + pos)
}

fn parse_usize(s: &str) -> Result<usize, String> {
    s.parse::<usize>()
        .map_err(|_| alloc::format!("Failed to parse '{}' as usize", s))
}