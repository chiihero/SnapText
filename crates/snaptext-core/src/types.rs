//! 跨模块共享的值类型。
//!
//! 仅声明数据结构，不定义 trait，不写业务逻辑。
//! 依赖关系：本模块单向依赖 [`crate::error`]（`Lang::FromStr` 复用 `CoreError`）。

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::time::SystemTime;

use image::RgbaImage;
use serde::{Deserialize, Serialize};

use crate::error::{ConfigError, CoreError};

/// ISO 639-1 语言码。支持中/英/日，以及自动检测（仅 OCR 源语言用）。
///
/// 序列化为小写代码（`"en"`/`"zh"`/`"ja"`/`"auto"`），
/// 反序列化宽容接受大小写与英文全名（见别名）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Lang {
    #[serde(rename = "en", alias = "EN", alias = "english", alias = "English")]
    En,
    #[serde(
        rename = "zh",
        alias = "ZH",
        alias = "CN",
        alias = "chinese",
        alias = "Chinese"
    )]
    Zh,
    #[serde(
        rename = "ja",
        alias = "JA",
        alias = "JP",
        alias = "japanese",
        alias = "Japanese"
    )]
    Ja,
    #[serde(rename = "auto", alias = "AUTO", alias = "Auto")]
    Auto,
}

impl fmt::Display for Lang {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Lang::En => "en",
            Lang::Zh => "zh",
            Lang::Ja => "ja",
            Lang::Auto => "auto",
        };
        f.write_str(s)
    }
}

impl FromStr for Lang {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "en" | "english" => Ok(Lang::En),
            "zh" | "cn" | "chinese" => Ok(Lang::Zh),
            "ja" | "jp" | "japanese" => Ok(Lang::Ja),
            "auto" => Ok(Lang::Auto),
            other => Err(CoreError::Config(ConfigError::Invalid {
                field: "lang".into(),
                reason: format!("不支持的语言代码：{other}"),
            })),
        }
    }
}

/// 矩形包围盒，使用 i32 像素坐标（虚拟桌面坐标系）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bbox {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// 显示器标识（newtype around `String`）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MonitorId(pub String);

impl MonitorId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MonitorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// 显示器元数据。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub id: MonitorId,
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// DPI 缩放比例（1.0 = 100%、1.5 = 150%、2.0 = 200%）。
    pub scale: f32,
    /// 显示器在虚拟桌面中的左上角 X。
    pub x: i32,
    /// 显示器在虚拟桌面中的左上角 Y。
    pub y: i32,
    /// 是否为主显示器。
    pub is_primary: bool,
}

/// 已捕获的一帧图像。
///
/// 运行时大对象，不实现 `Serialize`；跨线程共享时由调用方用 `Arc` 包裹。
#[derive(Debug, Clone)]
pub struct CapturedFrame {
    /// 来源显示器。
    pub monitor: MonitorInfo,
    /// RGBA 像素数据。
    pub image: RgbaImage,
    /// 捕获时刻。
    pub captured_at: SystemTime,
}

/// 书写方向（横排 / 竖排）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WritingDirection {
    Horizontal,
    Vertical,
}

/// OCR 识别出的一行文本。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OcrLine {
    pub text: String,
    pub bbox: Bbox,
    /// 置信度 0.0 ~ 1.0。
    pub confidence: f32,
    pub writing_direction: WritingDirection,
}

/// 翻译语言对。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LangPair {
    pub source: Lang,
    pub target: Lang,
}

/// 翻译请求。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslateRequest {
    pub text: String,
    pub source: Lang,
    pub target: Lang,
    /// 给 LLM 的上下文提示（可选）。
    pub context_hint: Option<String>,
    /// 术语表（可选，DU-22 已永久砍除生效逻辑，但结构保留供 Provider 选择性使用）。
    pub glossary: Option<HashMap<String, String>>,
}

/// 翻译响应。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslateResponse {
    pub translated_text: String,
    pub source: Lang,
    pub target: Lang,
    pub provider: ProviderId,
    /// 使用的模型名（LLM 类 Provider 填，专用 MT 可为 `None`）。
    pub model: Option<String>,
    pub token_usage: Option<TokenUsage>,
}

/// Token 用量。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

impl TokenUsage {
    /// 总 token 数。
    pub fn total(self) -> u64 {
        self.prompt_tokens + self.completion_tokens
    }
}

/// Provider 标识。
///
/// 用 `Cow<'static, str>` 同时支持 const 静态构造（零分配）与运行时 `String`。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProviderId(Cow<'static, str>);

impl ProviderId {
    /// const 构造（编译期字符串，零分配）。
    pub const fn new_static(s: &'static str) -> Self {
        Self(Cow::Borrowed(s))
    }

    /// 运行时构造（接受 `&'static str` 或 `String`）。
    pub fn new(s: impl Into<Cow<'static, str>>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&'static str> for ProviderId {
    fn from(s: &'static str) -> Self {
        Self::new_static(s)
    }
}

impl From<String> for ProviderId {
    fn from(s: String) -> Self {
        Self(Cow::Owned(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_roundtrip() {
        for lang in [Lang::En, Lang::Zh, Lang::Ja, Lang::Auto] {
            let s = lang.to_string();
            let back: Lang = s.parse().unwrap();
            assert_eq!(lang, back);
        }
    }

    #[test]
    fn lang_aliases_tolerant() {
        assert_eq!("EN".parse::<Lang>().unwrap(), Lang::En);
        assert_eq!("English".parse::<Lang>().unwrap(), Lang::En);
        assert_eq!("chinese".parse::<Lang>().unwrap(), Lang::Zh);
        assert_eq!("JP".parse::<Lang>().unwrap(), Lang::Ja);
        assert_eq!("japanese".parse::<Lang>().unwrap(), Lang::Ja);
    }

    #[test]
    fn lang_invalid_rejected() {
        assert!("klingon".parse::<Lang>().is_err());
        assert!("".parse::<Lang>().is_err());
    }

    #[test]
    fn lang_serde_lowercase() {
        assert_eq!(serde_json::to_string(&Lang::En).unwrap(), "\"en\"");
        assert_eq!(serde_json::to_string(&Lang::Zh).unwrap(), "\"zh\"");
        let ja: Lang = serde_json::from_str("\"ja\"").unwrap();
        assert_eq!(ja, Lang::Ja);
    }

    #[test]
    fn provider_id_static_and_owned() {
        const DEEPSEEK: ProviderId = ProviderId::new_static("deepseek");
        assert_eq!(DEEPSEEK.as_str(), "deepseek");
        assert_eq!(DEEPSEEK.to_string(), "deepseek");

        let owned = ProviderId::from(String::from("openai"));
        assert_eq!(owned.as_str(), "openai");
        assert_ne!(DEEPSEEK, owned);

        // serde 序列化为纯字符串
        let json = serde_json::to_string(&DEEPSEEK).unwrap();
        assert_eq!(json, "\"deepseek\"");
    }

    #[test]
    fn token_usage_total() {
        let u = TokenUsage {
            prompt_tokens: 120,
            completion_tokens: 30,
        };
        assert_eq!(u.total(), 150);
    }

    #[test]
    fn monitor_id_serde_transparent() {
        let id = MonitorId::new("\\\\.\\DISPLAY1");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"\\\\\\\\.\\\\DISPLAY1\"");
    }
}
