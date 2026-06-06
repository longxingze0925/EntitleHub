use std::{
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    jwks::{require_eddsa_public_key, JwksCache},
    signing::verify_ed25519_signature,
    SdkError, SdkResult,
};

pub use crate::jwks::JwksKey;

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateInfo {
    pub app_id: String,
    pub version: String,
    pub version_code: i64,
    pub download_url: String,
    pub file_size: i64,
    pub sha256: String,
    pub published_at_unix: i64,
    pub signature_kid: String,
    pub signature: String,
    pub signature_alg: String,
    pub force_update: bool,
}

impl UpdateInfo {
    pub fn from_json(json: &str) -> SdkResult<Self> {
        let update: Self =
            serde_json::from_str(json).map_err(|_| SdkError::InvalidUpdateInfo("response"))?;
        validate_update_info(&update)?;

        Ok(update)
    }

    pub fn from_api_response_json(json: &str) -> SdkResult<Self> {
        let update: Self = crate::response::parse_api_response_data(json)?.data;
        validate_update_info(&update)?;

        Ok(update)
    }
}

pub fn verify_downloaded_update(
    update: &UpdateInfo,
    file_path: impl AsRef<Path>,
    jwks: &[JwksKey],
) -> SdkResult<()> {
    validate_update_info(update)?;
    let actual_hash = sha256_hex_file(file_path)?;
    if actual_hash != update.sha256.to_lowercase() {
        return Err(SdkError::HashMismatch);
    }

    verify_update_signature(update, jwks)
}

pub fn verify_downloaded_update_with_jwks_refresh<F>(
    update: &UpdateInfo,
    file_path: impl AsRef<Path>,
    jwks_cache: &mut JwksCache,
    fetch_jwks_json: F,
) -> SdkResult<()>
where
    F: FnMut() -> SdkResult<String>,
{
    validate_update_info(update)?;
    let actual_hash = sha256_hex_file(file_path)?;
    if actual_hash != update.sha256.to_lowercase() {
        return Err(SdkError::HashMismatch);
    }

    verify_update_signature_with_jwks_refresh(update, jwks_cache, fetch_jwks_json)
}

pub fn verify_downloaded_update_for_current_version(
    update: &UpdateInfo,
    current_version_code: i64,
    file_path: impl AsRef<Path>,
    jwks: &[JwksKey],
) -> SdkResult<()> {
    ensure_update_not_downgrade(update, current_version_code)?;
    verify_downloaded_update(update, file_path, jwks)
}

pub fn download_update_from_reader(
    update: &UpdateInfo,
    mut reader: impl Read,
    target_path: impl AsRef<Path>,
    jwks: &[JwksKey],
) -> SdkResult<()> {
    validate_update_info(update)?;
    let target_path = target_path.as_ref();
    let temp_path = temp_update_path(target_path)?;
    let result = (|| -> SdkResult<()> {
        stream_to_file(&mut reader, &temp_path)?;
        verify_downloaded_update(update, &temp_path, jwks)?;
        fs::rename(&temp_path, target_path)?;

        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }

    result
}

pub fn download_update_from_reader_for_current_version(
    update: &UpdateInfo,
    current_version_code: i64,
    reader: impl Read,
    target_path: impl AsRef<Path>,
    jwks: &[JwksKey],
) -> SdkResult<()> {
    ensure_update_not_downgrade(update, current_version_code)?;
    download_update_from_reader(update, reader, target_path, jwks)
}

pub fn verify_update_signature(update: &UpdateInfo, jwks: &[JwksKey]) -> SdkResult<()> {
    validate_update_info(update)?;
    let public_key_pem = require_eddsa_public_key(jwks, &update.signature_kid)?;
    verify_update_signature_with_public_key(update, public_key_pem)
}

pub fn verify_update_signature_with_jwks_refresh<F>(
    update: &UpdateInfo,
    jwks_cache: &mut JwksCache,
    fetch_jwks_json: F,
) -> SdkResult<()>
where
    F: FnMut() -> SdkResult<String>,
{
    validate_update_info(update)?;
    let public_key_pem =
        jwks_cache.require_eddsa_public_key_with_refresh(&update.signature_kid, fetch_jwks_json)?;
    verify_update_signature_with_public_key(update, public_key_pem)
}

fn verify_update_signature_with_public_key(
    update: &UpdateInfo,
    public_key_pem: &str,
) -> SdkResult<()> {
    let payload = release_metadata_signature_payload(
        &update.app_id,
        &update.version,
        update.version_code,
        &update.sha256.to_lowercase(),
        update.file_size,
        update.published_at_unix,
    );
    verify_ed25519_signature(public_key_pem, payload.as_bytes(), &update.signature)
}

pub fn release_file_signature_payload(sha256: &str, file_size: i64) -> String {
    format!("{sha256}:{file_size}")
}

pub fn release_metadata_signature_payload(
    app_id: &str,
    version: &str,
    version_code: i64,
    sha256: &str,
    file_size: i64,
    published_at_unix: i64,
) -> String {
    format!("{app_id}\n{version}\n{version_code}\n{sha256}\n{file_size}\n{published_at_unix}")
}

pub fn ensure_update_not_downgrade(
    update: &UpdateInfo,
    current_version_code: i64,
) -> SdkResult<()> {
    validate_update_info(update)?;
    if current_version_code < 0 {
        return Err(SdkError::InvalidUpdateInfo("current_version_code"));
    }
    if update.version_code < current_version_code {
        return Err(SdkError::DowngradeNotAllowed);
    }

    Ok(())
}

fn validate_update_info(update: &UpdateInfo) -> SdkResult<()> {
    if update.app_id.trim().is_empty() {
        return Err(SdkError::InvalidUpdateInfo("app_id"));
    }
    if update.version.trim().is_empty() {
        return Err(SdkError::InvalidUpdateInfo("version"));
    }
    if update.version_code <= 0 {
        return Err(SdkError::InvalidUpdateInfo("version_code"));
    }
    if update.file_size <= 0 {
        return Err(SdkError::InvalidUpdateInfo("file_size"));
    }
    if update.published_at_unix <= 0 {
        return Err(SdkError::InvalidUpdateInfo("published_at_unix"));
    }
    if update.signature_alg != "Ed25519" {
        return Err(SdkError::UnsupportedSignatureAlg(
            update.signature_alg.clone(),
        ));
    }
    if update.signature.trim().is_empty() {
        return Err(SdkError::InvalidUpdateInfo("signature"));
    }
    if update.signature_kid.trim().is_empty() {
        return Err(SdkError::InvalidUpdateInfo("signature_kid"));
    }
    let sha256 = update.sha256.trim();
    if sha256.len() != 64 || !sha256.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(SdkError::InvalidUpdateInfo("sha256"));
    }

    Ok(())
}

