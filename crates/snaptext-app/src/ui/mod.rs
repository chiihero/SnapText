//! `SnapTextApp`（eframe::App）。
//!
//! 热键 → 截图 → 选区 overlay（DU-08）→ OCR/翻译（Orchestrator）→ 译文悬浮卡片（DU-09）。
//! 托盘"设置"打开 DU-14 面板（写回 config.toml）。

pub mod card;
pub mod fonts;
pub mod onboarding;
pub mod overlay;
pub mod settings;
pub mod theme;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use eframe::egui::Pos2;
use global_hotkey::hotkey::HotKey;
use global_hotkey::GlobalHotKeyEvent;
use tokio::sync::mpsc;
use tray_icon::{menu::MenuEvent, TrayIcon, TrayIconEvent};

use crate::clipboard;
use crate::orchestrator::{Command, Event};
use crate::ui::overlay::{show_overlay, OverlayState, SharedOverlay};
use snaptext_core::types::CapturedFrame;
use snaptext_core::Config;

pub struct SnapTextApp {
    _runtime: Arc<tokio::runtime::Runtime>,
    _tray: TrayIcon,
    hotkey_manager: global_hotkey::GlobalHotKeyManager,
    current_hotkey: HotKey,
    cmd_tx: mpsc::Sender<Command>,
    event_rx: mpsc::Receiver<Event>,
    status: String,
    last_original: Option<String>,
    last_translation: Option<String>,
    last_provider: String,
    last_elapsed: Option<String>,
    card_open: bool,
    card_state: Option<card::SharedCard>,
    card_seq: u64,
    last_card_anchor: Option<Pos2>,
    translate_started: Option<Instant>,
    overlay: Option<SharedOverlay>,
    config: Config,
    settings_open: bool,
    settings_state: Option<settings::SharedSettings>,
    onboarding_open: bool,
}

impl SnapTextApp {
    pub fn new(
        runtime: Arc<tokio::runtime::Runtime>,
        tray: TrayIcon,
        hotkey_manager: global_hotkey::GlobalHotKeyManager,
        current_hotkey: HotKey,
        cmd_tx: mpsc::Sender<Command>,
        event_rx: mpsc::Receiver<Event>,
        config: Config,
    ) -> Self {
        Self {
            onboarding_open: !config.general.onboarding_completed,
            _runtime: runtime,
            _tray: tray,
            hotkey_manager,
            current_hotkey,
            cmd_tx,
            event_rx,
            status: "就绪。按 Ctrl+Alt+Q 截图，框选文字区域。".into(),
            last_original: None,
            last_translation: None,
            last_provider: String::new(),
            last_elapsed: None,
            card_open: false,
            card_state: None,
            card_seq: 0,
            last_card_anchor: None,
            translate_started: None,
            overlay: None,
            config,
            settings_open: false,
            settings_state: None,
        }
    }

    fn start_overlay(&mut self, frames: Vec<CapturedFrame>) {
        if let Some(frame) = frames.into_iter().next() {
            let state = OverlayState {
                monitor: frame.monitor.id.clone(),
                origin: Pos2::new(frame.monitor.x as f32, frame.monitor.y as f32),
                scale: frame.monitor.scale,
                rgba: frame.image.clone(),
                texture: None,
                drag_start: None,
                drag_cur: None,
                result: None,
                dim_alpha: self.config.ui.overlay_dim_alpha,
                selected_anchor: None,
            };
            self.overlay = Some(Arc::new(Mutex::new(state)));
        }
    }

    /// 按当前 `config.hotkey` 重新注册全局热键；失败则保留旧热键，仅记日志。
    fn re_register_hotkey(&mut self) {
        let old = self.current_hotkey;
        match crate::hotkey::re_register(&self.hotkey_manager, old, &self.config.hotkey) {
            Ok(new) => self.current_hotkey = new,
            Err(e) => tracing::warn!(error = %e, "热键重注册失败，保留旧热键"),
        }
    }

    /// 触发一次截图（热键与主面板「开始截图」按钮共用）。
    fn trigger_capture_from_ui(&mut self) {
        self.status = "截图中…".into();
        self.last_original = None;
        self.last_translation = None;
        self.card_open = false;
        self.card_state = None;
        self.overlay = None;
        let _ = self.cmd_tx.try_send(Command::TriggerCapture);
    }
}

