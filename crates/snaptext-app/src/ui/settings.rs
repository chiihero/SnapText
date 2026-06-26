//! 设置面板（独立 viewport：自绘标题栏 + 左侧导航 + 右侧分组）。
//!
//! 独立 OS 窗口，可拖出主窗口。编辑 config 的草稿副本（`Arc<Mutex>`），保存时由
//! 主程序写回 + 下发 Orchestrator。分类对标 PixPin/Umi-OCR：
//! 通用 / 快捷键 / 截图 / 文字识别 / 翻译 / 界面显示 / 历史记录 / 关于。

use std::sync::{Arc, Mutex};

use eframe::egui::{self, Context, Key, Layout, Ui, ViewportBuilder, ViewportCommand, ViewportId};

use snaptext_core::config::{Config, DeepLPlan, ProviderKind, Tier};
use snaptext_core::types::Lang;

use crate::ui::theme;

const WIN_W: f32 = 640.0;
const WIN_H: f32 = 480.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
    Hotkey,
    Capture,
    Ocr,
    Translate,
    Ui,
    History,
    About,
}

impl SettingsTab {
    const ALL: [SettingsTab; 8] = [
        SettingsTab::General,
        SettingsTab::Hotkey,
        SettingsTab::Capture,
        SettingsTab::Ocr,
        SettingsTab::Translate,
        SettingsTab::Ui,
        SettingsTab::History,
        SettingsTab::About,
    ];
    fn label(self) -> &'static str {
        match self {
            SettingsTab::General => "通用",
            SettingsTab::Hotkey => "快捷键",
            SettingsTab::Capture => "截图",
            SettingsTab::Ocr => "文字识别",
            SettingsTab::Translate => "翻译",
            SettingsTab::Ui => "界面显示",
            SettingsTab::History => "历史记录",
            SettingsTab::About => "关于",
        }
    }
}

/// 设置面板交互结果（主程序轮询）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsOutcome {
    /// 仍在编辑。
    None,
    /// 用户点了「保存」。
    Save,
    /// 用户点了「取消」或标题栏 ✕。
    Cancel,
}

/// 设置面板跨帧共享状态。
pub struct SettingsState {
    /// 编辑中的配置副本，保存时整体写回。
    pub draft: Config,
    active_tab: SettingsTab,
    pub outcome: SettingsOutcome,
    show_deepseek_key: bool,
    show_deepl_key: bool,
    show_microsoft_key: bool,
}

impl SettingsState {
    pub fn new(config: &Config) -> Self {
        Self {
            draft: config.clone(),
            active_tab: SettingsTab::Translate,
            outcome: SettingsOutcome::None,
            show_deepseek_key: false,
            show_deepl_key: false,
            show_microsoft_key: false,
        }
    }
}

pub type SharedSettings = Arc<Mutex<SettingsState>>;

/// 驱动设置 viewport。主程序每帧调用，按 [`SettingsOutcome`] 决定保存/取消。
pub fn show_settings(ctx: &Context, state: &SharedSettings) {
    let vid = ViewportId::from_hash_of("snaptext-settings");
    let st = Arc::clone(state);
    ctx.show_viewport_deferred(
        vid,
        ViewportBuilder::default()
            .with_title("SnapText 设置")
            .with_inner_size([WIN_W, WIN_H])
            .with_min_inner_size([480.0, 360.0])
            .with_resizable(true),
        move |vctx, _class| {
            render(vctx, &st);
        },
    );
}

