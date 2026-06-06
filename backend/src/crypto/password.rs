use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};

use crate::error::AppError;

pub fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);

    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| AppError::crypto(format!("password hash failed: {error}")))
}

pub fn verify_password(password: &str, password_hash: &str) -> Result<bool, AppError> {
    let parsed_hash = PasswordHash::new(password_hash)
        .map_err(|error| AppError::crypto(format!("password hash parse failed: {error}")))?;

    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::{hash_password, verify_password};

    #[test]
    fn password_hash_uses_argon2id_and_verifies() {
        let hash = hash_password("Password@123456").expect("hash password");

        assert!(hash.starts_with("$argon2id$"));
        assert!(verify_password("Password@123456", &hash).expect("verify password"));
        assert!(!verify_password("Wrong@123456", &hash).expect("verify wrong password"));
    }

    #[test]
    fn invalid_password_hash_is_error() {
        assert!(verify_password("Password@123456", "not-a-password-hash").is_err());
    }
}
