use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};
use ring::{
    rand::{SecureRandom, SystemRandom},
    signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519},
};
use sha2::{Digest, Sha256};

use crate::{SdkError, SdkResult};

const NONCE_MIN_LEN: usize = 16;
const NONCE_MAX_LEN: usize = 128;
const NONCE_RANDOM_BYTES: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceKeypair {
    pub public_key_pem: String,
    pub public_key_base64: String,
    pub private_key_pkcs8_der: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct DeviceSignatureInput<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub body: &'a [u8],
    pub timestamp: i64,
    pub nonce: &'a str,
    pub device_id: &'a str,
    pub device_key_id: &'a str,
    pub session_id: &'a str,
    pub private_key_pkcs8_der: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceSignatureHeaders {
    pub device_id: String,
    pub device_key_id: String,
    pub timestamp: String,
    pub nonce: String,
    pub body_sha256: String,
    pub signature: String,
}

pub fn generate_device_keypair() -> SdkResult<DeviceKeypair> {
    let random = SystemRandom::new();
    let private_key =
        Ed25519KeyPair::generate_pkcs8(&random).map_err(|_| SdkError::InvalidPrivateKey)?;
    let key_pair = Ed25519KeyPair::from_pkcs8(private_key.as_ref())
        .map_err(|_| SdkError::InvalidPrivateKey)?;
    let raw_public_key = key_pair.public_key().as_ref();

    Ok(DeviceKeypair {
        public_key_pem: ed25519_public_key_pem(raw_public_key),
        public_key_base64: URL_SAFE_NO_PAD.encode(raw_public_key),
        private_key_pkcs8_der: private_key.as_ref().to_vec(),
    })
}

pub fn generate_device_nonce() -> SdkResult<String> {
    let random = SystemRandom::new();
    let mut bytes = [0_u8; NONCE_RANDOM_BYTES];
    random
        .fill(&mut bytes)
        .map_err(|_| SdkError::InvalidNonce)?;

    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub fn sign_device_request(input: DeviceSignatureInput<'_>) -> SdkResult<DeviceSignatureHeaders> {
    validate_device_nonce(input.nonce)?;
    let body_sha256 = device_body_sha256(input.body);
    let message = device_signature_message(
        input.method,
        input.path,
        &body_sha256,
        input.timestamp,
        input.nonce,
        input.device_id,
        input.device_key_id,
        input.session_id,
    );
    let key_pair = Ed25519KeyPair::from_pkcs8(input.private_key_pkcs8_der)
        .map_err(|_| SdkError::InvalidPrivateKey)?;
    let signature = URL_SAFE_NO_PAD.encode(key_pair.sign(message.as_bytes()).as_ref());

    Ok(DeviceSignatureHeaders {
        device_id: input.device_id.to_owned(),
        device_key_id: input.device_key_id.to_owned(),
        timestamp: input.timestamp.to_string(),
        nonce: input.nonce.to_owned(),
        body_sha256,
        signature,
    })
}

pub fn device_body_sha256(body: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(body))
}

pub fn device_signature_message(
    method: &str,
    path: &str,
    body_sha256: &str,
    timestamp: i64,
    nonce: &str,
    device_id: &str,
    device_key_id: &str,
    session_id: &str,
) -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        method.to_uppercase(),
        path,
        body_sha256,
        timestamp,
        nonce,
        device_id,
        device_key_id,
        session_id
    )
}

pub fn validate_device_nonce(nonce: &str) -> SdkResult<()> {
    if nonce.len() < NONCE_MIN_LEN
        || nonce.len() > NONCE_MAX_LEN
        || !nonce
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(SdkError::InvalidNonce);
    }

    Ok(())
}

pub fn verify_ed25519_signature(
    public_key: &str,
    message: &[u8],
    signature: &str,
) -> SdkResult<()> {
    let public_key = parse_ed25519_public_key(public_key)?;
    let signature = decode_base64_text(signature).map_err(|_| SdkError::InvalidSignature)?;
    let public_key = UnparsedPublicKey::new(&ED25519, public_key);

    public_key
        .verify(message, &signature)
        .map_err(|_| SdkError::InvalidSignature)
}

pub fn validate_ed25519_public_key(public_key: &str) -> SdkResult<()> {
    parse_ed25519_public_key(public_key).map(|_| ())
}

