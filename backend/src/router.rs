use axum::{
    extract::DefaultBodyLimit,
    middleware,
    routing::{delete, get, post, put},
    Router,
};

use crate::{
    http::{client_ip, request_id, security},
    metrics,
    modules::{
        ai, application, audit, auth, client_auth, customer, device, iam, license, notification,
        outbox, platform, release, secure_script, subscription, system, team, tenant,
    },
    state::AppState,
};

pub fn build(state: AppState) -> Router {
    let client_protected = Router::new()
        .route(
            "/api/client/auth/heartbeat",
            post(client_auth::heartbeat::heartbeat),
        )
        .route("/api/client/auth/verify", post(client_auth::verify::verify))
        .route("/api/client/auth/logout", post(client_auth::logout::logout))
        .route(
            "/api/client/releases/latest",
            get(release::client::latest_release),
        )
        .route(
            "/api/client/secure-scripts/versions",
            get(secure_script::client::list_versions),
        )
        .route(
            "/api/client/secure-scripts/fetch",
            post(secure_script::client::fetch_script),
        )
        .route(
            "/api/client/auth/email/verify/request",
            post(client_auth::email_verify::request_email_verify),
        )
        .route(
            "/api/client/devices/self",
            delete(device::client::unbind_self),
        )
        .route(
            "/api/client/devices/self/rotate-key",
            post(device::client::rotate_self_key),
        )
        .route(
            "/api/client/ai/v1/chat/completions",
            post(ai::gateway::client_chat_completions),
        )
        .route(
            "/api/client/ai/v1/embeddings",
            post(ai::gateway::client_embeddings),
        )
        .route(
            "/api/client/ai/v1/images/generations",
            post(ai::gateway::client_image_generations),
        )
        .route(
            "/api/client/ai/v1/models",
            get(ai::gateway::client_list_models),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            client_auth::signature::require_device_signature,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            client_auth::session::require_client_session,
        ));

    let protected = Router::new()
        .route("/api/auth/me", get(auth::me::me))
        .route("/api/auth/logout", post(auth::logout::logout))
        .route(
            "/api/auth/sessions",
            get(auth::admin_sessions::list_sessions),
        )
        .route(
            "/api/auth/sessions/{id}/revoke",
            post(auth::admin_sessions::revoke_session),
        )
        .route(
            "/api/auth/password",
            axum::routing::put(auth::password::change_password),
        )
        .route(
            "/api/auth/email/verify/request",
            post(auth::email_verify::request_email_verify),
        )
        .route("/api/auth/mfa/setup", post(auth::mfa::setup))
        .route("/api/auth/mfa/enable", post(auth::mfa::enable))
        .route("/api/auth/mfa/disable", post(auth::mfa::disable))
        .route(
            "/api/auth/mfa/recovery-codes/regenerate",
            post(auth::mfa::regenerate_recovery_codes),
        )
        .route(
            "/api/tenant",
            get(tenant::admin::get_tenant)
                .put(tenant::admin::update_tenant)
                .delete(tenant::admin::delete_tenant),
        )
        .route("/api/team/members", get(team::admin::list_members))
        .route("/api/team/invitations", post(team::admin::invite_member))
        .route(
            "/api/team/members/{id}/roles",
            axum::routing::put(team::admin::update_member_roles),
        )
        .route(
            "/api/team/members/{id}/disable",
            post(team::admin::disable_member),
        )
        .route(
            "/api/admin/roles",
            get(iam::admin::list_roles).post(iam::admin::create_role),
        )
        .route(
            "/api/admin/roles/{id}",
            put(iam::admin::update_role).delete(iam::admin::delete_role),
        )
        .route("/api/admin/permissions", get(iam::admin::list_permissions))
        .route(
            "/api/admin/ai/providers",
            get(ai::admin::list_ai_providers).post(ai::admin::create_ai_provider),
        )
        .route(
            "/api/admin/ai/providers/{id}",
            put(ai::admin::update_ai_provider),
        )
        .route(
            "/api/admin/ai/models",
            get(ai::admin::list_ai_models).post(ai::admin::create_ai_model),
        )
        .route("/api/admin/ai/models/{id}", put(ai::admin::update_ai_model))
        .route("/api/admin/ai/wallets", get(ai::admin::list_ai_wallets))
        .route(
            "/api/admin/ai/api-keys",
            get(ai::api_keys::list_ai_api_keys),
        )
        .route(
            "/api/admin/ai/api-keys/{id}",
            put(ai::api_keys::update_ai_api_key),
        )
        .route(
            "/api/admin/ai/customers/{id}/api-keys",
            post(ai::api_keys::create_ai_api_key),
        )
        .route(
            "/api/admin/ai/api-keys/{id}/revoke",
            post(ai::api_keys::revoke_ai_api_key),
        )
        .route(
            "/api/admin/ai/usage-records",
            get(ai::usage::list_ai_usage_records),
        )
        .route("/api/admin/ai/assets", get(ai::assets::list_ai_assets))
        .route(
            "/api/admin/ai/assets/{id}",
            delete(ai::assets::delete_ai_asset),
        )
        .route(
            "/api/admin/ai/customers/{id}/wallet/adjust",
            post(ai::admin::adjust_ai_wallet),
        )
        .route(
            "/api/admin/ai/customers/{id}/wallet/quota",
            put(ai::admin::update_ai_wallet_quota),
        )
        .route(
            "/api/admin/ai/customers/{id}/wallet/ledger",
            get(ai::admin::list_ai_wallet_ledger),
        )
        .route(
            "/api/admin/notification-channels",
            get(notification::admin::list_notification_channels)
                .post(notification::admin::create_notification_channel),
        )
        .route(
            "/api/admin/notification-channels/{id}",
            put(notification::admin::update_notification_channel),
        )
        .route(
            "/api/admin/notification-channels/{id}/test",
            post(notification::admin::test_notification_channel),
        )
        .route(
            "/api/admin/customers",
            get(customer::admin::list_customers).post(customer::admin::create_customer),
        )
        .route(
            "/api/admin/customers/{id}",
            axum::routing::put(customer::admin::update_customer),
        )
        .route(
            "/api/admin/customers/{id}/disable",
            post(customer::admin::disable_customer),
        )
        .route(
            "/api/admin/customers/{id}/reset-password",
            post(customer::admin::reset_customer_password),
        )
        .route(
            "/api/admin/apps",
            get(application::admin::list_applications).post(application::admin::create_application),
        )
        .route(
            "/api/admin/apps/{id}",
            axum::routing::put(application::admin::update_application)
                .get(application::admin::get_application),
        )
        .route(
            "/api/admin/apps/{id}/rotate-keys",
            post(application::admin::rotate_application_keys),
        )
        .route(
            "/api/admin/apps/{id}/signing-keys",
            get(application::admin::list_application_signing_keys),
        )
        .route(
            "/api/admin/apps/{id}/release-files",
            post(release::admin::register_release_file),
        )
        .route(
            "/api/admin/apps/{id}/release-files/upload",
            post(release::admin::upload_release_file).layer(DefaultBodyLimit::max(
                release::admin::MAX_RELEASE_UPLOAD_BYTES,
            )),
        )
        .route(
            "/api/admin/apps/{id}/releases",
            get(release::admin::list_releases).post(release::admin::create_release),
        )
        .route(
            "/api/admin/releases/{id}/publish",
            post(release::admin::publish_release),
        )
        .route(
            "/api/admin/releases/{id}/deprecate",
            post(release::admin::deprecate_release),
        )
        .route(
            "/api/admin/apps/{id}/secure-scripts",
            get(secure_script::admin::list_secure_scripts)
                .post(secure_script::admin::create_secure_script),
        )
        .route(
            "/api/admin/secure-scripts/{id}/content",
            post(secure_script::admin::update_secure_script_content),
        )
        .route(
            "/api/admin/secure-scripts/{id}/publish",
            post(secure_script::admin::publish_secure_script),
        )
        .route(
            "/api/admin/secure-scripts/{id}/deprecate",
            post(secure_script::admin::deprecate_secure_script),
        )
        .route("/api/admin/audit-logs", get(audit::admin::list_audit_logs))
        .route(
            "/api/admin/audit-logs/export",
            get(audit::admin::export_audit_logs),
        )
        .route(
            "/api/admin/audit-logs/{id}",
            get(audit::admin::get_audit_log),
        )
        .route(
            "/api/admin/system/settings",
            get(system::admin::list_system_settings),
        )
        .route(
            "/api/admin/system/settings/{key}",
            put(system::admin::update_system_setting),
        )
        .route(
            "/api/admin/system/email-settings",
            get(system::email::get_email_settings).put(system::email::update_email_settings),
        )
        .route(
            "/api/admin/system/email-settings/test",
            post(system::email::test_email_settings),
        )
        .route(
            "/api/admin/security/jwt-signing-keys",
            get(application::admin::list_global_jwt_signing_keys),
        )
        .route(
            "/api/admin/security/jwt-signing-keys/rotate",
            post(application::admin::rotate_global_jwt_signing_key),
        )
        .route(
            "/api/admin/outbox-events",
            get(outbox::admin::list_outbox_events),
        )
        .route(
            "/api/admin/outbox-events/{id}/retry",
            post(outbox::admin::retry_outbox_event),
        )
        .route(
            "/api/admin/licenses",
            get(license::admin::list_licenses).post(license::admin::create_license),
        )
        .route(
            "/api/admin/licenses/{id}/revoke",
            post(license::admin::revoke_license),
        )
        .route(
            "/api/admin/licenses/{id}/suspend",
            post(license::admin::suspend_license),
        )
        .route(
            "/api/admin/licenses/{id}/renew",
            post(license::admin::renew_license),
        )
        .route(
            "/api/admin/licenses/{id}/reset-devices",
            post(license::admin::reset_license_devices).layer(DefaultBodyLimit::max(
                license::admin::MAX_RESET_LICENSE_DEVICES_BODY_BYTES,
            )),
        )
        .route(
            "/api/admin/subscriptions",
            get(subscription::admin::list_subscriptions)
                .post(subscription::admin::create_subscription),
        )
        .route(
            "/api/admin/subscriptions/{id}/cancel",
            post(subscription::admin::cancel_subscription),
        )
        .route("/api/admin/devices", get(device::admin::list_devices))
        .route(
            "/api/admin/devices/{id}",
            get(device::admin::get_device).delete(device::admin::unbind_device),
        )
        .route(
            "/api/admin/devices/{id}/blacklist",
            post(device::admin::blacklist_device),
        )
        .route(
            "/api/admin/devices/{id}/unblacklist",
            post(device::admin::unblacklist_device),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::session::require_admin_session,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::csrf::require_csrf,
        ));

    Router::new()
        .route("/health", get(platform::health))
        .route("/healthz", get(platform::health))
        .route("/readyz", get(platform::readiness))
        .route("/metrics", get(metrics::scrape))
        .route(
            "/.well-known/jwks.json",
            get(application::jwks::global_jwks),
        )
        .route(
            "/api/client/apps/{app_key}/jwks",
            get(application::jwks::application_jwks),
        )
        .route("/api/auth/login", post(auth::login::login))
        .route(
            "/api/auth/refresh",
            post(auth::refresh::refresh).route_layer(middleware::from_fn_with_state(
                state.clone(),
                auth::csrf::require_csrf,
            )),
        )
        .route(
            "/api/client/auth/activate",
            post(client_auth::activate::activate),
        )
        .route("/api/client/auth/login", post(client_auth::login::login))
        .route(
            "/api/client/auth/email/verify/confirm",
            post(client_auth::email_verify::confirm_email_verify),
        )
        .route(
            "/api/client/auth/password/reset/confirm",
            post(client_auth::password::confirm_password_reset),
        )
        .route(
            "/api/internal/alertmanager/webhook",
            post(notification::alertmanager::receive_alertmanager_webhook),
        )
        .route("/api/ai/assets/{id}", get(ai::gateway::get_asset))
        .route("/v1/chat/completions", post(ai::gateway::chat_completions))
        .route("/v1/embeddings", post(ai::gateway::embeddings))
        .route(
            "/v1/images/generations",
            post(ai::gateway::image_generations),
        )
        .route("/v1/models", get(ai::gateway::list_models))
        .route(
            "/api/auth/password/reset/request",
            post(auth::password::request_password_reset),
        )
        .route(
            "/api/auth/password/reset/confirm",
            post(auth::password::confirm_password_reset),
        )
        .route(
            "/api/auth/email/verify/confirm",
            post(auth::email_verify::confirm_email_verify),
        )
        .route(
            "/api/team/invitations/accept",
            post(team::invitation::accept_invitation),
        )
        .route(
            "/api/client/auth/refresh",
            post(client_auth::refresh::refresh),
        )
        .route(
            "/api/client/releases/download/{file_name}",
            get(release::client::download_release),
        )
        .merge(client_protected)
        .merge(protected)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            client_ip::attach_client_ip,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security::apply_security,
        ))
        .layer(middleware::from_fn(request_id::attach))
        .layer(middleware::from_fn(metrics::record_http_metrics))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        net::{IpAddr, Ipv4Addr},
        path::PathBuf,
        sync::Arc,
        time::Duration,
    };

    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use serde_json::Value;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use crate::{
        config::{
            AlertingConfig, AppConfig, AppSection, DatabaseConfig, EmailConfig,
            ObjectStorageConfig, RedisConfig, SecurityConfig, ServerConfig,
        },
        state::AppState,
        storage::LocalObjectStore,
    };

    use super::build;

    #[test]
    fn openapi_lists_every_registered_route() {
        let registered = registered_routes(include_str!("router.rs"));
        let documented = openapi_paths(include_str!("../openapi.yaml"));

        let missing = registered
            .difference(&documented)
            .cloned()
            .collect::<Vec<_>>();
        let extra = documented
            .difference(&registered)
            .cloned()
            .collect::<Vec<_>>();

        assert!(
            missing.is_empty(),
            "routes missing from OpenAPI: {missing:?}"
        );
        assert!(extra.is_empty(), "OpenAPI paths not registered: {extra:?}");
    }

    #[tokio::test]
    async fn client_rotate_key_route_is_registered_and_session_protected() {
        let app = build(test_state());
        let request = Request::builder()
            .method("POST")
            .uri("/api/client/devices/self/rotate-key")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"device_public_key":"unused"}"#))
            .expect("request should build");

        let response = app.oneshot(request).await.expect("router should respond");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("body should read");
        let body: Value = serde_json::from_slice(&body).expect("body should be json");

        assert_eq!(body["code"], 40100);
        assert_eq!(body["message"], "unauthenticated");
    }

    fn registered_routes(source: &str) -> HashSet<String> {
        let source = source.split("#[cfg(test)]").next().unwrap_or(source);
        let mut routes = HashSet::new();
        let mut remaining = source;

        while let Some(index) = remaining.find(".route(") {
            remaining = &remaining[index + ".route(".len()..];
            let trimmed = remaining.trim_start();
            let Some(after_quote) = trimmed.strip_prefix('"') else {
                continue;
            };
            let Some(end) = after_quote.find('"') else {
                continue;
            };

            routes.insert(after_quote[..end].to_owned());
            remaining = &after_quote[end + 1..];
        }

        routes
    }

    fn openapi_paths(source: &str) -> HashSet<String> {
        let mut paths = HashSet::new();
        let mut in_paths = false;

        for line in source.lines() {
            if line == "paths:" {
                in_paths = true;
                continue;
            }
            if line == "components:" {
                break;
            }
            if !in_paths {
                continue;
            }

            let Some(path) = line.strip_prefix("  /") else {
                continue;
            };
            let Some(path) = path.strip_suffix(':') else {
                continue;
            };

            paths.insert(format!("/{path}"));
        }

        paths
    }

    fn test_state() -> AppState {
        let db = PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://postgres:postgres@127.0.0.1:1/user_admin_test")
            .expect("lazy db pool should build");
        let redis =
            redis::Client::open("redis://127.0.0.1:1/").expect("redis client should build lazily");
        let object_store = Arc::new(LocalObjectStore::new(PathBuf::from(".")));

        AppState {
            config: Arc::new(test_config()),
            db,
            redis,
            object_store,
        }
    }

    fn test_config() -> AppConfig {
        AppConfig {
            app: AppSection {
                env: "development".to_owned(),
                name: "user-admin-backend".to_owned(),
                log_level: "info".to_owned(),
                base_url: None,
            },
            server: ServerConfig {
                host: IpAddr::V4(Ipv4Addr::LOCALHOST),
                port: 0,
            },
            database: DatabaseConfig {
                url: "postgres://postgres:postgres@127.0.0.1:1/user_admin_test".to_owned(),
                max_connections: 1,
                connect_timeout: Duration::from_secs(1),
            },
            email: EmailConfig {
                outbox_worker_enabled: false,
                smtp_host: None,
                smtp_port: 587,
                smtp_user: None,
                smtp_password: None,
                smtp_from: None,
                outbox_poll_interval: Duration::from_secs(15),
                outbox_processing_timeout: Duration::from_secs(60),
                outbox_batch_size: 50,
                outbox_max_attempts: 5,
            },
            alerting: AlertingConfig {
                webhook_token: Some("alertmanager-webhook-token-32-bytes".to_owned()),
                delivery_timeout: Duration::from_secs(1),
            },
            redis: RedisConfig {
                url: "redis://127.0.0.1:1/".to_owned(),
                connect_timeout: Duration::from_secs(1),
            },
            security: SecurityConfig {
                session_secret: "session-secret-value-32-bytes-long".to_owned(),
                token_hash_pepper: "token-hash-pepper-value-32-bytes".to_owned(),
                refresh_token_pepper: "refresh-token-pepper-value-32-bytes".to_owned(),
                csrf_secret: "csrf-secret-value-32-bytes-long".to_owned(),
                master_key: [1_u8; 32],
                jwt_issuer: "https://api.example.com".to_owned(),
                jwt_audience: "client-sdk".to_owned(),
                cookie_secure: false,
                admin_session_ttl_seconds: 86_400,
                client_access_token_ttl_seconds: 900,
                client_refresh_token_ttl_seconds: 2_592_000,
                client_session_ttl_seconds: 2_592_000,
                download_token_ttl_seconds: 300,
                login_rate_limit_max: 10,
                login_rate_limit_window_seconds: 300,
                activation_rate_limit_max: 20,
                activation_rate_limit_window_seconds: 300,
                refresh_rate_limit_max: 60,
                refresh_rate_limit_window_seconds: 300,
                heartbeat_rate_limit_max: 120,
                heartbeat_rate_limit_window_seconds: 60,
                client_action_rate_limit_max: 120,
                client_action_rate_limit_window_seconds: 60,
                download_rate_limit_max: 30,
                download_rate_limit_window_seconds: 300,
                ai_gateway_rate_limit_max: 120,
                ai_gateway_rate_limit_window_seconds: 60,
                allowed_origins: vec!["https://admin.example.com".to_owned()],
                trusted_proxies: Vec::new(),
            },
            object_storage: ObjectStorageConfig {
                mode: "local".to_owned(),
                local_root: Some(PathBuf::from(".")),
                endpoint: None,
                bucket: None,
                access_key: None,
                secret_key: None,
                region: "auto".to_owned(),
            },
        }
    }
}
