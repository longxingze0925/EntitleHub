use std::{
    io,
    path::{Component, Path, PathBuf},
    pin::Pin,
    sync::Arc,
};

use chrono::{DateTime, Utc};
use futures_util::TryStreamExt;
use hmac::{Hmac, Mac};
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION, HOST},
    Client, Method, StatusCode, Url,
};
use sha2::{Digest, Sha256};
use tokio::{
    fs::{self, File},
    io::AsyncRead,
};
use tokio_util::io::StreamReader;

use crate::{config::ObjectStorageConfig, error::AppError};

type HmacSha256 = Hmac<Sha256>;
type ObjectReader = Pin<Box<dyn AsyncRead + Send>>;

pub struct StoredObject {
    pub reader: ObjectReader,
    pub size: u64,
}

pub trait ObjectStore: Send + Sync {
    fn open<'a>(
        &'a self,
        storage_key: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<StoredObject, AppError>> + Send + 'a>,
    >;

    fn put_bytes<'a>(
        &'a self,
        storage_key: &'a str,
        bytes: &'a [u8],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), AppError>> + Send + 'a>>;
}

#[derive(Debug, Clone)]
pub struct LocalObjectStore {
    root: PathBuf,
}

impl LocalObjectStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn resolve_path(&self, storage_key: &str) -> Result<PathBuf, AppError> {
        let relative_path = safe_relative_path(storage_key)?;
        let root = self.root.canonicalize().map_err(|error| {
            AppError::dependency(format!("object storage root invalid: {error}"))
        })?;
        let path = root.join(relative_path);

        if !path.starts_with(&root) {
            return Err(AppError::forbidden("storage key invalid"));
        }

        Ok(path)
    }
}

impl ObjectStore for LocalObjectStore {
    fn open<'a>(
        &'a self,
        storage_key: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<StoredObject, AppError>> + Send + 'a>,
    > {
        Box::pin(async move {
            let path = self.resolve_path(storage_key)?;
            let file = File::open(&path).await.map_err(|error| {
                AppError::not_found(format!("stored object not found: {error}"))
            })?;
            let metadata = file.metadata().await.map_err(|error| {
                AppError::dependency(format!("stored object metadata failed: {error}"))
            })?;
            if !metadata.is_file() {
                return Err(AppError::not_found("stored object not found"));
            }

            Ok(StoredObject {
                reader: Box::pin(file),
                size: metadata.len(),
            })
        })
    }

    fn put_bytes<'a>(
        &'a self,
        storage_key: &'a str,
        bytes: &'a [u8],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), AppError>> + Send + 'a>>
    {
        Box::pin(async move {
            let path = self.resolve_path(storage_key)?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).await.map_err(|error| {
                    AppError::dependency(format!("object storage directory create failed: {error}"))
                })?;
            }
            fs::write(&path, bytes).await.map_err(|error| {
                AppError::dependency(format!("stored object write failed: {error}"))
            })
        })
    }
}

#[derive(Debug, Clone)]
pub struct S3ObjectStore {
    client: Client,
    endpoint: Url,
    bucket: String,
    access_key: String,
    secret_key: String,
    region: String,
}

impl S3ObjectStore {
    pub fn new(
        endpoint: String,
        bucket: String,
        access_key: String,
        secret_key: String,
        region: String,
    ) -> Result<Self, AppError> {
        let endpoint = Url::parse(endpoint.trim()).map_err(|error| {
            AppError::config(format!("OBJECT_STORAGE_ENDPOINT invalid: {error}"))
        })?;
        if endpoint.host_str().is_none() {
            return Err(AppError::config(
                "OBJECT_STORAGE_ENDPOINT must include host",
            ));
        }
        if endpoint.query().is_some() || endpoint.fragment().is_some() {
            return Err(AppError::config(
                "OBJECT_STORAGE_ENDPOINT must not include query or fragment",
            ));
        }
        let bucket = bucket.trim().to_owned();
        if bucket.is_empty() {
            return Err(AppError::config("OBJECT_STORAGE_BUCKET must not be empty"));
        }
        let access_key = access_key.trim().to_owned();
        if access_key.is_empty() {
            return Err(AppError::config(
                "OBJECT_STORAGE_ACCESS_KEY must not be empty",
            ));
        }
        let secret_key = secret_key.trim().to_owned();
        if secret_key.is_empty() {
            return Err(AppError::config(
                "OBJECT_STORAGE_SECRET_KEY must not be empty",
            ));
        }
        let region = region.trim().to_owned();
        if region.is_empty() {
            return Err(AppError::config("OBJECT_STORAGE_REGION must not be empty"));
        }

        Ok(Self {
            client: Client::new(),
            endpoint,
            bucket,
            access_key,
            secret_key,
            region,
        })
    }

