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

use crate::commands::ocr_translate::SelectResult;

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
    /// 最近一次全屏截图缓存（capture_all 写入，select_region 读取裁剪）。
    pub captured: Mutex<Vec<snaptext_core::types::CapturedFrame>>,
    /// 最近一次选区结果缓存（select_region 写入，Result 窗口 onMounted 拉取）。
    ///
    /// 与 captured 同款反竞态模式：select_region 完成时结果窗口可能还没加载，
    /// 前端事件/Pinia 跨窗口不共享，故缓存后端、由结果窗口主动命令拉取。
    pub last_result: Mutex<Option<SelectResult>>,
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

        Ok(Self {
            capture,
            ocr,
            translate: Mutex::new(translate),
            history,
            config: Mutex::new(config),
            client,
            captured: Mutex::new(Vec::new()),
            last_result: Mutex::new(None),
        })
    }
}
