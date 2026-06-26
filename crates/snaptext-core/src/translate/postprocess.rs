//! 译文后处理（DU-16）。
//!
//! - trim 首尾空白
//! - 去多余包裹引号（LLM 偶尔加的 `"..."` / `""...""`）
//! - 去多余前缀（如 `Translation:` / `翻译：`）

/// 清理译文。
pub fn clean_translation(text: &str) -> String {
    let mut s = text.trim().to_string();
    // 去最多两层包裹引号。
    for _ in 0..2 {
        if s.len() > 1 && s.starts_with('"') && s.ends_with('"') {
            s = s[1..s.len() - 1].to_string();
        } else {
            break;
        }
    }
    // 去前缀。
    for prefix in [
        "Translation:",
        "Translated text:",
        "翻译：",
        "翻译:",
        "译文：",
        "译文:",
    ] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.trim().to_string();
            break;
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_wrapping_quotes() {
        assert_eq!(clean_translation(r#""你好""#), "你好");
        assert_eq!(clean_translation(r#""你好""#), "你好");
        assert_eq!(clean_translation("  hello  "), "hello");
    }

    #[test]
    fn strips_prefix() {
        assert_eq!(clean_translation("翻译：你好"), "你好");
        assert_eq!(clean_translation("Translation: hello"), "hello");
    }
}
