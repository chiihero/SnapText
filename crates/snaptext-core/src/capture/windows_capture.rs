//! Windows 截图 Provider：WGC（Graphics Capture API）优先，
//! 失败回退到 DXGI Desktop Duplication。
//!
//! WGC 用 `start_free_threaded` 启动后台会话，在 `on_frame_arrived` 拿到首帧后
//! 通过 channel 送出，外部收到后 `stop()` 回收线程。WGC 失败（如首启权限拒绝、
//! 会话异常）时回退 DXGI（同步取单帧，更可靠）。WGC 首启会触发 Win11 屏幕捕获
//! 权限提示（R6，不可避免）。

use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use image::RgbaImage;
use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::dxgi_duplication_api::{
    DxgiDuplicationApi, DxgiDuplicationFormat, Error as DxgiError,
};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::{Error as MonitorError, Monitor};
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

use super::CaptureProvider;
use crate::error::{CaptureError, CoreError};
use crate::types::{CapturedFrame, MonitorId, MonitorInfo};

/// WGC 等待首帧的最长时间。
const WGC_FRAME_TIMEOUT: Duration = Duration::from_secs(3);
/// DXGI 单次 `AcquireNextFrame` 的等待（毫秒）。
const DXGI_FRAME_TIMEOUT_MS: u32 = 100;
/// DXGI 空帧/超时最大重试次数。
const DXGI_MAX_RETRIES: usize = 10;

/// 默认 Windows 截图 Provider。
pub struct WindowsCaptureProvider;

impl WindowsCaptureProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsCaptureProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CaptureProvider for WindowsCaptureProvider {
    async fn list_monitors(&self) -> Result<Vec<MonitorInfo>, CoreError> {
        tokio::task::spawn_blocking(list_monitors_blocking)
            .await
            .map_err(join_err)?
            .map_err(CoreError::Capture)
    }

    async fn capture_monitor(&self, id: &MonitorId) -> Result<CapturedFrame, CoreError> {
        let id = id.clone();
        tokio::task::spawn_blocking(move || capture_monitor_blocking(&id))
            .await
            .map_err(join_err)?
            .map_err(CoreError::Capture)
    }

    async fn capture_all(&self) -> Result<Vec<CapturedFrame>, CoreError> {
        tokio::task::spawn_blocking(capture_all_blocking)
            .await
            .map_err(join_err)?
            .map_err(CoreError::Capture)
    }
}

fn join_err(e: tokio::task::JoinError) -> CoreError {
    CoreError::Capture(CaptureError::BackendUnavailable(format!(
        "截图工作线程异常：{e}"
    )))
}

fn list_monitors_blocking() -> Result<Vec<MonitorInfo>, CaptureError> {
    let monitors = enumerate_monitors()?;
    monitors.iter().map(monitor_to_info).collect()
}

fn capture_all_blocking() -> Result<Vec<CapturedFrame>, CaptureError> {
    let monitors = enumerate_monitors()?;
    let mut frames = Vec::with_capacity(monitors.len());
    for m in &monitors {
        let info = monitor_to_info(m)?;
        let image = capture_one_frame(m)?;
        frames.push(CapturedFrame {
            monitor: info,
            image,
            captured_at: SystemTime::now(),
        });
    }
    Ok(frames)
}

fn capture_monitor_blocking(id: &MonitorId) -> Result<CapturedFrame, CaptureError> {
    let monitor = find_monitor(id)?;
    let info = monitor_to_info(&monitor)?;
    let image = capture_one_frame(&monitor)?;
    Ok(CapturedFrame {
        monitor: info,
        image,
        captured_at: SystemTime::now(),
    })
}

fn enumerate_monitors() -> Result<Vec<Monitor>, CaptureError> {
    Monitor::enumerate().map_err(|e| CaptureError::BackendUnavailable(e.to_string()))
}

fn find_monitor(id: &MonitorId) -> Result<Monitor, CaptureError> {
    for m in enumerate_monitors()? {
        if let Ok(name) = m.device_name() {
            if name == id.as_str() {
                return Ok(m);
            }
        }
    }
    Err(CaptureError::MonitorNotFound(id.to_string()))
}

/// 截取一帧：WGC 优先，失败回退 DXGI。
fn capture_one_frame(monitor: &Monitor) -> Result<RgbaImage, CaptureError> {
    if let Ok(image) = capture_wgc(*monitor) {
        return Ok(image);
    }
    capture_dxgi(*monitor)
}

// ===== WGC 路径 =====

/// WGC 单帧处理器：拿到首帧后通过 channel 送出，外部停止会话。
struct OneShotFrameCapture {
    sender: mpsc::Sender<Result<RgbaImage, String>>,
}

