#![allow(dead_code)]

use crate::SignatureAlgorithm;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use alloc::format;
use simple_asn1_nostd::{ASN1Block, ASN1Class, from_der, oid};
use pdf_logger::debug_log;
use crypto_bigint::Zero;

pub struct VerifierParams {
    pub modulus: Option<Vec<u8>>,
    pub exponent: Option<Vec<u8>>,
    pub signature: Vec<u8>,
    pub signed_attrs_message_digest: Option<Vec<u8>>,
    pub actual_message_digest: Option<Vec<u8>>,
    pub sig_algorithm: SignatureAlgorithm,
    pub digest_algorithm: Option<Vec<u64>>,
    pub signed_attrs_der: Option<Vec<u8>>,
}

pub fn parse_signed_data(der_bytes: &[u8]) -> Result<VerifierParams, String> {
    debug_log!("parse_signed_data: DER length={}", der_bytes.len());
    
    let blocks = from_der(der_bytes).map_err(|e| format!("DER parse error: {:?}", e))?;
    
    let content_info = extract_content_info(&blocks)?;
    let signed_children = extract_signed_children(content_info)?;
    let signature_data = get_signature_data(signed_children.clone())?;
    
    let (modulus_bytes, exponent_bytes) = 
        extract_pubkey_components(&signed_children, &signature_data.signer_serial)?;
    
    Ok(VerifierParams {
        modulus: Some(modulus_bytes),
        exponent: Some(exponent_bytes),
        signature: signature_data.signature,
        signed_attrs_message_digest: Some(signature_data.expected_message_digest),
        actual_message_digest: None,
        sig_algorithm: signature_data.signed_algo,
        digest_algorithm: signature_data.digest_oid_vec,
        signed_attrs_der: Some(signature_data.signed_attrs_der),
    })
}

struct SignatureData {
    signature: Vec<u8>,
    signer_serial: Vec<u8>,
    signed_attrs_der: Vec<u8>,
    signed_algo: SignatureAlgorithm,
    expected_message_digest: Vec<u8>,
    digest_oid_vec: Option<Vec<u64>>,
}

fn get_signature_data(signed_data_seq: Vec<ASN1Block>) -> Result<SignatureData, String> {
    let signer_info_items = extract_signer_info(&signed_data_seq)?;
    let (signer_serial, digest_oid) = extract_issuer_and_digest_algorithm(&signer_info_items)?;
    let signed_attrs_der = extract_signed_attributes_der(&signer_info_items)?;
    let signed_algo = compute_signed_algorithm(&digest_oid)?;
    let signed_attrs = 
        from_der(&signed_attrs_der).map_err(|e| format!("signedAttrs parse error: {:?}", e))?;
    let expected_message_digest = extract_message_digest(&signed_attrs)
        .map_err(|e| format!("Failed to get messageDigest: {}", e))?;
    let signature = extract_signature(&signer_info_items)?;
    
    Ok(SignatureData {
        signature,
        signer_serial,
        signed_attrs_der,
        signed_algo,
        expected_message_digest,
        digest_oid_vec: Some(digest_oid.as_vec()),
    })
}

fn extract_signer_info(signed_data_seq: &Vec<ASN1Block>) -> Result<&Vec<ASN1Block>, String> {
    match signed_data_seq.last() {
        Some(ASN1Block::Set(_, items)) => match items.first() {
            Some(ASN1Block::Sequence(_, signer_info)) => Ok(signer_info),
            _ => Err("Expected SignerInfo SEQUENCE in SignerInfo SET".into()),
        },
        _ => Err("Expected SignerInfo SET in SignedData".into()),
    }
}

