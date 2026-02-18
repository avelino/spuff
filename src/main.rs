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

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "spuff=info,chrondb=off".into()),
        )
        .with(tracing_subscriber::fmt::layer().without_time())
        .init();

    let cli = Cli::parse();
    let result = cli.execute().await;

    // ChronDB uses GraalVM Native Image which spawns background OS threads.
    // These threads prevent the process from exiting naturally after the
    // tokio runtime shuts down. Explicitly exiting ensures the CLI always
    // terminates promptly after the command completes.
    match result {
        Ok(_) => std::process::exit(0),
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}
