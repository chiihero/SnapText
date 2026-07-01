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
                ProgressMsg::Step {
                    role,
                    received,
                    total,
                } => {
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
                // 进度回调运行在 tokio async 上下文内，不能用 blocking_send（会 panic：
                // "Cannot block the current thread from within a runtime"）。
                // try_send 非阻塞，channel 满时丢弃进度（前端按最新值渲染，丢几帧无影响）。
                let _ = tx_for_cb.try_send(ProgressMsg::Step {
                    role: role.to_string(),
                    received,
                    total,
                });
            },
        ));
        match result {
            Ok(()) => {
                tracing::info!("模型下载完成");
                let _ = tx_arc.blocking_send(ProgressMsg::Done {
                    ok: true,
                    error: String::new(),
                });
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
    Step {
        role: String,
        received: u64,
        total: Option<u64>,
    },
    Done {
        ok: bool,
        error: String,
    },
}

/// 拉取 DeepSeek 可用模型列表（`GET {base_url}/models`），供设置页下拉。
///
/// DeepSeek API 唯一依据为中文官方文档（见 DESIGN §4.3「DeepSeek API 事实基准」）。
/// base_url + api_key 由前端从设置草稿传入（兼容第三方 OpenAI 兼容端点）。
/// api_key 为空 → 友好错误；网络/解析失败 → 中文错误。
#[tauri::command]
pub async fn list_deepseek_models(
    state: State<'_, AppState>,
    base_url: String,
    api_key: String,
) -> Result<Vec<String>, String> {
    if api_key.trim().is_empty() {
        return Err("请先填写 API Key".into());
    }
    #[derive(serde::Deserialize)]
    struct ModelsResp {
        data: Vec<ModelItem>,
    }
    #[derive(serde::Deserialize)]
    struct ModelItem {
        id: String,
    }
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let resp = state
        .client
        .get(&url)
        .bearer_auth(&api_key)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("请求模型列表失败：{e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("模型列表请求失败（HTTP {status}）：{body}"));
    }
    let parsed: ModelsResp = resp
        .json()
        .await
        .map_err(|e| format!("解析模型列表失败：{e}"))?;
    Ok(parsed.data.into_iter().map(|m| m.id).collect())
}