fn render(vctx: &Context, state: &SharedSettings) {
    // [诊断-临时] 确认 deferred viewport cb 是否持续执行（每 30 次一条；若 cb 不执行则无输出）。
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let frame = N.fetch_add(1, Ordering::Relaxed);
    if frame % 30 == 0 {
        tracing::info!(target: "snaptext::diag", frame, "settings cb alive");
    }

    // 用户点 OS 标题栏 ✕ → close_requested；或按 Esc → 取消（主程序下一帧停止驱动）。
    // [诊断-临时] 同时记录 ViewportEvent::Close 计数，对比 flag vs event 哪个被派发。
    let (close_requested, close_event_n) = vctx.input(|i| {
        let vp = i.viewport();
        let n = vp
            .events
            .iter()
            .filter(|e| matches!(e, egui::ViewportEvent::Close))
            .count();
        (vp.close_requested(), n)
    });
    let esc = vctx.input(|i| i.key_pressed(Key::Escape));
    if close_requested || esc {
        tracing::info!(
            target: "snaptext::diag",
            close_requested,
            close_event_n,
            esc,
            "settings 触发关闭"
        );
        state.lock().unwrap().outcome = SettingsOutcome::Cancel;
        vctx.send_viewport_cmd(ViewportCommand::Close);
    }

    let mut s = state.lock().unwrap();

    // 底部保存/取消。
    egui::TopBottomPanel::bottom("settings_buttons")
        .exact_height(42.0)
        .show(vctx, |ui| {
            ui.add_space(6.0);
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("保存").clicked() {
                    tracing::info!(target: "snaptext::diag", "settings 保存 clicked");
                    s.outcome = SettingsOutcome::Save;
                }
                if ui.button("取消").clicked() {
                    tracing::info!(target: "snaptext::diag", "settings 取消 clicked");
                    s.outcome = SettingsOutcome::Cancel;
                }
            });
        });

    // 左侧导航。
    let active = s.active_tab;
    egui::SidePanel::left("settings_nav")
        .resizable(false)
        .exact_width(128.0)
        .show(vctx, |ui| {
            ui.add_space(6.0);
            for tab in SettingsTab::ALL {
                let selected = active == tab;
                if ui.selectable_label(selected, tab.label()).clicked() {
                    s.active_tab = tab;
                }
            }
        });

    // 右侧内容。
    egui::CentralPanel::default().show(vctx, |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.heading(active.label());
                ui.add_space(8.0);
                match active {
                    SettingsTab::General => section_general(ui, &mut s.draft),
                    SettingsTab::Hotkey => section_hotkey(ui, &mut s.draft),
                    SettingsTab::Capture => section_capture(ui, &mut s.draft),
                    SettingsTab::Ocr => section_ocr(ui, &mut s.draft),
                    SettingsTab::Translate => section_translate(ui, &mut s),
                    SettingsTab::Ui => section_ui(ui, &mut s.draft),
                    SettingsTab::History => section_history(ui, &mut s.draft),
                    SettingsTab::About => section_about(ui),
                }
            });
    });
}

// ===== 各分类内容 =====

fn section_general(ui: &mut Ui, cfg: &mut Config) {
    theme::card_frame(ui.style()).show(ui, |ui| {
        ui.checkbox(
            &mut cfg.ui.minimize_to_tray_on_close,
            "关闭主窗口时最小化到托盘（而非退出）",
        );
    });
}

fn section_hotkey(ui: &mut Ui, cfg: &mut Config) {
    theme::card_frame(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label("触发截图：");
            ui.text_edit_singleline(&mut cfg.hotkey.trigger);
        });
        ui.horizontal(|ui| {
            ui.label("取消选区：");
            ui.text_edit_singleline(&mut cfg.hotkey.cancel);
        });
        ui.label(
            egui::RichText::new("格式如 Ctrl+Alt+Q；保存后即时生效。")
                .small()
                .color(theme::TEXT_WEAK),
        );
    });
}

fn section_capture(ui: &mut Ui, cfg: &mut Config) {
    theme::card_frame(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label("选区蒙版不透明度：");
            ui.add(egui::Slider::new(&mut cfg.ui.overlay_dim_alpha, 0.0..=1.0));
        });
    });
}

fn section_ocr(ui: &mut Ui, cfg: &mut Config) {
    theme::card_frame(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label("档位：");
            ui.radio_value(&mut cfg.ocr.tier, Tier::Medium, "medium（精度）");
            ui.radio_value(&mut cfg.ocr.tier, Tier::Small, "small（速度）");
        });
        ui.checkbox(
            &mut cfg.ocr.postprocess,
            "识别结果后处理（去空格 / 合并换行）",
        );
        ui.label(
            egui::RichText::new("档位切换需重启生效。")
                .small()
                .color(theme::TEXT_WEAK),
        );
    });
}

