use user_admin_backend::{
    config::{AppConfig, DatabaseConfig},
    db,
    modules::{
        auth::{self, password::AdminPasswordResetCliInput},
        bootstrap::{initialize_owner, BootstrapOwnerInput},
        outbox,
    },
    router,
    state::AppState,
    telemetry,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    match std::env::args().nth(1).as_deref() {
        Some("migrate") => {
            let database = DatabaseConfig::from_env()?;
            let pool = db::connect(&database).await?;
            sqlx::migrate!("./migrations").run(&pool).await?;

            println!("database migrations applied");

            return Ok(());
        }
        Some("init-owner") => {
            let input = BootstrapOwnerInput::from_env()?;
            let database = DatabaseConfig::from_env()?;
            let pool = db::connect(&database).await?;
            let result = initialize_owner(&pool, input).await?;

            println!("bootstrap owner initialized");
            println!("tenant_id={}", result.tenant_id);
            println!("owner_id={}", result.owner_id);
            if let Some(password) = result.generated_password {
                println!("generated_password={password}");
            }

            return Ok(());
        }
        Some("reset-admin-password") => {
            let input = AdminPasswordResetCliInput::from_env()?;
            let database = DatabaseConfig::from_env()?;
            let pool = db::connect(&database).await?;
            let result = auth::password::reset_admin_password_cli(&pool, input).await?;

            println!("admin password reset");
            println!("tenant_id={}", result.tenant_id);
            println!("tenant_slug={}", result.tenant_slug);
            println!("team_member_id={}", result.team_member_id);
            println!("email={}", result.email);
            println!("revoked_sessions={}", result.revoked_sessions);
            println!("revoked_refresh_tokens={}", result.revoked_refresh_tokens);
            println!("mfa_disabled={}", result.mfa_disabled);
            if let Some(password) = result.generated_password {
                println!("generated_password={password}");
            }

            return Ok(());
        }
        _ => {}
    }

    let config = AppConfig::from_env()?;
    telemetry::init(&config);

    let addr = config.server.socket_addr();
    let state = AppState::connect(config).await?;
    auth::session_cleanup::spawn_admin_session_cleanup_worker(state.clone());
    if state.config.email.outbox_worker_enabled {
        outbox::worker::spawn_email_outbox_worker(state.clone());
    }
    let app = router::build(state);

    tracing::info!(%addr, "starting backend");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;

    Ok(())
}
