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
}

impl DeepLProvider {
    pub fn new(
        api_key: String,
        base_url: String,
        timeout: Duration,
        client: reqwest::Client,
    ) -> Self {
        Self {
            client,
            base_url,
            api_key,
            timeout,
            supported: common_pairs(),
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
            .map_err(|e| {
                if e.is_timeout() {
                    TranslateError::Timeout
                } else {
                    TranslateError::Request(e.to_string())
                }
            })?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(CoreError::Translate(TranslateError::Api {
                status: status.as_u16(),
                body,
            }));
        }
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
