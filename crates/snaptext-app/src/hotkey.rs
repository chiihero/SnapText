//! global-hotkey 封装（必须在主线程注册，见 CONVENTIONS §5）。

use anyhow::Result;
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyManager,
};
use snaptext_core::config::HotkeyConfig;

/// 注册全局触发热键（默认 Ctrl+Alt+Q）。返回 manager 与已注册的 HotKey
///（后者供设置保存后 [`re_register`] 替换）。
pub fn register(cfg: &HotkeyConfig) -> Result<(GlobalHotKeyManager, HotKey)> {
    let manager = GlobalHotKeyManager::new()?;
    let hotkey = parse(&cfg.trigger).unwrap_or_else(default_trigger);
    manager.register(hotkey)?;
    Ok((manager, hotkey))
}

/// 运行时切换热键：按 `cfg` 解析并注册新热键，成功后注销 `old`，返回新 HotKey 供下次替换。
///
/// 解析失败回退默认；新热键注册失败时保留旧热键不动（返回原错误）。
pub fn re_register(
    manager: &GlobalHotKeyManager,
    old: HotKey,
    cfg: &HotkeyConfig,
) -> Result<HotKey> {
    let new = parse(&cfg.trigger).unwrap_or_else(default_trigger);
    // 先注册新热键，成功后再注销旧的，避免切换中途无热键可用。
    manager.register(new)?;
    let _ = manager.unregister(old);
    Ok(new)
}

fn default_trigger() -> HotKey {
    HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyQ)
}

/// 解析配置中的热键字符串（如 "Control+Alt+KeyQ"），失败回退默认。
fn parse(s: &str) -> Option<HotKey> {
    s.parse::<HotKey>().ok()
}
