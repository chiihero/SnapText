//! DU-08：Snipaste 风格选区 overlay（eframe 0.30 Viewport）。
//!
//! 选区阶段全屏置顶无边框窗口，绘制截图背景 + 50% 蒙版；鼠标框选实时矩形 +
//! 尺寸标注；Esc 取消；鼠标抬起发 `Command::RegionSelected`。状态跨帧用
//! `Arc<Mutex<OverlayState>>`（deferred 闭包要求 Send+Sync+'static）。

use std::sync::{Arc, Mutex};

use eframe::egui::{
    self, Color32, Context, Key, LayerId, Order, Pos2, Rect, Rounding, Stroke, ViewportBuilder,
    ViewportCommand, ViewportId,
};
use tokio::sync::mpsc;

use snaptext_core::types::{Bbox, MonitorId};

use crate::orchestrator::Command;

/// overlay 跨帧共享状态。
pub struct OverlayState {
    pub monitor: MonitorId,
    /// 虚拟桌面原点（px）。
    pub origin: Pos2,
    pub scale: f32,
    pub rgba: image::RgbaImage,
    pub texture: Option<egui::TextureHandle>,
    pub drag_start: Option<Pos2>,
    pub drag_cur: Option<Pos2>,
    /// `None`=进行中；`Some(Some)`=完成；`Some(None)`=取消。
    pub result: Option<Option<Bbox>>,
    /// 选区蒙版不透明度（0.0~1.0），来自 `config.ui.overlay_dim_alpha`。
    pub dim_alpha: f32,
    /// 选区右下角虚拟桌面坐标，框选完成时写入，供卡片定位（`ui/card.rs`）。
    pub selected_anchor: Option<Pos2>,
}

pub type SharedOverlay = Arc<Mutex<OverlayState>>;

/// 每帧调用。返回 false 表示已完成/取消（主 App 停止调用，窗口自然消失）。
pub fn show_overlay(ctx: &Context, state: SharedOverlay, cmd_tx: &mpsc::Sender<Command>) -> bool {
    let vid = ViewportId::from_hash_of("snaptext-overlay");
    let st = Arc::clone(&state);
    let tx = cmd_tx.clone();
    ctx.show_viewport_deferred(
        vid,
        ViewportBuilder::default()
            .with_title("SnapText 选区")
            .with_decorations(false)
            .with_fullscreen(true)
            .with_resizable(false)
            .with_always_on_top(),
        move |vctx, _class| {
            render(vctx, &st);
            handle_input(vctx, &st, &tx);
        },
    );
    state.lock().unwrap().result.is_none()
}

