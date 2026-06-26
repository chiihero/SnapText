//! LLM 翻译 prompt 模板（OpenAI 兼容 Provider 共用）。

use crate::types::TranslateRequest;

/// 渲染翻译 prompt：约束"仅输出译文，不加解释/引号"（见 DESIGN §5.3）。
pub fn render_translate_prompt(req: &TranslateRequest) -> String {
    format!(
        "Translate the following text from {src} to {tgt}.\n\
         Output ONLY the translation. No explanations, no quotes, no notes.\n\n\
         Text:\n{input}",
        src = req.source,
        tgt = req.target,
        input = req.text,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Lang;

    fn req(text: &str, src: Lang, tgt: Lang) -> TranslateRequest {
        TranslateRequest {
            text: text.into(),
            source: src,
            target: tgt,
            context_hint: None,
            glossary: None,
        }
    }

    #[test]
    fn prompt_contains_direction_and_constraints_and_text() {
        let p = render_translate_prompt(&req("Hello", Lang::En, Lang::Zh));
        assert!(p.contains("from en to zh"));
        assert!(p.contains("ONLY the translation"));
        assert!(p.contains("Hello"));
    }
}
