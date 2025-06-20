//! RSA signature verification using RustCrypto/RSA v0.10.0-rc.0

use alloc::vec::Vec;
use crypto_bigint::BoxedUint;
use pdf_logger::debug_log;
use rsa::{traits::SignatureScheme, Pkcs1v15Sign, RsaPublicKey};
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha384, Sha512};

// DigestInfo prefixes for different hash algorithms (from RFC 3447)
const SHA1_PREFIX: &[u8] = &[
    0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04, 0x14,
];

const SHA256_PREFIX: &[u8] = &[
    0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05,
    0x00, 0x04, 0x20,
];

const SHA384_PREFIX: &[u8] = &[
    0x30, 0x41, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02, 0x05,
    0x00, 0x04, 0x30,
];

const SHA512_PREFIX: &[u8] = &[
    0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03, 0x05,
    0x00, 0x04, 0x40,
];

#[derive(Debug, Clone, Copy)]
pub enum HashAlgorithm {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl HashAlgorithm {
    pub fn hash(&self, data: &[u8]) -> Vec<u8> {
        match self {
            HashAlgorithm::Sha1 => {
                let mut hasher = Sha1::new();
                hasher.update(data);
                hasher.finalize().to_vec()
            }
            HashAlgorithm::Sha256 => {
                let mut hasher = Sha256::new();
                hasher.update(data);
                hasher.finalize().to_vec()
            }
            HashAlgorithm::Sha384 => {
                let mut hasher = Sha384::new();
                hasher.update(data);
                hasher.finalize().to_vec()
            }
            HashAlgorithm::Sha512 => {
                let mut hasher = Sha512::new();
                hasher.update(data);
                hasher.finalize().to_vec()
            }
        }
    }
}

pub struct PublicKey {
    inner: RsaPublicKey,
}

impl PublicKey {
    pub fn from_components(n: &[u8], e: &[u8]) -> Result<Self, alloc::string::String> {
        use alloc::string::String;
        use pdf_logger::debug_log;

        debug_log!(
            "RSA from_components: modulus len={}, exponent len={}",
            n.len(),
            e.len()
        );
        debug_log!("First few modulus bytes: {:02x?}", &n[..n.len().min(8)]);
        debug_log!("Exponent bytes: {:02x?}", e);

        // Determine the bit size we need
        let n_bits = n.len() * 8;
        let _max_size = n_bits.div_ceil(8); // Round up to nearest byte

        debug_log!("Calculated n_bits={}, max_size={}", n_bits, _max_size);

        // Create BoxedUint from the modulus and exponent
        // Note: from_be_slice expects bits_precision, not bytes
        let n_boxed = BoxedUint::from_be_slice(n, n_bits as u32).map_err(|_e| {
            debug_log!("Failed to create modulus BoxedUint: {:?}", _e);
            String::from("Failed to create modulus")
        })?;

        // For the exponent, use the modulus bit size to match RSA key requirements
        let e_boxed = BoxedUint::from_be_slice(e, n_bits as u32).map_err(|_e| {
            debug_log!("Failed to create exponent BoxedUint: {:?}", _e);
            String::from("Failed to create exponent")
        })?;

        // Create the RSA public key
        // Note: new() uses default max size of 4096 bits which is sufficient for most RSA keys
        let inner = RsaPublicKey::new(n_boxed, e_boxed).map_err(|_e| {
            debug_log!("Failed to create RSA public key: {:?}", _e);
            String::from("Failed to create RSA public key")
        })?;

        Ok(Self { inner })
    }

    pub fn verify_pkcs1v15(
        &self,
        hashed: &[u8],
        sig: &[u8],
        hash_alg: HashAlgorithm,
    ) -> Result<bool, alloc::string::String> {
        use alloc::vec;

        // Add DigestInfo prefix to the hash
        let prefix = match hash_alg {
            HashAlgorithm::Sha1 => SHA1_PREFIX,
            HashAlgorithm::Sha256 => SHA256_PREFIX,
            HashAlgorithm::Sha384 => SHA384_PREFIX,
            HashAlgorithm::Sha512 => SHA512_PREFIX,
        };

        let mut digest_info = vec![0u8; prefix.len() + hashed.len()];
        digest_info[..prefix.len()].copy_from_slice(prefix);
        digest_info[prefix.len()..].copy_from_slice(hashed);

        debug_log!(
            "DigestInfo length: {}, prefix: {} bytes, hash: {} bytes",
            digest_info.len(),
            prefix.len(),
            hashed.len()
        );

        // Use unprefixed verification with the complete DigestInfo
        let scheme = Pkcs1v15Sign::new_unprefixed();

        // Verify the signature
        match scheme.verify(&self.inner, &digest_info, sig) {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}
