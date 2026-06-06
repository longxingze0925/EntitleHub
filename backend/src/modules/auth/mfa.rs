use axum::{extract::State, http::HeaderMap, Extension, Json};
use chrono::Utc;
use rand_core::{OsRng, RngCore};
use ring::hmac;
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction};
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::{
    crypto::{
        envelope::{decrypt_bytes, encrypt_bytes, PrivateKeyEnvelope},
        password::verify_password,
        token::{generate_recovery_code, hash_token},
    },
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        audit::{self, AuditLogInput},
        auth::session::AdminContext,
        team::{model::TeamMember, repository::TeamMemberRepository},
    },
    rate_limit,
    state::AppState,
};

const TOTP_PERIOD_SECONDS: i64 = 30;
const TOTP_DIGITS: u32 = 6;
const TOTP_WINDOW_STEPS: i64 = 1;
const MFA_SECRET_BYTES: usize = 20;
const RECOVERY_CODE_COUNT: usize = 10;
const BASE32_ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

#[derive(Debug, Serialize)]
pub struct MfaSetupResponse {
    pub secret: String,
    pub otpauth_url: String,
    pub recovery_codes: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct MfaEnableRequest {
    pub code: String,
}

#[derive(Debug, Deserialize)]
pub struct MfaDisableRequest {
    pub password: String,
    pub code: String,
}

#[derive(Debug, Deserialize)]
pub struct MfaRegenerateRecoveryCodesRequest {
    pub password: String,
    pub code: String,
}

#[derive(Debug, Serialize)]
pub struct MfaOkResponse {
    pub ok: bool,
}

#[derive(Debug, Serialize)]
pub struct MfaRecoveryCodesResponse {
    pub recovery_codes: Vec<String>,
}

pub async fn setup(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<ApiResponse<MfaSetupResponse>>, AppError> {
    let member = TeamMemberRepository::new(state.db.clone())
        .find_by_id(admin.tenant_id, admin.team_member_id)
        .await?
        .ok_or_else(AppError::user_not_found)?;
    if member.mfa_enabled {
        return Err(AppError::mfa_already_enabled());
    }

    let secret = generate_base32_secret();
    let encrypted_secret = encrypt_secret_to_text(&state, &secret)?;
    let recovery_codes = generate_recovery_codes();
    let recovery_hashes = hash_recovery_codes(&state, &recovery_codes)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    update_member_mfa_secret_in_transaction(
        &mut transaction,
        admin.tenant_id,
        admin.team_member_id,
        Some(&encrypted_secret),
    )
    .await?;
    replace_recovery_codes_in_transaction(
        &mut transaction,
        admin.tenant_id,
        admin.team_member_id,
        &recovery_hashes,
    )
    .await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "team_member.mfa.setup",
            resource_type: "team_member",
            resource_id: Some(admin.team_member_id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(team_member_mfa_audit_json(&member)),
            after_json: Some(serde_json::json!({
                "id": admin.team_member_id,
                "mfa_enabled": false,
                "mfa_secret_configured": true,
                "recovery_code_count": recovery_codes.len(),
            })),
            metadata_json: serde_json::json!({}),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        MfaSetupResponse {
            secret: secret.clone(),
            otpauth_url: build_otpauth_url(&state.config.app.name, &member.email, &secret),
            recovery_codes,
        },
        request_id.to_string(),
    )))
}

pub async fn enable(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Json(payload): Json<MfaEnableRequest>,
) -> Result<Json<ApiResponse<MfaOkResponse>>, AppError> {
    check_mfa_rate_limit(&state, &admin, &headers).await?;
    let member = TeamMemberRepository::new(state.db.clone())
        .find_by_id(admin.tenant_id, admin.team_member_id)
        .await?
        .ok_or_else(AppError::user_not_found)?;
    if member.mfa_enabled {
        return Err(AppError::mfa_already_enabled());
    }

    let secret = decrypt_member_mfa_secret(&state, &member)?;
    if !verify_totp_code(&secret, &payload.code)? {
        return Err(AppError::mfa_failed());
    }

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let updated = set_member_mfa_enabled_in_transaction(
        &mut transaction,
        admin.tenant_id,
        admin.team_member_id,
        true,
    )
    .await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "team_member.mfa.enable",
            resource_type: "team_member",
            resource_id: Some(admin.team_member_id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(team_member_mfa_audit_json(&member)),
            after_json: Some(team_member_mfa_audit_json(&updated)),
            metadata_json: serde_json::json!({}),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        MfaOkResponse { ok: true },
        request_id.to_string(),
    )))
}

