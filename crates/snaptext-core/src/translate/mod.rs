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
            // prompt 模板：默认模式走后端固定常量（升级自动生效），自定义模式用字段值。
            let prompt_template = if cfg.prompt_use_custom {
                cfg.prompt_template.clone()
            } else {
                crate::translate::prompt::DEFAULT_PROMPT_TEMPLATE.to_string()
            };
            Ok(Box::new(OpenAiCompatProvider::new(
                ProviderId::new_static("deepseek"),
                dc.base_url.clone(),
                key,
                dc.model.clone(),
                Duration::from_secs(cfg.timeout_llm_secs),
                client.clone(),
                dc.reasoning_enabled,
                dc.reasoning_effort,
                prompt_template,
                cfg.max_retries,
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
                cfg.max_retries,
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
                cfg.max_retries,
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

/// 判断翻译错误是否值得重试（供各 Provider 重试循环用）。
///
/// 可重试：超时、网络层错误、HTTP 5xx、HTTP 429（限流）。
/// 不重试：HTTP 其他 4xx（401/403 等鉴权错误，重试无用）、响应解析失败、不支持的语言对。
pub fn is_retryable(err: &CoreError) -> bool {
    let crate::error::CoreError::Translate(e) = err else {
        return false;
    };
    use crate::error::TranslateError;
    match e {
        TranslateError::Timeout | TranslateError::Request(_) => true,
        TranslateError::Api { status, .. } => *status >= 500 || *status == 429,
        TranslateError::Parse(_) | TranslateError::UnsupportedPair { .. } => false,
    }
}

/// 重试退避基准（毫秒），第 n 次重试前等待 `RETRY_BASE_MS * 2^n`。供各 Provider 内联重试循环用。
pub const RETRY_BASE_MS: u64 = 500;


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

    #[test]
    fn is_retryable_classifies_correctly() {
        use crate::error::TranslateError;
        // 可重试：超时、网络层、5xx、429。
        assert!(is_retryable(&CoreError::Translate(TranslateError::Timeout)));
        assert!(is_retryable(&CoreError::Translate(TranslateError::Request("net".into()))));
        assert!(is_retryable(&CoreError::Translate(TranslateError::Api {
            status: 500,
            body: String::new()
        })));
        assert!(is_retryable(&CoreError::Translate(TranslateError::Api {
            status: 429,
            body: String::new()
        })));
        // 不可重试：其他 4xx（鉴权等）、解析失败、不支持语言对。
        assert!(!is_retryable(&CoreError::Translate(TranslateError::Api {
            status: 401,
            body: String::new()
        })));
        assert!(!is_retryable(&CoreError::Translate(TranslateError::Parse("bad".into()))));
        // 非 Translate 类错误不重试。
        assert!(!is_retryable(&CoreError::NotImplemented("x")));
    }
}