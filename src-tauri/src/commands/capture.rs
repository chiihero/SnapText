//! 截图命令：capture_all（截全屏 + 缓存帧）。
//!
//! CapturedFrame 含 RgbaImage（不可序列化），截图结果缓存进 state.captured，
//! 前端经 shot:// 自定义协议从内存直接取 BMP（见 main.rs 注册），不再写临时文件。
//! 前端选区窗口拿 shot_url 画全屏图，框选后调 crop_region 传回 bbox（三层命令第 1 层）。

use serde::Serialize;
use tauri::State;

use crate::state::AppState;

/// Windows 自定义协议的 URL 形式：`http://<scheme>.localhost/<path>`。
/// Tauri 在 Windows/Android 上用 http://，macOS/Linux 用 `<scheme>://localhost/`。
fn shot_uri(safe_id: &str) -> String {
    format!("http://shot.localhost/{safe_id}")
}

/// 给前端的显示器信息（含截图 shot:// URI，前端直接当 <img src> 用）。
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
    /// 全屏截图 URI（shot:// 协议，前端直接当 src，从内存取 BMP）。
    pub shot_path: String,
}

/// 截所有显示器：帧缓存进 state + 返回每屏信息（含 shot_uri）。
///
/// 前端拿到后打开选区窗口，按 monitor 渲染对应截图。
#[tauri::command]
pub async fn capture_all(
    state: State<'_, AppState>,
) -> Result<Vec<MonitorDto>, String> {
    do_capture_all(&state).await
}

/// 取最近一次截图的 DTO（trigger_capture_cmd 已截图缓存，Capture.vue 主动拉取）。
///
/// 设计：选区窗口常驻隐藏，热键截图后 emit 事件通知前端绘制（窗口已加载完，
/// 事件不再丢失）；但 onMounted 仍提供本命令作为兜底（如窗口重载后首次）。
#[tauri::command]
pub async fn get_last_capture(
    state: State<'_, AppState>,
) -> Result<Vec<MonitorDto>, String> {
    let captured = state.captured.lock().await;
    if captured.is_empty() {
        return Err("无缓存截图".into());
    }
    Ok(frames_to_dtos(&captured))
}

fn frames_to_dtos(
    frames: &[snaptext_core::types::CapturedFrame],
) -> Vec<MonitorDto> {
    frames
        .iter()
        .map(|frame| frame_to_dto(frame))
        .collect()
}

fn frame_to_dto(frame: &snaptext_core::types::CapturedFrame) -> MonitorDto {
    let m = &frame.monitor;
    let safe_id = m.id.as_str().replace(['\\', '/', ':'], "_");
    MonitorDto {
        id: m.id.as_str().to_string(),
        name: m.name.clone(),
        width: m.width,
        height: m.height,
        scale: m.scale,
        x: m.x,
        y: m.y,
        primary: m.is_primary,
        shot_path: shot_uri(&safe_id),
    }
}

/// 截图核心逻辑（命令 + trigger_capture 复用）。
///
/// 截所有显示器 → 缓存帧进 state.captured → 返回 DTO（shot URI 指向内存）。
/// 抽出来让 trigger_capture 能"先截图再开窗"，避免选区窗口盖住桌面导致截到白屏。
/// 选区窗口启动时已预创建并隐藏，此函数不再创建窗口（仅 emit 事件通知前端）。
pub async fn do_capture_all(state: &AppState) -> Result<Vec<MonitorDto>, String> {
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

    let dtos = frames_to_dtos(&frames);
    *state.captured.lock().await = frames;
    tracing::info!(total_ms = start.elapsed().as_millis(), "capture_all 完成（无写盘）");
    Ok(dtos)
}

// crop_region / recognize_region / translate_region（三层命令）在 ocr_translate.rs 实现。

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
