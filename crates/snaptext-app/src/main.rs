//! SnapText 入口（DU-07 骨架 + DU-11 集成 orchestrator 端到端）。
//!
//! 串起：single-instance → 配置/日志 → 首启下载模型 → 手动 tokio runtime →
//! 构造真实 Provider → Orchestrator + channel → tray + hotkey → eframe 事件循环。

mod clipboard;
mod first_run;
mod hotkey;
mod logging;
mod orchestrator;
mod tray;
mod ui;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use eframe::egui;
use snaptext_core::capture::WindowsCaptureProvider;
use snaptext_core::history::SqliteHistoryStore;
use snaptext_core::model_manager::{det_model_path, dict_path, rec_model_path};
use snaptext_core::ocr::PaddleOcrProvider;
use snaptext_core::translate::{build_provider, TranslationProvider};
use snaptext_core::Config;
use tokio::sync::mpsc;

use crate::orchestrator::Orchestrator;

fn main() -> Result<()> {
    // 1. 单实例保护。
    let instance = single_instance::SingleInstance::new("SnapText-mutex-2026")?;
    if !instance.is_single() {
        eprintln!("SnapText 已在运行，拒绝启动第二实例。");
        std::process::exit(0);
    }

    // 2. 配置 + 日志。
    let mut config = Config::load().unwrap_or_else(|e| {
        eprintln!("加载配置失败，使用默认配置：{e}");
        Config::default()
    });
    config.apply_env_overrides();
    logging::init(&config.general.log_level)?;
    tracing::info!("SnapText 启动");

    // 3. 首启：确保模型就绪（缺失则下载）。
    let tier = config.ocr.tier;
    first_run::ensure_models(tier)?;

    // 4. 手动 tokio runtime（eframe 阻塞主线程，CONVENTIONS §6）。
    let runtime = Arc::new(tokio::runtime::Runtime::new()?);
    let _enter = runtime.enter(); // 让 orchestrator.run 内的 tokio::spawn 找到 runtime。

    // 5. 构造 Provider。
    let capture = Arc::new(WindowsCaptureProvider::new());
    let ocr = Arc::new(PaddleOcrProvider::new(
        det_model_path(tier)?,
        rec_model_path(tier)?,
        dict_path(tier)?,
    )?);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    // 翻译 Provider：缺 API Key 时降级为 None，程序照常启动（设置中填 Key 后即时重建）。
    let translate: Option<Arc<dyn TranslationProvider>> = match build_provider(
        &config.translate,
        &client,
    ) {
        Ok(p) => {
            tracing::info!("翻译 Provider 就绪");
            Some(Arc::from(p))
        }
        Err(e) => {
            tracing::warn!(error = %e, "翻译 Provider 构造失败，已降级启动（请在设置中配置 API Key）");
            None
        }
    };
    let history = Arc::new(SqliteHistoryStore::open_default()?);

    // 6. Orchestrator + channel。
    let orchestrator = Orchestrator::new(
        capture,
        ocr,
        translate,
        history,
        config.ocr.postprocess,
        config.translate.clone(),
        client,
    );
    let (cmd_tx, cmd_rx) = mpsc::channel(16);
    let (event_tx, event_rx) = mpsc::channel(16);
    orchestrator.run(cmd_rx, event_tx);

    // 7. tray + hotkey（主线程）。
    let tray_icon = tray::build()?;
    let (hotkey_manager, current_hotkey) = hotkey::register(&config.hotkey)?;
    tracing::info!("托盘与热键就绪，端到端流程已接入");

    // 8. eframe（默认显示主窗口，便于观察；DU-08 overlay 后默认隐藏）。
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SnapText")
            .with_inner_size([440.0, 340.0])
            .with_visible(true),
        ..Default::default()
    };
    let app = ui::SnapTextApp::new(
        runtime,
        tray_icon,
        hotkey_manager,
        current_hotkey,
        cmd_tx,
        event_rx,
        config.clone(),
    );
    eframe::run_native(
        "SnapText",
        options,
        Box::new(move |cc| {
            ui::theme::apply(&cc.egui_ctx); // 应用浅色主题（字体注入前）。
            ui::fonts::install(&cc.egui_ctx); // 注入系统中文字体，避免中文乱码。
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe 启动失败：{e:?}"))
}
