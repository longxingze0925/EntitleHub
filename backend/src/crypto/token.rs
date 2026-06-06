use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use sha2::Sha256;
use subtle::ConstantTimeEq;

use crate::error::AppError;

type HmacSha256 = Hmac<Sha256>;

pub fn generate_token() -> String {
    random_base64_url(32)
}

pub fn generate_recovery_code() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

    let mut code = String::with_capacity(19);
    for index in 0..16 {
        if index > 0 && index % 4 == 0 {
            code.push('-');
        }

        let char_index = (OsRng.next_u32() as usize) % CHARSET.len();
        code.push(CHARSET[char_index] as char);
    }

    code
}

pub fn hash_token(pepper: &str, token: &str) -> Result<String, AppError> {
    let mut mac = HmacSha256::new_from_slice(pepper.as_bytes())
        .map_err(|error| AppError::crypto(format!("token hash key invalid: {error}")))?;

    mac.update(token.as_bytes());

    Ok(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
}

pub fn verify_token_hash(pepper: &str, token: &str, expected_hash: &str) -> Result<bool, AppError> {
    let actual_hash = hash_token(pepper, token)?;

    Ok(actual_hash
        .as_bytes()
        .ct_eq(expected_hash.as_bytes())
        .into())
}

fn random_base64_url(byte_len: usize) -> String {
    let mut bytes = vec![0_u8; byte_len];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::{generate_recovery_code, generate_token, hash_token, verify_token_hash};

    #[test]
    fn generated_token_is_high_entropy_text() {
        let token = generate_token();

        assert!(token.len() >= 40);
        assert!(!token.contains('='));
    }

    #[test]
    fn recovery_code_is_grouped_for_display() {
        let code = generate_recovery_code();
        let parts = code.split('-').collect::<Vec<_>>();

        assert_eq!(code.len(), 19);
        assert_eq!(parts.len(), 4);
        assert!(parts.iter().all(|part| part.len() == 4));
    }

    #[test]
    fn token_hash_uses_pepper_and_verifies() {
        let pepper = "test-pepper-with-enough-length";
        let token = "plain-token";
        let hash = hash_token(pepper, token).expect("hash token");

        assert_ne!(hash, token);
        assert!(verify_token_hash(pepper, token, &hash).expect("verify token"));
        assert!(!verify_token_hash(pepper, "other-token", &hash).expect("reject token"));
    }
}
