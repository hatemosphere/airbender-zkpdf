#![no_std]
#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use crypto_bigint::{Zero, U256};

/// An ASN.1 OID component (we'll use U256 for each component)
pub type OidComponent = U256;

/// An ASN.1 OID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OID(pub Vec<OidComponent>);

impl OID {
    /// Generate an ASN.1 OID. The vector should be in the obvious format,
    /// with each component going left-to-right.
    pub fn new(x: Vec<OidComponent>) -> OID {
        OID(x)
    }

    /// Create OID from u64 values
    pub fn from_slice(values: &[u64]) -> OID {
        let mut components = Vec::new();
        for &val in values {
            components.push(U256::from_u64(val));
        }
        OID(components)
    }

    pub fn as_vec(&self) -> Vec<u64> {
        let mut vec = Vec::new();
        for val in self.0.iter() {
            // Convert to u64, assuming values fit
            let bytes = val.to_le_bytes();
            let u64_val = u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]);
            vec.push(u64_val);
        }
        vec
    }
}

/// A handy macro for generating OIDs from a sequence of `u64`s.
#[macro_export]
macro_rules! oid {
    ( $( $e: expr ),* ) => {{
        $crate::OID::from_slice(&[$($e as u64),*])
    }};
}

/// An ASN.1 class.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ASN1Class {
    Universal,
    Application,
    ContextSpecific,
    Private,
}

/// For signed integers, we'll store the raw bytes and a sign flag
#[derive(Clone, Debug, PartialEq)]
pub struct SignedBigInt {
    pub bytes: Vec<u8>,
    pub negative: bool,
}

impl SignedBigInt {
    pub fn from_signed_bytes_be(bytes: &[u8]) -> Self {
        if bytes.is_empty() {
            return SignedBigInt {
                bytes: vec![0],
                negative: false,
            };
        }

        // Check if negative (high bit set)
        let negative = bytes[0] & 0x80 != 0;

        if negative {
            // Two's complement - invert and add 1
            let mut inverted = Vec::with_capacity(bytes.len());
            let mut carry = 1u8;

            for &byte in bytes.iter().rev() {
                let inverted_byte = !byte;
                let (sum, new_carry) = inverted_byte.overflowing_add(carry);
                inverted.push(sum);
                carry = if new_carry { 1 } else { 0 };
            }

            inverted.reverse();
            SignedBigInt {
                bytes: inverted,
                negative: true,
            }
        } else {
            SignedBigInt {
                bytes: bytes.to_vec(),
                negative: false,
            }
        }
    }
}

/// A primitive block from ASN.1.
#[derive(Clone, Debug)]
pub enum ASN1Block {
    Boolean(usize, bool),
    Integer(usize, SignedBigInt),
    BitString(usize, usize, Vec<u8>),
    OctetString(usize, Vec<u8>),
    Null(usize),
    ObjectIdentifier(usize, OID),
    UTF8String(usize, String),
    PrintableString(usize, String),
    TeletexString(usize, String),
    IA5String(usize, String),
    UTCTime(usize, Vec<u8>),         // Store as raw bytes for now
    GeneralizedTime(usize, Vec<u8>), // Store as raw bytes for now
    UniversalString(usize, String),
    BMPString(usize, String),
    Sequence(usize, Vec<ASN1Block>),
    Set(usize, Vec<ASN1Block>),
    /// An explicitly tagged block.
    Explicit(ASN1Class, usize, U256, alloc::boxed::Box<ASN1Block>),
    /// An unknown block.
    Unknown(ASN1Class, bool, usize, U256, Vec<u8>),
}

impl ASN1Block {
    /// Get the class associated with the given ASN1Block
    pub fn class(&self) -> ASN1Class {
        match *self {
            ASN1Block::Explicit(c, _, _, _) => c,
            ASN1Block::Unknown(c, _, _, _, _) => c,
            _ => ASN1Class::Universal,
        }
    }

