use daemon_console::{AppAction, TerminalApp, events::DaemonConsoleEvent, get_info, get_warn};

#[tokio::main]
async fn main() {
    let mut app = TerminalApp::new();

    app.set_unknown_command_handler(|_| {
        get_warn!("The command system disabled for developing the event system.")
    });

    // 获取 app 内部创建的 action sender 而不是自己创建
    let action_tx = app
        .get_action_sender()
        .expect("Failed to get action sender");
    let mut event_rx = app
        .subscribe_events()
        .expect("Failed to subscribe to events");

    // 启动事件监听任务
    tokio::spawn(async move {
        while let Ok(event) = event_rx.recv().await {
            match event {
                DaemonConsoleEvent::CommandPromptInput { raw, timestamp } => {
                    let _ = action_tx.send(AppAction::Info(format!(
                        "[Event] CommandPromptInput: '{}' at {}",
                        raw, timestamp
                    )));
                }

                DaemonConsoleEvent::TerminalLog {
                    level,
                    message,
                    module_name,
                    timestamp,
                } => {
                    let _ = action_tx.send(AppAction::Debug(format!(
                        "[Event] TerminalLog: [{:?}] {} (module: {:?}, ts: {})",
                        level, message, module_name, timestamp
                    )));
                }

                DaemonConsoleEvent::SubprocessLog {
                    pid,
                    message,
                    timestamp,
                } => {
                    let _ = action_tx.send(AppAction::Warn(format!(
                        "[Event] SubprocessLog: [PID:{}] {} at {}",
                        pid, message, timestamp
                    )));
                }
            }
        }
    });

    // 直接调用 run，让它管理自己的 action channel
    let _ = app
        .run(&get_info!("App demo starting...", "Demo"), "")
        .await;
}