impl eframe::App for SnapTextApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(100));

        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            tracing::info!(?event.state, id = event.id, "热键触发");
            self.trigger_capture_from_ui();
        }

        while let Ok(event) = MenuEvent::receiver().try_recv() {
            match event.id.0.as_str() {
                "show" => ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true)),
                "settings" => self.settings_open = true,
                "history" => self.status = "历史面板（DU-15 GUI）开发中；读取接口已就绪".into(),
                "quit" => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                other => tracing::warn!(%other, "未知菜单项"),
            }
        }
        while TrayIconEvent::receiver().try_recv().is_ok() {}

        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                Event::StateChanged(s) => {
                    if matches!(s, snaptext_core::types::AppState::Translating) {
                        self.translate_started = Some(Instant::now());
                    }
                    self.status = format!("状态：{s:?}");
                }
                Event::Captured(frames) => {
                    self.status = "已截图，请在屏幕上框选文字区域（Esc 取消）".into();
                    self.start_overlay(frames);
                }
                Event::OcrDone(texts) => {
                    self.last_original = Some(texts.join("\n"));
                    self.status = "OCR 完成，翻译中…".into();
                }
                Event::TranslateDone(resp) => {
                    let elapsed = self
                        .translate_started
                        .map(|t| format!("{:.2}s", t.elapsed().as_secs_f64()));
                    if self.config.ui.auto_copy_translation {
                        let _ = clipboard::set_text(&resp.translated_text);
                    }
                    let anchor = self
                        .last_card_anchor
                        .unwrap_or_else(|| Pos2::new(120.0, 120.0));
                    self.card_seq = self.card_seq.wrapping_add(1);
                    self.card_state = Some(Arc::new(Mutex::new(card::CardState::new(
                        self.last_original.clone().unwrap_or_default(),
                        resp.translated_text.clone(),
                        resp.provider.to_string(),
                        elapsed.clone(),
                        anchor,
                        self.config.ui.show_original,
                        self.config.ui.card_font_size,
                    ))));
                    self.last_translation = Some(resp.translated_text);
                    self.last_provider = resp.provider.to_string();
                    self.last_elapsed = elapsed;
                    self.status = "翻译完成。".into();
                    self.card_open = true;
                }
                Event::Error(e) => self.status = format!("错误：{e}"),
            }
        }

        // 选区 overlay。
        if let Some(shared) = self.overlay.take() {
            let alive = show_overlay(ctx, shared.clone(), &self.cmd_tx);
            if alive {
                self.overlay = Some(shared);
            } else {
                if let Some(anchor) = shared.lock().unwrap().selected_anchor {
                    self.last_card_anchor = Some(anchor);
                }
                self.status = "选区完成，识别中…".into();
            }
        }

        // 首次启动引导页。
        if self.onboarding_open {
            let mut open = self.onboarding_open;
            let outcome = onboarding::show_onboarding(ctx, &mut open, &mut self.config);
            self.onboarding_open = open;
            if outcome.completed || outcome.skipped {
                // 完成或跳过都标记已完成，后续启动不再弹。
                self.config.general.onboarding_completed = true;
                match self.config.save() {
                    Ok(()) => {
                        self.status = if outcome.skipped {
                            "已跳过引导，可稍后在托盘『设置』中配置。".into()
                        } else {
                            "配置已保存。".into()
                        };
                        if outcome.completed {
                            // 即时下发翻译配置与目标语言，并重注册热键（无需重启）。
                            let _ = self.cmd_tx.try_send(Command::UpdateTranslateConfig(
                                self.config.translate.clone(),
                            ));
                            let _ = self.cmd_tx.try_send(Command::UpdateTargetLang(
                                self.config.translate.target_lang,
                            ));
                            self.re_register_hotkey();
                        }
                    }
                    Err(e) => self.status = format!("保存配置失败：{e}"),
                }
            }
        }

        // 设置面板（独立 viewport）。
        if self.settings_open && self.settings_state.is_none() {
            self.settings_state = Some(Arc::new(Mutex::new(settings::SettingsState::new(
                &self.config,
            ))));
        }
        if let Some(state) = self.settings_state.clone() {
            settings::show_settings(ctx, &state);
            let outcome = state.lock().unwrap().outcome;
            if matches!(outcome, settings::SettingsOutcome::Save) {
                let draft = state.lock().unwrap().draft.clone();
                self.config = draft;
                self.status = "配置已保存。".into();
                match self.config.save() {
                    Ok(()) => {
                        // 即时下发翻译配置与目标语言，并重注册热键（无需重启）。
                        let _ = self.cmd_tx.try_send(Command::UpdateTranslateConfig(
                            self.config.translate.clone(),
                        ));
                        let _ = self
                            .cmd_tx
                            .try_send(Command::UpdateTargetLang(self.config.translate.target_lang));
                        self.re_register_hotkey();
                    }
                    Err(e) => self.status = format!("保存配置失败：{e}"),
                }
                self.settings_open = false;
                self.settings_state = None;
            } else if matches!(outcome, settings::SettingsOutcome::Cancel) {
                self.settings_open = false;
                self.settings_state = None;
            }
        }

        // 译文悬浮卡片（独立 viewport，跟随选区位置）。
        if self.card_open {
            if let Some(state) = self.card_state.clone() {
                let alive = card::show_card_viewport(ctx, state, self.card_seq);
                if !alive {
                    self.card_open = false;
                    self.card_state = None;
                }
            } else {
                self.card_open = false;
            }
        }

        // 主面板。
        if self.overlay.is_none() {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("SnapText");
                ui.label(&self.status);
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("开始截图").clicked() {
                        self.trigger_capture_from_ui();
                    }
                    ui.label(
                        egui::RichText::new(format!("或按 {}", self.config.hotkey.trigger))
                            .small()
                            .weak(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("设置").clicked() {
                            self.settings_open = true;
                        }
                    });
                });
                ui.add_space(4.0);
                ui.collapsing("原文", |ui| match &self.last_original {
                    Some(t) => ui.label(t),
                    None => ui.weak("（无）"),
                });
                ui.collapsing("译文", |ui| match &self.last_translation {
                    Some(t) => ui.label(t),
                    None => ui.weak("（无）"),
                });
            });
        }
    }
}
