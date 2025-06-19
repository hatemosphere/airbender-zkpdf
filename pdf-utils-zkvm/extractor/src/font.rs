use crate::parser::{resolve_reference, PdfObj};
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Debug, Clone)]
pub struct PdfFont {
    pub base_font: String,
    pub subtype: String,
    pub encoding: String,
    pub to_unicode: Option<BTreeMap<u32, String>>,
    pub differences: Option<BTreeMap<u32, String>>,
}

pub fn extract_fonts(
    font_dict: &BTreeMap<String, PdfObj>,
    objects: &BTreeMap<(u32, u16), PdfObj>,
) -> BTreeMap<String, PdfFont> {
    let mut fonts = BTreeMap::new();

    for (name, font_obj) in font_dict {
        let font = match font_obj {
            PdfObj::Reference(font_ref) => match resolve_reference(objects, font_ref) {
                Some(PdfObj::Dictionary(dict)) => parse_font(dict, objects),
                _ => None,
            },
            PdfObj::Dictionary(dict) => parse_font(dict, objects),
            _ => None,
        };

        if let Some(font) = font {
            fonts.insert(name.clone(), font);
        }
    }

    fonts
}

fn parse_font(
    font_dict: &BTreeMap<String, PdfObj>,
    objects: &BTreeMap<(u32, u16), PdfObj>,
) -> Option<PdfFont> {
    let base_font = match font_dict.get("BaseFont") {
        Some(PdfObj::Name(name)) => name.clone(),
        _ => String::from("Unknown"),
    };

    let subtype = match font_dict.get("Subtype") {
        Some(PdfObj::Name(name)) => name.clone(),
        _ => String::from("Type1"),
    };

    let encoding = extract_encoding(font_dict, objects);
    let to_unicode = extract_to_unicode(font_dict, objects);
    let differences = extract_differences(font_dict, objects);

    Some(PdfFont {
        base_font,
        subtype,
        encoding,
        to_unicode,
        differences,
    })
}

fn extract_encoding(
    font_dict: &BTreeMap<String, PdfObj>,
    objects: &BTreeMap<(u32, u16), PdfObj>,
) -> String {
    match font_dict.get("Encoding") {
        Some(PdfObj::Name(name)) => name.clone(),
        Some(PdfObj::Reference(enc_ref)) => match resolve_reference(objects, enc_ref) {
            Some(PdfObj::Name(name)) => name.clone(),
            Some(PdfObj::Dictionary(dict)) => match dict.get("BaseEncoding") {
                Some(PdfObj::Name(name)) => name.clone(),
                _ => String::from("Identity-H"),
            },
            _ => String::from("Identity-H"),
        },
        Some(PdfObj::Dictionary(dict)) => match dict.get("BaseEncoding") {
            Some(PdfObj::Name(name)) => name.clone(),
            _ => String::from("Identity-H"),
        },
        _ => String::from("Identity-H"),
    }
}

fn extract_to_unicode(
    font_dict: &BTreeMap<String, PdfObj>,
    objects: &BTreeMap<(u32, u16), PdfObj>,
) -> Option<BTreeMap<u32, String>> {
    let stream = match font_dict.get("ToUnicode") {
        Some(PdfObj::Reference(ref_)) => match resolve_reference(objects, ref_) {
            Some(PdfObj::Stream(stream)) => stream,
            _ => return None,
        },
        Some(PdfObj::Stream(stream)) => stream,
        _ => return None,
    };

    // Decompress stream
    let data = match crate::stream::handle_stream_filters(&stream.dict, &stream.data) {
        Ok(data) => data,
        Err(_) => return None,
    };

    parse_cmap(&data)
}

fn extract_differences(
    font_dict: &BTreeMap<String, PdfObj>,
    objects: &BTreeMap<(u32, u16), PdfObj>,
) -> Option<BTreeMap<u32, String>> {
    let encoding = match font_dict.get("Encoding") {
        Some(PdfObj::Dictionary(dict)) => dict,
        Some(PdfObj::Reference(enc_ref)) => match resolve_reference(objects, enc_ref) {
            Some(PdfObj::Dictionary(dict)) => dict,
            _ => return None,
        },
        _ => return None,
    };

    let differences = match encoding.get("Differences") {
        Some(PdfObj::Array(arr)) => arr,
        Some(PdfObj::Reference(diff_ref)) => match resolve_reference(objects, diff_ref) {
            Some(PdfObj::Array(arr)) => arr,
            _ => return None,
        },
        _ => return None,
    };

    let mut result = BTreeMap::new();
    let mut current_code = 0;

    for item in differences {
        match item {
            PdfObj::Number(n) => current_code = *n as u32,
            PdfObj::Name(name) => {
                result.insert(current_code, glyph_to_unicode(name));
                current_code += 1;
            }
            _ => {}
        }
    }

    Some(result)
}