fn section_translate(ui: &mut Ui, s: &mut SettingsState) {
    theme::card_frame(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label("引擎：");
            ui.radio_value(
                &mut s.draft.translate.provider,
                ProviderKind::DeepSeek,
                "DeepSeek",
            );
            ui.radio_value(
                &mut s.draft.translate.provider,
                ProviderKind::DeepL,
                "DeepL",
            );
            ui.radio_value(
                &mut s.draft.translate.provider,
                ProviderKind::Microsoft,
                "Microsoft",
            );
        });
        ui.horizontal(|ui| {
            ui.label("目标语言：");
            egui::ComboBox::from_id_salt("settings_target_lang")
                .selected_text(lang_label(s.draft.translate.target_lang))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.draft.translate.target_lang, Lang::Zh, "中文");
                    ui.selectable_value(&mut s.draft.translate.target_lang, Lang::En, "英文");
                    ui.selectable_value(&mut s.draft.translate.target_lang, Lang::Ja, "日文");
                });
        });

        ui.add_space(4.0);
        match s.draft.translate.provider {
            ProviderKind::DeepSeek => {
                let key = s
                    .draft
                    .translate
                    .deepseek
                    .api_key
                    .get_or_insert_with(String::new);
                ui.horizontal(|ui| {
                    ui.label("API Key：");
                    key_input(ui, key, &mut s.show_deepseek_key);
                });
            }
            ProviderKind::DeepL => {
                ui.horizontal(|ui| {
                    ui.label("套餐：");
                    ui.radio_value(&mut s.draft.translate.deepl.plan, DeepLPlan::Free, "Free");
                    ui.radio_value(&mut s.draft.translate.deepl.plan, DeepLPlan::Pro, "Pro");
                });
                let key = s
                    .draft
                    .translate
                    .deepl
                    .api_key
                    .get_or_insert_with(String::new);
                ui.horizontal(|ui| {
                    ui.label("API Key：");
                    key_input(ui, key, &mut s.show_deepl_key);
                });
            }
            ProviderKind::Microsoft => {
                ui.horizontal(|ui| {
                    ui.label("区域：");
                    ui.text_edit_singleline(&mut s.draft.translate.microsoft.region);
                });
                let key = s
                    .draft
                    .translate
                    .microsoft
                    .api_key
                    .get_or_insert_with(String::new);
                ui.horizontal(|ui| {
                    ui.label("Key：");
                    key_input(ui, key, &mut s.show_microsoft_key);
                });
            }
        }
    });
}

fn key_input(ui: &mut Ui, key: &mut String, show: &mut bool) {
    ui.add(
        egui::TextEdit::singleline(key)
            .password(!*show)
            .desired_width(240.0),
    );
    if ui.button(if *show { "隐藏" } else { "显示" }).clicked() {
        *show = !*show;
    }
}

fn section_ui(ui: &mut Ui, cfg: &mut Config) {
    theme::card_frame(ui.style()).show(ui, |ui| {
        ui.checkbox(&mut cfg.ui.auto_copy_translation, "翻译后自动复制译文");
        ui.checkbox(&mut cfg.ui.show_original, "悬浮卡片显示原文");
        ui.horizontal(|ui| {
            ui.label("卡片字体大小：");
            ui.add(egui::Slider::new(&mut cfg.ui.card_font_size, 10.0..=24.0).text("pt"));
        });
    });
}

fn section_history(ui: &mut Ui, cfg: &mut Config) {
    theme::card_frame(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label("保留天数：");
            ui.add(egui::Slider::new(&mut cfg.history.retention_days, 1..=365).text("天"));
        });
        ui.horizontal(|ui| {
            ui.label("最大记录数：");
            ui.add(egui::Slider::new(&mut cfg.history.max_records, 100..=20000).text("条"));
        });
        ui.checkbox(
            &mut cfg.history.auto_clean_on_start,
            "启动时自动清理过期记录",
        );
    });
}

fn section_about(ui: &mut Ui) {
    theme::card_frame(ui.style()).show(ui, |ui| {
        ui.heading("SnapText");
        ui.label(egui::RichText::new("Windows 11 截图 OCR + 翻译工具").color(theme::TEXT_WEAK));
        ui.add_space(6.0);
        let cfg_path = snaptext_core::config::config_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "(未知)".into());
        ui.label(
            egui::RichText::new(format!("配置文件：{cfg_path}"))
                .small()
                .color(theme::TEXT_WEAK),
        );
    });
}

fn lang_label(lang: Lang) -> &'static str {
    match lang {
        Lang::Zh => "中文",
        Lang::En => "英文",
        Lang::Ja => "日文",
        Lang::Auto => "自动",
    }
}
