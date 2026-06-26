//! Orchestrator：中央协调器，串联 capture → ocr → translate → history。
//!
//! 持有所有 Provider（`Arc<dyn ...>`），通过 channel 接收 `Command`、发送 `Event`
//! （见 DESIGN §5.5）。CPU 密集的 OCR 在 Provider 内部 `spawn_blocking`。

use std::sync::Arc;

use image::DynamicImage;
use tokio::sync::mpsc;
use tracing::{info, warn};

use snaptext_core::capture::CaptureProvider;
use snaptext_core::config::TranslateConfig;
use snaptext_core::history::{HistoryRecord, HistoryStore};
use snaptext_core::ocr::OcrProvider;
use snaptext_core::translate::{build_provider, TranslationProvider};
use snaptext_core::types::{
    AppState, Bbox, CapturedFrame, Lang, MonitorId, ProviderId, TranslateRequest, TranslateResponse,
};

/// UI/Hotkey → Orchestrator 的命令（见 DESIGN §5.5）。
#[derive(Debug)]
#[allow(dead_code)] // Cancel/RetryTranslate/CopyToClipboard/Shutdown 由 DU-09/10 UI 接入
pub enum Command {
    TriggerCapture,
    RegionSelected(MonitorId, Bbox),
    Cancel,
    RetryTranslate(ProviderId),
    CopyToClipboard(String),
    /// 设置保存后即时更新翻译配置并重建 Provider（缺 Key 则降级为不可用）。
    UpdateTranslateConfig(TranslateConfig),
    /// 即时切换翻译目标语言。
    UpdateTargetLang(Lang),
    Shutdown,
}

/// Orchestrator → UI 的事件。
#[derive(Debug)]
pub enum Event {
    Captured(Vec<CapturedFrame>),
    OcrDone(Vec<String>),
    TranslateDone(TranslateResponse),
    Error(String),
    StateChanged(AppState),
}

/// 中央协调器。
pub struct Orchestrator {
    capture: Arc<dyn CaptureProvider>,
    ocr: Arc<dyn OcrProvider>,
    /// 翻译 Provider；`None` 表示未配置（缺 API Key），翻译时提示去设置。
    translate: Option<Arc<dyn TranslationProvider>>,
    /// 翻译配置；设置保存后经 `UpdateTranslateConfig` 更新并触发 `rebuild_translate`。
    translate_config: TranslateConfig,
    /// 共享 HTTP 客户端，供 `rebuild_translate` 构造 Provider。
    client: reqwest::Client,
    history: Arc<dyn HistoryStore>,
    /// 最近一次 `capture_all` 的帧缓存，供 `RegionSelected` 裁剪。
    captured: Vec<CapturedFrame>,
    state: AppState,
    target_lang: Lang,
    ocr_postprocess: bool,
}

impl Orchestrator {
    pub fn new(
        capture: Arc<dyn CaptureProvider>,
        ocr: Arc<dyn OcrProvider>,
        translate: Option<Arc<dyn TranslationProvider>>,
        history: Arc<dyn HistoryStore>,
        ocr_postprocess: bool,
        translate_config: TranslateConfig,
        client: reqwest::Client,
    ) -> Self {
        Self {
            target_lang: translate_config.target_lang,
            capture,
            ocr,
            translate,
            translate_config,
            client,
            history,
            captured: Vec::new(),
            state: AppState::Idle,
            ocr_postprocess,
        }
    }

    /// 处理一条命令，返回产生的事件序列。
    pub async fn handle(&mut self, cmd: Command) -> Vec<Event> {
        match cmd {
            Command::TriggerCapture => self.trigger_capture().await,
            Command::RegionSelected(monitor, bbox) => self.region_selected(monitor, bbox).await,
            Command::Cancel | Command::Shutdown => {
                self.captured.clear();
                self.set_state(AppState::Idle)
            }
            Command::UpdateTranslateConfig(cfg) => {
                self.translate_config = cfg;
                self.rebuild_translate();
                Vec::new()
            }
            Command::UpdateTargetLang(lang) => {
                self.target_lang = lang;
                Vec::new()
            }
            Command::RetryTranslate(_) | Command::CopyToClipboard(_) => {
                // DU-09/14 接入完整交互后实现。
                Vec::new()
            }
        }
    }

