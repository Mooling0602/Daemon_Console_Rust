use crate::logger::LogLevel;
use chrono::Local;

#[derive(Debug, Clone)]
pub enum DaemonConsoleEvent {
    UserConsoleInput {
        raw: String,    // raw input
        timestamp: i64, // raw time
    },
    TerminalLog {
        level: LogLevel,
        message: String,
        module_name: Option<String>,
        timestamp: i64,
    },
    SubprocessLog {
        pid: u32,
        message: String,
        timestamp: i64,
    },
}

impl DaemonConsoleEvent {
    pub fn now_ts() -> i64 {
        Local::now().timestamp_millis()
    }
}
