//! 中文字体注入：运行时从 Windows 系统读取中文字体并注入 egui 默认字体族。
//!
//! egui 默认字体只含拉丁字形，中文会渲染成豆腐块（乱码）。这里优先加载
//! 微软雅黑（msyh.ttc），读不到或解析失败则回退黑体（simhei.ttf）。egui 解析
//! 非法字体数据会 panic，故注入前用 ab_glyph 预校验字节。

use std::fs;
use std::sync::Arc;

use eframe::egui::{FontData, FontDefinitions, FontFamily};

/// 候选字体（相对 `%WINDIR%\Fonts`），顺序即优先级。
const CJK_CANDIDATES: &[&str] = &["msyh.ttc", "simhei.ttf"];

/// 安装中文字体到 egui 上下文。无可用候选时保持默认（仅 warn，不崩）。
pub fn install(ctx: &eframe::egui::Context) {
    let Some((name, bytes)) = load_first_available() else {
        tracing::warn!("未找到可用的系统中文字体（msyh.ttc / simhei.ttf），UI 中文将显示为方块");
        return;
    };

    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert(name.to_owned(), Arc::new(FontData::from_owned(bytes)));
    // 插到最前：中文优先用系统字体，egui 默认拉丁字体作为后续回退。
    if let Some(list) = fonts.families.get_mut(&FontFamily::Proportional) {
        list.insert(0, name.to_owned());
    }
    if let Some(list) = fonts.families.get_mut(&FontFamily::Monospace) {
        list.push(name.to_owned());
    }
    ctx.set_fonts(fonts);
    tracing::info!("已加载系统中文字体：{name}");
}

/// 按优先级读取首个「存在且可解析」的候选，返回 (注册名, 字节)。
fn load_first_available() -> Option<(&'static str, Vec<u8>)> {
    let dir = windows_fonts_dir();
    for file in CJK_CANDIDATES {
        let path = dir.join(file);
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        // 预校验：egui 解析失败会 panic，这里先试 ab_glyph 能否吃下，失败则换下一个。
        if ab_glyph::FontVec::try_from_vec_and_index(bytes.clone(), 0).is_ok() {
            return Some((file, bytes));
        }
        tracing::warn!(?path, "系统字体存在但解析失败，尝试下一个候选");
    }
    None
}

/// `%WINDIR%\Fonts`，缺失 WINDIR 时回退 `C:\Windows\Fonts`。
fn windows_fonts_dir() -> std::path::PathBuf {
    std::env::var_os("WINDIR")
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"))
        .join("Fonts")
}