    /// Get the starting offset associated with the given ASN1Block
    pub fn offset(&self) -> usize {
        match *self {
            ASN1Block::Boolean(o, _) => o,
            ASN1Block::Integer(o, _) => o,
            ASN1Block::BitString(o, _, _) => o,
            ASN1Block::OctetString(o, _) => o,
            ASN1Block::Null(o) => o,
            ASN1Block::ObjectIdentifier(o, _) => o,
            ASN1Block::UTF8String(o, _) => o,
            ASN1Block::PrintableString(o, _) => o,
            ASN1Block::TeletexString(o, _) => o,
            ASN1Block::IA5String(o, _) => o,
            ASN1Block::UTCTime(o, _) => o,
            ASN1Block::GeneralizedTime(o, _) => o,
            ASN1Block::UniversalString(o, _) => o,
            ASN1Block::BMPString(o, _) => o,
            ASN1Block::Sequence(o, _) => o,
            ASN1Block::Set(o, _) => o,
            ASN1Block::Explicit(_, o, _, _) => o,
            ASN1Block::Unknown(_, _, o, _, _) => o,
        }
    }
}

impl PartialEq for ASN1Block {
    fn eq(&self, other: &ASN1Block) -> bool {
        match (self, other) {
            (ASN1Block::Boolean(_, a1), ASN1Block::Boolean(_, a2)) => a1 == a2,
            (ASN1Block::Integer(_, a1), ASN1Block::Integer(_, a2)) => a1 == a2,
            (ASN1Block::BitString(_, a1, b1), ASN1Block::BitString(_, a2, b2)) => {
                (a1 == a2) && (b1 == b2)
            }
            (ASN1Block::OctetString(_, a1), ASN1Block::OctetString(_, a2)) => a1 == a2,
            (ASN1Block::Null(_), ASN1Block::Null(_)) => true,
            (ASN1Block::ObjectIdentifier(_, a1), ASN1Block::ObjectIdentifier(_, a2)) => a1 == a2,
            (ASN1Block::Sequence(_, a1), ASN1Block::Sequence(_, a2)) => a1 == a2,
            (ASN1Block::Set(_, a1), ASN1Block::Set(_, a2)) => a1 == a2,
            _ => false,
        }
    }
}

/// An error that can arise decoding ASN.1 primitive blocks.
#[derive(Clone, Debug, PartialEq)]
pub enum ASN1DecodeErr {
    EmptyBuffer,
    BadBooleanLength(usize),
    LengthTooLarge(usize),
    UTF8DecodeFailure,
    PrintableStringDecodeFailure,
    InvalidDateValue(String),
    InvalidBitStringLength(isize),
    InvalidClass(u8),
    Incomplete,
    Overflow,
}

/// Translate a binary blob into a series of `ASN1Block`s
pub fn from_der(i: &[u8]) -> Result<Vec<ASN1Block>, ASN1DecodeErr> {
    from_der_(i, 0)
}

