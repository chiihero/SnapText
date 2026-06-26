//! 图像预处理。
//!
//! oar-ocr 后端内部完成 det 的 resize / pad / 归一化（PP-OCR 标准预处理），
//! 故本模块仅负责把输入图像转为 oar-ocr `predict` 要求的 8-bit `RgbImage`。
//! 若未来切 ort 自实现（DU-04 fallback），在此补 resize / normalize / pad。

use image::DynamicImage;

/// 转为 oar-ocr 所需的 `RgbImage`。
pub fn to_rgb(img: &DynamicImage) -> image::RgbImage {
    img.to_rgb8()
}
