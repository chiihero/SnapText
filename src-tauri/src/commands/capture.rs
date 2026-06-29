//! 截图命令：capture_all（截全屏 + 缓存帧 + 导出 PNG 给前端选区窗口）。
//!
//! CapturedFrame 含 RgbaImage（不可序列化），截图结果缓存进 state.captured，
//! 同时编码 PNG 写临时文件，用 `tauri::path` + `convertFileSrc` 暴露给 webview。
//! 前端选区窗口拿 shot_url 画全屏图，框选后调 select_region 传回 bbox。

use std::io::Cursor;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::state::AppState;

/// 给前端的显示器信息（含截图文件绝对路径，前端用 convertFileSrc 转 URL）。
#[derive(Serialize)]
pub struct MonitorDto {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub x: i32,
    pub y: i32,
    pub primary: bool,
    /// 截图文件绝对路径（前端 convertFileSrc 转 webview URL）。
    pub shot_path: String,
}

/// 截所有显示器：帧缓存进 state + 写临时 PNG + 返回每屏信息（含 shot_url）。
///
/// 前端拿到后打开选区窗口，按 monitor 渲染对应截图。
#[tauri::command]
pub async fn capture_all(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<MonitorDto>, String> {
    do_capture_all(&app, &state).await
}

/// 取最近一次截图的 DTO（trigger_capture_cmd 已截图缓存，Capture.vue 主动拉取）。
///
/// 设计：trigger_capture_cmd "先截图后开窗"，截图完成时窗口页面可能还没加载，
/// emit 事件会丢失（竞态）。改由前端 onMounted 主动调本命令拉取，无竞态。
#[tauri::command]
pub async fn get_last_capture(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<MonitorDto>, String> {
    let captured = state.captured.lock().await;
    if captured.is_empty() {
        return Err("无缓存截图".into());
    }
    // 从已缓存帧重建 DTO（路径与 do_capture_all 写入一致）。
    let tmp_dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("定位缓存目录失败：{e}"))?
        .join("tmp");
    let mut dtos = Vec::with_capacity(captured.len());
    for frame in captured.iter() {
        let m = &frame.monitor;
        let safe_id = m.id.as_str().replace(['\\', '/', ':'], "_");
        // 用 BMP（无压缩）而非 PNG：全屏 RGBA 编码 PNG 单线程耗时 ~1s，
        // BMP 近乎内存拷贝（<50ms）。此文件仅喂选区窗口 webview 显示，不持久化。
        let shot_path = tmp_dir.join(format!("shot_{safe_id}.bmp"));
        dtos.push(MonitorDto {
            id: m.id.as_str().to_string(),
            name: m.name.clone(),
            width: m.width,
            height: m.height,
            scale: m.scale,
            x: m.x,
            y: m.y,
            primary: m.is_primary,
            shot_path: shot_path.to_string_lossy().to_string(),
        });
    }
    Ok(dtos)
}

/// 截图核心逻辑（命令 + trigger_capture 复用）。
///
/// 截所有显示器 → 缓存帧进 state.captured → 写临时 BMP → 返回 DTO。
/// 抽出来让 trigger_capture 能"先截图再开窗"，避免选区窗口盖住桌面导致截到白屏。
pub async fn do_capture_all(
    app: &AppHandle,
    state: &AppState,
) -> Result<Vec<MonitorDto>, String> {
    let start = std::time::Instant::now();
    tracing::info!("capture_all 开始执行");
    let frames = state
        .capture
        .capture_all()
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "capture_all 截图失败");
            format!("截图失败：{e}")
        })?;
    tracing::info!(count = frames.len(), capture_ms = start.elapsed().as_millis(), "capture_all 抓帧完成");
    let encode_start = std::time::Instant::now();

    // 写临时 PNG：复用 appdata 下的 tmp 目录。
    let tmp_dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("定位缓存目录失败：{e}"))?
        .join("tmp");
    std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("创建缓存目录失败：{e}"))?;

    let mut dtos = Vec::with_capacity(frames.len());
    for frame in &frames {
        let m = &frame.monitor;
        // 文件名用 monitor id 的安全形式。
        let safe_id = m.id.as_str().replace(['\\', '/', ':'], "_");
        // 用 BMP（无压缩）而非 PNG：全屏 PNG 编码是框选前延迟主因（~1s），
        // BMP 近乎内存拷贝。此临时文件仅喂选区窗口 webview 显示，不持久化。
        let shot_path = tmp_dir.join(format!("shot_{safe_id}.bmp"));
        let mut buf = Cursor::new(Vec::new());
        frame
            .image
            .write_to(&mut buf, image::ImageFormat::Bmp)
            .map_err(|e| format!("编码截图失败：{e}"))?;
        std::fs::write(&shot_path, buf.into_inner())
            .map_err(|e| format!("写截图文件失败：{e}"))?;

        let shot_path = shot_path.to_string_lossy().to_string();
        dtos.push(MonitorDto {
            id: m.id.as_str().to_string(),
            name: m.name.clone(),
            width: m.width,
            height: m.height,
            scale: m.scale,
            x: m.x,
            y: m.y,
            primary: m.is_primary,
            shot_path,
        });
    }

    tracing::info!(encode_ms = encode_start.elapsed().as_millis(), total_ms = start.elapsed().as_millis(), "capture_all BMP 编码+写盘完成");
    *state.captured.lock().await = frames;
    Ok(dtos)
}