fn extract_issuer_and_digest_algorithm(
    signer_info: &Vec<ASN1Block>,
) -> Result<(Vec<u8>, simple_asn1_nostd::OID), String> {
    // issuerAndSerialNumber ::= SEQUENCE { issuer Name, serialNumber INTEGER }
    let signer_serial = match &signer_info[1] {
        ASN1Block::Sequence(_, parts) if parts.len() == 2 => {
            match &parts[1] {
                ASN1Block::Integer(_, signed_int) => {
                    signed_int.bytes.clone()
                }
                other => {
                    return Err(format!("Expected serialNumber INTEGER, got {:?}", 
                        match other {
                            ASN1Block::Sequence(_, _) => "SEQUENCE",
                            ASN1Block::Set(_, _) => "SET",
                            _ => "OTHER"
                        }).into())
                }
            }
        }
        other => {
            return Err(format!("Expected issuerAndSerialNumber SEQUENCE, got {:?}",
                match other {
                    ASN1Block::Sequence(_, _) => "SEQUENCE",
                    ASN1Block::Set(_, _) => "SET",
                    _ => "OTHER"
                }).into())
        }
    };
    
    let digest_oid = if let ASN1Block::Sequence(_, items) = &signer_info[2] {
        if let ASN1Block::ObjectIdentifier(_, oid) = &items[0] {
            oid.clone()
        } else {
            return Err("Invalid digestAlgorithm in SignerInfo".into());
        }
    } else {
        return Err("Digest algorithm missing".into());
    };
    
    Ok((signer_serial, digest_oid))
}

fn extract_signed_attributes_der(signer_info: &Vec<ASN1Block>) -> Result<Vec<u8>, String> {
    for block in signer_info {
        if let ASN1Block::Unknown(ASN1Class::ContextSpecific, true, _offset, tag_no, content) = block {
            if tag_no.is_zero().into() {
                // Build universal SET tag + length
                let mut out = Vec::with_capacity(content.len() + 4);
                out.push(0x31); // SET
                
                let len = content.len();
                if len < 128 {
                    out.push(len as u8);
                } else if len <= 0xFF {
                    out.push(0x81);
                    out.push(len as u8);
                } else {
                    out.push(0x82);
                    out.push((len >> 8) as u8);
                    out.push((len & 0xFF) as u8);
                }
                
                out.extend_from_slice(content);
                return Ok(out);
            }
        }
    }
    Err("signedAttrs [0] not found".into())
}

fn compute_signed_algorithm(
    digest_oid: &simple_asn1_nostd::OID,
) -> Result<SignatureAlgorithm, String> {
    let oid_vec = digest_oid.as_vec();
    match oid_vec.as_slice() {
        [2, 16, 840, 1, 101, 3, 4, 2, 1] => Ok(SignatureAlgorithm::Sha256WithRsaEncryption),
        [2, 16, 840, 1, 101, 3, 4, 2, 2] => Ok(SignatureAlgorithm::Sha384WithRsaEncryption),
        [2, 16, 840, 1, 101, 3, 4, 2, 3] => Ok(SignatureAlgorithm::Sha512WithRsaEncryption),
        [1, 3, 14, 3, 2, 26] => Ok(SignatureAlgorithm::Sha1WithRsaEncryption),
        _ => Err("Unsupported digest OID".into()),
    }
}

fn extract_signature(
    signer_info: &Vec<ASN1Block>,
) -> Result<Vec<u8>, String> {
    // Signature is typically the last element or after signed attributes
    for (i, block) in signer_info.iter().enumerate() {
        if let ASN1Block::OctetString(_, s) = block {
            // Make sure this is after digestAlgorithm and signedAttrs
            if i >= 4 {
                return Ok(s.clone());
            }
        }
    }
    Err("EncryptedDigest (signature) not found".into())
}

fn extract_content_info(blocks: &[ASN1Block]) -> Result<&[ASN1Block], String> {
    if let Some(ASN1Block::Sequence(_, children)) = blocks.get(0) {
        if let ASN1Block::ObjectIdentifier(_, oid_val) = &children[0] {
            let expected = oid!(1, 2, 840, 113549, 1, 7, 2);
            if oid_val == &expected {
                Ok(children)
            } else {
                Err("Not a SignedData contentType".into())
            }
        } else {
            Err("Missing contentType OID".into())
        }
    } else {
        Err("Top-level not a SEQUENCE".into())
    }
}

pub fn extract_signed_children(children: &[ASN1Block]) -> Result<Vec<ASN1Block>, String> {
    let block = children
        .get(1)
        .ok_or_else(|| String::from("Missing SignedData content"))?;
    
    match block {
        ASN1Block::Explicit(ASN1Class::ContextSpecific, _, _, inner) => {
            if let ASN1Block::Sequence(_, seq_children) = inner.as_ref() {
                Ok(seq_children.clone())
            } else {
                Err("Explicit SignedData not a SEQUENCE".into())
            }
        }
        ASN1Block::Unknown(ASN1Class::ContextSpecific, _, _, _, data) => {
            let parsed = 
                from_der(&data).map_err(|e| format!("Inner SignedData parse error: {:?}", e))?;
            if let ASN1Block::Sequence(_, seq_children) = &parsed[0] {
                Ok(seq_children.clone())
            } else {
                Err("Inner SignedData not a SEQUENCE".into())
            }
        }
        ASN1Block::Sequence(_, seq_children) => Ok(seq_children.clone()),
        other => Err(format!("Unexpected SignedData format: {:?}",
            match other {
                ASN1Block::Integer(_, _) => "INTEGER",
                ASN1Block::Set(_, _) => "SET",
                _ => "OTHER"
            })),
    }
}

