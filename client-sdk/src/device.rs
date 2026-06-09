use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    signing::{generate_device_keypair, validate_ed25519_public_key},
    SdkError, SdkResult,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceIdentity {
    pub machine_id: String,
    pub device_public_key: String,
    pub private_key_pkcs8_base64: String,
}

impl DeviceIdentity {
    pub fn generate(app_key: &str, fingerprint_parts: &[&str]) -> SdkResult<Self> {
        let machine_id = machine_id_from_fingerprint_parts(app_key, fingerprint_parts)?;
        let keypair = generate_device_keypair()?;

        Ok(Self {
            machine_id,
            device_public_key: keypair.public_key_pem,
            private_key_pkcs8_base64: URL_SAFE_NO_PAD.encode(keypair.private_key_pkcs8_der),
        })
    }

    pub fn from_stored(
        machine_id: &str,
        device_public_key: &str,
        private_key_pkcs8_base64: &str,
    ) -> SdkResult<Self> {
        let machine_id = normalize_machine_id(machine_id)?;
        let device_public_key = device_public_key.trim();
        if device_public_key.is_empty() {
            return Err(SdkError::InvalidPublicKey);
        }
        validate_ed25519_public_key(device_public_key)?;
        let private_key_pkcs8_base64 = private_key_pkcs8_base64.trim();
        if private_key_pkcs8_base64.is_empty() {
            return Err(SdkError::InvalidPrivateKey);
        }
        URL_SAFE_NO_PAD
            .decode(private_key_pkcs8_base64)
            .map_err(|_| SdkError::InvalidPrivateKey)?;

        Ok(Self {
            machine_id,
            device_public_key: device_public_key.to_owned(),
            private_key_pkcs8_base64: private_key_pkcs8_base64.to_owned(),
        })
    }

    pub fn private_key_pkcs8_der(&self) -> SdkResult<Vec<u8>> {
        URL_SAFE_NO_PAD
            .decode(self.private_key_pkcs8_base64.trim())
            .map_err(|_| SdkError::InvalidPrivateKey)
    }

