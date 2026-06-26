//! 译文悬浮卡片（独立 always-on-top viewport，跟随选区位置）。
//!
//! 卡片是一个无边框置顶 OS 窗口，定位到选区右下角（近右/下屏边自动翻向），固定显示。
//! 每次翻译用递增 `ViewportId`，确保新卡片重新定位而非复用旧窗口位置。
//! click-through 推迟；auto_copy 见 `ui/mod.rs` 的 `Event::TranslateDone`。

use std::sync::{Arc, Mutex};

use eframe::egui::{
    self, Context, Key, Pos2, Rect, Stroke, ViewportBuilder, ViewportCommand, ViewportId,
};

use crate::clipboard;
use crate::ui::theme;

/// 卡片预估尺寸（含内边距）。
const CARD_W: f32 = 384.0;
const CARD_H: f32 = 230.0;

/// 卡片跨帧共享状态（`Send + Sync`，供 deferred viewport 闭包跨帧持有）。
pub struct CardState {
    original: String,
    translation: String,
    provider: String,
    elapsed: Option<String>,
    /// 选区右下角虚拟桌面坐标，卡片定位锚点。
    anchor: Pos2,
    show_original: bool,
    font_size: f32,
    /// 窗口基准位置（首次 `fit_position` 计算后固定）。
    base_pos: Pos2,
    positioned: bool,
    close_requested: bool,
}

impl CardState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        original: String,
        translation: String,
        provider: String,
        elapsed: Option<String>,
        anchor: Pos2,
        show_original: bool,
        font_size: f32,
    ) -> Self {
        Self {
            original,
            translation,
            provider,
            elapsed,
            anchor,
            show_original,
            font_size,
            base_pos: anchor,
            positioned: false,
            close_requested: false,
        }
    }
}

pub type SharedCard = Arc<Mutex<CardState>>;

/// 驱动卡片 viewport。返回 false 表示用户请求关闭（主 App 停止驱动，窗口自然消失）。
pub fn show_card_viewport(ctx: &Context, state: SharedCard, seq: u64) -> bool {
    // 每次翻译用递增 ViewportId，让新卡片重新定位。
    let vid = ViewportId::from_hash_of(format!("snaptext-card-{seq}"));

    // 首次计算基准位置（含边界检测）。
    {
        let mut s = state.lock().unwrap();
        if !s.positioned {
            let screen = ctx.input(|i| i.screen_rect);
            s.base_pos = fit_position(s.anchor, CARD_W, CARD_H, screen);
            s.positioned = true;
        }
    }

    let init_pos = state.lock().unwrap().base_pos;
    let st = Arc::clone(&state);
    ctx.show_viewport_deferred(
        vid,
        ViewportBuilder::default()
            .with_title("SnapText 译文")
            .with_decorations(false)
            .with_resizable(false)
            .with_always_on_top()
            .with_inner_size([CARD_W, CARD_H])
            .with_position(init_pos),
        move |vctx, _class| {
            render(vctx, &st);
        },
    );
    !state.lock().unwrap().close_requested
}

fn render(vctx: &Context, state: &SharedCard) {
    // [诊断-临时] 确认 deferred viewport cb 是否持续执行。
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let frame = N.fetch_add(1, Ordering::Relaxed);
    if frame % 30 == 0 {
        tracing::info!(target: "snaptext::diag", frame, "card cb alive");
    }

    // 维持窗口位置（首次 fit_position 计算后固定，每帧下发确保就位）。
    {
        let s = state.lock().unwrap();
        vctx.send_viewport_cmd(ViewportCommand::OuterPosition(s.base_pos));
    }

    // Esc 关闭。
    if vctx.input(|i| i.key_pressed(Key::Escape)) {
        state.lock().unwrap().close_requested = true;
        return;
    }

    // 取出渲染数据，避免持有锁跨 egui 调用。
    let (original, translation, provider, elapsed, show_original, font_size) = {
        let s = state.lock().unwrap();
        (
            s.original.clone(),
            s.translation.clone(),
            s.provider.clone(),
            s.elapsed.clone(),
            s.show_original,
            s.font_size,
        )
    };

    egui::CentralPanel::default()
        .frame(egui::Frame::default().fill(theme::SURFACE))
        .show(vctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(theme::SURFACE)
                .stroke(Stroke::new(1.0, theme::ACCENT))
                .inner_margin(egui::Margin::same(8.0))
                .show(ui, |ui| {
                    ui.set_width(CARD_W - 24.0);

                    // 原文（可折叠，默认收起）。
                    if show_original && !original.trim().is_empty() {
                        egui::CollapsingHeader::new("原文")
                            .default_open(false)
                            .show(ui, |ui| {
                                ui.label(&original);
                            });
                    }

                    // 译文。
                    ui.label(
                        egui::RichText::new(translation.clone())
                            .strong()
                            .size(font_size),
                    );

                    ui.separator();

                    // 底部：来源/耗时 + 操作按钮。
                    ui.horizontal(|ui| {
                        let mut meta = provider.clone();
                        if let Some(e) = &elapsed {
                            meta.push_str(&format!(" · {e}"));
                        }
                        ui.label(egui::RichText::new(meta).small().weak());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("关闭").clicked() {
                                tracing::info!(target: "snaptext::diag", "card 关闭 clicked");
                                state.lock().unwrap().close_requested = true;
                            }
                            if ui.button("复制原文").clicked() {
                                tracing::info!(target: "snaptext::diag", "card 复制原文 clicked");
                                let _ = clipboard::set_text(&original);
                            }
                            if ui.button("复制译文").clicked() {
                                tracing::info!(target: "snaptext::diag", "card 复制译文 clicked");
                                let _ = clipboard::set_text(&translation);
                            }
                        });
                    });
                });
        });
}

/// 计算卡片窗口位置：默认锚点右下方，近右/下屏边则翻向左/上。
fn fit_position(anchor: Pos2, w: f32, h: f32, screen: Rect) -> Pos2 {
    let mut x = anchor.x + 8.0;
    let mut y = anchor.y + 8.0;
    if x + w > screen.max.x {
        x = anchor.x - w - 8.0;
    }
    if y + h > screen.max.y {
        y = anchor.y - h - 8.0;
    }
    Pos2::new(x.max(screen.min.x), y.max(screen.min.y))
}
