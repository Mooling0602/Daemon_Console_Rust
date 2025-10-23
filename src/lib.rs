//! # daemon_console
//!
//! A flexible console for daemon applications providing a terminal interface
//! with command registration, history navigation, and colored logging.
//!
//! # Examples
//!
//! ```rust
//! use daemon_console::TerminalApp;
//!
//! let mut app = TerminalApp::new();
//! // Register commands and run
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod logger;

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode},
};
use std::collections::HashMap;
use std::io::{Stdout, Write, stdout};
use std::time::Instant;
use unicode_width::UnicodeWidthChar;

/// Type alias for custom unknown command handlers.
type UnknownCommandHandler = Box<dyn Fn(&str) -> String + Send + Sync + 'static>;

/// Trait for command handlers that can be registered with the terminal application.
///
/// All commands must implement this trait to be executable within the terminal app.
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
/// - Custom command registration
/// - Configurable unknown command handling
pub struct TerminalApp {
    pub command_history: Vec<String>,
    pub current_input: String,
    pub history_index: Option<usize>,
    pub last_ctrl_c: Option<Instant>,
    pub cursor_position: usize,
    commands: HashMap<String, Box<dyn CommandHandler>>,
    unknown_command_handler: Option<UnknownCommandHandler>,
}

impl Default for TerminalApp {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalApp {
    /// Creates a new terminal application instance with default settings.
    pub fn new() -> Self {
        Self {
            command_history: Vec::new(),
            current_input: String::new(),
            history_index: None,
            last_ctrl_c: None,
            cursor_position: 0,
            commands: HashMap::new(),
            unknown_command_handler: None,
        }
    }

    /// Registers a command handler with the application.
    ///
    /// # Arguments
    ///
    /// * `name` - Command name that users will type
    /// * `handler` - Boxed command handler implementing `CommandHandler`
    pub fn register_command<S: Into<String>>(&mut self, name: S, handler: Box<dyn CommandHandler>) {
        self.commands.insert(name.into(), handler);
    }

    /// Sets a custom handler for unknown commands.
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

