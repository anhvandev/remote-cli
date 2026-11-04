use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let config = rcli_server::Config::from_env()?;
    let server = rcli_server::start(config, Arc::new(rcli_server::clock::SystemClock)).await?;
    tracing::info!("controller API on {}, agents on {}", server.http_addr, server.agent_addr);
    tokio::signal::ctrl_c().await?;
    Ok(())
}
