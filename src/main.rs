mod audio;
mod cleanup;
mod config;
mod daemon;
mod hotkey;
mod insert;
mod models;
mod overlay;
mod stt;
mod tui;
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
    /// Start the voice dictation daemon (overlay TUI by default)
    Start,
    /// Open the full dashboard (stops daemon, shows 4-tab TUI, restarts on exit)
    Dashboard,
    /// Toggle pause/resume (sends SIGUSR1 to running daemon)
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
        Commands::Start => {
            #[cfg(target_os = "macos")]
            {
                // Keep main thread for Cocoa overlay (global floating window visible on opencode/other terminals).
                // Run daemon on background thread, main thread services CFRunLoop for overlay + hotkey.
                let handle = std::thread::spawn(|| {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    rt.block_on(daemon::start())
                });
                // Main RunLoop — services overlay window and keeps dock icon hidden (Accessory).
                // This loop exits when the daemon thread finishes (e.g., after `miccli stop`).
                use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop};
                use std::time::Duration;
                while !handle.is_finished() {
                    unsafe {
                        CFRunLoop::run_in_mode(kCFRunLoopDefaultMode, Duration::from_millis(50), false);
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                return handle.join().unwrap();
            }
            #[cfg(not(target_os = "macos"))]
            {
                let rt = tokio::runtime::Runtime::new()?;
                return rt.block_on(daemon::start());
            }
        }
        Commands::Dashboard => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(daemon::dashboard())
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
