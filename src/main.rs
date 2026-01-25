mod cli;
mod config;
mod connector;
mod environment;
mod error;
mod project_config;
mod provider;
mod ssh;
mod state;
mod tui;
pub mod utils;
pub mod volume;

use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use cli::Cli;
use error::Result;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "spuff=info,chrondb=off".into()),
        )
        .with(tracing_subscriber::fmt::layer().without_time())
        .init();

    let cli = Cli::parse();
    cli.execute().await
}
