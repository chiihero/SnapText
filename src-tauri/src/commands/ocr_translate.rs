//! OCR + 翻译 + 行级配对命令：select_region（选区→识别→翻译→配对→落库）。
//!
//! 这是核心编排，从旧 orchestrator::region_selected 搬来，去掉 channel，
//! 改为命令直接返回结果。整段翻译后用 align_lines 按行配对，译文行数与原文
//! 不一致时多余并入末行 / 缺失补空。

use std::io::Cursor;
use std::time::Instant;

use image::DynamicImage;
use serde::Serialize;
use snaptext_core::history::HistoryRecord;
use snaptext_core::ocr::postprocess::clean_ocr_text;
use snaptext_core::translate::postprocess::clean_translation;
use snaptext_core::types::{Bbox, Lang, MonitorId, OcrLine, TranslateRequest};
use tauri::{AppHandle, Manager, State};

use crate::state::AppState;

/// 选区结果：返回给前端结果窗口渲染图上覆盖。
#[derive(Serialize, Clone)]
pub struct SelectResult {
    /// 裁剪图绝对路径（前端用 convertFileSrc 转 webview URL）。
    pub shot_path: String,
    /// OCR 行（含 bbox），前端按 bbox 定位译文。
    pub ocr_lines: Vec<OcrLine>,
    /// 与 ocr_lines 按索引配对的逐行译文。
    pub translations: Vec<String>,
    /// 整段原文（拼接，便于复制）。
    pub original: String,
    /// 整段译文（便于复制）。
    pub translated: String,
    pub provider: String,
    pub elapsed_ms: u64,
}

/// 选区→OCR→翻译→配对→落库。
///
/// `monitor_id` + `bbox` 由前端选区窗口框选后传入；bbox 是虚拟桌面坐标。
#[tauri::command]
pub async fn select_region(
    app: AppHandle,
    state: State<'_, AppState>,
    monitor_id: String,
    bbox: Bbox,
) -> Result<SelectResult, String> {
    let start = Instant::now();

    // 1. 从缓存帧找该屏 + 裁剪。
    tracing::info!(monitor = %monitor_id, bbox = ?bbox, "select_region 开始，裁剪");
    let crop: DynamicImage = {
        let captured = state.captured.lock().await;
        let frame = captured
            .iter()
            .find(|f| f.monitor.id.as_str() == monitor_id)
            .ok_or_else(|| format!("找不到显示器 {monitor_id} 的缓存帧"))?;
        crop_frame(frame, bbox)?
    };
    tracing::info!(crop_w = crop.width(), crop_h = crop.height(), elapsed_ms = start.elapsed().as_millis(), "裁剪完成，开始 OCR");

    // 2-3. OCR + 翻译 + 配对（核心管线，抽成纯函数便于 mock 测试）。
    let cfg = state.config.lock().await.clone();
    let provider = state.translate.lock().await.clone();
    let ocr_start = Instant::now();
    let outcome = run_ocr_translate(&crop, state.ocr.as_ref(), provider.as_deref(), &cfg).await?;
    tracing::info!(lines = outcome.ocr_lines.len(), elapsed_ms = ocr_start.elapsed().as_millis(), "OCR+翻译完成");

    // 4. 裁剪图编码 PNG（返回绝对路径，前端用 convertFileSrc 转 URL）。
    let shot_path = write_crop_png(&app, &crop, &monitor_id)?;

    // 5. 写历史。
    let record = HistoryRecord {
        id: 0,
        created_at: std::time::SystemTime::now(),
        source_lang: outcome.resp.source,
        target_lang: outcome.resp.target,
        original_text: outcome.original.clone(),
        translated_text: outcome.resp.translated_text.clone(),
        provider: outcome.resp.provider.clone(),
        model: outcome.resp.model.clone(),
        prompt_tokens: outcome.resp.token_usage.map(|u| u.prompt_tokens),
        completion_tokens: outcome.resp.token_usage.map(|u| u.completion_tokens),
        total_cost_cny_milli: None,
        monitor_id: Some(monitor_id.clone()),
        bbox: Some(bbox),
        notes: None,
        screenshot_png: Some({
            let mut buf = Cursor::new(Vec::new());
            crop.write_to(&mut buf, image::ImageFormat::Png)
                .map_err(|e| format!("编码历史截图失败：{e}"))?;
            buf.into_inner()
        }),
        ocr_lines: Some(outcome.ocr_lines.clone()),
        line_translations: Some(outcome.translations.clone()),
    };
    if let Err(e) = state.history.insert(record).await {
        tracing::warn!(error = %e, "写入历史失败");
    }

    let result = SelectResult {
        shot_path,
        ocr_lines: outcome.ocr_lines,
        translations: outcome.translations,
        original: outcome.original,
        translated: outcome.resp.translated_text,
        provider: outcome.resp.provider.to_string(),
        elapsed_ms: start.elapsed().as_millis() as u64,
    };
    // 缓存进 state：结果窗口是独立 WebView，Pinia 不跨窗口共享，
    // 改由 Result.vue onMounted 调 get_last_result 主动拉取（反竞态，与截图缓存同款）。
    *state.last_result.lock().await = Some(result.clone());
    Ok(result)
}

