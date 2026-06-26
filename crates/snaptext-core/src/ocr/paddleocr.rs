//! `PaddleOcrProvider`：基于 oar-ocr 的 PP-OCR（det + rec + 字典）封装。
//!
//! oar-ocr 的 `OAROCR` 已实现 `Send + Sync`（内部 `Arc<Session>`），故用 `Arc<OAROCR>`
//! 跨线程共享，无需 `Mutex`。ONNX 推理在 `spawn_blocking` 中执行。

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use oar_ocr::oarocr::{OAROCRBuilder, OAROCR};

use super::preprocess::to_rgb;
use crate::error::{CoreError, OcrError};
use crate::ocr::OcrProvider;
use crate::types::{Bbox, Lang, OcrLine, ProviderId, WritingDirection};

/// PP-OCR（oar-ocr 后端）Provider。
pub struct PaddleOcrProvider {
    engine: Arc<OAROCR>,
}

/// PP-OCR 支持的语言（PP-OCRv5/v6 多语言模型覆盖中 / 英 / 日）。
const SUPPORTED: [Lang; 3] = [Lang::En, Lang::Zh, Lang::Ja];

impl PaddleOcrProvider {
    /// 从本地模型文件构造：det 模型 + rec 模型 + 字符字典。
    ///
    /// 模型文件由 ModelManager 提供（见 DU-03），禁用 oar-ocr 自动下载。
    pub fn new(
        det_model: impl Into<PathBuf>,
        rec_model: impl Into<PathBuf>,
        dict_path: impl Into<PathBuf>,
    ) -> Result<Self, OcrError> {
        let det = det_model.into();
        let rec = rec_model.into();
        let dict = dict_path.into();
        if !det.exists() {
            return Err(OcrError::ModelNotFound { path: det });
        }
        if !rec.exists() {
            return Err(OcrError::ModelNotFound { path: rec });
        }
        if !dict.exists() {
            return Err(OcrError::ModelNotFound { path: dict });
        }
        let engine = OAROCRBuilder::new(det, rec, dict)
            .build()
            .map_err(|e| OcrError::ModelLoad(e.to_string()))?;
        Ok(Self {
            engine: Arc::new(engine),
        })
    }
}

#[async_trait]
impl OcrProvider for PaddleOcrProvider {
    fn id(&self) -> ProviderId {
        ProviderId::new_static("paddleocr")
    }

    fn supported_languages(&self) -> &[Lang] {
        &SUPPORTED
    }

    async fn recognize(
        &self,
        img: &image::DynamicImage,
        _lang: Lang,
    ) -> Result<Vec<OcrLine>, CoreError> {
        let engine = self.engine.clone();
        let rgb = to_rgb(img);
        let lines = tokio::task::spawn_blocking(move || -> Result<Vec<OcrLine>, OcrError> {
            let results = engine
                .predict(vec![rgb])
                .map_err(|e| OcrError::Inference(e.to_string()))?;
            let result = results
                .into_iter()
                .next()
                .ok_or_else(|| OcrError::Inference("OCR 未返回结果".into()))?;
            Ok(result
                .text_regions
                .into_iter()
                .map(|r| {
                    let bbox = &r.bounding_box;
                    OcrLine {
                        text: r.text.map(|t| t.to_string()).unwrap_or_default(),
                        bbox: Bbox {
                            x: bbox.x_min() as i32,
                            y: bbox.y_min() as i32,
                            w: (bbox.x_max() - bbox.x_min()) as i32,
                            h: (bbox.y_max() - bbox.y_min()) as i32,
                        },
                        confidence: r.confidence.unwrap_or(0.0),
                        writing_direction: WritingDirection::Horizontal,
                    }
                })
                .collect())
        })
        .await
        .map_err(|e| CoreError::Ocr(OcrError::Inference(format!("OCR 线程异常：{e}"))))??;
        Ok(lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{CaptureProvider, WindowsCaptureProvider};
    use crate::config::Tier;
    use crate::model_manager::{det_model_path, dict_path, rec_model_path};

    /// 已下载的 v6 small 模型路径（需先下载到可执行文件同级的 `models\ppocr\v6\small\`）。
    fn model_paths() -> (PathBuf, PathBuf, PathBuf) {
        (
            det_model_path(Tier::Small).unwrap(),
            rec_model_path(Tier::Small).unwrap(),
            dict_path(Tier::Small).unwrap(),
        )
    }

    #[tokio::test]
    #[ignore = "需要真实桌面会话 + 已下载 v6 small 模型（det/rec/dict）"]
    async fn recognize_screenshot_real() {
        let (det, rec, dict) = model_paths();
        let provider = PaddleOcrProvider::new(&det, &rec, &dict).expect("加载 OCR 模型失败");

        // 截取当前屏幕作为输入（真实场景：屏幕上应有文字）。
        let capture = WindowsCaptureProvider::new();
        let frames = capture.capture_all().await.expect("截图失败");
        let frame = frames.into_iter().next().expect("无显示器");
        let img = image::DynamicImage::ImageRgba8(frame.image);

        let lines = provider
            .recognize(&img, Lang::En)
            .await
            .expect("OCR 识别失败");
        println!("识别到 {} 行文字", lines.len());
        for line in lines.iter().take(30) {
            println!("  [{:.2}] {}", line.confidence, line.text);
        }
        assert!(
            !lines.is_empty(),
            "未识别到文字——截图可能无文字，或 v6 模型与 oar-ocr/dict 不兼容"
        );
    }
}
