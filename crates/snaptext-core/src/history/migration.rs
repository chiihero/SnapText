//! 迁移机制：`PRAGMA user_version` 版本化。

use rusqlite::Connection;

use crate::error::HistoryError;

/// V001 初始迁移 SQL（嵌入二进制）。
const V001: &str = include_str!("migrations/V001__initial.sql");

/// 当前目标版本。
const TARGET_VERSION: u32 = 1;

/// 按顺序执行未应用的迁移。
pub fn run_migrations(conn: &Connection) -> Result<(), HistoryError> {
    let current: u32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| HistoryError::Db(e.to_string()))?;

    if current < 1 {
        conn.execute_batch(V001)
            .map_err(|e| HistoryError::Db(e.to_string()))?;
    }

    if current < TARGET_VERSION {
        conn.execute_batch(&format!("PRAGMA user_version = {TARGET_VERSION}"))
            .map_err(|e| HistoryError::Db(e.to_string()))?;
    }
    Ok(())
}