/// 取最近一次选区结果（结果窗口 onMounted 拉取）。
///
/// Pinia 状态不跨窗口共享，选区窗口写入的结果结果窗口读不到，
/// 故缓存后端、由结果窗口主动调本命令拉取。无缓存时报错。
#[tauri::command]
pub async fn get_last_result(state: State<'_, AppState>) -> Result<SelectResult, String> {
    let guard = state.last_result.lock().await;
    guard
        .clone()
        .ok_or_else(|| "无缓存选区结果".to_string())
}

/// OCR + 翻译 + 行级配对的核心管线（不依赖 Tauri，便于 mock Provider 单元/集成测试）。
///
/// 输入裁剪图与 Provider 引用，返回 OCR 行、逐行译文、整段原文、翻译响应。
/// select_region 调它，再处理 Tauri 相关（写临时文件、写历史）。
pub async fn run_ocr_translate(
    crop: &DynamicImage,
    ocr: &dyn snaptext_core::ocr::OcrProvider,
    translate: Option<&dyn snaptext_core::translate::TranslationProvider>,
    cfg: &snaptext_core::Config,
) -> Result<OcrTranslateOutcome, String> {
    // OCR（含可选后处理）。
    let mut lines = ocr
        .recognize(crop, Lang::Auto)
        .await
        .map_err(|e| format!("OCR 失败：{e}"))?;
    if cfg.ocr.postprocess {
        for l in lines.iter_mut() {
            l.text = clean_ocr_text(&l.text);
        }
    }
    let original: String = lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n");
    if original.trim().is_empty() {
        return Err("识别结果为空".into());
    }

    // 翻译（含可选后处理）。
    let provider = translate.ok_or_else(|| "未配置翻译 API Key，请在设置中填写".to_string())?;
    let req = TranslateRequest {
        text: original.clone(),
        source: Lang::Auto,
        target: cfg.translate.target_lang,
        context_hint: None,
        glossary: None,
    };
    let mut resp = provider
        .translate(req)
        .await
        .map_err(|e| format!("翻译失败：{e}"))?;
    if cfg.translate.postprocess {
        resp.translated_text = clean_translation(&resp.translated_text);
    }

    // 行级配对。
    let translations = align_lines(&resp.translated_text, lines.len());
    Ok(OcrTranslateOutcome {
        ocr_lines: lines,
        translations,
        original,
        resp,
    })
}

/// run_ocr_translate 的产出。
#[cfg_attr(test, derive(Debug))]
pub struct OcrTranslateOutcome {
    pub ocr_lines: Vec<OcrLine>,
    pub translations: Vec<String>,
    pub original: String,
    pub resp: snaptext_core::types::TranslateResponse,
}

/// 裁剪缓存帧的 bbox 区域（虚拟桌面坐标 → 屏内坐标）。
///
/// bbox 是前端算出的虚拟桌面坐标，先减 monitor 原点转为屏内坐标。
/// 再 clamp 到图像边界——`crop_imm` 在 `x+w > width` 时会 panic，多屏坐标错位
/// 或框选越出屏幕时会触发（B6），故超界区域返回错误而非 panic。
pub fn crop_frame(frame: &snaptext_core::types::CapturedFrame, bbox: Bbox) -> Result<DynamicImage, String> {
    let m = &frame.monitor;
    let img = &frame.image;
    let img_w = img.width();
    let img_h = img.height();

    let x = (bbox.x - m.x).max(0) as u32;
    let y = (bbox.y - m.y).max(0) as u32;
    let w = bbox.w.max(0) as u32;
    let h = bbox.h.max(0) as u32;
    if w == 0 || h == 0 {
        return Err("选区尺寸为 0".into());
    }
    // 选区起点已在图外（如多屏 bbox 错位到屏幕外），无可裁剪区域。
    if x >= img_w || y >= img_h {
        return Err(format!(
            "选区起点 ({x},{y}) 超出截图范围 ({img_w}×{img_h})"
        ));
    }
    // clamp 宽高到图像右下边界，避免 crop_imm 越界 panic。
    let w = w.min(img_w - x);
    let h = h.min(img_h - y);
    let cropped = image::imageops::crop_imm(img, x, y, w, h).to_image();
    Ok(DynamicImage::ImageRgba8(cropped))
}

