use crate::page::PageContent;
use crate::stream::handle_stream_filters;
use crate::PdfError;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::str;

#[derive(Debug, Clone, PartialEq)]
pub enum PdfObj {
    Null,
    Boolean(bool),
    Number(f32),
    Name(String),
    String(Vec<u8>),
    Array(Vec<PdfObj>),
    Dictionary(BTreeMap<String, PdfObj>),
    Stream(PdfStream),
    Reference((u32, u16)),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PdfStream {
    pub dict: BTreeMap<String, PdfObj>,
    pub data: Vec<u8>,
}

pub struct Parser<'a> {
    pub data: &'a [u8],
    pub pos: usize,
    pub len: usize,
}

impl<'a> Parser<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        let len = data.len();
        Self { data, pos: 0, len }
    }

    fn peek(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    pub fn remaining_starts_with(&self, pattern: &[u8]) -> bool {
        self.data[self.pos..].starts_with(pattern)
    }

    pub fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_ascii_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn skip_comment(&mut self) {
        if self.peek() == Some(b'%') {
            while let Some(ch) = self.peek() {
                self.advance();
                if ch == b'\n' || ch == b'\r' {
                    break;
                }
            }
        }
    }

    pub fn parse_number(&mut self) -> Result<PdfObj, String> {
        let start = self.pos;
        let mut has_dot = false;

        if self.peek() == Some(b'-') || self.peek() == Some(b'+') {
            self.advance();
        }

        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                self.advance();
            } else if ch == b'.' && !has_dot {
                has_dot = true;
                self.advance();
            } else {
                break;
            }
        }

        let num_str =
            str::from_utf8(&self.data[start..self.pos]).map_err(|_| "Invalid UTF-8 in number")?;

        let num = num_str
            .parse::<f32>()
            .map_err(|_| "Failed to parse number")?;

        Ok(PdfObj::Number(num))
    }

    fn parse_name(&mut self) -> Result<PdfObj, String> {
        if self.peek() != Some(b'/') {
            return Err("Expected name to start with /".to_string());
        }
        self.advance();

        let start = self.pos;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_whitespace() || b"()<>[]{}/%".contains(&ch) {
                break;
            }
            self.advance();
        }

        let name =
            str::from_utf8(&self.data[start..self.pos]).map_err(|_| "Invalid UTF-8 in name")?;

        Ok(PdfObj::Name(name.to_string()))
    }

    fn parse_string(&mut self) -> Result<PdfObj, String> {
        if self.peek() != Some(b'(') {
            return Err("Expected string to start with (".to_string());
        }
        self.advance();

        let mut result = Vec::new();
        let mut paren_depth = 1;
        let mut escape = false;

        while paren_depth > 0 {
            match self.peek() {
                None => return Err("Unexpected end of string".to_string()),
                Some(b'\\') if !escape => {
                    escape = true;
                    self.advance();
                }
                Some(ch) => {
                    if escape {
                        let escaped = match ch {
                            b'n' => b'\n',
                            b'r' => b'\r',
                            b't' => b'\t',
                            b'b' => b'\x08',
                            b'f' => b'\x0C',
                            b'(' => b'(',
                            b')' => b')',
                            b'\\' => b'\\',
                            _ => ch,
                        };
                        result.push(escaped);
                        escape = false;
                    } else {
                        match ch {
                            b'(' => {
                                paren_depth += 1;
                                result.push(ch);
                            }
                            b')' => {
                                paren_depth -= 1;
                                if paren_depth > 0 {
                                    result.push(ch);
                                }
                            }
                            _ => result.push(ch),
                        }
                    }
                    self.advance();
                }
            }
        }

        Ok(PdfObj::String(result))
    }

    fn parse_hex_string(&mut self) -> Result<PdfObj, String> {
        if self.peek() != Some(b'<') {
            return Err("Expected hex string to start with <".to_string());
        }
        self.advance();

        let mut hex_chars = Vec::new();

        loop {
            self.skip_whitespace();
            match self.peek() {
                Some(b'>') => {
                    self.advance();
                    break;
                }
                Some(ch) if ch.is_ascii_hexdigit() => {
                    hex_chars.push(ch);
                    self.advance();
                }
                Some(_) => return Err("Invalid character in hex string".to_string()),
                None => return Err("Unexpected end of hex string".to_string()),
            }
        }

        // Pad with 0 if odd number of hex digits
        if hex_chars.len() % 2 == 1 {
            hex_chars.push(b'0');
        }

        let mut result = Vec::new();
        for chunk in hex_chars.chunks(2) {
            let high = hex_digit_value(chunk[0])?;
            let low = hex_digit_value(chunk[1])?;
            result.push((high << 4) | low);
        }

        Ok(PdfObj::String(result))
    }

    fn parse_array(&mut self) -> Result<PdfObj, String> {
        if self.peek() != Some(b'[') {
            return Err("Expected array to start with [".to_string());
        }
        self.advance();

        let mut array = Vec::new();

        loop {
            self.skip_whitespace_and_comments();

            if self.peek() == Some(b']') {
                self.advance();
                break;
            }

            array.push(self.parse_object()?);
        }

        Ok(PdfObj::Array(array))
    }

    pub fn parse_dictionary(&mut self) -> Result<BTreeMap<String, PdfObj>, String> {
        // Skip whitespace before dictionary
        self.skip_whitespace_and_comments();

        // Debug: show what we're looking at
        let preview_len = core::cmp::min(20, self.len.saturating_sub(self.pos));
        let preview = &self.data[self.pos..self.pos + preview_len];

        if self.peek() != Some(b'<') || self.data.get(self.pos + 1) != Some(&b'<') {
            return Err(alloc::format!(
                "Expected dictionary to start with <<, found: {preview:?}"
            ));
        }
        self.advance();
        self.advance();

        let mut dict = BTreeMap::new();

        loop {
            self.skip_whitespace_and_comments();

            if self.peek() == Some(b'>') && self.data.get(self.pos + 1) == Some(&b'>') {
                self.advance();
                self.advance();
                break;
            }

            let key = match self.parse_object()? {
                PdfObj::Name(name) => name,
                _ => return Err("Dictionary key must be a name".to_string()),
            };

            self.skip_whitespace_and_comments();
            let value = self.parse_object()?;

            dict.insert(key, value);
        }

        Ok(dict)
    }

    fn parse_reference(&mut self, num: u32) -> Result<PdfObj, String> {
        self.skip_whitespace();

        let gen = match self.parse_object()? {
            PdfObj::Number(n) => n as u16,
            _ => return Err("Expected generation number".to_string()),
        };

        self.skip_whitespace();

        if self.peek() == Some(b'R') {
            self.advance();
            Ok(PdfObj::Reference((num, gen)))
        } else {
            Err("Expected R after generation number".to_string())
        }
    }

    fn check_keyword(&self, keyword: &str) -> bool {
        let end = self.pos + keyword.len();
        if end > self.data.len() {
            return false;
        }
        &self.data[self.pos..end] == keyword.as_bytes()
    }

    pub fn skip_whitespace_and_comments(&mut self) {
        loop {
            self.skip_whitespace();
            if self.peek() == Some(b'%') {
                self.skip_comment();
            } else {
                break;
            }
        }
    }

    pub fn parse_value(&mut self) -> Result<PdfObj, String> {
        self.parse_object()
    }

    pub fn parse_object(&mut self) -> Result<PdfObj, String> {
        self.skip_whitespace_and_comments();

        match self.peek() {
            None => Err("Unexpected end of input".to_string()),
            Some(b'n') if self.check_keyword("null") => {
                self.pos += 4;
                Ok(PdfObj::Null)
            }
            Some(b't') if self.check_keyword("true") => {
                self.pos += 4;
                Ok(PdfObj::Boolean(true))
            }
            Some(b'f') if self.check_keyword("false") => {
                self.pos += 5;
                Ok(PdfObj::Boolean(false))
            }
            Some(b'/') => self.parse_name(),
            Some(b'(') => self.parse_string(),
            Some(b'<') => {
                if self.data.get(self.pos + 1) == Some(&b'<') {
                    self.parse_dictionary().map(PdfObj::Dictionary)
                } else {
                    self.parse_hex_string()
                }
            }
            Some(b'[') => self.parse_array(),
            Some(ch) if ch.is_ascii_digit() || ch == b'-' || ch == b'+' || ch == b'.' => {
                let num_obj = self.parse_number()?;

                // Check if this is a reference
                if let PdfObj::Number(num) = num_obj {
                    let saved_pos = self.pos;
                    self.skip_whitespace();

                    if let Some(ch) = self.peek() {
                        if ch.is_ascii_digit() {
                            // Might be a reference
                            match self.parse_reference(num as u32) {
                                Ok(ref_obj) => Ok(ref_obj),
                                Err(_) => {
                                    self.pos = saved_pos;
                                    Ok(num_obj)
                                }
                            }
                        } else {
                            self.pos = saved_pos;
                            Ok(num_obj)
                        }
                    } else {
                        Ok(num_obj)
                    }
                } else {
                    Ok(num_obj)
                }
            }
            Some(ch) => Err(alloc::format!("Unexpected character: {}", ch as char)),
        }
    }
}

