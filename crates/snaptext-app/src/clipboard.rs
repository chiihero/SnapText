//! arboard 剪贴板封装。Windows 上剪贴板操作必须在主线程（见 CONVENTIONS §5）。
//!
//! `set_text` 由 DU-09 卡片"复制译文"调用；`get_text` 留 DU-10/15 接入。

use anyhow::Result;

/// 写入剪贴板。
pub fn set_text(text: &str) -> Result<()> {
    let mut cb = arboard::Clipboard::new()?;
    cb.set_text(text.to_string())?;
    Ok(())
}

/// 读取剪贴板文本；无文本内容时返回 None。
#[allow(dead_code)] // DU-10/15 接入
pub fn get_text() -> Result<Option<String>> {
    let mut cb = arboard::Clipboard::new()?;
    match cb.get_text() {
        Ok(s) => Ok(Some(s)),
        Err(arboard::Error::ContentNotAvailable) => Ok(None),
        Err(e) => Err(e.into()),
    }
}
