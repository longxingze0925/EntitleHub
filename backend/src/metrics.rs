use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        OnceLock,
    },
    time::Instant,
};

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::state::AppState;

const DURATION_BUCKETS_SECONDS: [f64; 10] =
    [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0];
const NOTIFICATION_KIND_LABELS: [&str; 4] = ["webhook", "email", "pagerduty", "unknown"];
const AI_GATEWAY_ENDPOINT_LABELS: [&str; 7] = [
    "chat_completions",
    "image_generations",
    "video_generations",
    "embeddings",
    "models",
    "assets",
    "unknown",
];
const AI_GATEWAY_STATUS_LABELS: [&str; 6] = [
    "success",
    "provider_error",
    "client_error",
    "rate_limited",
    "error",
    "replay",
];

static METRICS: OnceLock<AppMetrics> = OnceLock::new();

pub struct AppMetrics {
    http_requests_total: AtomicU64,
    http_errors_total: AtomicU64,
    http_server_errors_total: AtomicU64,
    rate_limited_total: AtomicU64,
    login_failures_total: AtomicU64,
    client_refresh_failures_total: AtomicU64,
    nonce_replay_total: AtomicU64,
    file_downloads_total: AtomicU64,
    worker_jobs_failed_total: AtomicU64,
    redis_errors_total: AtomicU64,
    notification_delivery_success_total: [AtomicU64; NOTIFICATION_KIND_LABELS.len()],
    notification_delivery_failure_total: [AtomicU64; NOTIFICATION_KIND_LABELS.len()],
    notification_delivery_duration_seconds_count: [AtomicU64; NOTIFICATION_KIND_LABELS.len()],
    notification_delivery_duration_micros_sum: [AtomicU64; NOTIFICATION_KIND_LABELS.len()],
    notification_delivery_duration_seconds_buckets:
        [[AtomicU64; DURATION_BUCKETS_SECONDS.len()]; NOTIFICATION_KIND_LABELS.len()],
    ai_gateway_requests_total:
        [[AtomicU64; AI_GATEWAY_STATUS_LABELS.len()]; AI_GATEWAY_ENDPOINT_LABELS.len()],
    ai_gateway_charged_minor_total: [AtomicU64; AI_GATEWAY_ENDPOINT_LABELS.len()],
    ai_gateway_provider_duration_seconds_count: [AtomicU64; AI_GATEWAY_ENDPOINT_LABELS.len()],
    ai_gateway_provider_duration_micros_sum: [AtomicU64; AI_GATEWAY_ENDPOINT_LABELS.len()],
    ai_gateway_provider_duration_seconds_buckets:
        [[AtomicU64; DURATION_BUCKETS_SECONDS.len()]; AI_GATEWAY_ENDPOINT_LABELS.len()],
    ai_gateway_asset_cache_failures_total: AtomicU64,
    ai_gateway_idempotency_replays_total: [AtomicU64; AI_GATEWAY_ENDPOINT_LABELS.len()],
    http_request_duration_seconds_count: AtomicU64,
    http_request_duration_micros_sum: AtomicU64,
    http_request_duration_seconds_buckets: [AtomicU64; DURATION_BUCKETS_SECONDS.len()],
}

