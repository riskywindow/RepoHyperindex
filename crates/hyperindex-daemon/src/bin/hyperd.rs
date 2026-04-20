use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use hyperindex_daemon::{runtime::RuntimeState, server::DaemonServer};
use hyperindex_protocol::config::{LogFormat, LogVerbosity, LoggingSettings};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "hyperd",
    version = "0.1.0",
    about = "Repo Hyperindex daemon scaffold."
)]
struct Cli {
    #[arg(long)]
    config_path: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Serve,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let runtime = RuntimeState::bootstrap(cli.config_path.as_deref())?;
    init_daemon_tracing("hyperd", &runtime.loaded_config.config.logging);
    let server = DaemonServer::new(runtime);
    match cli.command.unwrap_or(Commands::Serve) {
        Commands::Serve => server.serve().await?,
    }
    Ok(())
}

fn init_daemon_tracing(component: &str, logging: &LoggingSettings) {
    let directive = match logging.verbosity {
        LogVerbosity::Error => "error",
        LogVerbosity::Warn => "warn",
        LogVerbosity::Info => "info",
        LogVerbosity::Debug => "debug",
        LogVerbosity::Trace => "trace",
    };
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("{component}={directive}")));
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false);
    match logging.format {
        LogFormat::Json => {
            let _ = subscriber.compact().try_init();
        }
        LogFormat::Text => {
            let _ = subscriber.try_init();
        }
    }
}
