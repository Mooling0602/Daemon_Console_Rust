# Daemon Console

A flexible console for daemon applications providing a terminal interface with command registration, history navigation, and colored logging.

## Features

- Command history with up/down arrow navigation
- Colored logging with different severity levels (info, warn, error, debug, critical)
- Customizable unknown command handling
- Raw terminal mode for smooth user experience
- Support for both sync and async command handlers

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
daemon_console = "0.2.0"
```

Basic usage:

```rust
use daemon_console::TerminalApp;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = TerminalApp::new();
    
    // Register a simple command
    app.register_command(
        "hello",
        Box::new(|_app: &mut TerminalApp, args: &[&str]| -> String {
            if args.is_empty() {
                "Hello, World!".to_string()
            } else {
                format!("Hello, {}!", args.join(" "))
            }
        }),
    );
    
    // Run the application
    app.run("Terminal started. Press Ctrl+D to exit.", "Goodbye!")
}
```

## Async Commands

The library also supports async command handlers:

```rust
use daemon_console::TerminalApp;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = TerminalApp::new();
    
    // Register an async command
    app.register_async_command(
        "async_hello",
        Box::new(AsyncHelloCommand {}),
    );
    
    app.run("Terminal started. Press Ctrl+D to exit.", "Goodbye!")
}

struct AsyncHelloCommand;

#[async_trait::async_trait]
impl daemon_console::AsyncCommandHandler for AsyncHelloCommand {
    async fn execute(&mut self, _app: &mut TerminalApp, args: &[&str]) -> String {
        // Simulate async work
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        
        if args.is_empty() {
            "Hello, World!".to_string()
        } else {
            format!("Hello, {}!", args.join(" "))
        }
    }
}
```

## License

This project is licensed under GPL-3.0.