/// 把裁剪图写临时文件 + 返回绝对路径（前端用 convertFileSrc 转 URL）。
fn write_crop_png(app: &AppHandle, crop: &DynamicImage, monitor_id: &str) -> Result<String, String> {
    let tmp_dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("定位缓存目录失败：{e}"))?
        .join("tmp");
    std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("创建缓存目录失败：{e}"))?;
    // monitor_id 含 Windows 非法文件名字符（如 \\.\DISPLAY1），需清洗（与 capture.rs 一致）。
    let safe_id = monitor_id.replace(['\\', '/', ':'], "_");
    let path = tmp_dir.join(format!("result_{safe_id}.png"));
    let mut buf = Cursor::new(Vec::new());
    crop.write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| format!("编码结果图失败：{e}"))?;
    std::fs::write(&path, buf.into_inner()).map_err(|e| format!("写结果图失败：{e}"))?;
    Ok(path.to_string_lossy().to_string())
}

/// 把整段译文按行切分并与 OCR 行配对。
///
/// 整段翻译的固有限制：译文行数可能与原文不一致。
/// - 行数相等：逐行配对；
/// - 译文多于原文：多余的并入最后一行；
/// - 译文少于原文：缺失行补空串。
pub fn align_lines(translated: &str, n_lines: usize) -> Vec<String> {
    let parts: Vec<&str> = translated.lines().collect();
    if parts.len() >= n_lines {
        let mut out: Vec<String> = parts[..n_lines.saturating_sub(1)]
            .iter()
            .map(|s| s.to_string())
            .collect();
        if n_lines > 0 {
            let tail = parts[n_lines.saturating_sub(1)..].join("\n");
            out.push(tail);
        }
        out
    } else {
        let mut out: Vec<String> = parts.iter().map(|s| s.to_string()).collect();
        out.resize(n_lines, String::new());
        out
    }
}

/// 下面的类型仅用于编译期类型检查保留（避免 unused 警告）。
#[allow(dead_code)]
fn _types_used(_: MonitorId) {}

#[cfg(test)]
mod tests {
    use super::align_lines;

    #[test]
    fn align_equal_lines() {
        // 译文行数 == 原文行数：逐行配对。
        let out = align_lines("a\nb\nc", 3);
        assert_eq!(out, vec!["a", "b", "c"]);
    }

    #[test]
    fn align_more_translated_merged_to_tail() {
        // 译文多于原文：多余行并入最后一行（保留分隔）。
        let out = align_lines("a\nb\nc\nd", 3);
        assert_eq!(out, vec!["a", "b", "c\nd"]);
    }

    #[test]
    fn align_less_translated_pad_empty() {
        // 译文少于原文：缺失行补空串。
        let out = align_lines("a", 3);
        assert_eq!(out, vec!["a", "", ""]);
    }

    #[test]
    fn align_empty_translated() {
        // 空译文：全部补空串。
        let out = align_lines("", 2);
        assert_eq!(out, vec!["", ""]);
    }

    #[test]
    fn align_single_line() {
        // 单行原文：整段译文归到唯一行。
        let out = align_lines("hello\nworld", 1);
        assert_eq!(out, vec!["hello\nworld"]);
    }

    #[test]
    fn align_zero_lines() {
        // 零行原文：返回空 Vec（不 panic）。
        let out = align_lines("a\nb", 0);
        assert!(out.is_empty());
    }
}