pub fn extract_pubkey_components(
    signed_data_seq: &Vec<ASN1Block>,
    signed_serial_number: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let certificates = find_certificates(signed_data_seq)?;
    let tbs_fields = get_correct_tbs(&certificates, signed_serial_number)
        .map_err(|e| format!("Failed to get correct tbsCertificate: {}", e))?;
    let spki_fields = find_subject_public_key_info(&tbs_fields)?;
    let public_key_bitstring = extract_public_key_bitstring(&spki_fields)?;
    let rsa_sequence = parse_rsa_public_key(&public_key_bitstring)?;
    let modulus = extract_modulus(&rsa_sequence)?;
    let exponent = extract_exponent(&rsa_sequence)?;
    
    Ok((modulus, exponent))
}

fn find_certificates(signed_data_seq: &Vec<ASN1Block>) -> Result<Vec<ASN1Block>, String> {
    let certs_block = signed_data_seq.iter().find(|block| match block {
        ASN1Block::Explicit(ASN1Class::ContextSpecific, _, tag, _) => {
            tag.is_zero().into()
        }
        ASN1Block::Unknown(ASN1Class::ContextSpecific, _, _, tag, _) => {
            tag.is_zero().into()
        }
        _ => false,
    });
    
    match certs_block {
        Some(cert_block) => match cert_block {
            ASN1Block::Unknown(ASN1Class::ContextSpecific, _, _, tag, data)
                if tag.is_zero().into() =>
            {
                let parsed_inner = 
                    from_der(data).map_err(|e| format!("Cert wrapper parse error: {:?}", e))?;
                match parsed_inner.as_slice() {
                    [ASN1Block::Set(_, items)] => Ok(items.clone()),
                    [ASN1Block::Sequence(_, items)] => Ok(items.clone()),
                    seqs if seqs.iter().all(|b| matches!(b, ASN1Block::Sequence(_, _))) => {
                        Ok(seqs.to_vec())
                    }
                    other => Err(format!(
                        "Unexpected structure inside implicit certificate block: {} items",
                        other.len()
                    )
                    .into()),
                }
            }
            ASN1Block::Explicit(ASN1Class::ContextSpecific, _, tag, inner)
                if tag.is_zero().into() =>
            {
                match inner.as_ref() {
                    ASN1Block::Set(_, certs) => Ok(certs.clone()),
                    ASN1Block::Sequence(tag, fields) => {
                        Ok(vec![ASN1Block::Sequence(*tag, fields.clone())])
                    }
                    other => Err(format!(
                        "Expected SET or SEQUENCE inside Explicit certificate block, got {:?}",
                        match other {
                            ASN1Block::Integer(_, _) => "INTEGER",
                            ASN1Block::Set(_, _) => "SET",
                            _ => "OTHER"
                        }
                    )
                    .into()),
                }
            }
            ASN1Block::Set(_, items)
                if items.iter().all(|i| matches!(i, ASN1Block::Sequence(_, _))) =>
            {
                Ok(items.clone())
            }
            other => Err(format!("Unexpected certificates block type: {:?}",
                match other {
                    ASN1Block::Sequence(_, _) => "SEQUENCE",
                    ASN1Block::Set(_, _) => "SET",
                    _ => "OTHER"
                }).into()),
        },
        None => Ok(Vec::new()),
    }
}

