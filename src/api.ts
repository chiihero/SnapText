// Tauri 命令的 TypeScript 封装：每个 invoke 配对应 DTO 类型，与 Rust 端签名对齐。
// 前端所有后端调用都走这里，便于统一类型检查与错误处理。

import { invoke } from "@tauri-apps/api/core";
import { convertFileSrc } from "@tauri-apps/api/core";

// ===== 共享类型（与 snaptext_core::types 对齐）=====

export interface Bbox {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface OcrLine {
  text: string;
  bbox: Bbox;
  confidence: number;
  writing_direction: "Horizontal" | "Vertical";
}

export type Tier = "Medium" | "Small";
export type Lang = "en" | "zh" | "ja" | "auto";
export type ProviderKind = "DeepSeek" | "DeepL" | "Microsoft";

// ===== Config（与 snaptext_core::config::Config 对齐）=====

export interface Config {
  general: { log_level: string; log_file: string | null; onboarding_completed: boolean };
  hotkey: { trigger: string; cancel: string };
  capture: { fallback_to_dxgi: boolean };
  ocr: { tier: Tier; postprocess: boolean };
  translate: TranslateConfig;
  history: { retention_days: number; max_records: number; auto_clean_on_start: boolean };
  ui: {
    auto_copy_translation: boolean;
    show_original: boolean;
    overlay_dim_alpha: number;
    card_font_size: number;
    minimize_to_tray_on_close: boolean;
  };
}

export interface TranslateConfig {
  provider: ProviderKind;
  target_lang: Lang;
  deepseek: { base_url: string; model: string; api_key: string | null };
  deepl: { plan: "Free" | "Pro"; api_key: string | null };
  microsoft: { endpoint: string; region: string; api_key: string | null };
  timeout_llm_secs: number;
  timeout_mt_secs: number;
  max_retries: number;
  postprocess: boolean;
  fallback_order: ProviderKind[];
}

// ===== 命令 DTO（与 src-tauri 的命令返回类型对齐）=====

export interface MonitorDto {
  id: string;
  name: string;
  width: number;
  height: number;
  scale: number;
  x: number;
  y: number;
  primary: boolean;
  shot_path: string;
}

export interface SelectResult {
  shot_path: string;
  ocr_lines: OcrLine[];
  translations: string[];
  original: string;
  translated: string;
  provider: string;
  elapsed_ms: number;
}

export interface HistoryDto {
  id: number;
  created_at_ms: number;
  source_lang: string;
  target_lang: string;
  original_text: string;
  translated_text: string;
  provider: string;
  model: string | null;
  monitor_id: string | null;
  bbox: Bbox | null;
  has_screenshot: boolean;
  ocr_lines: OcrLine[] | null;
  line_translations: string[] | null;
}

// ===== 命令封装 =====

export const api = {
  // 配置
  getConfig: () => invoke<Config>("get_config"),
  saveConfig: (cfg: Config) => invoke<boolean>("save_config", { cfg }),
  checkTranslateReady: () => invoke<boolean>("check_translate_ready"),

  // 模型
  modelsReady: (tier: Tier) => invoke<boolean>("models_ready", { tier }),
  downloadModels: (tier: Tier) => invoke<void>("download_models", { tier }),

  // 截图 + 选区
  captureAll: () => invoke<MonitorDto[]>("capture_all"),
  getLastCapture: () => invoke<MonitorDto[]>("get_last_capture"),
  triggerCapture: () => invoke<void>("trigger_capture_cmd"),
  selectRegion: (monitorId: string, bbox: Bbox) =>
    invoke<SelectResult>("select_region", { monitorId, bbox }),
  saveImageCopy: (sourcePath: string, destPath: string) =>
    invoke<void>("save_image_copy", { sourcePath, destPath }),
  logDiag: (tag: string, message: string) => invoke<void>("log_diag", { tag, message }),
  checkFile: (path: string) => invoke<string>("check_file", { path }),

  // 历史
  historyList: (limit: number) => invoke<HistoryDto[]>("history_list", { limit }),
  historySearch: (limit: number, keyword: string) =>
    invoke<HistoryDto[]>("history_search", { limit, keyword }),
  historyGetScreenshot: (id: number) => invoke<string | null>("history_get_screenshot", { id }),
  historyDelete: (id: number) => invoke<boolean>("history_delete", { id }),
  historyClear: () => invoke<number>("history_clear"),
  historyStats: () => invoke<number>("history_stats"),

  // 工具：本地路径转 webview URL
  fileSrc: (path: string) => convertFileSrc(path),
};
