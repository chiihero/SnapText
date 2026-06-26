//! `tracing` + `tracing-subscriber` 初始化，双输出（stderr + 文件）。
//!
//! 文件路径：`%APPDATA%\SnapText\logs\snaptext.log`（用 `dirs::config_dir()` 解析，不硬编码）。
//! 默认级别由调用方传入（取自 `Config.general.log_level`），`RUST_LOG` 优先级最高。

use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context, Result};
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// 初始化全局日志订阅。双输出：stderr + `%APPDATA%\SnapText\logs\snaptext.log`。
///
/// - `default_level`：配置中的默认级别（如 `"info"`）。
/// - `RUST_LOG` 环境变量优先级最高，可覆盖 `default_level`。
/// - 文件打开失败不致命：仅 stderr 输出，并记录一条告警（如果订阅已就绪则记入，否则忽略）。
pub fn init(default_level: &str) -> Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(default_level))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let file_layer = match open_log_file() {
        Ok(file) => Some(fmt::layer().with_writer(Mutex::new(file)).with_ansi(false)),
        Err(e) => {
            // 文件层失败不应阻止启动；告警打到 stderr（订阅尚未 init，eprintln 兜底）。
            eprintln!("警告：无法打开日志文件，仅使用 stderr 输出：{e}");
            None
        }
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(file_layer)
        .init();

    Ok(())
}

fn open_log_file() -> Result<std::fs::File> {
    let path = log_file_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("创建日志目录失败")?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("打开日志文件失败：{}", path.display()))
}

fn log_file_path() -> Result<PathBuf> {
    let dir = dirs::config_dir()
        .context("无法定位用户配置目录")?
        .join("SnapText")
        .join("logs");
    Ok(dir.join("snaptext.log"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_file_path_under_appdata() {
        let path = log_file_path().unwrap();
        assert!(
            path.ends_with("SnapText\\logs\\snaptext.log")
                || path.ends_with("SnapText/logs/snaptext.log")
        );
    }
}