    fn object_url_and_path(&self, storage_key: &str) -> Result<(String, String), AppError> {
        safe_relative_path(storage_key)?;

        let path = self.canonical_object_path(storage_key);
        let mut origin = self.endpoint.clone();
        origin.set_path("");
        origin.set_query(None);
        origin.set_fragment(None);
        let url = format!("{}{}", origin.as_str().trim_end_matches('/'), path);

        Ok((url, path))
    }

    fn canonical_object_path(&self, storage_key: &str) -> String {
        let base_path = self.endpoint.path().trim_end_matches('/');
        let mut path = String::new();
        if !base_path.is_empty() {
            path.push_str(base_path);
        }
        path.push('/');
        path.push_str(&percent_encode_path_component(&self.bucket));
        path.push('/');
        path.push_str(&percent_encode_s3_key(storage_key.trim()));
        path
    }

    fn host_header(&self) -> Result<String, AppError> {
        let host = self
            .endpoint
            .host_str()
            .ok_or_else(|| AppError::config("OBJECT_STORAGE_ENDPOINT must include host"))?;
        if let Some(port) = self.endpoint.port() {
            Ok(format!("{host}:{port}"))
        } else {
            Ok(host.to_owned())
        }
    }

    fn signed_headers(
        &self,
        method: &Method,
        canonical_path: &str,
        payload_hash: &str,
        now: DateTime<Utc>,
    ) -> Result<HeaderMap, AppError> {
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let short_date = now.format("%Y%m%d").to_string();
        let credential_scope = format!("{}/{}/s3/aws4_request", short_date, self.region);
        let host = self.host_header()?;
        let canonical_headers =
            format!("host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n");
        let signed_headers = "host;x-amz-content-sha256;x-amz-date";
        let canonical_request = format!(
            "{}\n{canonical_path}\n\n{canonical_headers}\n{signed_headers}\n{payload_hash}",
            method.as_str()
        );
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
            sha256_hex(canonical_request.as_bytes())
        );
        let signature = s3_signature(&self.secret_key, &short_date, &self.region, &string_to_sign)?;
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.access_key
        );

        let mut headers = HeaderMap::new();
        headers.insert(HOST, header_value(&host)?);
        headers.insert("x-amz-content-sha256", header_value(payload_hash)?);
        headers.insert("x-amz-date", header_value(&amz_date)?);
        headers.insert(AUTHORIZATION, header_value(&authorization)?);

        Ok(headers)
    }
}

impl ObjectStore for S3ObjectStore {
    fn open<'a>(
        &'a self,
        storage_key: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<StoredObject, AppError>> + Send + 'a>>
    {
        Box::pin(async move {
            let (url, canonical_path) = self.object_url_and_path(storage_key)?;
            let payload_hash = sha256_hex(&[]);
            let headers =
                self.signed_headers(&Method::GET, &canonical_path, &payload_hash, Utc::now())?;
            let response = self
                .client
                .get(url)
                .headers(headers)
                .send()
                .await
                .map_err(|error| AppError::dependency(format!("s3 object read failed: {error}")))?;

            if response.status() == StatusCode::NOT_FOUND {
                return Err(AppError::not_found("stored object not found"));
            }
            if !response.status().is_success() {
                return Err(AppError::dependency(format!(
                    "s3 object read failed: status {}",
                    response.status()
                )));
            }

            let size = response.content_length().ok_or_else(|| {
                AppError::dependency("s3 object read failed: missing content length")
            })?;
            let stream = response.bytes_stream().map_err(|error| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("s3 object stream failed: {error}"),
                )
            });
            let reader = StreamReader::new(stream);

            Ok(StoredObject {
                reader: Box::pin(reader),
                size,
            })
        })
    }

    fn put_bytes<'a>(
        &'a self,
        storage_key: &'a str,
        bytes: &'a [u8],
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), AppError>> + Send + 'a>> {
        Box::pin(async move {
            let (url, canonical_path) = self.object_url_and_path(storage_key)?;
            let payload_hash = sha256_hex(bytes);
            let headers =
                self.signed_headers(&Method::PUT, &canonical_path, &payload_hash, Utc::now())?;
            let response = self
                .client
                .put(url)
                .headers(headers)
                .body(bytes.to_vec())
                .send()
                .await
                .map_err(|error| {
                    AppError::dependency(format!("s3 object write failed: {error}"))
                })?;

            if !response.status().is_success() {
                return Err(AppError::dependency(format!(
                    "s3 object write failed: status {}",
                    response.status()
                )));
            }

            Ok(())
        })
    }
}

