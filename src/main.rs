mod audio;
mod cleanup;
mod config;
mod daemon;
mod hotkey;
mod insert;
mod models;
mod stt;
mod vad;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "miccli", about = "Terminal voice dictation CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the voice dictation daemon
    Start {
        /// Run in foreground (no background)
        #[arg(long)]
        foreground: bool,
    },
    /// Toggle recording on/off (sends signal to running daemon)
    Toggle,
    /// Stop the running daemon
    Stop,
    /// Open or show config
    Config {
        /// Print config path and exit
        #[arg(long)]
        path: bool,
    },
    /// List and manage Whisper models
    Models {
        #[command(subcommand)]
        action: Option<ModelsAction>,
    },
    /// Run as MCP server (for future opencode integration)
    Mcp,
}

#[derive(Subcommand)]
enum ModelsAction {
    /// List downloaded models
    List,
    /// Download a model
    Download {
        /// Model name: tiny, base, small, medium
        name: String,
    },
    /// Remove a downloaded model
    Remove {
        name: String,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "miccli=info".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Start { foreground } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(daemon::start(foreground))
        }
        Commands::Toggle => daemon::send_signal("toggle"),
        Commands::Stop => daemon::send_signal("stop"),
        Commands::Config { path } => {
            let config_dir = config::config_dir()?;
            if path {
                println!("{}", config_dir.display());
            } else {
                println!("Config directory: {}", config_dir.display());
                println!("Edit config.toml to customize settings.");
            }
            Ok(())
        }
        Commands::Models { action } => match action.unwrap_or(ModelsAction::List) {
            ModelsAction::List => models::list(),
            ModelsAction::Download { name } => models::download(&name),
            ModelsAction::Remove { name } => models::remove(&name),
        },
        Commands::Mcp => {
            anyhow::bail!("MCP server mode not yet implemented");
        }
    }
}
