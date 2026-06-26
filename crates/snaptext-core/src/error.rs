//! 统一错误类型。所有 Provider trait 方法返回 `Result<T, CoreError>`。
//!
//! 设计要点（见 PROGRESS 关键决策）：
//! - 本模块为叶节点，**不依赖** `types`，避免循环依赖（`types` 单向依赖本模块）。
//! - 涉及语言的错误用 `String`（语言的 Display），不直接持有 `Lang`。
//! - `CoreError::NotImplemented(&'static str)` 供 P0 阶段未实现的读取接口标记。

use std::path::PathBuf;

/// 核心错误，聚合各模块子错误。
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("截图失败：{0}")]
    Capture(#[from] CaptureError),
    #[error("OCR 失败：{0}")]
    Ocr(#[from] OcrError),
    #[error("翻译失败：{0}")]
    Translate(#[from] TranslateError),
    #[error("历史记录失败：{0}")]
    History(#[from] HistoryError),
    #[error("模型管理失败：{0}")]
    ModelManager(#[from] ModelManagerError),
    #[error("配置错误：{0}")]
    Config(#[from] ConfigError),
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
    /// P0 阶段尚未实现的接口（如 history 的 list/stats），DU-15 补齐。
    #[error("功能未实现：{0}")]
    NotImplemented(&'static str),
}

/// 截图模块错误。
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("找不到显示器：{0}")]
    MonitorNotFound(String),
    #[error("截图后端不可用：{0}")]
    BackendUnavailable(String),
    #[error("截图失败：{0}")]
    CaptureFailed(String),
}

/// OCR 模块错误。
#[derive(Debug, thiserror::Error)]
pub enum OcrError {
    #[error("模型文件未找到：{path}")]
    ModelNotFound { path: PathBuf },
    #[error("模型加载失败：{0}")]
    ModelLoad(String),
    #[error("推理失败：{0}")]
    Inference(String),
    #[error("图像解码失败：{0}")]
    Decode(String),
    #[error("不支持的 OCR 语言：{0}")]
    UnsupportedLanguage(String),
}

/// 翻译模块错误。
#[derive(Debug, thiserror::Error)]
pub enum TranslateError {
    #[error("翻译请求失败：{0}")]
    Request(String),
    #[error("翻译 API 返回错误（HTTP {status}）：{body}")]
    Api { status: u16, body: String },
    #[error("解析翻译响应失败：{0}")]
    Parse(String),
    // 注：字段名避开 thiserror 保留名 `source`（会被当作 error source）。
    #[error("不支持的翻译方向：{src} -> {dst}")]
    UnsupportedPair { src: String, dst: String },
    #[error("请求超时")]
    Timeout,
}

/// 历史记录模块错误。
///
/// 注：DU-01 阶段 `Db` 暂用 `String`，DU-06 引入 `rusqlite` 后改为 `#[from] rusqlite::Error`。
#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("数据库错误：{0}")]
    Db(String),
    #[error("连接池错误：{0}")]
    Pool(String),
}

/// 模型管理模块错误。
#[derive(Debug, thiserror::Error)]
pub enum ModelManagerError {
    #[error("模型文件未找到：{path}")]
    ModelNotFound { path: PathBuf },
    #[error("模型校验失败：{0}")]
    Checksum(String),
    #[error("模型下载失败：{0}")]
    Download(String),
}

/// 配置模块错误。
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("配置文件读取失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("配置文件解析失败：{0}")]
    Parse(#[from] toml::de::Error),
    #[error("配置项无效：{field}：{reason}")]
    Invalid { field: String, reason: String },
}
