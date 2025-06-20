use crate::parser::PdfObj;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use miniz_oxide::inflate::decompress_to_vec_zlib;

pub fn handle_stream_filters(
    stream_dict: &BTreeMap<String, PdfObj>,
    data: &[u8],
) -> Result<Vec<u8>, String> {
    match stream_dict.get("Filter") {
        Some(PdfObj::Name(filter)) => {
            let result = apply_filter(filter, data)?;
            // Check for DecodeParms
            if let Some(decode_parms) = stream_dict.get("DecodeParms") {
                apply_decode_parms(&result, decode_parms)
            } else {
                Ok(result)
            }
        }
        Some(PdfObj::Array(filters)) => {
            let mut result = data.to_vec();
            // Get DecodeParms array if present
            let decode_parms_array = match stream_dict.get("DecodeParms") {
                Some(PdfObj::Array(arr)) => Some(arr),
                _ => None,
            };

            for (i, filter) in filters.iter().enumerate() {
                if let PdfObj::Name(filter_name) = filter {
                    result = apply_filter(filter_name, &result)?;

                    // Apply corresponding DecodeParms if present
                    if let Some(parms_array) = decode_parms_array {
                        if let Some(parms) = parms_array.get(i) {
                            result = apply_decode_parms(&result, parms)?;
                        }
                    }
                }
            }
            Ok(result)
        }
        None => Ok(data.to_vec()),
        _ => Err("Invalid Filter type".into()),
    }
}

fn apply_filter(filter_name: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    match filter_name {
        "FlateDecode" => {
            // Debug: check data size
            if data.is_empty() {
                return Err("FlateDecode data is empty".to_string());
            }

            decompress_to_vec_zlib(data).map_err(|e| {
                alloc::format!(
                    "Failed to decompress FlateDecode data: {:?}, data size: {}",
                    e,
                    data.len()
                )
            })
        }
        "ASCIIHexDecode" => decode_ascii_hex(data),
        "ASCII85Decode" => decode_ascii85(data),
        _ => Err(alloc::format!("Unsupported filter: {filter_name}")),
    }
}

fn decode_ascii_hex(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    let mut chars = data.iter().filter(|&&b| !b.is_ascii_whitespace());

    loop {
        match (chars.next(), chars.next()) {
            (Some(&b'>'), _) | (None, _) => break,
            (Some(&a), Some(&b'>')) => {
                let high = hex_digit_value(a)?;
                result.push(high << 4);
                break;
            }
            (Some(&a), Some(&b)) => {
                let high = hex_digit_value(a)?;
                let low = hex_digit_value(b)?;
                result.push((high << 4) | low);
            }
            (Some(&a), None) => {
                let high = hex_digit_value(a)?;
                result.push(high << 4);
                break;
            }
        }
    }

    Ok(result)
}

fn decode_ascii85(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    let mut tuple = [0u8; 5];
    let mut count = 0;

    for &byte in data {
        if byte.is_ascii_whitespace() {
            continue;
        }

        if byte == b'~' {
            // Check for end marker ~>
            break;
        }

        if byte == b'z' && count == 0 {
            // Special case: z represents four null bytes
            result.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }

        if !(b'!'..=b'u').contains(&byte) {
            return Err("Invalid ASCII85 character".into());
        }

        tuple[count] = byte - b'!';
        count += 1;

        if count == 5 {
            let value = (tuple[0] as u32) * 85u32.pow(4)
                + (tuple[1] as u32) * 85u32.pow(3)
                + (tuple[2] as u32) * 85u32.pow(2)
                + (tuple[3] as u32) * 85
                + (tuple[4] as u32);

            result.push((value >> 24) as u8);
            result.push((value >> 16) as u8);
            result.push((value >> 8) as u8);
            result.push(value as u8);

            count = 0;
            tuple = [0u8; 5];
        }
    }

    // Handle remaining bytes
    if count > 0 {
        for slot in tuple.iter_mut().skip(count) {
            *slot = 84; // 'u' - '!'
        }

        let value = (tuple[0] as u32) * 85u32.pow(4)
            + (tuple[1] as u32) * 85u32.pow(3)
            + (tuple[2] as u32) * 85u32.pow(2)
            + (tuple[3] as u32) * 85
            + (tuple[4] as u32);

        for i in 0..(count - 1) {
            result.push((value >> (24 - 8 * i)) as u8);
        }
    }

    Ok(result)
}

