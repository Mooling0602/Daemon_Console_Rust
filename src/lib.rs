//! # daemon_console
//!
//! A flexible console for daemon applications providing a terminal interface
//! with command registration, history navigation, and colored logging.
//!
//! # Examples
//!
//! A simple way to create a `TerminalApp` instance.
//!
//! ```rust
//! use daemon_console::TerminalApp;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut app = TerminalApp::new();
//!     // Register commands and run
//!     app.run("App started", "App exited").await
//! }
//! ```
//!
//! See more details in `src/main.rs` in source code.

#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod logger;

use async_trait::async_trait;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, poll},
    execute,
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode},
};
use futures::future::BoxFuture;
use std::collections::HashMap;
use std::io::{Stdout, Write, stdout};
use std::time::Instant;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use unicode_width::UnicodeWidthChar;

/// Type alias for custom unknown command handlers.
type UnknownCommandHandler = Box<dyn Fn(&str) -> String + Send + Sync + 'static>;

/// Type alias for async unknown command handlers.
type AsyncUnknownCommandHandler =
    Box<dyn Fn(&str) -> BoxFuture<'static, String> + Send + Sync + 'static>;

/// Result from command execution
#[derive(Debug)]
pub struct CommandResult {
    pub command: String,
    pub output: String,
}

/// Status of running commands
#[derive(Debug)]
struct RunningCommand {
    pub command: String,
    pub handle: JoinHandle<String>,
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
enum CommandHandlerType {
    Sync(Box<dyn CommandHandler>),
    Async(Box<dyn AsyncCommandHandler>),
}

/// Blanket implementation of `CommandHandler` for closures.
///
/// This allows simple closures to be used as command handlers without
/// explicitly implementing the trait.
impl<F> CommandHandler for F
where
    F: FnMut(&mut TerminalApp, &[&str]) -> String + Send + Sync + 'static,
{
    fn execute(&mut self, app: &mut TerminalApp, args: &[&str]) -> String {
        self(app, args)
    }
}

/// Main terminal application structure managing state and command execution.
///
/// `TerminalApp` provides a complete terminal interface with:
/// - Command history navigation
/// - Cursor management
/// - Custom command registration (sync and async)
/// - Configurable unknown command handling
/// - Non-blocking async command execution
pub struct TerminalApp {
    pub command_history: Vec<String>,
    pub current_input: String,
    pub history_index: Option<usize>,
    pub last_ctrl_c: Option<Instant>,
    pub cursor_position: usize,
    commands: HashMap<String, CommandHandlerType>,
    unknown_command_handler: Option<UnknownCommandHandler>,
    async_unknown_command_handler: Option<AsyncUnknownCommandHandler>,
    command_result_rx: Option<mpsc::UnboundedReceiver<CommandResult>>,
    command_result_tx: Option<mpsc::UnboundedSender<CommandResult>>,
    running_commands: Vec<RunningCommand>,
    last_key_event: Option<KeyEvent>,
}

impl Default for TerminalApp {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalApp {
    /// Removes a character at a specific index in a string.
    fn remove_char_at(&mut self, index: usize) {
        let mut chars: Vec<char> = self.current_input.chars().collect();
        if index < chars.len() {
            chars.remove(index);
            self.current_input = chars.into_iter().collect();
        }
    }

    /// Creates a new terminal application instance with default settings.
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            command_history: Vec::new(),
            current_input: String::new(),
            history_index: None,
            last_ctrl_c: None,
            cursor_position: 0,
            commands: HashMap::new(),
            unknown_command_handler: None,
            async_unknown_command_handler: None,
            command_result_rx: Some(rx),
            command_result_tx: Some(tx),
            running_commands: Vec::new(),
            last_key_event: None,
        }
    }

