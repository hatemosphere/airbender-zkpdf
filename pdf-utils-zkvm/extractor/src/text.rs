use crate::font::PdfFont;
use crate::page::PageContent;
use crate::parser::{resolve_reference, PdfObj};
use crate::stream::handle_stream_filters;
use crate::token::{Token, TokenParser};
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

pub fn extract_text_from_page_content(
    page: &PageContent,
    objects: &BTreeMap<(u32, u16), PdfObj>,
) -> String {
    // Concatenate all content streams first, like the reference implementation
    let mut all_content = Vec::new();
    for stream_data in page.content_streams.iter() {
        if !all_content.is_empty() {
            all_content.push(b' '); // Add space between streams
        }
        all_content.extend_from_slice(stream_data);
    }

    if all_content.is_empty() {
        return String::new();
    }

    extract_text_from_stream(&all_content, &page.fonts, &page.resources, objects)
}

fn extract_text_from_stream(
    stream_data: &[u8],
    fonts: &BTreeMap<String, PdfFont>,
    resources: &BTreeMap<String, PdfObj>,
    objects: &BTreeMap<(u32, u16), PdfObj>,
) -> String {
    let mut parser = TokenParser::new(stream_data);
    let tokens = parser.parse_all();

    // Debug: count text operators
    #[cfg(target_arch = "riscv32")]
    {
        let tj_count = tokens
            .iter()
            .filter(|t| matches!(t, Token::Operator(op) if op == "Tj"))
            .count();
        let tj_array_count = tokens
            .iter()
            .filter(|t| matches!(t, Token::Operator(op) if op == "TJ"))
            .count();
        if tj_count > 0 || tj_array_count > 0 {
            // We found text operators but returning empty - debug needed
        }
    }

    let mut text = String::new();
    let mut current_font: Option<&PdfFont> = None;
    let mut i = 0;
    let mut in_text = false;
    let mut text_line = String::new();

    while i < tokens.len() {
        if let Token::Operator(op) = &tokens[i] {
            match op.as_str() {
                "BT" => {
                    in_text = true;
                    text_line.clear();
                }
                "ET" => {
                    if !text_line.is_empty() {
                        if !text.is_empty() {
                            text.push(' ');
                        }
                        text.push_str(&text_line);
                        text_line.clear();
                    }
                    in_text = false;
                }
                "Tf" => {
                    // Set font
                    if i >= 2 {
                        if let Token::Name(font_name) = &tokens[i - 2] {
                            current_font = fonts.get(font_name);
                        }
                    }
                }
                "Tj" => {
                    // Show text
                    if i >= 1 && in_text {
                        if let Token::String(bytes) = &tokens[i - 1] {
                            let decoded = decode_text(bytes, current_font);
                            text_line.push_str(&decoded);
                        }
                    }
                }
                "TJ" => {
                    // Show text with individual glyph positioning
                    if i >= 1 && in_text {
                        if let Token::ArrayEnd = &tokens[i - 1] {
                            // Find array end
                            let mut j = i - 2;
                            let mut array_items = Vec::new();
                            let mut depth = 1;

                            while j > 0 && depth > 0 {
                                match &tokens[j] {
                                    Token::ArrayEnd => depth += 1,
                                    Token::ArrayStart => depth -= 1,
                                    _ if depth == 1 => array_items.push(&tokens[j]),
                                    _ => {}
                                }
                                j -= 1;
                            }

                            array_items.reverse();
                            for item in array_items {
                                match item {
                                    Token::String(bytes) => {
                                        let decoded = decode_text(bytes, current_font);
                                        text_line.push_str(&decoded);
                                    }
                                    Token::Number(n) if *n < -200.0 => {
                                        // Large negative numbers indicate word spacing
                                        text_line.push(' ');
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                "'" => {
                    // Move to next line and show text
                    if i >= 1 && in_text {
                        if !text_line.is_empty() {
                            if !text.is_empty() {
                                text.push(' ');
                            }
                            text.push_str(&text_line);
                            text_line.clear();
                        }
                        if let Token::String(bytes) = &tokens[i - 1] {
                            let decoded = decode_text(bytes, current_font);
                            text_line.push_str(&decoded);
                        }
                    }
                }
                "\"" => {
                    // Set word and char spacing, move to next line, show text
                    if i >= 3 && in_text {
                        if !text_line.is_empty() {
                            if !text.is_empty() {
                                text.push(' ');
                            }
                            text.push_str(&text_line);
                            text_line.clear();
                        }
                        if let Token::String(bytes) = &tokens[i - 1] {
                            let decoded = decode_text(bytes, current_font);
                            text_line.push_str(&decoded);
                        }
                    }
                }
                "Do" => {
                    // Draw XObject
                    if i >= 1 {
                        if let Token::Name(xobj_name) = &tokens[i - 1] {
                            if let Some(xobj_text) =
                                process_xobject(xobj_name, resources, objects, fonts)
                            {
                                if !text.is_empty() {
                                    text.push(' ');
                                }
                                text.push_str(&xobj_text);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }

    // Don't forget remaining text
    if !text_line.is_empty() {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&text_line);
    }

    text
}

fn decode_text(bytes: &[u8], font: Option<&PdfFont>) -> String {
    if let Some(font) = font {
        decode_with_font(bytes, font)
    } else {
        // Default decoding
        bytes
            .iter()
            .filter_map(|&b| {
                if (32..127).contains(&b) {
                    Some(b as char)
                } else {
                    None
                }
            })
            .collect()
    }
}

fn decode_with_font(bytes: &[u8], font: &PdfFont) -> String {
    let mut result = String::new();

    // Check if it's a CID font (Type0)
    let is_cid =
        font.subtype == "Type0" || font.encoding == "Identity-H" || font.encoding == "Identity-V";

    if is_cid {
        // CID fonts - 2 bytes per character
        let mut i = 0;
        while i < bytes.len() {
            let cid = if i + 1 < bytes.len() {
                ((bytes[i] as u32) << 8) | (bytes[i + 1] as u32)
            } else {
                bytes[i] as u32
            };
            i += 2;

            // Check ToUnicode mapping first
            if let Some(unicode_map) = &font.to_unicode {
                if let Some(unicode_str) = unicode_map.get(&cid) {
                    result.push_str(unicode_str);
                    continue;
                }
            }

            // For CID fonts without ToUnicode, we should use replacement character
            // as direct CID to Unicode conversion is rarely correct
            result.push('�');
        }
    } else {
        // Single byte encodings
        for &byte in bytes {
            let code = byte as u32;

            // Check differences first
            if let Some(differences) = &font.differences {
                if let Some(glyph_name) = differences.get(&code) {
                    result.push_str(glyph_name);
                    continue;
                }
            }

            // Check ToUnicode
            if let Some(unicode_map) = &font.to_unicode {
                if let Some(unicode_str) = unicode_map.get(&code) {
                    result.push_str(unicode_str);
                    continue;
                }
            }

            // Apply encoding
            let ch = match font.encoding.as_str() {
                "WinAnsiEncoding" => decode_winansi(byte),
                "MacRomanEncoding" => decode_macroman(byte),
                _ => {
                    if (32..127).contains(&byte) {
                        byte as char
                    } else {
                        '?'
                    }
                }
            };

            result.push(ch);
        }
    }

    result
}

fn process_xobject(
    xobj_name: &str,
    resources: &BTreeMap<String, PdfObj>,
    objects: &BTreeMap<(u32, u16), PdfObj>,
    parent_fonts: &BTreeMap<String, PdfFont>,
) -> Option<String> {
    let xobjects = match resources.get("XObject") {
        Some(PdfObj::Dictionary(dict)) => dict,
        Some(PdfObj::Reference(xobj_ref)) => match resolve_reference(objects, xobj_ref) {
            Some(PdfObj::Dictionary(dict)) => dict,
            _ => return None,
        },
        _ => return None,
    };

    let xobj_ref = match xobjects.get(xobj_name) {
        Some(PdfObj::Reference(r)) => r,
        _ => return None,
    };

    let xobj = resolve_reference(objects, xobj_ref)?;

    match xobj {
        PdfObj::Stream(stream) => {
            // Check if it's a form XObject
            let subtype = match stream.dict.get("Subtype") {
                Some(PdfObj::Name(name)) => name,
                _ => return None,
            };

            if subtype != "Form" {
                return None;
            }

            // Get stream data
            let data = handle_stream_filters(&stream.dict, &stream.data).ok()?;

            // Get resources
            let mut xobj_resources = BTreeMap::new();
            match stream.dict.get("Resources") {
                Some(PdfObj::Dictionary(res)) => {
                    xobj_resources = res.clone();
                }
                Some(PdfObj::Reference(res_ref)) => {
                    if let Some(PdfObj::Dictionary(res)) = resolve_reference(objects, res_ref) {
                        xobj_resources = res.clone();
                    }
                }
                _ => {}
            }

            // Extract fonts from XObject resources
            let mut fonts = parent_fonts.clone();
            if let Some(PdfObj::Dictionary(font_dict)) = xobj_resources.get("Font") {
                let xobj_fonts = crate::font::extract_fonts(font_dict, objects);
                fonts.extend(xobj_fonts);
            }

            // Extract text from form
            Some(extract_text_from_stream(
                &data,
                &fonts,
                &xobj_resources,
                objects,
            ))
        }
        _ => None,
    }
}

fn decode_winansi(byte: u8) -> char {
    match byte {
        0x80 => '€',
        0x82 => '‚',
        0x83 => 'ƒ',
        0x84 => '„',
        0x85 => '…',
        0x86 => '†',
        0x87 => '‡',
        0x88 => 'ˆ',
        0x89 => '‰',
        0x8A => 'Š',
        0x8B => '‹',
        0x8C => 'Œ',
        0x8E => 'Ž',
        0x91 => '\'',
        0x92 => '\'',
        0x93 => '"',
        0x94 => '"',
        0x95 => '•',
        0x96 => '–',
        0x97 => '—',
        0x98 => '˜',
        0x99 => '™',
        0x9A => 'š',
        0x9B => '›',
        0x9C => 'œ',
        0x9E => 'ž',
        0x9F => 'Ÿ',
        b if b < 0x20 => '?',
        b => b as char,
    }
}

fn decode_macroman(byte: u8) -> char {
    match byte {
        0x80 => 'Ä',
        0x81 => 'Å',
        0x82 => 'Ç',
        0x83 => 'É',
        0x84 => 'Ñ',
        0x85 => 'Ö',
        0x86 => 'Ü',
        0x87 => 'á',
        0x88 => 'à',
        0x89 => 'â',
        0x8A => 'ä',
        0x8B => 'ã',
        0x8C => 'å',
        0x8D => 'ç',
        0x8E => 'é',
        0x8F => 'è',
        0x90 => 'ê',
        0x91 => 'ë',
        0x92 => 'í',
        0x93 => 'ì',
        0x94 => 'î',
        0x95 => 'ï',
        0x96 => 'ñ',
        0x97 => 'ó',
        0x98 => 'ò',
        0x99 => 'ô',
        0x9A => 'ö',
        0x9B => 'õ',
        0x9C => 'ú',
        0x9D => 'ù',
        0x9E => 'û',
        0x9F => 'ü',
        b if b < 0x20 => '?',
        b => b as char,
    }
}
