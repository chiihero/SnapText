//! DeepL Provider，走 `POST /v2/translate`（Free → api-free.deepl.com，Pro → api.deepl.com）。

use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;

use super::{common_pairs, TranslationProvider};
use crate::error::{CoreError, TranslateError};
use crate::types::{Lang, LangPair, ProviderId, TranslateRequest, TranslateResponse};

/// DeepL 翻译 Provider。
pub struct DeepLProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    timeout: Duration,
    supported: Vec<LangPair>,
    /// 失败重试次数（指数退避，仅重试可重试错误，见 `super::is_retryable`）。
    max_retries: u32,
}

impl DeepLProvider {
    pub fn new(
        api_key: String,
        base_url: String,
        timeout: Duration,
        client: reqwest::Client,
        max_retries: u32,
    ) -> Self {
        Self {
            client,
            base_url,
            api_key,
            timeout,
            supported: common_pairs(),
            max_retries,
        }
    }
}

#[derive(Deserialize)]
struct DeepLResponse {
    translations: Vec<DeepLTranslation>,
}
#[derive(Deserialize)]
struct DeepLTranslation {
    text: String,
}

/// Lang → DeepL 语言码（大写）。Auto 不传 source_lang，让 DeepL 检测。
fn deepl_code(lang: Lang) -> Option<&'static str> {
    match lang {
        Lang::En => Some("EN"),
        Lang::Zh => Some("ZH"),
        Lang::Ja => Some("JA"),
        Lang::Auto => None,
    }
}

#[async_trait]
impl TranslationProvider for DeepLProvider {
    fn id(&self) -> ProviderId {
        ProviderId::new_static("deepl")
    }

    fn supported_pairs(&self) -> &[LangPair] {
        &self.supported
    }

    async fn translate(&self, req: TranslateRequest) -> Result<TranslateResponse, CoreError> {
        // 重试包装：仅重试可重试错误（超时/网络/5xx/429），指数退避。
        super::with_retry(self.max_retries, || {
            let req = &req;
            async move { self.do_once(req).await }
        })
        .await
    }
}

impl DeepLProvider {
    /// 单次翻译请求（不含重试）。translate() 在外层包重试循环调用本方法。
    async fn do_once(&self, req: &TranslateRequest) -> Result<TranslateResponse, CoreError> {
        let target = deepl_code(req.target).ok_or_else(|| TranslateError::UnsupportedPair {
            src: req.source.to_string(),
            dst: req.target.to_string(),
        })?;
        let url = format!("{}/translate", self.base_url.trim_end_matches('/'));
        let mut form: Vec<(String, String)> = vec![
            ("auth_key".into(), self.api_key.clone()),
            ("text".into(), req.text.clone()),
            ("target_lang".into(), target.to_string()),
        ];
        if let Some(src) = deepl_code(req.source) {
            form.push(("source_lang".into(), src.to_string()));
        }
        let resp = self
            .client
            .post(&url)
            .timeout(self.timeout)
            .form(&form)
            .send()
            .await
            .map_err(super::classify_send_err)?;
        let resp = super::ensure_2xx(resp).await?;
        let parsed: DeepLResponse = resp
            .json()
            .await
            .map_err(|e| TranslateError::Parse(e.to_string()))?;
        let text = parsed
            .translations
            .into_iter()
            .next()
            .ok_or_else(|| TranslateError::Parse("DeepL 响应无 translations".into()))?
            .text;
        Ok(TranslateResponse {
            translated_text: text,
            source: req.source,
            target: req.target,
            provider: ProviderId::new_static("deepl"),
            model: None,
            token_usage: None,
        })
    }
}
