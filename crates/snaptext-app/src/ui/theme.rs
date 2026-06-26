//! 浅色主题：统一配色常量 + `apply` 应用到 egui 上下文 + `card_frame` 分组容器。
//!
//! 固定浅色（不做主题切换）。配色对标主流工具的清爽风格：柔和蓝强调、灰白背景、
//! 白卡片、浅灰边框、深灰文字。设置/引导/卡片等面板统一用 [`card_frame`] 包裹分组，
//! 保证视觉一致。

use eframe::egui::{Color32, Context, Frame, Margin, Rounding, Stroke, Style, Vec2, Visuals};

/// 强调色（按钮/链接/选中）。
pub const ACCENT: Color32 = Color32::from_rgb(13, 110, 170);
/// 强调色（按下）。
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(11, 94, 145);
/// 强调色的浅底（hover 行/选中行）。
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(235, 242, 249);
/// 主背景（面板）。
pub const BG: Color32 = Color32::from_rgb(247, 248, 250);
/// 卡片/窗口表面。
pub const SURFACE: Color32 = Color32::from_rgb(255, 255, 255);
/// 边框。
pub const BORDER: Color32 = Color32::from_rgb(220, 224, 230);
/// 正文文字。
pub const TEXT: Color32 = Color32::from_rgb(33, 41, 52);
/// 次要文字。
pub const TEXT_WEAK: Color32 = Color32::from_rgb(120, 130, 142);

/// 应用浅色主题到 egui 上下文。
pub fn apply(ctx: &Context) {
    let mut v = Visuals::light();
    v.panel_fill = BG;
    v.window_fill = SURFACE;
    v.extreme_bg_color = Color32::from_rgb(240, 242, 245);
    v.faint_bg_color = Color32::from_rgb(241, 243, 246);
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.noninteractive.bg_fill = BG;
    v.widgets.noninteractive.rounding = Rounding::same(6.0);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.inactive.bg_fill = SURFACE;
    v.widgets.inactive.rounding = Rounding::same(6.0);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.hovered.bg_fill = ACCENT_SOFT;
    v.widgets.hovered.rounding = Rounding::same(6.0);
    v.widgets.active.fg_stroke = Stroke::new(1.0, ACCENT_HOVER);
    v.widgets.active.bg_fill = Color32::from_rgb(222, 233, 244);
    v.widgets.active.rounding = Rounding::same(6.0);
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    ctx.set_visuals(v);

    let mut style: Style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.window_margin = Margin::same(10.0);
    style.spacing.button_padding = Vec2::new(10.0, 4.0);
    ctx.set_style(style);
}

/// 统一分组卡片容器：白底 + 浅灰边框 + 6px 圆角 + 10px 内边距。
pub fn card_frame(style: &Style) -> Frame {
    Frame::group(style)
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .rounding(Rounding::same(6.0))
        .inner_margin(Margin::same(10.0))
}
