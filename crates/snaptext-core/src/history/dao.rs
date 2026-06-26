//! CRUD 操作（P0 仅 `insert`，DU-15 补 `list` / `delete_before` / `stats`）。

use rusqlite::{params, Connection, Row};
use std::time::SystemTime;

use crate::error::HistoryError;
use crate::history::HistoryRecord;
use crate::types::{Bbox, Lang, ProviderId};

/// rusqlite 错误 → HistoryError（供 dao 内 `?` 自动转换）。
impl From<rusqlite::Error> for HistoryError {
    fn from(e: rusqlite::Error) -> Self {
        HistoryError::Db(e.to_string())
    }
}

/// 插入一条历史记录（同步，由 `SqliteHistoryStore` 在 `spawn_blocking` 中调用）。
pub fn insert(conn: &Connection, record: &HistoryRecord) -> Result<(), HistoryError> {
    let created_at = chrono::DateTime::<chrono::Utc>::from(record.created_at).to_rfc3339();
    let (bx, by, bw, bh) = record
        .bbox
        .map(|b| (Some(b.x), Some(b.y), Some(b.w), Some(b.h)))
        .unwrap_or((None, None, None, None));

    conn.execute(
        "INSERT INTO translation_history
         (created_at, source_lang, target_lang, original_text, translated_text, provider, model,
          prompt_tokens, completion_tokens, total_cost_cny_milli, monitor_id, bbox_x, bbox_y, bbox_w, bbox_h, notes)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
        params![
            created_at,
            record.source_lang.to_string(),
            record.target_lang.to_string(),
            record.original_text,
            record.translated_text,
            record.provider.to_string(),
            record.model,
            record.prompt_tokens.map(|v| v as i64),
            record.completion_tokens.map(|v| v as i64),
            record.total_cost_cny_milli,
            record.monitor_id,
            bx,
            by,
            bw,
            bh,
            record.notes,
        ],
    )
    .map_err(|e| HistoryError::Db(e.to_string()))?;
    Ok(())
}

/// 清理过期 / 超量记录（由 Orchestrator 启动时调用）。
///
/// - `retention_days`：删除创建时间早于该天数的记录（0 表示不按时间删）。
/// - `max_records`：仅保留最新的 N 条（0 表示不按数量删）。
///
/// 返回删除行数。
pub fn cleanup(
    conn: &Connection,
    retention_days: u32,
    max_records: u32,
) -> Result<u64, HistoryError> {
    let mut deleted = 0u64;
    if retention_days > 0 {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days as i64);
        let n = conn
            .execute(
                "DELETE FROM translation_history WHERE created_at < ?1",
                params![cutoff.to_rfc3339()],
            )
            .map_err(|e| HistoryError::Db(e.to_string()))?;
        deleted += n as u64;
    }
    if max_records > 0 {
        let n = conn
            .execute(
                "DELETE FROM translation_history WHERE id NOT IN (
                    SELECT id FROM translation_history ORDER BY id DESC LIMIT ?1
                )",
                params![max_records as i64],
            )
            .map_err(|e| HistoryError::Db(e.to_string()))?;
        deleted += n as u64;
    }
    Ok(deleted)
}

const SELECT_COLS: &str = "id, created_at, source_lang, target_lang, original_text, translated_text, provider, model, prompt_tokens, completion_tokens, total_cost_cny_milli, monitor_id, bbox_x, bbox_y, bbox_w, bbox_h, notes";

/// 列出历史记录（按 id 倒序）。可选关键词搜索（原文 + 译文 LIKE）。
pub fn list(
    conn: &Connection,
    limit: u32,
    search: Option<&str>,
) -> Result<Vec<HistoryRecord>, HistoryError> {
    let limit = limit as i64;
    let search = search.filter(|s| !s.is_empty());
    let sql = if search.is_some() {
        format!("SELECT {SELECT_COLS} FROM translation_history WHERE original_text LIKE ?1 OR translated_text LIKE ?1 ORDER BY id DESC LIMIT ?2")
    } else {
        format!("SELECT {SELECT_COLS} FROM translation_history ORDER BY id DESC LIMIT ?1")
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = if let Some(s) = search {
        stmt.query_map(params![format!("%{s}%"), limit], row_to_record)?
    } else {
        stmt.query_map(params![limit], row_to_record)?
    };
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(HistoryError::from)
}

/// 删除创建时间早于 `before` 的记录，返回删除行数。
pub fn delete_before(conn: &Connection, before: SystemTime) -> Result<u64, HistoryError> {
    let cutoff = chrono::DateTime::<chrono::Utc>::from(before).to_rfc3339();
    let n = conn
        .execute(
            "DELETE FROM translation_history WHERE created_at < ?1",
            params![cutoff],
        )
        .map_err(|e| HistoryError::Db(e.to_string()))?;
    Ok(n as u64)
}

/// 统计记录总数。
pub fn stats(conn: &Connection) -> Result<u64, HistoryError> {
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM translation_history", [], |r| r.get(0))
        .map_err(|e| HistoryError::Db(e.to_string()))?;
    Ok(total as u64)
}

/// 行 → HistoryRecord（列顺序同 SELECT_COLS）。
fn row_to_record(row: &Row) -> rusqlite::Result<HistoryRecord> {
    let created_str: String = row.get(1)?;
    let created_at = chrono::DateTime::parse_from_rfc3339(&created_str)
        .map(|dt| dt.with_timezone(&chrono::Utc).into())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let source_lang: String = row.get(2)?;
    let target_lang: String = row.get(3)?;
    let bx: Option<i32> = row.get(12)?;
    let by: Option<i32> = row.get(13)?;
    let bw: Option<i32> = row.get(14)?;
    let bh: Option<i32> = row.get(15)?;
    let bbox = match (bx, by, bw, bh) {
        (Some(x), Some(y), Some(w), Some(h)) => Some(Bbox { x, y, w, h }),
        _ => None,
    };
    Ok(HistoryRecord {
        created_at,
        source_lang: source_lang.parse().unwrap_or(Lang::Auto),
        target_lang: target_lang.parse().unwrap_or(Lang::Auto),
        original_text: row.get(4)?,
        translated_text: row.get(5)?,
        provider: ProviderId::from(row.get::<_, String>(6)?),
        model: row.get(7)?,
        prompt_tokens: row.get::<_, Option<i64>>(8)?.map(|v| v as u64),
        completion_tokens: row.get::<_, Option<i64>>(9)?.map(|v| v as u64),
        total_cost_cny_milli: row.get(10)?,
        monitor_id: row.get(11)?,
        bbox,
        notes: row.get(16)?,
    })
}