#[cfg(test)]
mod integration_tests {
    //! 核心管线（OCR→翻译→配对）集成测试，用 mock Provider 覆盖端到端逻辑。
    //! 取代旧 snaptext-app/orchestrator.rs 的 full_pipeline 测试（随 crate 删除丢失）。
    use super::*;
    use async_trait::async_trait;
    use image::RgbaImage;
    use snaptext_core::error::{CaptureError, CoreError, OcrError, TranslateError};
    use snaptext_core::ocr::OcrProvider;
    use snaptext_core::translate::TranslationProvider;
    use snaptext_core::types::{
        CapturedFrame, MonitorId, MonitorInfo, ProviderId, TokenUsage, TranslateResponse,
        WritingDirection,
    };

    /// Mock OCR：固定返回两行（带 bbox），模拟识别结果。
    struct MockOcr {
        lines: Vec<OcrLine>,
    }
    #[async_trait]
    impl OcrProvider for MockOcr {
        fn id(&self) -> ProviderId {
            ProviderId::new_static("mock-ocr")
        }
        fn supported_languages(&self) -> &[Lang] {
            &[Lang::Auto]
        }
        async fn recognize(
            &self,
            _img: &DynamicImage,
            _lang: Lang,
        ) -> Result<Vec<OcrLine>, CoreError> {
            Ok(self.lines.clone())
        }
    }

