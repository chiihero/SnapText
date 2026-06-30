//! OCR + 翻译 + 行级配对命令（三层分阶段）。
//!
//! 旧 `select_region` 是一个干完全部的大命令（裁剪→OCR→翻译→配对→落库），
//! 框选抬起后选区窗要 await 整个管线几秒才开结果窗。拆成三层命令：
//! - `crop_region`：裁剪缓存帧 + 写临时 PNG（抬起即调，几十 ms）
//! - `recognize_region`：从裁剪图 OCR + 后处理，返回 OCR 行与整段原文
//! - `translate_region`：整段翻译 + `align_lines` 行配对 + 写历史
//!
//! 三层之间用 `state.last_crop` / `state.last_ocr` 接力，沿用"后端缓存+前端主动拉取"
//! 反竞态模式（不引入事件）。核心管线拆为 `run_ocr` / `run_translate` 两纯函数，
//! 不依赖 Tauri，便于 mock 测试。

use std::io::Cursor;

use image::DynamicImage;
use serde::Serialize;
use snaptext_core::history::HistoryRecord;
use snaptext_core::ocr::postprocess::clean_ocr_text;
use snaptext_core::translate::postprocess::clean_translation;
use snaptext_core::types::{Bbox, Lang, OcrLine, TranslateRequest};
use tauri::{AppHandle, Manager, State};

use crate::state::AppState;

/// 裁剪阶段缓存：给 `recognize_region` 提供 OCR 输入图。
/// `shot_path` 同时供结果窗 onMounted 渲染原图（在 OCR 之前就能显示）。
pub struct LastCrop {
    pub shot_path: String,
    pub image: DynamicImage,
    pub monitor_id: String,
    pub bbox: Bbox,
}

/// OCR 阶段缓存：给 `translate_region` 提供原文与定位。
pub struct LastOcr {
    pub ocr_lines: Vec<OcrLine>,
    pub original: String,
    pub monitor_id: String,
    pub bbox: Bbox,
}

/// `crop_region` 返回前端的最小信息：结果窗拿它渲染原图。
#[derive(Serialize)]
pub struct CropResult {
    /// 裁剪图绝对路径（前端用 convertFileSrc 转 webview URL）。
    pub shot_path: String,
}

/// `recognize_region` 返回：OCR 行 + 整段原文。结果窗据此先显示原文。
#[derive(Serialize)]
pub struct OcrResult {
    pub ocr_lines: Vec<OcrLine>,
    pub original: String,
}

/// `translate_region` 返回：逐行译文 + 整段译文 + Provider + 耗时。
#[derive(Serialize)]
pub struct TranslateResult {
    pub translations: Vec<String>,
    pub translated: String,
    pub provider: String,
}

/// 第 1 层：裁剪缓存帧的 bbox 区 + 写临时 PNG。
///
/// 抬起即调，几十 ms。返回路径供结果窗 onMounted 渲染原图；裁剪图同时
/// 缓存进 `state.last_crop` 供 `recognize_region` OCR。
#[tauri::command]
pub async fn crop_region(
    app: AppHandle,
    state: State<'_, AppState>,
    monitor_id: String,
    bbox: Bbox,
) -> Result<CropResult, String> {
    let (image, shot_path) = {
        let captured = state.captured.lock().await;
        let frame = captured
            .iter()
            .find(|f| f.monitor.id.as_str() == monitor_id)
            .ok_or_else(|| format!("找不到显示器 {monitor_id} 的缓存帧"))?;
        let image = crop_frame(frame, bbox)?;
        let shot_path = write_crop_png(&app, &image, &monitor_id)?;
        (image, shot_path)
    };
    *state.last_crop.lock().await = Some(LastCrop {
        shot_path: shot_path.clone(),
        image,
        monitor_id,
        bbox,
    });
    Ok(CropResult { shot_path })
}

/// 取最近一次裁剪的图片路径（结果窗口 onMounted 拉取，先渲染原图再 OCR）。
///
/// Pinia 不跨窗口共享，Capture.vue 拿到的 CropResult 传不到 Result.vue，
/// 故后端缓存 last_crop、由结果窗口主动调本命令取路径。
#[tauri::command]
pub async fn get_last_crop(state: State<'_, AppState>) -> Result<CropResult, String> {
    let guard = state.last_crop.lock().await;
    guard
        .as_ref()
        .map(|c| CropResult {
            shot_path: c.shot_path.clone(),
        })
        .ok_or_else(|| "无缓存裁剪图".to_string())
}

/// 第 2 层：OCR + 后处理，返回 OCR 行与整段原文。
///
/// 从 `state.last_crop` 取裁剪图（`crop_region` 写入）。结果缓存进
/// `state.last_ocr` 供 `translate_region`。
#[tauri::command]
pub async fn recognize_region(state: State<'_, AppState>) -> Result<OcrResult, String> {
    let (image, monitor_id, bbox) = {
        let last = state.last_crop.lock().await;
        let last = last
            .as_ref()
            .ok_or_else(|| "无缓存裁剪图（请先框选）".to_string())?;
        (last.image.clone(), last.monitor_id.clone(), last.bbox)
    };
    let cfg = state.config.lock().await.clone();
    let ocr_lines = run_ocr(&image, state.ocr.as_ref(), &cfg).await?;
    let original: String = ocr_lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if original.trim().is_empty() {
        return Err("识别结果为空".into());
    }
    *state.last_ocr.lock().await = Some(LastOcr {
        ocr_lines: ocr_lines.clone(),
        original: original.clone(),
        monitor_id,
        bbox,
    });
    Ok(OcrResult {
        ocr_lines,
        original,
    })
}

