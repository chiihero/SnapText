//! 用户配置（`%APPDATA%\SnapText\config.toml`）。
//!
//! 设计要点（见 PROGRESS 关键决策 / CONVENTIONS）：
//! - `Config` 用 `#[derive(Default)]`，各子结构手写 `impl Default` 提供自定义默认值。
//! - 所有字段 `#[serde(default)]`：缺失字段回退默认，保证旧配置可平滑升级。
//! - API key 支持 env 覆盖（`SNAPTEXT_DEEPSEEK_KEY` / `SNAPTEXT_DEEPL_KEY`）。
//! - env 覆盖测试合并为单函数，避免 `cargo test` 并行竞争环境变量。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{ConfigError, CoreError};
use crate::types::Lang;

/// 顶层配置，对应 `config.toml`，共 7 段。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub hotkey: HotkeyConfig,
    pub capture: CaptureConfig,
    pub ocr: OcrConfig,
    pub translate: TranslateConfig,
    pub history: HistoryConfig,
    pub ui: UiConfig,
}

impl Config {
    /// 从默认路径 `%APPDATA%\SnapText\config.toml` 加载。
    ///
    /// 文件不存在时返回默认配置（不视为错误），便于首次启动。
    pub fn load() -> Result<Self, CoreError> {
        load_inner().map_err(CoreError::from)
    }

    /// 写回默认路径（自动创建父目录）。
    pub fn save(&self) -> Result<(), CoreError> {
        save_inner(self).map_err(CoreError::from)
    }

    /// 用环境变量覆盖 API key（优先级：env > 文件）。
    pub fn apply_env_overrides(&mut self) {
        if let Ok(key) = std::env::var("SNAPTEXT_DEEPSEEK_KEY") {
            if !key.is_empty() {
                self.translate.deepseek.api_key = Some(key);
            }
        }
        if let Ok(key) = std::env::var("SNAPTEXT_DEEPL_KEY") {
            if !key.is_empty() {
                self.translate.deepl.api_key = Some(key);
            }
        }
    }
}

/// 从 TOML 文本解析配置，缺失字段使用默认值。
pub fn parse(content: &str) -> Result<Config, ConfigError> {
    let cfg: Config = toml::from_str(content)?;
    Ok(cfg)
}

fn load_inner() -> Result<Config, ConfigError> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Config::default());
    }
    let content = std::fs::read_to_string(&path)?;
    parse(&content)
}

fn save_inner(cfg: &Config) -> Result<(), ConfigError> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(cfg).map_err(|e| ConfigError::Invalid {
        field: "config".into(),
        reason: e.to_string(),
    })?;
    std::fs::write(&path, content)?;
    Ok(())
}

/// 配置文件路径：`%APPDATA%\SnapText\config.toml`。
pub fn config_path() -> Result<PathBuf, ConfigError> {
    Ok(dirs::config_dir()
        .ok_or_else(|| ConfigError::Invalid {
            field: "config_dir".into(),
            reason: "无法定位用户配置目录".into(),
        })?
        .join("SnapText")
        .join("config.toml"))
}

// ===== 各配置段 =====

/// 通用段：日志等。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    /// 日志级别（`error`/`warn`/`info`/`debug`/`trace`）。
    pub log_level: String,
    /// 自定义日志文件路径；`None` 用默认 `%APPDATA%\SnapText\logs\snaptext.log`。
    pub log_file: Option<String>,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            log_level: "info".to_string(),
            log_file: None,
        }
    }
}

/// 热键段。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeyConfig {
    /// 触发截图的热键。
    pub trigger: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            trigger: "Ctrl+Alt+Q".to_string(),
        }
    }
}

/// 截图段（当前无字段，保留结构供未来扩展；DXGI 回退为无条件默认行为，不暴露开关）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureConfig {}

/// OCR 档位（medium 精度优先 / small 速度优先）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    #[default]
    Medium,
    Small,
}

/// OCR 段。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OcrConfig {
    /// 模型档位。
    pub tier: Tier,
    /// 是否启用 OCR 输出后处理（DU-16）。
    pub postprocess: bool,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            tier: Tier::default(),
            postprocess: true,
        }
    }
}

/// 翻译 Provider 种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    DeepSeek,
    DeepL,
    Microsoft,
}

/// DeepSeek 思考强度（见 DESIGN §4.3「DeepSeek API 事实基准」）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    #[default]
    High,
    Max,
}

