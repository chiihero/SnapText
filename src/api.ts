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

// 与后端 serde 的 rename_all = "lowercase" 对齐（config.rs Tier / ProviderKind）
export type Tier = "medium" | "small";
export type Lang = "en" | "zh" | "ja" | "auto";
export type ProviderKind = "deepseek" | "deepl" | "microsoft";
// DeepSeek 思考强度（与后端 config.rs ReasoningEffort serde 小写对齐）。
export type ReasoningEffort = "high" | "max";

// ===== Config（与 snaptext_core::config::Config 对齐）=====

export interface Config {
  general: {
    log_level: string;
    log_file: string | null;
    /** 框选后是否自动 OCR；关闭则结果窗手动点"原文"触发。 */
    auto_ocr: boolean;
    /** OCR 完成后是否自动翻译；关闭则结果窗手动点"译文"触发。 */
    auto_translate: boolean;
    /** 首启引导是否完成。后端 GeneralConfig.onboarding_completed 对齐。 */
    onboarding_completed: boolean;
  };
  hotkey: { trigger: string };
  capture: Record<string, never>;
  ocr: { tier: Tier; postprocess: boolean };
  translate: TranslateConfig;
  history: { retention_days: number; max_records: number; auto_clean_on_start: boolean };
  ui: {
    auto_copy_translation: boolean;
    overlay_dim_alpha: number;
    card_font_size: number;
    minimize_to_tray_on_close: boolean;
  };
}

export interface TranslateConfig {
  provider: ProviderKind;
  target_lang: Lang;
  deepseek: {
    base_url: string;
    model: string;
    api_key: string | null;
    reasoning_enabled: boolean;
    reasoning_effort: ReasoningEffort;
  };
  deepl: { plan: "Free" | "Pro"; api_key: string | null };
  microsoft: { endpoint: string; region: string; api_key: string | null };
  timeout_llm_secs: number;
  timeout_mt_secs: number;
  max_retries: number;
  postprocess: boolean;
  /** LLM 翻译 prompt 模板，占位符 {{source}}/{{target}}/{{input}}。后端 TranslateConfig.prompt_template 对齐。 */
  prompt_template: string;
  /** 是否使用自定义 prompt。false=系统默认（渲染走后端常量）；true=用 prompt_template 字段。 */
  prompt_use_custom: boolean;
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

// 选区分阶段命令的 DTO（三层命令：crop → recognize → translate）。
export interface CropResult {
  shot_path: string;
}
export interface OcrResult {
  ocr_lines: OcrLine[];
  original: string;
}
export interface TranslateResult {
  translations: string[];
  translated: string;
  provider: string;
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
  // 系统默认翻译 prompt（设置页"系统默认"模式只读展示用，单一数据源取自后端常量）。
  getDefaultPrompt: () => invoke<string>("get_default_prompt"),
  // 全局热键注册状态：null=已注册；非空字符串=注册失败（被占用等），前端用于提示。
  getHotkeyStatus: () => invoke<string | null>("get_hotkey_status"),
  // 标记首启引导完成（置 onboarding_completed=true 并落盘）。
  completeOnboarding: () => invoke<void>("complete_onboarding"),
  // 重建 OCR Provider（模型下载完成后调用，即时生效无需重启）。
  reloadOcrProvider: () => invoke<void>("reload_ocr_provider"),
  // DeepSeek 模型列表（设置页填 key 后拉取，GET {base_url}/models）。
  listDeepseekModels: (baseUrl: string, apiKey: string) =>
    invoke<string[]>("list_deepseek_models", { baseUrl, apiKey }),

  // 模型
  modelsReady: (tier: Tier) => invoke<boolean>("models_ready", { tier }),
  downloadModels: (tier: Tier) => invoke<void>("download_models", { tier }),

  // 截图 + 选区（三层命令：框选抬起仅 crop 即开结果窗，OCR/翻译在结果窗内分阶段）
  captureAll: () => invoke<MonitorDto[]>("capture_all"),
  getLastCapture: () => invoke<MonitorDto[]>("get_last_capture"),
  triggerCapture: () => invoke<void>("trigger_capture_cmd"),
  cropRegion: (monitorId: string, bbox: Bbox) =>
    invoke<CropResult>("crop_region", { monitorId, bbox }),
  getLastCrop: () => invoke<CropResult>("get_last_crop"),
  recognizeRegion: () => invoke<OcrResult>("recognize_region"),
  translateRegion: () => invoke<TranslateResult>("translate_region"),
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