impl GraphicsCaptureApiHandler for OneShotFrameCapture {
    type Flags = mpsc::Sender<Result<RgbaImage, String>>;
    type Error = String;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self { sender: ctx.flags })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let width = frame.width();
        let height = frame.height();
        let buf = frame.buffer().map_err(|e| e.to_string())?;
        let mut scratch = Vec::new();
        let pixels = buf.as_nopadding_buffer(&mut scratch);
        let image = RgbaImage::from_raw(width, height, pixels.to_vec())
            .ok_or_else(|| "WGC 帧尺寸无效".to_string())?;
        // 仅关心首帧；接收方取走后即停止会话，后续发送会被忽略。
        let _ = self.sender.send(Ok(image));
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn capture_wgc(monitor: Monitor) -> Result<RgbaImage, CaptureError> {
    let (tx, rx) = mpsc::channel::<Result<RgbaImage, String>>();
    let settings = Settings::new(
        monitor,
        CursorCaptureSettings::Default,
        DrawBorderSettings::WithoutBorder,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        tx,
    );
    let control = OneShotFrameCapture::start_free_threaded(settings)
        .map_err(|e| CaptureError::BackendUnavailable(format!("WGC 会话启动失败：{e:?}")))?;
    // 关键：无论 recv_timeout 成功还是超时/出错，都必须 stop() 回收会话线程，
    // 否则每次失败截图泄漏一个 WGC 后台捕获会话（B3）。
    let result = rx.recv_timeout(WGC_FRAME_TIMEOUT);
    let _ = control.stop();
    let result = result.map_err(|e| CaptureError::CaptureFailed(format!("WGC 等待首帧失败：{e}")))?;
    result.map_err(CaptureError::CaptureFailed)
}

// ===== DXGI 路径 =====

fn capture_dxgi(monitor: Monitor) -> Result<RgbaImage, CaptureError> {
    let mut dup = DxgiDuplicationApi::new(monitor)
        .map_err(|e| CaptureError::BackendUnavailable(format!("DXGI 初始化失败：{e:?}")))?;
    for _ in 0..DXGI_MAX_RETRIES {
        match dup.acquire_next_frame(DXGI_FRAME_TIMEOUT_MS) {
            Ok(mut frame) => {
                // 首帧可能 LastPresentTime == 0（空帧），跳过等待真实内容。
                if frame.frame_info().LastPresentTime == 0 {
                    continue;
                }
                let width = frame.width();
                let height = frame.height();
                let format = frame.format();
                let buf = frame
                    .buffer()
                    .map_err(|e| CaptureError::CaptureFailed(format!("DXGI 映射帧失败：{e:?}")))?;
                let mut scratch = Vec::new();
                let pixels = buf.as_nopadding_buffer(&mut scratch).to_vec();
                return pixels_to_rgba(pixels, width, height, format);
            }
            Err(DxgiError::Timeout) => continue,
            Err(DxgiError::AccessLost) => {
                dup = dup.recreate().map_err(|e| {
                    CaptureError::BackendUnavailable(format!("DXGI 重建失败：{e:?}"))
                })?;
                continue;
            }
            Err(e) => {
                return Err(CaptureError::CaptureFailed(format!("DXGI 取帧失败：{e:?}")));
            }
        }
    }
    Err(CaptureError::CaptureFailed(format!(
        "DXGI 连续 {DXGI_MAX_RETRIES} 次未获取有效帧"
    )))
}

/// 将 DXGI 原始像素转为 `RgbaImage`（BGRA8 需交换通道）。
fn pixels_to_rgba(
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    format: DxgiDuplicationFormat,
) -> Result<RgbaImage, CaptureError> {
    match format {
        DxgiDuplicationFormat::Rgba8 | DxgiDuplicationFormat::Rgba8Srgb => {
            RgbaImage::from_raw(width, height, pixels)
                .ok_or_else(|| CaptureError::CaptureFailed("DXGI RGBA 像素尺寸无效".into()))
        }
        DxgiDuplicationFormat::Bgra8 | DxgiDuplicationFormat::Bgra8Srgb => {
            let mut data = pixels;
            for px in data.chunks_exact_mut(4) {
                px.swap(0, 2); // BGRA → RGBA
            }
            RgbaImage::from_raw(width, height, data)
                .ok_or_else(|| CaptureError::CaptureFailed("DXGI BGRA 像素尺寸无效".into()))
        }
        other => Err(CaptureError::CaptureFailed(format!(
            "DXGI 不支持的像素格式：{other:?}"
        ))),
    }
}

// ===== 显示器元数据 =====

