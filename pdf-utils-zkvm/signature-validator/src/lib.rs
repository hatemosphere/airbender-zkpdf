#![no_std]
#![allow(clippy::new_without_default)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use sha1::Sha1;
use sha2::{Sha256, Sha384, Sha512};

pub mod logger;
pub mod pkcs7_reference;
pub mod rsa_rustcrypto;
pub mod signed_bytes_extractor;

// Use logging macro
use pdf_logger::debug_log;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SignatureAlgorithm {
    Sha1WithRsaEncryption,
    Sha256WithRsaEncryption,
    Sha384WithRsaEncryption,
    Sha512WithRsaEncryption,
}

impl fmt::Display for SignatureAlgorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SignatureAlgorithm::Sha1WithRsaEncryption => write!(f, "SHA1 with RSA Encryption"),
            SignatureAlgorithm::Sha256WithRsaEncryption => write!(f, "SHA256 with RSA Encryption"),
            SignatureAlgorithm::Sha384WithRsaEncryption => write!(f, "SHA384 with RSA Encryption"),
            SignatureAlgorithm::Sha512WithRsaEncryption => write!(f, "SHA512 with RSA Encryption"),
        }
    }
}

pub fn verify_pdf_signature(pdf_bytes: &[u8]) -> Result<bool, String> {
    // First extract the signature DER and signed data from the PDF
    let (signature_der, signed_data) = signed_bytes_extractor::get_signature_der(pdf_bytes)?;

    // Parse the PKCS#7 signed data first to get the digest algorithm
    // Parse the PKCS#7 structure using reference implementation
    let mut verifier_params = pkcs7_reference::parse_signed_data(&signature_der)?;

    // Calculate hash of the actual signed PDF data using the algorithm from PKCS#7
    let calculated_signed_data_hash =
        calculate_pdf_data_hash(&signed_data, &verifier_params.sig_algorithm)?;

    // Store the calculated hash as the actual message digest
    verifier_params.actual_message_digest = Some(calculated_signed_data_hash.clone());

    // Check if the calculated hash matches the one stored in signedAttrs
    if let Some(stored_digest) = &verifier_params.signed_attrs_message_digest {
        debug_log!("Stored digest: {:02x?}", stored_digest);
        debug_log!("Calculated digest: {:02x?}", &calculated_signed_data_hash);
        if stored_digest != &calculated_signed_data_hash {
            debug_log!("Message digest mismatch!");
            return Ok(false);
        }
        debug_log!("Message digests match!");
    } else {
        return Err(String::from("No message digest found in signedAttrs"));
    }

    let sig_algorithm_and_digest_algorithm_match =
        check_alg_consistency_internal(&verifier_params)?;
    if !sig_algorithm_and_digest_algorithm_match {
        return Ok(false);
    }

    let calculated_digest = calculate_signed_attrs_hash(&verifier_params)?;
    debug_log!("Calculated signed attrs hash: {:02x?}", &calculated_digest);
    debug_log!("Signature bytes: {:02x?}", &verifier_params.signature[..16]); // First 16 bytes

    let rsa_public_key = create_rsa_public_key(&verifier_params)?;
    let hash_alg = get_hash_algorithm(&verifier_params.sig_algorithm);
    let signature_valid = verify_rsa_signature(
        &rsa_public_key,
        &calculated_digest,
        &verifier_params.signature,
        hash_alg,
    )?;

    Ok(signature_valid)
}

#[cfg(test)]
pub fn check_alg_consistency(params: &pkcs7_reference::VerifierParams) -> Result<bool, String> {
    check_alg_consistency_internal(params)
}

fn check_alg_consistency_internal(
    params: &pkcs7_reference::VerifierParams,
) -> Result<bool, String> {
    let digest_alg = params
        .digest_algorithm
        .as_ref()
        .ok_or_else(|| String::from("Digest algorithm not found"))?;

    let expected_alg = match digest_alg.as_slice() {
        [1, 3, 14, 3, 2, 26] => SignatureAlgorithm::Sha1WithRsaEncryption,
        [2, 16, 840, 1, 101, 3, 4, 2, 1] => SignatureAlgorithm::Sha256WithRsaEncryption,
        [2, 16, 840, 1, 101, 3, 4, 2, 2] => SignatureAlgorithm::Sha384WithRsaEncryption,
        [2, 16, 840, 1, 101, 3, 4, 2, 3] => SignatureAlgorithm::Sha512WithRsaEncryption,
        _ => return Err(String::from("Unknown digest algorithm")),
    };

    Ok(params.sig_algorithm == expected_alg)
}

