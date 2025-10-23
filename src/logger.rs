//! Logging utilities with colored terminal output.
//!
//! This module provides a logging system with different severity levels
//! (Info, Warn, Error, Debug) and automatic timestamp formatting.

use chrono::Local;
use crossterm::style::{self, Color, ResetColor, SetForegroundColor};

/// Log level enumeration for categorizing log messages.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
    Debug,
    Critical,
}

/// Formats a log message with timestamp, level indicator, and color coding.
///
/// # Arguments
///
/// * `level` - Severity level of the log message
/// * `message` - Content of the log message
/// * `module_name` - Optional module name prefix
///
/// # Returns
///
/// Formatted string with ANSI color codes for terminal display
///
/// # Examples
///
/// ```
/// use daemon_console::logger::{log_message, LogLevel};
///
/// let msg = log_message(LogLevel::Info, "Application started", Some("main"));
/// println!("{}", msg);
/// ```
pub fn log_message(level: LogLevel, message: &str, module_name: Option<&str>) -> String {
    let now = Local::now();
    let timestamp = now.format("%H:%M:%S").to_string();

    let (level_str, color) = match level {
        LogLevel::Info => ("INFO", Color::Green),
        LogLevel::Warn => ("WARN", Color::Yellow),
        LogLevel::Error => ("ERROR", Color::Red),
        LogLevel::Debug => ("DEBUG", Color::DarkGrey),
        LogLevel::Critical => ("CRITICAL", Color::AnsiValue(5)),
    };

    let module_prefix = module_name.map_or_else(String::new, |name| format!("{}/", name));

    match level {
        LogLevel::Info | LogLevel::Warn | LogLevel::Error | LogLevel::Critical => {
            format!(
                "[{}] {}[{}{}{}{}{}]{} {}{}",
                timestamp,
                style::Attribute::Bold,
                module_prefix,
                SetForegroundColor(color),
                level_str,
                ResetColor,
                style::Attribute::Bold,
                ResetColor,
                message,
                ResetColor
            )
        }
        LogLevel::Debug => {
            format!(
                "{}{}[{}] [{}{}]{} {}{}{}",
                SetForegroundColor(color),
                style::Attribute::Italic,
                timestamp,
                module_prefix,
                level_str,
                ResetColor,
                style::Attribute::Italic,
                message,
                ResetColor,
            )
        }
    }
}

/// Macro for creating info-level log messages.
///
/// # Examples
///
/// ```
/// use daemon_console::info;
///
/// let msg = info!("Server started");
/// let msg_with_module = info!("Database connected", "db");
/// ```
#[macro_export]
macro_rules! info {
    ($message:expr) => {
        $crate::logger::log_message($crate::logger::LogLevel::Info, $message, None)
    };
    ($message:expr, $module_name:expr) => {
        $crate::logger::log_message($crate::logger::LogLevel::Info, $message, Some($module_name))
    };
}

/// Macro for creating warning-level log messages.
///
/// # Examples
///
/// ```
/// use daemon_console::warn;
///
/// let msg = warn!("Memory usage high");
/// let msg_with_module = warn!("Connection timeout", "network");
/// ```
#[macro_export]
macro_rules! warn {
    ($message:expr) => {
        $crate::logger::log_message($crate::logger::LogLevel::Warn, $message, None)
    };
    ($message:expr, $module_name:expr) => {
        $crate::logger::log_message($crate::logger::LogLevel::Warn, $message, Some($module_name))
    };
}

/// Macro for creating error-level log messages.
///
/// # Examples
///
/// ```
/// use daemon_console::error;
///
/// let msg = error!("Failed to connect");
/// let msg_with_module = error!("Authentication failed", "auth");
/// ```
#[macro_export]
macro_rules! error {
    ($message:expr) => {
        $crate::logger::log_message($crate::logger::LogLevel::Error, $message, None)
    };
    ($message:expr, $module_name:expr) => {
        $crate::logger::log_message(
            $crate::logger::LogLevel::Error,
            $message,
            Some($module_name),
        )
    };
}

/// Macro for creating debug-level log messages.
///
/// # Examples
///
/// ```
/// use daemon_console::debug;
///
/// let msg = debug!("Variable value: 42");
/// let msg_with_module = debug!("Request received", "http");
/// ```
#[macro_export]
macro_rules! debug {
    ($message:expr) => {
        $crate::logger::log_message($crate::logger::LogLevel::Debug, $message, None)
    };
    ($message:expr, $module_name:expr) => {
        $crate::logger::log_message(
            $crate::logger::LogLevel::Debug,
            $message,
            Some($module_name),
        )
    };
}

/// Macro for creating critical-level log messages.
///
/// # Examples
///
/// ```
/// use daemon_console::critical;
///
/// let msg = critical!("Critical error");
/// let msg_with_module = critical!("Critical error", "database");
/// ```
#[macro_export]
macro_rules! critical {
    ($message:expr) => {
        $crate::logger::log_message($crate::logger::LogLevel::Critical, $message, None)
    };
    ($message:expr, $module_name:expr) => {
        $crate::logger::log_message(
            $crate::logger::LogLevel::Critical,
            $message,
        )
    }
}