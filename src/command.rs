use crate::{TerminalApp, get_error, get_info, get_warn};
use async_trait::async_trait;
use futures::future::BoxFuture;
use tokio::task::JoinHandle;

/// Result from command execution
#[derive(Debug)]
pub struct CommandResult {
    pub command: String,
    pub output: String,
}

/// Trait for synchronous command handlers that can be registered with the terminal application.
///
/// All synchronous commands must implement this trait to be executable within the terminal app.
/// Handlers receive mutable access to the application state and command arguments.
pub trait CommandHandler: Send + Sync + 'static {
    /// Executes the command with the given application state and arguments.
    ///
    /// # Arguments
    ///
    /// * `app` - Mutable reference to the terminal application
    /// * `args` - Slice of command arguments
    ///
    /// # Returns
    ///
    /// String output to be displayed to the user
    fn execute(&mut self, app: &mut TerminalApp, args: &[&str]) -> String;
}

/// Trait for asynchronous command handlers that can be registered with the terminal application.
///
/// All asynchronous commands must implement this trait to be executable within the terminal app.
/// Handlers receive mutable access to the application state and command arguments.
#[async_trait]
pub trait AsyncCommandHandler: Send + Sync + 'static {
    /// Executes the command asynchronously with the given application state and arguments.
    ///
    /// # Arguments
    ///
    /// * `app` - Mutable reference to the terminal application
    /// * `args` - Slice of command arguments
    ///
    /// # Returns
    ///
    /// String output to be displayed to the user
    async fn execute_async(&mut self, app: &mut TerminalApp, args: &[&str]) -> String;

    /// Creates a boxed clone of this handler for reuse
    fn box_clone(&self) -> Box<dyn AsyncCommandHandler>;
}

/// Internal enum to hold either sync or async command handlers.
pub enum CommandHandlerType {
    PubSync(Box<dyn CommandHandler>),
    PubAsync(Box<dyn AsyncCommandHandler>),
}

/// Status of running commands
#[derive(Debug)]
pub struct RunningCommand {
    pub command: String,
    pub handle: JoinHandle<String>,
}

// Re-export the variants with expected names inside crate via type aliasing
impl CommandHandlerType {
    pub fn as_sync(&self) -> bool {
        matches!(self, CommandHandlerType::PubSync(_))
    }
}

pub type UnknownCommandHandler = Box<dyn Fn(&str) -> String + Send + Sync + 'static>;
pub type AsyncUnknownCommandHandler =
    Box<dyn Fn(&str) -> BoxFuture<'static, String> + Send + Sync + 'static>;

// Blanket implementation of `CommandHandler` for closures.
impl<F> CommandHandler for F
where
    F: FnMut(&mut TerminalApp, &[&str]) -> String + Send + Sync + 'static,
{
    fn execute(&mut self, app: &mut TerminalApp, args: &[&str]) -> String {
        self(app, args)
    }
}

/// Executes a command by looking it up in the registered commands.
///
/// For sync commands, executes immediately and returns the result.
/// For async commands, spawns them in the background and returns immediately.
///
/// # Arguments
///
/// * `app` - Terminal application
/// * `command` - Full command string including arguments
///
/// # Returns
///
/// String output from the command execution (empty for async commands)
pub async fn execute_command(app: &mut TerminalApp, command: &str) -> String {
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.is_empty() {
        return String::new();
    }

    let cmd_name = parts[0];
    let args = &parts[1..];

    if let Some(handler) = app.commands.get(cmd_name) {
        match handler {
            CommandHandlerType::PubSync(_) => {
                // Remove, execute, and put back sync handler
                if let Some(CommandHandlerType::PubSync(mut sync_handler)) =
                    app.commands.remove(cmd_name)
                {
                    let result = sync_handler.execute(app, args);
                    app.commands.insert(
                        cmd_name.to_string(),
                        CommandHandlerType::PubSync(sync_handler),
                    );
                    result
                } else {
                    get_error!("Internal error: sync handler not found", "CommandStatus")
                }
            }
            CommandHandlerType::PubAsync(async_handler) => {
                // Clone the async handler for execution
                let cloned_handler = async_handler.box_clone();
                match app
                    .spawn_async_command(command.to_string(), cloned_handler)
                    .await
                {
                    Ok(_) => {
                        get_info!(
                            &format!("Async command '{}' started in the background", cmd_name),
                            "CommandStatus"
                        )
                    }
                    Err(e) => {
                        get_error!(
                            &format!("Failed to spawn async command: {}", e),
                            "CommandStatus"
                        )
                    }
                }
            }
        }
    } else if let Some(ref handler) = app.async_unknown_command_handler {
        handler(command).await
    } else if let Some(ref handler) = app.unknown_command_handler {
        handler(command)
    } else {
        get_warn!(
            &format!("Command not found or registered: '{}'", command),
            "CommandStatus"
        )
    }
}
