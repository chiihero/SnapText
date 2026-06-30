//! OpenAI 兼容 Provider（含 DeepSeek），走 `POST /v1/chat/completions`。

use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;

use super::prompt::render_translate_prompt;
use super::{common_pairs, TranslationProvider};
use crate::config::ReasoningEffort;
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
    /// 思考模式开关（DeepSeek V3.2+ thinking 参数，见 DESIGN §4.3）。
    reasoning_enabled: bool,
    reasoning_effort: ReasoningEffort,
    /// LLM 翻译 prompt 模板（占位符 `{{source}}`/`{{target}}`/`{{input}}`）。
    prompt_template: String,
    /// 失败重试次数（指数退避，仅重试可重试错误，见 `super::is_retryable`）。
    max_retries: u32,
}

/// 构造 [`OpenAiCompatProvider`] 的入参（字段众多，收敛为 struct 自文档且避免 `too_many_arguments`）。
pub struct OpenAiCompatParams {
    pub id: ProviderId,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub timeout: Duration,
    pub client: reqwest::Client,
    pub reasoning_enabled: bool,
    pub reasoning_effort: ReasoningEffort,
    pub prompt_template: String,
    pub max_retries: u32,
}

impl OpenAiCompatProvider {
    pub fn new(params: OpenAiCompatParams) -> Self {
        let OpenAiCompatParams {
            id,
            base_url,
            api_key,
            model,
            timeout,
            client,
            reasoning_enabled,
            reasoning_effort,
            prompt_template,
            max_retries,
        } = params;
        Self {
            client,
            base_url,
            api_key,
            model,
            timeout,
            id,
            supported: common_pairs(),
            reasoning_enabled,
            reasoning_effort,
            prompt_template,
            max_retries,
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
    /// 思考模式下的思维链（与 content 同级）。翻译单轮场景忽略，仅声明以兼容响应。
    #[serde(default)]
    #[allow(dead_code)]
    reasoning_content: Option<String>,
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
        if self.model.trim().is_empty() {
            return Err(CoreError::Translate(TranslateError::Request(
                "请先在设置选择模型".into(),
            )));
        }
        // 重试包装：仅重试可重试错误（超时/网络/5xx/429），指数退避。
        super::with_retry(self.max_retries, || {
            let req = &req;
            async move { self.do_once(req).await }
        })
        .await
    }
}

impl OpenAiCompatProvider {
    /// 单次翻译请求（不含重试）。translate() 在外层包重试循环调用本方法。
    async fn do_once(&self, req: &TranslateRequest) -> Result<TranslateResponse, CoreError> {
        let prompt = render_translate_prompt(req, &self.prompt_template);
        // 思考模式参数（DESIGN §4.3 事实基准）：thinking 与 reasoning_effort 互斥。
        // 关 → thinking:disabled；开 → thinking:enabled + reasoning_effort。
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": [{"role":"user","content":prompt}],
        });
        if self.reasoning_enabled {
            body["thinking"] = serde_json::json!({ "type": "enabled" });
            body["reasoning_effort"] = serde_json::json!(match self.reasoning_effort {
                ReasoningEffort::High => "high",
                ReasoningEffort::Max => "max",
            });
        } else {
            body["thinking"] = serde_json::json!({ "type": "disabled" });
        }
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .timeout(self.timeout)
            .json(&body)
            .send()
            .await
            .map_err(super::classify_send_err)?;
        let resp = super::ensure_2xx(resp).await?;
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
        let provider = OpenAiCompatProvider::new(OpenAiCompatParams {
            id: ProviderId::new_static("deepseek"),
            base_url: "https://api.deepseek.com/v1".into(),
            api_key: key,
            model: "deepseek-v4-flash".into(),
            timeout: Duration::from_secs(30),
            client: reqwest::Client::new(),
            reasoning_enabled: false,
            reasoning_effort: ReasoningEffort::High,
            prompt_template: crate::translate::prompt::DEFAULT_PROMPT_TEMPLATE.to_string(),
            max_retries: 2,
        });
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
