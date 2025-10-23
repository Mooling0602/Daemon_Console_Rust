use daemon_console::{TerminalApp, info, warn, error, debug};
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = TerminalApp::new();
    
    // 注册所有命令
    register_commands(&mut app);
    
    // 设置自定义的未知命令处理器
    app.set_unknown_command_handler(|command: &str| {
        // 可以自定义不同的处理逻辑
        if command.starts_with("sudo") {
            error!(&format!("权限不足: 无法执行 '{}'", command))
        } else if command.len() > 20 {
            warn!(&format!("命令过长: '{}'", command))
        } else {
            warn!(&format!("命令 '{}' 不存在。输入 'help' 查看可用命令。", command))
        }
    });
    
    // 运行应用
    let startup_message = info!("TerminalApp started. Press Ctrl+D or Ctrl+C twice to exit.", "Daemon");
    let exit_message = info!("TerminalApp exiting. Goodbye!");
    
    app.run(&startup_message, &exit_message)
}

fn register_commands(app: &mut TerminalApp) {
    // 注册 list 命令
    app.register_command("list", Box::new(|_app: &mut TerminalApp, args: &[&str]| -> String {
        if cfg!(target_os = "windows") {
            Command::new("cmd")
                .arg("/C")
                .arg("dir")
                .output()
                .map_or_else(
                    |e| error!(&format!("Error executing command: {}", e)),
                    |output| String::from_utf8_lossy(&output.stdout).trim_end().to_string(),
                )
        } else {
            Command::new("ls")
                .args(args) // 传递 ls 的参数
                .output()
                .map_or_else(
                    |e| error!(&format!("Error executing command: {}", e)),
                    |output| String::from_utf8_lossy(&output.stdout).trim_end().to_string(),
                )
        }
    }));
    
    // 注册 help 命令
    app.register_command("help", Box::new(|_: &mut TerminalApp, _: &[&str]| -> String {
        info!("可用命令: list, help, exit, debug, hello (使用 Ctrl+C 或 Ctrl+D 退出)")
    }));
    
    // 注册 exit 命令
    app.register_command("exit", Box::new(|_: &mut TerminalApp, _: &[&str]| -> String {
        warn!("使用 Ctrl+C 或 Ctrl+D 退出")
    }));
    
    // 注册 debug 命令
    app.register_command("debug", Box::new(|_: &mut TerminalApp, _: &[&str]| -> String {
        debug!("这是一条调试日志")
    }));
    
    // 可以在这里添加更多自定义命令
    app.register_command("hello", Box::new(|_: &mut TerminalApp, args: &[&str]| -> String {
        if args.is_empty() {
            info!("Hello, World!")
        } else {
            info!(&format!("Hello, {}!", args.join(" ")))
        }
    }));

    app.register_command("test", Box::new(|_: &mut TerminalApp, args: &[&str]| -> String {
        if !args.is_empty() {
            error!("此命令不接受任何参数！")
        } else {
            info!("测试成功")
        }
    }))
}