fn hex_digit_value(ch: u8) -> Result<u8, String> {
    match ch {
        b'0'..=b'9' => Ok(ch - b'0'),
        b'A'..=b'F' => Ok(ch - b'A' + 10),
        b'a'..=b'f' => Ok(ch - b'a' + 10),
        _ => Err("Invalid hex digit".to_string()),
    }
}

type PdfParseResult = (Vec<PageContent>, BTreeMap<(u32, u16), PdfObj>);

pub fn parse_pdf(data: &[u8]) -> Result<PdfParseResult, PdfError> {
    let mut parser = Parser::new(data);
    let mut objects: BTreeMap<(u32, u16), PdfObj> = BTreeMap::new();

    // Skip PDF header (e.g. %PDF-1.7)
    if parser.pos < parser.len && parser.remaining_starts_with(b"%PDF") {
        // find end of line
        while parser.pos < parser.len
            && parser.data[parser.pos] != b'\n'
            && parser.data[parser.pos] != b'\r'
        {
            parser.pos += 1;
        }
        // skip newline(s)
        if parser.pos < parser.len && parser.data[parser.pos] == b'\r' {
            parser.pos += 1;
            if parser.pos < parser.len && parser.data[parser.pos] == b'\n' {
                parser.pos += 1;
            }
        } else if parser.pos < parser.len && parser.data[parser.pos] == b'\n' {
            parser.pos += 1;
        }
    }

    // Parse objects linearly
    loop {
        parser.skip_whitespace_and_comments();
        if parser.pos >= parser.len {
            break;
        }

        if parser.remaining_starts_with(b"xref") || parser.remaining_starts_with(b"trailer") {
            break;
        }

        if parser.remaining_starts_with(b"startxref") {
            parser.pos += 9; // len("startxref")
            parser.skip_whitespace_and_comments();
            if parser.pos < parser.len {
                let _ = parser.parse_number();
            }
            parser.skip_whitespace_and_comments();
            if parser.remaining_starts_with(b"%%EOF") {
                parser.pos += 5;
            }
            continue;
        }

        // Parse object: "<obj_id> <gen_id> obj"
        let obj_id = match parser.parse_number().map_err(PdfError::ParseError)? {
            PdfObj::Number(num) => num as u32,
            _ => return Err(PdfError::ParseError("Invalid object id".to_string())),
        };
        parser.skip_whitespace_and_comments();

        let gen_id = match parser.parse_number().map_err(PdfError::ParseError)? {
            PdfObj::Number(num) => num as u16,
            _ => {
                return Err(PdfError::ParseError(
                    "Invalid generation number".to_string(),
                ))
            }
        };
        parser.skip_whitespace_and_comments();

        if !parser.remaining_starts_with(b"obj") {
            return Err(PdfError::ParseError("Missing 'obj' keyword".to_string()));
        }
        parser.pos += 3;
        parser.skip_whitespace_and_comments();

        // Parse object value
        let obj_value = if parser.pos < parser.len
            && parser.data[parser.pos] == b'<'
            && parser.pos + 1 < parser.len
            && parser.data[parser.pos + 1] == b'<'
        {
            // Dictionary object - don't advance, let parse_dictionary handle it
            let dict_obj = parser.parse_dictionary().map_err(PdfError::ParseError)?;

            parser.skip_whitespace_and_comments();
            if parser.remaining_starts_with(b"stream") {
                // Handle stream - this is where we handle it inline
                parser.pos += 6;

                // Skip EOL after stream
                if parser.pos < parser.len && parser.data[parser.pos] == b'\r' {
                    parser.pos += 1;
                    if parser.pos < parser.len && parser.data[parser.pos] == b'\n' {
                        parser.pos += 1;
                    }
                } else if parser.pos < parser.len && parser.data[parser.pos] == b'\n' {
                    parser.pos += 1;
                }

                let stream_start = parser.pos;

                // Find endstream
                let search_term = b"endstream";
                let search_len = search_term.len();

                // Try to use Length if available
                let stream_data = if let Some(PdfObj::Number(length)) = dict_obj.get("Length") {
                    let length = *length as usize;
                    if stream_start + length <= parser.len {
                        parser.pos = stream_start + length;
                        let data_end = stream_start + length;

                        // Skip whitespace before endstream
                        while parser.pos > stream_start
                            && parser.data[parser.pos - 1].is_ascii_whitespace()
                        {
                            parser.pos -= 1;
                        }
                        parser.skip_whitespace_and_comments();
                        if !parser.remaining_starts_with(search_term) {
                            return Err(PdfError::ParseError("Missing 'endstream'".to_string()));
                        }
                        parser.data[stream_start..data_end].to_vec()
                    } else {
                        // Length is wrong, search for endstream
                        search_for_endstream(&parser, stream_start, search_term)?
                    }
                } else {
                    // No length, search for endstream
                    search_for_endstream(&parser, stream_start, search_term)?
                };

                parser.pos += search_len;
                parser.skip_whitespace_and_comments();
                if !parser.remaining_starts_with(b"endobj") {
                    return Err(PdfError::ParseError(
                        "Missing 'endobj' after stream".to_string(),
                    ));
                }
                parser.pos += 6;

                let stream_obj = PdfStream {
                    dict: dict_obj,
                    data: stream_data,
                };

                // Check if this is an object stream and parse it
                if let Some(PdfObj::Name(t)) = stream_obj.dict.get("Type") {
                    if t == "ObjStm" {
                        if let (Some(PdfObj::Number(first)), Some(PdfObj::Number(n))) =
                            (stream_obj.dict.get("First"), stream_obj.dict.get("N"))
                        {
                            // Decompress and parse the object stream
                            if let Ok(decompressed) =
                                handle_stream_filters(&stream_obj.dict, &stream_obj.data)
                            {
                                parse_obj_stream(
                                    &decompressed,
                                    *first as usize,
                                    *n as usize,
                                    &mut objects,
                                )?;
                            }
                        }
                    }
                }

                PdfObj::Stream(stream_obj)
            } else {
                // Just a dictionary
                parser.skip_whitespace_and_comments();
                if !parser.remaining_starts_with(b"endobj") {
                    return Err(PdfError::ParseError(
                        "Missing 'endobj' for dictionary object".to_string(),
                    ));
                }
                parser.pos += 6;
                PdfObj::Dictionary(dict_obj)
            }
        } else {
            // Other value type
            let value_obj = parser.parse_value().map_err(PdfError::ParseError)?;
            parser.skip_whitespace_and_comments();
            if !parser.remaining_starts_with(b"endobj") {
                return Err(PdfError::ParseError(
                    "Missing 'endobj' for object".to_string(),
                ));
            }
            parser.pos += 6;
            value_obj
        };

        objects.insert((obj_id, gen_id), obj_value);
    }

    // Find trailer or cross-reference stream
    let mut trailer_dict = None;

    // First check if we have a traditional trailer
    if parser.remaining_starts_with(b"trailer") {
        parser.pos += 7; // Skip "trailer"
        parser.skip_whitespace_and_comments();
        trailer_dict = Some(parser.parse_dictionary().map_err(PdfError::ParseError)?);
    } else {
        // Search for trailer backwards
        let data_bytes = parser.data;
        for i in (0..data_bytes.len().saturating_sub(7)).rev() {
            if data_bytes[i..].starts_with(b"trailer") {
                parser.pos = i + 7; // Skip "trailer"
                parser.skip_whitespace_and_comments();
                trailer_dict = Some(parser.parse_dictionary().map_err(PdfError::ParseError)?);
                break;
            }
        }
    }

    // If no traditional trailer found, look for cross-reference stream
    let trailer_dict = if let Some(dict) = trailer_dict {
        dict
    } else {
        // Look for a cross-reference stream object
        // These have Type/XRef and contain the trailer dictionary
        let mut xref_stream_dict = None;
        let mut xref_stream_data = None;

        for ((_id, _gen), obj) in objects.iter() {
            if let PdfObj::Stream(stream) = obj {
                if let Some(PdfObj::Name(type_name)) = stream.dict.get("Type") {
                    if type_name == "XRef" {
                        xref_stream_dict = Some(stream.dict.clone());
                        xref_stream_data = Some((stream.dict.clone(), stream.data.clone()));
                        break;
                    }
                }
            }
        }

        // If we found an XRef stream, parse it to get more objects
        if let Some((xref_dict, xref_data)) = xref_stream_data {
            // Parse the cross-reference stream to get object offsets
            let xref_stream = PdfStream {
                dict: xref_dict,
                data: xref_data,
            };
            parse_xref_stream(&mut objects, parser.data, &xref_stream)?;
        }

        xref_stream_dict.ok_or(PdfError::ParseError(alloc::format!(
            "No trailer or cross-reference stream found. Parsed {} objects",
            objects.len()
        )))?
    };

    // Debug: log how many objects we parsed
    let _obj_count = objects.len();

    // Get root reference
    let root_ref = match trailer_dict.get("Root") {
        Some(PdfObj::Reference(r)) => r,
        _ => {
            return Err(PdfError::ParseError(alloc::format!(
                "No Root in trailer. Trailer: {trailer_dict:?}"
            )))
        }
    };

    // Debug: log object count
    // Only enable for debugging
    // if obj_count == 0 {
    //     return Err(PdfError::ParseError(alloc::format!("No objects parsed. Found XRef stream: {}", xref_stream_dict.is_some())));
    // }

    // Now find pages using the existing page tree parser
    let pages = parse_page_tree(&objects, root_ref)?;

    Ok((pages, objects))
}

