use anyhow::Result;
use tracing_subscriber::EnvFilter;

use dogitor::config::Config;
use dogitor::server;

#[tokio::main]
async fn main() -> Result<()> {
    // Load DOGITOR_BOT_TOKEN / DOGITOR_API_TOKEN from a `.env` file next to the
    // binary, if present. Real env vars still take precedence (dotenvy never
    // overrides an already-set var).
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,dogitor=info")),
        )
        .init();

    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".to_string());
    let cfg = Config::load(&config_path)?;

    let _handle = server::start(cfg).await?;

    tokio::signal::ctrl_c().await?;
    Ok(())
}