fn from_der_(i: &[u8], start_offset: usize) -> Result<Vec<ASN1Block>, ASN1DecodeErr> {
    let mut result: Vec<ASN1Block> = Vec::new();
    let mut index: usize = 0;
    let len = i.len();

    while index < len {
        let soff = start_offset + index;
        let (tag, constructed, class) = decode_tag(i, &mut index)?;
        let len = decode_length(i, &mut index)?;
        let checklen = index
            .checked_add(len)
            .ok_or(ASN1DecodeErr::LengthTooLarge(len))?;
        if checklen > i.len() {
            return Err(ASN1DecodeErr::Incomplete);
        }
        let body = &i[index..(index + len)];

        if class != ASN1Class::Universal {
            if constructed {
                // Try to read as explicitly tagged
                if let Ok(mut items) = from_der_(body, start_offset + index) {
                    if items.len() == 1 {
                        result.push(ASN1Block::Explicit(
                            class,
                            soff,
                            tag,
                            alloc::boxed::Box::new(items.remove(0)),
                        ));
                        index += len;
                        continue;
                    }
                }
            }
            result.push(ASN1Block::Unknown(
                class,
                constructed,
                soff,
                tag,
                body.to_vec(),
            ));
            index += len;
            continue;
        }

        // Universal class - check if tag fits in u8
        let tag_u8 = if tag.is_zero().into() {
            Some(0u8)
        } else {
            // Try to convert to u8
            let bytes = tag.to_le_bytes();
            if bytes[1..].iter().all(|&b| b == 0) {
                Some(bytes[0])
            } else {
                None
            }
        };

        match tag_u8 {
            // BOOLEAN
            Some(0x01) => {
                if len != 1 {
                    return Err(ASN1DecodeErr::BadBooleanLength(len));
                }
                result.push(ASN1Block::Boolean(soff, body[0] != 0));
            }
            // INTEGER
            Some(0x02) => {
                let res = SignedBigInt::from_signed_bytes_be(body);
                result.push(ASN1Block::Integer(soff, res));
            }
            // BIT STRING
            Some(0x03) if body.is_empty() => result.push(ASN1Block::BitString(soff, 0, Vec::new())),
            Some(0x03) => {
                let bits = body[1..].to_vec();
                let bitcount = bits.len() * 8;
                let rest = body[0] as usize;
                if bitcount < rest {
                    return Err(ASN1DecodeErr::InvalidBitStringLength(
                        bitcount as isize - rest as isize,
                    ));
                }
                let nbits = bitcount - (body[0] as usize);
                result.push(ASN1Block::BitString(soff, nbits, bits))
            }
            // OCTET STRING
            Some(0x04) => result.push(ASN1Block::OctetString(soff, body.to_vec())),
            // NULL
            Some(0x05) => {
                result.push(ASN1Block::Null(soff));
            }
            // OBJECT IDENTIFIER
            Some(0x06) => {
                let mut value1 = U256::ZERO;
                if body.is_empty() {
                    return Err(ASN1DecodeErr::Incomplete);
                }
                let mut value2 = U256::from_u8(body[0]);
                let mut oidres = Vec::new();
                let mut bindex = 1;

                if body[0] >= 40 {
                    if body[0] < 80 {
                        value1 = U256::ONE;
                        value2 = value2.wrapping_sub(&U256::from_u8(40));
                    } else {
                        value1 = U256::from_u8(2);
                        value2 = value2.wrapping_sub(&U256::from_u8(80));
                    }
                }

                oidres.push(value1);
                oidres.push(value2);
                while bindex < body.len() {
                    oidres.push(decode_base127(body, &mut bindex)?);
                }
                let res = OID(oidres);

                result.push(ASN1Block::ObjectIdentifier(soff, res))
            }
            // UTF8STRING
            Some(0x0C) => match core::str::from_utf8(body) {
                Ok(v) => result.push(ASN1Block::UTF8String(soff, String::from(v))),
                Err(_) => return Err(ASN1DecodeErr::UTF8DecodeFailure),
            },
            // SEQUENCE
            Some(0x10) => match from_der_(body, start_offset + index) {
                Ok(items) => result.push(ASN1Block::Sequence(soff, items)),
                Err(e) => return Err(e),
            },
            // SET
            Some(0x11) => match from_der_(body, start_offset + index) {
                Ok(items) => result.push(ASN1Block::Set(soff, items)),
                Err(e) => return Err(e),
            },
            // PRINTABLE STRING
            Some(0x13) => {
                const PRINTABLE_CHARS: &str =
                    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789'()+,-./:=? ";
                let mut res = String::new();
                let val = body.iter().map(|x| *x as char);

                for c in val {
                    if PRINTABLE_CHARS.contains(c) {
                        res.push(c);
                    } else {
                        return Err(ASN1DecodeErr::PrintableStringDecodeFailure);
                    }
                }
                result.push(ASN1Block::PrintableString(soff, res));
            }
            // TELETEX STRINGS
            Some(0x14) => match core::str::from_utf8(body) {
                Ok(v) => result.push(ASN1Block::TeletexString(soff, String::from(v))),
                Err(_) => return Err(ASN1DecodeErr::UTF8DecodeFailure),
            },
            // IA5 (ASCII) STRING
            Some(0x16) => {
                let val = body.iter().map(|x| *x as char);
                let res = String::from_iter(val);
                result.push(ASN1Block::IA5String(soff, res))
            }
            // UTCTime - just store raw bytes for now
            Some(0x17) => {
                result.push(ASN1Block::UTCTime(soff, body.to_vec()));
            }
            // GeneralizedTime - just store raw bytes for now
            Some(0x18) => {
                result.push(ASN1Block::GeneralizedTime(soff, body.to_vec()));
            }
            // UNIVERSAL STRINGS
            Some(0x1C) => match core::str::from_utf8(body) {
                Ok(v) => result.push(ASN1Block::UniversalString(soff, String::from(v))),
                Err(_) => return Err(ASN1DecodeErr::UTF8DecodeFailure),
            },
            // BMP STRINGS
            Some(0x1E) => match core::str::from_utf8(body) {
                Ok(v) => result.push(ASN1Block::BMPString(soff, String::from(v))),
                Err(_) => return Err(ASN1DecodeErr::UTF8DecodeFailure),
            },
            // Unknown
            _ => {
                result.push(ASN1Block::Unknown(
                    class,
                    constructed,
                    soff,
                    tag,
                    body.to_vec(),
                ));
            }
        }
        index += len;
    }

    if result.is_empty() {
        Err(ASN1DecodeErr::EmptyBuffer)
    } else {
        Ok(result)
    }
}

