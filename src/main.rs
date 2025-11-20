//! Main entry point for the daemon console application.
//!
//! This application provides a terminal interface with registered example commands
//! including 'list', 'help', 'exit', 'debug', 'hello', 'test', and async commands like 'sleep'.

use async_trait::async_trait;
use crossterm::terminal::disable_raw_mode;
use daemon_console::{AsyncCommandHandler, TerminalApp, get_debug, get_error, get_info, get_warn};
use std::io::{Write, stdout};
use std::process::Command;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = TerminalApp::new();

    register_commands(&mut app).await;

    app.set_unknown_command_handler(|command: &str| {
        if command.starts_with("sudo") {
            get_error!(
                &format!("Permission denied: could not execute '{}'", command),
                "CommandResp"
            )
        } else if command.len() > 20 {
            get_warn!(&format!("Command too long: '{}'", command))
        } else {
            get_warn!(&format!(
                "Command '{}' does not exist. Type 'help' to see available commands.",
                command
            ))
        }
    });

    let startup_message = get_info!(
        "Running in async mode (v0.3.0+). Press Ctrl+D or Ctrl+C twice to exit.",
        "Daemon Console"
    );
    let exit_message = "Daemon Console exiting. Goodbye!";

    app.run(&startup_message, &exit_message).await
}

/// Async command handler for the sleep command
#[derive(Clone)]
struct SleepCommand;

#[async_trait]
impl AsyncCommandHandler for SleepCommand {
    async fn execute_async(&mut self, app: &mut TerminalApp, args: &[&str]) -> String {
        if args.is_empty() {
            app.info("This command is used to test async features.");
            return get_info!("Usage: wait <seconds>", "CommandHelp");
        }

        match args[0].parse::<u64>() {
            Ok(seconds) => {
                sleep(Duration::from_secs(seconds)).await;
                app.info("Wake up!");
                get_info!(
                    &format!("Finished sleeping for {} seconds!", seconds),
                    "CommandResp"
                )
            }
            Err(_) => {
                get_error!(
                    "Invalid number format. Please provide a valid number of seconds.",
                    "CommandResp"
                )
            }
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
        Box::new(|app: &mut TerminalApp, args: &[&str]| -> String {
            if cfg!(target_os = "windows") {
                app.info("Detected Windows system, using 'dir' command.");
                Command::new("cmd")
                    .arg("/C")
                    .arg("dir")
                    .output()
                    .map_or_else(
                        |e| get_error!(&format!("Error executing command: {}", e), "CommandResp"),
                        |output| {
                            String::from_utf8_lossy(&output.stdout)
                                .trim_end()
                                .to_string()
                        },
                    )
            } else {
                app.info("Seems system is Unix-like, using 'ls' command.");
                Command::new("ls").args(args).output().map_or_else(
                    |e| get_error!(&format!("Error executing command: {}", e), "CommandResp"),
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
            get_info!("Available commands:\n- Sync: 'list', 'help', 'exit', 'debug', 'hello', 'test', 'crash'\n- Async (non-blocking): 'wait <seconds>'\nAsync commands run in the background - you can continue typing while they execute!", "CommandHelp")
        }),
    );

    app.register_command(
        "exit",
        Box::new(|app: &mut TerminalApp, _: &[&str]| -> String {
            app.should_exit = true;
            get_warn!("Exiting application by command 'exit'...", "CommandResp")
        }),
    );

    app.register_command(
        "debug",
        Box::new(|_: &mut TerminalApp, _: &[&str]| -> String {
            get_debug!("This is a debug log message.", "CommandResp")
        }),
    );

    app.register_command(
        "hello",
        Box::new(|_: &mut TerminalApp, args: &[&str]| -> String {
            if args.is_empty() {
                get_info!("Hello, World!", "CommandResp")
            } else {
                get_info!(&format!("Hello, {}!", args.join(" ")), "CommandResp")
            }
        }),
    );

    app.register_command(
        "test",
        Box::new(|_: &mut TerminalApp, args: &[&str]| -> String {
            if !args.is_empty() {
                get_error!("This command rejects arguments!", "CommandResp")
            } else {
                get_info!("Success!", "CommandResp")
            }
        }),
    );

    app.register_command(
        "crash",
        Box::new(|app: &mut TerminalApp, args: &[&str]| -> String {
            let crash_count = app
                .command_history
                .iter()
                .filter(|&cmd| cmd.trim() == "crash")
                .count();

            if crash_count > 1 || args.contains(&"--confirm") {
                let mut stdout = stdout();
                app.warn("You have confirmed to crash the application.");
                app.critical("Crashing...");
                disable_raw_mode().expect("");
                stdout.flush().expect("");
                panic!("Application crashed intentionally!");
            }

            app.info("This command crashs the application.");
            app.warn("Dangerous option!");
            get_info!(
                "Type this command again to crash the application.",
                "CommandResp"
            )
        }),
    );

    // Asynchronous commands
    app.register_async_command("wait", Box::new(SleepCommand));
}
