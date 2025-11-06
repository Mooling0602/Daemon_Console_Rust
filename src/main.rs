//! Main entry point for the daemon console application.
//!
//! This application provides a terminal interface with registered example commands
//! including 'list', 'help', 'exit', 'debug', 'hello', and 'test'.

use daemon_console::{TerminalApp, critical, get_debug, get_error, get_info, get_warn, info, warn};
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = TerminalApp::new();

    register_commands(&mut app);

    app.set_unknown_command_handler(|command: &str| {
        if command.starts_with("sudo") {
            get_error!(&format!(
                "Permission denied: could not execute '{}'",
                command
            ))
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
        "TerminalApp started. Press Ctrl+D or Ctrl+C twice to exit.",
        "Daemon"
    );
    let exit_message = get_info!("TerminalApp exiting. Goodbye!", "Daemon");

    app.run(&startup_message, &exit_message)
}

/// Registers all available commands with the terminal application.
///
/// # Arguments
///
/// * `app` - Mutable reference to the terminal application
fn register_commands(app: &mut TerminalApp) {
    app.register_command(
        "list",
        Box::new(|_app: &mut TerminalApp, args: &[&str]| -> String {
            if cfg!(target_os = "windows") {
                Command::new("cmd")
                    .arg("/C")
                    .arg("dir")
                    .output()
                    .map_or_else(
                        |e| get_error!(&format!("Error executing command: {}", e)),
                        |output| {
                            String::from_utf8_lossy(&output.stdout)
                                .trim_end()
                                .to_string()
                        },
                    )
            } else {
                Command::new("ls").args(args).output().map_or_else(
                    |e| get_error!(&format!("Error executing command: {}", e)),
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
            get_info!("Available commands: 'list', 'help', 'exit', 'debug', 'hello' and 'test'.")
        }),
    );

    app.register_command(
        "exit",
        Box::new(|_: &mut TerminalApp, _: &[&str]| -> String {
            get_warn!("Press Ctrl+C(twice to confirm) or Ctrl+D to exit.")
        }),
    );

    app.register_command(
        "debug",
        Box::new(|_: &mut TerminalApp, _: &[&str]| -> String {
            get_debug!("This is a debug log message.")
        }),
    );

    app.register_command(
        "hello",
        Box::new(|_: &mut TerminalApp, args: &[&str]| -> String {
            if args.is_empty() {
                get_info!("Hello, World!")
            } else {
                get_info!(&format!("Hello, {}!", args.join(" ")))
            }
        }),
    );

    app.register_command(
        "test",
        Box::new(|_: &mut TerminalApp, args: &[&str]| -> String {
            if !args.is_empty() {
                get_error!("This command rejects arguments!")
            } else {
                get_info!("Success!")
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
                warn!("You have confirmed to crash the application.");
                critical!("Crashing...");
                panic!("Application crashed intentionally!");
            }

            // 第一次执行，显示提示信息
            info!("This command does not crash the application.");
            warn!("Dangerous option!");
            get_info!("Type this command again to crash the application.")
        }),
    );
}
