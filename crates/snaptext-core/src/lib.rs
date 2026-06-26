//! `snaptext-core`：纯逻辑层，包含所有共享类型、错误与配置。
//!
//! 不依赖任何 UI 框架，可被 `snaptext-app` 及集成测试独立使用。
//! 后续 DU 将在此追加 `capture` / `ocr` / `translate` / `history` / `model_manager` 模块。
//!
//! 架构铁律：本 crate 不得依赖 `snaptext-app`，也不得引入 UI 依赖。

pub mod capture;
pub mod config;
pub mod error;
pub mod history;
pub mod model_manager;
pub mod ocr;
pub mod translate;
pub mod types;

pub use config::Config;
pub use error::CoreError;
