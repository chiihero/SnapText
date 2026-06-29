//! `SqliteHistoryStore`：r2d2 连接池 + sqlite 实现。

use std::path::PathBuf;
use std::time::SystemTime;

use async_trait::async_trait;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

use crate::error::{CoreError, HistoryError};
use crate::history::{dao, migration, HistoryRecord, HistoryStats, HistoryStore};

/// sqlite 历史存储。`Pool` 内部 `Arc`，可廉价 clone（但通常单实例 + Arc 共享）。
pub struct SqliteHistoryStore {
    pool: Pool<SqliteConnectionManager>,
}

impl SqliteHistoryStore {
    /// 打开指定路径的数据库（不存在则创建），运行迁移。
    pub fn open(path: PathBuf) -> Result<Self, HistoryError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| HistoryError::Db(e.to_string()))?;
        }
        let manager = SqliteConnectionManager::file(path);
        let pool = Pool::builder()
            .max_size(5)
            .build(manager)
            .map_err(|e| HistoryError::Pool(e.to_string()))?;
        {
            let conn = pool.get().map_err(|e| HistoryError::Pool(e.to_string()))?;
            migration::run_migrations(&conn)?;
        }
        Ok(Self { pool })
    }

    /// 默认路径：`%APPDATA%\SnapText\history.db`。
    pub fn open_default() -> Result<Self, HistoryError> {
        let path = dirs::config_dir()
            .ok_or_else(|| HistoryError::Db("无法定位用户配置目录".into()))?
            .join("SnapText")
            .join("history.db");
        Self::open(path)
    }

    /// 同步清理过期 / 超量记录（Orchestrator 启动时在 `spawn_blocking` 调用）。
    pub fn cleanup_blocking(
        &self,
        retention_days: u32,
        max_records: u32,
    ) -> Result<u64, HistoryError> {
        let conn = self
            .pool
            .get()
            .map_err(|e| HistoryError::Pool(e.to_string()))?;
        dao::cleanup(&conn, retention_days, max_records)
    }
}