fn hex_digit_value(ch: u8) -> Result<u8, String> {
    match ch {
        b'0'..=b'9' => Ok(ch - b'0'),
        b'A'..=b'F' => Ok(ch - b'A' + 10),
        b'a'..=b'f' => Ok(ch - b'a' + 10),
        _ => Err("Invalid hex digit".into()),
    }
}

fn apply_decode_parms(data: &[u8], decode_parms: &PdfObj) -> Result<Vec<u8>, String> {
    match decode_parms {
        PdfObj::Dictionary(dict) => {
            // Check for predictor
            if let Some(PdfObj::Number(predictor)) = dict.get("Predictor") {
                let predictor = *predictor as i32;
                if predictor > 1 {
                    // PNG predictors
                    if (10..=15).contains(&predictor) {
                        let columns = match dict.get("Columns") {
                            Some(PdfObj::Number(n)) => *n as usize,
                            _ => return Err("Missing Columns for predictor".to_string()),
                        };
                        apply_png_predictor(data, predictor, columns)
                    } else {
                        Err(alloc::format!("Unsupported predictor: {predictor}"))
                    }
                } else {
                    Ok(data.to_vec())
                }
            } else {
                Ok(data.to_vec())
            }
        }
        PdfObj::Null => Ok(data.to_vec()),
        _ => Err("Invalid DecodeParms type".to_string()),
    }
}

fn apply_png_predictor(data: &[u8], _predictor: i32, columns: usize) -> Result<Vec<u8>, String> {
    // PNG predictors work on rows
    let row_size = columns + 1; // +1 for predictor byte

    if data.len() % row_size != 0 {
        return Err("Invalid data size for predictor".to_string());
    }

    let mut result = Vec::with_capacity(data.len() - data.len() / row_size);
    let mut prev_row = vec![0u8; columns];

    for row_data in data.chunks(row_size) {
        if row_data.len() != row_size {
            break;
        }

        let predictor_byte = row_data[0];
        let row = &row_data[1..];
        let mut decoded_row = vec![0u8; columns];

        match predictor_byte {
            0 => {
                // No prediction
                decoded_row.copy_from_slice(row);
            }
            1 => {
                // Sub: each byte is the sum of itself and the byte to its left
                decoded_row[0] = row[0];
                for i in 1..columns {
                    decoded_row[i] = row[i].wrapping_add(decoded_row[i - 1]);
                }
            }
            2 => {
                // Up: each byte is the sum of itself and the corresponding byte in the previous row
                for i in 0..columns {
                    decoded_row[i] = row[i].wrapping_add(prev_row[i]);
                }
            }
            3 => {
                // Average: each byte is the sum of itself and the average of left and up
                for i in 0..columns {
                    let left = if i > 0 { decoded_row[i - 1] } else { 0 };
                    let up = prev_row[i];
                    let avg = (left as u16 + up as u16) / 2;
                    decoded_row[i] = row[i].wrapping_add(avg as u8);
                }
            }
            4 => {
                // Paeth: complex predictor
                for i in 0..columns {
                    let a = if i > 0 { decoded_row[i - 1] } else { 0 };
                    let b = prev_row[i];
                    let c = if i > 0 { prev_row[i - 1] } else { 0 };
                    decoded_row[i] = row[i].wrapping_add(paeth_predictor(a, b, c));
                }
            }
            _ => {
                // For predictor >= 10, the predictor byte determines the algorithm
                // In this case, we should use predictor - 10 as the actual algorithm
                let algo = if predictor_byte >= 10 {
                    predictor_byte - 10
                } else {
                    predictor_byte
                };
                match algo {
                    0 => decoded_row.copy_from_slice(row),
                    1 => {
                        decoded_row[0] = row[0];
                        for i in 1..columns {
                            decoded_row[i] = row[i].wrapping_add(decoded_row[i - 1]);
                        }
                    }
                    2 => {
                        for i in 0..columns {
                            decoded_row[i] = row[i].wrapping_add(prev_row[i]);
                        }
                    }
                    _ => return Err(alloc::format!("Unsupported predictor algorithm: {algo}")),
                }
            }
        }

        result.extend_from_slice(&decoded_row);
        prev_row = decoded_row;
    }

    Ok(result)
}

fn paeth_predictor(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let pa = (p - a as i16).abs();
    let pb = (p - b as i16).abs();
    let pc = (p - c as i16).abs();

    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}
