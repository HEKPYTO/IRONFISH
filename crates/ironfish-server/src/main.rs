use tracing::info;
use tracing_subscriber::EnvFilter;

mod app;
mod config;

use app::Application;
use config::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logging();

    let config = Config::load()?;
    info!("loaded configuration");

    let app = Application::new(config).await?;
    info!("application initialized");

    app.run().await?;

    Ok(())
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .init();
}