    /// Registers a synchronous command handler with the application.
    ///
    /// # Arguments
    ///
    /// * `name` - Command name that users will type
    /// * `handler` - Boxed command handler implementing `CommandHandler`
    pub fn register_command<S: Into<String>>(&mut self, name: S, handler: Box<dyn CommandHandler>) {
        self.commands
            .insert(name.into(), CommandHandlerType::Sync(handler));
    }

    /// Registers an asynchronous command handler with the application.
    ///
    /// # Arguments
    ///
    /// * `name` - Command name that users will type
    /// * `handler` - Boxed async command handler implementing `AsyncCommandHandler`
    pub fn register_async_command<S: Into<String>>(
        &mut self,
        name: S,
        handler: Box<dyn AsyncCommandHandler>,
    ) {
        self.commands
            .insert(name.into(), CommandHandlerType::Async(handler));
    }

    /// Sets a custom handler for unknown commands (synchronous).
    ///
    /// # Arguments
    ///
    /// * `handler` - Closure that takes the full command string and returns a response
    pub fn set_unknown_command_handler<F>(&mut self, handler: F)
    where
        F: Fn(&str) -> String + Send + Sync + 'static,
    {
        self.unknown_command_handler = Some(Box::new(handler));
    }

    /// Sets a custom handler for unknown commands (asynchronous).
    ///
    /// # Arguments
    ///
    /// * `handler` - Closure that takes the full command string and returns a future
    pub fn set_async_unknown_command_handler<F>(&mut self, handler: F)
    where
        F: Fn(&str) -> BoxFuture<'static, String> + Send + Sync + 'static,
    {
        self.async_unknown_command_handler = Some(Box::new(handler));
    }

    /// Removes the custom unknown command handler.
    pub fn clear_unknown_command_handler(&mut self) {
        self.unknown_command_handler = None;
        self.async_unknown_command_handler = None;
    }

    /// Initializes the terminal with raw mode and displays startup messages.
    ///
    /// # Arguments
    ///
    /// * `stdout` - Mutable reference to standard output
    /// * `startup_message` - Message to display on startup
    ///
    /// # Errors
    ///
    /// Returns an error if terminal initialization fails.
    pub async fn init_terminal(
        &mut self,
        stdout: &mut Stdout,
        startup_message: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        enable_raw_mode()?;
        writeln!(stdout, "{}", startup_message)?;
        self.render_input_line(stdout)?;
        Ok(())
    }

