//! Provider 故障转移包装器（DU-17）。
//!
//! 主 Provider 调用失败时自动切换到备用（如 DeepSeek → DeepL）。

use async_trait::async_trait;

use crate::error::CoreError;
use crate::translate::TranslationProvider;
use crate::types::{LangPair, ProviderId, TranslateRequest, TranslateResponse};

/// 主失败切备用。`id` / `supported_pairs` 透传主 Provider。
pub struct FallbackProvider {
    primary: Box<dyn TranslationProvider>,
    backup: Box<dyn TranslationProvider>,
}

impl FallbackProvider {
    pub fn new(
        primary: Box<dyn TranslationProvider>,
        backup: Box<dyn TranslationProvider>,
    ) -> Self {
        Self { primary, backup }
    }
}

#[async_trait]
impl TranslationProvider for FallbackProvider {
    fn id(&self) -> ProviderId {
        self.primary.id()
    }
    fn supported_pairs(&self) -> &[LangPair] {
        self.primary.supported_pairs()
    }
    async fn translate(&self, req: TranslateRequest) -> Result<TranslateResponse, CoreError> {
        match self.primary.translate(req.clone()).await {
            Ok(resp) => Ok(resp),
            Err(e) => {
                // 主失败 → 备用（保留主错误以便诊断：备用也失败时返回备用错误）。
                eprintln!("[fallback] 主 Provider 失败，切换备用：{e}");
                self.backup.translate(req).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::TranslateError;
    use crate::types::Lang;

    struct OkProvider(&'static str);
    #[async_trait]
    impl TranslationProvider for OkProvider {
        fn id(&self) -> ProviderId {
            ProviderId::new_static(self.0)
        }
        fn supported_pairs(&self) -> &[LangPair] {
            &[]
        }
        async fn translate(&self, req: TranslateRequest) -> Result<TranslateResponse, CoreError> {
            Ok(TranslateResponse {
                translated_text: format!("[{}] {}", self.0, req.text),
                source: req.source,
                target: req.target,
                provider: self.id(),
                model: None,
                token_usage: None,
            })
        }
    }

    struct ErrProvider;
    #[async_trait]
    impl TranslationProvider for ErrProvider {
        fn id(&self) -> ProviderId {
            ProviderId::new_static("err")
        }
        fn supported_pairs(&self) -> &[LangPair] {
            &[]
        }
        async fn translate(&self, _req: TranslateRequest) -> Result<TranslateResponse, CoreError> {
            Err(CoreError::Translate(TranslateError::Request(
                "primary fail".into(),
            )))
        }
    }

    fn req() -> TranslateRequest {
        TranslateRequest {
            text: "hi".into(),
            source: Lang::En,
            target: Lang::Zh,
            context_hint: None,
            glossary: None,
        }
    }

    #[tokio::test]
    async fn fallback_on_primary_failure() {
        let fb = FallbackProvider::new(Box::new(ErrProvider), Box::new(OkProvider("backup")));
        let resp = fb.translate(req()).await.unwrap();
        assert_eq!(resp.provider.as_str(), "backup");
    }

    #[tokio::test]
    async fn no_fallback_when_primary_ok() {
        let fb = FallbackProvider::new(
            Box::new(OkProvider("primary")),
            Box::new(OkProvider("backup")),
        );
        let resp = fb.translate(req()).await.unwrap();
        assert_eq!(resp.provider.as_str(), "primary");
    }

    #[tokio::test]
    async fn both_fail_returns_backup_error() {
        let fb = FallbackProvider::new(Box::new(ErrProvider), Box::new(ErrProvider));
        assert!(fb.translate(req()).await.is_err());
    }
}