fn monitor_to_info(monitor: &Monitor) -> Result<MonitorInfo, CaptureError> {
    let device_name = monitor.device_name().map_err(monitor_err)?;
    let friendly = monitor.name().unwrap_or_else(|_| device_name.clone());
    let width = monitor.width().map_err(monitor_err)?;
    let height = monitor.height().map_err(monitor_err)?;
    let is_primary = Monitor::primary().map(|p| p == *monitor).unwrap_or(false);
    // 截图帧尺寸 = 物理像素（windows-capture width/height 取自 dmPelsWidth/Height），
    // 前端窗口是逻辑像素；scale = dpi/96 用于前端把框选的逻辑坐标换算成物理坐标。
    // MVP 仅支持单显示器：x/y 固定 0（多屏 origin 需 GetMonitorInfoW 的 rcMonitor，未做）。
    let scale = dpi_scale(monitor);
    Ok(MonitorInfo {
        id: MonitorId::new(device_name),
        name: friendly,
        width,
        height,
        scale,
        x: 0,
        y: 0,
        is_primary,
    })
}

/// 取显示器 DPI scale（`dpi/96.0`）。查询失败回退 1.0（不阻断截图）。
///
/// 前端选区坐标换算依赖此值：物理像素 = 逻辑像素 × scale。
/// 高 DPI（如 150% 缩放）下 scale=1.5，错值会导致框选区域与实际裁剪错位。
fn dpi_scale(monitor: &Monitor) -> f32 {
    use windows::Win32::Graphics::Gdi::HMONITOR;
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MONITOR_DPI_TYPE};

    // windows-capture 的 as_raw_hmonitor 返回 *mut c_void（即 HMONITOR 句柄）。
    let hmon = HMONITOR(monitor.as_raw_hmonitor());
    let (mut dpi_x, mut _dpi_y) = (0u32, 0u32);
    let ok = unsafe { GetDpiForMonitor(hmon, MONITOR_DPI_TYPE(0), &mut dpi_x, &mut _dpi_y).is_ok() };
    if ok && dpi_x > 0 {
        dpi_x as f32 / 96.0
    } else {
        // 兜底：GetDpiForMonitor 极少失败（仅句柄无效），失败时按 100% 处理。
        1.0
    }
}

fn monitor_err(e: MonitorError) -> CaptureError {
    CaptureError::BackendUnavailable(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra8_swaps_to_rgba() {
        // 输入 BGRA = [10, 20, 30, 40] → 期望 RGBA = [30, 20, 10, 40]
        let img = pixels_to_rgba(vec![10, 20, 30, 40], 1, 1, DxgiDuplicationFormat::Bgra8).unwrap();
        assert_eq!(img.get_pixel(0, 0).0, [30, 20, 10, 40]);
    }

    #[test]
    fn rgba8_passes_through() {
        let img = pixels_to_rgba(vec![1, 2, 3, 4], 1, 1, DxgiDuplicationFormat::Rgba8).unwrap();
        assert_eq!(img.get_pixel(0, 0).0, [1, 2, 3, 4]);
    }

    #[test]
    fn unsupported_format_rejected() {
        assert!(pixels_to_rgba(vec![0; 8], 1, 1, DxgiDuplicationFormat::Rgba16F).is_err());
    }

    #[tokio::test]
    #[ignore = "需要真实桌面会话（WGC 可能首启触发权限提示）"]
    async fn list_monitors_real() {
        let provider = WindowsCaptureProvider::new();
        let monitors = provider.list_monitors().await.unwrap();
        assert!(!monitors.is_empty());
        for m in &monitors {
            println!("{m:?}");
        }
    }

    #[tokio::test]
    #[ignore = "需要真实桌面会话"]
    async fn capture_all_real() {
        let provider = WindowsCaptureProvider::new();
        let frames = provider.capture_all().await.unwrap();
        assert!(!frames.is_empty());
        for f in &frames {
            assert!(f.image.width() > 0);
            assert!(f.image.height() > 0);
        }
    }

    #[tokio::test]
    #[ignore = "需要真实桌面会话（会写一张 PNG 用于肉眼校验截图非空）"]
    async fn capture_save_png_real() {
        let provider = WindowsCaptureProvider::new();
        let frames = provider.capture_all().await.unwrap();
        let f = &frames[0];
        // 统计非透明非黑像素，粗略证明截图有真实内容。
        let total = (f.image.width() * f.image.height()) as usize;
        let nonblank = f
            .image
            .pixels()
            .filter(|p| !(p.0[0] < 5 && p.0[1] < 5 && p.0[2] < 5))
            .count();
        let path = dirs::config_dir()
            .unwrap()
            .join("SnapText")
            .join("test_capture.png");
        f.image.save(&path).unwrap();
        println!(
            "{}x{} nonblank={}/{} ({:.1}%) -> {}",
            f.image.width(),
            f.image.height(),
            nonblank,
            total,
            100.0 * nonblank as f32 / total as f32,
            path.display()
        );
        assert!(nonblank * 100 > total, "截图疑似全黑，非空像素占比过低");
    }
}