/// Returns the tag, if the type is constructed and the class.
fn decode_tag(i: &[u8], index: &mut usize) -> Result<(U256, bool, ASN1Class), ASN1DecodeErr> {
    if *index >= i.len() {
        return Err(ASN1DecodeErr::Incomplete);
    }
    let tagbyte = i[*index];
    let constructed = (tagbyte & 0b0010_0000) != 0;
    let class = decode_class(tagbyte)?;
    let basetag = tagbyte & 0b1_1111;

    *index += 1;

    if basetag == 0b1_1111 {
        let res = decode_base127(i, index)?;
        Ok((res, constructed, class))
    } else {
        Ok((U256::from_u8(basetag), constructed, class))
    }
}

fn decode_base127(i: &[u8], index: &mut usize) -> Result<U256, ASN1DecodeErr> {
    let mut res = U256::ZERO;

    loop {
        if *index >= i.len() {
            return Err(ASN1DecodeErr::Incomplete);
        }

        let nextbyte = i[*index];
        *index += 1;

        // Shift left by 7 bits and add the lower 7 bits of nextbyte
        res = res.shl_vartime(7);
        res = res.wrapping_add(&U256::from_u8(nextbyte & 0x7f));

        if (nextbyte & 0x80) == 0 {
            return Ok(res);
        }
    }
}

fn decode_class(i: u8) -> Result<ASN1Class, ASN1DecodeErr> {
    match i >> 6 {
        0b00 => Ok(ASN1Class::Universal),
        0b01 => Ok(ASN1Class::Application),
        0b10 => Ok(ASN1Class::ContextSpecific),
        0b11 => Ok(ASN1Class::Private),
        _ => Err(ASN1DecodeErr::InvalidClass(i)),
    }
}

fn decode_length(i: &[u8], index: &mut usize) -> Result<usize, ASN1DecodeErr> {
    if *index >= i.len() {
        return Err(ASN1DecodeErr::Incomplete);
    }
    let startbyte = i[*index];

    *index += 1;
    if startbyte >= 0x80 {
        let mut lenlen = (startbyte & 0x7f) as usize;
        let mut res = 0;

        if lenlen > core::mem::size_of::<usize>() {
            return Err(ASN1DecodeErr::LengthTooLarge(lenlen));
        }

        while lenlen > 0 {
            if *index >= i.len() {
                return Err(ASN1DecodeErr::Incomplete);
            }

            res = (res << 8) + (i[*index] as usize);

            *index += 1;
            lenlen -= 1;
        }

        Ok(res)
    } else {
        Ok(startbyte as usize)
    }
}
