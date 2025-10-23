pub mod logger; // 声明 logger 模块为公共的，以便在库内和库外使用

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType},
};
use std::io::{stdout, Stdout, Write};
use std::time::Instant;
use unicode_width::UnicodeWidthChar;
use std::collections::HashMap;


// 定义一个 trait，所有可注册命令都必须实现它。
// CommandHandler 负责接收应用状态和命令参数，并返回执行结果字符串。
pub trait CommandHandler: Send + Sync + 'static {
    // execute 方法需要可变引用到 TerminalApp，以便命令可以修改应用状态
    fn execute(&mut self, app: &mut TerminalApp, args: &[&str]) -> String;
}

// 为简单的闭包提供一个实现 CommandHandler 的便捷方式
impl<F> CommandHandler for F
where
    // F 必须是一个 FnMut 闭包，允许修改捕获的状态
    F: FnMut(&mut TerminalApp, &[&str]) -> String + Send + Sync + 'static,
{
    // 实现 execute 方法
    fn execute(&mut self, app: &mut TerminalApp, args: &[&str]) -> String {
        self(app, args)
    }
}

// 将 TerminalApp 结构体公开
pub struct TerminalApp {
    pub command_history: Vec<String>,
    pub current_input: String,
    pub history_index: Option<usize>,
    pub last_ctrl_c: Option<Instant>,
    pub cursor_position: usize,
    // 添加一个字段来存储注册的命令
    commands: HashMap<String, Box<dyn CommandHandler>>,
    // 添加一个字段来存储未知命令处理器
    unknown_command_handler: Option<Box<dyn Fn(&str) -> String + Send + Sync + 'static>>,
}

impl TerminalApp {
    pub fn new() -> Self {
        Self {
            command_history: Vec::new(),
            current_input: String::new(),
            history_index: None,
            last_ctrl_c: None,
            cursor_position: 0,
            commands: HashMap::new(), // 初始化命令 Map
            unknown_command_handler: None, // 默认没有自定义处理器
        }
    }

    // 新增一个方法用于注册命令
    pub fn register_command<S: Into<String>>(&mut self, name: S, handler: Box<dyn CommandHandler>) {
        self.commands.insert(name.into(), handler);
    }

    // 新增方法用于设置未知命令处理器
    pub fn set_unknown_command_handler<F>(&mut self, handler: F)
    where
        F: Fn(&str) -> String + Send + Sync + 'static,
    {
        self.unknown_command_handler = Some(Box::new(handler));
    }

    // 清除未知命令处理器
    pub fn clear_unknown_command_handler(&mut self) {
        self.unknown_command_handler = None;
    }

    // 拆分 run 函数，提供更细粒度的控制
    pub fn init_terminal(&mut self, stdout: &mut Stdout, startup_message: &str) -> Result<(), Box<dyn std::error::Error>> {
        enable_raw_mode()?;
        writeln!(stdout, "{}", startup_message)?;
        self._render_input_line(stdout)?;
        Ok(())
    }