    pub fn rotate_key(&self) -> SdkResult<Self> {
        let machine_id = normalize_machine_id(&self.machine_id)?;
        let keypair = generate_device_keypair()?;

        Ok(Self {
            machine_id,
            device_public_key: keypair.public_key_pem,
            private_key_pkcs8_base64: URL_SAFE_NO_PAD.encode(keypair.private_key_pkcs8_der),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RotateDeviceKeyRequestPayload {
    pub device_public_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RotateDeviceKeyResponse {
    pub device_key_id: String,
    pub device_public_key: String,
    pub algorithm: String,
    pub status: String,
    pub rotated_device_key_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DeviceSummary {
    pub id: String,
    pub app_id: String,
    #[serde(default)]
    pub customer_id: Option<String>,
    #[serde(default)]
    pub license_id: Option<String>,
    #[serde(default)]
    pub subscription_id: Option<String>,
    pub machine_id: String,
    #[serde(default)]
    pub device_name: Option<String>,
    #[serde(default)]
    pub os: Option<String>,
    #[serde(default)]
    pub app_version: Option<String>,
    pub status: String,
    pub first_seen_at: String,
    #[serde(default)]
    pub last_seen_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SelfUnbindDeviceResponse {
    pub device: DeviceSummary,
    pub revoked_sessions: u64,
}

impl RotateDeviceKeyResponse {
    pub fn from_json(json: &str) -> SdkResult<Self> {
        let response: Self = serde_json::from_str(json).map_err(|_| SdkError::InvalidSession)?;
        response.validate()?;

        Ok(response)
    }

    pub fn from_api_response_json(json: &str) -> SdkResult<Self> {
        let response: Self = crate::response::parse_api_response_data(json)?.data;
        response.validate()?;

        Ok(response)
    }

    pub fn validate(&self) -> SdkResult<()> {
        if self.device_key_id.trim().is_empty()
            || self.algorithm.trim() != "Ed25519"
            || self.status.trim() != "active"
            || self
                .rotated_device_key_ids
                .iter()
                .any(|key_id| key_id.trim().is_empty())
        {
            return Err(SdkError::InvalidSession);
        }
        validate_ed25519_public_key(&self.device_public_key)
            .map_err(|_| SdkError::InvalidSession)?;

        Ok(())
    }
}

impl SelfUnbindDeviceResponse {
    pub fn from_json(json: &str) -> SdkResult<Self> {
        let response: Self = serde_json::from_str(json).map_err(|_| SdkError::InvalidSession)?;
        response.validate()?;

        Ok(response)
    }

    pub fn from_api_response_json(json: &str) -> SdkResult<Self> {
        let response: Self = crate::response::parse_api_response_data(json)?.data;
        response.validate()?;

        Ok(response)
    }

    fn validate(&self) -> SdkResult<()> {
        if self.device.id.trim().is_empty()
            || self.device.app_id.trim().is_empty()
            || self.device.machine_id.trim().is_empty()
            || self.device.status.trim().is_empty()
            || self.device.first_seen_at.trim().is_empty()
            || self.device.created_at.trim().is_empty()
            || self.device.updated_at.trim().is_empty()
        {
            return Err(SdkError::InvalidSession);
        }

        Ok(())
    }
}

pub fn build_rotate_device_key_request(
    device: &DeviceIdentity,
) -> SdkResult<RotateDeviceKeyRequestPayload> {
    if device.device_public_key.trim().is_empty() {
        return Err(SdkError::InvalidPublicKey);
    }
    validate_ed25519_public_key(&device.device_public_key)?;
    device.private_key_pkcs8_der()?;

    Ok(RotateDeviceKeyRequestPayload {
        device_public_key: device.device_public_key.clone(),
    })
}

pub fn machine_id_from_fingerprint_parts(app_key: &str, parts: &[&str]) -> SdkResult<String> {
    let app_key = app_key.trim();
    if app_key.is_empty() {
        return Err(SdkError::InvalidMachineId);
    }

    let mut normalized = parts
        .iter()
        .filter_map(|part| normalize_fingerprint_part(part))
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    if normalized.is_empty() {
        return Err(SdkError::InvalidMachineId);
    }

    let payload = format!("{app_key}\n{}", normalized.join("\n"));

    Ok(URL_SAFE_NO_PAD.encode(Sha256::digest(payload.as_bytes())))
}

pub fn normalize_machine_id(machine_id: &str) -> SdkResult<String> {
    let machine_id = machine_id.trim();
    if machine_id.is_empty() {
        return Err(SdkError::InvalidMachineId);
    }

    Ok(machine_id.to_owned())
}

fn normalize_fingerprint_part(value: &str) -> Option<String> {
    let value = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if value.is_empty() {
        return None;
    }

    Some(value)
}

#[cfg(test)]
mod tests {
    use crate::signing::{
        device_body_sha256, device_signature_message, sign_device_request,
        verify_ed25519_signature, DeviceSignatureInput,
    };

    use super::{
        build_rotate_device_key_request, machine_id_from_fingerprint_parts, normalize_machine_id,
        DeviceIdentity, RotateDeviceKeyResponse, SelfUnbindDeviceResponse,
    };

    #[test]
    fn machine_id_hash_is_stable_across_order_case_and_spacing() {
        let first = machine_id_from_fingerprint_parts(
            "app_key",
            &[" CPU  12th Gen ", "Disk-001", "MainBoard-A"],
        )
        .expect("machine id should build");
        let second = machine_id_from_fingerprint_parts(
            "app_key",
            &["mainboard-a", "disk-001", "cpu 12TH gen", "disk-001"],
        )
        .expect("machine id should build");

        assert_eq!(first, second);
        assert_eq!(first.len(), 43);
        assert!(first
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_')));
    }

    #[test]
    fn machine_id_hash_is_scoped_by_app_key() {
        let first = machine_id_from_fingerprint_parts("app_a", &["machine"]).expect("machine id");
        let second = machine_id_from_fingerprint_parts("app_b", &["machine"]).expect("machine id");

        assert_ne!(first, second);
    }

    #[test]
    fn machine_id_rejects_blank_inputs() {
        assert!(machine_id_from_fingerprint_parts("", &["machine"]).is_err());
        assert!(machine_id_from_fingerprint_parts("app", &[" ", "\n"]).is_err());
        assert!(normalize_machine_id(" ").is_err());
        assert_eq!(
            normalize_machine_id(" machine ").expect("machine id should normalize"),
            "machine"
        );
    }

    #[test]
    fn device_identity_generates_uploadable_key_and_private_key_text() {
        let identity =
            DeviceIdentity::generate("app_key", &["machine"]).expect("identity should generate");
        let private_key = identity
            .private_key_pkcs8_der()
            .expect("private key should decode");
        let body = br#"{"ok":true}"#;
        let headers = sign_device_request(DeviceSignatureInput {
            method: "post",
            path: "/api/client/auth/verify",
            body,
            timestamp: 1_717_171_717,
            nonce: "0123456789abcdef",
            device_id: "device-id",
            device_key_id: "device-key-id",
            session_id: "session-id",
            private_key_pkcs8_der: &private_key,
        })
        .expect("request should sign");
        let message = device_signature_message(
            "post",
            "/api/client/auth/verify",
            &device_body_sha256(body),
            1_717_171_717,
            "0123456789abcdef",
            "device-id",
            "device-key-id",
            "session-id",
        );

        assert!(identity
            .device_public_key
            .starts_with("-----BEGIN PUBLIC KEY-----"));
        verify_ed25519_signature(
            &identity.device_public_key,
            message.as_bytes(),
            &headers.signature,
        )
        .expect("device public key should verify request signature");
    }

    #[test]
    fn device_identity_round_trips_stored_shape() {
        let identity =
            DeviceIdentity::generate("app_key", &["machine"]).expect("identity should generate");
        let json = serde_json::to_string(&identity).expect("identity should serialize");
        let decoded: DeviceIdentity =
            serde_json::from_str(&json).expect("identity should deserialize");
        let restored = DeviceIdentity::from_stored(
            &decoded.machine_id,
            &decoded.device_public_key,
            &decoded.private_key_pkcs8_base64,
        )
        .expect("stored identity should validate");

        assert_eq!(identity, restored);
    }

    #[test]
    fn device_identity_rejects_invalid_stored_private_key() {
        assert!(DeviceIdentity::from_stored("machine", "public-key", "bad private key").is_err());
    }

    #[test]
    fn rotate_key_preserves_machine_id_and_replaces_key_material() {
        let identity =
            DeviceIdentity::generate("app_key", &["machine"]).expect("identity should generate");
        let rotated = identity.rotate_key().expect("rotated identity");

        assert_eq!(rotated.machine_id, identity.machine_id);
        assert_ne!(rotated.device_public_key, identity.device_public_key);
        assert_ne!(
            rotated.private_key_pkcs8_base64,
            identity.private_key_pkcs8_base64
        );
        rotated
            .private_key_pkcs8_der()
            .expect("rotated private key should decode");
    }

    #[test]
    fn rotate_device_key_request_uses_new_public_key() {
        let identity =
            DeviceIdentity::generate("app_key", &["machine"]).expect("identity should generate");
        let rotated = identity.rotate_key().expect("rotated identity");
        let payload =
            build_rotate_device_key_request(&rotated).expect("rotation payload should build");

        assert_eq!(payload.device_public_key, rotated.device_public_key);
    }

    #[test]
    fn rotate_device_key_response_validates_backend_payload() {
        let identity =
            DeviceIdentity::generate("app_key", &["machine"]).expect("identity should generate");
        let json = serde_json::json!({
            "device_key_id": "00000000-0000-0000-0000-000000000002",
            "device_public_key": identity.device_public_key,
            "algorithm": "Ed25519",
            "status": "active",
            "rotated_device_key_ids": ["00000000-0000-0000-0000-000000000001"]
        })
        .to_string();

        let response = RotateDeviceKeyResponse::from_json(&json).expect("response should parse");

        assert_eq!(response.status, "active");
        assert_eq!(response.rotated_device_key_ids.len(), 1);
        assert!(RotateDeviceKeyResponse::from_json(
            r#"{
              "device_key_id": "",
              "device_public_key": "public-key",
              "algorithm": "Ed25519",
              "status": "active",
              "rotated_device_key_ids": []
            }"#,
        )
        .is_err());
    }

    #[test]
    fn rotate_device_key_response_parses_api_response_wrapper() {
        let identity =
            DeviceIdentity::generate("app_key", &["machine"]).expect("identity should generate");
        let json = serde_json::json!({
            "code": 0,
            "message": "ok",
            "data": {
                "device_key_id": "00000000-0000-0000-0000-000000000002",
                "device_public_key": identity.device_public_key,
                "algorithm": "Ed25519",
                "status": "active",
                "rotated_device_key_ids": ["00000000-0000-0000-0000-000000000001"]
            },
            "request_id": "req_1"
        })
        .to_string();

        let response = RotateDeviceKeyResponse::from_api_response_json(&json)
            .expect("api response should parse");

        assert_eq!(
            response.device_key_id,
            "00000000-0000-0000-0000-000000000002"
        );
    }

    #[test]
    fn self_unbind_device_response_parses_api_response_wrapper() {
        let response = SelfUnbindDeviceResponse::from_api_response_json(
            r#"{
              "code": 0,
              "message": "ok",
              "data": {
                "device": {
                  "id": "00000000-0000-0000-0000-000000000001",
                  "app_id": "00000000-0000-0000-0000-000000000002",
                  "customer_id": null,
                  "license_id": "00000000-0000-0000-0000-000000000003",
                  "subscription_id": null,
                  "machine_id": "machine",
                  "device_name": "Workstation",
                  "os": "Windows",
                  "app_version": "1.0.0",
                  "status": "unbound",
                  "first_seen_at": "2026-01-01T00:00:00Z",
                  "last_seen_at": "2026-01-02T00:00:00Z",
                  "created_at": "2026-01-01T00:00:00Z",
                  "updated_at": "2026-01-02T00:00:00Z"
                },
                "revoked_sessions": 1
              },
              "request_id": "req_1"
            }"#,
        )
        .expect("unbind response should parse");

        assert_eq!(response.device.status, "unbound");
        assert_eq!(response.revoked_sessions, 1);
        assert!(SelfUnbindDeviceResponse::from_json(r#"{"revoked_sessions":1}"#).is_err());
    }
}