pub fn build_object_store(config: &ObjectStorageConfig) -> Result<Arc<dyn ObjectStore>, AppError> {
    match config.mode.as_str() {
        "local" => {
            let root = config.local_root.clone().ok_or_else(|| {
                AppError::config("OBJECT_STORAGE_LOCAL_ROOT is required for local object storage")
            })?;
            Ok(Arc::new(LocalObjectStore::new(root)))
        }
        "s3" => Ok(Arc::new(S3ObjectStore::new(
            config.endpoint.clone().ok_or_else(|| {
                AppError::config("OBJECT_STORAGE_ENDPOINT is required for s3 object storage")
            })?,
            config.bucket.clone().ok_or_else(|| {
                AppError::config("OBJECT_STORAGE_BUCKET is required for s3 object storage")
            })?,
            config.access_key.clone().ok_or_else(|| {
                AppError::config("OBJECT_STORAGE_ACCESS_KEY is required for s3 object storage")
            })?,
            config.secret_key.clone().ok_or_else(|| {
                AppError::config("OBJECT_STORAGE_SECRET_KEY is required for s3 object storage")
            })?,
            config.region.clone(),
        )?)),
        _ => Err(AppError::config("OBJECT_STORAGE_MODE is invalid")),
    }
}

fn safe_relative_path(storage_key: &str) -> Result<PathBuf, AppError> {
    let storage_key = storage_key.trim();
    if storage_key.is_empty() || storage_key.contains('\\') || storage_key.contains('\0') {
        return Err(AppError::forbidden("storage key invalid"));
    }

    let path = Path::new(storage_key);
    if path.is_absolute() {
        return Err(AppError::forbidden("storage key invalid"));
    }

    let mut safe = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => safe.push(value),
            _ => return Err(AppError::forbidden("storage key invalid")),
        }
    }

    Ok(safe)
}

fn percent_encode_s3_key(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .flat_map(|byte| {
            if *byte == b'/' {
                "/".to_owned()
            } else {
                percent_encode_byte(*byte)
            }
            .into_bytes()
        })
        .map(char::from)
        .collect()
}

fn percent_encode_path_component(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .flat_map(|byte| percent_encode_byte(*byte).into_bytes())
        .map(char::from)
        .collect()
}

fn percent_encode_byte(byte: u8) -> String {
    if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
        char::from(byte).to_string()
    } else {
        format!("%{byte:02X}")
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn s3_signature(
    secret_key: &str,
    short_date: &str,
    region: &str,
    string_to_sign: &str,
) -> Result<String, AppError> {
    let date_key = hmac_sha256(
        format!("AWS4{secret_key}").as_bytes(),
        short_date.as_bytes(),
    )?;
    let region_key = hmac_sha256(&date_key, region.as_bytes())?;
    let service_key = hmac_sha256(&region_key, b"s3")?;
    let signing_key = hmac_sha256(&service_key, b"aws4_request")?;
    let signature = hmac_sha256(&signing_key, string_to_sign.as_bytes())?;

    Ok(hex_bytes(&signature))
}

fn hmac_sha256(key: &[u8], value: &[u8]) -> Result<Vec<u8>, AppError> {
    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|error| AppError::crypto(format!("hmac key invalid: {error}")))?;
    mac.update(value);

    Ok(mac.finalize().into_bytes().to_vec())
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push_str(&format!("{byte:02x}"));
    }
    value
}

fn header_value(value: &str) -> Result<HeaderValue, AppError> {
    HeaderValue::from_str(value)
        .map_err(|error| AppError::dependency(format!("object storage header invalid: {error}")))
}