    /// Processes a single terminal event and returns whether the app should quit.
    ///
    /// # Arguments
    ///
    /// * `event` - Terminal event to process
    /// * `stdout` - Mutable reference to standard output
    ///
    /// # Returns
    ///
    /// `Ok(true)` if the application should exit, `Ok(false)` otherwise
    ///
    /// # Errors
    ///
    /// Returns an error if event processing fails.
    pub async fn process_event(
        &mut self,
        event: Event,
        stdout: &mut Stdout,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let mut should_quit = false;

        if let Event::Key(key_event) = &event {
            // Debugging codes, so fuck you Windows
            // fyi: https://github.com/crossterm-rs/crossterm/pull/745
            //
            // if key_event.kind == KeyEventKind::Press {
            //     println!("[raw_debug]Key press: {}", key_event.code)
            // }
            // if key_event.kind == KeyEventKind::Release {
            //     println!("[raw_debug]Key release: {}", key_event.code);
            // }
            //

            if key_event.kind == KeyEventKind::Release {
                return Ok(should_quit);
            }

            if let Some(last_event) = &self.last_key_event {
                if last_event.code == key_event.code
                    && last_event.modifiers == key_event.modifiers
                    && last_event.kind == key_event.kind
                {
                    let is_control_key = match key_event.code {
                        KeyCode::Char('c') if key_event.modifiers == KeyModifiers::CONTROL => true,
                        KeyCode::Char('d') if key_event.modifiers == KeyModifiers::CONTROL => true,
                        _ => false,
                    };

                    if !is_control_key {
                        return Ok(should_quit);
                    }
                }
            }

            match key_event.code {
                KeyCode::Char('c') if key_event.modifiers == KeyModifiers::CONTROL => {
                    self.last_key_event = Some(*key_event);
                }
                KeyCode::Char('d') if key_event.modifiers == KeyModifiers::CONTROL => {
                    self.last_key_event = Some(*key_event);
                }
                _ => {}
            }
        }

        if let Event::Key(KeyEvent {
            code, modifiers, ..
        }) = event
        {
            match code {
                KeyCode::Char('d') if modifiers == KeyModifiers::CONTROL => {
                    should_quit = self.handle_ctrl_d().await?;
                }
                KeyCode::Char('c') if modifiers == KeyModifiers::CONTROL => {
                    let (quit, message) = self.handle_ctrl_c().await?;
                    should_quit = quit;
                    self.print_log_entry(stdout, &message).await?;
                }
                KeyCode::Up => {
                    self.handle_up_key();
                    self.render_input_line(stdout)?;
                }
                KeyCode::Down => {
                    self.handle_down_key();
                    self.render_input_line(stdout)?;
                }
                KeyCode::Left => {
                    if self.cursor_position > 0 {
                        self.cursor_position -= 1;
                        self.render_input_line(stdout)?;
                    }
                }
                KeyCode::Right => {
                    if self.cursor_position < self.current_input.chars().count() {
                        self.cursor_position += 1;
                        self.render_input_line(stdout)?;
                    }
                }
                KeyCode::Enter => {
                    self.handle_enter_key(stdout, "> ").await?;
                }
                KeyCode::Char(c) => {
                    self.handle_char_input(c);
                    self.render_input_line(stdout)?;
                }
                KeyCode::Backspace => {
                    if self.cursor_position > 0 {
                        self.remove_char_at(self.cursor_position - 1);
                        self.cursor_position -= 1;
                        self.render_input_line(stdout)?;
                    }
                }
                _ => {}
            }
        }
        Ok(should_quit)
    }

    /// Shuts down the terminal and displays exit messages.
    ///
    /// # Arguments
    ///
    /// * `stdout` - Mutable reference to standard output
    /// * `exit_message` - Message to display on exit
    ///
    /// # Errors
    ///
    /// Returns an error if terminal shutdown fails.
    pub async fn shutdown_terminal(
        &mut self,
        stdout: &mut Stdout,
        exit_message: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        disable_raw_mode()?;
        writeln!(stdout, "{}", exit_message)?;
        stdout.flush()?;
        Ok(())
    }

    /// Convenience method that runs the complete terminal application lifecycle.
    ///
    /// This method encapsulates initialization, event loop, and shutdown.
    ///
    /// # Arguments
    ///
    /// * `startup_message` - Message to display on startup
    /// * `exit_message` - Message to display on exit
    ///
    /// # Errors
    ///
    /// Returns an error if any part of the application lifecycle fails.
    pub async fn run(
        &mut self,
        startup_message: &str,
        exit_message: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut stdout = stdout();
        self.init_terminal(&mut stdout, startup_message).await?;

        let mut rx = self.command_result_rx.take().unwrap();

        let result = async {
            loop {
                tokio::select! {
                    // Handle terminal events (non-blocking)
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(50)) => {
                        // Check if events are available without blocking
                        if poll(std::time::Duration::from_millis(0))? {
                            if let Ok(event) = event::read() {
                                if self.process_event(event, &mut stdout).await? {
                                    break;
                                }
                            }
                        }
                    }

                    // Handle completed async commands
                    Some(result) = rx.recv() => {
                        self.handle_command_result(result, &mut stdout).await?;
                    }

                    // Check for completed running commands
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {
                        self.check_running_commands(&mut stdout).await?;
                    }
                }
            }
            Ok::<(), Box<dyn std::error::Error>>(())
        }
        .await;

