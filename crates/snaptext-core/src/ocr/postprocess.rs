//! OCR 输出后处理（DU-16）。
//!
//! - 去 CJK 字符间的多余空格（OCR 常见噪声）
//! - 合并被错误拆分的换行
//! - trim 首尾空白

/// 清理 OCR 文本。
pub fn clean_ocr_text(text: &str) -> String {
    let s = remove_cjk_spaces(text);
    let s = merge_broken_newlines(&s);
    s.trim().to_string()
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x4E00..=0x9FFF | 0x3040..=0x30FF | 0x3400..=0x4DBF | 0xF900..=0xFAFF)
}

/// 去掉 CJK 字符之间的空格（"汉 字" → "汉字"），保留其他空格。
pub fn remove_cjk_spaces(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(chars.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == ' ' {
            let prev = if i > 0 {
                chars.get(i - 1).copied()
            } else {
                None
            };
            let next = chars.get(i + 1).copied();
            if prev.map(is_cjk).unwrap_or(false) && next.map(is_cjk).unwrap_or(false) {
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// 把换行合并为单个空格（OCR 把同一句拆行的常见修正）。
pub fn merge_broken_newlines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev = '\0';
    for c in s.chars() {
        if c == '\n' || c == '\r' {
            if !matches!(prev, ' ' | '\n' | '\r') {
                out.push(' ');
                prev = ' ';
            }
        } else {
            out.push(c);
            prev = c;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_cjk_inner_spaces() {
        assert_eq!(remove_cjk_spaces("汉 字 识 别"), "汉字识别");
        assert_eq!(remove_cjk_spaces("hello world"), "hello world");
        assert_eq!(remove_cjk_spaces("中 a 文"), "中 a 文");
    }

    #[test]
    fn merges_newlines() {
        assert_eq!(merge_broken_newlines("第一行\n第二行"), "第一行 第二行");
        assert_eq!(merge_broken_newlines("a\n\nb"), "a b");
    }

    #[test]
    fn clean_full_pipeline() {
        assert_eq!(clean_ocr_text(" 汉 字\n识 别 "), "汉字 识别");
    }
}
