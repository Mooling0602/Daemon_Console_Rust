use crossterm::style::{self, Color, ResetColor, SetForegroundColor};
use chrono::Local;

/// 定义日志级别
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
    Debug,
}

/// 格式化日志消息，包含时间戳、日志级别和颜色
/// 返回一个包含ANSI颜色码的字符串，供终端应用显示
pub fn log_message(level: LogLevel, message: &str, module_name: Option<&str>) -> String {
    let now = Local::now();
    let timestamp = now.format("%H:%M:%S").to_string();

    let (level_str, color) = match level {
        LogLevel::Info => ("INFO", Color::Green),
        LogLevel::Warn => ("WARN", Color::Yellow),
        LogLevel::Error => ("ERROR", Color::Red),
        LogLevel::Debug => ("DEBUG", Color::DarkGrey),
    };

    let module_prefix = module_name.map_or_else(String::new, |name| format!("{}/", name));

    match level {
        // 对于 Info, Warn, Error 级别，只对日志级别字符串应用颜色和粗体
        LogLevel::Info | LogLevel::Warn | LogLevel::Error => {
            format!(
                "[{}] {}[{}{}{}{}{}]{} {}{}", // 时间戳默认，级别字符串颜色粗体，消息默认
                timestamp,
                style::Attribute::Bold,             // 设置粗体
                module_prefix,                      // 模块名称
                SetForegroundColor(color),          // 设置前景颜色
                level_str,                          // 日志级别字符串
                ResetColor,                         // 重置样式
                style::Attribute::Bold,             //
                ResetColor,                         //
                message,                            // 实际消息
                ResetColor
            )
        }
        // 对于 Debug 级别，整条日志都应用颜色和粗体
        LogLevel::Debug => {
            format!(
                "{}{}[{}] [{}{}]{} {}{}{}",
                SetForegroundColor(color),          // 设置前景颜色
                style::Attribute::Italic,             // 设置斜体
                timestamp,                          // 时间戳
                module_prefix,
                level_str,                          // 日志级别字符串
                ResetColor,
                style::Attribute::Italic,
                message,                            // 实际消息
                ResetColor,                         // 重置所有颜色和属性
            )
        }
    }
}

// 便捷的日志函数
// pub fn info(message: &str, module_name: Option<&str>) -> String {
//     log_message(LogLevel::Info, message, module_name)
// }

#[macro_export]
macro_rules! info {
    ($message:expr) => {
        $crate::logger::log_message($crate::logger::LogLevel::Info, $message, None) // 移除分号
    };
    ($message:expr, $module_name:expr) => {
        $crate::logger::log_message($crate::logger::LogLevel::Info, $message, Some($module_name)) // 移除分号
    };
}

// pub fn warn(message: &str, module_name: Option<&str>) -> String {
//     log_message(LogLevel::Warn, message, module_name)
// }

#[macro_export]
macro_rules! warn {
    ($message:expr) => {
        $crate::logger::log_message($crate::logger::LogLevel::Warn, $message, None) // 移除分号
    };
    ($message:expr, $module_name:expr) => {
        $crate::logger::log_message($crate::logger::LogLevel::Warn, $message, Some($module_name)) // 移除分号
    };
}

// pub fn error(message: &str, module_name: Option<&str>) -> String {
//     log_message(LogLevel::Error, message, module_name)
// }

#[macro_export]
macro_rules! error {
    ($message:expr) => {
        $crate::logger::log_message($crate::logger::LogLevel::Error, $message, None) // 移除分号
    };
    ($message:expr, $module_name:expr) => {
        $crate::logger::log_message($crate::logger::LogLevel::Error, $message, Some($module_name)) // 移除分号
    };
}

// pub fn debug(message: &str, module_name: Option<&str>) -> String {
//     log_message(LogLevel::Debug, message, module_name)
// }

#[macro_export]
macro_rules! debug {
    ($message:expr) => {
        $crate::logger::log_message($crate::logger::LogLevel::Debug, $message, None) // 移除分号
    };
    ($message:expr, $module_name:expr) => {
        $crate::logger::log_message($crate::logger::LogLevel::Debug, $message, Some($module_name)) // 移除分号
    };
}