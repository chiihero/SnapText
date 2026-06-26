//! OCR 模块：[`OcrProvider`] trait 与默认 [`PaddleOcrProvider`]（oar-ocr 后端）。

pub mod paddleocr;
pub mod postprocess;
pub mod preprocess;

pub use paddleocr::PaddleOcrProvider;

use async_trait::async_trait;

use crate::error::CoreError;
use crate::types::{Lang, OcrLine, ProviderId};

/// OCR 能力抽象。
///
/// 所有实现必须 `Send + Sync`。`recognize` 内部应用 `spawn_blocking` 承载 ONNX 推理
/// （CPU 密集，见 CONVENTIONS §3）。
#[async_trait]
pub trait OcrProvider: Send + Sync {
    /// Provider 标识。
    fn id(&self) -> ProviderId;
    /// 支持的语言列表。
    fn supported_languages(&self) -> &[Lang];
    /// 识别图像中的文字行。
    async fn recognize(
        &self,
        img: &image::DynamicImage,
        lang: Lang,
    ) -> Result<Vec<OcrLine>, CoreError>;
}
