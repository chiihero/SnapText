//! 截图模块：[`CaptureProvider`] trait 定义与 Windows 默认实现。

pub mod windows_capture;

pub use windows_capture::WindowsCaptureProvider;

use async_trait::async_trait;

use crate::error::CoreError;
use crate::types::{CapturedFrame, MonitorId, MonitorInfo};

/// 截图能力抽象。
///
/// 所有实现必须 `Send + Sync`，方法返回 [`CoreError`]。
/// CPU/GPU 密集的截图操作应在内部用 `spawn_blocking` 完成（见 CONVENTIONS §3）。
#[async_trait]
pub trait CaptureProvider: Send + Sync {
    /// 列出所有可用显示器。
    async fn list_monitors(&self) -> Result<Vec<MonitorInfo>, CoreError>;
    /// 捕获指定显示器的一帧。
    async fn capture_monitor(&self, id: &MonitorId) -> Result<CapturedFrame, CoreError>;
    /// 捕获所有显示器各一帧。
    ///
    /// 设计意图（DESIGN §5.1）：热键触发后立即对所有显示器各捕获一帧并缓存，
    /// 选区 Overlay 直接以该帧为背景，避免选区过程中屏幕内容变化。
    async fn capture_all(&self) -> Result<Vec<CapturedFrame>, CoreError>;
}
