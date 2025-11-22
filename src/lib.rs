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
//! use std::io::stdout;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut app = TerminalApp::new();
//!     Ok(())
//! }
//! ```
//!
//! See more details in `src/main.rs` in source code.

#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod command;
pub mod events;
pub mod logger;
pub mod utils;

use crossterm::{
    cursor,
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, poll,
    },
    execute,
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode},
};
use futures::future::BoxFuture;
use std::collections::HashMap;
use std::io::{Stdout, Write, stdout};
use std::time::Instant;
use tokio::sync::{broadcast, mpsc};
use unicode_width::UnicodeWidthChar;

pub use crate::command::{
    AsyncCommandHandler, AsyncUnknownCommandHandler, CommandHandler, CommandHandlerType,
    CommandResult, RunningCommand, UnknownCommandHandler,
};
use crate::logger::LogLevel;

/// Actions that can be sent from async commands to the main application
pub enum AppAction {
    /// Register a new command: (command_name, handler)
    RegisterCommand(String, Box<dyn CommandHandler>),
    /// Log an info message
    Info(String),
    /// Log a debug message
    Debug(String),
    /// Log a warn message
    Warn(String),
    /// Log an error message
    Error(String),
    /// Log a critical message
    Critical(String),
}

impl std::fmt::Debug for AppAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppAction::RegisterCommand(name, _) => f
                .debug_struct("RegisterCommand")
                .field("name", name)
                .field("handler", &"Box<dyn CommandHandler>")
                .finish(),
            AppAction::Info(msg) => f.debug_tuple("Info").field(msg).finish(),
            AppAction::Debug(msg) => f.debug_tuple("Debug").field(msg).finish(),
            AppAction::Warn(msg) => f.debug_tuple("Warn").field(msg).finish(),
            AppAction::Error(msg) => f.debug_tuple("Error").field(msg).finish(),
            AppAction::Critical(msg) => f.debug_tuple("Critical").field(msg).finish(),
        }
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
    pub stdout_handle: Stdout,
    pub command_history: Vec<String>,
    pub current_input: String,
    pub history_index: Option<usize>,
    pub last_ctrl_c: Option<Instant>,
    pub cursor_position: usize,
    pub should_exit: bool,
    pub(crate) commands: HashMap<String, CommandHandlerType>,
    pub(crate) unknown_command_handler: Option<UnknownCommandHandler>,
    pub(crate) async_unknown_command_handler: Option<AsyncUnknownCommandHandler>,
    command_result_rx: Option<mpsc::UnboundedReceiver<CommandResult>>,
    command_result_tx: Option<mpsc::UnboundedSender<CommandResult>>,
    running_commands: Vec<RunningCommand>,
    last_key_event: Option<KeyEvent>,
    dispatch_event: bool,
    pub action_sender: Option<mpsc::UnboundedSender<AppAction>>,
    pub action_receiver: Option<mpsc::UnboundedReceiver<AppAction>>,
    pub events_tx: Option<broadcast::Sender<events::DaemonConsoleEvent>>,
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
        let (events_tx, _events_rx) = broadcast::channel::<events::DaemonConsoleEvent>(256);
        let (action_tx, action_rx) = mpsc::unbounded_channel();
        Self {
            stdout_handle: stdout(),
            command_history: Vec::new(),
            current_input: String::new(),
            history_index: None,
            last_ctrl_c: None,
            cursor_position: 0,
            should_exit: false,
            commands: HashMap::new(),
            unknown_command_handler: None,
            async_unknown_command_handler: None,
            command_result_rx: Some(rx),
            command_result_tx: Some(tx),
            running_commands: Vec::new(),
            last_key_event: None,
            action_sender: Some(action_tx),
            action_receiver: Some(action_rx),
            events_tx: Some(events_tx),
            dispatch_event: true,
        }
    }

    /// Gets a clone of the action sender for communication with async commands
    pub fn get_action_sender(&self) -> Option<mpsc::UnboundedSender<AppAction>> {
        self.action_sender.clone()
    }

    /// Toggles the event dispatch flag
    fn switch_if_dispatch_event(&mut self) {
        self.dispatch_event = !self.dispatch_event;
    }

    /// Subscribes to daemon console events
    pub fn subscribe_events(&self) -> Option<broadcast::Receiver<events::DaemonConsoleEvent>> {
        self.events_tx.as_ref().map(|tx| tx.subscribe())
    }

    /// Emits an event to the event channel
    fn emit_events(&self, _event: events::DaemonConsoleEvent) {
        if let Some(tx) = &self.events_tx {
            let _ = tx.send(_event);
        }
    }

    /// Registers a synchronous command with the terminal application
    pub fn register_command<S: Into<String>>(&mut self, name: S, handler: Box<dyn CommandHandler>) {
        self.commands
            .insert(name.into(), CommandHandlerType::PubSync(handler));
    }

    /// Registers an asynchronous command with the terminal application
    pub fn register_async_command<S: Into<String>>(
        &mut self,
        name: S,
        handler: Box<dyn AsyncCommandHandler>,
    ) {
        self.commands
            .insert(name.into(), CommandHandlerType::PubAsync(handler));
    }

    /// Sets the action sender for communication with async commands
    ///
    /// # Arguments
    ///
    /// * `sender` - The sender to use for sending actions from async commands
    pub(crate) fn set_action_sender(&mut self, sender: mpsc::UnboundedSender<AppAction>) {
        self.action_sender = Some(sender);
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
    /// * `startup_message` - Message to display on startup
    ///
    /// # Errors
    ///
    /// Returns an error if terminal initialization fails.
    pub async fn init_terminal(
        &mut self,
        startup_message: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.setup_terminal()?;

        if !startup_message.is_empty() {
            self.print_startup_message(startup_message).await?;
        }

        Ok(())
    }

    /// Sets up the terminal in raw mode and enables mouse capture
    fn setup_terminal(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        enable_raw_mode()?;
        execute!(&mut self.stdout_handle, EnableMouseCapture, cursor::Hide)?;
        self.stdout_handle.flush()?;
        Ok(())
    }

    /// Prints the startup message to the terminal
    async fn print_startup_message(
        &mut self,
        message: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        writeln!(self.stdout_handle, "{}", message)?;
        self.stdout_handle.flush()?;
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

            if let Some(last_event) = &self.last_key_event
                && last_event.code == key_event.code
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
                    self.print_log_entry(&message);
                }
                KeyCode::Up => {
                    self.handle_up_key();
                    self.render_input_line()?;
                }
                KeyCode::Down => {
                    self.handle_down_key();
                    self.render_input_line()?;
                }
                KeyCode::Left => {
                    if self.cursor_position > 0 {
                        self.cursor_position -= 1;
                        self.render_input_line()?;
                    }
                }
                KeyCode::Right => {
                    if self.cursor_position < self.current_input.chars().count() {
                        self.cursor_position += 1;
                        self.render_input_line()?;
                    }
                }
                KeyCode::Enter => {
                    let should_exit = self.handle_enter_key("> ").await?;
                    if should_exit {
                        return Ok(true);
                    }
                }
                KeyCode::Char(c) => {
                    self.handle_char_input(c);
                    self.render_input_line()?;
                }
                KeyCode::Backspace => {
                    if self.cursor_position > 0 {
                        self.remove_char_at(self.cursor_position - 1);
                        self.cursor_position -= 1;
                        self.render_input_line()?;
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
        exit_message: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        disable_raw_mode()?;
        writeln!(self.stdout_handle, "{}", exit_message)?;
        self.stdout_handle.flush()?;
        Ok(())
    }

    /// Main application loop that handles terminal input and command execution.
    ///
    /// Initializes terminal event handling, processes keyboard input, and manages
    /// command execution until exit is requested. Handles special key combinations
    /// like Ctrl+C for graceful shutdown.
    ///
    /// # Arguments
    ///
    /// * `startup_message` - Optional message to display on startup
    /// * `exit_message` - Optional message to display on exit
    ///
    /// # Errors
    ///
    /// Returns an error if terminal initialization or event handling fails.
    pub async fn run(
        &mut self,
        startup_message: &str,
        exit_message: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut action_rx = self.action_receiver.take().unwrap();

        enable_raw_mode()?;
        execute!(self.stdout_handle, EnableMouseCapture, cursor::Hide)?;

        if !startup_message.is_empty() {
            self.print_log_entry(startup_message);
        }

        loop {
            // Handle AppAction messages
            while let Ok(action) = action_rx.try_recv() {
                match action {
                    AppAction::RegisterCommand(name, handler) => {
                        self.register_command(name, handler);
                    }
                    AppAction::Info(_)
                    | AppAction::Debug(_)
                    | AppAction::Warn(_)
                    | AppAction::Error(_)
                    | AppAction::Critical(_) => {
                        self.handle_log_action(action);
                    }
                }
            }

            // Check for completed async commands
            self.check_running_commands().await?;

            // Process command results
            if let Some(ref mut rx) = self.command_result_rx
                && let Ok(result) = rx.try_recv()
            {
                self.handle_command_result(result).await?;
            }

            // Handle terminal events (non-blocking)
            tokio::select! {
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(50)) => {
                    // Check if events are available without blocking
                    if poll(std::time::Duration::from_millis(0))?
                        && let Ok(event) = event::read()
                            && self.process_event(event).await? {
                                break;
                            }
                }
            }

            if self.should_exit {
                break;
            }
        }

        disable_raw_mode()?;
        execute!(self.stdout_handle, DisableMouseCapture, cursor::Show)?;

        if !exit_message.is_empty() {
            println!("{}", exit_message);
        }

        Ok(())
    }

    /// Clear the current input line and re-renders it.
    pub fn clear_input_line(&mut self) {
        let _ = execute!(
            self.stdout_handle,
            cursor::MoveToColumn(0),
            Clear(ClearType::CurrentLine)
        );
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
    pub fn print_log_entry(&mut self, log_line: &str) {
        self.clear_input_line();
        let _ = writeln!(self.stdout_handle, "{}", log_line);
        let _ = self.stdout_handle.flush();
        let _ = self.render_input_line();
    }

    /// Renders the input line with prompt and cursor positioning.
    fn render_input_line(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            execute!(self.stdout_handle, cursor::Hide)?;
            self.clear_input_line();
            execute!(
                self.stdout_handle,
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
                self.stdout_handle,
                cursor::MoveToColumn(visual_cursor_pos as u16),
                cursor::Show
            )?;
            self.stdout_handle.flush()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = execute!(self.stdout_handle, cursor::Show);
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
                get_info!(
                    "Input cleared. Press Ctrl+C again to exit.",
                    "Daemon Console"
                ),
            ));
        }
        if let Some(last_time) = self.last_ctrl_c
            && last_time.elapsed().as_secs() < 5
        {
            return Ok((
                true,
                get_warn!("Exiting application. Goodbye!", "Daemon Console"),
            ));
        }
        self.last_ctrl_c = Some(Instant::now());
        Ok((
            false,
            get_info!("Press Ctrl+C again to exit.", "Daemon Console"),
        ))
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

    /// Handles the enter key press event, executing commands and managing input history
    pub async fn handle_enter_key(
        &mut self,
        input_prefix: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if !self.current_input.trim().is_empty() {
            self.command_history.push(self.current_input.clone());
            self.clear_input_line();
            writeln!(self.stdout_handle, "{}{}", input_prefix, self.current_input)?;
            let input_copy = self.current_input.clone();
            self.emit_events(events::DaemonConsoleEvent::UserConsoleInput {
                raw: input_copy.clone(),
                timestamp: events::DaemonConsoleEvent::now_ts(),
            });
            let command_output = command::execute_command(self, &input_copy).await;
            if !command_output.is_empty() {
                for line in command_output.lines() {
                    execute!(self.stdout_handle, cursor::MoveToColumn(0))?;
                    writeln!(self.stdout_handle, "{}", line.trim_start())?;
                }
            } else {
                writeln!(self.stdout_handle)?;
            }
            self.current_input.clear();
            self.cursor_position = 0;
            self.history_index = None;
            self.render_input_line()?;
        } else {
            self.clear_input_line();
            self.render_input_line()?;
        }
        Ok(self.should_exit)
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

    /// Dispatches log events if event dispatching is enabled
    fn dispatch_log_events(&mut self, message: &str, level: LogLevel) {
        if self.dispatch_event {
            self.emit_events(events::DaemonConsoleEvent::TerminalLog {
                level,
                message: message.to_string(),
                module_name: Some("Stream".into()),
                timestamp: events::DaemonConsoleEvent::now_ts(),
            });
        }
    }

    /// Handles log actions with event dispatch toggling
    fn handle_log_action(&mut self, action: AppAction) {
        self.switch_if_dispatch_event();
        match action {
            AppAction::Info(msg) => self.info(&msg),
            AppAction::Debug(msg) => self.debug(&msg),
            AppAction::Warn(msg) => self.warn(&msg),
            AppAction::Error(msg) => self.error(&msg),
            AppAction::Critical(msg) => self.critical(&msg),
            _ => {} // Should not reach here
        }
        self.switch_if_dispatch_event();
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
    /// ```rust
    /// use daemon_console::TerminalApp;
    ///
    /// fn needless_main() {
    ///     let mut app = TerminalApp::new();
    ///     app.info("Application started successfully!");
    ///     app.info("Running tasks...");
    /// }
    /// ```
    pub fn info(&mut self, message: &str) {
        self.print_log_entry(&get_info!(message, "Stream"));
        self.dispatch_log_events(message, LogLevel::Info);
    }

    /// Log debug-level messages.
    ///
    /// # Examples
    ///
    /// ```
    /// use daemon_console::TerminalApp;
    ///
    /// fn needless_main() {
    ///     let mut app = TerminalApp::new();
    ///     app.debug("Debugging information...");
    ///     app.debug("Debugging more...");
    /// }
    /// ```
    pub fn debug(&mut self, message: &str) {
        self.print_log_entry(&get_debug!(message, "Stream"));
        self.dispatch_log_events(message, LogLevel::Debug);
    }

    /// Log warn-level messages.
    ///
    /// # Examples
    ///
    /// ```
    /// use daemon_console::TerminalApp;
    ///
    /// fn needless_main() {
    ///     let mut app = TerminalApp::new();
    ///     app.warn("You get a warning!");
    ///     app.warn("Continue running...");
    /// }
    /// ```
    pub fn warn(&mut self, message: &str) {
        self.print_log_entry(&get_warn!(message, "Stream"));
        self.dispatch_log_events(message, LogLevel::Warn);
    }

    /// Log error-level messages.
    ///
    /// # Examples
    ///
    /// ```
    /// use daemon_console::TerminalApp;
    ///
    /// fn needless_main() {
    ///     let mut app = TerminalApp::new();
    ///     app.error("An error occurred!");
    ///     app.error("Failed to run tasks.");
    /// }
    /// ```
    pub fn error(&mut self, message: &str) {
        self.print_log_entry(&get_error!(message, "Stream"));
        self.dispatch_log_events(message, LogLevel::Error);
    }

    /// Log critical-level messages.
    ///
    /// # Examples
    ///
    /// ```
    /// use daemon_console::TerminalApp;
    ///
    /// fn needless_main() {
    ///     let mut app = TerminalApp::new();
    ///     app.critical("Application crashed!");
    ///     app.critical("Exception: unknown.");
    /// }
    /// ```
    pub fn critical(&mut self, message: &str) {
        self.print_log_entry(&get_critical!(message, "Stream"));
        self.dispatch_log_events(message, LogLevel::Critical);
    }

    /// Handles completed command results from async commands
    async fn handle_command_result(
        &mut self,
        result: CommandResult,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !result.output.is_empty() {
            for line in result.output.lines() {
                self.print_log_entry(line.trim_start());
            }
        }
        Ok(())
    }

    /// Checks for completed running commands and cleans up finished tasks
    async fn check_running_commands(&mut self) -> Result<(), Box<dyn std::error::Error>> {
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

    async fn spawn_async_command(
        &mut self,
        command: String,
        mut handler: Box<dyn AsyncCommandHandler>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let parts: Vec<&str> = command.split_whitespace().collect();
        let args = if parts.len() > 1 { &parts[1..] } else { &[] };
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let tx = self.command_result_tx.as_ref().unwrap().clone();
        let cmd_copy = command.clone();
        // Clone action_sender to pass to the async command
        let action_sender = self.action_sender.clone();

        let handle = tokio::spawn(async move {
            // Create a temporary app instance for the async command
            let mut temp_app = TerminalApp::new();
            // Set the action sender for the temporary app
            if let Some(sender) = action_sender {
                temp_app.set_action_sender(sender);
            }
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
