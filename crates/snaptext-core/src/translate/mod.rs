//! 翻译模块：[`TranslationProvider`] trait + Provider 实现 + 工厂。
//!
//! MVP 仅 DeepSeek（OpenAI 兼容）+ DeepL。microsoft.rs / baidu.rs 推迟 P2 DU-18。

pub mod deepl;
pub mod fallback;
pub mod microsoft;
pub mod openai_compat;
pub mod postprocess;
pub mod prompt;

pub use deepl::DeepLProvider;
pub use microsoft::MicrosoftProvider;
pub use openai_compat::OpenAiCompatProvider;

use std::time::Duration;

use async_trait::async_trait;

use crate::config::{DeepLPlan, ProviderKind, TranslateConfig};
use crate::error::{ConfigError, CoreError};
use crate::types::{Lang, LangPair, ProviderId, TranslateRequest, TranslateResponse};

/// 项目主用翻译方向：英↔中、日→中、日→英。
pub fn common_pairs() -> Vec<LangPair> {
    vec![
        LangPair {
            source: Lang::En,
            target: Lang::Zh,
        },
        LangPair {
            source: Lang::Zh,
            target: Lang::En,
        },
        LangPair {
            source: Lang::Ja,
            target: Lang::Zh,
        },
        LangPair {
            source: Lang::Ja,
            target: Lang::En,
        },
    ]
}

/// 翻译能力抽象。所有实现 `Send + Sync`，HTTP 调用带超时（LLM 30s / MT 10s，见 CONVENTIONS §3）。
#[async_trait]
pub trait TranslationProvider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn supported_pairs(&self) -> &[LangPair];
    async fn translate(&self, req: TranslateRequest) -> Result<TranslateResponse, CoreError>;
}

/// 按 config 构造 Provider（共享传入的 `reqwest::Client`，不每次 new）。
pub fn build_provider(
    cfg: &TranslateConfig,
    client: &reqwest::Client,
) -> Result<Box<dyn TranslationProvider>, CoreError> {
    match cfg.provider {
        ProviderKind::DeepSeek => {
            let dc = &cfg.deepseek;
            let key = require_key(&dc.api_key, "deepseek.api_key / SNAPTEXT_DEEPSEEK_KEY")?;
            Ok(Box::new(OpenAiCompatProvider::new(
                ProviderId::new_static("deepseek"),
                dc.base_url.clone(),
                key,
                dc.model.clone(),
                Duration::from_secs(cfg.timeout_llm_secs),
                client.clone(),
            )))
        }
        ProviderKind::DeepL => {
            let dc = &cfg.deepl;
            let key = require_key(&dc.api_key, "deepl.api_key / SNAPTEXT_DEEPL_KEY")?;
            let base = match dc.plan {
                DeepLPlan::Free => "https://api-free.deepl.com/v2",
                DeepLPlan::Pro => "https://api.deepl.com/v2",
            };
            Ok(Box::new(DeepLProvider::new(
                key,
                base.to_string(),
                Duration::from_secs(cfg.timeout_mt_secs),
                client.clone(),
            )))
        }
        ProviderKind::Microsoft => {
            let mc = &cfg.microsoft;
            let key = require_key(&mc.api_key, "microsoft.api_key")?;
            Ok(Box::new(MicrosoftProvider::new(
                key,
                mc.region.clone(),
                mc.endpoint.clone(),
                Duration::from_secs(cfg.timeout_mt_secs),
                client.clone(),
            )))
        }
    }
}

fn require_key(opt: &Option<String>, hint: &str) -> Result<String, CoreError> {
    opt.clone().filter(|k| !k.is_empty()).ok_or_else(|| {
        CoreError::Config(ConfigError::Invalid {
            field: "api_key".into(),
            reason: format!("缺少 API Key（请设置 {hint}）"),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_provider_errors_without_key() {
        let cfg = TranslateConfig::default();
        let client = reqwest::Client::new();
        // 无 API Key 应返回错误（dyn Trait 无 Debug，仅断言 is_err）。
        assert!(build_provider(&cfg, &client).is_err());
    }
}
