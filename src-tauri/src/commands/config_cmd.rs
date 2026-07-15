//! 配置命令：读取 / 保存配置，保存后即时重建翻译 Provider + 重注册热键。

use snaptext_core::ocr::{OcrProvider, PaddleOcrProvider};
use snaptext_core::translate::{build_provider, TranslationProvider};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::state::AppState;

/// 读取当前配置（前端启动时拉取缓存到 Pinia）。
#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> snaptext_core::Config {
    state.config.blocking_lock().clone()
}

/// 保存配置：写盘 + 重建翻译 Provider + 重注册热键。返回新配置是否让翻译可用。
#[tauri::command]
pub async fn save_config(
    app: AppHandle,
    state: State<'_, AppState>,
    cfg: snaptext_core::Config,
) -> Result<bool, String> {
    cfg.save().map_err(|e| format!("保存配置失败：{e}"))?;

    // 重建翻译 Provider。
    let new_translate: Option<Arc<dyn TranslationProvider>> =
        match build_provider(&cfg.translate, &state.client) {
            Ok(p) => Some(Arc::from(p)),
            Err(e) => {
                tracing::warn!(error = %e, "保存配置后重建翻译 Provider 失败（缺 Key？）");
                None
            }
        };
    let ready = new_translate.is_some();
    *state.translate.lock().await = new_translate;
    *state.config.lock().await = cfg.clone();

    // 重注册热键：先注销全部，再注册新的。结果写回 hotkey_error 供前端即时感知。
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let new_err = if let Ok(sc) = cfg.hotkey.trigger.parse::<Shortcut>() {
        match gs.register(sc) {
            Ok(()) => {
                tracing::info!(hotkey = %cfg.hotkey.trigger, "热键已更新");
                None
            }
            Err(e) => {
                let msg = format!(
                    "热键「{trigger}」注册失败：{e}（可能被其他程序占用，请更换快捷键）",
                    trigger = cfg.hotkey.trigger
                );
                tracing::warn!(error = %e, hotkey = %cfg.hotkey.trigger, "重注册热键失败，保留空热键");
                Some(msg)
            }
        }
    } else {
        None
    };
    *state.hotkey_error.lock().await = new_err;

    Ok(ready)
}

/// 全局热键注册状态：None=已注册；Some(msg)=注册失败（被占用等），前端用于提示。
#[tauri::command]
pub async fn get_hotkey_status(state: State<'_, AppState>) -> Result<Option<String>, String> {
    Ok(state.hotkey_error.lock().await.clone())
}

/// 当前翻译是否可用（缺 Key 则 false）。
#[tauri::command]
pub async fn check_translate_ready(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.translate.lock().await.is_some())
}

/// 返回系统默认翻译 prompt 模板（前端设置页"系统默认"模式只读展示用）。
/// 单一数据源：直接取自 `snaptext_core::translate::prompt::DEFAULT_PROMPT_TEMPLATE`，
/// 前端零硬编码，避免两端不同步。
#[tauri::command]
pub fn get_default_prompt() -> String {
    snaptext_core::translate::prompt::default_prompt_template().to_string()
}

/// 标记首启引导完成：把内存中的 config.general.onboarding_completed 置 true 并落盘。
///
/// 不复用 `save_config`——引导页的配置草稿已在调用本命令前由 `save_config` 落盘，
/// 此处仅置标志位，无需重建 Provider / 重注册热键。
#[tauri::command]
pub async fn complete_onboarding(state: State<'_, AppState>) -> Result<(), String> {
    let mut cfg = state.config.lock().await.clone();
    cfg.general.onboarding_completed = true;
    cfg.save().map_err(|e| format!("保存配置失败：{e}"))?;
    *state.config.lock().await = cfg;
    Ok(())
}

/// 纯构造：用 tier 加载 det/rec/dict 模型，返回 OCR Provider。
///
/// 同步重活（读模型文件 + ORT session 创建 + 图优化），调用方应放进 `spawn_blocking`，
/// 避免阻塞 tokio worker。不触碰 state，纯函数便于复用（懒重建 / 强制 reload 共用）。
fn build_ocr_provider(tier: snaptext_core::config::Tier) -> Result<Arc<dyn OcrProvider>, String> {
    let ocr = PaddleOcrProvider::new(
        snaptext_core::model_manager::det_model_path(tier).map_err(|e| e.to_string())?,
        snaptext_core::model_manager::rec_model_path(tier).map_err(|e| e.to_string())?,
        snaptext_core::model_manager::dict_path(tier).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("OCR 模型加载失败：{e}"))?;
    Ok(Arc::new(ocr))
}

/// 懒确保 OCR provider 存在：若已被空闲回收（None）则按当前 tier 重建，否则直接返回。
///
/// **单飞**：通过 `ocr_init` 锁 + 二次检查防止并发请求各自加载一套 ONNX session
/// （瞬时内存反而飙高）。模型加载走 `spawn_blocking` 不阻塞 runtime。
/// 成功时刷新 `last_used`（本次"使用"从重建完成起算）。
pub async fn ensure_ocr_provider(app: &AppHandle) -> Result<Arc<dyn OcrProvider>, String> {
    let state = app.state::<AppState>();

    // 快速路径：slot 有 provider，clone 一份 + 刷新 last_used。
    {
        let mut slot = state.ocr.lock().await;
        if let Some(p) = slot.provider.as_ref() {
            let p = Arc::clone(p);
            slot.last_used = std::time::Instant::now();
            return Ok(p);
        }
    }

    // 慢路径：provider 为 None，抢单飞锁后二次检查（防惊群）。
    let _init = state.ocr_init.lock().await;
    {
        let mut slot = state.ocr.lock().await;
        if let Some(p) = slot.provider.as_ref() {
            // 别的请求已重建好，复用。
            let p = Arc::clone(p);
            slot.last_used = std::time::Instant::now();
            return Ok(p);
        }
    }

    // 不持任何 state 锁地加载模型（同步重活放 blocking 线程）。
    let tier = state.config.lock().await.ocr.tier;
    let provider = tauri::async_runtime::spawn_blocking(move || build_ocr_provider(tier))
        .await
        .map_err(|e| format!("OCR 重建任务异常：{e}"))??;

    let mut slot = state.ocr.lock().await;
    slot.provider = Some(Arc::clone(&provider));
    slot.last_used = std::time::Instant::now();
    tracing::info!(?tier, "OCR Provider 懒重建完成");
    Ok(provider)
}

/// 强制重建 OCR Provider（模型下载完成后调此命令即时生效，无需重启）。
///
/// 与 `ensure_ocr_provider` 的区别：本命令**无条件重建**——即使 provider 已存在也替换
/// （用于下载完模型后旧 session 失效、或切档位后 tier 变了的场景）。
#[tauri::command]
pub async fn reload_ocr_provider(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let tier = state.config.lock().await.ocr.tier;
    let provider = tauri::async_runtime::spawn_blocking(move || build_ocr_provider(tier))
        .await
        .map_err(|e| format!("OCR 重建任务异常：{e}"))??;
    let mut slot = state.ocr.lock().await;
    slot.provider = Some(provider);
    slot.last_used = std::time::Instant::now();
    tracing::info!(?tier, "OCR Provider 已强制重建");
    Ok(())
}