    /// 后台主循环：从 `cmd_rx` 取命令，处理，把事件发到 `event_tx`。
    pub fn run(mut self, mut cmd_rx: mpsc::Receiver<Command>, event_tx: mpsc::Sender<Event>) {
        tokio::spawn(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                if matches!(cmd, Command::Shutdown) {
                    break;
                }
                let events = self.handle(cmd).await;
                for event in events {
                    if event_tx.send(event).await.is_err() {
                        break;
                    }
                }
            }
        });
    }

    async fn trigger_capture(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        match self.capture.capture_all().await {
            Ok(frames) => {
                events.push(Event::Captured(frames.clone()));
                self.captured = frames;
                events.extend(self.set_state(AppState::Selecting));
            }
            Err(e) => events.push(Event::Error(e.to_string())),
        }
        events
    }

    async fn region_selected(&mut self, monitor: MonitorId, bbox: Bbox) -> Vec<Event> {
        // 找帧 + 裁剪在块内完成，释放 self.captured 不可变借用后再 set_state（可变借用）。
        let crop = {
            let frame = match self.captured.iter().find(|f| f.monitor.id == monitor) {
                Some(f) => f,
                None => return vec![Event::Error(format!("找不到显示器 {monitor} 的缓存帧"))],
            };
            match crop_frame(frame, bbox) {
                Ok(img) => img,
                Err(e) => return vec![Event::Error(e)],
            }
        };

        let mut events = Vec::new();
        events.extend(self.set_state(AppState::Recognizing));
        let lines = match self.ocr.recognize(&crop, Lang::Auto).await {
            Ok(ls) => ls,
            Err(e) => {
                events.push(Event::Error(e.to_string()));
                return events;
            }
        };
        let ocr_pp = self.ocr_postprocess;
        let texts: Vec<String> = lines
            .iter()
            .map(|l| {
                if ocr_pp {
                    snaptext_core::ocr::postprocess::clean_ocr_text(&l.text)
                } else {
                    l.text.clone()
                }
            })
            .collect();
        let original = texts.join("\n");
        events.push(Event::OcrDone(texts));

        if original.trim().is_empty() {
            events.extend(self.set_state(AppState::Showing));
            return events;
        }

        events.extend(self.set_state(AppState::Translating));
        let provider = match self.translate.clone() {
            Some(p) => p,
            None => {
                events.push(Event::Error(
                    "未配置翻译 API Key，请在托盘『设置』中填写并保存".into(),
                ));
                return events;
            }
        };
        let req = TranslateRequest {
            text: original.clone(),
            source: Lang::Auto,
            target: self.target_lang,
            context_hint: None,
            glossary: None,
        };
        match provider.translate(req).await {
            Ok(mut resp) => {
                if self.translate_config.postprocess {
                    resp.translated_text = snaptext_core::translate::postprocess::clean_translation(
                        &resp.translated_text,
                    );
                }
                let response_clone = resp.clone();
                events.push(Event::TranslateDone(resp));
                if let Err(e) = self
                    .history_insert(&response_clone, &original, monitor, bbox)
                    .await
                {
                    warn!(error = %e, "写入历史失败");
                }
                events.extend(self.set_state(AppState::Showing));
            }
            Err(e) => events.push(Event::Error(e.to_string())),
        }
        events
    }

    async fn history_insert(
        &self,
        resp: &TranslateResponse,
        original: &str,
        monitor: MonitorId,
        bbox: Bbox,
    ) -> Result<(), String> {
        let record = HistoryRecord {
            created_at: std::time::SystemTime::now(),
            source_lang: resp.source,
            target_lang: resp.target,
            original_text: original.to_string(),
            translated_text: resp.translated_text.clone(),
            provider: resp.provider.clone(),
            model: resp.model.clone(),
            prompt_tokens: resp.token_usage.map(|u| u.prompt_tokens),
            completion_tokens: resp.token_usage.map(|u| u.completion_tokens),
            total_cost_cny_milli: None,
            monitor_id: Some(monitor.to_string()),
            bbox: Some(bbox),
            notes: None,
        };
        self.history.insert(record).await.map_err(|e| e.to_string())
    }

    fn set_state(&mut self, state: AppState) -> Vec<Event> {
        self.state = state;
        vec![Event::StateChanged(state)]
    }

    /// 用当前 `translate_config` 重建翻译 Provider。
    ///
    /// 成功则后续翻译走新 Provider；失败（如缺 API Key）置 `None`，翻译时提示去设置。
    fn rebuild_translate(&mut self) {
        match build_provider(&self.translate_config, &self.client) {
            Ok(p) => {
                self.translate = Some(Arc::from(p));
                info!(provider = ?self.translate_config.provider, "翻译 Provider 已更新");
            }
            Err(e) => {
                self.translate = None;
                warn!(error = %e, "翻译 Provider 构造失败，翻译暂不可用（请在设置中配置 API Key）");
            }
        }
    }
}

