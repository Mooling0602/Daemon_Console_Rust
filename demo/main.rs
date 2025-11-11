use async_trait::async_trait;
use daemon_console::{AppAction, AsyncCommandHandler, TerminalApp, get_info};
use std::boxed::Box;

#[tokio::main]
async fn main() {
    let mut app = TerminalApp::new();
    register_commands(&mut app).await;
    let _ = app
        .run(&get_info!("App demo starting...", "Demo"), "")
        .await;
}

async fn register_commands(app: &mut TerminalApp) {
    app.register_async_command("register", Box::new(RegisterCommand {}));
}

/// 异步命令处理器用于注册新命令
#[derive(Clone)]
struct RegisterCommand;

#[async_trait]
impl AsyncCommandHandler for RegisterCommand {
    async fn execute_async(&mut self, app: &mut TerminalApp, args: &[&str]) -> String {
        if args.len() < 2 {
            return get_info!("Usage: register <command> <reply>", "CommandHelp");
        }

        let cmd = args[0];
        let reply = args[1..].join(" ");

        // 创建一个新的命令处理器
        let handler =
            Box::new(move |_app: &mut TerminalApp, _args: &[&str]| -> String { get_info!(&reply) });

        // 通过action_sender发送注册命令的请求
        if let Some(sender) = &app.action_sender {
            let _ = sender.send(AppAction::RegisterCommand(cmd.to_string(), handler));
            get_info!(
                &format!("Command '{}' registered successfully!", cmd),
                "CommandResp"
            )
        } else {
            get_info!(
                "Failed to register command: no action sender available",
                "CommandResp"
            )
        }
    }

    fn box_clone(&self) -> Box<dyn AsyncCommandHandler> {
        Box::new(self.clone())
    }
}
