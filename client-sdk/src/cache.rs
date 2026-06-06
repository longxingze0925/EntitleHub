use serde::{Deserialize, Serialize};

use crate::{
    device::DeviceIdentity,
    jwks::{require_eddsa_public_key, JwksCache, JwksKey},
    session::{ClientSessionState, SessionManager},
    SdkError, SdkResult,
};

pub const SDK_CACHE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SdkCacheEnvelope {
    pub version: u32,
    pub app_key: String,
    pub device: DeviceIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_key_id: Option<String>,
    pub session: Option<ClientSessionState>,
    pub jwks_keys: Vec<JwksKey>,
    pub saved_at_unix: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogoutClearOptions {
    pub keep_device_identity: bool,
    pub keep_jwks_keys: bool,
}

impl Default for LogoutClearOptions {
    fn default() -> Self {
        Self {
            keep_device_identity: true,
            keep_jwks_keys: true,
        }
    }
}

impl SdkCacheEnvelope {
    pub fn new(
        app_key: &str,
        device: DeviceIdentity,
        session: Option<ClientSessionState>,
        jwks_cache: &JwksCache,
        saved_at_unix: i64,
    ) -> SdkResult<Self> {
        Self::new_with_device_key_id(app_key, device, None, session, jwks_cache, saved_at_unix)
    }

    pub fn new_with_device_key_id(
        app_key: &str,
        device: DeviceIdentity,
        device_key_id: Option<&str>,
        session: Option<ClientSessionState>,
        jwks_cache: &JwksCache,
        saved_at_unix: i64,
    ) -> SdkResult<Self> {
        let envelope = Self {
            version: SDK_CACHE_VERSION,
            app_key: clean_app_key(app_key)?,
            device,
            device_key_id: clean_optional_device_key_id(device_key_id)?,
            session,
            jwks_keys: jwks_cache.keys().to_vec(),
            saved_at_unix,
        };
        envelope.validate()?;

        Ok(envelope)
    }

    pub fn from_json(json: &str) -> SdkResult<Self> {
        let envelope: Self = serde_json::from_str(json).map_err(|_| SdkError::InvalidCache)?;
        envelope.validate()?;

        Ok(envelope)
    }

    pub fn to_json(&self) -> SdkResult<String> {
        self.validate()?;
        serde_json::to_string(self).map_err(|_| SdkError::InvalidCache)
    }

    pub fn validate(&self) -> SdkResult<()> {
        if self.version != SDK_CACHE_VERSION {
            return Err(SdkError::InvalidCache);
        }
        clean_app_key(&self.app_key)?;
        DeviceIdentity::from_stored(
            &self.device.machine_id,
            &self.device.device_public_key,
            &self.device.private_key_pkcs8_base64,
        )?;
        if let Some(device_key_id) = &self.device_key_id {
            clean_device_key_id(device_key_id)?;
        }
        if let Some(session) = &self.session {
            session.validate()?;
        }
        for key in &self.jwks_keys {
            validate_jwks_key(key)?;
        }
        if self.saved_at_unix < 0 {
            return Err(SdkError::InvalidCache);
        }

        Ok(())
    }

    pub fn jwks_cache(&self) -> JwksCache {
        JwksCache::from_keys(self.jwks_keys.clone())
    }

    pub fn session_manager(&self) -> SessionManager {
        SessionManager::new(self.session.clone())
    }

    pub fn apply_device_key_rotation(
        &mut self,
        device: DeviceIdentity,
        device_key_id: &str,
        saved_at_unix: i64,
    ) -> SdkResult<()> {
        let mut next = self.clone();
        next.device = device;
        next.device_key_id = Some(clean_device_key_id(device_key_id)?);
        next.saved_at_unix = saved_at_unix;
        next.validate()?;

        *self = next;
        Ok(())
    }

    pub fn clear_session_for_logout(&mut self, saved_at_unix: i64) -> SdkResult<()> {
        self.session = None;
        self.saved_at_unix = saved_at_unix;
        self.validate()
    }

    pub fn into_logout_cache(
        mut self,
        options: LogoutClearOptions,
        saved_at_unix: i64,
    ) -> SdkResult<Option<Self>> {
        if !options.keep_device_identity {
            return Ok(None);
        }

        self.session = None;
        if !options.keep_jwks_keys {
            self.jwks_keys.clear();
        }
        self.saved_at_unix = saved_at_unix;
        self.validate()?;

        Ok(Some(self))
    }
}

fn clean_app_key(app_key: &str) -> SdkResult<String> {
    let app_key = app_key.trim();
    if app_key.is_empty() {
        return Err(SdkError::InvalidCache);
    }

    Ok(app_key.to_owned())
}

fn clean_optional_device_key_id(device_key_id: Option<&str>) -> SdkResult<Option<String>> {
    device_key_id.map(clean_device_key_id).transpose()
}

fn clean_device_key_id(device_key_id: &str) -> SdkResult<String> {
    let device_key_id = device_key_id.trim();
    if device_key_id.is_empty() || device_key_id.contains(char::is_whitespace) {
        return Err(SdkError::InvalidCache);
    }

    Ok(device_key_id.to_owned())
}

fn validate_jwks_key(key: &JwksKey) -> SdkResult<()> {
    if key.kid.trim().is_empty() || key.alg.trim() != "EdDSA" {
        return Err(SdkError::InvalidCache);
    }

    require_eddsa_public_key(std::slice::from_ref(key), &key.kid)
        .map(|_| ())
        .map_err(|_| SdkError::InvalidCache)
}

#[cfg(test)]
mod tests {
    use crate::{
        device::DeviceIdentity,
        jwks::{JwksCache, JwksKey},
        session::{ClientSessionState, SessionInit},
        signing::generate_device_keypair,
    };

    use super::{LogoutClearOptions, SdkCacheEnvelope, SDK_CACHE_VERSION};

    #[test]
    fn cache_envelope_round_trips_device_session_and_jwks() {
        let device = DeviceIdentity::generate("app_key", &["machine"]).expect("device identity");
        let session = fixture_session();
        let jwks = JwksCache::from_keys(vec![fixture_jwks_key()]);
        let envelope = SdkCacheEnvelope::new(
            " app_key ",
            device.clone(),
            Some(session.clone()),
            &jwks,
            1_717_171_717,
        )
        .expect("cache envelope should build");
        let json = envelope.to_json().expect("cache should serialize");

        let decoded = SdkCacheEnvelope::from_json(&json).expect("cache should deserialize");

        assert_eq!(decoded.version, SDK_CACHE_VERSION);
        assert_eq!(decoded.app_key, "app_key");
        assert_eq!(decoded.device, device);
        assert_eq!(decoded.device_key_id, None);
        assert_eq!(decoded.session, Some(session));
        assert_eq!(decoded.jwks_cache().keys().len(), 1);
        assert!(decoded.session_manager().current_session().is_ok());
    }

    #[test]
    fn cache_envelope_rejects_invalid_version_or_session_shape() {
        let device = DeviceIdentity::generate("app_key", &["machine"]).expect("device identity");
        let jwks = JwksCache::from_keys(vec![fixture_jwks_key()]);
        let mut envelope = SdkCacheEnvelope::new_with_device_key_id(
            "app_key",
            device,
            Some("device-key-id"),
            Some(fixture_session()),
            &jwks,
            1,
        )
        .expect("cache envelope should build");

        envelope.version = 2;
        assert!(envelope.validate().is_err());

        envelope.version = SDK_CACHE_VERSION;
        envelope.session.as_mut().expect("session").access_token = " ".to_owned();
        assert!(envelope.validate().is_err());

        envelope.session = Some(fixture_session());
        envelope.device_key_id = Some(" ".to_owned());
        assert!(envelope.validate().is_err());
    }

    #[test]
    fn cache_envelope_rejects_invalid_jwks_key() {
        let device = DeviceIdentity::generate("app_key", &["machine"]).expect("device identity");
        let jwks = JwksCache::from_keys(vec![JwksKey {
            kid: "kid".to_owned(),
            alg: "HS256".to_owned(),
            public_key_pem: "public".to_owned(),
        }]);

        assert!(SdkCacheEnvelope::new("app_key", device, None, &jwks, 1).is_err());
    }

    #[test]
    fn cache_envelope_tracks_device_key_id_and_rotation() {
        let device = DeviceIdentity::generate("app_key", &["machine"]).expect("device identity");
        let jwks = JwksCache::from_keys(vec![fixture_jwks_key()]);
        let mut envelope = SdkCacheEnvelope::new_with_device_key_id(
            "app_key",
            device.clone(),
            Some(" old-key-id "),
            Some(fixture_session()),
            &jwks,
            1,
        )
        .expect("cache envelope should build");
        let rotated = device.rotate_key().expect("device key should rotate");

        envelope
            .apply_device_key_rotation(rotated.clone(), "next-key-id", 2)
            .expect("rotation should update cache");
        let decoded =
            SdkCacheEnvelope::from_json(&envelope.to_json().expect("cache should serialize"))
                .expect("cache should deserialize");

        assert_eq!(decoded.device, rotated);
        assert_eq!(decoded.device_key_id.as_deref(), Some("next-key-id"));
        assert_eq!(decoded.saved_at_unix, 2);
    }

    #[test]
    fn cache_logout_clear_defaults_to_removing_session_only() {
        let device = DeviceIdentity::generate("app_key", &["machine"]).expect("device identity");
        let jwks = JwksCache::from_keys(vec![fixture_jwks_key()]);
        let envelope = SdkCacheEnvelope::new_with_device_key_id(
            "app_key",
            device.clone(),
            Some("device-key-id"),
            Some(fixture_session()),
            &jwks,
            1,
        )
        .expect("cache envelope should build");

        let cleared = envelope
            .into_logout_cache(LogoutClearOptions::default(), 2)
            .expect("logout cache should build")
            .expect("device identity should be retained");

        assert_eq!(cleared.device, device);
        assert_eq!(cleared.device_key_id.as_deref(), Some("device-key-id"));
        assert_eq!(cleared.session, None);
        assert_eq!(cleared.jwks_keys.len(), 1);
        assert_eq!(cleared.saved_at_unix, 2);
    }

    #[test]
    fn cache_logout_clear_can_remove_jwks_or_whole_cache() {
        let device = DeviceIdentity::generate("app_key", &["machine"]).expect("device identity");
        let jwks = JwksCache::from_keys(vec![fixture_jwks_key()]);
        let envelope =
            SdkCacheEnvelope::new("app_key", device.clone(), Some(fixture_session()), &jwks, 1)
                .expect("cache envelope should build");

        let cleared = envelope
            .clone()
            .into_logout_cache(
                LogoutClearOptions {
                    keep_device_identity: true,
                    keep_jwks_keys: false,
                },
                2,
            )
            .expect("logout cache should build")
            .expect("device identity should be retained");
        assert!(cleared.jwks_keys.is_empty());

        let removed = envelope
            .into_logout_cache(
                LogoutClearOptions {
                    keep_device_identity: false,
                    keep_jwks_keys: true,
                },
                2,
            )
            .expect("logout cache should build");
        assert_eq!(removed, None);
    }

    fn fixture_session() -> ClientSessionState {
        ClientSessionState::from_init(
            SessionInit {
                session_id: "session-id".to_owned(),
                device_id: "device-id".to_owned(),
                access_token: "access-token".to_owned(),
                refresh_token: "refresh-token".to_owned(),
                token_type: None,
                expires_in: 900,
                refresh_expires_in: 2_500,
                features: serde_json::json!({}),
            },
            100,
        )
        .expect("session should build")
    }

    fn fixture_jwks_key() -> JwksKey {
        let keypair = generate_device_keypair().expect("jwks fixture keypair");

        JwksKey {
            kid: "kid".to_owned(),
            alg: "EdDSA".to_owned(),
            public_key_pem: keypair.public_key_pem,
        }
    }
}