impl Default for AppMetrics {
    fn default() -> Self {
        Self {
            http_requests_total: AtomicU64::new(0),
            http_errors_total: AtomicU64::new(0),
            http_server_errors_total: AtomicU64::new(0),
            rate_limited_total: AtomicU64::new(0),
            login_failures_total: AtomicU64::new(0),
            client_refresh_failures_total: AtomicU64::new(0),
            nonce_replay_total: AtomicU64::new(0),
            file_downloads_total: AtomicU64::new(0),
            worker_jobs_failed_total: AtomicU64::new(0),
            redis_errors_total: AtomicU64::new(0),
            notification_delivery_success_total: std::array::from_fn(|_| AtomicU64::new(0)),
            notification_delivery_failure_total: std::array::from_fn(|_| AtomicU64::new(0)),
            notification_delivery_duration_seconds_count: std::array::from_fn(|_| {
                AtomicU64::new(0)
            }),
            notification_delivery_duration_micros_sum: std::array::from_fn(|_| AtomicU64::new(0)),
            notification_delivery_duration_seconds_buckets: std::array::from_fn(|_| {
                std::array::from_fn(|_| AtomicU64::new(0))
            }),
            ai_gateway_requests_total: std::array::from_fn(|_| {
                std::array::from_fn(|_| AtomicU64::new(0))
            }),
            ai_gateway_charged_minor_total: std::array::from_fn(|_| AtomicU64::new(0)),
            ai_gateway_provider_duration_seconds_count: std::array::from_fn(|_| AtomicU64::new(0)),
            ai_gateway_provider_duration_micros_sum: std::array::from_fn(|_| AtomicU64::new(0)),
            ai_gateway_provider_duration_seconds_buckets: std::array::from_fn(|_| {
                std::array::from_fn(|_| AtomicU64::new(0))
            }),
            ai_gateway_asset_cache_failures_total: AtomicU64::new(0),
            ai_gateway_idempotency_replays_total: std::array::from_fn(|_| AtomicU64::new(0)),
            http_request_duration_seconds_count: AtomicU64::new(0),
            http_request_duration_micros_sum: AtomicU64::new(0),
            http_request_duration_seconds_buckets: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
}

impl AppMetrics {
    fn record_http_response(
        &self,
        status: axum::http::StatusCode,
        duration: std::time::Duration,
        path: &str,
    ) {
        self.http_requests_total.fetch_add(1, Ordering::Relaxed);
        if status.as_u16() >= 400 {
            self.http_errors_total.fetch_add(1, Ordering::Relaxed);
        }
        if status.as_u16() >= 500 {
            self.http_server_errors_total
                .fetch_add(1, Ordering::Relaxed);
        }
        if status == axum::http::StatusCode::TOO_MANY_REQUESTS {
            self.rate_limited_total.fetch_add(1, Ordering::Relaxed);
        }
        if is_login_path(path) && status.as_u16() >= 400 && status.as_u16() != 429 {
            self.login_failures_total.fetch_add(1, Ordering::Relaxed);
        }
        if path == "/api/client/auth/refresh" && status.as_u16() >= 400 && status.as_u16() != 429 {
            self.client_refresh_failures_total
                .fetch_add(1, Ordering::Relaxed);
        }
        if path.starts_with("/api/client/releases/download/") && status.is_success() {
            self.file_downloads_total.fetch_add(1, Ordering::Relaxed);
        }

        let duration_seconds = duration.as_secs_f64();
        for (index, bucket) in DURATION_BUCKETS_SECONDS.iter().enumerate() {
            if duration_seconds <= *bucket {
                self.http_request_duration_seconds_buckets[index].fetch_add(1, Ordering::Relaxed);
            }
        }
        self.http_request_duration_seconds_count
            .fetch_add(1, Ordering::Relaxed);
        self.http_request_duration_micros_sum.fetch_add(
            duration.as_micros().min(u128::from(u64::MAX)) as u64,
            Ordering::Relaxed,
        );
    }

    fn record_notification_delivery(
        &self,
        kind: &str,
        status: NotificationDeliveryStatus,
        duration: std::time::Duration,
    ) {
        let kind_index = notification_kind_index(kind);
        match status {
            NotificationDeliveryStatus::Success => {
                self.notification_delivery_success_total[kind_index].fetch_add(1, Ordering::Relaxed)
            }
            NotificationDeliveryStatus::Failure => {
                self.notification_delivery_failure_total[kind_index].fetch_add(1, Ordering::Relaxed)
            }
        };

        let duration_seconds = duration.as_secs_f64();
        for (bucket_index, bucket) in DURATION_BUCKETS_SECONDS.iter().enumerate() {
            if duration_seconds <= *bucket {
                self.notification_delivery_duration_seconds_buckets[kind_index][bucket_index]
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        self.notification_delivery_duration_seconds_count[kind_index]
            .fetch_add(1, Ordering::Relaxed);
        self.notification_delivery_duration_micros_sum[kind_index].fetch_add(
            duration.as_micros().min(u128::from(u64::MAX)) as u64,
            Ordering::Relaxed,
        );
    }

    fn record_ai_gateway_request(&self, endpoint: &str, status: AiGatewayRequestStatus) {
        let endpoint_index = ai_gateway_endpoint_index(endpoint);
        let status_index = status.index();
        self.ai_gateway_requests_total[endpoint_index][status_index]
            .fetch_add(1, Ordering::Relaxed);
    }

    fn record_ai_gateway_charged(&self, endpoint: &str, charged_minor: i64) {
        if charged_minor <= 0 {
            return;
        }
        let endpoint_index = ai_gateway_endpoint_index(endpoint);
        self.ai_gateway_charged_minor_total[endpoint_index]
            .fetch_add(charged_minor as u64, Ordering::Relaxed);
    }

    fn record_ai_gateway_provider_duration(&self, endpoint: &str, duration: std::time::Duration) {
        let endpoint_index = ai_gateway_endpoint_index(endpoint);
        let duration_seconds = duration.as_secs_f64();
        for (bucket_index, bucket) in DURATION_BUCKETS_SECONDS.iter().enumerate() {
            if duration_seconds <= *bucket {
                self.ai_gateway_provider_duration_seconds_buckets[endpoint_index][bucket_index]
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        self.ai_gateway_provider_duration_seconds_count[endpoint_index]
            .fetch_add(1, Ordering::Relaxed);
        self.ai_gateway_provider_duration_micros_sum[endpoint_index].fetch_add(
            duration.as_micros().min(u128::from(u64::MAX)) as u64,
            Ordering::Relaxed,
        );
    }

    fn record_ai_gateway_idempotency_replay(&self, endpoint: &str) {
        let endpoint_index = ai_gateway_endpoint_index(endpoint);
        self.ai_gateway_idempotency_replays_total[endpoint_index].fetch_add(1, Ordering::Relaxed);
    }
}

pub enum NotificationDeliveryStatus {
    Success,
    Failure,
}

#[derive(Debug, Clone, Copy)]
pub enum AiGatewayRequestStatus {
    Success,
    ProviderError,
    ClientError,
    RateLimited,
    Error,
    Replay,
}

impl AiGatewayRequestStatus {
    fn index(self) -> usize {
        match self {
            Self::Success => 0,
            Self::ProviderError => 1,
            Self::ClientError => 2,
            Self::RateLimited => 3,
            Self::Error => 4,
            Self::Replay => 5,
        }
    }
}

pub async fn record_http_metrics(request: Request, next: Next) -> Response {
    let started_at = Instant::now();
    let path = request.uri().path().to_owned();
    let response = next.run(request).await;

    metrics().record_http_response(response.status(), started_at.elapsed(), &path);

    response
}

pub fn record_nonce_replay() {
    metrics().nonce_replay_total.fetch_add(1, Ordering::Relaxed);
}

pub fn record_worker_job_failed() {
    metrics()
        .worker_jobs_failed_total
        .fetch_add(1, Ordering::Relaxed);
}

pub fn record_redis_error() {
    metrics().redis_errors_total.fetch_add(1, Ordering::Relaxed);
}

pub fn record_notification_delivery(
    kind: &str,
    status: NotificationDeliveryStatus,
    duration: std::time::Duration,
) {
    metrics().record_notification_delivery(kind, status, duration);
}

pub fn record_ai_gateway_request(endpoint: &str, status: AiGatewayRequestStatus) {
    metrics().record_ai_gateway_request(endpoint, status);
}

pub fn record_ai_gateway_charged(endpoint: &str, charged_minor: i64) {
    metrics().record_ai_gateway_charged(endpoint, charged_minor);
}

pub fn record_ai_gateway_provider_duration(endpoint: &str, duration: std::time::Duration) {
    metrics().record_ai_gateway_provider_duration(endpoint, duration);
}

pub fn record_ai_gateway_asset_cache_failure() {
    metrics()
        .ai_gateway_asset_cache_failures_total
        .fetch_add(1, Ordering::Relaxed);
}

pub fn record_ai_gateway_idempotency_replay(endpoint: &str) {
    metrics().record_ai_gateway_idempotency_replay(endpoint);
}

pub async fn scrape(State(state): State<AppState>) -> impl IntoResponse {
    (
        [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
        render_prometheus(Some(&state.db)),
    )
}

fn metrics() -> &'static AppMetrics {
    METRICS.get_or_init(AppMetrics::default)
}

fn render_prometheus(db_pool: Option<&sqlx::PgPool>) -> String {
    let metrics = metrics();
    let requests_total = metrics.http_requests_total.load(Ordering::Relaxed);
    let errors_total = metrics.http_errors_total.load(Ordering::Relaxed);
    let server_errors_total = metrics.http_server_errors_total.load(Ordering::Relaxed);
    let rate_limited_total = metrics.rate_limited_total.load(Ordering::Relaxed);
    let login_failures_total = metrics.login_failures_total.load(Ordering::Relaxed);
    let client_refresh_failures_total = metrics
        .client_refresh_failures_total
        .load(Ordering::Relaxed);
    let nonce_replay_total = metrics.nonce_replay_total.load(Ordering::Relaxed);
    let file_downloads_total = metrics.file_downloads_total.load(Ordering::Relaxed);
    let worker_jobs_failed_total = metrics.worker_jobs_failed_total.load(Ordering::Relaxed);
    let redis_errors_total = metrics.redis_errors_total.load(Ordering::Relaxed);
    let duration_count = metrics
        .http_request_duration_seconds_count
        .load(Ordering::Relaxed);
    let duration_sum = metrics
        .http_request_duration_micros_sum
        .load(Ordering::Relaxed) as f64
        / 1_000_000.0;

    let mut output = String::new();
    output.push_str("# HELP http_requests_total Total HTTP requests observed.\n");
    output.push_str("# TYPE http_requests_total counter\n");
    output.push_str(&format!("http_requests_total {requests_total}\n"));
    output.push_str("# HELP http_errors_total Total HTTP responses with status code >= 400.\n");
    output.push_str("# TYPE http_errors_total counter\n");
    output.push_str(&format!("http_errors_total {errors_total}\n"));
    output.push_str(
        "# HELP http_server_errors_total Total HTTP responses with status code >= 500.\n",
    );
    output.push_str("# TYPE http_server_errors_total counter\n");
    output.push_str(&format!("http_server_errors_total {server_errors_total}\n"));
    output.push_str("# HELP rate_limited_total Total HTTP responses rejected by rate limiting.\n");
    output.push_str("# TYPE rate_limited_total counter\n");
    output.push_str(&format!("rate_limited_total {rate_limited_total}\n"));
    output.push_str("# HELP login_failures_total Total failed admin or client login attempts.\n");
    output.push_str("# TYPE login_failures_total counter\n");
    output.push_str(&format!("login_failures_total {login_failures_total}\n"));
    output.push_str("# HELP client_refresh_failures_total Total failed client refresh attempts.\n");
    output.push_str("# TYPE client_refresh_failures_total counter\n");
    output.push_str(&format!(
        "client_refresh_failures_total {client_refresh_failures_total}\n"
    ));
    output.push_str("# HELP nonce_replay_total Total rejected device signature nonce replays.\n");
    output.push_str("# TYPE nonce_replay_total counter\n");
    output.push_str(&format!("nonce_replay_total {nonce_replay_total}\n"));
    output.push_str("# HELP file_downloads_total Total successful release file downloads.\n");
    output.push_str("# TYPE file_downloads_total counter\n");
    output.push_str(&format!("file_downloads_total {file_downloads_total}\n"));
    output.push_str("# HELP worker_jobs_failed_total Total worker jobs marked failed.\n");
    output.push_str("# TYPE worker_jobs_failed_total counter\n");
    output.push_str(&format!(
        "worker_jobs_failed_total {worker_jobs_failed_total}\n"
    ));
    output.push_str(
        "# HELP redis_errors_total Total Redis operation failures observed by the backend.\n",
    );
    output.push_str("# TYPE redis_errors_total counter\n");
    output.push_str(&format!("redis_errors_total {redis_errors_total}\n"));
    output.push_str(
        "# HELP notification_delivery_total Total notification channel delivery attempts by kind and status.\n",
    );
    output.push_str("# TYPE notification_delivery_total counter\n");
    for (kind_index, kind_label) in NOTIFICATION_KIND_LABELS.iter().enumerate() {
        let success_total =
            metrics.notification_delivery_success_total[kind_index].load(Ordering::Relaxed);
        let failure_total =
            metrics.notification_delivery_failure_total[kind_index].load(Ordering::Relaxed);
        output.push_str(&format!(
            "notification_delivery_total{{kind=\"{kind_label}\",status=\"success\"}} {success_total}\n"
        ));
        output.push_str(&format!(
            "notification_delivery_total{{kind=\"{kind_label}\",status=\"failure\"}} {failure_total}\n"
        ));
    }
    output.push_str(
        "# HELP notification_delivery_failures_total Total failed notification channel delivery attempts by kind.\n",
    );
    output.push_str("# TYPE notification_delivery_failures_total counter\n");
    for (kind_index, kind_label) in NOTIFICATION_KIND_LABELS.iter().enumerate() {
        let failure_total =
            metrics.notification_delivery_failure_total[kind_index].load(Ordering::Relaxed);
        output.push_str(&format!(
            "notification_delivery_failures_total{{kind=\"{kind_label}\"}} {failure_total}\n"
        ));
    }
    output.push_str(
        "# HELP notification_delivery_duration_seconds Notification channel delivery duration histogram by kind.\n",
    );
    output.push_str("# TYPE notification_delivery_duration_seconds histogram\n");
    for (kind_index, kind_label) in NOTIFICATION_KIND_LABELS.iter().enumerate() {
        let duration_count = metrics.notification_delivery_duration_seconds_count[kind_index]
            .load(Ordering::Relaxed);
        let duration_sum = metrics.notification_delivery_duration_micros_sum[kind_index]
            .load(Ordering::Relaxed) as f64
            / 1_000_000.0;
        for (bucket_index, bucket) in DURATION_BUCKETS_SECONDS.iter().enumerate() {
            let value = metrics.notification_delivery_duration_seconds_buckets[kind_index]
                [bucket_index]
                .load(Ordering::Relaxed);
            output.push_str(&format!(
                "notification_delivery_duration_seconds_bucket{{kind=\"{kind_label}\",le=\"{bucket}\"}} {value}\n"
            ));
        }
        output.push_str(&format!(
            "notification_delivery_duration_seconds_bucket{{kind=\"{kind_label}\",le=\"+Inf\"}} {duration_count}\n"
        ));
        output.push_str(&format!(
            "notification_delivery_duration_seconds_sum{{kind=\"{kind_label}\"}} {duration_sum:.6}\n"
        ));
        output.push_str(&format!(
            "notification_delivery_duration_seconds_count{{kind=\"{kind_label}\"}} {duration_count}\n"
        ));
    }
    output.push_str(
        "# HELP ai_gateway_requests_total Total AI gateway requests by endpoint and status.\n",
    );
    output.push_str("# TYPE ai_gateway_requests_total counter\n");
    for (endpoint_index, endpoint_label) in AI_GATEWAY_ENDPOINT_LABELS.iter().enumerate() {
        for (status_index, status_label) in AI_GATEWAY_STATUS_LABELS.iter().enumerate() {
            let value = metrics.ai_gateway_requests_total[endpoint_index][status_index]
                .load(Ordering::Relaxed);
            output.push_str(&format!(
                "ai_gateway_requests_total{{endpoint=\"{endpoint_label}\",status=\"{status_label}\"}} {value}\n"
            ));
        }
    }
    output.push_str(
        "# HELP ai_gateway_charged_minor_total Total minor currency units charged by AI gateway endpoint.\n",
    );
    output.push_str("# TYPE ai_gateway_charged_minor_total counter\n");
    for (endpoint_index, endpoint_label) in AI_GATEWAY_ENDPOINT_LABELS.iter().enumerate() {
        let value = metrics.ai_gateway_charged_minor_total[endpoint_index].load(Ordering::Relaxed);
        output.push_str(&format!(
            "ai_gateway_charged_minor_total{{endpoint=\"{endpoint_label}\"}} {value}\n"
        ));
    }
    output.push_str(
        "# HELP ai_gateway_provider_duration_seconds AI provider request duration histogram by gateway endpoint.\n",
    );
    output.push_str("# TYPE ai_gateway_provider_duration_seconds histogram\n");
    for (endpoint_index, endpoint_label) in AI_GATEWAY_ENDPOINT_LABELS.iter().enumerate() {
        let duration_count = metrics.ai_gateway_provider_duration_seconds_count[endpoint_index]
            .load(Ordering::Relaxed);
        let duration_sum = metrics.ai_gateway_provider_duration_micros_sum[endpoint_index]
            .load(Ordering::Relaxed) as f64
            / 1_000_000.0;
        for (bucket_index, bucket) in DURATION_BUCKETS_SECONDS.iter().enumerate() {
            let value = metrics.ai_gateway_provider_duration_seconds_buckets[endpoint_index]
                [bucket_index]
                .load(Ordering::Relaxed);
            output.push_str(&format!(
                "ai_gateway_provider_duration_seconds_bucket{{endpoint=\"{endpoint_label}\",le=\"{bucket}\"}} {value}\n"
            ));
        }
        output.push_str(&format!(
            "ai_gateway_provider_duration_seconds_bucket{{endpoint=\"{endpoint_label}\",le=\"+Inf\"}} {duration_count}\n"
        ));
        output.push_str(&format!(
            "ai_gateway_provider_duration_seconds_sum{{endpoint=\"{endpoint_label}\"}} {duration_sum:.6}\n"
        ));
        output.push_str(&format!(
            "ai_gateway_provider_duration_seconds_count{{endpoint=\"{endpoint_label}\"}} {duration_count}\n"
        ));
    }
    let asset_cache_failures_total = metrics
        .ai_gateway_asset_cache_failures_total
        .load(Ordering::Relaxed);
    output.push_str(
        "# HELP ai_gateway_asset_cache_failures_total Total AI generated asset cache failures.\n",
    );
    output.push_str("# TYPE ai_gateway_asset_cache_failures_total counter\n");
    output.push_str(&format!(
        "ai_gateway_asset_cache_failures_total {asset_cache_failures_total}\n"
    ));
    output.push_str(
        "# HELP ai_gateway_idempotency_replays_total Total AI gateway idempotency replay responses by endpoint.\n",
    );
    output.push_str("# TYPE ai_gateway_idempotency_replays_total counter\n");
    for (endpoint_index, endpoint_label) in AI_GATEWAY_ENDPOINT_LABELS.iter().enumerate() {
        let value =
            metrics.ai_gateway_idempotency_replays_total[endpoint_index].load(Ordering::Relaxed);
        output.push_str(&format!(
            "ai_gateway_idempotency_replays_total{{endpoint=\"{endpoint_label}\"}} {value}\n"
        ));
    }
    if let Some(pool) = db_pool {
        output.push_str("# HELP db_pool_connections Current database pool connections.\n");
        output.push_str("# TYPE db_pool_connections gauge\n");
        output.push_str(&format!("db_pool_connections {}\n", pool.size()));
        output
            .push_str("# HELP db_pool_idle_connections Current idle database pool connections.\n");
        output.push_str("# TYPE db_pool_idle_connections gauge\n");
        output.push_str(&format!("db_pool_idle_connections {}\n", pool.num_idle()));
    }
    output.push_str("# HELP http_request_duration_seconds HTTP request duration histogram.\n");
    output.push_str("# TYPE http_request_duration_seconds histogram\n");
    for (index, bucket) in DURATION_BUCKETS_SECONDS.iter().enumerate() {
        let value = metrics.http_request_duration_seconds_buckets[index].load(Ordering::Relaxed);
        output.push_str(&format!(
            "http_request_duration_seconds_bucket{{le=\"{bucket}\"}} {value}\n"
        ));
    }
    output.push_str(&format!(
        "http_request_duration_seconds_bucket{{le=\"+Inf\"}} {duration_count}\n"
    ));
    output.push_str(&format!(
        "http_request_duration_seconds_sum {duration_sum:.6}\n"
    ));
    output.push_str(&format!(
        "http_request_duration_seconds_count {duration_count}\n"
    ));

    output
}

fn is_login_path(path: &str) -> bool {
    matches!(path, "/api/auth/login" | "/api/client/auth/login")
}

fn notification_kind_index(kind: &str) -> usize {
    match kind {
        "webhook" => 0,
        "email" => 1,
        "pagerduty" => 2,
        _ => 3,
    }
}

fn ai_gateway_endpoint_index(endpoint: &str) -> usize {
    match endpoint {
        "/v1/chat/completions"
        | "/api/server/ai/v1/chat/completions"
        | "/api/client/ai/v1/chat/completions"
        | "chat_completions" => 0,
        "/v1/images/generations"
        | "/api/server/ai/v1/images/generations"
        | "/api/client/ai/v1/images/generations"
        | "image_generations" => 1,
        "/v1/videos/generations"
        | "/api/server/ai/v1/videos/generations"
        | "/api/client/ai/v1/videos/generations"
        | "video_generations" => 2,
        "/v1/embeddings"
        | "/api/server/ai/v1/embeddings"
        | "/api/client/ai/v1/embeddings"
        | "embeddings" => 3,
        "/v1/models" | "/api/server/ai/v1/models" | "/api/client/ai/v1/models" | "models" => 4,
        "/api/ai/assets/{id}" | "assets" => 5,
        _ => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        metrics, record_ai_gateway_asset_cache_failure, record_ai_gateway_charged,
        record_ai_gateway_idempotency_replay, record_ai_gateway_provider_duration,
        record_ai_gateway_request, record_nonce_replay, record_notification_delivery,
        record_redis_error, record_worker_job_failed, render_prometheus, AiGatewayRequestStatus,
        NotificationDeliveryStatus,
    };

    #[test]
    fn prometheus_text_contains_required_metrics() {
        metrics().record_http_response(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            std::time::Duration::from_millis(12),
            "/api/system/settings",
        );
        metrics().record_http_response(
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            std::time::Duration::from_millis(5),
            "/api/auth/login",
        );
        metrics().record_http_response(
            axum::http::StatusCode::UNAUTHORIZED,
            std::time::Duration::from_millis(5),
            "/api/client/auth/login",
        );
        metrics().record_http_response(
            axum::http::StatusCode::UNAUTHORIZED,
            std::time::Duration::from_millis(5),
            "/api/client/auth/refresh",
        );
        metrics().record_http_response(
            axum::http::StatusCode::OK,
            std::time::Duration::from_millis(5),
            "/api/client/releases/download/app.zip",
        );
        record_nonce_replay();
        record_worker_job_failed();
        record_redis_error();
        record_notification_delivery(
            "webhook",
            NotificationDeliveryStatus::Success,
            std::time::Duration::from_millis(25),
        );
        record_notification_delivery(
            "unknown-channel-kind",
            NotificationDeliveryStatus::Failure,
            std::time::Duration::from_millis(25),
        );
        record_ai_gateway_request("/v1/chat/completions", AiGatewayRequestStatus::Success);
        record_ai_gateway_request(
            "/v1/images/generations",
            AiGatewayRequestStatus::ProviderError,
        );
        record_ai_gateway_charged("/v1/chat/completions", 123);
        record_ai_gateway_provider_duration(
            "/v1/chat/completions",
            std::time::Duration::from_millis(25),
        );
        record_ai_gateway_asset_cache_failure();
        record_ai_gateway_idempotency_replay("/v1/chat/completions");

        let rendered = render_prometheus(None);

        assert!(rendered.contains("http_requests_total"));
        assert!(rendered.contains("http_errors_total"));
        assert!(rendered.contains("http_server_errors_total"));
        assert!(rendered.contains("rate_limited_total"));
        assert!(rendered.contains("login_failures_total"));
        assert!(rendered.contains("client_refresh_failures_total"));
        assert!(rendered.contains("nonce_replay_total"));
        assert!(rendered.contains("file_downloads_total"));
        assert!(rendered.contains("worker_jobs_failed_total"));
        assert!(rendered.contains("redis_errors_total"));
        assert!(rendered.contains("notification_delivery_total"));
        assert!(rendered.contains("notification_delivery_failures_total"));
        assert!(rendered.contains("notification_delivery_duration_seconds_bucket"));
        assert!(rendered.contains("notification_delivery_duration_seconds_sum"));
        assert!(rendered.contains("notification_delivery_duration_seconds_count"));
        assert!(rendered.contains("ai_gateway_requests_total"));
        assert!(rendered.contains(
            "ai_gateway_requests_total{endpoint=\"chat_completions\",status=\"success\"}"
        ));
        assert!(rendered.contains("ai_gateway_charged_minor_total"));
        assert!(rendered.contains("ai_gateway_provider_duration_seconds_bucket"));
        assert!(rendered.contains("ai_gateway_asset_cache_failures_total"));
        assert!(rendered.contains("ai_gateway_idempotency_replays_total"));
        assert!(
            rendered.contains("notification_delivery_total{kind=\"webhook\",status=\"success\"}")
        );
        assert!(
            rendered.contains("notification_delivery_total{kind=\"unknown\",status=\"failure\"}")
        );
        assert!(rendered.contains("http_request_duration_seconds_bucket"));
        assert!(rendered.contains("http_request_duration_seconds_sum"));
        assert!(rendered.contains("http_request_duration_seconds_count"));
    }
}