fn search_for_endstream(
    parser: &Parser,
    stream_start: usize,
    search_term: &[u8],
) -> Result<Vec<u8>, PdfError> {
    let search_len = search_term.len();
    let mut endstream_index = None;
    let mut i = stream_start;

    while i + search_len <= parser.len {
        if &parser.data[i..i + search_len] == search_term {
            // Check context
            let prev_ok = if i == 0 {
                true
            } else {
                let prev = parser.data[i - 1];
                prev == b'\n' || prev == b'\r' || prev.is_ascii_whitespace()
            };
            let next_ok = if i + search_len >= parser.len
                || parser.data[i + search_len..].starts_with(b"endobj")
            {
                true
            } else {
                let next = parser.data[i + search_len];
                next.is_ascii_whitespace()
            };
            if prev_ok && next_ok {
                endstream_index = Some(i);
                break;
            }
        }
        i += 1;
    }

    let end_idx = endstream_index.ok_or(PdfError::ParseError("Missing 'endstream'".to_string()))?;
    let mut data_end = end_idx;

    // Trim trailing whitespace
    while data_end > stream_start && parser.data[data_end - 1].is_ascii_whitespace() {
        data_end -= 1;
    }

    Ok(parser.data[stream_start..data_end].to_vec())
}

fn parse_page_tree(
    objects: &BTreeMap<(u32, u16), PdfObj>,
    root_ref: &(u32, u16),
) -> Result<Vec<PageContent>, PdfError> {
    let root = resolve_reference(objects, root_ref).ok_or_else(|| {
        PdfError::ParseError(alloc::format!(
            "Could not resolve root reference {:?}. Available objects: {:?}",
            root_ref,
            objects.keys().collect::<Vec<_>>()
        ))
    })?;

    let root_dict = match root {
        PdfObj::Dictionary(dict) => dict,
        _ => return Err(PdfError::ParseError("Root is not a dictionary".to_string())),
    };

    let pages_ref = match root_dict.get("Pages") {
        Some(PdfObj::Reference(r)) => r,
        _ => return Err(PdfError::ParseError("No Pages in root".to_string())),
    };

    let mut pages = Vec::new();
    let mut visited = BTreeSet::new();

    collect_pages(
        objects,
        pages_ref,
        &mut pages,
        &mut visited,
        &BTreeMap::new(),
    )?;

    Ok(pages)
}

