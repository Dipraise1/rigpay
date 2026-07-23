use rigpay_gateway::{config, server};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cfg_path = std::env::args().nth(1).unwrap_or_else(|| "services.toml".into());
    let cfg = config::load(&cfg_path)?;
    let bind = cfg.gateway.bind.clone();

    let (router, app) = server::build(cfg)?;
    tokio::spawn(server::evict_expired(app));

    tracing::info!(%bind, "rigpay-gateway listening");
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("shutting down");
        })
        .await?;
    Ok(())
}