    /// Mock 翻译：把每行原文前缀 "[译] "，模拟整段翻译。
    struct MockTranslate;
    #[async_trait]
    impl TranslationProvider for MockTranslate {
        fn id(&self) -> ProviderId {
            ProviderId::new_static("mock-translate")
        }
        fn supported_pairs(&self) -> &[snaptext_core::types::LangPair] {
            &[]
        }
        async fn translate(
            &self,
            req: TranslateRequest,
        ) -> Result<TranslateResponse, CoreError> {
            Ok(TranslateResponse {
                translated_text: req
                    .text
                    .lines()
                    .map(|l| format!("[译] {l}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
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

    fn mock_ocr() -> MockOcr {
        MockOcr {
            lines: vec![
                OcrLine {
                    text: "Hello".into(),
                    bbox: Bbox { x: 0, y: 0, w: 100, h: 30 },
                    confidence: 0.9,
                    writing_direction: WritingDirection::Horizontal,
                },
                OcrLine {
                    text: "World".into(),
                    bbox: Bbox { x: 0, y: 40, w: 100, h: 30 },
                    confidence: 0.88,
                    writing_direction: WritingDirection::Horizontal,
                },
            ],
        }
    }

    /// 构造一张 10×10 透明小图作为裁剪图（内容无关紧要，MockOcr 不看图）。
    fn dummy_crop() -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::new(10, 10))
    }

    fn default_config() -> snaptext_core::Config {
        snaptext_core::Config::default()
    }

    #[tokio::test]
    async fn pipeline_ocr_translate_aligns() {
        // 端到端：OCR 两行 → 翻译 → align 配对 → 译文行数 == 原文行数。
        let ocr = mock_ocr();
        let translate = MockTranslate;
        let cfg = default_config();
        let crop = dummy_crop();
        let outcome = run_ocr_translate(&crop, &ocr, Some(&translate), &cfg)
            .await
            .expect("管线应成功");

        assert_eq!(outcome.ocr_lines.len(), 2);
        assert_eq!(outcome.original, "Hello\nWorld");
        // 译文逐行配对。
        assert_eq!(outcome.translations, vec!["[译] Hello", "[译] World"]);
        // 整段译文。
        assert_eq!(outcome.resp.translated_text, "[译] Hello\n[译] World");
    }

    #[tokio::test]
    async fn pipeline_without_provider_errors() {
        // 缺翻译 Provider：报错提示去设置，不 panic。
        let ocr = mock_ocr();
        let cfg = default_config();
        let crop = dummy_crop();
        let err = run_ocr_translate(&crop, &ocr, None, &cfg)
            .await
            .expect_err("缺 Provider 应报错");
        assert!(err.contains("API Key"), "错误应提示配置 Key：{err}");
    }

    #[tokio::test]
    async fn pipeline_empty_ocr_errors() {
        // OCR 全空：报错"识别结果为空"。
        let ocr = MockOcr { lines: vec![] };
        let translate = MockTranslate;
        let cfg = default_config();
        let crop = dummy_crop();
        let err = run_ocr_translate(&crop, &ocr, Some(&translate), &cfg)
            .await
            .expect_err("空 OCR 应报错");
        assert!(err.contains("为空"), "错误应说明识别为空：{err}");
    }

    #[tokio::test]
    async fn pipeline_postprocess_applied() {
        // 开启 OCR 后处理：文本经 clean_ocr_text（去 CJK 空格等）。
        let ocr = MockOcr {
            lines: vec![OcrLine {
                text: " Hel lo ".into(), // 带空格，clean 后变化
                bbox: Bbox { x: 0, y: 0, w: 10, h: 10 },
                confidence: 1.0,
                writing_direction: WritingDirection::Horizontal,
            }],
        };
        let translate = MockTranslate;
        let mut cfg = default_config();
        cfg.ocr.postprocess = true;
        let crop = dummy_crop();
        let outcome = run_ocr_translate(&crop, &ocr, Some(&translate), &cfg)
            .await
            .expect("管线应成功");
        // postprocess 后原文应被清洗（trim）。
        assert!(!outcome.original.starts_with(' '), "原文应被 trim：{}", outcome.original);
    }

    #[test]
    fn crop_frame_virtual_to_screen_coords() {
        // bbox 是虚拟桌面坐标；帧属于某显示器（origin 非零），裁剪应减去原点。
        let frame = CapturedFrame {
            monitor: MonitorInfo {
                id: MonitorId::new("DISPLAY1"),
                name: "m".into(),
                width: 100,
                height: 100,
                scale: 1.0,
                x: 1920, // 显示器在虚拟桌面 x=1920
                y: 0,
                is_primary: false,
            },
            image: RgbaImage::new(100, 100),
            captured_at: std::time::SystemTime::now(),
        };
        // 虚拟坐标 (1950, 10) → 屏内 (30, 10)。
        let crop = crop_frame(&frame, Bbox { x: 1950, y: 10, w: 20, h: 30 })
            .expect("裁剪应成功");
        assert_eq!(crop.width(), 20);
        assert_eq!(crop.height(), 30);
    }

    #[test]
    fn crop_frame_zero_size_errors() {
        // 选区尺寸 0：报错。
        let frame = CapturedFrame {
            monitor: MonitorInfo {
                id: MonitorId::new("D1"),
                name: "m".into(),
                width: 10,
                height: 10,
                scale: 1.0,
                x: 0,
                y: 0,
                is_primary: true,
            },
            image: RgbaImage::new(10, 10),
            captured_at: std::time::SystemTime::now(),
        };
        let err = crop_frame(&frame, Bbox { x: 0, y: 0, w: 0, h: 0 })
            .expect_err("零尺寸应报错");
        assert!(err.contains("0"));
    }

    #[test]
    fn crop_frame_bbox_beyond_image_is_clamped() {
        // B6：bbox 超出图像右下边界 → clamp 到边界，不 panic。
        // 多屏坐标错位或框选越出屏幕时会触发此路径。
        let frame = CapturedFrame {
            monitor: MonitorInfo {
                id: MonitorId::new("D1"),
                name: "m".into(),
                width: 10,
                height: 10,
                scale: 1.0,
                x: 0,
                y: 0,
                is_primary: true,
            },
            image: RgbaImage::new(10, 10),
            captured_at: std::time::SystemTime::now(),
        };
        // 选区 (8,8) 起、宽高各 100，远超 10×10 图。
        let crop = crop_frame(&frame, Bbox { x: 8, y: 8, w: 100, h: 100 })
            .expect("越界应 clamp 成功而非报错");
        // 应只裁到图像剩余的 2×2。
        assert_eq!(crop.width(), 2);
        assert_eq!(crop.height(), 2);
    }

    #[test]
    fn crop_frame_origin_outside_image_errors() {
        // B6：选区起点已在图外（如多屏 bbox 错位）→ 返回 Err，不 panic。
        let frame = CapturedFrame {
            monitor: MonitorInfo {
                id: MonitorId::new("D1"),
                name: "m".into(),
                width: 10,
                height: 10,
                scale: 1.0,
                x: 0,
                y: 0,
                is_primary: true,
            },
            image: RgbaImage::new(10, 10),
            captured_at: std::time::SystemTime::now(),
        };
        let err = crop_frame(&frame, Bbox { x: 100, y: 100, w: 5, h: 5 })
            .expect_err("起点越界应报错");
        assert!(err.contains("超出截图范围"), "错误应说明越界：{err}");
    }

    // 保留对错误类型的引用，避免 import 被裁（mock 语义文档化）。
    #[allow(dead_code)]
    fn _err_types(_: CaptureError, _: OcrError, _: TranslateError) {}
}