fn sha256_hex_file(path: impl AsRef<Path>) -> SdkResult<String> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn stream_to_file(reader: &mut impl Read, path: &Path) -> SdkResult<()> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        writer.write_all(&buffer[..read])?;
    }
    writer.flush()?;

    Ok(())
}

fn temp_update_path(target_path: &Path) -> SdkResult<PathBuf> {
    let file_name = target_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(SdkError::InvalidUpdateInfo("target_path"))?;
    let temp_file_name = format!(".{file_name}.download");

    Ok(target_path.with_file_name(temp_file_name))
}

#[cfg(test)]
mod tests {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use ring::{
        rand::SystemRandom,
        signature::{Ed25519KeyPair, KeyPair},
    };
    use sha2::{Digest, Sha256};
    use std::{fs, io::Cursor, io::Write};
    use tempfile::{NamedTempFile, TempDir};

    use crate::jwks::JwksCache;

    use super::{
        ensure_update_not_downgrade, release_metadata_signature_payload, verify_downloaded_update,
        verify_downloaded_update_with_jwks_refresh, JwksKey, UpdateInfo,
    };

    #[test]
    fn verify_downloaded_update_accepts_valid_hash_and_signature() {
        let (update, jwks, mut file) = fixture_update(b"release bytes");
        file.flush().expect("flush file");

        verify_downloaded_update(&update, file.path(), &jwks).expect("update should verify");
    }

    #[test]
    fn verify_downloaded_update_rejects_tampered_file() {
        let (update, jwks, file) = fixture_update(b"release bytes");
        fs::write(file.path(), b"tampered").expect("tamper file");

        let error = verify_downloaded_update(&update, file.path(), &jwks)
            .expect_err("tampered file should fail");

        assert!(matches!(error, crate::SdkError::HashMismatch));
    }

    #[test]
    fn verify_downloaded_update_rejects_missing_signature() {
        let (mut update, jwks, file) = fixture_update(b"release bytes");
        update.signature.clear();

        let error = verify_downloaded_update(&update, file.path(), &jwks)
            .expect_err("missing signature should fail");

        assert!(matches!(
            error,
            crate::SdkError::InvalidUpdateInfo("signature")
        ));
    }

    #[test]
    fn verify_downloaded_update_refreshes_missing_jwks_key() {
        let (update, _jwks, file, jwks_json) = fixture_update_with_jwks_json(b"release bytes");
        let mut cache = JwksCache::new();

        verify_downloaded_update_with_jwks_refresh(&update, file.path(), &mut cache, || {
            Ok(jwks_json.clone())
        })
        .expect("update should verify after jwks refresh");

        assert!(cache.find("kid").is_some());
    }

    #[test]
    fn verify_downloaded_update_rejects_tampered_metadata() {
        let (mut update, jwks, file) = fixture_update(b"release bytes");
        update.version_code += 1;

        let error = verify_downloaded_update(&update, file.path(), &jwks)
            .expect_err("tampered metadata should fail");

        assert!(matches!(error, crate::SdkError::InvalidSignature));
    }

    #[test]
    fn download_update_from_reader_writes_verified_target() {
        let bytes = b"release bytes";
        let (update, jwks, _file) = fixture_update(bytes);
        let temp_dir = TempDir::new().expect("temp dir");
        let target_path = temp_dir.path().join("app.zip");

        super::download_update_from_reader(&update, Cursor::new(bytes), &target_path, &jwks)
            .expect("download should verify and move");

        assert_eq!(fs::read(target_path).expect("read target"), bytes);
    }

