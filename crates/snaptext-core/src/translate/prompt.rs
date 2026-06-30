//! LLM 翻译 prompt 模板（OpenAI 兼容 Provider 共用）。

use crate::types::TranslateRequest;

/// 默认 prompt 模板（单一数据源）：
/// - `config.rs::TranslateConfig::default` 引用本常量作为字段默认值；
/// - 前端设置页"恢复默认"按钮文案需与此手动对齐（见 `Settings.vue` 同步注释）。
///
/// 设计：英文系统指令（LLM 对英文更稳、token 更省）+ 角色 + 输出约束 + 保留换行/标点。
/// 占位符用双花括号避免与用户原文里的 `{...}` 冲突（Jinja2/mustache 惯例）。
pub const DEFAULT_PROMPT_TEMPLATE: &str = "\
You are a precise translator. Translate the text below from {{source}} to {{target}}.\n\
- Output ONLY the translation.\n\
- No explanations, quotes, or prefixes.\n\
- Preserve line breaks and the original punctuation style.\n\n\
{{input}}";

/// 返回默认 prompt 模板（命令层 `get_default_prompt` 经此暴露给前端只读展示）。
/// 单一数据源：本函数与 `DEFAULT_PROMPT_TEMPLATE` 常量同源。
pub fn default_prompt_template() -> &'static str {
    DEFAULT_PROMPT_TEMPLATE
}

/// 渲染翻译 prompt：用 `template` 中的占位符替换为实际值。
///
/// 占位符：`{{source}}` 源语言（iso 码如 `en`）、`{{target}}` 目标语言、`{{input}}` 原文。
///
/// 容错：若 `template` 不含 `{{input}}`（用户改坏导致原文进不去），自动在末尾追加原文，
/// 防止模型拿不到源文本瞎编。
pub fn render_translate_prompt(req: &TranslateRequest, template: &str) -> String {
    let rendered = template
        .replace("{{source}}", &req.source.to_string())
        .replace("{{target}}", &req.target.to_string());
    if template.contains("{{input}}") {
        rendered.replace("{{input}}", &req.text)
    } else {
        // 模板缺失 {{input}}：追加原文兜底。
        format!("{rendered}\n\n{}", req.text)
    }
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
    fn default_template_contains_direction_constraints_and_text() {
        let p = render_translate_prompt(&req("Hello", Lang::En, Lang::Zh), DEFAULT_PROMPT_TEMPLATE);
        assert!(p.contains("from en to zh"));
        assert!(p.contains("ONLY the translation"));
        assert!(p.contains("Hello"));
    }

    #[test]
    fn custom_template_resolves_placeholders() {
        let tpl = "把 {{input}} 从 {{source}} 译为 {{target}}";
        let p = render_translate_prompt(&req("Hi", Lang::En, Lang::Zh), tpl);
        assert_eq!(p, "把 Hi 从 en 译为 zh");
    }

    #[test]
    fn template_without_input_placeholder_appends_text() {
        // 用户模板若漏掉 {{input}}，原文应被追加（防模型拿不到源文本瞎编）。
        let tpl = "Translate from {{source}} to {{target}}, output only.";
        let p = render_translate_prompt(&req("Body", Lang::Ja, Lang::En), tpl);
        assert!(p.contains("from ja to en"));
        assert!(p.contains("Body"), "原文必须出现在渲染结果里");
        assert!(p.ends_with("Body"));
    }
}