#[async_trait]
impl HistoryStore for SqliteHistoryStore {
    async fn insert(&self, record: HistoryRecord) -> Result<(), CoreError> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || -> Result<(), HistoryError> {
            let conn = pool.get().map_err(|e| HistoryError::Pool(e.to_string()))?;
            dao::insert(&conn, &record)
        })
        .await
        .map_err(|e| CoreError::History(HistoryError::Db(format!("历史记录线程异常：{e}"))))?
        .map_err(CoreError::History)
    }

    async fn list(&self, limit: u32) -> Result<Vec<HistoryRecord>, CoreError> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<HistoryRecord>, HistoryError> {
            let conn = pool.get().map_err(|e| HistoryError::Pool(e.to_string()))?;
            dao::list(&conn, limit, None)
        })
        .await
        .map_err(|e| CoreError::History(HistoryError::Db(format!("历史读取线程异常：{e}"))))?
        .map_err(CoreError::History)
    }

    async fn search(&self, limit: u32, keyword: &str) -> Result<Vec<HistoryRecord>, CoreError> {
        let pool = self.pool.clone();
        let keyword = keyword.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<HistoryRecord>, HistoryError> {
            let conn = pool.get().map_err(|e| HistoryError::Pool(e.to_string()))?;
            dao::list(&conn, limit, Some(&keyword))
        })
        .await
        .map_err(|e| CoreError::History(HistoryError::Db(format!("历史搜索线程异常：{e}"))))?
        .map_err(CoreError::History)
    }

    async fn delete_before(&self, before: SystemTime) -> Result<u64, CoreError> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || -> Result<u64, HistoryError> {
            let conn = pool.get().map_err(|e| HistoryError::Pool(e.to_string()))?;
            dao::delete_before(&conn, before)
        })
        .await
        .map_err(|e| CoreError::History(HistoryError::Db(format!("历史删除线程异常：{e}"))))?
        .map_err(CoreError::History)
    }

    async fn delete_by_id(&self, id: i64) -> Result<bool, CoreError> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || -> Result<bool, HistoryError> {
            let conn = pool.get().map_err(|e| HistoryError::Pool(e.to_string()))?;
            dao::delete_by_id(&conn, id)
        })
        .await
        .map_err(|e| CoreError::History(HistoryError::Db(format!("历史删除线程异常：{e}"))))?
        .map_err(CoreError::History)
    }

    async fn clear_all(&self) -> Result<u64, CoreError> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || -> Result<u64, HistoryError> {
            let conn = pool.get().map_err(|e| HistoryError::Pool(e.to_string()))?;
            dao::clear_all(&conn)
        })
        .await
        .map_err(|e| CoreError::History(HistoryError::Db(format!("历史清空线程异常：{e}"))))?
        .map_err(CoreError::History)
    }

    async fn stats(&self) -> Result<HistoryStats, CoreError> {
        let pool = self.pool.clone();
        let total = tokio::task::spawn_blocking(move || -> Result<u64, HistoryError> {
            let conn = pool.get().map_err(|e| HistoryError::Pool(e.to_string()))?;
            dao::stats(&conn)
        })
        .await
        .map_err(|e| CoreError::History(HistoryError::Db(format!("历史统计线程异常：{e}"))))?
        .map_err(CoreError::History)?;
        Ok(HistoryStats {
            total_records: total,
        })
    }

    async fn get_screenshot(&self, id: i64) -> Result<Option<Vec<u8>>, CoreError> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Vec<u8>>, HistoryError> {
            let conn = pool.get().map_err(|e| HistoryError::Pool(e.to_string()))?;
            dao::get_screenshot(&conn, id)
        })
        .await
        .map_err(|e| CoreError::History(HistoryError::Db(format!("历史截图读取线程异常：{e}"))))?
        .map_err(CoreError::History)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Lang, ProviderId};

    fn sample_record() -> HistoryRecord {
        HistoryRecord {
            id: 0,
            created_at: SystemTime::now(),
            source_lang: Lang::En,
            target_lang: Lang::Zh,
            original_text: "Hello".into(),
            translated_text: "你好".into(),
            provider: ProviderId::new_static("deepseek"),
            model: Some("deepseek-chat".into()),
            prompt_tokens: Some(10),
            completion_tokens: Some(5),
            total_cost_cny_milli: Some(1),
            monitor_id: None,
            bbox: None,
            notes: None,
            screenshot_png: None,
            ocr_lines: None,
            line_translations: None,
        }
    }

    #[tokio::test]
    async fn insert_writes_row() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SqliteHistoryStore::open(tmp.path().join("test.db")).unwrap();
        store.insert(sample_record()).await.unwrap();
        let conn = store.pool.get().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM translation_history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn list_and_stats_work() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SqliteHistoryStore::open(tmp.path().join("test.db")).unwrap();
        store.insert(sample_record()).await.unwrap();
        store.insert(sample_record()).await.unwrap();
        let listed = store.list(10).await.unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(store.stats().await.unwrap().total_records, 2);
        // 搜索
        let conn = store.pool.get().unwrap();
        let searched = dao::list(&conn, 10, Some("Hello")).unwrap();
        assert_eq!(searched.len(), 2);
    }

    #[tokio::test]
    async fn open_runs_migration_idempotently() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a.db");
        {
            let _ = SqliteHistoryStore::open(path.clone()).unwrap();
        }
        // 再次打开应不报错（迁移幂等）。
        let _ = SqliteHistoryStore::open(path).unwrap();
    }

    #[tokio::test]
    async fn cleanup_enforces_max_records() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SqliteHistoryStore::open(tmp.path().join("c.db")).unwrap();
        for _ in 0..3 {
            store.insert(sample_record()).await.unwrap();
        }
        // 仅保留最新 1 条，应删除 2 条。
        let deleted = store.cleanup_blocking(0, 1).unwrap();
        assert_eq!(deleted, 2);
        let conn = store.pool.get().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM translation_history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn get_screenshot_returns_png_for_existing_id() {
        // 取单条截图：不依赖 list 上限，按 id 精确查单列。
        let tmp = tempfile::tempdir().unwrap();
        let store = SqliteHistoryStore::open(tmp.path().join("d.db")).unwrap();
        let mut rec = sample_record();
        rec.screenshot_png = Some(vec![1, 2, 3, 4]);
        store.insert(rec).await.unwrap();
        // 取回最新一条的 id。
        let listed = store.list(1).await.unwrap();
        let id = listed[0].id;

        let png = store.get_screenshot(id).await.unwrap();
        assert_eq!(png, Some(vec![1, 2, 3, 4]));
    }

    #[tokio::test]
    async fn get_screenshot_none_for_missing_id() {
        // id 不存在：返回 None（不报错）。
        let tmp = tempfile::tempdir().unwrap();
        let store = SqliteHistoryStore::open(tmp.path().join("e.db")).unwrap();
        assert_eq!(store.get_screenshot(9999).await.unwrap(), None);
    }

    #[tokio::test]
    async fn get_screenshot_none_when_no_png_stored() {
        // 记录存在但无截图：返回 None。
        let tmp = tempfile::tempdir().unwrap();
        let store = SqliteHistoryStore::open(tmp.path().join("f.db")).unwrap();
        store.insert(sample_record()).await.unwrap();
        let id = store.list(1).await.unwrap()[0].id;
        assert_eq!(store.get_screenshot(id).await.unwrap(), None);
    }

    #[tokio::test]
    async fn list_reads_back_v002_fields_when_populated() {
        // 回归：ocr_lines_json / line_translations_json / screenshot_png 列索引必须正确。
        // 旧代码把 index 17(=screenshot_png BLOB) 当 ocr_lines_json(String) 读，
        // 任何带截图的记录 list 都会因 BLOB→String 类型不符而崩溃。
        let tmp = tempfile::tempdir().unwrap();
        let store = SqliteHistoryStore::open(tmp.path().join("v002.db")).unwrap();
        let mut rec = sample_record();
        rec.screenshot_png = Some(vec![9, 9, 9]);
        rec.ocr_lines = Some(vec![crate::types::OcrLine {
            text: "hi".into(),
            bbox: crate::types::Bbox { x: 1, y: 2, w: 3, h: 4 },
            confidence: 0.5,
            writing_direction: crate::types::WritingDirection::Horizontal,
        }]);
        rec.line_translations = Some(vec!["嗨".into()]);
        store.insert(rec).await.unwrap();

        let listed = store.list(1).await.unwrap();
        assert_eq!(listed.len(), 1);
        let r = &listed[0];
        assert_eq!(r.screenshot_png.as_deref(), Some(&[9, 9, 9][..]));
        assert_eq!(r.ocr_lines.as_ref().unwrap()[0].text, "hi");
        assert_eq!(r.line_translations.as_ref().unwrap()[0], "嗨");
    }
}