/// 从缓存帧裁剪 bbox 区域（虚拟桌面坐标 → 该屏内坐标）。
fn crop_frame(frame: &CapturedFrame, bbox: Bbox) -> Result<DynamicImage, String> {
    let monitor = &frame.monitor;
    // bbox 是虚拟桌面坐标；转到屏内坐标（减去显示器原点）。
    let x = (bbox.x - monitor.x).max(0) as u32;
    let y = (bbox.y - monitor.y).max(0) as u32;
    let w = bbox.w.max(0) as u32;
    let h = bbox.h.max(0) as u32;
    if w == 0 || h == 0 {
        return Err("选区尺寸为 0".into());
    }
    let img = image::imageops::crop_imm(&frame.image, x, y, w, h).to_image();
    Ok(DynamicImage::ImageRgba8(img))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use snaptext_core::error::CoreError;
    use snaptext_core::types::{
        MonitorInfo, OcrLine, TokenUsage, TranslateResponse as CoreTR, WritingDirection,
    };

    // ===== Mock Providers =====

    struct MockCapture;
    #[async_trait]
    impl CaptureProvider for MockCapture {
        async fn list_monitors(&self) -> Result<Vec<MonitorInfo>, CoreError> {
            Ok(Vec::new())
        }
        async fn capture_monitor(&self, _id: &MonitorId) -> Result<CapturedFrame, CoreError> {
            Err(CoreError::NotImplemented("mock"))
        }
        async fn capture_all(&self) -> Result<Vec<CapturedFrame>, CoreError> {
            // 1×1 红色帧，主显示器 DISPLAY1。
            let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
            let monitor = MonitorInfo {
                id: MonitorId::new("\\\\.\\DISPLAY1"),
                name: "mock".into(),
                width: 2,
                height: 2,
                scale: 1.0,
                x: 0,
                y: 0,
                is_primary: true,
            };
            Ok(vec![CapturedFrame {
                monitor,
                image: img,
                captured_at: std::time::SystemTime::now(),
            }])
        }
    }

    struct MockOcr;
    #[async_trait]
    impl OcrProvider for MockOcr {
        fn id(&self) -> ProviderId {
            ProviderId::new_static("mock-ocr")
        }
        fn supported_languages(&self) -> &[Lang] {
            &[]
        }
        async fn recognize(
            &self,
            _img: &DynamicImage,
            _lang: Lang,
        ) -> Result<Vec<OcrLine>, CoreError> {
            Ok(vec![OcrLine {
                text: "Hello".into(),
                bbox: Bbox {
                    x: 0,
                    y: 0,
                    w: 1,
                    h: 1,
                },
                confidence: 0.9,
                writing_direction: WritingDirection::Horizontal,
            }])
        }
    }

    struct MockTranslate;
    #[async_trait]
    impl TranslationProvider for MockTranslate {
        fn id(&self) -> ProviderId {
            ProviderId::new_static("mock-translate")
        }
        fn supported_pairs(&self) -> &[snaptext_core::types::LangPair] {
            &[]
        }
        async fn translate(&self, req: TranslateRequest) -> Result<TranslateResponse, CoreError> {
            Ok(CoreTR {
                translated_text: format!("[译] {}", req.text),
                source: req.source,
                target: req.target,
                provider: ProviderId::new_static("mock-translate"),
                model: Some("mock".into()),
                token_usage: Some(TokenUsage {
                    prompt_tokens: 1,
                    completion_tokens: 1,
                }),
            })
        }
    }

    struct MockHistory;
    #[async_trait]
    impl HistoryStore for MockHistory {
        async fn insert(&self, _record: HistoryRecord) -> Result<(), CoreError> {
            Ok(())
        }
        async fn list(&self, _limit: u32) -> Result<Vec<HistoryRecord>, CoreError> {
            Err(CoreError::NotImplemented("mock"))
        }
        async fn delete_before(&self, _before: std::time::SystemTime) -> Result<u64, CoreError> {
            Err(CoreError::NotImplemented("mock"))
        }
        async fn stats(&self) -> Result<snaptext_core::history::HistoryStats, CoreError> {
            Err(CoreError::NotImplemented("mock"))
        }
    }

    fn orchestrator() -> Orchestrator {
        Orchestrator::new(
            Arc::new(MockCapture),
            Arc::new(MockOcr),
            Some(Arc::new(MockTranslate) as Arc<dyn TranslationProvider>),
            Arc::new(MockHistory),
            false,
            TranslateConfig::default(),
            reqwest::Client::new(),
        )
    }

    #[tokio::test]
    async fn full_pipeline_trigger_to_translate() {
        let mut o = orchestrator();
        // 1. 触发截图。
        let evs = o.handle(Command::TriggerCapture).await;
        assert!(evs.iter().any(|e| matches!(e, Event::Captured(_))));
        assert!(o.captured.len() == 1);

        // 2. 框选 → OCR → 翻译 → 历史。
        let bbox = Bbox {
            x: 0,
            y: 0,
            w: 2,
            h: 2,
        };
        let evs = o
            .handle(Command::RegionSelected(
                MonitorId::new("\\\\.\\DISPLAY1"),
                bbox,
            ))
            .await;
        assert!(evs.iter().any(|e| matches!(e, Event::OcrDone(_))));
        assert!(evs.iter().any(|e| matches!(e, Event::TranslateDone(_))));
        assert!(evs
            .iter()
            .any(|e| matches!(e, Event::StateChanged(AppState::Showing))));
    }

    #[tokio::test]
    async fn cancel_clears_state() {
        let mut o = orchestrator();
        o.handle(Command::TriggerCapture).await;
        let evs = o.handle(Command::Cancel).await;
        assert!(evs
            .iter()
            .any(|e| matches!(e, Event::StateChanged(AppState::Idle))));
        assert!(o.captured.is_empty());
    }

    #[tokio::test]
    async fn region_selected_unknown_monitor_errors() {
        let mut o = orchestrator();
        o.handle(Command::TriggerCapture).await;
        let evs = o
            .handle(Command::RegionSelected(
                MonitorId::new("UNKNOWN"),
                Bbox {
                    x: 0,
                    y: 0,
                    w: 1,
                    h: 1,
                },
            ))
            .await;
        assert!(evs.iter().any(|e| matches!(e, Event::Error(_))));
    }

    #[tokio::test]
    async fn translate_without_provider_returns_error() {
        // 降级路径：translate=None（缺 API Key），框选后应报错而非 panic。
        let mut o = Orchestrator::new(
            Arc::new(MockCapture),
            Arc::new(MockOcr),
            None,
            Arc::new(MockHistory),
            false,
            TranslateConfig::default(),
            reqwest::Client::new(),
        );
        o.handle(Command::TriggerCapture).await;
        let evs = o
            .handle(Command::RegionSelected(
                MonitorId::new("\\\\.\\DISPLAY1"),
                Bbox {
                    x: 0,
                    y: 0,
                    w: 2,
                    h: 2,
                },
            ))
            .await;
        assert!(evs.iter().any(|e| matches!(e, Event::Error(_))));
        assert!(!evs.iter().any(|e| matches!(e, Event::TranslateDone(_))));
    }
}
