//! 首次启动引导页。
//!
//! 首次启动（`config.general.onboarding_completed == false`）时弹出，
//! 一次配齐热键 / 翻译引擎 / API Key / OCR 档位 / 目标语言。
//! 「完成」与「跳过」都标记引导已完成（不再弹）；是否保存、是否即时下发 Orchestrator
//! 由调用方（`ui/mod.rs`）根据 [`OnboardingOutcome`] 决定——跳过时不写空 Key。

use eframe::egui;

use snaptext_core::config::{Config, ProviderKind, Tier};
use snaptext_core::types::Lang;

/// 引导页交互结果。
#[derive(Default, Clone, Copy)]
pub struct OnboardingOutcome {
    /// 用户点了「完成」（保存配置并即时下发）。
    pub completed: bool,
    /// 用户点了「跳过」（仅标记已完成，不强制保存空 Key）。
    pub skipped: bool,
}

/// 显示首次引导窗口。`cfg` 就地修改，调用方按 outcome 决定保存与下发。
pub fn show_onboarding(
    ctx: &egui::Context,
    open: &mut bool,
    cfg: &mut Config,
) -> OnboardingOutcome {
    let mut outcome = OnboardingOutcome::default();
    egui::Window::new("欢迎使用 SnapText")
        .open(open)
        .resizable(false)
        .collapsible(false)
        .default_width(420.0)
        .show(ctx, |ui| {
            ui.label("截图框选 → OCR → 翻译。下面把几项基本设置一次配齐：");
            ui.add_space(6.0);

            ui.heading("触发热键");
            ui.horizontal(|ui| {
                ui.label("触发:");
                ui.text_edit_singleline(&mut cfg.hotkey.trigger);
            });
            ui.label(
                egui::RichText::new("格式如 Ctrl+Alt+Q；保存后即时生效。")
                    .small()
                    .weak(),
            );

            ui.add_space(4.0);
            ui.heading("翻译");
            ui.horizontal(|ui| {
                ui.label("引擎:");
                ui.radio_value(
                    &mut cfg.translate.provider,
                    ProviderKind::DeepSeek,
                    "DeepSeek",
                );
                ui.radio_value(&mut cfg.translate.provider, ProviderKind::DeepL, "DeepL");
            });
            ui.horizontal(|ui| {
                ui.label("API Key:");
                match cfg.translate.provider {
                    ProviderKind::DeepSeek => {
                        let key = cfg
                            .translate
                            .deepseek
                            .api_key
                            .get_or_insert_with(String::new);
                        ui.text_edit_singleline(key);
                    }
                    ProviderKind::DeepL => {
                        let key = cfg.translate.deepl.api_key.get_or_insert_with(String::new);
                        ui.text_edit_singleline(key);
                    }
                    ProviderKind::Microsoft => {
                        // 引导页暂不暴露 Microsoft，保留默认，引导用户去「设置」。
                        ui.weak("(Microsoft 请稍后在『设置』中配置)");
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("目标语言:");
                egui::ComboBox::from_id_salt("onboarding_target_lang")
                    .selected_text(lang_label(cfg.translate.target_lang))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut cfg.translate.target_lang, Lang::Zh, "中文");
                        ui.selectable_value(&mut cfg.translate.target_lang, Lang::En, "英文");
                        ui.selectable_value(&mut cfg.translate.target_lang, Lang::Ja, "日文");
                    });
            });

            ui.add_space(4.0);
            ui.heading("OCR");
            ui.horizontal(|ui| {
                ui.label("档位:");
                ui.radio_value(&mut cfg.ocr.tier, Tier::Medium, "medium（精度）");
                ui.radio_value(&mut cfg.ocr.tier, Tier::Small, "small（速度）");
            });
            ui.label(egui::RichText::new("档位切换需重启生效。").small().weak());

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("完成").clicked() {
                    outcome.completed = true;
                }
                if ui.button("跳过").clicked() {
                    outcome.skipped = true;
                }
            });
        });

    if outcome.completed || outcome.skipped {
        *open = false;
    }
    outcome
}

fn lang_label(lang: Lang) -> &'static str {
    match lang {
        Lang::Zh => "中文",
        Lang::En => "英文",
        Lang::Ja => "日文",
        Lang::Auto => "自动",
    }
}
