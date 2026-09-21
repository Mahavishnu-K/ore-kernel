use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
use hmac::{Hmac, Mac};
use md5::{Digest, Md5};
use sha2::{Sha256, Sha512};
use subtle::ConstantTimeEq;

type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;
type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

pub struct KernelCrypto;

impl KernelCrypto {
    /// Cryptographically Secure Random Number Generation (CSPRNG)
    pub fn random_bytes(len: usize) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; len];
        getrandom::getrandom(&mut buf).map_err(|e| format!("CSPRNG error: {}", e))?;
        Ok(buf)
    }

    /// Fast, native SHA-256
    pub fn sha256(data: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(data);
        hasher.finalize().into()
    }

    /// Fast, native SHA-512
    pub fn sha512(data: &[u8]) -> [u8; 64] {
        let mut hasher = Sha512::new();
        hasher.update(data);
        hasher.finalize().into()
    }

    /// MD5 hash
    pub fn md5(data: &[u8]) -> [u8; 16] {
        let mut hasher = Md5::new();
        hasher.update(data);
        hasher.finalize().into()
    }

    /// HMAC-SHA256 (Used for AWS SigV4, Stripe, GitHub webhooks)
    pub fn hmac_sha256(key: &[u8], data: &[u8]) -> Result<[u8; 32], String> {
        type HmacSha256 = Hmac<Sha256>;
        let mut mac = <HmacSha256 as Mac>::new_from_slice(key)
            .map_err(|_| "HMAC initialization failed: invalid key length".to_string())?;
        mac.update(data);
        let result = mac.finalize().into_bytes();
        Ok(result.into())
    }

    /// HMAC-SHA512
    pub fn hmac_sha512(key: &[u8], data: &[u8]) -> Result<[u8; 64], String> {
        type HmacSha512 = Hmac<Sha512>;
        let mut mac = <HmacSha512 as Mac>::new_from_slice(key)
            .map_err(|_| "HMAC initialization failed: invalid key length".to_string())?;
        mac.update(data);
        let result = mac.finalize().into_bytes();
        Ok(result.into())
    }

    /// AES-256-GCM Authenticated Encryption (Zero-Knowledge Vaults)
    pub fn aes_256_gcm_encrypt(key: &[u8], iv: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, String> {
        if key.len() != 32 {
            return Err("AES-256 key must be exactly 32 bytes".into());
        }
        if iv.len() != 12 {
            return Err("AES-GCM IV/Nonce must be exactly 12 bytes".into());
        }

        let cipher = Aes256Gcm::new_from_slice(key)
            .map_err(|e| format!("Failed to create cipher: {}", e))?;
        let nonce = Nonce::from_slice(iv);

        cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| format!("AES-GCM encryption error: {}", e))
    }

    /// AES-256-GCM Authenticated Decryption
    pub fn aes_256_gcm_decrypt(
        key: &[u8],
        iv: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, String> {
        if key.len() != 32 {
            return Err("AES-256 key must be exactly 32 bytes".into());
        }
        if iv.len() != 12 {
            return Err("AES-GCM IV/Nonce must be exactly 12 bytes".into());
        }

        let cipher = Aes256Gcm::new_from_slice(key)
            .map_err(|e| format!("Failed to create cipher: {}", e))?;
        let nonce = Nonce::from_slice(iv);

        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| format!("AES-GCM authentication/decryption failure: {}", e))
    }

    /// Hardware-Accelerated AES-CBC Encryption with PKCS#7 Padding
    pub fn aes_cbc_encrypt(key: &[u8], iv: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, String> {
        if iv.len() != 16 {
            return Err("AES-CBC requires an IV of exactly 16 bytes".into());
        }

        let mut buf = vec![0u8; plaintext.len() + 16]; // Buffer with room for padding
        buf[..plaintext.len()].copy_from_slice(plaintext);

        if key.len() == 32 {
            let ct = Aes256CbcEnc::new_from_slices(key, iv)
                .map_err(|e| format!("Cipher init error: {}", e))?
                .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext.len())
                .map_err(|e| format!("Padding error: {}", e))?;
            Ok(ct.to_vec())
        } else if key.len() == 16 {
            let ct = Aes128CbcEnc::new_from_slices(key, iv)
                .map_err(|e| format!("Cipher init error: {}", e))?
                .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext.len())
                .map_err(|e| format!("Padding error: {}", e))?;
            Ok(ct.to_vec())
        } else {
            Err("AES-CBC key must be 16 bytes (AES-128) or 32 bytes (AES-256)".into())
        }
    }

    /// Hardware-Accelerated AES-CBC Decryption with PKCS#7 Unpadding
    pub fn aes_cbc_decrypt(key: &[u8], iv: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, String> {
        if iv.len() != 16 {
            return Err("AES-CBC requires an IV of exactly 16 bytes".into());
        }
        if !ciphertext.len().is_multiple_of(16) {
            return Err("Invalid ciphertext length: must be a multiple of 16".into());
        }

        let mut buf = ciphertext.to_vec();

        if key.len() == 32 {
            let pt = Aes256CbcDec::new_from_slices(key, iv)
                .map_err(|e| format!("Cipher init error: {}", e))?
                .decrypt_padded_mut::<Pkcs7>(&mut buf)
                .map_err(|_| "Invalid PKCS#7 padding: decryption failed".to_string())?;
            Ok(pt.to_vec())
        } else if key.len() == 16 {
            let pt = Aes128CbcDec::new_from_slices(key, iv)
                .map_err(|e| format!("Cipher init error: {}", e))?
                .decrypt_padded_mut::<Pkcs7>(&mut buf)
                .map_err(|_| "Invalid PKCS#7 padding: decryption failed".to_string())?;
            Ok(pt.to_vec())
        } else {
            Err("AES-CBC key must be 16 bytes (AES-128) or 32 bytes (AES-256)".into())
        }
    }

    /// PBKDF2 Key Derivation with HMAC-SHA256
    pub fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32, key_len: usize) -> Vec<u8> {
        let mut key = vec![0u8; key_len];
        pbkdf2::pbkdf2_hmac::<Sha256>(password, salt, iterations, &mut key);
        key
    }

    /// Constant-Time Buffer Equality (Mitigates Side-Channel Timing Attacks)
    pub fn timing_safe_equal(a: &[u8], b: &[u8]) -> bool {
        if a.len() != b.len() {
            return false;
        }
        a.ct_eq(b).into()
    }

    // ORE VFS CRYPTO PORTAL PROTOCOL DISPATCHER
    // Command IDs:
    // 1 = RANDOM_BYTES     [1 byte cmd | 4 bytes len (BE)]
    // 2 = SHA256           [1 byte cmd | payload...]
    // 3 = SHA512           [1 byte cmd | payload...]
    // 4 = MD5              [1 byte cmd | payload...]
    // 5 = HMAC_SHA256      [1 byte cmd | 2 bytes key_len (BE) | key | data]
    // 6 = AES_GCM_ENCRYPT  [1 byte cmd | 32 bytes key | 12 bytes iv | plaintext]
    // 7 = AES_GCM_DECRYPT  [1 byte cmd | 32 bytes key | 12 bytes iv | ciphertext]
    // 8 = TIMING_SAFE_EQ   [1 byte cmd | 4 bytes len (BE) | buf1 | buf2]
    // 9 = PBKDF2_SHA256    [1 byte cmd | 4 bytes iterations (BE) | 4 bytes key_len (BE) | 2 bytes salt_len (BE) | salt | password]
    // 10 = HMAC_SHA512     [1 byte cmd | 2 bytes key_len (BE) | key | data]
    // 11 = AES_CBC_ENCRYPT [1 byte cmd | 1 byte key_len | key | 16 bytes iv | plaintext]
    // 12 = AES_CBC_DECRYPT [1 byte cmd | 1 byte key_len | key | 16 bytes iv | ciphertext]

    pub fn process_portal_request(req: &[u8]) -> Result<Vec<u8>, String> {
        if req.is_empty() {
            return Err("Empty crypto portal request".into());
        }

        let cmd = req[0];
        let payload = &req[1..];

        match cmd {
            // 1: RANDOM_BYTES
            1 => {
                if payload.len() < 4 {
                    return Err("Invalid random_bytes payload".into());
                }
                let len = u32::from_be_bytes(payload[0..4].try_into().unwrap()) as usize;
                Self::random_bytes(len)
            }

            // 2: SHA256
            2 => {
                let digest = Self::sha256(payload);
                Ok(digest.to_vec())
            }

            // 3: SHA512
            3 => {
                let digest = Self::sha512(payload);
                Ok(digest.to_vec())
            }

            // 4: MD5
            4 => {
                let digest = Self::md5(payload);
                Ok(digest.to_vec())
            }

            // 5: HMAC_SHA256
            5 => {
                if payload.len() < 2 {
                    return Err("Invalid HMAC payload".into());
                }
                let key_len = u16::from_be_bytes(payload[0..2].try_into().unwrap()) as usize;
                if payload.len() < 2 + key_len {
                    return Err("HMAC payload too short for specified key".into());
                }
                let key = &payload[2..2 + key_len];
                let data = &payload[2 + key_len..];
                let mac = Self::hmac_sha256(key, data)?;
                Ok(mac.to_vec())
            }

            // 6: AES_GCM_ENCRYPT
            6 => {
                if payload.len() < 32 + 12 {
                    return Err("AES encrypt payload too short (need 32B key + 12B IV)".into());
                }
                let key = &payload[0..32];
                let iv = &payload[32..44];
                let plaintext = &payload[44..];
                Self::aes_256_gcm_encrypt(key, iv, plaintext)
            }

            // 7: AES_GCM_DECRYPT
            7 => {
                if payload.len() < 32 + 12 {
                    return Err("AES decrypt payload too short (need 32B key + 12B IV)".into());
                }
                let key = &payload[0..32];
                let iv = &payload[32..44];
                let ciphertext = &payload[44..];
                Self::aes_256_gcm_decrypt(key, iv, ciphertext)
            }

            // 8: TIMING_SAFE_EQ
            8 => {
                if payload.len() < 4 {
                    return Err("Invalid timingSafeEqual payload".into());
                }
                let len = u32::from_be_bytes(payload[0..4].try_into().unwrap()) as usize;
                if payload.len() != 4 + (len * 2) {
                    return Err("Buffer length mismatch".into());
                }
                let a = &payload[4..4 + len];
                let b = &payload[4 + len..4 + (len * 2)];
                let is_equal = Self::timing_safe_equal(a, b);
                Ok(vec![if is_equal { 1 } else { 0 }])
            }

            // 9: PBKDF2_SHA256 [1B cmd | 4B iterations (BE) | 4B key_len (BE) | 2B salt_len (BE) | salt | password]
            9 => {
                if payload.len() < 10 {
                    return Err("PBKDF2 payload too short".into());
                }
                let iterations = u32::from_be_bytes(payload[0..4].try_into().unwrap());
                let key_len = u32::from_be_bytes(payload[4..8].try_into().unwrap()) as usize;
                let salt_len = u16::from_be_bytes(payload[8..10].try_into().unwrap()) as usize;

                if payload.len() < 10 + salt_len {
                    return Err("PBKDF2 invalid salt offset".into());
                }
                let salt = &payload[10..10 + salt_len];
                let password = &payload[10 + salt_len..];

                Ok(Self::pbkdf2_sha256(password, salt, iterations, key_len))
            }

            // 10: HMAC_SHA512 [1B cmd | 2B key_len (BE) | key | data]
            10 => {
                if payload.len() < 2 {
                    return Err("Invalid HMAC-SHA512 payload".into());
                }
                let key_len = u16::from_be_bytes(payload[0..2].try_into().unwrap()) as usize;
                if payload.len() < 2 + key_len {
                    return Err("HMAC-SHA512 payload too short".into());
                }
                let key = &payload[2..2 + key_len];
                let data = &payload[2 + key_len..];
                let mac = Self::hmac_sha512(key, data)?;
                Ok(mac.to_vec())
            }

            // 11: AES_CBC_ENCRYPT [1B cmd | 1B key_len | key | 16B iv | plaintext]
            11 => {
                if payload.len() < 1 + 16 {
                    return Err("Payload too short for AES-CBC".into());
                }
                let key_len = payload[0] as usize;
                if payload.len() < 1 + key_len + 16 {
                    return Err("Invalid CBC payload size".into());
                }
                let key = &payload[1..1 + key_len];
                let iv = &payload[1 + key_len..1 + key_len + 16];
                let plaintext = &payload[1 + key_len + 16..];
                Self::aes_cbc_encrypt(key, iv, plaintext)
            }

            // 12: AES_CBC_DECRYPT [1B cmd | 1B key_len | key | 16B iv | ciphertext]
            12 => {
                if payload.len() < 1 + 16 {
                    return Err("Payload too short for AES-CBC".into());
                }
                let key_len = payload[0] as usize;
                if payload.len() < 1 + key_len + 16 {
                    return Err("Invalid CBC payload size".into());
                }
                let key = &payload[1..1 + key_len];
                let iv = &payload[1 + key_len..1 + key_len + 16];
                let ciphertext = &payload[1 + key_len + 16..];
                Self::aes_cbc_decrypt(key, iv, ciphertext)
            }

            _ => Err(format!("Unknown crypto command ID: {}", cmd)),
        }
    }
}
