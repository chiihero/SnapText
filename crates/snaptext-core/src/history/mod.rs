//! 历史记录模块：[`HistoryStore`] trait + sqlite 实现。
//!
//! P0 阶段（DU-06）仅实现 `insert`（Orchestrator 翻译后写入）。
//! `list` / `delete_before` / `stats` 返回 `CoreError::NotImplemented`，DU-15 补齐。

pub mod dao;
pub mod migration;
pub mod sqlite_store;

pub use sqlite_store::SqliteHistoryStore;

use std::time::SystemTime;

use async_trait::async_trait;

use crate::error::CoreError;
use crate::types::{Bbox, Lang, ProviderId};

/// 一条翻译历史记录（对应 `translation_history` 表，见 DESIGN §5.6）。
#[derive(Debug, Clone)]
pub struct HistoryRecord {
    pub created_at: SystemTime,
    pub source_lang: Lang,
    pub target_lang: Lang,
    pub original_text: String,
    pub translated_text: String,
    pub provider: ProviderId,
    pub model: Option<String>,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    /// 估算费用（人民币毫，即 0.001 元）。
    pub total_cost_cny_milli: Option<i64>,
    pub monitor_id: Option<String>,
    pub bbox: Option<Bbox>,
    pub notes: Option<String>,
}

/// 历史统计（DU-15 实现）。
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct HistoryStats {
    pub total_records: u64,
}

/// 历史存储抽象。
#[async_trait]
pub trait HistoryStore: Send + Sync {
    async fn insert(&self, record: HistoryRecord) -> Result<(), CoreError>;
    async fn list(&self, limit: u32) -> Result<Vec<HistoryRecord>, CoreError>;
    async fn delete_before(&self, before: SystemTime) -> Result<u64, CoreError>;
    async fn stats(&self) -> Result<HistoryStats, CoreError>;
}
