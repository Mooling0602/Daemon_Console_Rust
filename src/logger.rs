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
                "{}{}[{}] [{}{}] {}{}{}",
                SetForegroundColor(color),
                style::Attribute::Italic,
                timestamp,
                module_prefix,
                level_str,
                style::Attribute::Italic,
                message,
                ResetColor,
            )
        }
    }
}

/// Format multi-line messages with log-levels.
///
/// > Middleware method for macros like `get_info!`.
pub fn format_multiline_message(
    level: LogLevel,
    message: &str,
    module_name: Option<&str>,
) -> String {
    if !message.contains('\n') {
        return log_message(level, message, module_name);
    }

    message
        .lines()
        .map(|line| log_message(level, line, module_name))
        .collect::<Vec<String>>()
        .join("\n")
}

/// Macro for creating info-level log messages.
///
/// # Examples
///
/// ```
/// use daemon_console::get_info;
///
/// let msg = get_info!("Server started");
/// let msg_with_module = get_info!("Database connected", "db");
/// ```
#[macro_export]
macro_rules! get_info {
    ($message:expr) => {
        $crate::logger::format_multiline_message($crate::logger::LogLevel::Info, $message, None)
    };
    ($message:expr, $module_name:expr) => {
        $crate::logger::format_multiline_message(
            $crate::logger::LogLevel::Info,
            $message,
            Some($module_name),
        )
    };
}

/// Macro for creating warning-level log messages.
///
/// # Examples
///
/// ```
/// use daemon_console::get_warn;
///
/// let msg = get_warn!("Memory usage high");
/// let msg_with_module = get_warn!("Connection timeout", "network");
/// ```
#[macro_export]
macro_rules! get_warn {
    ($message:expr) => {
        $crate::logger::format_multiline_message($crate::logger::LogLevel::Warn, $message, None)
    };
    ($message:expr, $module_name:expr) => {
        $crate::logger::format_multiline_message(
            $crate::logger::LogLevel::Warn,
            $message,
            Some($module_name),
        )
    };
}

/// Macro for creating error-level log messages.
///
/// # Examples
///
/// ```
/// use daemon_console::get_error;
///
/// let msg = get_error!("Failed to connect");
/// let msg_with_module = get_error!("Authentication failed", "auth");
/// ```
#[macro_export]
macro_rules! get_error {
    ($message:expr) => {
        $crate::logger::format_multiline_message($crate::logger::LogLevel::Error, $message, None)
    };
    ($message:expr, $module_name:expr) => {
        $crate::logger::format_multiline_message(
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
/// use daemon_console::get_debug;
///
/// let msg = get_debug!("Variable value: 42");
/// let msg_with_module = get_debug!("Request received", "http");
/// ```
#[macro_export]
macro_rules! get_debug {
    ($message:expr) => {
        $crate::logger::format_multiline_message($crate::logger::LogLevel::Debug, $message, None)
    };
    ($message:expr, $module_name:expr) => {
        $crate::logger::format_multiline_message(
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
/// use daemon_console::get_critical;
///
/// let msg = get_critical!("Critical error");
/// let msg_with_module = get_critical!("Critical error", "database");
/// ```
#[macro_export]
macro_rules! get_critical {
    ($message:expr) => {
        $crate::logger::format_multiline_message($crate::logger::LogLevel::Critical, $message, None)
    };
    ($message:expr, $module_name:expr) => {
        $crate::logger::format_multiline_message(
            $crate::logger::LogLevel::Critical,
            $message,
            Some($module_name),
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_message_info_contains_level_and_message() {
        let msg = log_message(LogLevel::Info, "server started", None);
        assert!(msg.contains("INFO"), "should contain level name");
        assert!(msg.contains("server started"), "should contain message");
    }

    #[test]
    fn log_message_warn_contains_level_and_message() {
        let msg = log_message(LogLevel::Warn, "low memory", None);
        assert!(msg.contains("WARN"), "should contain level name");
        assert!(msg.contains("low memory"), "should contain message");
    }

    #[test]
    fn log_message_error_contains_level_and_message() {
        let msg = log_message(LogLevel::Error, "connection failed", None);
        assert!(msg.contains("ERROR"), "should contain level name");
        assert!(msg.contains("connection failed"), "should contain message");
    }

    #[test]
    fn log_message_debug_contains_level_and_message() {
        let msg = log_message(LogLevel::Debug, "trace data", None);
        assert!(msg.contains("DEBUG"), "should contain level name");
        assert!(msg.contains("trace data"), "should contain message");
    }

    #[test]
    fn log_message_critical_contains_level_and_message() {
        let msg = log_message(LogLevel::Critical, "system halt", None);
        assert!(msg.contains("CRITICAL"), "should contain level name");
        assert!(msg.contains("system halt"), "should contain message");
    }

    #[test]
    fn log_message_includes_module_name_when_provided() {
        let msg = log_message(LogLevel::Info, "test", Some("my_module"));
        assert!(msg.contains("my_module"), "should contain module name");
    }

    #[test]
    fn log_message_no_module_omits_slash_prefix() {
        let msg = log_message(LogLevel::Info, "test", None);
        assert!(!msg.contains("[]"), "empty module should not insert /");
        assert!(msg.contains("INFO"), "should still contain level");
    }

    #[test]
    fn log_message_has_timestamp_format() {
        let msg = log_message(LogLevel::Info, "test", None);
        let prefix: String = msg.chars().take(10).collect();
        assert!(
            prefix.starts_with('[') && prefix.contains(':') && prefix.contains(']'),
            "should contain [HH:MM:SS] timestamp, got: {}",
            prefix
        );
    }

    #[test]
    fn format_multiline_single_line_same_as_log_message() {
        let single = format_multiline_message(LogLevel::Info, "one line", None);
        let direct = log_message(LogLevel::Info, "one line", None);
        assert_eq!(single, direct);
    }

    #[test]
    fn format_multiline_splits_on_newlines() {
        let result = format_multiline_message(LogLevel::Info, "line1\nline2\nline3", None);
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 3, "should produce 3 formatted lines");
        assert!(lines[0].contains("line1"));
        assert!(lines[1].contains("line2"));
        assert!(lines[2].contains("line3"));
    }

    #[test]
    fn format_multiline_preserves_module_name() {
        let result = format_multiline_message(LogLevel::Warn, "a\nb", Some("mod"));
        assert!(result.contains("mod"), "each line should have module name");
        assert_eq!(result.lines().count(), 2);
    }
}