        // Cancel any remaining running commands
        for cmd in &self.running_commands {
            cmd.handle.abort();
        }

        self.shutdown_terminal(&mut stdout, exit_message).await?;
        result
    }

    /// Clear the current input line and re-renders it.
    pub fn clear_input_line(
        &mut self,
        stdout: &mut Stdout,
    ) -> Result<(), Box<dyn std::error::Error>> {
        execute!(
            stdout,
            cursor::MoveToColumn(0),
            Clear(ClearType::CurrentLine)
        )?;
        Ok(())
    }

    /// Prints a log entry while preserving the input line.
    ///
    /// Clears the current line, prints the log message, and re-renders the input line.
    ///
    /// # Arguments
    ///
    /// * `stdout` - Mutable reference to standard output
    /// * `log_line` - Log message to display
    ///
    /// # Errors
    ///
    /// Returns an error if writing to stdout fails.
    pub async fn print_log_entry(
        &mut self,
        stdout: &mut Stdout,
        log_line: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.clear_input_line(stdout)?;
        writeln!(stdout, "{}", log_line)?;
        self.render_input_line(stdout)?;
        Ok(())
    }

    /// Renders the input line with prompt and cursor positioning.
    fn render_input_line(
        &mut self,
        stdout: &mut Stdout,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            execute!(stdout, cursor::Hide)?;
            self.clear_input_line(stdout)?;
            execute!(
                stdout,
                crossterm::style::Print("> "),
                crossterm::style::Print(&self.current_input)
            )?;
            let visual_cursor_pos = 2 + self
                .current_input
                .chars()
                .take(self.cursor_position)
                .map(|c| c.width().unwrap_or(0))
                .sum::<usize>();
            execute!(
                stdout,
                cursor::MoveToColumn(visual_cursor_pos as u16),
                cursor::Show
            )?;
            stdout.flush()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = execute!(stdout, cursor::Show);
        }
        result
    }

    /// Handles Ctrl+D key press, signaling application exit.
    ///
    /// # Returns
    ///
    /// `Ok(true)` to signal the application should quit.
    pub async fn handle_ctrl_d(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(true)
    }

    /// Handles Ctrl+C key press with double-press confirmation.
    ///
    /// The first press clears input, the second press within 5 seconds exits.
    ///
    /// # Returns
    ///
    /// Tuple of (should_quit, message_to_display)
    pub async fn handle_ctrl_c(&mut self) -> Result<(bool, String), Box<dyn std::error::Error>> {
        if !self.current_input.is_empty() {
            self.current_input.clear();
            self.cursor_position = 0;
            self.last_ctrl_c = Some(Instant::now());
            return Ok((
                false,
                get_info!("Input cleared. Press Ctrl+C again to exit."),
            ));
        }
        if let Some(last_time) = self.last_ctrl_c
            && last_time.elapsed().as_secs() < 5
        {
            return Ok((true, get_warn!("Exiting application. Goodbye!")));
        }
        self.last_ctrl_c = Some(Instant::now());
        Ok((false, get_info!("Press Ctrl+C again to exit.")))
    }

    /// Handles up the arrow key press for command history navigation.
    fn handle_up_key(&mut self) {
        if self.command_history.is_empty() {
            return;
        }
        let new_index = match self.history_index {
            Some(idx) if idx > 0 => idx - 1,
            Some(_) => return,
            None => self.command_history.len() - 1,
        };
        self.history_index = Some(new_index);
        self.current_input = self.command_history[new_index].clone();
        self.cursor_position = self.current_input.chars().count();
    }

    /// Handles down the arrow key press for command history navigation.
    fn handle_down_key(&mut self) {
        let new_index = match self.history_index {
            Some(idx) if idx < self.command_history.len() - 1 => idx + 1,
            Some(_) => {
                self.history_index = None;
                self.current_input.clear();
                self.cursor_position = 0;
                return;
            }
            None => return,
        };
        self.history_index = Some(new_index);
        self.current_input = self.command_history[new_index].clone();
        self.cursor_position = self.current_input.chars().count();
    }

    /// Handles Enter key press to execute the current command.
    ///
    /// # Arguments
    ///
    /// * `stdout` - Mutable reference to standard output
    /// * `input_prefix` - Prefix to display before echoing the command
    ///
    /// # Errors
    ///
    /// Returns an error if writing to stdout fails.
    pub async fn handle_enter_key(
        &mut self,
        stdout: &mut Stdout,
        input_prefix: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !self.current_input.trim().is_empty() {
            self.command_history.push(self.current_input.clone());
            self.clear_input_line(stdout)?;
            writeln!(stdout, "{}{}", input_prefix, self.current_input)?;
            let input_copy = self.current_input.clone();
            let command_output = self.execute_command(&input_copy).await;
            if !command_output.is_empty() {
                for line in command_output.lines() {
                    execute!(stdout, cursor::MoveToColumn(0))?;
                    writeln!(stdout, "{}", line.trim_start())?;
                }
            } else {
                writeln!(stdout)?;
            }
            self.current_input.clear();
            self.cursor_position = 0;
            self.history_index = None;
            self.render_input_line(stdout)?;
        } else {
            self.clear_input_line(stdout)?;
            self.render_input_line(stdout)?;
        }
        Ok(())
    }

    /// Executes a command by looking it up in the registered commands.
    ///
    /// For sync commands, executes immediately and returns the result.
    /// For async commands, spawns them in the background and returns immediately.
    ///
    /// # Arguments
    ///
    /// * `command` - Full command string including arguments
    ///
    /// # Returns
    ///
    /// String output from the command execution (empty for async commands)
    pub async fn execute_command(&mut self, command: &str) -> String {
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return String::new();
        }

        let cmd_name = parts[0];
        let args = &parts[1..];

        if let Some(handler) = self.commands.get(cmd_name) {
            match handler {
                CommandHandlerType::Sync(_) => {
                    // Remove, execute, and put back sync handler
                    if let Some(CommandHandlerType::Sync(mut sync_handler)) =
                        self.commands.remove(cmd_name)
                    {
                        let result = sync_handler.execute(self, args);
                        self.commands
                            .insert(cmd_name.to_string(), CommandHandlerType::Sync(sync_handler));
                        result
                    } else {
                        error!("Internal error: sync handler not found")
                    }
                }
                CommandHandlerType::Async(async_handler) => {
                    // Clone the async handler for execution
                    let cloned_handler = async_handler.box_clone();
                    match self
                        .spawn_async_command(command.to_string(), cloned_handler)
                        .await
                    {
                        Ok(_) => {
                            info!(&format!(
                                "Async command '{}' started in the background",
                                cmd_name
                            ))
                        }
                        Err(e) => {
                            error!(&format!("Failed to spawn async command: {}", e))
                        }
                    }
                }
            }
        } else {
            if let Some(ref handler) = self.async_unknown_command_handler {
                handler(command).await
            } else if let Some(ref handler) = self.unknown_command_handler {
                handler(command)
            } else {
                get_warn!(&format!("Unknown command: '{}'", command))
            }
        }
    }

    /// Handles character input by inserting at the cursor position.
    fn handle_char_input(&mut self, c: char) {
        let char_count = self.current_input.chars().count();

        if self.cursor_position > char_count {
            self.cursor_position = char_count;
        }

        let mut chars: Vec<char> = self.current_input.chars().collect();
        chars.insert(self.cursor_position, c);
        self.current_input = chars.into_iter().collect();
        self.cursor_position += 1;
    }

    /// Log info-level messages.
    ///
    /// This method ensures proper terminal line management by clearing the current
    /// input line, printing the log message, and then re-rendering the input line.
    ///
    /// # Arguments
    ///
    /// * `message` - The message content to be logged.
    ///
    /// # Examples
    ///
    /// ```
    /// use daemon_console::TerminalApp;
    ///
    /// fn main() {
    ///     let mut app = TerminalApp::new();
    ///     // let mut stdout = stdout();
    ///     app.info("Application started successfully!");
    ///     app.info("Running tasks...");
    ///     // app.shutdown_terminal(&mut stdout, "ok");
    /// }
    /// ```
    pub fn info(&mut self, message: &str) {
        let mut stdout = stdout();
        let _ = self.print_log_entry(&mut stdout, &*get_info!(message));
    }

    /// Log warn-level messages.
    ///
    /// # Examples
    ///
    /// ```
    /// use daemon_console::TerminalApp;
    ///
    /// fn main() {
    ///     let mut app = TerminalApp::new();
    ///     app.warn("You get a warning!");
    ///     app.warn("Continue running...");
    /// }
    /// ```
    pub fn warn(&mut self, message: &str) {
        let mut stdout = stdout();
        let _ = self.print_log_entry(&mut stdout, &*get_warn!(message));
    }

    /// Log error-level messages.
    ///
    /// # Examples
    ///
    /// ```
    /// use daemon_console::TerminalApp;
    ///
    /// fn main() {
    ///     let mut app = TerminalApp::new();
    ///     app.error("An error occurred!");
    ///     app.error("Failed to run tasks.");
    /// }
    /// ```
    pub fn error(&mut self, message: &str) {
        let mut stdout = stdout();
        let _ = self.print_log_entry(&mut stdout, &*get_error!(message));
    }

    /// Log critical-level messages.
    ///
    /// # Examples
    ///
    /// ```
    /// use daemon_console::TerminalApp;
    ///
    /// fn main() {
    ///     let mut app = TerminalApp::new();
    ///     app.critical("Application crashed!");
    ///     app.critical("Exception: unknown.");
    /// }
    /// ```
    pub fn critical(&mut self, message: &str) {
        let mut stdout = stdout();
        let _ = self.print_log_entry(&mut stdout, &*get_critical!(message));
    }

    /// Handles completed command results from async commands
    async fn handle_command_result(
        &mut self,
        result: CommandResult,
        stdout: &mut Stdout,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !result.output.is_empty() {
            for line in result.output.lines() {
                self.print_log_entry(stdout, line.trim_start()).await?;
            }
        }
        Ok(())
    }

    /// Checks for completed running commands and cleans up finished tasks
    async fn check_running_commands(
        &mut self,
        _stdout: &mut Stdout,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut completed_indices = Vec::new();

        for (i, cmd) in self.running_commands.iter().enumerate() {
            if cmd.handle.is_finished() {
                // Log completion (using the command field)
                let _ = &cmd.command;
                completed_indices.push(i);
            }
        }

        // Remove completed commands in reverse order to maintain indices
        for &i in completed_indices.iter().rev() {
            self.running_commands.remove(i);
        }

        Ok(())
    }

    /// Spawns an async command in the background
    async fn spawn_async_command(
        &mut self,
        command: String,
        mut handler: Box<dyn AsyncCommandHandler>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let parts: Vec<&str> = command.split_whitespace().collect();
        let args = if parts.len() > 1 { &parts[1..] } else { &[] };
        let args: Vec<String> = args.iter().map(|&s| s.to_string()).collect();
        let tx = self.command_result_tx.as_ref().unwrap().clone();
        let cmd_copy = command.clone();

        let handle = tokio::spawn(async move {
            // Create a temporary app instance for the async command
            let mut temp_app = TerminalApp::new();
            let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let result = handler.execute_async(&mut temp_app, &args_refs).await;

            let _ = tx.send(CommandResult {
                command: cmd_copy,
                output: result.clone(),
            });

            result
        });

        self.running_commands
            .push(RunningCommand { command, handle });

        Ok(())
    }
}
