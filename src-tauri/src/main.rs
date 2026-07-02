// SnapText Tauri 后端入口。
//
// 串起：单实例 → 日志 → 构造 AppState（Provider 全套）→ 注册命令 + 插件 → 运行。
// 热键（阶段 4 接入 global-shortcut）、托盘（阶段 4）在此注册。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod state;
mod window;

use std::time::Duration;

use snaptext_core::Config;
use tauri::http::{Response, StatusCode};
use tauri::Manager;
use tauri_plugin_global_shortcut::{Builder, GlobalShortcutExt, Shortcut, ShortcutState};

fn main() {
    tauri::Builder::default()
        // shot:// 自定义协议：选区窗口 <img> 经此从内存直接取全屏截图 BMP 字节，
        // 取代"全屏 RGBA 写临时 BMP 文件 + webview 读盘"（省 ~150ms 写盘/读盘）。
        // 监听器解析 URI 里的 monitor safe_id，从 state.captured 找对应帧编码 BMP 返回。
        .register_asynchronous_uri_scheme_protocol("shot", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            // 异步协议：取 tokio Mutex（captured）需 await，放独立 task 执行。
            tauri::async_runtime::spawn(async move {
                let path = request.uri().path();
                // path 形如 "/DISPLAY1"（safe_id：原 monitor id 已替换非法字符）。
                let wanted = path.trim_start_matches('/');
                let state = app.state::<crate::state::AppState>();
                // 在持锁期间直接编码 BMP，免 clone 整张全屏图（4K RGBA 单份 30MB+）。
                // BMP 无压缩，编码很快；shot:// 仅选区窗绘制时拉一次，无并发竞争。
                // 三态：Some(Ok)=成功 / Some(Err)=编码失败(500) / None=无对应帧(404)，
                // 保持原有状态码语义。
                let encoded: Option<Result<Vec<u8>, ()>> = {
                    let captured = state.captured.lock().await;
                    captured
                        .iter()
                        .find(|f| {
                            let safe = f.monitor.id.as_str().replace(['\\', '/', ':'], "_");
                            safe == wanted
                        })
                        .map(|f| {
                            let mut buf = std::io::Cursor::new(Vec::new());
                            f.image
                                .write_to(&mut buf, image::ImageFormat::Bmp)
                                .map(|()| buf.into_inner())
                                .map_err(|e| {
                                    tracing::warn!(error = %e, "shot 协议编码 BMP 失败");
                                })
                        })
                };
                let response = match encoded {
                    Some(Ok(body)) => Response::builder()
                        .header("Content-Type", "image/bmp")
                        // shot:// URL 按 monitor id 固定，无此头 WebView2 会命中缓存显示旧截图。
                        // 前端另加 ?t= 时间戳双保险。
                        .header("Cache-Control", "no-store")
                        .body(body),
                    Some(Err(())) => Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Vec::new()),
                    None => Response::builder()
                        .status(StatusCode::NOT_FOUND)
                        .body(Vec::new()),
                };
                responder.respond(response.unwrap());
            });
        })
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
                        // 选区窗口启动时已预创建并隐藏（不遮挡桌面），此处截图后
                        // emit 事件通知前端绘制 + show，省掉每次 WebView2 冷启动。
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

            // 共享 HTTP 客户端。
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()?;

            // 构造 AppState（含全部 Provider，慢操作）。
            let app_state = state::AppState::build(client)?;
            app.manage(app_state);

            // 注册全局热键：失败不阻断启动（可能被其他程序占用），降级为状态供前端提示。
            let hotkey_str = config.hotkey.trigger.clone();
            let shortcut: Shortcut = hotkey_str
                .parse()
                .unwrap_or_else(|_| "Ctrl+Alt+Q".parse().unwrap());
            let hotkey_error = match app.global_shortcut().register(shortcut) {
                Ok(()) => {
                    tracing::info!(hotkey = %hotkey_str, "全局热键已注册");
                    None
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        hotkey = %hotkey_str,
                        "全局热键注册失败（可能被占用），降级运行"
                    );
                    Some(format!(
                        "热键「{hotkey_str}」注册失败：{e}（可能被其他程序占用，请前往设置更换快捷键）"
                    ))
                }
            };
            *app.state::<crate::state::AppState>()
                .hotkey_error
                .blocking_lock() = hotkey_error;

            // 托盘：显示主窗口 / 设置 / 历史 / 退出。
            window::build_tray(app.handle())?;

            // 预创建选区窗口并隐藏（WebView2/Vue 实例常驻预热）。热键时直接 show，
            // 省掉每次创建窗口的 ~400ms 冷启动（参照 Snipaste/Flameshot 模式）。
            window::ensure_capture_window(app.handle())?;

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
            commands::config_cmd::get_hotkey_status,
            commands::config_cmd::complete_onboarding,
            commands::config_cmd::reload_ocr_provider,
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