    #[test]
    fn download_update_from_reader_does_not_overwrite_target_on_hash_failure() {
        let (update, jwks, _file) = fixture_update(b"release bytes");
        let temp_dir = TempDir::new().expect("temp dir");
        let target_path = temp_dir.path().join("app.zip");
        fs::write(&target_path, b"existing").expect("write existing target");

        let error = super::download_update_from_reader(
            &update,
            Cursor::new(b"tampered"),
            &target_path,
            &jwks,
        )
        .expect_err("tampered download should fail");

        assert!(matches!(error, crate::SdkError::HashMismatch));
        assert_eq!(
            fs::read(&target_path).expect("read target"),
            b"existing".to_vec()
        );
        assert!(!target_path.with_file_name(".app.zip.download").exists());
    }

    #[test]
    fn update_downgrade_guard_rejects_older_version_code() {
        let (mut update, _jwks, _file) = fixture_update(b"release bytes");
        update.version_code = 99;

        let error =
            ensure_update_not_downgrade(&update, 100).expect_err("older update should be rejected");

        assert!(matches!(error, crate::SdkError::DowngradeNotAllowed));
        update.version_code = 100;
        assert!(ensure_update_not_downgrade(&update, 100).is_ok());
    }

    #[test]
    fn update_info_parses_api_response_wrapper_and_validates_shape() {
        let (update, _jwks, _file) = fixture_update(b"release bytes");
        let json = serde_json::json!({
            "code": 0,
            "message": "ok",
            "data": {
                "app_id": update.app_id,
                "version": update.version,
                "version_code": update.version_code,
                "download_url": update.download_url,
                "file_size": update.file_size,
                "sha256": update.sha256,
                "published_at_unix": update.published_at_unix,
                "signature_kid": update.signature_kid,
                "signature": update.signature,
                "signature_alg": update.signature_alg,
                "force_update": update.force_update
            },
            "request_id": "req_1"
        })
        .to_string();

        let parsed = UpdateInfo::from_api_response_json(&json).expect("api response should parse");

        assert_eq!(parsed.version_code, 100);
        assert!(UpdateInfo::from_json(r#"{"version_code": 0}"#).is_err());
    }

    fn fixture_update(bytes: &[u8]) -> (UpdateInfo, Vec<JwksKey>, NamedTempFile) {
        let (update, jwks, file, _jwks_json) = fixture_update_with_jwks_json(bytes);

        (update, jwks, file)
    }

    fn fixture_update_with_jwks_json(
        bytes: &[u8],
    ) -> (UpdateInfo, Vec<JwksKey>, NamedTempFile, String) {
        let mut file = NamedTempFile::new().expect("temp file");
        file.write_all(bytes).expect("write release bytes");
        let sha256 = format!("{:x}", Sha256::digest(bytes));
        let file_size = bytes.len() as i64;
        let key_pair = generate_key_pair();
        let raw_public_key = key_pair.public_key().as_ref();
        let public_key_pem = public_key_pem(raw_public_key);
        let app_id = "00000000-0000-0000-0000-000000000001".to_owned();
        let version = "1.0.0".to_owned();
        let version_code = 100;
        let published_at_unix = 1_780_000_000;
        let payload = release_metadata_signature_payload(
            &app_id,
            &version,
            version_code,
            &sha256,
            file_size,
            published_at_unix,
        );
        let signature = URL_SAFE_NO_PAD.encode(key_pair.sign(payload.as_bytes()).as_ref());
        let update = UpdateInfo {
            app_id,
            version,
            version_code,
            download_url: "/api/client/releases/download/app.zip?token=token".to_owned(),
            file_size,
            sha256,
            published_at_unix,
            signature_kid: "kid".to_owned(),
            signature,
            signature_alg: "Ed25519".to_owned(),
            force_update: false,
        };
        let jwks = vec![JwksKey {
            kid: "kid".to_owned(),
            alg: "EdDSA".to_owned(),
            public_key_pem,
        }];
        let jwks_json = jwks_json("kid", raw_public_key);

        (update, jwks, file, jwks_json)
    }

    fn generate_key_pair() -> Ed25519KeyPair {
        let random = SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&random).expect("generate key");
        Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).expect("read key")
    }

    fn public_key_pem(raw_public_key: &[u8]) -> String {
        let mut der = vec![
            0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
        ];
        der.extend_from_slice(raw_public_key);
        let encoded = base64::engine::general_purpose::STANDARD.encode(der);
        format!("-----BEGIN PUBLIC KEY-----\n{encoded}\n-----END PUBLIC KEY-----\n")
    }

    fn jwks_json(kid: &str, raw_public_key: &[u8]) -> String {
        format!(
            r#"{{
              "keys": [{{
                "kid": "{kid}",
                "kty": "OKP",
                "crv": "Ed25519",
                "alg": "EdDSA",
                "use": "sig",
                "x": "{}"
              }}]
            }}"#,
            URL_SAFE_NO_PAD.encode(raw_public_key)
        )
    }
}
