//! tray-icon 封装（必须在主线程构造，见 CONVENTIONS §5）。

use anyhow::{Context, Result};
use tray_icon::{
    menu::{Menu, MenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};

/// 托盘图标 PNG（32×32，编译期嵌入）。
const TRAY_PNG: &[u8] = include_bytes!("../assets/tray.png");

/// 构造托盘：图标 + 菜单（显示 / 退出）。
pub fn build() -> Result<TrayIcon> {
    let menu = Menu::new();
    menu.append(&MenuItem::with_id("show", "显示", true, None))?;
    menu.append(&MenuItem::with_id("settings", "设置", true, None))?;
    menu.append(&MenuItem::with_id("history", "历史", true, None))?;
    menu.append(&MenuItem::with_id("quit", "退出", true, None))?;

    let img = image::load_from_memory(TRAY_PNG)?.to_rgba8();
    let (w, h) = img.dimensions();
    let icon = Icon::from_rgba(img.into_raw(), w, h).context("托盘图标解码失败")?;

    Ok(TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("SnapText")
        .with_icon(icon)
        .build()?)
}
