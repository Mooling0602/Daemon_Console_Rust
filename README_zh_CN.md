# Daemon Console

[English](README.md) | [简体中文](README_zh_CN.md)

一个灵活的守护进程（daemon）应用控制台，提供带命令注册、历史导航与彩色日志的终端界面。

## 特性

- 命令历史记录，支持上下箭头导航
- 彩色日志，区分不同严重级别（info、warn、error、debug、critical）
- 可自定义的未知命令处理
- Raw 终端模式，带来流畅的用户体验
- 同时支持同步与异步命令处理器

## 使用方法

在你的 `Cargo.toml` 中添加：

```toml
[dependencies]
daemon_console = "0.3.0"
```

然后阅读[文档](https://docs.rs/daemon_console)。

如有任何问题，欢迎在 issues 中提问，我会乐意解答。

## 参与贡献

本项目包含两个分支：`main` 和 `dev`。
> v0.3.1 之后

文档将在 `main` 分支中维护，代码改动将在 `dev` 分支中进行。

一旦 `dev` 分支的代码趋于稳定，就会被合并回 `main` 分支，并发布为新版本。
> 感谢 [cargo-dist](https://github.com/axodotdev/cargo-dist) 和 [cargo-binstall](https://github.com/cargo-bins/cargo-binstall) 这类项目，它们帮助了这个项目的分发。

## 许可证

本项目基于 GPL-3.0 许可证发布。