fn get_correct_tbs(
    certificates: &Vec<ASN1Block>,
    signed_serial_number: &[u8],
) -> Result<Vec<ASN1Block>, String> {
    for certificate in certificates {
        let cert_fields = if let ASN1Block::Sequence(_, fields) = certificate {
            fields
        } else {
            return Err("Certificate not a SEQUENCE".into());
        };
        
        let tbs_fields = match &cert_fields[0] {
            ASN1Block::Explicit(ASN1Class::ContextSpecific, _, _, _) => cert_fields.clone(),
            ASN1Block::Sequence(_, seq) => seq.clone(),
            _ => return Err("tbsCertificate not found".into()),
        };
        
        // Check version tag (optional)
        let serial_idx = if matches!(&tbs_fields[0], 
            ASN1Block::Explicit(ASN1Class::ContextSpecific, _, tag, _) if tag.is_zero().into()) {
            1
        } else {
            0
        };
        
        let serial_number = if let ASN1Block::Integer(_, signed_int) = &tbs_fields[serial_idx] {
            &signed_int.bytes
        } else {
            return Err("Serial number not found".into());
        };
        
        debug_log!("Checking cert serial {:02x?} against signer serial {:02x?}", 
            serial_number, signed_serial_number);
        
        // Check if the serial number matches the one we are looking for
        if serial_number == signed_serial_number {
            return Ok(tbs_fields);
        }
    }
    Err("No matching certificate found".into())
}

fn find_subject_public_key_info(tbs_fields: &Vec<ASN1Block>) -> Result<&Vec<ASN1Block>, String> {
    tbs_fields
        .iter()
        .find_map(|b| {
            if let ASN1Block::Sequence(_, sf) = b {
                if let ASN1Block::Sequence(_, alg) = &sf[0] {
                    if let Some(ASN1Block::ObjectIdentifier(_, o)) = alg.get(0) {
                        let rsa_oid = oid!(1, 2, 840, 113549, 1, 1, 1);
                        if o == &rsa_oid {
                            return Some(sf);
                        }
                    }
                }
            }
            None
        })
        .ok_or_else(|| String::from("subjectPublicKeyInfo not found"))
}

fn extract_public_key_bitstring(spki_fields: &Vec<ASN1Block>) -> Result<Vec<u8>, String> {
    if let ASN1Block::BitString(_, _, d) = &spki_fields[1] {
        Ok(d.clone())
    } else {
        Err("Expected BIT STRING for public key".into())
    }
}

fn parse_rsa_public_key(bitstring: &[u8]) -> Result<Vec<ASN1Block>, String> {
    let rsa_blocks = from_der(bitstring).map_err(|e| format!("RSAPublicKey parse error: {:?}", e))?;
    if let ASN1Block::Sequence(_, items) = &rsa_blocks[0] {
        Ok(items.clone())
    } else {
        Err("RSAPublicKey not a SEQUENCE".into())
    }
}

fn extract_exponent(rsa_sequence: &Vec<ASN1Block>) -> Result<Vec<u8>, String> {
    if let ASN1Block::Integer(_, signed_int) = &rsa_sequence[1] {
        Ok(signed_int.bytes.clone())
    } else {
        Err("Exponent not found".into())
    }
}

fn extract_modulus(rsa_sequence: &Vec<ASN1Block>) -> Result<Vec<u8>, String> {
    if let ASN1Block::Integer(_, signed_int) = &rsa_sequence[0] {
        // Skip leading zero if present
        let bytes = &signed_int.bytes;
        if bytes.len() > 1 && bytes[0] == 0 {
            Ok(bytes[1..].to_vec())
        } else {
            Ok(bytes.clone())
        }
    } else {
        Err("Modulus not found".into())
    }
}

/// find and return the messageDigest OCTET STRING bytes.
fn extract_message_digest(attrs: &[ASN1Block]) -> Result<Vec<u8>, String> {
    let candidates: &[ASN1Block] = if attrs.len() == 1 {
        if let ASN1Block::Set(_, inner) = &attrs[0] {
            inner.as_slice()
        } else {
            attrs
        }
    } else {
        attrs
    };
    
    for attr in candidates {
        if let ASN1Block::Sequence(_, items) = attr {
            if let ASN1Block::ObjectIdentifier(_, oid) = &items[0] {
                let msg_digest_oid = oid!(1, 2, 840, 113549, 1, 9, 4);
                if oid == &msg_digest_oid {
                    if let ASN1Block::Set(_, inner_vals) = &items[1] {
                        if let ASN1Block::OctetString(_, data) = &inner_vals[0] {
                            return Ok(data.clone());
                        } else {
                            return Err("messageDigest value not an OctetString".into());
                        }
                    } else {
                        return Err("messageDigest missing inner Set".into());
                    }
                }
            }
        }
    }
    Err("messageDigest attribute (OID 1.2.840.113549.1.9.4) not found".into())
}