/// 第 3 层：整段翻译 + 行配对 + 写历史。
///
/// 从 `state.last_ocr` 取原文与行数（`recognize_region` 写入）。
#[tauri::command]
pub async fn translate_region(state: State<'_, AppState>) -> Result<TranslateResult, String> {
    let (ocr_lines, original, monitor_id, bbox) = {
        let last = state.last_ocr.lock().await;
        let last = last
            .as_ref()
            .ok_or_else(|| "无缓存 OCR 结果（请先识别）".to_string())?;
        (
            last.ocr_lines.clone(),
            last.original.clone(),
            last.monitor_id.clone(),
            last.bbox,
        )
    };
    let cfg = state.config.lock().await.clone();
    let provider = state
        .translate
        .lock()
        .await
        .clone()
        .ok_or_else(|| "未配置翻译 API Key，请在设置中填写".to_string())?;

    let (translations, resp) =
        run_translate(&original, ocr_lines.len(), provider.as_ref(), &cfg).await?;

    // 裁剪图编码为 PNG 入历史（裁剪图已由 crop_region 写盘，此处复用内存图）。
    let screenshot_png = read_last_crop_png(&state).await;

    // 写历史（与旧 select_region 同：四要素齐了再落库）。
    let record = HistoryRecord {
        id: 0,
        created_at: std::time::SystemTime::now(),
        source_lang: resp.source,
        target_lang: resp.target,
        original_text: original,
        translated_text: resp.translated_text.clone(),
        provider: resp.provider.clone(),
        model: resp.model.clone(),
        prompt_tokens: resp.token_usage.map(|u| u.prompt_tokens),
        completion_tokens: resp.token_usage.map(|u| u.completion_tokens),
        total_cost_cny_milli: None,
        monitor_id: Some(monitor_id),
        bbox: Some(bbox),
        notes: None,
        screenshot_png,
        ocr_lines: Some(ocr_lines.clone()),
        line_translations: Some(translations.clone()),
    };
    if let Err(e) = state.history.insert(record).await {
        tracing::warn!(error = %e, "写入历史失败");
    }

    Ok(TranslateResult {
        translations,
        translated: resp.translated_text,
        provider: resp.provider.to_string(),
    })
}

/// 取最近一次裁剪图并编码为 PNG（供历史记录入库）。无缓存裁剪图时返回 None（不报错）。
///
/// 裁剪图是 `crop_region` 写入 `last_crop` 的，此处复用同一张内存图编码入历史，
/// 避免在 `HistoryRecord` 字面量里嵌套 4 层 match。
async fn read_last_crop_png(state: &AppState) -> Option<Vec<u8>> {
    let image = state
        .last_crop
        .lock()
        .await
        .as_ref()
        .map(|c| c.image.clone());
    image.and_then(|img| encode_png(&img).ok())
}

/// OCR 纯函数：识别 + 可选后处理。不依赖 Tauri，便于 mock 测试。
pub async fn run_ocr(
    image: &DynamicImage,
    ocr: &dyn snaptext_core::ocr::OcrProvider,
    cfg: &snaptext_core::Config,
) -> Result<Vec<OcrLine>, String> {
    let mut lines = ocr
        .recognize(image, Lang::Auto)
        .await
        .map_err(|e| format!("OCR 失败：{e}"))?;
    if cfg.ocr.postprocess {
        for l in lines.iter_mut() {
            l.text = clean_ocr_text(&l.text);
        }
    }
    Ok(lines)
}

/// 翻译纯函数：整段翻译 + 可选后处理 + `align_lines` 行配对。
/// 不依赖 Tauri，便于 mock 测试。
pub async fn run_translate(
    original: &str,
    n_lines: usize,
    provider: &dyn snaptext_core::translate::TranslationProvider,
    cfg: &snaptext_core::Config,
) -> Result<(Vec<String>, snaptext_core::types::TranslateResponse), String> {
    let req = TranslateRequest {
        text: original.to_string(),
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
    let translations = align_lines(&resp.translated_text, n_lines);
    Ok((translations, resp))
}

/// 裁剪缓存帧的 bbox 区域（虚拟桌面坐标 → 屏内坐标）。
///
/// bbox 是前端算出的虚拟桌面坐标，先减 monitor 原点转为屏内坐标。
/// 再 clamp 到图像边界——`crop_imm` 在 `x+w > width` 时会 panic，多屏坐标错位
/// 或框选越出屏幕时会触发（B6），故超界区域返回错误而非 panic。
pub fn crop_frame(
    frame: &snaptext_core::types::CapturedFrame,
    bbox: Bbox,
) -> Result<DynamicImage, String> {
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
        return Err(format!("选区起点 ({x},{y}) 超出截图范围 ({img_w}×{img_h})"));
    }
    // clamp 宽高到图像右下边界，避免 crop_imm 越界 panic。
    let w = w.min(img_w - x);
    let h = h.min(img_h - y);
    let cropped = image::imageops::crop_imm(img, x, y, w, h).to_image();
    Ok(DynamicImage::ImageRgba8(cropped))
}