// select_region（选区→OCR→翻译→配对→历史）在 ocr_translate.rs 实现。

/// 保存结果图片：把指定路径的截图复制到用户选定路径。
///
/// 前端 Result.vue 调 dialog 选目标路径后调用本命令写盘（webview 无法直接写盘）。
#[tauri::command]
pub async fn save_image_copy(source_path: String, dest_path: String) -> Result<(), String> {
    std::fs::copy(&source_path, &dest_path).map_err(|e| format!("复制图片失败：{e}"))?;
    Ok(())
}

/// 诊断日志：前端把关键状态（路径、URL、加载结果）传过来，写入 tracing 日志文件。
///
/// 选区窗口白屏排查用：前端无法直接写文件，借此命令把 webview 侧信息落到
/// %APPDATA%\SnapText\logs\snaptext.log，便于定位 asset 协议加载失败等问题。
#[tauri::command]
pub async fn log_diag(tag: String, message: String) {
    tracing::info!(tag = %tag, diag = %message, "前端诊断");
}

/// 验证文件是否真实存在 + 返回信息（诊断用）。
#[tauri::command]
pub async fn check_file(path: String) -> Result<String, String> {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return Ok(format!("不存在: {path}"));
    }
    let meta = std::fs::metadata(p).map_err(|e| format!("读元数据失败: {e}"))?;
    Ok(format!("存在, 大小={} 字节, 绝对路径={}", meta.len(), p.canonicalize().map(|x| x.display().to_string()).unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    //! capture 命令层测试：save_image_copy 文件复制逻辑。
    use super::save_image_copy;

    #[tokio::test]
    async fn save_image_copy_writes_file() {
        // 源文件内容应被完整复制到目标路径。
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.png");
        let dst = dir.path().join("dst.png");
        std::fs::write(&src, b"PNG-DATA").unwrap();

        save_image_copy(src.to_string_lossy().to_string(), dst.to_string_lossy().to_string())
            .await
            .expect("复制应成功");

        assert_eq!(std::fs::read(&dst).unwrap(), b"PNG-DATA");
    }

    #[tokio::test]
    async fn save_image_copy_missing_source_errors() {
        // 源不存在：返回错误字符串（不 panic）。
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("nope.png");
        let dst = dir.path().join("dst.png");
        let err = save_image_copy(src.to_string_lossy().to_string(), dst.to_string_lossy().to_string())
            .await
            .expect_err("源缺失应报错");
        assert!(err.contains("复制图片失败"));
    }
}
