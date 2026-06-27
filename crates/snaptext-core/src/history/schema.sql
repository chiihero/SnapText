-- 翻译历史记录表（DESIGN §5.6）。
-- 此文件为表结构权威定义；迁移机制实际执行版本在 migrations/ 下。
-- V001 建表；V002 增加 screenshot_png / ocr_lines_json / line_translations_json。
CREATE TABLE IF NOT EXISTS translation_history (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at        TEXT NOT NULL,
    source_lang       TEXT NOT NULL,
    target_lang       TEXT NOT NULL,
    original_text     TEXT NOT NULL,
    translated_text   TEXT NOT NULL,
    provider          TEXT NOT NULL,
    model             TEXT,
    prompt_tokens     INTEGER,
    completion_tokens INTEGER,
    total_cost_cny_milli INTEGER,
    monitor_id        TEXT,
    bbox_x            INTEGER,
    bbox_y            INTEGER,
    bbox_w            INTEGER,
    bbox_h            INTEGER,
    notes             TEXT,
    -- V002 字段（迁移新增，历史表权威定义一并列出）。
    screenshot_png          BLOB,   -- 选区截图（PNG 压缩）
    ocr_lines_json          TEXT,   -- Vec<OcrLine> 的 JSON（text+bbox+confidence+direction）
    line_translations_json  TEXT    -- Vec<String> 逐行译文的 JSON
);

CREATE INDEX IF NOT EXISTS idx_history_created ON translation_history(created_at DESC);
