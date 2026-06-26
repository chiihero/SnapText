-- 翻译历史记录表（DESIGN §5.6）。
-- 此文件与 migrations/V001__initial.sql 内容一致；后者为迁移机制实际执行版本。
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
    notes             TEXT
);

CREATE INDEX IF NOT EXISTS idx_history_created ON translation_history(created_at DESC);
