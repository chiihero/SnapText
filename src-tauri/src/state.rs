//! 应用全局状态：持有所有 Provider 句柄 + 配置。
//!
//! 取代旧 egui 版的 Orchestrator 角色——Tauri 命令层直接读 state 调 Provider，
//! 无需 channel。Provider 在 setup 时一次性构造（PaddleOcrProvider 加载模型慢），
//! 命令内只取用。可变部分（翻译 Provider 重建、config 写回）用 Mutex 保护。

use std::sync::Arc;
use std::time::Instant;

use snaptext_core::capture::{CaptureProvider, WindowsCaptureProvider};
use snaptext_core::history::{HistoryStore, SqliteHistoryStore};
use snaptext_core::ocr::{OcrProvider, PaddleOcrProvider};
use snaptext_core::translate::{build_provider, TranslationProvider};
use snaptext_core::Config;
use tokio::sync::Mutex;

use crate::commands::ocr_translate::{LastCrop, LastOcr};

/// OCR session 槽：provider + 最后使用时间，合并到同一把锁，避免后台回收与
/// OCR 入口的 TOCTOU 竞态（参见 main.rs 的空闲回收 tick）。
pub struct OcrSlot {
    /// OCR Provider；模型缺失/空闲被回收时为 None。
    pub provider: Option<Arc<dyn OcrProvider>>,
    /// 最后一次取得 provider 的时刻。启动时初始化为 now()——
    /// 即"启动后 10 分钟没用也回收"，符合"挂着不用就回落"的诉求。
    pub last_used: Instant,
}

/// 全局应用状态，经 `app.manage()` 注入，命令用 `State<'_, AppState>` 取用。
pub struct AppState {
    pub capture: Arc<dyn CaptureProvider>,
    /// OCR session 槽（provider + last_used 合并在同一把锁，消除回收竞态）。
    /// provider 为 None 的两种情况：① 模型缺失启动降级；② 空闲超时被后台 tick 回收。
    /// 取用时经 ensure_ocr_provider 懒重建（含单飞锁 ocr_init）。
    pub ocr: Mutex<OcrSlot>,
    /// OCR Provider 重建单飞锁：保证同一时刻最多一个懒重建任务在加载模型，
    /// 防止并发请求各自加载一套 ONNX session（瞬时内存反而飙高）。
    pub ocr_init: Mutex<()>,
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
        // OCR 模型缺失时降级为 None（启动不崩），与翻译降级同款哲学。
        // 用户在引导页/设置页下载后调 reload_ocr_provider 命令重建。
        let ocr: Option<Arc<dyn OcrProvider>> = match PaddleOcrProvider::new(
            snaptext_core::model_manager::det_model_path(tier)?,
            snaptext_core::model_manager::rec_model_path(tier)?,
            snaptext_core::model_manager::dict_path(tier)?,
        ) {
            Ok(p) => Some(Arc::new(p)),
            Err(e) => {
                tracing::warn!(error = %e, "OCR Provider 构造失败（模型缺失？），已降级（请下载模型）");
                None
            }
        };
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
            ocr: Mutex::new(OcrSlot {
                provider: ocr,
                last_used: Instant::now(),
            }),
            ocr_init: Mutex::new(()),
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
