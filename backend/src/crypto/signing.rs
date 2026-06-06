use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519},
};

use crate::{crypto::token::generate_token, error::AppError};

#[derive(Debug, Clone)]
pub struct GeneratedSigningKey {
    pub kid: String,
    pub public_key_pem: String,
    pub private_key_pkcs8_der: Vec<u8>,
}

pub fn generate_ed25519_key() -> Result<GeneratedSigningKey, AppError> {
    let random = SystemRandom::new();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&random)
        .map_err(|_| AppError::crypto("failed to generate Ed25519 keypair"))?;
    let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref())
        .map_err(|_| AppError::crypto("failed to read generated Ed25519 keypair"))?;
    let public_key_pem = pem_block(
        "PUBLIC KEY",
        &ed25519_public_key_der(key_pair.public_key().as_ref()),
    );

    Ok(GeneratedSigningKey {
        kid: format!("kid_{}", generate_token()),
        public_key_pem,
        private_key_pkcs8_der: pkcs8.as_ref().to_vec(),
    })
}

pub fn verify_ed25519_signature(
    public_key: &str,
    message: &[u8],
    signature: &str,
) -> Result<(), AppError> {
    let public_key_bytes = parse_ed25519_public_key(public_key)?;
    let signature_bytes = decode_base64_text(signature)
        .map_err(|_| AppError::invalid_request("signature invalid"))?;
    let public_key = UnparsedPublicKey::new(&ED25519, public_key_bytes);

    public_key
        .verify(message, &signature_bytes)
        .map_err(|_| AppError::invalid_request("signature invalid"))
}

pub fn sign_ed25519(private_key_pkcs8_der: &[u8], message: &[u8]) -> Result<String, AppError> {
    let key_pair = Ed25519KeyPair::from_pkcs8(private_key_pkcs8_der)
        .map_err(|_| AppError::crypto("failed to read Ed25519 signing key"))?;
    let signature = key_pair.sign(message);

    Ok(URL_SAFE_NO_PAD.encode(signature.as_ref()))
}

fn pem_block(label: &str, der: &[u8]) -> String {
    let encoded = STANDARD.encode(der);
    let mut pem = format!("-----BEGIN {label}-----\n");

    for chunk in encoded.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(chunk).expect("base64 is valid utf-8"));
        pem.push('\n');
    }

    pem.push_str(&format!("-----END {label}-----\n"));
    pem
}

fn ed25519_public_key_der(raw_public_key: &[u8]) -> Vec<u8> {
    let mut der = Vec::with_capacity(ed25519_spki_prefix().len() + raw_public_key.len());
    der.extend_from_slice(ed25519_spki_prefix());
    der.extend_from_slice(raw_public_key);
    der
}

pub fn parse_ed25519_public_key(public_key: &str) -> Result<Vec<u8>, AppError> {
    let public_key = public_key.trim();
    if public_key.starts_with("-----BEGIN PUBLIC KEY-----") {
        let encoded = public_key
            .lines()
            .filter(|line| !line.starts_with("-----"))
            .collect::<String>();
        let der = STANDARD
            .decode(encoded)
            .map_err(|_| AppError::invalid_request("device public key invalid"))?;

        return der
            .strip_prefix(ed25519_spki_prefix())
            .map(|raw| raw.to_vec())
            .ok_or_else(|| AppError::invalid_request("device public key invalid"));
    }

    let raw = decode_base64_text(public_key)
        .map_err(|_| AppError::invalid_request("device public key invalid"))?;
    if raw.len() != 32 {
        return Err(AppError::invalid_request("device public key invalid"));
    }

    Ok(raw)
}

fn decode_base64_text(value: &str) -> Result<Vec<u8>, base64::DecodeError> {
    URL_SAFE_NO_PAD
        .decode(value.trim())
        .or_else(|_| STANDARD.decode(value.trim()))
}

fn ed25519_spki_prefix() -> &'static [u8] {
    &[
        0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
    ]
}

#[cfg(test)]
mod tests {
    use super::{generate_ed25519_key, sign_ed25519, verify_ed25519_signature};

    #[test]
    fn generated_key_has_kid_public_key_and_private_der() {
        let key = generate_ed25519_key().expect("generate key");

        assert!(key.kid.starts_with("kid_"));
        assert!(key.public_key_pem.starts_with("-----BEGIN PUBLIC KEY-----"));
        assert!(key.public_key_pem.ends_with("-----END PUBLIC KEY-----\n"));
        assert!(!key.private_key_pkcs8_der.is_empty());
    }

    #[test]
    fn verify_rejects_invalid_signature() {
        let key = generate_ed25519_key().expect("generate key");

        assert!(super::verify_ed25519_signature(&key.public_key_pem, b"message", "bad").is_err());
    }

    #[test]
    fn signed_message_verifies_with_public_key() {
        let key = generate_ed25519_key().expect("generate key");
        let signature = sign_ed25519(&key.private_key_pkcs8_der, b"message").expect("sign message");

        verify_ed25519_signature(&key.public_key_pem, b"message", &signature)
            .expect("signature should verify");
    }
}