/// DeepSeek（OpenAI 兼容）配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DeepSeekConfig {
    /// OpenAI 兼容 base_url。
    pub base_url: String,
    /// 模型名。默认空——设置页填 Key 后调 `GET /v1/models` 动态拉取选择，也可手输。
    pub model: String,
    /// API key（可由 `SNAPTEXT_DEEPSEEK_KEY` 覆盖）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// 是否开启思考模式。翻译是简单请求，官方建议关闭以加速、降本，故默认 false。
    pub reasoning_enabled: bool,
    /// 思考强度（仅 reasoning_enabled=true 时生效）。
    pub reasoning_effort: ReasoningEffort,
}

impl Default for DeepSeekConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.deepseek.com/v1".to_string(),
            // 默认空：用户在设置页拉取/输入模型后再翻译（空 model 翻译时报错）。
            model: String::new(),
            api_key: None,
            reasoning_enabled: false,
            reasoning_effort: ReasoningEffort::default(),
        }
    }
}

/// DeepL 套餐。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeepLPlan {
    Free,
    Pro,
}

/// DeepL 配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DeepLConfig {
    /// Free 走 `api-free.deepl.com`，Pro 走 `api.deepl.com`。
    pub plan: DeepLPlan,
    /// API key（可由 `SNAPTEXT_DEEPL_KEY` 覆盖）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

impl Default for DeepLConfig {
    fn default() -> Self {
        Self {
            plan: DeepLPlan::Free,
            api_key: None,
        }
    }
}

/// Microsoft Azure Translator 配置（DU-18）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MicrosoftConfig {
    /// Azure Translator 端点，如 `https://api.cognitive.microsofttranslator.com`。
    pub endpoint: String,
    /// 区域，如 `eastasia`。
    pub region: String,
    /// Subscription Key。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

impl Default for MicrosoftConfig {
    fn default() -> Self {
        Self {
            endpoint: "https://api.cognitive.microsofttranslator.com".into(),
            region: String::new(),
            api_key: None,
        }
    }
}

/// 翻译段。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TranslateConfig {
    /// 当前 Provider。
    pub provider: ProviderKind,
    /// 翻译目标语言（源语言固定 `Auto`，由 OCR 检测）。
    pub target_lang: Lang,
    pub deepseek: DeepSeekConfig,
    pub deepl: DeepLConfig,
    pub microsoft: MicrosoftConfig,
    /// LLM 类 Provider 超时（秒）。
    pub timeout_llm_secs: u64,
    /// 专用 MT Provider 超时（秒）。
    pub timeout_mt_secs: u64,
    /// 失败重试次数（指数退避）。
    pub max_retries: u32,
    /// 是否启用译文后处理（DU-16）。
    pub postprocess: bool,
    /// LLM 翻译 prompt 模板（OpenAI 兼容 Provider 共用）。
    /// 占位符 `{{source}}`/`{{target}}`/`{{input}}`，渲染见 `translate::prompt`。
    /// 默认见 `translate::prompt::DEFAULT_PROMPT_TEMPLATE`（与设置页"恢复默认"同源）。
    ///
    /// 仅当 `prompt_use_custom = true` 时生效；为 false 时渲染走后端固定常量，
    /// 这样后端升级默认 prompt 能让所有默认模式用户自动受益（不读本字段）。
    pub prompt_template: String,
    /// 是否使用自定义 prompt 模板。false（默认）= 系统默认（只读展示，渲染走常量）；
    /// true = 用 `prompt_template` 字段渲染。
    pub prompt_use_custom: bool,
    /// 故障转移顺序（P2 DU-17）。
    pub fallback_order: Vec<ProviderKind>,
}

impl Default for TranslateConfig {
    fn default() -> Self {
        Self {
            provider: ProviderKind::DeepSeek,
            target_lang: Lang::Zh,
            deepseek: DeepSeekConfig::default(),
            deepl: DeepLConfig::default(),
            microsoft: MicrosoftConfig::default(),
            timeout_llm_secs: 30,
            timeout_mt_secs: 10,
            max_retries: 2,
            postprocess: true,
            prompt_template: crate::translate::prompt::DEFAULT_PROMPT_TEMPLATE.to_string(),
            prompt_use_custom: false,
            fallback_order: vec![ProviderKind::DeepSeek, ProviderKind::DeepL],
        }
    }
}

/// 历史记录段。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryConfig {
    /// 保留天数，超过自动清理。
    pub retention_days: u32,
    /// 最大记录数，超过自动清理最早的。
    pub max_records: u32,
    /// 启动时是否自动清理过期记录。
    pub auto_clean_on_start: bool,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            retention_days: 30,
            max_records: 5000,
            auto_clean_on_start: true,
        }
    }
}