pub async fn disable(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Json(payload): Json<MfaDisableRequest>,
) -> Result<Json<ApiResponse<MfaOkResponse>>, AppError> {
    check_mfa_rate_limit(&state, &admin, &headers).await?;
    let member = TeamMemberRepository::new(state.db.clone())
        .find_by_id(admin.tenant_id, admin.team_member_id)
        .await?
        .ok_or_else(AppError::user_not_found)?;
    if !member.mfa_enabled {
        return Err(AppError::mfa_not_enabled());
    }
    if !verify_password(&payload.password, &member.password_hash)? {
        return Err(AppError::invalid_credentials());
    }

    let secret = decrypt_member_mfa_secret(&state, &member)?;
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    verify_mfa_code_in_transaction(
        &state,
        &mut transaction,
        admin.tenant_id,
        admin.team_member_id,
        &secret,
        &payload.code,
    )
    .await?;
    let updated =
        disable_member_mfa_in_transaction(&mut transaction, admin.tenant_id, admin.team_member_id)
            .await?;
    revoke_recovery_codes_in_transaction(&mut transaction, admin.tenant_id, admin.team_member_id)
        .await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "team_member.mfa.disable",
            resource_type: "team_member",
            resource_id: Some(admin.team_member_id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(team_member_mfa_audit_json(&member)),
            after_json: Some(team_member_mfa_audit_json(&updated)),
            metadata_json: serde_json::json!({}),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        MfaOkResponse { ok: true },
        request_id.to_string(),
    )))
}

pub async fn regenerate_recovery_codes(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Json(payload): Json<MfaRegenerateRecoveryCodesRequest>,
) -> Result<Json<ApiResponse<MfaRecoveryCodesResponse>>, AppError> {
    check_mfa_rate_limit(&state, &admin, &headers).await?;
    let member = TeamMemberRepository::new(state.db.clone())
        .find_by_id(admin.tenant_id, admin.team_member_id)
        .await?
        .ok_or_else(AppError::user_not_found)?;
    if !member.mfa_enabled {
        return Err(AppError::mfa_not_enabled());
    }
    if !verify_password(&payload.password, &member.password_hash)? {
        return Err(AppError::invalid_credentials());
    }

    let secret = decrypt_member_mfa_secret(&state, &member)?;
    let recovery_codes = generate_recovery_codes();
    let recovery_hashes = hash_recovery_codes(&state, &recovery_codes)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    verify_mfa_code_in_transaction(
        &state,
        &mut transaction,
        admin.tenant_id,
        admin.team_member_id,
        &secret,
        &payload.code,
    )
    .await?;
    replace_recovery_codes_in_transaction(
        &mut transaction,
        admin.tenant_id,
        admin.team_member_id,
        &recovery_hashes,
    )
    .await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "team_member.mfa.recovery_codes.regenerate",
            resource_type: "team_member",
            resource_id: Some(admin.team_member_id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(team_member_mfa_audit_json(&member)),
            after_json: Some(serde_json::json!({
                "id": admin.team_member_id,
                "mfa_enabled": true,
                "recovery_code_count": recovery_codes.len(),
            })),
            metadata_json: serde_json::json!({}),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        MfaRecoveryCodesResponse { recovery_codes },
        request_id.to_string(),
    )))
}

pub async fn verify_member_mfa_code(
    state: &AppState,
    tenant_id: Uuid,
    team_member_id: Uuid,
    encrypted_secret: Option<&str>,
    code: &str,
) -> Result<bool, AppError> {
    let encrypted_secret =
        encrypted_secret.ok_or_else(|| AppError::config("mfa secret is not configured"))?;
    let secret = decrypt_secret_text(state, encrypted_secret)?;
    if verify_totp_code(&secret, code)? {
        return Ok(true);
    }

    let Some(recovery_code) = normalize_recovery_code(code) else {
        return Ok(false);
    };
    let code_hash = hash_token(&state.config.security.token_hash_pepper, &recovery_code)?;
    let used = sqlx::query_scalar::<_, Uuid>(
        r#"
        update admin_mfa_recovery_codes
        set used_at = now()
        where tenant_id = $1
          and team_member_id = $2
          and code_hash = $3
          and used_at is null
          and revoked_at is null
        returning id
        "#,
    )
    .bind(tenant_id)
    .bind(team_member_id)
    .bind(code_hash)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?;

    Ok(used.is_some())
}