    /// Removes the custom unknown command handler.
    pub fn clear_unknown_command_handler(&mut self) {
        self.unknown_command_handler = None;
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
    pub fn init_terminal(
        &mut self,
        stdout: &mut Stdout,
        startup_message: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        enable_raw_mode()?;
        writeln!(stdout, "{}", startup_message)?;
        self._render_input_line(stdout)?;
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
    pub fn process_event(
        &mut self,
        event: Event,
        stdout: &mut Stdout,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let mut should_quit = false;
        if let Event::Key(KeyEvent {
            code, modifiers, ..
        }) = event
        {
            match code {
                KeyCode::Char('d') if modifiers == KeyModifiers::CONTROL => {
                    should_quit = self.handle_ctrl_d()?;
                }
                KeyCode::Char('c') if modifiers == KeyModifiers::CONTROL => {
                    let (quit, message) = self.handle_ctrl_c()?;
                    should_quit = quit;
                    self.print_log_entry(stdout, &message)?;
                }
                KeyCode::Up => {
                    self.handle_up_key();
                    self._render_input_line(stdout)?;
                }
                KeyCode::Down => {
                    self.handle_down_key();
                    self._render_input_line(stdout)?;
                }
                KeyCode::Left => {
                    if self.cursor_position > 0 {
                        self.cursor_position -= 1;
                        self._render_input_line(stdout)?;
                    }
                }
                KeyCode::Right => {
                    if self.cursor_position < self.current_input.chars().count() {
                        self.cursor_position += 1;
                        self._render_input_line(stdout)?;
                    }
                }
                KeyCode::Enter => {
                    self.handle_enter_key(stdout, "> ")?;
                }
                KeyCode::Char(c) => {
                    self.handle_char_input(c);
                    self._render_input_line(stdout)?;
                }
                KeyCode::Backspace => {
                    if self.cursor_position > 0 {
                        self.current_input.remove(self.cursor_position - 1);
                        self.cursor_position -= 1;
                        self._render_input_line(stdout)?;
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
    pub fn shutdown_terminal(
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
    pub fn run(
        &mut self,
        startup_message: &str,
        exit_message: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut stdout = stdout();
        self.init_terminal(&mut stdout, startup_message)?;

        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            while let Ok(event) = event::read() {
                if self.process_event(event, &mut stdout)? {
                    break;
                }
            }
            Ok(())
        })();

        self.shutdown_terminal(&mut stdout, exit_message)?;
        result
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
    pub fn print_log_entry(
        &mut self,
        stdout: &mut Stdout,
        log_line: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        execute!(
            stdout,
            cursor::MoveToColumn(0),
            Clear(ClearType::CurrentLine)
        )?;
        writeln!(stdout, "{}", log_line)?;
        self._render_input_line(stdout)?;
        Ok(())
    }

    /// Renders the input line with prompt and cursor positioning.
    fn _render_input_line(&self, stdout: &mut Stdout) -> Result<(), Box<dyn std::error::Error>> {
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            execute!(stdout, cursor::Hide)?;
            execute!(
                stdout,
                cursor::MoveToColumn(0),
                Clear(ClearType::CurrentLine)
            )?;
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
    pub fn handle_ctrl_d(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(true)
    }

    /// Handles Ctrl+C key press with double-press confirmation.
    ///
    /// The first press clears input, the second press within 5 seconds exits.
    ///
    /// # Returns
    ///
    /// Tuple of (should_quit, message_to_display)
    pub fn handle_ctrl_c(&mut self) -> Result<(bool, String), Box<dyn std::error::Error>> {
        if !self.current_input.is_empty() {
            self.current_input.clear();
            self.cursor_position = 0;
            self.last_ctrl_c = Some(Instant::now());
            return Ok((false, info!("Input cleared. Press Ctrl+C again to exit.")));
        }
        if let Some(last_time) = self.last_ctrl_c
            && last_time.elapsed().as_secs() < 5
        {
            return Ok((true, warn!("Exiting application. Goodbye!")));
        }
        self.last_ctrl_c = Some(Instant::now());
        Ok((false, info!("Press Ctrl+C again to exit.")))
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

    /// Handles down arrow key press for command history navigation.
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
    pub fn handle_enter_key(
        &mut self,
        stdout: &mut Stdout,
        input_prefix: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !self.current_input.trim().is_empty() {
            self.command_history.push(self.current_input.clone());
            execute!(
                stdout,
                cursor::MoveToColumn(0),
                Clear(ClearType::CurrentLine)
            )?;
            writeln!(stdout, "{}{}", input_prefix, self.current_input)?;
            let input_copy = self.current_input.clone();
            let command_output = self.execute_command(&input_copy);
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
            self._render_input_line(stdout)?;
        } else {
            execute!(
                stdout,
                cursor::MoveToColumn(0),
                Clear(ClearType::CurrentLine)
            )?;
            self._render_input_line(stdout)?;
        }
        Ok(())
    }

    /// Executes a command by looking it up in the registered commands.
    ///
    /// If the command is not found, uses the custom unknown command handler
    /// or returns a default error message.
    ///
    /// # Arguments
    ///
    /// * `command` - Full command string including arguments
    ///
    /// # Returns
    ///
    /// String output from the command execution
    pub fn execute_command(&mut self, command: &str) -> String {
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return String::new();
        }

        let cmd_name = parts[0];
        let args = &parts[1..];

        if self.commands.contains_key(cmd_name) {
            let mut handler = self.commands.remove(cmd_name).unwrap();
            let result = handler.execute(self, args);
            self.commands.insert(cmd_name.to_string(), handler);
            result
        } else {
            if let Some(ref handler) = self.unknown_command_handler {
                handler(command)
            } else {
                warn!(&format!("Unknown command: '{}'", command))
            }
        }
    }

    /// Handles character input by inserting at cursor position.
    fn handle_char_input(&mut self, c: char) {
        self.current_input.insert(self.cursor_position, c);
        self.cursor_position += 1;
    }
}
