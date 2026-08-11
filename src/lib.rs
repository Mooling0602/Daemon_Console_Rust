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
pub mod tab;
pub mod utils;

use crossterm::{
    cursor::{self, RestorePosition, SavePosition},
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, poll,
    },
    execute,
    style::{Color, ResetColor, SetForegroundColor},
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode, size},
};
use futures::future::BoxFuture;
use std::collections::HashMap;
use std::io::{Stdout, Write, stdout};
use std::time::Instant;
use tokio::sync::{broadcast, mpsc};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub use crate::command::{
    AsyncCommandHandler, AsyncUnknownCommandHandler, CommandHandler, CommandHandlerType,
    CommandResult, RunningCommand, UnknownCommandHandler,
};
use crate::logger::LogLevel;
use crate::tab::{CompletionCandidate, TabTree};

fn width_of_chars(chars: &[char]) -> usize {
    chars.iter().map(|c| c.width().unwrap_or(0)).sum()
}

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
    /// Unified logger to log any message
    Logger(LogLevel, String, Option<String>, Option<bool>),
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
            AppAction::Logger(level, msg, _, _) => f
                .debug_struct("Logger")
                .field("level", level)
                .field("msg", msg)
                .field("module", &"Option<String>")
                .field("dp_evt", &"Option<bool>")
                .finish(),
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
    /// Application name, could be set to any valid text your like.
    pub app_name: String,
    /// Whether raw mode is enabled
    pub raw_mode_enabled: bool,
    pub(crate) commands: HashMap<String, CommandHandlerType>,
    pub(crate) unknown_command_handler: Option<UnknownCommandHandler>,
    pub(crate) async_unknown_command_handler: Option<AsyncUnknownCommandHandler>,
    command_result_rx: Option<mpsc::UnboundedReceiver<CommandResult>>,
    command_result_tx: Option<mpsc::UnboundedSender<CommandResult>>,
    running_commands: Vec<RunningCommand>,
    last_key_event: Option<KeyEvent>,
    dispatch_event: bool,
    /// Maximum number of tab completion nodes allowed
    tab_completion_limit: usize,
    /// Temporary storage for current input when browsing history
    pending_input: Option<String>,
    /// Cursor position for pending input
    pending_cursor_position: usize,
    /// Whether completions are currently hidden
    completions_hidden: bool,
    /// Whether focus is currently on completions (true) or text input (false)
    focus_on_completions: bool,
    pub action_sender: Option<mpsc::UnboundedSender<AppAction>>,
    pub action_receiver: Option<mpsc::UnboundedReceiver<AppAction>>,
    pub events_tx: Option<broadcast::Sender<events::DaemonConsoleEvent>>,
    is_shadow: bool,
    tab_tree: Option<TabTree>,
    current_completions: Vec<CompletionCandidate>,
    hints_rendered: bool,
    selected_completion_index: usize,
    warned_no_tab_tree: bool,
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
            app_name: String::from("Daemon Console"),
            raw_mode_enabled: false,
            commands: HashMap::new(),
            unknown_command_handler: None,
            async_unknown_command_handler: None,
            command_result_rx: Some(rx),
            command_result_tx: Some(tx),
            running_commands: Vec::new(),
            last_key_event: None,
            dispatch_event: true,
            tab_completion_limit: 10000,
            pending_input: None,
            pending_cursor_position: 0,
            completions_hidden: false,
            focus_on_completions: false,
            action_sender: Some(action_tx),
            action_receiver: Some(action_rx),
            events_tx: Some(events_tx),
            is_shadow: false,
            tab_tree: None,
            current_completions: Vec::new(),
            hints_rendered: false,
            selected_completion_index: 0,
            warned_no_tab_tree: false,
        }
    }

    /// Gets a clone of the action sender for communication with async commands
    pub fn get_action_sender(&self) -> Option<mpsc::UnboundedSender<AppAction>> {
        self.action_sender.clone()
    }

    /// Enables tab completion and initializes the completion tree.
    pub fn enable_tab_completion(&mut self) {
        if self.tab_tree.is_none() {
            self.tab_tree = Some(TabTree::new());
        } else {
            self.logger(
                LogLevel::Warn,
                "Tab completion is already enabled.",
                None,
                None,
            );
        }
    }

    /// Checks if tab completion is currently enabled.
    pub fn is_tab_completion_enabled(&self) -> bool {
        self.tab_tree.is_some()
    }

    /// Registers completions for a given context.
    ///
    /// # Arguments
    ///
    /// * `context` - The input prefix that triggers these completions (empty string for root)
    /// * `completions` - List of completion texts
    ///
    /// # Examples
    ///
    /// ```
    /// use daemon_console::TerminalApp;
    ///
    /// let mut app = TerminalApp::new();
    /// app.enable_tab_completion();
    /// app.register_tab_completions("!config", &["start", "stop", "restart"]);
    /// ```
    pub fn register_tab_completions(&mut self, context: &str, completions: &[&str]) {
        if let Some(tree) = &mut self.tab_tree {
            let current_count = tree.count_total_items();
            if current_count + completions.len() > self.tab_completion_limit {
                self.logger(
                    LogLevel::Warn,
                    &format!(
                        "Cannot register {} completions: would exceed limit of {}. Current count: {}",
                        completions.len(),
                        self.tab_completion_limit,
                        current_count
                    ),
                    None,
                    None,
                );
                return;
            }
            tree.register_completions(context, completions);
        } else if !self.warned_no_tab_tree {
            self.logger(
                LogLevel::Warn,
                "Tab completion is not enabled. Call enable_tab_completion() first.",
                None,
                None,
            );
            self.warned_no_tab_tree = true;
        }
    }

    /// Registers completions with descriptions.
    ///
    /// # Arguments
    ///
    /// * `context` - The input prefix that triggers these completions
    /// * `items` - List of (text, description) tuples
    pub fn register_tab_completions_with_desc(&mut self, context: &str, items: &[(&str, &str)]) {
        if let Some(tree) = &mut self.tab_tree {
            let current_count = tree.count_total_items();
            if current_count + items.len() > self.tab_completion_limit {
                self.logger(
                    LogLevel::Warn,
                    &format!(
                        "Cannot register {} completions: would exceed limit of {}. Current count: {}",
                        items.len(),
                        self.tab_completion_limit,
                        current_count
                    ),
                    None,
                    None,
                );
                return;
            }

            let duplicates = tree.register_completions_with_desc(context, items);
            for dup in duplicates {
                self.logger(
                    LogLevel::Warn,
                    &format!(
                        "Duplicate completion item '{}' ignored in context '{}'",
                        dup.text,
                        if context.is_empty() {
                            "<root>"
                        } else {
                            context
                        }
                    ),
                    None,
                    None,
                );
            }
        } else if !self.warned_no_tab_tree {
            self.logger(
                LogLevel::Warn,
                "Tab completion is not enabled. Call enable_tab_completion() first.",
                None,
                None,
            );
            self.warned_no_tab_tree = true;
        }
    }

    /// Adds a single completion item to an existing context.
    ///
    /// # Arguments
    ///
    /// * `context` - The context to add to
    /// * `text` - Completion text
    /// * `description` - Optional description
    pub fn add_tab_completion(&mut self, context: &str, text: &str, description: Option<&str>) {
        if let Some(tree) = &mut self.tab_tree {
            let current_count = tree.count_total_items();
            if current_count + 1 > self.tab_completion_limit {
                self.logger(
                    LogLevel::Warn,
                    &format!(
                        "Cannot add completion: would exceed limit of {}. Current count: {}",
                        self.tab_completion_limit, current_count
                    ),
                    None,
                    None,
                );
                return;
            }
            tree.add_completion(context, text, description);
        }
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
                KeyCode::Esc => {
                    self.completions_hidden = true;
                    self.focus_on_completions = false;
                    self.render_input_line()?;
                }
                KeyCode::Up => {
                    if self.focus_on_completions {
                        self.focus_on_completions = false;
                        self.render_input_line()?;
                    } else {
                        self.handle_up_key();
                        self.render_input_line()?;
                    }
                }
                KeyCode::Down => {
                    if self.focus_on_completions {
                    } else {
                        if self.history_index.is_none() {
                            if !self.current_completions.is_empty() && !self.completions_hidden {
                                self.focus_on_completions = true;
                                self.render_input_line()?;
                            }
                        } else {
                            self.handle_down_key();
                            self.render_input_line()?;
                        }
                    }
                }
                KeyCode::Left => {
                    if self.focus_on_completions
                        && !self.completions_hidden
                        && !self.current_completions.is_empty()
                    {
                        if self.selected_completion_index == 0 {
                            self.selected_completion_index = self.current_completions.len() - 1;
                        } else {
                            self.selected_completion_index -= 1;
                        }
                        self.render_input_line()?;
                    } else {
                        self.focus_on_completions = false;
                        if self.cursor_position > 0 {
                            self.cursor_position -= 1;
                            self.render_input_line()?;
                        }
                    }
                }
                KeyCode::Right => {
                    if self.focus_on_completions
                        && !self.completions_hidden
                        && !self.current_completions.is_empty()
                    {
                        if self.selected_completion_index == self.current_completions.len() - 1 {
                            self.selected_completion_index = 0;
                        } else {
                            self.selected_completion_index += 1;
                        }
                        self.render_input_line()?;
                    } else {
                        self.focus_on_completions = false;

                        if self.cursor_position < self.current_input.chars().count() {
                            self.cursor_position += 1;
                            self.render_input_line()?;
                        } else {
                            if !self.current_completions.is_empty() && !self.completions_hidden {
                                self.focus_on_completions = true;
                                self.render_input_line()?;
                            }
                        }
                    }
                }
                KeyCode::Tab => {
                    if !self.completions_hidden && !self.current_completions.is_empty() {
                        self.handle_tab_key();
                    } else {
                        self.completions_hidden = !self.completions_hidden;
                    }
                    self.render_input_line()?;
                }
                KeyCode::Enter => {
                    let should_exit = self.handle_enter_key("> ").await?;
                    if should_exit {
                        return Ok(true);
                    }
                }
                KeyCode::Char(c) => {
                    self.handle_char_input(c);
                    self.update_completions();
                    self.render_input_line()?;
                }
                KeyCode::Backspace if self.cursor_position > 0 => {
                    self.remove_char_at(self.cursor_position - 1);
                    self.cursor_position -= 1;
                    self.update_completions();
                    self.render_input_line()?;
                }
                _ => {}
            }
        } else if let Event::Resize(_, _) = event {
            self.completions_hidden = true;
            self.focus_on_completions = false;
            self.render_input_line()?;
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

    /// Waits for and returns the next user input event.
    ///
    /// This method processes terminal events in a non-blocking manner and returns
    /// when the user presses Enter with non-empty input or when a quit signal is received.
    ///
    /// # Returns
    ///
    /// - `Ok(Some(String))` - User entered a non-empty string
    /// - `Ok(None)` - User should exit (Ctrl+C, Ctrl+D, or should_exit flag set)
    ///
    /// # Errors
    ///
    /// Returns an error if terminal event processing fails.
    pub async fn read_input(&mut self) -> Result<Option<String>, Box<dyn std::error::Error>> {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(50)) => {
                    if poll(std::time::Duration::from_millis(0))?
                        && let Ok(event) = event::read() {
                            if let Event::Key(KeyEvent { code: KeyCode::Enter, .. }) = event {
                                let saved_input = self.current_input.clone();
                                let should_exit = self.handle_enter_key("> ").await?;
                                if should_exit {
                                    return Ok(None);
                                }
                                if !saved_input.trim().is_empty() {
                                    return Ok(Some(saved_input));
                                }
                            } else if self.process_event(event).await? {
                                return Ok(None);
                            }
                        }
                }
            }

            if self.should_exit {
                return Ok(None);
            }
        }
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

        let loop_result: Result<(), Box<dyn std::error::Error>> = async {
            enable_raw_mode()?;
            execute!(self.stdout_handle, EnableMouseCapture, cursor::Hide)?;

            if !startup_message.is_empty() {
                self.print_log_entry(startup_message);
            }

            loop {
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
                        AppAction::Logger(level, message, module_name, dispatch_event) => {
                            self.handle_logger_action(level, message, module_name, dispatch_event);
                        }
                    }
                }

                self.check_running_commands().await?;

                if let Some(ref mut rx) = self.command_result_rx
                    && let Ok(result) = rx.try_recv()
                {
                    self.handle_command_result(result).await?;
                }

                tokio::select! {
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(50)) => {
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
            #[allow(unreachable_code)]
            Ok(())
        }
        .await;

        let cleanup_result: Result<(), Box<dyn std::error::Error>> = (|| {
            disable_raw_mode()?;
            execute!(self.stdout_handle, DisableMouseCapture, cursor::Show)?;
            Ok(())
        })();

        if !exit_message.is_empty() {
            println!("{}", exit_message);
        }

        loop_result.and(cleanup_result)
    }

    /// Clears the current input line and completion hints if rendered.
    ///
    /// If `hints_rendered` is true, this clears both the input line and the line below it
    /// containing completion hints. Otherwise, only the current line is cleared.
    pub fn clear_input_line(&mut self) {
        if self.hints_rendered {
            let _ = execute!(
                self.stdout_handle,
                cursor::MoveToColumn(0),
                Clear(ClearType::CurrentLine),
                cursor::MoveDown(1),
                Clear(ClearType::CurrentLine),
                cursor::MoveUp(1),
                cursor::MoveToColumn(0)
            );
            self.hints_rendered = false;
        } else {
            let _ = execute!(
                self.stdout_handle,
                cursor::MoveToColumn(0),
                Clear(ClearType::CurrentLine)
            );
        }
    }

    /// Prints a log entry while preserving the input line.
    ///
    /// Clears the input line, outputs the log message, then re-renders the input line
    /// on a new line without clearing first.
    pub fn print_log_entry(&mut self, log_line: &str) {
        self.clear_input_line();
        if log_line.contains('\n') {
            for line in log_line.lines() {
                let _ = writeln!(self.stdout_handle, "{}", line);
                let _ = execute!(self.stdout_handle, cursor::MoveToColumn(0));
            }
        } else {
            let _ = writeln!(self.stdout_handle, "{}", log_line);
        }

        let _ = execute!(self.stdout_handle, cursor::MoveToColumn(0));
        let _ = self.render_input_line_no_clear();
    }

    /// Renders the input line with prompt, text, and completion hints.
    ///
    /// Clears the current line first, then displays the prompt and input text.
    /// If completions are available, renders hints below the input line.
    /// Finally, positions the cursor at `cursor_position`.
    fn render_input_line(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            execute!(self.stdout_handle, cursor::Hide)?;
            self.clear_input_line();
            self.render_input_content()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = execute!(self.stdout_handle, cursor::Show);
        }
        result
    }

    /// Truncates a string to the specified maximum length, adding "..." if truncated.
    ///
    /// # Arguments
    ///
    /// * `text` - The text to truncate
    /// * `max_length` - Maximum length including the "..." suffix
    ///
    /// # Returns
    ///
    /// Truncated string with "..." if it exceeds max_length, otherwise the original string
    fn truncate_text(&self, text: &str, max_length: usize) -> String {
        if text.chars().count() <= max_length {
            return text.to_string();
        }

        if max_length <= 3 {
            return "...".to_string();
        }

        let truncated_chars: Vec<char> = text.chars().take(max_length - 3).collect();
        format!("{}...", truncated_chars.iter().collect::<String>())
    }

    /// Renders completion hints below the input line.
    ///
    /// This method dynamically calculates which completion candidates fits within the current
    /// terminal width, ensuring the selected candidate is always visible.
    fn render_completion_hints(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.completions_hidden {
            self.hints_rendered = false;
            return Ok(());
        }

        let (term_cols, _) = size()?;
        let term_width = (term_cols as usize).saturating_sub(1);

        let total_count = self.current_completions.len();
        if total_count == 0 {
            self.hints_rendered = false;
            return Ok(());
        }

        let get_display_text = |app: &TerminalApp, idx: usize, max_len: usize| -> String {
            let candidate = &app.current_completions[idx];
            let mut item_text = String::from("[");

            if candidate.completion.is_empty() && candidate.description.is_some() {
                item_text.clear();
                item_text.push('<');
                item_text.push_str(candidate.description.as_ref().unwrap_or(&String::from("")));
                item_text.push('>');
            } else if app.current_completions.len() == 1 {
                let truncated_completion = app.truncate_text(&candidate.completion, max_len);
                item_text.push_str(&truncated_completion);
                if let Some(desc) = &candidate.description {
                    item_text.push_str(": ");
                    item_text.push_str(desc);
                }
                item_text.push(']');
            } else {
                let truncated_completion = app.truncate_text(&candidate.completion, max_len);
                item_text.push_str(&truncated_completion);
                if let Some(desc) = &candidate.description {
                    item_text.push_str(": ");
                    let truncated_desc = app.truncate_text(desc, max_len);
                    item_text.push_str(&truncated_desc);
                }
                item_text.push(']');
            }
            item_text
        };

        let dynamic_max_len = term_width.saturating_sub(6).max(10);

        let mut start_idx = self.selected_completion_index;
        let mut end_idx = self.selected_completion_index + 1;

        let selected_text = get_display_text(self, self.selected_completion_index, dynamic_max_len);
        let mut current_width = selected_text.width();

        loop {
            let hidden_left = start_idx;
            let hidden_right = total_count - end_idx;

            let left_marker_width = if hidden_left > 0 {
                format!(" (+{})", hidden_left).width()
            } else {
                0
            };
            let right_marker_width = if hidden_right > 0 {
                format!(" (+{})", hidden_right).width()
            } else {
                0
            };

            let extra_left_space = if hidden_left > 0 { 1 } else { 0 };

            let total_needed =
                left_marker_width + extra_left_space + current_width + right_marker_width;

            if total_needed > term_width {
                break;
            }

            let can_go_left = start_idx > 0;
            let can_go_right = end_idx < total_count;

            if !can_go_left && !can_go_right {
                break;
            }

            let left_count = self.selected_completion_index - start_idx;
            let right_count = end_idx - 1 - self.selected_completion_index;

            let mut added = false;

            if can_go_left && (left_count <= right_count || !can_go_right) {
                let prev_idx = start_idx - 1;
                let text = get_display_text(self, prev_idx, dynamic_max_len);
                let added_width = 1 + text.width();

                let new_hidden_left = prev_idx;
                let new_left_marker = if new_hidden_left > 0 {
                    format!(" (+{})", new_hidden_left).width()
                } else {
                    0
                };
                let new_extra_space = if new_hidden_left > 0 { 1 } else { 0 };

                let new_content_width = current_width + added_width;
                if new_left_marker + new_extra_space + new_content_width + right_marker_width
                    <= term_width
                {
                    start_idx = prev_idx;
                    current_width += added_width;
                    added = true;
                }
            }

            if !added && can_go_right {
                let next_idx = end_idx;
                let text = get_display_text(self, next_idx, dynamic_max_len);
                let added_width = 1 + text.width();

                let new_hidden_right = total_count - (next_idx + 1);
                let new_right_marker = if new_hidden_right > 0 {
                    format!(" (+{})", new_hidden_right).width()
                } else {
                    0
                };

                let current_total_left = left_marker_width + extra_left_space;

                let new_content_width = current_width + added_width;
                if current_total_left + new_content_width + new_right_marker <= term_width {
                    end_idx += 1;
                    current_width += added_width;
                    added = true;
                }
            }

            if !added {
                break;
            }
        }

        execute!(
            self.stdout_handle,
            SavePosition,
            crossterm::style::Print("\n"),
            cursor::MoveToColumn(0)
        )?;

        let hidden_left = start_idx;
        let hidden_right = total_count - end_idx;

        if hidden_left > 0 {
            execute!(
                self.stdout_handle,
                SetForegroundColor(Color::DarkGrey),
                crossterm::style::Print(&format!(" (+{})", hidden_left))
            )?;
        }

        for idx in start_idx..end_idx {
            if idx > start_idx || hidden_left > 0 {
                execute!(self.stdout_handle, crossterm::style::Print(" "))?;
            }

            let is_selected = idx == self.selected_completion_index;
            let color = if is_selected && self.focus_on_completions {
                Color::Cyan
            } else {
                Color::DarkGrey
            };

            execute!(self.stdout_handle, SetForegroundColor(color))?;

            let item_text = get_display_text(self, idx, dynamic_max_len);
            execute!(self.stdout_handle, crossterm::style::Print(&item_text))?;
        }

        if hidden_right > 0 {
            execute!(
                self.stdout_handle,
                SetForegroundColor(Color::DarkGrey),
                crossterm::style::Print(&format!(" (+{})", hidden_right))
            )?;
        }

        execute!(
            self.stdout_handle,
            ResetColor,
            Clear(ClearType::UntilNewLine),
            RestorePosition
        )?;

        self.hints_rendered = true;
        Ok(())
    }

    /// Renders prompt, input text, and completion hints.
    ///
    /// This is the core rendering logic shared by both `render_input_line()`
    /// and `render_input_line_no_clear()`.
    ///
    /// Handles text truncation if the input line exceeds the terminal width,
    /// adding "..." at the start or end to keep the cursor visible.
    fn render_input_content(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let (term_cols, _) = size()?;
        let term_width = term_cols as usize;
        let prompt_width = 2;
        let available_width = term_width.saturating_sub(prompt_width).saturating_sub(1);

        let input_chars: Vec<char> = self.current_input.chars().collect();
        let input_len = input_chars.len();

        let total_input_width = self.current_input.width();

        let (display_text, visual_idx_start) = if total_input_width <= available_width {
            (self.current_input.clone(), 0)
        } else {
            let cursor_char_idx = self.cursor_position;

            let char_widths: Vec<usize> =
                input_chars.iter().map(|c| c.width().unwrap_or(0)).collect();

            let mut idx_l;
            let mut idx_r;

            let mut current_width = 0;
            let mut limit = 0;
            for (i, w) in char_widths.iter().enumerate() {
                if current_width + w + 3 > available_width {
                    break;
                }
                current_width += w;
                limit = i + 1;
            }

            if cursor_char_idx <= limit && limit < input_len {
                idx_l = 0;
                idx_r = limit;
            } else {
                let mut tail_width = 0;
                let mut start_from = input_len;
                for (i, w) in char_widths.iter().enumerate().rev() {
                    if tail_width + w + 3 > available_width {
                        break;
                    }
                    tail_width += w;
                    start_from = i;
                }

                if cursor_char_idx >= start_from {
                    idx_l = start_from;
                    idx_r = input_len;
                } else {
                    let content_budget = available_width.saturating_sub(6);
                    idx_l = cursor_char_idx;
                    idx_r = cursor_char_idx;

                    if cursor_char_idx == input_len {
                        idx_l = input_len.saturating_sub(1);
                        idx_r = input_len;
                    }

                    let mut used = 0;
                    loop {
                        let mut expanded = false;
                        if idx_l > 0 && used + char_widths[idx_l - 1] <= content_budget {
                            idx_l -= 1;
                            used += char_widths[idx_l];
                            expanded = true;
                        }
                        if idx_r < input_len && used + char_widths[idx_r] <= content_budget {
                            used += char_widths[idx_r];
                            idx_r += 1;
                            expanded = true;
                        }
                        if !expanded {
                            break;
                        }
                    }
                }
            }

            let sub: String = input_chars[idx_l..idx_r].iter().collect();
            let mut out = String::new();

            let has_left_ellipsis = idx_l > 0;
            let has_right_ellipsis = idx_r < input_len;

            if has_left_ellipsis {
                out.push_str("...");
            }
            out.push_str(&sub);
            if has_right_ellipsis {
                out.push_str("...");
            }

            (
                out,
                if has_left_ellipsis { 3 } else { 0 }
                    + width_of_chars(&input_chars[idx_l..cursor_char_idx]),
            )
        };

        execute!(
            self.stdout_handle,
            crossterm::style::Print("> "),
            crossterm::style::Print(&display_text)
        )?;

        if !self.current_completions.is_empty() {
            self.render_completion_hints()?;
        }

        let visual_cursor_col = if total_input_width <= available_width {
            2 + self
                .current_input
                .chars()
                .take(self.cursor_position)
                .map(|c| c.width().unwrap_or(0))
                .sum::<usize>()
        } else {
            2 + visual_idx_start
        };

        execute!(
            self.stdout_handle,
            cursor::MoveToColumn(visual_cursor_col as u16),
            cursor::Show
        )?;
        self.stdout_handle.flush()?;
        Ok(())
    }

    /// Renders the input line without clearing first.
    ///
    /// Used after log output where the cursor is already on a new line.
    /// Ensures the cursor starts at column 0, then renders prompt, input text,
    /// and completion hints if available.
    fn render_input_line_no_clear(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            execute!(self.stdout_handle, cursor::Hide)?;
            execute!(self.stdout_handle, cursor::MoveToColumn(0))?;
            self.render_input_content()?;
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
        self.current_completions.clear();
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
        self.current_completions.clear();
        if !self.current_input.is_empty() {
            self.current_input.clear();
            self.cursor_position = 0;
            self.last_ctrl_c = Some(Instant::now());
            return Ok((
                false,
                get_info!("Input cleared. Press Ctrl+C again to exit.", &self.app_name),
            ));
        }
        if let Some(last_time) = self.last_ctrl_c
            && last_time.elapsed().as_secs() < 5
        {
            return Ok((
                true,
                get_warn!("Exiting application. Goodbye!", &self.app_name),
            ));
        }
        self.last_ctrl_c = Some(Instant::now());
        Ok((
            false,
            get_info!("Press Ctrl+C again to exit.", &self.app_name),
        ))
    }

    /// Handles up the arrow key press for command history navigation.
    fn handle_up_key(&mut self) {
        if self.command_history.is_empty() {
            return;
        }

        if self.history_index.is_none() {
            self.pending_input = Some(self.current_input.clone());
            self.pending_cursor_position = self.cursor_position;
        }

        let new_index = match self.history_index {
            Some(idx) if idx > 0 => idx - 1,
            Some(_) => return,
            None => self.command_history.len() - 1,
        };
        self.history_index = Some(new_index);
        self.current_input = self.command_history[new_index].clone();
        self.cursor_position = self.current_input.chars().count();
        self.update_completions();
    }

    /// Handles down the arrow key press for command history navigation.
    fn handle_down_key(&mut self) {
        let new_index = match self.history_index {
            Some(idx) if idx < self.command_history.len() - 1 => idx + 1,
            Some(_) => {
                self.history_index = None;
                if let Some(pending) = self.pending_input.take() {
                    self.current_input = pending;
                    self.cursor_position = self.pending_cursor_position;
                } else {
                    self.current_input.clear();
                    self.cursor_position = 0;
                }
                self.update_completions();
                return;
            }
            None => return,
        };
        self.history_index = Some(new_index);
        self.current_input = self.command_history[new_index].clone();
        self.cursor_position = self.current_input.chars().count();
        self.update_completions();
    }

    /// Handles the enter key press event, executing commands and managing input history
    pub async fn handle_enter_key(
        &mut self,
        input_prefix: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if !self.current_input.trim().is_empty() {
            self.command_history.push(self.current_input.clone());
            self.current_completions.clear();
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
            self.current_completions.clear();
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
        self.update_completions();
    }

    /// Handles Tab key press to apply the selected completion.
    ///
    /// If a completion is selected (via Left/Right arrows), uses that completion.
    /// Otherwise, uses the best match from the completion tree.
    fn handle_tab_key(&mut self) {
        if !self.current_completions.is_empty()
            && self.selected_completion_index < self.current_completions.len()
        {
            self.current_input = self.current_completions[self.selected_completion_index]
                .full_text
                .clone();
            self.cursor_position = self.current_input.chars().count();
            self.update_completions();
        } else if let Some(tree) = &mut self.tab_tree
            && let Some(completion) = tree.get_best_match(&self.current_input)
        {
            self.current_input = completion;
            self.cursor_position = self.current_input.chars().count();
            self.update_completions();
        }
    }

    /// Updates completion candidates based on current input.
    ///
    /// Resets the selected completion index to 0 when candidates change.
    fn update_completions(&mut self) {
        if let Some(tree) = &mut self.tab_tree {
            self.current_completions = tree.get_candidates(&self.current_input);
            self.selected_completion_index = 0;
        }
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

    /// Handles logger actions with event dispatch toggling
    fn handle_logger_action(
        &mut self,
        level: LogLevel,
        message: String,
        module_name: Option<String>,
        dispatch_event: Option<bool>,
    ) {
        self.switch_if_dispatch_event();
        let module_str = module_name.as_deref();
        self.logger(level, &message, module_str, dispatch_event);
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
        self.logger(LogLevel::Info, message, Some("Stream"), None);
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
        self.logger(LogLevel::Debug, message, Some("Stream"), None);
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
        self.logger(LogLevel::Warn, message, Some("Stream"), None);
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
        self.logger(LogLevel::Error, message, Some("Stream"), None);
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
        self.logger(LogLevel::Critical, message, Some("Stream"), None);
    }

    /// Unified logger method that allows specifying a custom module name for the log message.
    ///
    /// # Arguments
    ///
    /// * `level` - The log level (Info, Warn, Error, Debug, Critical)
    /// * `message` - The message content to be logged
    /// * `module_name` - The name of the module to associate with the log message (optional)
    /// * `dispatch_event` - Whether to dispatch log events (optional, defaults to true)
    ///
    /// # Examples
    ///
    /// ```
    /// use daemon_console::{TerminalApp, logger::LogLevel};
    ///
    /// fn example() {
    ///     let mut app = TerminalApp::new();
    ///     app.logger(LogLevel::Info, "Application started", Some("Main"), None);
    ///     app.logger(LogLevel::Error, "Database connection failed", None, Some(true));
    /// }
    /// ```
    pub fn logger(
        &mut self,
        level: LogLevel,
        message: &str,
        module_name: Option<&str>,
        dp_evt: Option<bool>,
    ) {
        if self.is_shadow {
            if let Some(ref sender) = self.action_sender {
                let _ = sender.send(AppAction::Logger(
                    level,
                    message.to_string(),
                    module_name.map(|s| s.to_string()),
                    dp_evt,
                ));
            }
            return;
        }

        let formatted_message = match level {
            LogLevel::Info => {
                if let Some(module) = module_name {
                    get_info!(message, module)
                } else {
                    get_info!(message)
                }
            }
            LogLevel::Warn => {
                if let Some(module) = module_name {
                    get_warn!(message, module)
                } else {
                    get_warn!(message)
                }
            }
            LogLevel::Error => {
                if let Some(module) = module_name {
                    get_error!(message, module)
                } else {
                    get_error!(message)
                }
            }
            LogLevel::Debug => {
                if let Some(module) = module_name {
                    get_debug!(message, module)
                } else {
                    get_debug!(message)
                }
            }
            LogLevel::Critical => {
                if let Some(module) = module_name {
                    get_critical!(message, module)
                } else {
                    get_critical!(message)
                }
            }
        };
        self.print_log_entry(&formatted_message);
        let should_dispatch = dp_evt.unwrap_or(true);
        if should_dispatch {
            self.dispatch_log_events(message, level);
        };
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
            let mut temp_app = TerminalApp::new();
            temp_app.is_shadow = true;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_app() -> TerminalApp {
        TerminalApp::new()
    }

    #[test]
    fn handle_char_input_inserts_at_cursor() {
        let mut app = make_app();
        app.handle_char_input('a');
        assert_eq!(app.current_input, "a");
        assert_eq!(app.cursor_position, 1);

        app.handle_char_input('b');
        assert_eq!(app.current_input, "ab");
        assert_eq!(app.cursor_position, 2);
    }

    #[test]
    fn handle_char_input_inserts_at_mid_cursor() {
        let mut app = make_app();
        app.current_input = "ac".into();
        app.cursor_position = 1;
        app.handle_char_input('b');
        assert_eq!(app.current_input, "abc");
        assert_eq!(app.cursor_position, 2);
    }

    #[test]
    fn remove_char_at_removes_correct_character() {
        let mut app = make_app();
        app.current_input = "abc".into();
        app.remove_char_at(1);
        assert_eq!(app.current_input, "ac");
    }

    #[test]
    fn remove_char_at_out_of_bounds_does_nothing() {
        let mut app = make_app();
        app.current_input = "a".into();
        app.remove_char_at(5);
        assert_eq!(app.current_input, "a");
    }

    #[test]
    fn handle_up_key_navigates_to_last_history() {
        let mut app = make_app();
        app.command_history = vec!["cmd1".into(), "cmd2".into(), "cmd3".into()];
        app.handle_up_key();
        assert_eq!(app.current_input, "cmd3");
        assert_eq!(app.cursor_position, 4);
        assert_eq!(app.history_index, Some(2));
    }

    #[test]
    fn handle_up_key_twice_navigates_back_two() {
        let mut app = make_app();
        app.command_history = vec!["cmd1".into(), "cmd2".into(), "cmd3".into()];
        app.handle_up_key();
        app.handle_up_key();
        assert_eq!(app.current_input, "cmd2");
        assert_eq!(app.history_index, Some(1));
    }

    #[test]
    fn handle_up_key_at_top_stops() {
        let mut app = make_app();
        app.command_history = vec!["cmd1".into(), "cmd2".into()];
        app.handle_up_key();
        app.handle_up_key();
        assert_eq!(app.current_input, "cmd1");
        app.handle_up_key();
        assert_eq!(app.current_input, "cmd1");
    }

    #[test]
    fn handle_up_key_empty_history_does_nothing() {
        let mut app = make_app();
        app.current_input = "test".into();
        app.handle_up_key();
        assert_eq!(app.current_input, "test");
        assert!(app.history_index.is_none());
    }

    #[test]
    fn handle_down_key_returns_forward() {
        let mut app = make_app();
        app.command_history = vec!["a".into(), "b".into(), "c".into()];
        app.history_index = Some(0);
        app.handle_down_key();
        assert_eq!(app.current_input, "b");
        assert_eq!(app.history_index, Some(1));
    }

    #[test]
    fn handle_down_key_at_end_clears_input() {
        let mut app = make_app();
        app.command_history = vec!["cmd".into()];
        app.history_index = Some(0);
        app.current_input = "cmd".into();
        app.handle_down_key();
        assert_eq!(app.current_input, "");
        assert_eq!(app.cursor_position, 0);
        assert!(app.history_index.is_none());
    }

    #[test]
    fn handle_down_key_no_history_does_nothing() {
        let mut app = make_app();
        app.current_input = "x".into();
        app.handle_down_key();
        assert_eq!(app.current_input, "x");
    }

    #[tokio::test]
    async fn handle_ctrl_c_clears_input_on_first_press() {
        let mut app = make_app();
        app.current_input = "some text".into();
        app.cursor_position = 5;
        let (quit, msg) = app.handle_ctrl_c().await.unwrap();
        assert!(!quit);
        assert!(app.current_input.is_empty());
        assert_eq!(app.cursor_position, 0);
        assert!(msg.contains("cleared"));
    }

    #[tokio::test]
    async fn handle_ctrl_c_second_press_exits() {
        let mut app = make_app();
        app.last_ctrl_c = Some(std::time::Instant::now());
        let (quit, msg) = app.handle_ctrl_c().await.unwrap();
        assert!(quit);
        assert!(msg.contains("Exiting"));
    }

    #[tokio::test]
    async fn handle_ctrl_c_empty_input_first_press_shows_hint() {
        let mut app = make_app();
        let (quit, msg) = app.handle_ctrl_c().await.unwrap();
        assert!(!quit);
        assert!(msg.contains("again"));
    }

    #[test]
    fn history_up_then_down_returns_to_empty() {
        let mut app = make_app();
        app.command_history = vec!["hello".into()];
        app.handle_up_key();
        assert_eq!(app.current_input, "hello");
        app.handle_down_key();
        assert_eq!(app.current_input, "");
        assert!(app.history_index.is_none());
    }
}
