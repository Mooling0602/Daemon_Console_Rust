//! Main entry point for the daemon console application.
//!
//! This application provides a terminal interface with registered example commands
//! including 'list', 'help', 'exit', 'debug', 'hello', 'test', and async commands like 'sleep'.

use async_trait::async_trait;
use daemon_console::{AsyncCommandHandler, TerminalApp, critical, debug, error, info, warn};
use std::process::Command;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = TerminalApp::new();

    register_commands(&mut app).await;

    app.set_unknown_command_handler(|command: &str| {
        if command.starts_with("sudo") {
            error!(&format!(
                "Permission denied: could not execute '{}'",
                command
            ))
        } else if command.len() > 20 {
            warn!(&format!("Command too long: '{}'", command))
        } else {
            warn!(&format!(
                "Command '{}' does not exist. Type 'help' to see available commands.",
                command
            ))
        }
    });

    let startup_message = info!(
        "TerminalApp started with async support. Press Ctrl+D or Ctrl+C twice to exit.",
        "Daemon"
    );
    let exit_message = info!("TerminalApp exiting. Goodbye!", "Daemon");

    app.run(&startup_message, &exit_message).await
}

/// Async command handler for the sleep command
#[derive(Clone)]
struct SleepCommand;

#[async_trait]
impl AsyncCommandHandler for SleepCommand {
    async fn execute_async(&mut self, _app: &mut TerminalApp, args: &[&str]) -> String {
        if args.is_empty() {
            return error!("Usage: sleep <seconds>");
        }

        match args[0].parse::<u64>() {
            Ok(seconds) => {
                sleep(Duration::from_secs(seconds)).await;
                info!(&format!("Finished sleeping for {} seconds!", seconds))
            }
            Err(_) => error!("Invalid number format. Please provide a valid number of seconds."),
        }
    }

    fn box_clone(&self) -> Box<dyn AsyncCommandHandler> {
        Box::new(self.clone())
    }
}

/// Registers all available commands with the terminal application.
///
/// # Arguments
///
/// * `app` - Mutable reference to the terminal application
async fn register_commands(app: &mut TerminalApp) {
    // Synchronous commands
    app.register_command(
        "list",
        Box::new(|_app: &mut TerminalApp, args: &[&str]| -> String {
            if cfg!(target_os = "windows") {
                Command::new("cmd")
                    .arg("/C")
                    .arg("dir")
                    .output()
                    .map_or_else(
                        |e| error!(&format!("Error executing command: {}", e)),
                        |output| {
                            String::from_utf8_lossy(&output.stdout)
                                .trim_end()
                                .to_string()
                        },
                    )
            } else {
                Command::new("ls").args(args).output().map_or_else(
                    |e| error!(&format!("Error executing command: {}", e)),
                    |output| {
                        String::from_utf8_lossy(&output.stdout)
                            .trim_end()
                            .to_string()
                    },
                )
            }
        }),
    );

    app.register_command(
        "help",
        Box::new(|_: &mut TerminalApp, _: &[&str]| -> String {
            info!("Available commands:\n  Sync: 'list', 'help', 'exit', 'debug', 'hello', 'test', 'crash'\n  Async (non-blocking): 'wait <seconds>' \nAsync commands run in the background - you can continue typing while they execute!")
        }),
    );

    app.register_command(
        "exit",
        Box::new(|app: &mut TerminalApp, _: &[&str]| -> String {
            app.should_exit = true;
            warn!("Exiting application by command 'exit'...")
        }),
    );

    app.register_command(
        "debug",
        Box::new(|_: &mut TerminalApp, _: &[&str]| -> String {
            debug!("This is a debug log message.")
        }),
    );

    app.register_command(
        "hello",
        Box::new(|_: &mut TerminalApp, args: &[&str]| -> String {
            if args.is_empty() {
                info!("Hello, World!")
            } else {
                info!(&format!("Hello, {}!", args.join(" ")))
            }
        }),
    );

    app.register_command(
        "test",
        Box::new(|_: &mut TerminalApp, args: &[&str]| -> String {
            if !args.is_empty() {
                error!("This command rejects arguments!")
            } else {
                info!("Success!")
            }
        }),
    );

    app.register_command(
        "crash",
        Box::new(|_: &mut TerminalApp, _: &[&str]| -> String { critical!("Dangerous option!") }),
    );

    // Asynchronous commands
    app.register_async_command("wait", Box::new(SleepCommand));
}
