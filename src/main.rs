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
    Start {
        /// Run in background (daemonize, returns immediately; overlay stays global)
        #[arg(short, long)]
        background: bool,
        /// Log file for background daemon (default ~/.config/miccli/miccli.log)
        #[arg(long)]
        log_file: Option<std::path::PathBuf>,
    },
    /// Open the full dashboard (stops daemon, shows 4-tab TUI, restarts on exit)
    Dashboard,
    /// Toggle pause/resume (sends SIGUSR1 to running daemon)
    Toggle,
    /// Stop the running daemon
    Stop,
    /// Show daemon status (running / paused / PID / log)
    Status,
    /// Restart the daemon (stop if running, then start)
    Restart {
        /// Run in background (daemonize, returns immediately; overlay stays global)
        #[arg(short, long)]
        background: bool,
        /// Log file for background daemon (default ~/.config/miccli/miccli.log)
        #[arg(long)]
        log_file: Option<std::path::PathBuf>,
    },
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
    Remove { name: String },
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
        Commands::Start {
            background,
            log_file,
        } => {
            if background {
                daemonize(log_file)?;
            } else if let Some(path) = log_file {
                eprintln!(
                    "warning: --log-file is only used with --background, ignoring {}",
                    path.display()
                );
            }
            // Duplicate check for foreground (background already checked in daemonize)
            if !background {
                check_duplicate_instance()?;
            }
            #[cfg(target_os = "macos")]
            {
                // Keep main thread for Cocoa overlay (global floating window visible on opencode/other terminals).
                // Run daemon on background thread, main thread services CFRunLoop for overlay + hotkey.
                let handle = std::thread::spawn(|| {
                    let rt =
                        tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
                    rt.block_on(daemon::start())
                });
                // Main RunLoop — services overlay window and keeps dock icon hidden (Accessory).
                // This loop exits when the daemon thread finishes (e.g., after `miccli stop`).
                use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop};
                use std::time::Duration;
                while !handle.is_finished() {
                    unsafe {
                        CFRunLoop::run_in_mode(
                            kCFRunLoopDefaultMode,
                            Duration::from_millis(50),
                            false,
                        );
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                handle
                    .join()
                    .map_err(|_| anyhow::anyhow!("daemon thread panicked"))?
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
        Commands::Status => daemon::status(),
        Commands::Restart {
            background,
            log_file,
        } => {
            // Stop if running (best effort)
            let pid_file = daemon::pid_file_path()?;
            if pid_file.exists() {
                println!("Stopping existing daemon...");
                let _ = daemon::send_signal("stop");
                // Wait for pid file removal without creating tokio runtime before fork (fork safety on macOS)
                for _ in 0..50 {
                    if !pid_file.exists() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
                if pid_file.exists() {
                    eprintln!("Warning: old daemon PID file still exists, forcing removal");
                    let _ = std::fs::remove_file(&pid_file);
                } else {
                    println!("Daemon stopped.");
                }
            }
            if background {
                daemonize(log_file)?;
            } else if let Some(path) = log_file {
                eprintln!(
                    "warning: --log-file is only used with --background, ignoring {}",
                    path.display()
                );
            }
            if !background {
                check_duplicate_instance()?;
            }
            println!(
                "Starting daemon{}...",
                if background { " in background" } else { "" }
            );
            #[cfg(target_os = "macos")]
            {
                let handle = std::thread::spawn(|| {
                    let rt =
                        tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
                    rt.block_on(daemon::start())
                });
                use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop};
                use std::time::Duration;
                while !handle.is_finished() {
                    unsafe {
                        CFRunLoop::run_in_mode(
                            kCFRunLoopDefaultMode,
                            Duration::from_millis(50),
                            false,
                        );
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                handle
                    .join()
                    .map_err(|_| anyhow::anyhow!("daemon thread panicked"))?
            }
            #[cfg(not(target_os = "macos"))]
            {
                let rt = tokio::runtime::Runtime::new()?;
                return rt.block_on(daemon::start());
            }
        }
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

fn pid_file_path() -> Result<std::path::PathBuf> {
    crate::config::pid_file_path()
}

fn default_log_file() -> Result<std::path::PathBuf> {
    crate::config::log_file_path()
}

#[cfg(unix)]
fn is_process_alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

#[cfg(not(unix))]
fn is_process_alive(_pid: i32) -> bool {
    false
}

fn check_duplicate_instance() -> Result<()> {
    let pid_file = pid_file_path()?;
    if !pid_file.exists() {
        return Ok(());
    }
    let pid_str = std::fs::read_to_string(&pid_file)?;
    if let Ok(pid) = pid_str.trim().parse::<i32>() {
        if is_process_alive(pid) {
            anyhow::bail!(
                "miccli is already running (PID {}), use `miccli stop` or `miccli restart` or `miccli status`",
                pid
            );
        } else {
            // Stale PID file
            eprintln!("Removing stale PID file (PID {} not running)", pid);
            let _ = std::fs::remove_file(&pid_file);
        }
    } else {
        // Invalid PID file, remove
        let _ = std::fs::remove_file(&pid_file);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn daemonize(log_file: Option<std::path::PathBuf>) -> Result<()> {
    use std::io::Write;
    // Check duplicate before forking so parent can report immediately
    check_duplicate_instance()?;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    // Resolve log file (default for background)
    let log_path = if let Some(p) = log_file {
        p
    } else {
        default_log_file()?
    };
    unsafe {
        let pid = libc::fork();
        if pid < 0 {
            anyhow::bail!("fork failed: {}", std::io::Error::last_os_error());
        }
        if pid > 0 {
            // Parent exits immediately — terminal returns. Child keeps CFRunLoop + native overlay alive.
            // Single fork (no setsid) on macOS to preserve WindowServer connection for NSWindow.
            // Child will be adopted by launchd (PPID 1) but remains in same login session, so overlay stays global.
            println!("miccli started in background (log: {})", log_path.display());
            let _ = std::io::stdout().flush();
            std::process::exit(0);
        }
        // Child: daemon — stay in same session (no setsid) so NSWindow can connect to WindowServer.
        // Redirect stdin to /dev/null, stdout/stderr to log file (keep GUI, detach terminal)
        let devnull = std::ffi::CString::new("/dev/null").expect("CString /dev/null");
        let fd_null = libc::open(devnull.as_ptr(), libc::O_RDWR);
        if fd_null >= 0 {
            libc::dup2(fd_null, libc::STDIN_FILENO);
            if fd_null > libc::STDERR_FILENO {
                libc::close(fd_null);
            }
        }
        // Open log file (append, create) — handle NUL in path gracefully
        let log_cstr =
            match std::ffi::CString::new(log_path.to_string_lossy().as_ref().as_bytes().to_vec()) {
                Ok(c) => c,
                Err(_) => {
                    tracing::warn!("log path contains NUL, falling back to /dev/null");
                    std::ffi::CString::new("/dev/null").expect("CString /dev/null")
                }
            };
        // Ensure parent dir exists
        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let fd_log = libc::open(
            log_cstr.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_APPEND,
            0o644,
        );
        if fd_log >= 0 {
            libc::dup2(fd_log, libc::STDOUT_FILENO);
            libc::dup2(fd_log, libc::STDERR_FILENO);
            if fd_log > libc::STDERR_FILENO {
                libc::close(fd_log);
            }
        } else {
            // Fallback to /dev/null for stdout/stderr if log open fails
            let fd2 = libc::open(devnull.as_ptr(), libc::O_RDWR);
            if fd2 >= 0 {
                libc::dup2(fd2, libc::STDOUT_FILENO);
                libc::dup2(fd2, libc::STDERR_FILENO);
                if fd2 > libc::STDERR_FILENO {
                    libc::close(fd2);
                }
            }
        }
        libc::umask(0);
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn daemonize(log_file: Option<std::path::PathBuf>) -> Result<()> {
    use std::io::Write;
    // Check duplicate before forking so parent can report immediately
    check_duplicate_instance()?;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    // Resolve log file (default for background)
    let log_path = if let Some(p) = log_file {
        p
    } else {
        default_log_file()?
    };
    unsafe {
        let pid = libc::fork();
        if pid < 0 {
            anyhow::bail!("fork failed: {}", std::io::Error::last_os_error());
        }
        if pid > 0 {
            // Parent exits immediately — terminal returns. Child keeps running.
            // Note: pid is first child, daemon is grandchild (PPID 1), so don't print PID as daemon PID.
            println!("miccli started in background (log: {})", log_path.display());
            let _ = std::io::stdout().flush();
            std::process::exit(0);
        }
        // First child: become session leader, detach from controlling terminal.
        if libc::setsid() < 0 {
            anyhow::bail!("setsid failed: {}", std::io::Error::last_os_error());
        }
        let pid2 = libc::fork();
        if pid2 < 0 {
            anyhow::bail!("second fork failed: {}", std::io::Error::last_os_error());
        }
        if pid2 > 0 {
            std::process::exit(0);
        }
        // Grandchild: daemon — fully detached, no controlling terminal.
        // Redirect stdin to /dev/null, stdout/stderr to log file
        let devnull = std::ffi::CString::new("/dev/null").expect("CString /dev/null");
        let fd_null = libc::open(devnull.as_ptr(), libc::O_RDWR);
        if fd_null >= 0 {
            libc::dup2(fd_null, libc::STDIN_FILENO);
            if fd_null > libc::STDERR_FILENO {
                libc::close(fd_null);
            }
        }
        // Open log file (append, create) — handle NUL in path gracefully
        let log_cstr =
            match std::ffi::CString::new(log_path.to_string_lossy().as_ref().as_bytes().to_vec()) {
                Ok(c) => c,
                Err(_) => {
                    tracing::warn!("log path contains NUL, falling back to /dev/null");
                    std::ffi::CString::new("/dev/null").expect("CString /dev/null")
                }
            };
        // Ensure parent dir exists
        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let fd_log = libc::open(
            log_cstr.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_APPEND,
            0o644,
        );
        if fd_log >= 0 {
            libc::dup2(fd_log, libc::STDOUT_FILENO);
            libc::dup2(fd_log, libc::STDERR_FILENO);
            if fd_log > libc::STDERR_FILENO {
                libc::close(fd_log);
            }
        } else {
            // Fallback to /dev/null for stdout/stderr if log open fails
            let fd2 = libc::open(devnull.as_ptr(), libc::O_RDWR);
            if fd2 >= 0 {
                libc::dup2(fd2, libc::STDOUT_FILENO);
                libc::dup2(fd2, libc::STDERR_FILENO);
                if fd2 > libc::STDERR_FILENO {
                    libc::close(fd2);
                }
            }
        }
        libc::umask(0);
    }
    Ok(())
}

#[cfg(not(unix))]
fn daemonize(_log_file: Option<std::path::PathBuf>) -> Result<()> {
    anyhow::bail!("--background is only supported on Unix platforms");
}
