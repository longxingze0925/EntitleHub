use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{
        header::{CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE},
        HeaderMap, HeaderValue,
    },
    response::IntoResponse,
    Extension, Json,
};
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde::Serialize;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::{
    crypto::token::{generate_token, hash_token},
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        client_auth::session::{ensure_active_entitlement, ClientContext},
        release::{
            model::{validate_download_file_name, NewDownloadToken, ReleaseWithFile},
            repository::ReleaseRepository,
        },
    },
    rate_limit,
    state::AppState,
};

#[derive(Debug, Serialize)]
pub struct LatestReleaseResponse {
    pub app_id: Uuid,
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

#[derive(Debug, Deserialize)]
pub struct DownloadReleaseQuery {
    pub token: String,
}

pub async fn latest_release(
    State(state): State<AppState>,
    Extension(client): Extension<ClientContext>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<ApiResponse<LatestReleaseResponse>>, AppError> {
    rate_limit::check_client_action(&state, "latest_release", &client.device_id.to_string())
        .await?;
    ensure_active_entitlement(&client)?;

    let repository = ReleaseRepository::new(state.db.clone());
    let release = repository
        .latest_published(client.tenant_id, client.app_id)
        .await?
        .ok_or_else(AppError::release_not_found)?;
    let download_token = generate_token();
    let token_hash = hash_token(&state.config.security.token_hash_pepper, &download_token)?;
    let expires_at =
        Utc::now() + Duration::seconds(state.config.security.download_token_ttl_seconds);
    let new_download_token = NewDownloadToken::release_file(
        client.tenant_id,
        client.app_id,
        client.device_id,
        release.file_id,
        token_hash,
        expires_at,
    )?;
    repository.create_download_token(new_download_token).await?;

    Ok(Json(ApiResponse::ok(
        latest_release_response(release, &download_token)?,
        request_id.to_string(),
    )))
}

pub async fn download_release(
    State(state): State<AppState>,
    Path(file_name): Path<String>,
    Query(query): Query<DownloadReleaseQuery>,
) -> Result<impl IntoResponse, AppError> {
    let file_name = validate_download_file_name(&file_name)?;
    let token = query.token.trim();
    if token.is_empty() {
        return Err(AppError::invalid_request("download token is required"));
    }
    let token_hash = hash_token(&state.config.security.token_hash_pepper, token)?;
    let download = ReleaseRepository::new(state.db.clone())
        .consume_download_token(&file_name, &token_hash)
        .await?
        .ok_or_else(|| AppError::forbidden("download token invalid"))?;
    if download.file_name != file_name {
        return Err(AppError::forbidden("download token invalid"));
    }
    rate_limit::check_fixed_window(
        &state,
        rate_limit::download_key(
            &download.file_id.to_string(),
            &download.device_id.to_string(),
        ),
        state.config.security.download_rate_limit_max,
        state.config.security.download_rate_limit_window_seconds,
        AppError::rate_limited,
    )
    .await?;

    let object = state.object_store.open(&download.storage_key).await?;
    if object.size != download.file_size as u64 {
        return Err(AppError::dependency("stored object size mismatch"));
    }

    let stream = ReaderStream::new(object.reader);
    let body = Body::from_stream(stream);
    let mut headers = HeaderMap::new();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&download.file_size.to_string())
            .map_err(|error| AppError::dependency(format!("content length invalid: {error}")))?,
    );
    headers.insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", download.file_name))
            .map_err(|error| {
                AppError::dependency(format!("content disposition invalid: {error}"))
            })?,
    );

    Ok((headers, body))
}

fn latest_release_response(
    release: ReleaseWithFile,
    token: &str,
) -> Result<LatestReleaseResponse, AppError> {
    let published_at = release
        .published_at
        .ok_or_else(|| AppError::dependency("published release missing published_at"))?;
    let download_url = format!(
        "/api/client/releases/download/{}?token={}",
        release.file_name, token
    );

    Ok(LatestReleaseResponse {
        app_id: release.app_id,
        version: release.version,
        version_code: release.version_code,
        download_url,
        file_size: release.file_size,
        sha256: release.sha256,
        published_at_unix: published_at.timestamp(),
        signature_kid: release.release_signature_kid,
        signature: release.release_signature,
        signature_alg: release.release_signature_alg,
        force_update: release.force_update,
    })
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use uuid::Uuid;

    use crate::modules::release::model::ReleaseWithFile;

    use super::latest_release_response;

    #[test]
    fn latest_response_embeds_download_token() {
        let release = ReleaseWithFile {
            id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            app_id: Uuid::nil(),
            file_id: Uuid::nil(),
            version: "1.0.0".to_owned(),
            version_code: 100,
            status: "published".to_owned(),
            changelog: None,
            force_update: false,
            signing_key_id: Uuid::nil(),
            release_signature_kid: "release-kid".to_owned(),
            release_signature: "release-sig".to_owned(),
            release_signature_alg: "Ed25519".to_owned(),
            published_at: Some(Utc::now()),
            deprecated_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            file_name: "app.zip".to_owned(),
            file_size: 123,
            sha256: "a".repeat(64),
            signature_kid: "kid".to_owned(),
            signature: "sig".to_owned(),
            signature_alg: "Ed25519".to_owned(),
        };

        let response = latest_release_response(release, "token").expect("latest response");

        assert_eq!(
            response.download_url,
            "/api/client/releases/download/app.zip?token=token"
        );
        assert_eq!(response.signature_kid, "release-kid");
    }
}