#[allow(dead_code)]
fn _assert_reader_is_async_read<T: AsyncRead>(_reader: &T) {}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use axum::{
        body::{Body, Bytes},
        extract::{Path, State},
        http::{header::CONTENT_LENGTH, HeaderMap, Response, StatusCode as HttpStatusCode},
        routing::put,
        Router,
    };
    use tokio::{io::AsyncReadExt, net::TcpListener, sync::Mutex};

    use super::{
        hex_bytes, hmac_sha256, percent_encode_s3_key, safe_relative_path, ObjectStore,
        S3ObjectStore,
    };

    type FakeS3Objects = Arc<Mutex<HashMap<String, Vec<u8>>>>;

    #[test]
    fn safe_relative_path_rejects_traversal_and_absolute_paths() {
        assert!(safe_relative_path("../secret").is_err());
        assert!(safe_relative_path("/secret").is_err());
        assert!(safe_relative_path("tenants/a/file").is_ok());
    }

    #[test]
    fn s3_object_path_uses_path_style_and_encodes_key() {
        let store = S3ObjectStore::new(
            "http://minio:9000".to_owned(),
            "release-bucket".to_owned(),
            "access".to_owned(),
            "secret".to_owned(),
            "us-east-1".to_owned(),
        )
        .expect("s3 store");

        assert_eq!(
            store.canonical_object_path("tenants/a/releases/app 1.zip"),
            "/release-bucket/tenants/a/releases/app%201.zip"
        );
    }

    #[test]
    fn s3_key_encoding_preserves_slashes() {
        assert_eq!(
            percent_encode_s3_key("tenants/a+b/app.zip"),
            "tenants/a%2Bb/app.zip"
        );
    }

    #[test]
    fn hmac_sha256_matches_rfc4231_vector() {
        let key = [0x0b; 20];
        let digest = hmac_sha256(&key, b"Hi There").expect("hmac");

        assert_eq!(
            hex_bytes(&digest),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[tokio::test]
    async fn s3_object_store_puts_and_reads_with_signed_requests() {
        let objects = Arc::new(Mutex::new(HashMap::new()));
        let app = Router::new()
            .route("/{bucket}/{*key}", put(fake_s3_put).get(fake_s3_get))
            .with_state(objects);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake s3");
        let address = listener.local_addr().expect("fake s3 address");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("fake s3 server");
        });
        let store = S3ObjectStore::new(
            format!("http://{address}"),
            "releases".to_owned(),
            "access".to_owned(),
            "secret".to_owned(),
            "us-east-1".to_owned(),
        )
        .expect("s3 store");

        store
            .put_bytes("tenants/a/releases/app.zip", b"hello")
            .await
            .expect("put object");
        let object = store
            .open("tenants/a/releases/app.zip")
            .await
            .expect("open object");
        let mut reader = object.reader;
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.expect("read object");

        assert_eq!(object.size, 5);
        assert_eq!(bytes, b"hello");
    }

    async fn fake_s3_put(
        Path((bucket, key)): Path<(String, String)>,
        State(objects): State<FakeS3Objects>,
        headers: HeaderMap,
        body: Bytes,
    ) -> HttpStatusCode {
        if bucket != "releases" || !has_sigv4_headers(&headers) {
            return HttpStatusCode::BAD_REQUEST;
        }

        objects.lock().await.insert(key, body.to_vec());
        HttpStatusCode::OK
    }

    async fn fake_s3_get(
        Path((bucket, key)): Path<(String, String)>,
        State(objects): State<FakeS3Objects>,
        headers: HeaderMap,
    ) -> Response<Body> {
        if bucket != "releases" || !has_sigv4_headers(&headers) {
            return Response::builder()
                .status(HttpStatusCode::BAD_REQUEST)
                .body(Body::empty())
                .expect("bad request response");
        }

        let Some(bytes) = objects.lock().await.get(&key).cloned() else {
            return Response::builder()
                .status(HttpStatusCode::NOT_FOUND)
                .body(Body::empty())
                .expect("not found response");
        };

        Response::builder()
            .status(HttpStatusCode::OK)
            .header(CONTENT_LENGTH, bytes.len().to_string())
            .body(Body::from(bytes))
            .expect("object response")
    }

    fn has_sigv4_headers(headers: &HeaderMap) -> bool {
        headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("AWS4-HMAC-SHA256 "))
            && headers.contains_key("x-amz-date")
            && headers.contains_key("x-amz-content-sha256")
    }
}