async fn verify_mfa_code_in_transaction(
    state: &AppState,
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    team_member_id: Uuid,
    secret: &str,
    code: &str,
) -> Result<(), AppError> {
    if verify_totp_code(secret, code)? {
        return Ok(());
    }

    let Some(recovery_code) = normalize_recovery_code(code) else {
        return Err(AppError::mfa_failed());
    };
    let code_hash = hash_token(&state.config.security.token_hash_pepper, &recovery_code)?;
    let used =
        consume_recovery_code_in_transaction(transaction, tenant_id, team_member_id, &code_hash)
            .await?;
    if !used {
        return Err(AppError::mfa_failed());
    }

    Ok(())
}

async fn check_mfa_rate_limit(
    state: &AppState,
    admin: &AdminContext,
    headers: &HeaderMap,
) -> Result<(), AppError> {
    let ip = rate_limit::client_ip(headers);
    rate_limit::check_fixed_window(
        state,
        rate_limit::mfa_key(&admin.team_member_id.to_string(), &ip),
        state.config.security.login_rate_limit_max,
        state.config.security.login_rate_limit_window_seconds,
        AppError::login_rate_limited,
    )
    .await
}

fn generate_base32_secret() -> String {
    let mut bytes = [0_u8; MFA_SECRET_BYTES];
    OsRng.fill_bytes(&mut bytes);
    base32_encode(&bytes)
}

fn generate_recovery_codes() -> Vec<String> {
    (0..RECOVERY_CODE_COUNT)
        .map(|_| generate_recovery_code())
        .collect()
}

fn hash_recovery_codes(
    state: &AppState,
    recovery_codes: &[String],
) -> Result<Vec<String>, AppError> {
    recovery_codes
        .iter()
        .map(|code| hash_token(&state.config.security.token_hash_pepper, code))
        .collect()
}

fn encrypt_secret_to_text(state: &AppState, secret: &str) -> Result<String, AppError> {
    let envelope = encrypt_bytes(&state.config.security.master_key, secret.as_bytes())?;

    serde_json::to_string(&envelope)
        .map_err(|error| AppError::crypto(format!("mfa secret serialization failed: {error}")))
}

fn decrypt_member_mfa_secret(state: &AppState, member: &TeamMember) -> Result<String, AppError> {
    let encrypted_secret = member
        .mfa_secret_encrypted
        .as_deref()
        .ok_or_else(|| AppError::config("mfa secret is not configured"))?;

    decrypt_secret_text(state, encrypted_secret)
}

fn decrypt_secret_text(state: &AppState, encrypted_secret: &str) -> Result<String, AppError> {
    let envelope: PrivateKeyEnvelope = serde_json::from_str(encrypted_secret)
        .map_err(|error| AppError::crypto(format!("mfa secret envelope invalid: {error}")))?;
    let plaintext = decrypt_bytes(&state.config.security.master_key, &envelope)?;

    String::from_utf8(plaintext)
        .map_err(|error| AppError::crypto(format!("mfa secret plaintext invalid: {error}")))
}

fn verify_totp_code(secret: &str, code: &str) -> Result<bool, AppError> {
    let Some(code) = normalize_totp_code(code) else {
        return Ok(false);
    };
    let secret_bytes = base32_decode(secret)?;
    let now = Utc::now().timestamp();
    let current_step = now / TOTP_PERIOD_SECONDS;

    for offset in -TOTP_WINDOW_STEPS..=TOTP_WINDOW_STEPS {
        let step = current_step + offset;
        if step < 0 {
            continue;
        }
        let expected = totp_code_at_step(&secret_bytes, step as u64, TOTP_DIGITS);
        if expected.as_bytes().ct_eq(code.as_bytes()).into() {
            return Ok(true);
        }
    }

    Ok(false)
}

fn totp_code_at_step(secret: &[u8], step: u64, digits: u32) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret);
    let digest = hmac::sign(&key, &step.to_be_bytes());
    let digest = digest.as_ref();
    let offset = (digest[digest.len() - 1] & 0x0f) as usize;
    let binary = (((digest[offset] & 0x7f) as u32) << 24)
        | ((digest[offset + 1] as u32) << 16)
        | ((digest[offset + 2] as u32) << 8)
        | (digest[offset + 3] as u32);
    let modulo = 10_u32.pow(digits);

    format!("{:0width$}", binary % modulo, width = digits as usize)
}

