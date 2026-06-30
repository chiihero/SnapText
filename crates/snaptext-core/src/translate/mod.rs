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
pub use openai_compat::{OpenAiCompatParams, OpenAiCompatProvider};

use std::time::Duration;

use async_trait::async_trait;

use crate::config::{DeepLPlan, ProviderKind, TranslateConfig};
use crate::error::{ConfigError, CoreError, TranslateError};
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
            Ok(Box::new(OpenAiCompatProvider::new(OpenAiCompatParams {
                id: ProviderId::new_static("deepseek"),
                base_url: dc.base_url.clone(),
                api_key: key,
                model: dc.model.clone(),
                timeout: Duration::from_secs(cfg.timeout_llm_secs),
                client: client.clone(),
                reasoning_enabled: dc.reasoning_enabled,
                reasoning_effort: dc.reasoning_effort,
                prompt_template,
                max_retries: cfg.max_retries,
            })))
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

/// 重试退避基准（毫秒），第 n 次重试前等待 `RETRY_BASE_MS * 2^n`。
pub const RETRY_BASE_MS: u64 = 500;

/// 把 reqwest 发送错误归类为 `TranslateError`：超时 → `Timeout`，其余 → `Request`。
///
/// 三个 Provider 的 `do_once` 此前各内联一份完全相同的 `if e.is_timeout() {...}` 分类，
/// 抽出后用 `map_err(classify_send_err)` 一行替代。
pub fn classify_send_err(e: reqwest::Error) -> TranslateError {
    if e.is_timeout() {
        TranslateError::Timeout
    } else {
        TranslateError::Request(e.to_string())
    }
}

/// 检查 HTTP 响应是否 2xx，否则读 body 封装为 `TranslateError::Api`。
///
/// 三个 Provider 此前各内联一份完全相同的 `if !status.is_success() {...}` 样板。
/// 成功时原样返回 `resp` 供后续解析。
pub async fn ensure_2xx(resp: reqwest::Response) -> Result<reqwest::Response, CoreError> {
    let status = resp.status();
    if status.is_success() {
        Ok(resp)
    } else {
        let body = resp.text().await.unwrap_or_default();
        Err(CoreError::Translate(TranslateError::Api {
            status: status.as_u16(),
            body,
        }))
    }
}

/// 统一的翻译重试包装（供三个 Provider 复用）。
///
/// 接受单次请求闭包 `do_once` 与重试次数 `max_retries`，按 `is_retryable` 判定
/// 是否重试（超时/网络/5xx/429 重试，其他 4xx/Parse/UnsupportedPair 不重试），
/// 退避 `RETRY_BASE_MS * 2^attempt`。各 Provider 把单次 HTTP 逻辑抽成 `do_once`，
/// `translate()` 直接调本函数。
pub async fn with_retry<F, Fut, T>(max_retries: u32, mut do_once: F) -> Result<T, CoreError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, CoreError>>,
{
    let mut last_err: Option<CoreError> = None;
    for attempt in 0..=max_retries {
        match do_once().await {
            Ok(resp) => return Ok(resp),
            Err(e) => {
                let retryable = is_retryable(&e);
                last_err = Some(e);
                if retryable && attempt < max_retries {
                    let delay = Duration::from_millis(RETRY_BASE_MS * 2u64.pow(attempt));
                    tokio::time::sleep(delay).await;
                    continue;
                }
                break;
            }
        }
    }
    // 循环至少执行一次（attempt 从 0 开始），last_err 必有值。
    Err(last_err.expect("重试循环至少执行一次，必有 last_err"))
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

    #[test]
    fn is_retryable_classifies_correctly() {
        use crate::error::TranslateError;
        // 可重试：超时、网络层、5xx、429。
        assert!(is_retryable(&CoreError::Translate(TranslateError::Timeout)));
        assert!(is_retryable(&CoreError::Translate(
            TranslateError::Request("net".into())
        )));
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
        assert!(!is_retryable(&CoreError::Translate(TranslateError::Parse(
            "bad".into()
        ))));
        // 非 Translate 类错误不重试。
        assert!(!is_retryable(&CoreError::NotImplemented("x")));
    }
}
