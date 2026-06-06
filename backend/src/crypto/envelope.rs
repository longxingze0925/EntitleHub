use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};
use ring::{
    aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM},
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const KEY_VERSION: &str = "v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivateKeyEnvelope {
    pub ciphertext: String,
    pub nonce: String,
    pub tag: String,
    pub key_version: String,
    pub alg: String,
}

pub fn encrypt_private_key(
    master_key: &[u8; 32],
    plaintext: &[u8],
) -> Result<PrivateKeyEnvelope, AppError> {
    encrypt_bytes(master_key, plaintext)
}

pub fn encrypt_bytes(
    master_key: &[u8; 32],
    plaintext: &[u8],
) -> Result<PrivateKeyEnvelope, AppError> {
    let random = SystemRandom::new();
    let mut nonce_bytes = [0_u8; NONCE_LEN];
    random
        .fill(&mut nonce_bytes)
        .map_err(|_| AppError::crypto("failed to generate private key encryption nonce"))?;

    let key = encryption_key(master_key)?;
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);
    let mut in_out = plaintext.to_vec();
    key.seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
        .map_err(|_| AppError::crypto("failed to encrypt private key"))?;

    let tag = in_out.split_off(in_out.len() - TAG_LEN);

    Ok(PrivateKeyEnvelope {
        ciphertext: URL_SAFE_NO_PAD.encode(in_out),
        nonce: URL_SAFE_NO_PAD.encode(nonce_bytes),
        tag: URL_SAFE_NO_PAD.encode(tag),
        key_version: KEY_VERSION.to_owned(),
        alg: "AES-256-GCM".to_owned(),
    })
}

pub fn decrypt_private_key(
    master_key: &[u8; 32],
    envelope: &PrivateKeyEnvelope,
) -> Result<Vec<u8>, AppError> {
    decrypt_bytes(master_key, envelope)
}

pub fn decrypt_bytes(
    master_key: &[u8; 32],
    envelope: &PrivateKeyEnvelope,
) -> Result<Vec<u8>, AppError> {
    if envelope.key_version != KEY_VERSION {
        return Err(AppError::crypto("unsupported private key envelope version"));
    }
    if envelope.alg != "AES-256-GCM" {
        return Err(AppError::crypto("unsupported private key envelope alg"));
    }

    let nonce_bytes = decode_base64_text(&envelope.nonce)
        .map_err(|_| AppError::crypto("private key envelope nonce invalid"))?;
    let nonce_bytes: [u8; NONCE_LEN] = nonce_bytes
        .try_into()
        .map_err(|_| AppError::crypto("private key envelope nonce invalid"))?;
    let mut in_out = decode_base64_text(&envelope.ciphertext)
        .map_err(|_| AppError::crypto("private key envelope ciphertext invalid"))?;
    let tag = decode_base64_text(&envelope.tag)
        .map_err(|_| AppError::crypto("private key envelope tag invalid"))?;
    if tag.len() != TAG_LEN {
        return Err(AppError::crypto("private key envelope tag invalid"));
    }
    in_out.extend_from_slice(&tag);

    let key = encryption_key(master_key)?;
    let plaintext = key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce_bytes),
            Aad::empty(),
            &mut in_out,
        )
        .map_err(|_| AppError::crypto("failed to decrypt private key"))?;

    Ok(plaintext.to_vec())
}

fn encryption_key(master_key: &[u8; 32]) -> Result<LessSafeKey, AppError> {
    let unbound = UnboundKey::new(&AES_256_GCM, master_key)
        .map_err(|_| AppError::crypto("invalid master key"))?;

    Ok(LessSafeKey::new(unbound))
}

fn decode_base64_text(value: &str) -> Result<Vec<u8>, base64::DecodeError> {
    URL_SAFE_NO_PAD
        .decode(value.trim())
        .or_else(|_| STANDARD.decode(value.trim()))
}

#[cfg(test)]
mod tests {
    use super::{decrypt_private_key, encrypt_private_key};

    #[test]
    fn envelope_contains_required_fields_without_plaintext() {
        let envelope =
            encrypt_private_key(&[3_u8; 32], b"private key bytes").expect("encrypt envelope");

        assert!(!envelope.ciphertext.is_empty());
        assert!(!envelope.nonce.is_empty());
        assert!(!envelope.tag.is_empty());
        assert_eq!(envelope.key_version, "v1");
        assert_eq!(envelope.alg, "AES-256-GCM");
        assert_ne!(envelope.ciphertext, "private key bytes");
    }

    #[test]
    fn encrypted_private_key_round_trips() {
        let envelope =
            encrypt_private_key(&[3_u8; 32], b"private key bytes").expect("encrypt envelope");
        let plaintext = decrypt_private_key(&[3_u8; 32], &envelope).expect("decrypt private key");

        assert_eq!(plaintext, b"private key bytes");
    }
}