fn parse_ed25519_public_key(public_key: &str) -> SdkResult<Vec<u8>> {
    let public_key = public_key.trim();
    if public_key.starts_with("-----BEGIN PUBLIC KEY-----") {
        let encoded = public_key
            .lines()
            .filter(|line| !line.starts_with("-----"))
            .collect::<String>();
        let der = STANDARD
            .decode(encoded)
            .map_err(|_| SdkError::InvalidPublicKey)?;

        return der
            .strip_prefix(ed25519_spki_prefix())
            .map(|raw| raw.to_vec())
            .ok_or(SdkError::InvalidPublicKey);
    }

    let raw = decode_base64_text(public_key).map_err(|_| SdkError::InvalidPublicKey)?;
    if raw.len() != 32 {
        return Err(SdkError::InvalidPublicKey);
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

fn ed25519_public_key_pem(raw_public_key: &[u8]) -> String {
    let mut der = ed25519_spki_prefix().to_vec();
    der.extend_from_slice(raw_public_key);
    let encoded = STANDARD.encode(der);

    format!("-----BEGIN PUBLIC KEY-----\n{encoded}\n-----END PUBLIC KEY-----")
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;
    use ring::{
        rand::SystemRandom,
        signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519},
    };

    use super::{
        device_body_sha256, device_signature_message, generate_device_keypair,
        generate_device_nonce, sign_device_request, validate_device_nonce,
        verify_ed25519_signature, DeviceSignatureInput,
    };

    #[test]
    fn generated_device_nonce_is_url_safe_and_random_sized() {
        let nonce = generate_device_nonce().expect("nonce should generate");
        let other = generate_device_nonce().expect("nonce should generate");

        assert_ne!(nonce, other);
        assert!(validate_device_nonce(&nonce).is_ok());
        assert_eq!(nonce.len(), 32);
    }

    #[test]
    fn generated_device_keypair_signs_requests_with_uploadable_public_keys() {
        let keypair = generate_device_keypair().expect("device keypair should generate");
        let body = br#"{"ok":true}"#;
        let headers = sign_device_request(DeviceSignatureInput {
            method: "post",
            path: "/api/client/auth/verify",
            body,
            timestamp: 1_717_171_717,
            nonce: "0123456789abcdef",
            device_id: "00000000-0000-0000-0000-000000000001",
            device_key_id: "00000000-0000-0000-0000-000000000002",
            session_id: "00000000-0000-0000-0000-000000000003",
            private_key_pkcs8_der: &keypair.private_key_pkcs8_der,
        })
        .expect("sign request");
        let message = device_signature_message(
            "post",
            "/api/client/auth/verify",
            &headers.body_sha256,
            1_717_171_717,
            "0123456789abcdef",
            "00000000-0000-0000-0000-000000000001",
            "00000000-0000-0000-0000-000000000002",
            "00000000-0000-0000-0000-000000000003",
        );

        verify_ed25519_signature(
            &keypair.public_key_pem,
            message.as_bytes(),
            &headers.signature,
        )
        .expect("pem public key should verify");
        verify_ed25519_signature(
            &keypair.public_key_base64,
            message.as_bytes(),
            &headers.signature,
        )
        .expect("base64 public key should verify");
    }

    #[test]
    fn sign_device_request_builds_backend_compatible_headers() {
        let random = SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&random).expect("generate key");
        let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).expect("read key");
        let body = br#"{"ok":true}"#;
        let nonce = "0123456789abcdef";
        let headers = sign_device_request(DeviceSignatureInput {
            method: "post",
            path: "/api/client/auth/verify",
            body,
            timestamp: 1_717_171_717,
            nonce,
            device_id: "00000000-0000-0000-0000-000000000001",
            device_key_id: "00000000-0000-0000-0000-000000000002",
            session_id: "00000000-0000-0000-0000-000000000003",
            private_key_pkcs8_der: pkcs8.as_ref(),
        })
        .expect("sign request");

        assert_eq!(headers.body_sha256, device_body_sha256(body));
        assert_eq!(headers.nonce, nonce);
        let message = device_signature_message(
            "post",
            "/api/client/auth/verify",
            &headers.body_sha256,
            1_717_171_717,
            nonce,
            "00000000-0000-0000-0000-000000000001",
            "00000000-0000-0000-0000-000000000002",
            "00000000-0000-0000-0000-000000000003",
        );
        let public_key = UnparsedPublicKey::new(&ED25519, key_pair.public_key().as_ref());
        let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(headers.signature)
            .expect("signature base64");
        public_key
            .verify(message.as_bytes(), &signature)
            .expect("signature should verify");
    }

    #[test]
    fn device_nonce_rejects_short_long_or_unsafe_values() {
        assert!(validate_device_nonce("0123456789abcdef").is_ok());
        assert!(validate_device_nonce("short").is_err());
        assert!(validate_device_nonce("bad:nonce:value").is_err());
        assert!(validate_device_nonce(&"a".repeat(129)).is_err());
    }
}
