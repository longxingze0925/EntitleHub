use std::time::Duration;

use crate::{error::AppError, state::AppState};

const CLEANUP_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const RETENTION_SECONDS: i64 = 30 * 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdminSessionCleanupResult {
    pub deleted_refresh_tokens: u64,
    pub deleted_sessions: u64,
}

pub fn spawn_admin_session_cleanup_worker(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        run_admin_session_cleanup_worker(state).await;
    })
}

async fn run_admin_session_cleanup_worker(state: AppState) {
    let mut interval = tokio::time::interval(CLEANUP_INTERVAL);
    loop {
        interval.tick().await;
        match cleanup_admin_sessions(&state).await {
            Ok(result) if result.deleted_sessions > 0 || result.deleted_refresh_tokens > 0 => {
                tracing::info!(
                    deleted_sessions = result.deleted_sessions,
                    deleted_refresh_tokens = result.deleted_refresh_tokens,
                    "admin session cleanup completed"
                );
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(%error, "admin session cleanup failed");
            }
        }
    }
}

pub async fn cleanup_admin_sessions(
    state: &AppState,
) -> Result<AdminSessionCleanupResult, AppError> {
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let deleted_refresh_tokens = sqlx::query(
        r#"
        delete from admin_refresh_tokens rt
        using admin_sessions s
        where rt.session_id = s.id
          and (
            s.expires_at < now() - ($1::bigint * interval '1 second')
            or (
              s.revoked_at is not null
              and s.revoked_at < now() - ($1::bigint * interval '1 second')
            )
          )
        "#,
    )
    .bind(RETENTION_SECONDS)
    .execute(&mut *transaction)
    .await
    .map(|result| result.rows_affected())
    .map_err(map_db_error)?;

    let deleted_sessions = sqlx::query(
        r#"
        delete from admin_sessions
        where expires_at < now() - ($1::bigint * interval '1 second')
           or (
             revoked_at is not null
             and revoked_at < now() - ($1::bigint * interval '1 second')
           )
        "#,
    )
    .bind(RETENTION_SECONDS)
    .execute(&mut *transaction)
    .await
    .map(|result| result.rows_affected())
    .map_err(map_db_error)?;

    transaction.commit().await.map_err(map_db_error)?;

    Ok(AdminSessionCleanupResult {
        deleted_refresh_tokens,
        deleted_sessions,
    })
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("admin session cleanup database error: {error}"))
}