fn base32_encode(bytes: &[u8]) -> String {
    let mut output = String::with_capacity((bytes.len() * 8).div_ceil(5));
    let mut buffer = 0_u16;
    let mut bits_left = 0_u8;

    for byte in bytes {
        buffer = (buffer << 8) | u16::from(*byte);
        bits_left += 8;
        while bits_left >= 5 {
            let index = ((buffer >> (bits_left - 5)) & 0x1f) as usize;
            output.push(BASE32_ALPHABET[index] as char);
            bits_left -= 5;
        }
    }
    if bits_left > 0 {
        let index = ((buffer << (5 - bits_left)) & 0x1f) as usize;
        output.push(BASE32_ALPHABET[index] as char);
    }

    output
}

fn base32_decode(value: &str) -> Result<Vec<u8>, AppError> {
    let mut output = Vec::with_capacity(value.len() * 5 / 8);
    let mut buffer = 0_u32;
    let mut bits_left = 0_u8;

    for character in value.chars().filter(|character| *character != '=') {
        let value = base32_value(character)
            .ok_or_else(|| AppError::validation_failed("mfa secret is invalid"))?;
        buffer = (buffer << 5) | u32::from(value);
        bits_left += 5;
        if bits_left >= 8 {
            let byte = (buffer >> (bits_left - 8)) as u8;
            output.push(byte);
            bits_left -= 8;
            buffer &= (1_u32 << bits_left) - 1;
        }
    }

    Ok(output)
}

fn base32_value(character: char) -> Option<u8> {
    match character.to_ascii_uppercase() {
        'A'..='Z' => Some(character.to_ascii_uppercase() as u8 - b'A'),
        '2'..='7' => Some(character as u8 - b'2' + 26),
        _ => None,
    }
}

fn normalize_totp_code(code: &str) -> Option<String> {
    let code = code.trim();
    (code.len() == TOTP_DIGITS as usize && code.chars().all(|character| character.is_ascii_digit()))
        .then_some(code.to_owned())
}

fn normalize_recovery_code(code: &str) -> Option<String> {
    let compact = code
        .trim()
        .chars()
        .filter(|character| !character.is_ascii_whitespace() && *character != '-')
        .map(|character| character.to_ascii_uppercase())
        .collect::<String>();
    if compact.len() != 16
        || !compact
            .chars()
            .all(|character| matches!(character, 'A'..='Z' | '2'..='9'))
    {
        return None;
    }

    Some(format!(
        "{}-{}-{}-{}",
        &compact[0..4],
        &compact[4..8],
        &compact[8..12],
        &compact[12..16]
    ))
}

fn build_otpauth_url(issuer: &str, email: &str, secret: &str) -> String {
    let issuer = issuer.trim();
    let label = format!("{issuer}:{email}");

    format!(
        "otpauth://totp/{}?secret={}&issuer={}&algorithm=SHA1&digits={}&period={}",
        percent_encode(&label),
        secret,
        percent_encode(issuer),
        TOTP_DIGITS,
        TOTP_PERIOD_SECONDS
    )
}

fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}

async fn update_member_mfa_secret_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    team_member_id: Uuid,
    encrypted_secret: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        update team_members
        set
          mfa_enabled = false,
          mfa_secret_encrypted = $3,
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and deleted_at is null
        "#,
    )
    .bind(tenant_id)
    .bind(team_member_id)
    .bind(encrypted_secret)
    .execute(&mut **transaction)
    .await
    .map_err(map_db_error)?;

    Ok(())
}

