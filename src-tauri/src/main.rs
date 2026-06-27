// SnapText Tauri 后端入口。
//
// 串起：单实例 → 日志 → 构造 AppState（Provider 全套）→ 注册命令 + 插件 → 运行。
// 热键（阶段 4 接入 global-shortcut）、托盘（阶段 4）在此注册。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod state;
mod window;

use std::time::Duration;

use snaptext_core::config::Tier;
use snaptext_core::Config;
use tauri::Manager;
use tauri_plugin_global_shortcut::{Builder, GlobalShortcutExt, Shortcut, ShortcutState};

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // 第二实例：聚焦已有主窗口。
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(
            Builder::new()
                .with_handler(move |app, shortcut, event| {
                    // 只响应按下。
                    if event.state() == ShortcutState::Pressed {
                        let _ = shortcut;
                        // 先截图再开窗（避免窗口盖住桌面截到白屏），异步执行。
                        let handle = app.app_handle().clone();
                        tauri::async_runtime::spawn(async move {
                            if let Err(e) = window::trigger_capture_cmd(handle).await {
                                tracing::warn!(error = %e, "热键触发截图失败");
                            }
                        });
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // 日志初始化（tracing 双输出，搬自旧 logging.rs）。
            init_logging()?;

            // 加载配置（构造 AppState 时会再读一次，这里仅为热键/日志）。
            let config = Config::load().unwrap_or_default();

            // 首启：确保 OCR 模型就绪（缺失则同步下载）。
            ensure_models(config.ocr.tier)?;

            // 共享 HTTP 客户端。
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()?;

            // 构造 AppState（含全部 Provider，慢操作）。
            let app_state = state::AppState::build(client)?;
            app.manage(app_state);

            // 注册全局热键（默认 Ctrl+Alt+Q；阶段 4 从 config 读取）。
            let hotkey_str = config.hotkey.trigger.clone();
            let shortcut: Shortcut = hotkey_str
                .parse()
                .unwrap_or_else(|_| "Ctrl+Alt+Q".parse().unwrap());
            app.global_shortcut().register(shortcut)?;
            tracing::info!(hotkey = %hotkey_str, "全局热键已注册");

            // 托盘：显示主窗口 / 设置 / 历史 / 退出。
            window::build_tray(app.handle())?;

            tracing::info!("SnapText 启动完成");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::config_cmd::get_config,
            commands::config_cmd::save_config,
            commands::config_cmd::check_translate_ready,
            commands::models::models_ready,
            commands::models::download_models,
            commands::capture::capture_all,
            commands::capture::get_last_capture,
            commands::capture::save_image_copy,
            commands::capture::log_diag,
            commands::capture::check_file,
            commands::ocr_translate::select_region,
            window::trigger_capture_cmd,
            commands::history::history_list,
            commands::history::history_search,
            commands::history::history_get_screenshot,
            commands::history::history_delete,
            commands::history::history_clear,
            commands::history::history_stats,
        ])
        .run(tauri::generate_context!())
        .expect("启动 SnapText 失败");
}

/// tracing 双输出（stderr + %APPDATA%\SnapText\logs\snaptext.log），搬自旧 logging.rs。
fn init_logging() -> Result<(), Box<dyn std::error::Error>> {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::{fmt, EnvFilter};

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let stderr_layer = fmt::layer().with_writer(std::io::stderr);

    // 文件输出（目录不可写时退化为仅 stderr）。
    let log_dir = dirs::config_dir()
        .map(|d| d.join("SnapText").join("logs"))
        .ok_or("无法定位用户配置目录")?;
    std::fs::create_dir_all(&log_dir)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("snaptext.log"))?;
    let file_layer = fmt::layer().with_writer(file);

    tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .try_init()?;
    Ok(())
}

/// 首启确保模型就绪（缺失则同步下载，搬自旧 first_run.rs）。
fn ensure_models(tier: Tier) -> Result<(), Box<dyn std::error::Error>> {
    if snaptext_core::model_manager::is_models_ready(tier) {
        return Ok(());
    }
    tracing::info!(?tier, "模型缺失，开始下载...");
    let rt = tokio::runtime::Runtime::new()?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;
    rt.block_on(snaptext_core::model_manager::downloader::download_models(
        tier,
        &client,
        &[],
        |role, received, total| {
            eprintln!("[模型下载] {role}: {received} bytes (total={total:?})");
        },
    ))?;
    Ok(())
}
