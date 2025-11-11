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
daemon_console = "0.3.0"
```

Then read the [docs](https://docs.rs/daemon_console).

If you have any questions, ask in issues, I'll glad to reply you.

## Contributing
This project have two branches: `main` and `dev`.
> After v0.3.1

Documentations will be edited in `main` branch, and code changes will be made in `dev` branch.

Once the code in `dev` branch is stable, it will be merged into `main` branch, then released as a new version.
> Thanks to project like [cargo-dist](https://github.com/axodotdev/cargo-dist) and [cargo-binstall](https://github.com/cargo-bins/cargo-binstall), using them helps distribute this project.

## License

This project is licensed under GPL-3.0.