async fn set_member_mfa_enabled_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    team_member_id: Uuid,
    enabled: bool,
) -> Result<TeamMember, AppError> {
    sqlx::query_as::<_, TeamMember>(
        r#"
        update team_members
        set
          mfa_enabled = $3,
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and deleted_at is null
        returning
          id,
          tenant_id,
          email,
          password_hash,
          name,
          phone,
          avatar,
          status,
          email_verified,
          mfa_enabled,
          mfa_secret_encrypted,
          last_login_at,
          last_login_ip::text as last_login_ip,
          created_at,
          updated_at,
          deleted_at
        "#,
    )
    .bind(tenant_id)
    .bind(team_member_id)
    .bind(enabled)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn disable_member_mfa_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    team_member_id: Uuid,
) -> Result<TeamMember, AppError> {
    sqlx::query_as::<_, TeamMember>(
        r#"
        update team_members
        set
          mfa_enabled = false,
          mfa_secret_encrypted = null,
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and deleted_at is null
        returning
          id,
          tenant_id,
          email,
          password_hash,
          name,
          phone,
          avatar,
          status,
          email_verified,
          mfa_enabled,
          mfa_secret_encrypted,
          last_login_at,
          last_login_ip::text as last_login_ip,
          created_at,
          updated_at,
          deleted_at
        "#,
    )
    .bind(tenant_id)
    .bind(team_member_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn replace_recovery_codes_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    team_member_id: Uuid,
    code_hashes: &[String],
) -> Result<(), AppError> {
    revoke_recovery_codes_in_transaction(transaction, tenant_id, team_member_id).await?;

    for code_hash in code_hashes {
        sqlx::query(
            r#"
            insert into admin_mfa_recovery_codes (
              id,
              tenant_id,
              team_member_id,
              code_hash
            )
            values ($1, $2, $3, $4)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(tenant_id)
        .bind(team_member_id)
        .bind(code_hash)
        .execute(&mut **transaction)
        .await
        .map_err(map_db_error)?;
    }

    Ok(())
}

async fn revoke_recovery_codes_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    team_member_id: Uuid,
) -> Result<u64, AppError> {
    sqlx::query(
        r#"
        update admin_mfa_recovery_codes
        set revoked_at = now()
        where tenant_id = $1
          and team_member_id = $2
          and used_at is null
          and revoked_at is null
        "#,
    )
    .bind(tenant_id)
    .bind(team_member_id)
    .execute(&mut **transaction)
    .await
    .map(|result| result.rows_affected())
    .map_err(map_db_error)
}

async fn consume_recovery_code_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    team_member_id: Uuid,
    code_hash: &str,
) -> Result<bool, AppError> {
    let used = sqlx::query_scalar::<_, Uuid>(
        r#"
        update admin_mfa_recovery_codes
        set used_at = now()
        where tenant_id = $1
          and team_member_id = $2
          and code_hash = $3
          and used_at is null
          and revoked_at is null
        returning id
        "#,
    )
    .bind(tenant_id)
    .bind(team_member_id)
    .bind(code_hash)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)?;

    Ok(used.is_some())
}

fn team_member_mfa_audit_json(member: &TeamMember) -> serde_json::Value {
    serde_json::json!({
        "id": member.id,
        "email": member.email,
        "mfa_enabled": member.mfa_enabled,
        "mfa_secret_configured": member.mfa_secret_encrypted.is_some(),
    })
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("auth mfa database error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{
        base32_decode, base32_encode, build_otpauth_url, normalize_recovery_code,
        normalize_totp_code, percent_encode, totp_code_at_step,
    };

    #[test]
    fn base32_round_trips_secret_bytes() {
        let bytes = b"12345678901234567890";
        let encoded = base32_encode(bytes);
        let decoded = base32_decode(&encoded).expect("base32 should decode");

        assert_eq!(decoded, bytes);
    }

    #[test]
    fn totp_matches_rfc6238_sha1_vector_truncated_to_six_digits() {
        let code = totp_code_at_step(b"12345678901234567890", 1, 6);

        assert_eq!(code, "287082");
    }

    #[test]
    fn recovery_code_normalizes_with_or_without_dashes() {
        assert_eq!(
            normalize_recovery_code("abcd efgh ijkl mn23").expect("recovery code"),
            "ABCD-EFGH-IJKL-MN23"
        );
        assert_eq!(
            normalize_recovery_code("abcd-efgh-ijkl-mn23").expect("recovery code"),
            "ABCD-EFGH-IJKL-MN23"
        );
    }

    #[test]
    fn totp_code_requires_six_digits() {
        assert_eq!(normalize_totp_code(" 123456 "), Some("123456".to_owned()));
        assert_eq!(normalize_totp_code("12345"), None);
        assert_eq!(normalize_totp_code("12345a"), None);
    }

    #[test]
    fn otpauth_url_percent_encodes_label_and_issuer() {
        assert_eq!(
            percent_encode("App Admin:me@example.com"),
            "App%20Admin%3Ame%40example.com"
        );
        assert!(build_otpauth_url("App Admin", "me@example.com", "SECRET")
            .contains("issuer=App%20Admin"));
    }
}
