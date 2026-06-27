//! 配置命令：读取 / 保存配置，保存后即时重建翻译 Provider + 重注册热键。

use snaptext_core::translate::{build_provider, TranslationProvider};
use std::sync::Arc;
use tauri::{AppHandle, State};
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
    let new_translate: Option<Arc<dyn TranslationProvider>> = match build_provider(
        &cfg.translate,
        &state.client,
    ) {
        Ok(p) => Some(Arc::from(p)),
        Err(e) => {
            tracing::warn!(error = %e, "保存配置后重建翻译 Provider 失败（缺 Key？）");
            None
        }
    };
    let ready = new_translate.is_some();
    *state.translate.lock().await = new_translate;
    *state.config.lock().await = cfg.clone();

    // 重注册热键：先注销全部，再注册新的。
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    if let Ok(sc) = cfg.hotkey.trigger.parse::<Shortcut>() {
        if let Err(e) = gs.register(sc) {
            tracing::warn!(error = %e, hotkey = %cfg.hotkey.trigger, "重注册热键失败，保留空热键");
        } else {
            tracing::info!(hotkey = %cfg.hotkey.trigger, "热键已更新");
        }
    }

    Ok(ready)
}

/// 当前翻译是否可用（缺 Key 则 false）。
#[tauri::command]
pub async fn check_translate_ready(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.translate.lock().await.is_some())
}
