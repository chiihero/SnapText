//! OpenAI 兼容 Provider（含 DeepSeek），走 `POST /v1/chat/completions`。

use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;

use super::prompt::render_translate_prompt;
use super::{common_pairs, TranslationProvider};
use crate::error::{CoreError, TranslateError};
use crate::types::{LangPair, ProviderId, TokenUsage, TranslateRequest, TranslateResponse};

/// OpenAI 兼容（DeepSeek / OpenAI / Moonshot 等）翻译 Provider。
pub struct OpenAiCompatProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
    timeout: Duration,
    id: ProviderId,
    supported: Vec<LangPair>,
}

impl OpenAiCompatProvider {
    pub fn new(
        id: ProviderId,
        base_url: String,
        api_key: String,
        model: String,
        timeout: Duration,
        client: reqwest::Client,
    ) -> Self {
        Self {
            client,
            base_url,
            api_key,
            model,
            timeout,
            id,
            supported: common_pairs(),
        }
    }
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
    #[serde(default)]
    usage: Option<ChatUsage>,
}
#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}
#[derive(Deserialize)]
struct ChatMessage {
    content: String,
}
#[derive(Deserialize)]
struct ChatUsage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
}

#[async_trait]
impl TranslationProvider for OpenAiCompatProvider {
    fn id(&self) -> ProviderId {
        self.id.clone()
    }

    fn supported_pairs(&self) -> &[LangPair] {
        &self.supported
    }

    async fn translate(&self, req: TranslateRequest) -> Result<TranslateResponse, CoreError> {
        let prompt = render_translate_prompt(&req);
        let body = serde_json::json!({
            "model": self.model,
            "messages": [{"role":"user","content":prompt}],
        });
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .timeout(self.timeout)
            .json(&body)
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
        let parsed: ChatResponse = resp
            .json()
            .await
            .map_err(|e| TranslateError::Parse(e.to_string()))?;
        let text = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| TranslateError::Parse("响应无 choices".into()))?
            .message
            .content
            .trim()
            .to_string();
        Ok(TranslateResponse {
            translated_text: text,
            source: req.source,
            target: req.target,
            provider: self.id.clone(),
            model: Some(self.model.clone()),
            token_usage: parsed.usage.map(|u| TokenUsage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Lang;

    #[tokio::test]
    #[ignore = "需要 SNAPTEXT_DEEPSEEK_KEY 真实调用 DeepSeek"]
    async fn deepseek_translate_real() {
        let key = match std::env::var("SNAPTEXT_DEEPSEEK_KEY") {
            Ok(k) if !k.is_empty() => k,
            _ => {
                println!("跳过：未设置 SNAPTEXT_DEEPSEEK_KEY");
                return;
            }
        };
        let provider = OpenAiCompatProvider::new(
            ProviderId::new_static("deepseek"),
            "https://api.deepseek.com/v1".into(),
            key,
            "deepseek-chat".into(),
            Duration::from_secs(30),
            reqwest::Client::new(),
        );
        let req = TranslateRequest {
            text: "Hello, world.".into(),
            source: Lang::En,
            target: Lang::Zh,
            context_hint: None,
            glossary: None,
        };
        let resp = provider.translate(req).await.expect("翻译失败");
        println!("译文：{}", resp.translated_text);
        assert!(!resp.translated_text.is_empty());
    }
}
