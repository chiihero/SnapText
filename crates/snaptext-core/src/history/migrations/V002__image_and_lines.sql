-- V002：历史记录增加截图 + OCR 行 + 逐行译文（DESIGN §5.6）。
-- 配合译文图上原位覆盖（result_overlay）与历史面板 GUI。
ALTER TABLE translation_history ADD COLUMN screenshot_png BLOB;
ALTER TABLE translation_history ADD COLUMN ocr_lines_json TEXT;
ALTER TABLE translation_history ADD COLUMN line_translations_json TEXT;
