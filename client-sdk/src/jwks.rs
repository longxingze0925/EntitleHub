use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};
use serde::{Deserialize, Serialize};

use crate::{SdkError, SdkResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JwksKey {
    pub kid: String,
    pub alg: String,
    pub public_key_pem: String,
}

#[derive(Debug, Clone, Default)]
pub struct JwksCache {
    keys: Vec<JwksKey>,
}

impl JwksCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_keys(keys: Vec<JwksKey>) -> Self {
        let mut cache = Self::new();
        cache.upsert_all(keys);
        cache
    }

    pub fn keys(&self) -> &[JwksKey] {
        &self.keys
    }

    pub fn find(&self, kid: &str) -> Option<&JwksKey> {
        self.keys.iter().find(|key| key.kid == kid)
    }

    pub fn upsert_all(&mut self, keys: impl IntoIterator<Item = JwksKey>) {
        for key in keys {
            if let Some(existing) = self
                .keys
                .iter_mut()
                .find(|existing| existing.kid == key.kid)
            {
                *existing = key;
            } else {
                self.keys.push(key);
            }
        }
    }

    pub fn upsert_jwks_json(&mut self, jwks_json: &str) -> SdkResult<()> {
        self.upsert_all(parse_jwks_json(jwks_json)?);

        Ok(())
    }

    pub fn require_eddsa_public_key(&self, kid: &str) -> SdkResult<&str> {
        require_eddsa_public_key(&self.keys, kid)
    }

    pub fn require_eddsa_public_key_with_refresh<F>(
        &mut self,
        kid: &str,
        mut fetch_jwks_json: F,
    ) -> SdkResult<&str>
    where
        F: FnMut() -> SdkResult<String>,
    {
        if self.find(kid).is_none() {
            let jwks_json = fetch_jwks_json()?;
            self.upsert_jwks_json(&jwks_json)?;
        }

        self.require_eddsa_public_key(kid)
    }
}

#[derive(Debug, Deserialize)]
struct JwksResponse {
    keys: Vec<JwkKey>,
}

#[derive(Debug, Deserialize)]
struct JwkKey {
    kid: String,
    kty: String,
    crv: String,
    alg: String,
    #[serde(rename = "use")]
    key_use: Option<String>,
    x: String,
}

pub fn parse_jwks_json(jwks_json: &str) -> SdkResult<Vec<JwksKey>> {
    let response: JwksResponse =
        serde_json::from_str(jwks_json).map_err(|_| SdkError::InvalidJwks)?;
    response.keys.into_iter().map(jwks_key_from_jwk).collect()
}

pub fn require_eddsa_public_key<'a>(jwks: &'a [JwksKey], kid: &str) -> SdkResult<&'a str> {
    let key = jwks
        .iter()
        .find(|key| key.kid == kid)
        .ok_or(SdkError::InvalidSignature)?;
    if key.alg != "EdDSA" {
        return Err(SdkError::UnsupportedSignatureAlg(key.alg.clone()));
    }

    Ok(&key.public_key_pem)
}

fn jwks_key_from_jwk(jwk: JwkKey) -> SdkResult<JwksKey> {
    if jwk.kid.trim().is_empty()
        || jwk.kty != "OKP"
        || jwk.crv != "Ed25519"
        || jwk.alg != "EdDSA"
        || jwk.key_use.as_deref().is_some_and(|value| value != "sig")
    {
        return Err(SdkError::InvalidJwks);
    }

    let raw_public_key = URL_SAFE_NO_PAD
        .decode(jwk.x.trim())
        .map_err(|_| SdkError::InvalidJwks)?;
    if raw_public_key.len() != 32 {
        return Err(SdkError::InvalidJwks);
    }

    Ok(JwksKey {
        kid: jwk.kid,
        alg: jwk.alg,
        public_key_pem: ed25519_public_key_pem(&raw_public_key),
    })
}

fn ed25519_public_key_pem(raw_public_key: &[u8]) -> String {
    let mut der = vec![
        0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
    ];
    der.extend_from_slice(raw_public_key);
    let encoded = STANDARD.encode(der);
    let mut pem = "-----BEGIN PUBLIC KEY-----\n".to_owned();
    for chunk in encoded.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(chunk).expect("base64 is valid utf-8"));
        pem.push('\n');
    }
    pem.push_str("-----END PUBLIC KEY-----\n");

    pem
}

#[cfg(test)]
mod tests {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    use super::{parse_jwks_json, require_eddsa_public_key, JwksCache, JwksKey};

    #[test]
    fn cache_upserts_keys_by_kid() {
        let mut cache = JwksCache::from_keys(vec![key("kid", "old")]);
        cache.upsert_all(vec![key("kid", "new"), key("other", "value")]);

        assert_eq!(cache.keys().len(), 2);
        assert_eq!(
            cache.find("kid").expect("kid should exist").public_key_pem,
            "new"
        );
    }

    #[test]
    fn require_eddsa_public_key_rejects_missing_or_unsupported_key() {
        let keys = vec![JwksKey {
            kid: "kid".to_owned(),
            alg: "RS256".to_owned(),
            public_key_pem: "pem".to_owned(),
        }];

        assert!(require_eddsa_public_key(&keys, "missing").is_err());
        assert!(require_eddsa_public_key(&keys, "kid").is_err());
    }

    #[test]
    fn parse_jwks_json_converts_okp_ed25519_key_to_pem() {
        let raw_public_key = [7_u8; 32];
        let jwks_json = format!(
            r#"{{
              "keys": [{{
                "kid": "kid",
                "kty": "OKP",
                "crv": "Ed25519",
                "alg": "EdDSA",
                "use": "sig",
                "x": "{}"
              }}]
            }}"#,
            URL_SAFE_NO_PAD.encode(raw_public_key)
        );

        let keys = parse_jwks_json(&jwks_json).expect("jwks should parse");

        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].kid, "kid");
        assert!(keys[0]
            .public_key_pem
            .starts_with("-----BEGIN PUBLIC KEY-----"));
    }

    #[test]
    fn parse_jwks_json_rejects_wrong_key_type() {
        let jwks_json = r#"{
          "keys": [{
            "kid": "kid",
            "kty": "RSA",
            "crv": "Ed25519",
            "alg": "EdDSA",
            "x": "bad"
          }]
        }"#;

        assert!(parse_jwks_json(jwks_json).is_err());
    }

    #[test]
    fn cache_refreshes_jwks_json_on_missing_kid() {
        let raw_public_key = [9_u8; 32];
        let jwks_json = format!(
            r#"{{
              "keys": [{{
                "kid": "kid",
                "kty": "OKP",
                "crv": "Ed25519",
                "alg": "EdDSA",
                "use": "sig",
                "x": "{}"
              }}]
            }}"#,
            URL_SAFE_NO_PAD.encode(raw_public_key)
        );
        let mut cache = JwksCache::new();
        let public_key_pem = cache
            .require_eddsa_public_key_with_refresh("kid", || Ok(jwks_json.clone()))
            .expect("jwks should refresh");

        assert!(public_key_pem.starts_with("-----BEGIN PUBLIC KEY-----"));
        assert!(cache.find("kid").is_some());
    }

    fn key(kid: &str, public_key_pem: &str) -> JwksKey {
        JwksKey {
            kid: kid.to_owned(),
            alg: "EdDSA".to_owned(),
            public_key_pem: public_key_pem.to_owned(),
        }
    }
}