    // 处理单个事件，并返回是否应该退出应用
    pub fn process_event(&mut self, event: Event, stdout: &mut Stdout) -> Result<bool, Box<dyn std::error::Error>> {
        let mut should_quit = false;
        match event {
            Event::Key(KeyEvent { code, modifiers, .. }) => {
                match code {
                    KeyCode::Char('d') if modifiers == KeyModifiers::CONTROL => {
                        should_quit = self.handle_ctrl_d()?;
                    }
                    KeyCode::Char('c') if modifiers == KeyModifiers::CONTROL => {
                        let (quit, message) = self.handle_ctrl_c()?;
                        should_quit = quit;
                        // 使用 print_log_entry 方法来正确显示消息并重新渲染输入行
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
            // 可以处理其他事件类型，例如鼠标事件等
            _ => {}
        }
        Ok(should_quit)
    }

    pub fn shutdown_terminal(&mut self, stdout: &mut Stdout, exit_message: &str) -> Result<(), Box<dyn std::error::Error>> {
        disable_raw_mode()?;
        writeln!(stdout, "{}", exit_message)?;
        stdout.flush()?;
        Ok(())
    }

    // 重新定义 run 方法，使其使用新的细粒度方法
    // 这个 run 方法是作为一个便捷的入口，它封装了 init_terminal, process_event 循环和 shutdown_terminal
    pub fn run(&mut self, startup_message: &str, exit_message: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut stdout = stdout();
        self.init_terminal(&mut stdout, startup_message)?;

        let result = (|| {
            loop {
                // Event::read() 会阻塞直到接收到一个事件
                // 注意：这里读取的是任意 Event，如果只需要 Key 事件，可以进行过滤
                if let Ok(event) = event::read() {
                    if self.process_event(event, &mut stdout)? {
                        break; // TerminalApp 信号退出
                    }
                } else {
                    // 处理 event::read() 的错误，例如终端被关闭
                    // 为了简单起见，这里直接退出循环，实际应用中可能需要更精细的处理
                    break;
                }
            }
            Ok(())
        })();

        self.shutdown_terminal(&mut stdout, exit_message)?;
        result
    }

    // _print_log_entry 方法保持不变，因为它已经接收 stdout 引用，足够灵活
    pub fn print_log_entry(&mut self, stdout: &mut Stdout, log_line: &str) -> Result<(), Box<dyn std::error::Error>> {
        execute!(stdout, cursor::MoveToColumn(0), Clear(ClearType::CurrentLine))?;
        writeln!(stdout, "{}", log_line)?;
        self._render_input_line(stdout)?;
        Ok(())
    }

    fn _render_input_line(&self, stdout: &mut Stdout) -> Result<(), Box<dyn std::error::Error>> {
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            execute!(stdout, cursor::Hide)?;
            execute!(stdout, cursor::MoveToColumn(0), Clear(ClearType::CurrentLine))?;
            execute!(
                stdout,
                crossterm::style::Print("> "),
                crossterm::style::Print(&self.current_input)
            )?;
            let visual_cursor_pos = 2 + self.current_input.chars()
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

    pub fn handle_ctrl_d(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        // 让调用方决定日志消息
        Ok(true)
    }

    pub fn handle_ctrl_c(&mut self) -> Result<(bool, String), Box<dyn std::error::Error>> {
        if !self.current_input.is_empty() {
            self.current_input.clear();
            self.cursor_position = 0;
            self.last_ctrl_c = Some(Instant::now());
            return Ok((false, info!("Input cleared. Press Ctrl+C again to exit.")));
        }
        if let Some(last_time) = self.last_ctrl_c {
            if last_time.elapsed().as_secs() < 5 {
                return Ok((true, warn!("Exiting application. Goodbye!")));
            }
        }
        self.last_ctrl_c = Some(Instant::now());
        Ok((false, info!("Press Ctrl+C again to exit.")))
    }

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

    pub fn handle_enter_key(&mut self, stdout: &mut Stdout, input_prefix: &str) -> Result<(), Box<dyn std::error::Error>> {
        if !self.current_input.trim().is_empty() {
            self.command_history.push(self.current_input.clone());
            execute!(stdout, cursor::MoveToColumn(0), Clear(ClearType::CurrentLine))?;
            writeln!(stdout, "{}{}", input_prefix, self.current_input)?;
            let input_copy = self.current_input.clone();
            let command_output = self.execute_command(&input_copy);
            if !command_output.is_empty() {
                for line in command_output.lines() {
                    execute!(stdout, cursor::MoveToColumn(0))?;
                    writeln!(stdout, "{}", line.trim_start())?;
                }
            } else {
                writeln!(stdout, "")?;
            }
            self.current_input.clear();
            self.cursor_position = 0;
            self.history_index = None;
            self._render_input_line(stdout)?;
        } else {
            execute!(stdout, cursor::MoveToColumn(0), Clear(ClearType::CurrentLine))?;
            self._render_input_line(stdout)?;
        }
        Ok(())
    }

    // 修改 execute_command 以使用注册的命令
    pub fn execute_command(&mut self, command: &str) -> String {
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return String::new();
        }

        let cmd_name = parts[0];
        let args = &parts[1..];

        // Check if the command exists first
        if self.commands.contains_key(cmd_name) {
            // Get the handler, clone it if necessary to avoid double mutable borrow
            let mut handler = self.commands.remove(cmd_name).unwrap();
            let result = handler.execute(self, args);
            // Put the handler back
            self.commands.insert(cmd_name.to_string(), handler);
            result
        } else {
            // 使用自定义的未知命令处理器，如果没有则使用默认处理
            if let Some(ref handler) = self.unknown_command_handler {
                handler(command)
            } else {
                // 默认的未知命令处理
                warn!(&format!("未知命令: {}", command))
            }
        }
    }

    fn handle_char_input(&mut self, c: char) {
        self.current_input.insert(self.cursor_position, c);
        self.cursor_position += 1;
    }
}