/// 把图像编码为 PNG 字节（结果图写盘 / 历史截图入库 复用）。
fn encode_png(image: &DynamicImage) -> Result<Vec<u8>, String> {
    let mut buf = Cursor::new(Vec::new());
    image
        .write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| format!("编码 PNG 失败：{e}"))?;
    Ok(buf.into_inner())
}

/// 把裁剪图写临时文件 + 返回绝对路径（前端用 convertFileSrc 转 URL）。
fn write_crop_png(
    app: &AppHandle,
    crop: &DynamicImage,
    monitor_id: &str,
) -> Result<String, String> {
    let tmp_dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("定位缓存目录失败：{e}"))?
        .join("tmp");
    std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("创建缓存目录失败：{e}"))?;
    // monitor_id 含 Windows 非法文件名字符（如 \\.\DISPLAY1），需清洗（与 capture.rs 一致）。
    let safe_id = monitor_id.replace(['\\', '/', ':'], "_");
    let path = tmp_dir.join(format!("result_{safe_id}.png"));
    std::fs::write(&path, encode_png(crop)?).map_err(|e| format!("写结果图失败：{e}"))?;
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
    //! 拆分后 run_ocr / run_translate 分别测试，仍覆盖旧的整条管线语义。
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
        async fn translate(&self, req: TranslateRequest) -> Result<TranslateResponse, CoreError> {
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
                    bbox: Bbox {
                        x: 0,
                        y: 0,
                        w: 100,
                        h: 30,
                    },
                    confidence: 0.9,
                    writing_direction: WritingDirection::Horizontal,
                },
                OcrLine {
                    text: "World".into(),
                    bbox: Bbox {
                        x: 0,
                        y: 40,
                        w: 100,
                        h: 30,
                    },
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
    async fn run_ocr_returns_lines() {
        // OCR 两行：run_ocr 直接返回（后处理默认 trim）。
        let ocr = mock_ocr();
        let cfg = default_config();
        let crop = dummy_crop();
        let lines = run_ocr(&crop, &ocr, &cfg).await.expect("OCR 应成功");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "Hello");
        assert_eq!(lines[1].text, "World");
    }

    #[tokio::test]
    async fn run_translate_aligns() {
        // 翻译 + align：两行原文配两行译文。
        let translate = MockTranslate;
        let cfg = default_config();
        let (translations, resp) = run_translate("Hello\nWorld", 2, &translate, &cfg)
            .await
            .expect("翻译应成功");
        assert_eq!(translations, vec!["[译] Hello", "[译] World"]);
        assert_eq!(resp.translated_text, "[译] Hello\n[译] World");
    }

    #[tokio::test]
    async fn run_ocr_postprocess_applied() {
        // 开启 OCR 后处理：文本经 clean_ocr_text（去 CJK 空格等）。
        let ocr = MockOcr {
            lines: vec![OcrLine {
                text: " Hel lo ".into(), // 带空格，clean 后变化
                bbox: Bbox {
                    x: 0,
                    y: 0,
                    w: 10,
                    h: 10,
                },
                confidence: 1.0,
                writing_direction: WritingDirection::Horizontal,
            }],
        };
        let mut cfg = default_config();
        cfg.ocr.postprocess = true;
        let crop = dummy_crop();
        let lines = run_ocr(&crop, &ocr, &cfg).await.expect("OCR 应成功");
        // postprocess 后原文应被清洗（trim）。
        assert!(
            !lines[0].text.starts_with(' '),
            "原文应被 trim：{}",
            lines[0].text
        );
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
        let crop = crop_frame(
            &frame,
            Bbox {
                x: 1950,
                y: 10,
                w: 20,
                h: 30,
            },
        )
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
        let err = crop_frame(
            &frame,
            Bbox {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            },
        )
        .expect_err("零尺寸应报错");
        assert!(err.contains("0"));
    }

    #[test]
    fn crop_frame_bbox_beyond_image_is_clamped() {
        // B6：bbox 超出图像右下边界 → clamp 到边界，不 panic。
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
        let crop = crop_frame(
            &frame,
            Bbox {
                x: 8,
                y: 8,
                w: 100,
                h: 100,
            },
        )
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
        let err = crop_frame(
            &frame,
            Bbox {
                x: 100,
                y: 100,
                w: 5,
                h: 5,
            },
        )
        .expect_err("起点越界应报错");
        assert!(err.contains("超出截图范围"), "错误应说明越界：{err}");
    }

    // 保留对错误类型的引用，避免 import 被裁（mock 语义文档化）。
    #[allow(dead_code)]
    fn _err_types(_: CaptureError, _: OcrError, _: TranslateError) {}
}
