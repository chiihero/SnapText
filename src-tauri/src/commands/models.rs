//! 模型命令：就绪检查 + 后台下载（进度经事件推送前端）。
//!
//! 下载是长任务，命令本身触发后台线程，进度回调通过 `app.emit("download-progress", ...)`
//! 推送，前端监听该事件更新进度条。
//!
//! 注意：core 的 download_models 闭包是非 Send 的（内部 download_role 用 &dyn Fn
//! 跨 await），故下载在专用线程 + 专用 runtime 的 block_on 里跑，进度经 channel
//! （Send）转发到主 runtime 的 emit。

use std::sync::Arc;

use snaptext_core::config::Tier;
use snaptext_core::model_manager;
use tauri::{AppHandle, Emitter, State};

use crate::state::AppState;

/// 检查 OCR 模型是否就绪（首启引导 / 设置页诊断用）。
#[tauri::command]
pub fn models_ready(tier: Tier) -> bool {
    model_manager::is_models_ready(tier)
}

/// 后台下载 OCR 模型。命令立即返回，进度走 "download-progress" 事件，完成走 "download-done"。
#[tauri::command]
pub async fn download_models(
    app: AppHandle,
    state: State<'_, AppState>,
    tier: Tier,
) -> Result<(), String> {
    // 进度 channel（Send），连接下载线程与 emit。
    let (tx, mut rx) = tokio::sync::mpsc::channel::<ProgressMsg>(64);

    // emit 循环：主 runtime 上把进度推给前端。
    let app_clone = app.clone();
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            match msg {
                ProgressMsg::Step { role, received, total } => {
                    let _ = app_clone.emit(
                        "download-progress",
                        serde_json::json!({ "role": role, "received": received, "total": total }),
                    );
                }
                ProgressMsg::Done { ok, error } => {
                    let _ = app_clone.emit(
                        "download-done",
                        serde_json::json!({ "ok": ok, "error": error }),
                    );
                    break;
                }
            }
        }
    });

    // 下载线程：专用 runtime，避免非 Send future 污染主 runtime。
    let client = state.client.clone();
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.blocking_send(ProgressMsg::Done {
                    ok: false,
                    error: format!("创建 runtime 失败：{e}"),
                });
                return;
            }
        };
        let tx_arc = Arc::new(tx);
        let tx_for_cb = tx_arc.clone();
        let result = rt.block_on(model_manager::downloader::download_models(
            tier,
            &client,
            &[],
            move |role, received, total| {
                let _ = tx_for_cb.blocking_send(ProgressMsg::Step {
                    role: role.to_string(),
                    received,
                    total,
                });
            },
        ));
        match result {
            Ok(()) => {
                tracing::info!("模型下载完成");
                let _ = tx_arc.blocking_send(ProgressMsg::Done { ok: true, error: String::new() });
            }
            Err(e) => {
                tracing::error!(error = %e, "模型下载失败");
                let _ = tx_arc.blocking_send(ProgressMsg::Done {
                    ok: false,
                    error: e.to_string(),
                });
            }
        }
    });

    Ok(())
}

enum ProgressMsg {
    Step { role: String, received: u64, total: Option<u64> },
    Done { ok: bool, error: String },
}
