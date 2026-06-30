//! Microsoft Azure Translator Provider（DU-18）。
//!
//! 走 Azure Cognitive Services Translator REST API：
//! `POST {endpoint}/translate?api-version=3.0&to=<lang>`，
//! 头 `Ocp-Apim-Subscription-Key` / `Ocp-Apim-Subscription-Region`。

use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;

use super::{common_pairs, TranslationProvider};
use crate::error::{CoreError, TranslateError};
use crate::types::{Lang, LangPair, ProviderId, TranslateRequest, TranslateResponse};

/// Azure Translator Provider。
pub struct MicrosoftProvider {
    client: reqwest::Client,
    key: String,
    region: String,
    endpoint: String,
    timeout: Duration,
    supported: Vec<LangPair>,
    /// 失败重试次数（指数退避，仅重试可重试错误，见 `super::is_retryable`）。
    max_retries: u32,
}

impl MicrosoftProvider {
    pub fn new(
        key: String,
        region: String,
        endpoint: String,
        timeout: Duration,
        client: reqwest::Client,
        max_retries: u32,
    ) -> Self {
        Self {
            client,
            key,
            region,
            endpoint,
            timeout,
            supported: common_pairs(),
            max_retries,
        }
    }
}

fn azure_code(lang: Lang) -> Option<&'static str> {
    match lang {
        Lang::En => Some("en"),
        Lang::Zh => Some("zh-Hans"),
        Lang::Ja => Some("ja"),
        Lang::Auto => None,
    }
}

#[derive(Deserialize)]
struct AzureResponse {
    translations: Vec<AzureTranslation>,
}
#[derive(Deserialize)]
struct AzureTranslation {
    text: String,
}

#[async_trait]
impl TranslationProvider for MicrosoftProvider {
    fn id(&self) -> ProviderId {
        ProviderId::new_static("microsoft")
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

impl MicrosoftProvider {
    /// 单次翻译请求（不含重试）。translate() 在外层包重试循环调用本方法。
    async fn do_once(&self, req: &TranslateRequest) -> Result<TranslateResponse, CoreError> {
        let to = azure_code(req.target).ok_or_else(|| TranslateError::UnsupportedPair {
            src: req.source.to_string(),
            dst: req.target.to_string(),
        })?;
        let mut url = format!(
            "{}/translate?api-version=3.0&to={}",
            self.endpoint.trim_end_matches('/'),
            to
        );
        if let Some(from) = azure_code(req.source) {
            url.push_str(&format!("&from={from}"));
        }
        let body = vec![serde_json::json!({ "Text": req.text })];
        let resp = self
            .client
            .post(&url)
            .header("Ocp-Apim-Subscription-Key", &self.key)
            .header("Ocp-Apim-Subscription-Region", &self.region)
            .timeout(self.timeout)
            .json(&body)
            .send()
            .await
            .map_err(super::classify_send_err)?;
        let resp = super::ensure_2xx(resp).await?;
        let parsed: Vec<AzureResponse> = resp
            .json()
            .await
            .map_err(|e| TranslateError::Parse(e.to_string()))?;
        let text = parsed
            .into_iter()
            .next()
            .and_then(|r| r.translations.into_iter().next())
            .map(|t| t.text)
            .ok_or_else(|| TranslateError::Parse("Azure 响应无 translations".into()))?;
        Ok(TranslateResponse {
            translated_text: text,
            source: req.source,
            target: req.target,
            provider: ProviderId::new_static("microsoft"),
            model: None,
            token_usage: None,
        })
    }
}