fn calculate_signed_attrs_hash(
    params: &pkcs7_reference::VerifierParams,
) -> Result<Vec<u8>, String> {
    use sha1::Digest;

    let signed_attrs_der = params
        .signed_attrs_der
        .as_ref()
        .ok_or_else(|| String::from("Signed attributes DER not found"))?;

    let hash = match params.sig_algorithm {
        SignatureAlgorithm::Sha1WithRsaEncryption => {
            let mut hasher = Sha1::new();
            hasher.update(signed_attrs_der);
            hasher.finalize().to_vec()
        }
        SignatureAlgorithm::Sha256WithRsaEncryption => {
            let mut hasher = Sha256::new();
            hasher.update(signed_attrs_der);
            hasher.finalize().to_vec()
        }
        SignatureAlgorithm::Sha384WithRsaEncryption => {
            let mut hasher = Sha384::new();
            hasher.update(signed_attrs_der);
            hasher.finalize().to_vec()
        }
        SignatureAlgorithm::Sha512WithRsaEncryption => {
            let mut hasher = Sha512::new();
            hasher.update(signed_attrs_der);
            hasher.finalize().to_vec()
        }
    };

    Ok(hash)
}

fn calculate_pdf_data_hash(
    signed_data: &[u8],
    algorithm: &SignatureAlgorithm,
) -> Result<Vec<u8>, String> {
    use sha1::Digest;

    let hash = match algorithm {
        SignatureAlgorithm::Sha1WithRsaEncryption => {
            let mut hasher = Sha1::new();
            hasher.update(signed_data);
            hasher.finalize().to_vec()
        }
        SignatureAlgorithm::Sha256WithRsaEncryption => {
            let mut hasher = Sha256::new();
            hasher.update(signed_data);
            hasher.finalize().to_vec()
        }
        SignatureAlgorithm::Sha384WithRsaEncryption => {
            let mut hasher = Sha384::new();
            hasher.update(signed_data);
            hasher.finalize().to_vec()
        }
        SignatureAlgorithm::Sha512WithRsaEncryption => {
            let mut hasher = Sha512::new();
            hasher.update(signed_data);
            hasher.finalize().to_vec()
        }
    };

    Ok(hash)
}

fn create_rsa_public_key(
    params: &pkcs7_reference::VerifierParams,
) -> Result<rsa_rustcrypto::PublicKey, String> {
    let modulus = params
        .modulus
        .as_ref()
        .ok_or_else(|| String::from("Modulus not found"))?;
    let exponent = params
        .exponent
        .as_ref()
        .ok_or_else(|| String::from("Exponent not found"))?;

    rsa_rustcrypto::PublicKey::from_components(modulus, exponent)
}

fn get_hash_algorithm(algorithm: &SignatureAlgorithm) -> rsa_rustcrypto::HashAlgorithm {
    match algorithm {
        SignatureAlgorithm::Sha1WithRsaEncryption => rsa_rustcrypto::HashAlgorithm::Sha1,
        SignatureAlgorithm::Sha256WithRsaEncryption => rsa_rustcrypto::HashAlgorithm::Sha256,
        SignatureAlgorithm::Sha384WithRsaEncryption => rsa_rustcrypto::HashAlgorithm::Sha384,
        SignatureAlgorithm::Sha512WithRsaEncryption => rsa_rustcrypto::HashAlgorithm::Sha512,
    }
}

fn verify_rsa_signature(
    public_key: &rsa_rustcrypto::PublicKey,
    message: &[u8],
    signature: &[u8],
    hash_alg: rsa_rustcrypto::HashAlgorithm,
) -> Result<bool, String> {
    debug_log!("Verifying RSA signature:");
    debug_log!("  Message length: {}", message.len());
    debug_log!("  Signature length: {}", signature.len());
    debug_log!("  Hash algorithm: {:?}", hash_alg);

    let result = public_key.verify_pkcs1v15(message, signature, hash_alg)?;

    debug_log!("  Verification result: {}", result);
    Ok(result)
}