/// UI 段。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// 翻译完成后是否自动复制译文到剪贴板。
    pub auto_copy_translation: bool,
    /// 选区蒙版不透明度（0.0 ~ 1.0）。
    pub overlay_dim_alpha: f32,
    /// 悬浮卡片字体大小（pt）。
    pub card_font_size: f32,
    /// 关闭主窗口时是否最小化到托盘（true）而非退出程序（false）。
    pub minimize_to_tray_on_close: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            auto_copy_translation: true,
            overlay_dim_alpha: 0.5,
            card_font_size: 14.0,
            minimize_to_tray_on_close: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_roundtrip() {
        let cfg = Config::default();
        let s = toml::to_string(&cfg).unwrap();
        let back = parse(&s).unwrap();
        assert_eq!(cfg.hotkey.trigger, back.hotkey.trigger);
        assert_eq!(cfg.ocr.tier, back.ocr.tier);
        assert_eq!(cfg.translate.provider, back.translate.provider);
        assert_eq!(cfg.ui.overlay_dim_alpha, back.ui.overlay_dim_alpha);
    }

    #[test]
    fn parse_uses_defaults_for_missing_sections() {
        // 空 TOML → 全部默认值（#[serde(default)] 生效）。
        let cfg = parse("").unwrap();
        assert_eq!(cfg.hotkey.trigger, "Ctrl+Alt+Q");
        assert_eq!(cfg.ocr.tier, Tier::Medium);
        assert_eq!(cfg.translate.provider, ProviderKind::DeepSeek);
        assert_eq!(cfg.translate.target_lang, Lang::Zh);
        assert_eq!(cfg.translate.timeout_llm_secs, 30);
        assert_eq!(cfg.ui.overlay_dim_alpha, 0.5);
        assert!(cfg.history.auto_clean_on_start);
    }

    #[test]
    fn parse_partial_overrides() {
        let toml = r#"
[ocr]
tier = "small"
postprocess = false
[hotkey]
trigger = "Ctrl+Alt+E"
"#;
        let cfg = parse(toml).unwrap();
        assert_eq!(cfg.ocr.tier, Tier::Small);
        assert!(!cfg.ocr.postprocess);
        assert_eq!(cfg.hotkey.trigger, "Ctrl+Alt+E");
        // 未覆盖项仍为默认
        assert_eq!(cfg.translate.provider, ProviderKind::DeepSeek);
    }

    #[test]
    fn tier_serde_lowercase() {
        assert_eq!(serde_json::to_string(&Tier::Medium).unwrap(), "\"medium\"");
        let small: Tier = serde_json::from_str("\"small\"").unwrap();
        assert_eq!(small, Tier::Small);
    }

    #[test]
    fn deepseek_defaults() {
        // 默认：model 空（设置页拉取）、思考关、强度 high。
        let d = DeepSeekConfig::default();
        assert_eq!(d.model, "");
        assert!(!d.reasoning_enabled);
        assert_eq!(d.reasoning_effort, ReasoningEffort::High);
    }

    #[test]
    fn reasoning_effort_serde_lowercase() {
        // effort serde 小写往返（与前端 TS 字面对齐）。
        assert_eq!(
            serde_json::to_string(&ReasoningEffort::High).unwrap(),
            "\"high\""
        );
        let max: ReasoningEffort = serde_json::from_str("\"max\"").unwrap();
        assert_eq!(max, ReasoningEffort::Max);
    }

    #[test]
    fn deepseek_config_serde_defaults_for_new_fields() {
        // 旧配置（无 reasoning 字段）反序列化时，新字段取默认值（#[serde(default)]）。
        let json = r#"{ "base_url":"x","model":"m","api_key":null }"#;
        let d: DeepSeekConfig = serde_json::from_str(json).unwrap();
        assert!(!d.reasoning_enabled);
        assert_eq!(d.reasoning_effort, ReasoningEffort::High);
    }

    #[test]
    fn env_overrides_merged_in_one_fn_to_avoid_parallel_race() {
        // 关键决策：env 相关断言合并到单个测试函数，避免 cargo test 并行竞争环境变量。
        std::env::set_var("SNAPTEXT_DEEPSEEK_KEY", "env-deepseek-key");
        std::env::set_var("SNAPTEXT_DEEPL_KEY", "env-deepl-key");

        let mut cfg = Config::default();
        cfg.apply_env_overrides();
        assert_eq!(
            cfg.translate.deepseek.api_key.as_deref(),
            Some("env-deepseek-key")
        );
        assert_eq!(
            cfg.translate.deepl.api_key.as_deref(),
            Some("env-deepl-key")
        );

        std::env::remove_var("SNAPTEXT_DEEPSEEK_KEY");
        std::env::remove_var("SNAPTEXT_DEEPL_KEY");

        // 移除后，对默认配置不再覆盖。
        let mut cfg2 = Config::default();
        cfg2.apply_env_overrides();
        assert!(cfg2.translate.deepseek.api_key.is_none());
        assert!(cfg2.translate.deepl.api_key.is_none());
    }
}