fn parse_cmap(data: &[u8]) -> Option<BTreeMap<u32, String>> {
    let content = match core::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => {
            return None;
        }
    };
    let mut map = BTreeMap::new();

    // Simple CMap parser for bfchar and bfrange
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();

        if line.ends_with("beginbfchar") {
            i += 1;
            while i < lines.len() && !lines[i].trim_end().ends_with("endbfchar") {
                let l = lines[i].trim();
                if l.starts_with('<') {
                    let parts: Vec<&str> = l.split_ascii_whitespace().collect();
                    if parts.len() >= 2 {
                        if let (Some(src), Some(dst)) =
                            (parse_hex_u32(parts[0]), parse_hex_string(parts[1]))
                        {
                            map.insert(src, dst);
                        }
                    }
                }
                i += 1;
            }
        } else if line.ends_with("beginbfrange") {
            i += 1;
            while i < lines.len() && !lines[i].trim_end().ends_with("endbfrange") {
                let l = lines[i].trim();
                if l.starts_with('<') {
                    let parts: Vec<&str> = l.split_ascii_whitespace().collect();
                    if parts.len() >= 3 {
                        let start_hex = parts[0].trim_matches(|c| c == '<' || c == '>');
                        let end_hex = parts[1].trim_matches(|c| c == '<' || c == '>');
                        if let (Ok(start_code), Ok(end_code)) = (
                            u32::from_str_radix(start_hex, 16),
                            u32::from_str_radix(end_hex, 16),
                        ) {
                            if parts[2].starts_with('[') {
                                // Array format - not implemented yet in simplified version
                            } else {
                                // Range mapping
                                let dest_start_hex =
                                    parts[2].trim_matches(|c| c == '<' || c == '>');
                                if let Some(dest_start_str) = parse_hex_string(dest_start_hex) {
                                    let mut dest_start_codes: Vec<u32> =
                                        dest_start_str.chars().map(|ch| ch as u32).collect();
                                    for code in start_code..=end_code {
                                        let dest_string: String = dest_start_codes
                                            .iter()
                                            .map(|&u| char::from_u32(u).unwrap_or('?'))
                                            .collect();
                                        map.insert(code, dest_string);
                                        if let Some(last) = dest_start_codes.last_mut() {
                                            *last += 1;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                i += 1;
            }
        }

        i += 1;
    }

    if map.is_empty() {
        None
    } else {
        Some(map)
    }
}

fn parse_hex_u32(s: &str) -> Option<u32> {
    let s = s.trim_start_matches('<').trim_end_matches('>');
    u32::from_str_radix(s, 16).ok()
}

fn parse_hex_string(hex: &str) -> Option<String> {
    let hex = hex.trim_start_matches('<').trim_end_matches('>');

    if hex.is_empty() {
        return Some(String::new());
    }
    if hex.len() % 4 != 0 {
        return None;
    }

    let chunks: Vec<&[u8]> = hex.as_bytes().chunks(4).collect();
    let mut out = String::new();
    let mut i = 0;

    while i < chunks.len() {
        let chunk = chunks[i];
        if chunk.len() < 4 {
            break;
        }
        let part = core::str::from_utf8(chunk).ok()?;
        let code = u16::from_str_radix(part, 16).ok()?;

        if (0xD800..=0xDBFF).contains(&code) {
            if i + 1 < chunks.len() {
                let next_part = core::str::from_utf8(chunks[i + 1]).ok()?;
                if let Ok(low) = u16::from_str_radix(next_part, 16) {
                    if (0xDC00..=0xDFFF).contains(&low) {
                        let combined =
                            0x10000 + (((code - 0xD800) as u32) << 10) + ((low - 0xDC00) as u32);
                        if let Some(ch) = char::from_u32(combined) {
                            out.push(ch);
                            i += 2;
                            continue;
                        }
                    }
                }
            }
            out.push('�');
            i += 1;
            continue;
        } else if (0xDC00..=0xDFFF).contains(&code) {
            out.push('�');
        } else if let Some(ch) = char::from_u32(code as u32) {
            out.push(ch);
        } else {
            out.push('�');
        }
        i += 1;
    }

    Some(out)
}


fn glyph_to_unicode(glyph_name: &str) -> String {
    // Common glyph name mappings
    match glyph_name {
        "space" => " ",
        "exclam" => "!",
        "quotedbl" => "\"",
        "numbersign" => "#",
        "dollar" => "$",
        "percent" => "%",
        "ampersand" => "&",
        "quotesingle" => "'",
        "parenleft" => "(",
        "parenright" => ")",
        "asterisk" => "*",
        "plus" => "+",
        "comma" => ",",
        "hyphen" | "minus" => "-",
        "period" => ".",
        "slash" => "/",
        "zero" => "0",
        "one" => "1",
        "two" => "2",
        "three" => "3",
        "four" => "4",
        "five" => "5",
        "six" => "6",
        "seven" => "7",
        "eight" => "8",
        "nine" => "9",
        "colon" => ":",
        "semicolon" => ";",
        "less" => "<",
        "equal" => "=",
        "greater" => ">",
        "question" => "?",
        "at" => "@",
        "A" => "A",
        "B" => "B",
        "C" => "C",
        "D" => "D",
        "E" => "E",
        "F" => "F",
        "G" => "G",
        "H" => "H",
        "I" => "I",
        "J" => "J",
        "K" => "K",
        "L" => "L",
        "M" => "M",
        "N" => "N",
        "O" => "O",
        "P" => "P",
        "Q" => "Q",
        "R" => "R",
        "S" => "S",
        "T" => "T",
        "U" => "U",
        "V" => "V",
        "W" => "W",
        "X" => "X",
        "Y" => "Y",
        "Z" => "Z",
        "bracketleft" => "[",
        "backslash" => "\\",
        "bracketright" => "]",
        "asciicircum" => "^",
        "underscore" => "_",
        "grave" => "`",
        "a" => "a",
        "b" => "b",
        "c" => "c",
        "d" => "d",
        "e" => "e",
        "f" => "f",
        "g" => "g",
        "h" => "h",
        "i" => "i",
        "j" => "j",
        "k" => "k",
        "l" => "l",
        "m" => "m",
        "n" => "n",
        "o" => "o",
        "p" => "p",
        "q" => "q",
        "r" => "r",
        "s" => "s",
        "t" => "t",
        "u" => "u",
        "v" => "v",
        "w" => "w",
        "x" => "x",
        "y" => "y",
        "z" => "z",
        "braceleft" => "{",
        "bar" => "|",
        "braceright" => "}",
        "asciitilde" => "~",
        _ => "?", // Unknown glyph
    }
    .to_string()
}