fn collect_pages(
    objects: &BTreeMap<(u32, u16), PdfObj>,
    page_ref: &(u32, u16),
    pages: &mut Vec<PageContent>,
    visited: &mut BTreeSet<(u32, u16)>,
    inherited_resources: &BTreeMap<String, PdfObj>,
) -> Result<(), PdfError> {
    if visited.contains(page_ref) {
        return Ok(());
    }
    visited.insert(*page_ref);

    let page_obj = resolve_reference(objects, page_ref).ok_or_else(|| {
        PdfError::ParseError(alloc::format!(
            "Could not resolve page reference {page_ref:?}"
        ))
    })?;

    let page_dict = match page_obj {
        PdfObj::Dictionary(dict) => dict,
        _ => return Err(PdfError::ParseError("Page is not a dictionary".to_string())),
    };

    let page_type = match page_dict.get("Type") {
        Some(PdfObj::Name(name)) => name.as_str(),
        _ => "",
    };

    match page_type {
        "Page" => {
            let mut page_content = PageContent::new();

            // Merge inherited and local resources
            let mut resources = inherited_resources.clone();
            if let Some(PdfObj::Dictionary(local_res)) = page_dict.get("Resources") {
                for (k, v) in local_res {
                    resources.insert(k.clone(), v.clone());
                }
            } else if let Some(PdfObj::Reference(res_ref)) = page_dict.get("Resources") {
                if let Some(PdfObj::Dictionary(res_dict)) = resolve_reference(objects, res_ref) {
                    for (k, v) in res_dict {
                        resources.insert(k.clone(), v.clone());
                    }
                }
            }

            page_content.resources = resources;

            // Extract fonts
            match page_content.resources.get("Font") {
                Some(PdfObj::Dictionary(font_dict)) => {
                    page_content.fonts = crate::font::extract_fonts(font_dict, objects);
                }
                Some(PdfObj::Reference(font_ref)) => {
                    if let Some(PdfObj::Dictionary(font_dict)) =
                        resolve_reference(objects, font_ref)
                    {
                        page_content.fonts = crate::font::extract_fonts(font_dict, objects);
                    }
                }
                _ => {}
            }

            // Extract content streams
            match page_dict.get("Contents") {
                Some(PdfObj::Reference(content_ref)) => {
                    if let Some(content_obj) = resolve_reference(objects, content_ref) {
                        match content_obj {
                            PdfObj::Stream(stream) => {
                                let decompressed =
                                    handle_stream_filters(&stream.dict, &stream.data)
                                        .map_err(PdfError::ParseError)?;
                                page_content.content_streams.push(decompressed);
                            }
                            PdfObj::Array(arr) => {
                                for item in arr {
                                    if let PdfObj::Reference(stream_ref) = item {
                                        if let Some(PdfObj::Stream(stream)) =
                                            resolve_reference(objects, stream_ref)
                                        {
                                            let decompressed =
                                                handle_stream_filters(&stream.dict, &stream.data)
                                                    .map_err(PdfError::ParseError)?;
                                            page_content.content_streams.push(decompressed);
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Some(PdfObj::Array(contents)) => {
                    for content in contents {
                        if let PdfObj::Reference(content_ref) = content {
                            if let Some(PdfObj::Stream(stream)) =
                                resolve_reference(objects, content_ref)
                            {
                                let decompressed =
                                    handle_stream_filters(&stream.dict, &stream.data)
                                        .map_err(PdfError::ParseError)?;
                                page_content.content_streams.push(decompressed);
                            }
                        }
                    }
                }
                _ => {}
            }

            pages.push(page_content);
        }
        "Pages" => {
            // Get resources to inherit
            let mut new_inherited = inherited_resources.clone();
            if let Some(PdfObj::Dictionary(res)) = page_dict.get("Resources") {
                for (k, v) in res {
                    new_inherited.insert(k.clone(), v.clone());
                }
            } else if let Some(PdfObj::Reference(res_ref)) = page_dict.get("Resources") {
                if let Some(PdfObj::Dictionary(res_dict)) = resolve_reference(objects, res_ref) {
                    for (k, v) in res_dict {
                        new_inherited.insert(k.clone(), v.clone());
                    }
                }
            }

            // Process kids
            if let Some(PdfObj::Array(kids)) = page_dict.get("Kids") {
                for kid in kids {
                    if let PdfObj::Reference(kid_ref) = kid {
                        collect_pages(objects, kid_ref, pages, visited, &new_inherited)?;
                    }
                }
            }
        }
        _ => {}
    }

    Ok(())
}

pub fn resolve_reference<'a>(
    objects: &'a BTreeMap<(u32, u16), PdfObj>,
    reference: &(u32, u16),
) -> Option<&'a PdfObj> {
    objects.get(reference)
}

fn parse_xref_stream(
    objects: &mut BTreeMap<(u32, u16), PdfObj>,
    pdf_data: &[u8],
    xref_stream: &PdfStream,
) -> Result<(), PdfError> {
    // Get the W array which describes field widths
    let w_array = match xref_stream.dict.get("W") {
        Some(PdfObj::Array(arr)) => arr,
        _ => {
            return Err(PdfError::ParseError(
                "XRef stream missing W array".to_string(),
            ))
        }
    };

    if w_array.len() != 3 {
        return Err(PdfError::ParseError(
            "XRef stream W array must have 3 elements".to_string(),
        ));
    }

    let w: Vec<usize> = w_array
        .iter()
        .map(|obj| match obj {
            PdfObj::Number(n) => *n as usize,
            _ => 0,
        })
        .collect();

    // Get the Index array (if present) or use default [0, Size]
    let index_array = match xref_stream.dict.get("Index") {
        Some(PdfObj::Array(arr)) => {
            let mut indices = Vec::new();
            for i in (0..arr.len()).step_by(2) {
                if let (Some(PdfObj::Number(start)), Some(PdfObj::Number(count))) =
                    (arr.get(i), arr.get(i + 1))
                {
                    indices.push((*start as u32, *count as u32));
                }
            }
            indices
        }
        _ => {
            // Default to [0, Size]
            match xref_stream.dict.get("Size") {
                Some(PdfObj::Number(size)) => vec![(0, *size as u32)],
                _ => vec![(0, 0)],
            }
        }
    };

    // Decompress the stream data
    let decompressed_data = handle_stream_filters(&xref_stream.dict, &xref_stream.data)
        .map_err(PdfError::ParseError)?;

    // Parse entries
    let entry_size = w[0] + w[1] + w[2];
    let mut data_pos = 0;

    for (start_obj_num, count) in index_array {
        for i in 0..count {
            if data_pos + entry_size > decompressed_data.len() {
                break;
            }

            let obj_num = start_obj_num + i;
            let entry_data = &decompressed_data[data_pos..data_pos + entry_size];
            data_pos += entry_size;

            // Parse entry fields
            let mut field_pos = 0;

            // Field 1: Type (default 1 if w[0] == 0)
            let entry_type = if w[0] == 0 {
                1
            } else {
                let mut val = 0u64;
                for j in 0..w[0] {
                    val = (val << 8) | (entry_data[field_pos + j] as u64);
                }
                field_pos += w[0];
                val
            };

            // Field 2: Offset or object number
            let mut field2 = 0u64;
            for j in 0..w[1] {
                field2 = (field2 << 8) | (entry_data[field_pos + j] as u64);
            }
            field_pos += w[1];

            // Field 3: Generation or index
            let mut field3 = 0u64;
            for j in 0..w[2] {
                field3 = (field3 << 8) | (entry_data[field_pos + j] as u64);
            }

            // Process based on type
            match entry_type {
                1 => {
                    // Type 1: In-use object
                    let offset = field2 as usize;
                    let gen = field3 as u16;

                    // Parse the object at this offset
                    if offset < pdf_data.len() {
                        let mut obj_parser = Parser::new(&pdf_data[offset..]);

                        // Parse object header
                        if let Ok(PdfObj::Number(parsed_num)) = obj_parser.parse_number() {
                            if parsed_num as u32 == obj_num {
                                obj_parser.skip_whitespace();
                                if let Ok(PdfObj::Number(_)) = obj_parser.parse_number() {
                                    obj_parser.skip_whitespace();
                                    if obj_parser.remaining_starts_with(b"obj") {
                                        obj_parser.pos += 3;
                                        obj_parser.skip_whitespace_and_comments();

                                        // Parse the object value
                                        if let Ok(obj_value) = parse_object_value(&mut obj_parser) {
                                            objects.insert((obj_num, gen), obj_value);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                2 => {
                    // Type 2: Compressed object
                    // These are stored in object streams, not supported yet
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn parse_object_value(parser: &mut Parser) -> Result<PdfObj, String> {
    // Check if it's a dictionary that might be a stream
    if parser.peek() == Some(b'<') && parser.data.get(parser.pos + 1) == Some(&b'<') {
        let dict = parser.parse_dictionary()?;
        parser.skip_whitespace_and_comments();

        if parser.remaining_starts_with(b"stream") {
            // Parse stream
            parser.pos += 6;

            // Skip EOL after stream
            if parser.pos < parser.len && parser.data[parser.pos] == b'\r' {
                parser.pos += 1;
                if parser.pos < parser.len && parser.data[parser.pos] == b'\n' {
                    parser.pos += 1;
                }
            } else if parser.pos < parser.len && parser.data[parser.pos] == b'\n' {
                parser.pos += 1;
            }

            let stream_start = parser.pos;

            // Find endstream
            let search_term = b"endstream";
            let stream_data =
                search_for_endstream(parser, stream_start, search_term).map_err(|e| match e {
                    PdfError::ParseError(s) => s,
                    _ => "Stream parsing error".to_string(),
                })?;
            parser.pos += search_term.len();

            Ok(PdfObj::Stream(PdfStream {
                dict,
                data: stream_data,
            }))
        } else {
            Ok(PdfObj::Dictionary(dict))
        }
    } else {
        parser.parse_object()
    }
}

fn parse_obj_stream(
    data: &[u8],
    first: usize,
    count: usize,
    objects: &mut BTreeMap<(u32, u16), PdfObj>,
) -> Result<(), PdfError> {
    let mut parser = Parser::new(data);
    let mut headers = Vec::new();

    // Parse headers
    for i in 0..count {
        parser.skip_whitespace_and_comments();
        let obj_num = match parser.parse_number() {
            Ok(PdfObj::Number(n)) => n as u32,
            _ => {
                return Err(PdfError::ParseError(alloc::format!(
                    "Invalid object number in ObjStm at index {}, pos: {}",
                    i,
                    parser.pos
                )))
            }
        };
        parser.skip_whitespace_and_comments();
        let offset = match parser.parse_number() {
            Ok(PdfObj::Number(n)) => n as usize,
            _ => {
                return Err(PdfError::ParseError(
                    "Invalid object offset in ObjStm".to_string(),
                ))
            }
        };
        headers.push((obj_num, offset));
    }

    // Parse objects
    for i in 0..count {
        let start = first + headers[i].1;
        let end = if i + 1 < count {
            first + headers[i + 1].1
        } else {
            data.len()
        };

        if start < data.len() && end <= data.len() && start < end {
            let mut sub_parser = Parser::new(&data[start..end]);
            if let Ok(value) = sub_parser.parse_value() {
                // Objects in streams always have generation 0
                objects.insert((headers[i].0, 0), value);
            }
        }
    }

    Ok(())
}