fn render(ctx: &Context, state: &SharedOverlay) {
    // [诊断-临时] 确认 deferred viewport cb 是否持续执行。
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let frame = N.fetch_add(1, Ordering::Relaxed);
    if frame % 30 == 0 {
        tracing::info!(target: "snaptext::diag", frame, "overlay cb alive");
    }
    let mut s = state.lock().unwrap();
    let scale = s.scale;
    if s.texture.is_none() {
        let size = [s.rgba.width() as usize, s.rgba.height() as usize];
        let image = egui::ColorImage::from_rgba_unmultiplied(size, s.rgba.as_raw());
        s.texture =
            Some(ctx.load_texture("snaptext-overlay-bg", image, egui::TextureOptions::LINEAR));
    }
    let tex = s.texture.as_ref().unwrap();
    let screen = ctx.screen_rect();
    let p = ctx.layer_painter(LayerId::new(Order::Background, "overlay-bg".into()));
    p.image(
        tex.id(),
        screen,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );

    let dim = Color32::from_black_alpha((s.dim_alpha * 255.0).clamp(0.0, 255.0) as u8);
    if let (Some(a), Some(b)) = (s.drag_start, s.drag_cur) {
        let sel = Rect::from_two_pos(a, b);
        // 选区外四块蒙版（选区内透出原图）。
        p.rect_filled(
            Rect::from_min_max(screen.min, Pos2::new(sel.max.x, screen.min.y)),
            Rounding::ZERO,
            dim,
        );
        p.rect_filled(
            Rect::from_min_max(Pos2::new(sel.min.x, sel.max.y), screen.max),
            Rounding::ZERO,
            dim,
        );
        p.rect_filled(
            Rect::from_min_max(
                Pos2::new(screen.min.x, sel.min.y),
                Pos2::new(sel.min.x, sel.max.y),
            ),
            Rounding::ZERO,
            dim,
        );
        p.rect_filled(
            Rect::from_min_max(
                Pos2::new(sel.max.x, sel.min.y),
                Pos2::new(screen.max.x, sel.max.y),
            ),
            Rounding::ZERO,
            dim,
        );
        // 十字辅助线：贯穿全屏，经过当前鼠标点，便于对齐。
        let cross = Color32::from_rgba_unmultiplied(0, 120, 215, 110);
        p.line_segment(
            [Pos2::new(screen.min.x, b.y), Pos2::new(screen.max.x, b.y)],
            Stroke::new(1.0, cross),
        );
        p.line_segment(
            [Pos2::new(b.x, screen.min.y), Pos2::new(b.x, screen.max.y)],
            Stroke::new(1.0, cross),
        );
        p.rect_stroke(
            sel,
            Rounding::ZERO,
            Stroke::new(1.5, Color32::from_rgb(0, 120, 215)),
        );
        let label = format!(
            "{}×{} px",
            (sel.width() * scale) as i32,
            (sel.height() * scale) as i32
        );
        p.text(
            sel.max + egui::Vec2::new(4.0, 4.0),
            egui::Align2::LEFT_TOP,
            label,
            egui::FontId::proportional(13.0),
            Color32::WHITE,
        );
    } else {
        p.rect_filled(screen, Rounding::ZERO, dim);
        // 顶部操作提示。
        p.text(
            Pos2::new(screen.center().x, screen.min.y + 28.0),
            egui::Align2::CENTER_TOP,
            "拖动鼠标框选文字区域 · Esc 取消",
            egui::FontId::proportional(15.0),
            Color32::WHITE,
        );
    }
}

fn handle_input(ctx: &Context, state: &SharedOverlay, cmd_tx: &mpsc::Sender<Command>) {
    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        state.lock().unwrap().result = Some(None);
        let _ = cmd_tx.try_send(Command::Cancel);
        ctx.send_viewport_cmd(ViewportCommand::Close);
        return;
    }
    let (cur, down, released) = ctx.input(|i| {
        (
            i.pointer.interact_pos(),
            i.pointer.primary_down(),
            i.pointer.primary_released(),
        )
    });
    let mut s = state.lock().unwrap();
    if down && s.drag_start.is_none() {
        tracing::info!(target: "snaptext::diag", "overlay 拖拽开始");
        s.drag_start = cur;
        s.drag_cur = cur;
    }
    if s.drag_start.is_some() && down {
        s.drag_cur = cur;
    }
    if released {
        let finished = if let (Some(a), Some(b)) = (s.drag_start, s.drag_cur) {
            let sel = Rect::from_two_pos(a, b);
            if sel.width() > 2.0 && sel.height() > 2.0 {
                let bbox = Bbox {
                    x: (s.origin.x + sel.min.x * s.scale) as i32,
                    y: (s.origin.y + sel.min.y * s.scale) as i32,
                    w: (sel.width() * s.scale) as i32,
                    h: (sel.height() * s.scale) as i32,
                };
                let monitor = s.monitor.clone();
                s.selected_anchor = Some(Pos2::new(
                    s.origin.x + sel.max.x * s.scale,
                    s.origin.y + sel.max.y * s.scale,
                ));
                s.result = Some(Some(bbox));
                Some((monitor, bbox))
            } else {
                s.result = Some(None);
                None
            }
        } else {
            s.result = Some(None);
            None
        };
        drop(s);
        if let Some((monitor, bbox)) = finished {
            let _ = cmd_tx.try_send(Command::RegionSelected(monitor, bbox));
        }
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }
}
