//! 应用全局状态：持有所有 Provider 句柄 + 配置。
//!
//! 取代旧 egui 版的 Orchestrator 角色——Tauri 命令层直接读 state 调 Provider，
//! 无需 channel。Provider 在 setup 时一次性构造（PaddleOcrProvider 加载模型慢），
//! 命令内只取用。可变部分（翻译 Provider 重建、config 写回）用 Mutex 保护。

use std::sync::Arc;

use snaptext_core::capture::{CaptureProvider, WindowsCaptureProvider};
use snaptext_core::history::{HistoryStore, SqliteHistoryStore};
use snaptext_core::ocr::{OcrProvider, PaddleOcrProvider};
use snaptext_core::translate::{build_provider, TranslationProvider};
use snaptext_core::Config;
use tokio::sync::Mutex;

use crate::commands::ocr_translate::{LastCrop, LastOcr};

/// 全局应用状态，经 `app.manage()` 注入，命令用 `State<'_, AppState>` 取用。
pub struct AppState {
    pub capture: Arc<dyn CaptureProvider>,
    pub ocr: Arc<dyn OcrProvider>,
    /// 翻译 Provider；缺 API Key 时为 None（设置保存后 rebuild）。
    pub translate: Mutex<Option<Arc<dyn TranslationProvider>>>,
    pub history: Arc<dyn HistoryStore>,
    pub config: Mutex<Config>,
    /// 共享 HTTP 客户端（翻译 Provider 构造 + 模型下载复用）。
    pub client: reqwest::Client,
    /// 最近一次全屏截图缓存（capture_all 写入，crop_region 读取裁剪）。
    pub captured: Mutex<Vec<snaptext_core::types::CapturedFrame>>,
    /// 三层命令接力缓存：crop_region 写入，recognize_region 读取 OCR。
    pub last_crop: Mutex<Option<LastCrop>>,
    /// 三层命令接力缓存：recognize_region 写入，translate_region 读取翻译。
    pub last_ocr: Mutex<Option<LastOcr>>,
    /// 全局热键注册状态：None=已注册成功；Some(msg)=注册失败（被其他程序占用等）。
    /// 启动注册或 save_config 重注册时写入，前端经 get_hotkey_status 拉取并提示。
    pub hotkey_error: Mutex<Option<String>>,
}

impl AppState {
    /// 在 Tauri setup 中构造。构造失败（如模型缺失）应阻断启动。
    pub fn build(client: reqwest::Client) -> anyhow::Result<Self> {
        let mut config = Config::load().unwrap_or_else(|e| {
            tracing::warn!(error = %e, "加载配置失败，使用默认配置");
            Config::default()
        });
        config.apply_env_overrides();

        let tier = config.ocr.tier;
        let capture = Arc::new(WindowsCaptureProvider::new());
        let ocr = Arc::new(PaddleOcrProvider::new(
            snaptext_core::model_manager::det_model_path(tier)?,
            snaptext_core::model_manager::rec_model_path(tier)?,
            snaptext_core::model_manager::dict_path(tier)?,
        )?);
        let translate: Option<Arc<dyn TranslationProvider>> = match build_provider(
            &config.translate,
            &client,
        ) {
            Ok(p) => {
                tracing::info!("翻译 Provider 就绪");
                Some(Arc::from(p))
            }
            Err(e) => {
                tracing::warn!(error = %e, "翻译 Provider 构造失败，已降级（请在设置中配置 API Key）");
                None
            }
        };
        let history = Arc::new(SqliteHistoryStore::open_default()?);

        // 启动清理：按配置删除过期/超量记录（清理逻辑在 core 已实现，此前未接线）。
        // cleanup_blocking 是同步阻塞调用，但启动时跑一次且删除很快，可接受。
        if config.history.auto_clean_on_start {
            if let Err(e) =
                history.cleanup_blocking(config.history.retention_days, config.history.max_records)
            {
                tracing::warn!(error = %e, "启动历史清理失败");
            }
        }

        Ok(Self {
            capture,
            ocr,
            translate: Mutex::new(translate),
            history,
            config: Mutex::new(config),
            client,
            captured: Mutex::new(Vec::new()),
            last_crop: Mutex::new(None),
            last_ocr: Mutex::new(None),
            hotkey_error: Mutex::new(None),
        })
    }
}
