//! Tauri 命令层：把 core 的 Provider 包装成前端可调用的 `#[tauri::command]`。
//!
//! 分文件：config_cmd（配置）、models（模型）、capture（截图）、
//! ocr_translate（OCR+翻译+配对）、history（历史 CRUD）。

pub mod capture;
pub mod config_cmd;
pub mod history;
pub mod models;
pub mod ocr_translate;
