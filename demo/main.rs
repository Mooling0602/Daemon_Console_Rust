use daemon_console::{
    AppAction, TerminalApp, events::DaemonConsoleEvent, get_info, get_warn, logger::LogLevel,
    utils::get_local_timestring,
};

fn handle_user_input_event(
    raw: &str,
    timestamp: i64,
    action_tx: &tokio::sync::mpsc::UnboundedSender<AppAction>,
) {
    if raw.trim() == "test" {
        let _ = action_tx.send(AppAction::Info("ok".to_string()));
    } else if raw.trim() == "hello" {
        let _ = action_tx.send(AppAction::Info("Hello there!".to_string()));
    } else if raw.trim().starts_with("echo ") {
        let echo_content = raw.trim().strip_prefix("echo ").unwrap_or("");
        let _ = action_tx.send(AppAction::Info(format!("Echo: {}", echo_content)));
    }

    let _ = action_tx.send(AppAction::Info(format!(
        "event 'CommandPromptInput': raw='{}', local_time={}",
        raw,
        get_local_timestring(timestamp)
    )));
}

fn handle_terminal_log_event(
    level: LogLevel,
    message: &str,
    module_name: &Option<String>,
    timestamp: i64,
    action_tx: &tokio::sync::mpsc::UnboundedSender<AppAction>,
) {
    let _ = action_tx.send(AppAction::Debug(format!(
        "event 'TerminalLog': level={:?}, message='{}', module_name='{:?}', timestamp={}",
        level, message, module_name, timestamp
    )));
}

fn handle_subprocess_log_event(
    pid: u32,
    message: &str,
    timestamp: i64,
    action_tx: &tokio::sync::mpsc::UnboundedSender<AppAction>,
) {
    let _ = action_tx.send(AppAction::Warn(format!(
        "event 'SubprocessLog': pid={}, message='{}', timestamp={}",
        pid, message, timestamp
    )));
}

fn start_event_listener(
    mut event_rx: tokio::sync::broadcast::Receiver<DaemonConsoleEvent>,
    action_tx: tokio::sync::mpsc::UnboundedSender<AppAction>,
) {
    tokio::spawn(async move {
        while let Ok(event) = event_rx.recv().await {
            match event {
                DaemonConsoleEvent::UserConsoleInput { raw, timestamp } => {
                    handle_user_input_event(&raw, timestamp, &action_tx);
                }
                DaemonConsoleEvent::TerminalLog {
                    level,
                    message,
                    module_name,
                    timestamp,
                } => {
                    handle_terminal_log_event(level, &message, &module_name, timestamp, &action_tx);
                }
                DaemonConsoleEvent::SubprocessLog {
                    pid,
                    message,
                    timestamp,
                } => {
                    handle_subprocess_log_event(pid, &message, timestamp, &action_tx);
                }
            }
        }
    });
}

#[tokio::main]
async fn main() {
    let mut app = TerminalApp::new();

    app.set_unknown_command_handler(|_| {
        get_warn!("The command system disabled for developing the event system.")
    });

    let action_tx = app
        .get_action_sender()
        .expect("Failed to get action sender");
    let event_rx = app
        .subscribe_events()
        .expect("Failed to subscribe to events");

    start_event_listener(event_rx, action_tx);

    app.info("This message used for debugging 'TerminalLog' event.");

    let _ = app
        .run(&get_info!("App demo starting...", "Demo"), "")
        .await;
}
