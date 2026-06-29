//! 历史命令：列表/搜索/详情截图/删除/清空/统计。
//!
//! HistoryRecord 含 screenshot_png（Vec<u8>，不可直接序列化给前端），故 list/search
//! 返回剥离二进制的 HistoryDto；详情截图单独用 history_get_screenshot 取（base64 data URL）。

use serde::Serialize;
use snaptext_core::history::HistoryRecord;
use tauri::State;

use crate::state::AppState;

/// 前端友好的历史记录（剥离二进制）。
#[derive(Serialize)]
pub struct HistoryDto {
    pub id: i64,
    /// 创建时间（Unix 毫秒）。
    pub created_at_ms: u64,
    pub source_lang: String,
    pub target_lang: String,
    pub original_text: String,
    pub translated_text: String,
    pub provider: String,
    pub model: Option<String>,
    pub monitor_id: Option<String>,
    pub bbox: Option<snaptext_core::types::Bbox>,
    pub has_screenshot: bool,
    /// OCR 行（若有），供历史回看重现图上覆盖。
    pub ocr_lines: Option<Vec<snaptext_core::types::OcrLine>>,
    pub line_translations: Option<Vec<String>>,
}

pub(crate) fn to_dto(r: HistoryRecord) -> HistoryDto {
    let created_at_ms = r
        .created_at
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let has_screenshot = r.screenshot_png.is_some();
    HistoryDto {
        id: r.id,
        created_at_ms,
        source_lang: r.source_lang.to_string(),
        target_lang: r.target_lang.to_string(),
        original_text: r.original_text,
        translated_text: r.translated_text,
        provider: r.provider.to_string(),
        model: r.model,
        monitor_id: r.monitor_id,
        bbox: r.bbox,
        has_screenshot,
        ocr_lines: r.ocr_lines,
        line_translations: r.line_translations,
    }
}

#[tauri::command]
pub async fn history_list(state: State<'_, AppState>, limit: u32) -> Result<Vec<HistoryDto>, String> {
    state
        .history
        .list(limit)
        .await
        .map(|rs| rs.into_iter().map(to_dto).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn history_search(
    state: State<'_, AppState>,
    limit: u32,
    keyword: String,
) -> Result<Vec<HistoryDto>, String> {
    state
        .history
        .search(limit, &keyword)
        .await
        .map(|rs| rs.into_iter().map(to_dto).collect())
        .map_err(|e| e.to_string())
}

/// 取单条记录的截图（base64 data URL，前端 <img> 直接用）。
///
/// 按主键精确查单列 `screenshot_png`，不依赖 list（旧实现 list(10000) 全表拉 BLOB，
/// 记录超 1 万条会丢图且每次点选全表读 BLOB）。
#[tauri::command]
pub async fn history_get_screenshot(
    state: State<'_, AppState>,
    id: i64,
) -> Result<Option<String>, String> {
    let png = state
        .history
        .get_screenshot(id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(png.map(|bytes| {
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
        format!("data:image/png;base64,{b64}")
    }))
}

#[tauri::command]
pub async fn history_delete(state: State<'_, AppState>, id: i64) -> Result<bool, String> {
    state.history.delete_by_id(id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn history_clear(state: State<'_, AppState>) -> Result<u64, String> {
    state.history.clear_all().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn history_stats(state: State<'_, AppState>) -> Result<u64, String> {
    state
        .history
        .stats()
        .await
        .map(|s| s.total_records)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    //! history 命令层纯函数测试：HistoryRecord → HistoryDto 转换（含截图标志、时间戳）。
    use super::*;
    use snaptext_core::types::{Bbox, Lang, ProviderId};

    fn sample_record(id: i64, with_png: bool) -> HistoryRecord {
        HistoryRecord {
            id,
            created_at: std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
            source_lang: Lang::En,
            target_lang: Lang::Zh,
            original_text: "Hello".into(),
            translated_text: "你好".into(),
            provider: ProviderId::new_static("deepseek"),
            model: Some("deepseek-v4-flash".into()),
            prompt_tokens: Some(10),
            completion_tokens: Some(5),
            total_cost_cny_milli: None,
            monitor_id: Some("D1".into()),
            bbox: Some(Bbox { x: 1, y: 2, w: 3, h: 4 }),
            notes: None,
            screenshot_png: if with_png { Some(vec![1, 2, 3]) } else { None },
            ocr_lines: None,
            line_translations: None,
        }
    }

    #[test]
    fn to_dto_strips_screenshot_bytes() {
        // 有截图：has_screenshot=true，DTO 不含二进制字段（HistoryDto 无 screenshot_png）。
        let dto = to_dto(sample_record(1, true));
        assert!(dto.has_screenshot);
        assert_eq!(dto.id, 1);
        assert_eq!(dto.original_text, "Hello");
        assert_eq!(dto.translated_text, "你好");
    }

    #[test]
    fn to_dto_no_screenshot_flag_false() {
        // 无截图：has_screenshot=false。
        let dto = to_dto(sample_record(2, false));
        assert!(!dto.has_screenshot);
    }

    #[test]
    fn to_dto_timestamp_millis() {
        // created_at → 毫秒时间戳（1700000000s = 1700000000000ms）。
        let dto = to_dto(sample_record(3, false));
        assert_eq!(dto.created_at_ms, 1_700_000_000_000);
    }

    #[test]
    fn to_dto_preserves_bbox_and_lang() {
        let dto = to_dto(sample_record(4, false));
        assert_eq!(dto.bbox, Some(Bbox { x: 1, y: 2, w: 3, h: 4 }));
        assert_eq!(dto.source_lang, "en");
        assert_eq!(dto.target_lang, "zh");
        assert_eq!(dto.provider, "deepseek");
    }
}
