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
            // 加载配置（构造 AppState 时会再读一次，这里仅为日志/热键/模型）。
            let config = Config::load().unwrap_or_default();

            // 日志初始化（按 config.general.log_level / log_file 配置，搬自旧 logging.rs）。
            init_logging(&config)?;

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
        .on_window_event(|window, event| {
            // 主窗口关闭拦截：开启"最小化到托盘"时隐藏窗口而非退出（设置页开关）。
            // 其他窗口（选区/结果/设置/历史）的 X 一律正常关闭，不拦截。
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    let state = window.app_handle().state::<crate::state::AppState>();
                    let minimize = state.config.blocking_lock().ui.minimize_to_tray_on_close;
                    if minimize {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::config_cmd::get_config,
            commands::config_cmd::save_config,
            commands::config_cmd::check_translate_ready,
            commands::config_cmd::get_default_prompt,
            commands::models::models_ready,
            commands::models::download_models,
            commands::models::list_deepseek_models,
            commands::capture::capture_all,
            commands::capture::get_last_capture,
            commands::capture::save_image_copy,
            commands::capture::log_diag,
            commands::capture::check_file,
            commands::ocr_translate::crop_region,
            commands::ocr_translate::get_last_crop,
            commands::ocr_translate::recognize_region,
            commands::ocr_translate::translate_region,
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

/// tracing 双输出（stderr + 日志文件），按 config 配置 level/file，搬自旧 logging.rs。
fn init_logging(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    use std::path::PathBuf;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::{fmt, EnvFilter};

    // log_level：空或缺失回退 "info"（env 优先级最高）。
    let level = if config.general.log_level.trim().is_empty() {
        "info"
    } else {
        config.general.log_level.trim()
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));

    let stderr_layer = fmt::layer().with_writer(std::io::stderr);

    // log_file：None 用默认 %APPDATA%\SnapText\logs\snaptext.log；Some 用自定义路径。
    let log_path = match &config.general.log_file {
        Some(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => dirs::config_dir()
            .map(|d| d.join("SnapText").join("logs").join("snaptext.log"))
            .ok_or("无法定位用户配置目录")?,
    };
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
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
