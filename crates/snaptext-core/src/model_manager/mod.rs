//! 模型管理：路径解析、完整性校验。
//!
//! 模型目录：可执行文件同级的 `models\ppocr\v6\{tier}\{det,rec}.onnx` + `dict.txt`（便携模式，跟程序走）。
//!
//! 模型来源：oar-ocr 官方 **PP-OCRv6**（medium/small）+ `ppocrv6_dict`。
//! oar-ocr 0.7+ 原生支持 v6（0.6.3 仅 v5，曾因此乱码改用 v5；升 0.7.1 后回归 v6）。
//! 模型文件托管在 ModelScope（`greatv/oar-ocr`），非 oar-ocr GitHub Releases（仅 v3-v5）。

pub mod downloader;

use std::path::{Path, PathBuf};

use crate::config::Tier;
use crate::error::ModelManagerError;

/// 模型档位 → 目录名。
pub fn tier_dir(tier: Tier) -> &'static str {
    match tier {
        Tier::Medium => "medium",
        Tier::Small => "small",
    }
}

/// 档位 → v6 尺寸名（v6 直接同名：medium/small，非 v5 的 server/mobile）。
pub fn tier_variant(tier: Tier) -> &'static str {
    match tier {
        Tier::Medium => "medium",
        Tier::Small => "small",
    }
}

/// 模型根目录：可执行文件同级的 `models\`（便携模式，模型跟程序走）。
///
/// 开发运行（`cargo run`）时位于 `target\{debug,release}\models\`；
/// 安装后位于安装目录。⚠️ 安装目录须可写——勿装到 `Program Files`，
/// 否则首启下载会因普通用户无写权限而失败。
pub fn model_root() -> Result<PathBuf, ModelManagerError> {
    let exe = std::env::current_exe()
        .map_err(|e| ModelManagerError::Download(format!("无法定位可执行文件：{e}")))?;
    let dir = exe
        .parent()
        .ok_or_else(|| ModelManagerError::Download("无法定位可执行文件所在目录".into()))?;
    Ok(dir.join("models"))
}

/// PP-OCR 模型目录：`<可执行文件目录>\models\ppocr\v6\{tier}\`。
///
/// 含 `v6` 段以隔离旧 v5 模型（曾存 `ppocr\{tier}\`）：升级后 `is_models_ready`
/// 查新路径不存在即触发 v6 重下，避免误用旧 v5 文件。
pub fn ppocr_dir(tier: Tier) -> Result<PathBuf, ModelManagerError> {
    Ok(model_root()?.join("ppocr").join("v6").join(tier_dir(tier)))
}

/// 检测模型路径。
pub fn det_model_path(tier: Tier) -> Result<PathBuf, ModelManagerError> {
    Ok(ppocr_dir(tier)?.join("det.onnx"))
}

/// 识别模型路径。
pub fn rec_model_path(tier: Tier) -> Result<PathBuf, ModelManagerError> {
    Ok(ppocr_dir(tier)?.join("rec.onnx"))
}

/// 字符字典路径（识别用）。
pub fn dict_path(tier: Tier) -> Result<PathBuf, ModelManagerError> {
    Ok(ppocr_dir(tier)?.join("dict.txt"))
}

/// 全部模型文件：`[(角色, 路径); 3]` = det + rec + dict。
pub fn model_files(tier: Tier) -> Result<[(&'static str, PathBuf); 3], ModelManagerError> {
    Ok([
        ("det", det_model_path(tier)?),
        ("rec", rec_model_path(tier)?),
        ("dict", dict_path(tier)?),
    ])
}

/// 检查指定档位的 det + rec + dict 是否齐全。
pub fn is_models_ready(tier: Tier) -> bool {
    model_files(tier)
        .map(|files| files.iter().all(|(_, p)| p.exists()))
        .unwrap_or(false)
}

/// 校验一组文件：文件存在；若提供预期 SHA256 则逐个比对。
pub fn verify_files(
    files: &[(&'static str, PathBuf)],
    expected: &[(String, String)],
) -> Result<(), ModelManagerError> {
    for (name, path) in files {
        if !path.exists() {
            return Err(ModelManagerError::ModelNotFound { path: path.clone() });
        }
        if let Some(h) = expected
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, h)| h.as_str())
        {
            let actual = sha256_file(path)?;
            if actual != h {
                return Err(ModelManagerError::Checksum(format!(
                    "{name} SHA256 不匹配：期望 {h}，实际 {actual}"
                )));
            }
        }
    }
    Ok(())
}

/// 校验指定档位的模型完整性。
pub fn verify_models(tier: Tier, expected: &[(String, String)]) -> Result<(), ModelManagerError> {
    let files = model_files(tier)?;
    let files_ref: Vec<(&'static str, PathBuf)> =
        files.iter().map(|(n, p)| (*n, p.clone())).collect();
    verify_files(&files_ref, expected)
}

/// 计算文件 SHA256（小写 hex）。
pub fn sha256_file(path: &Path) -> Result<String, ModelManagerError> {
    let data = std::fs::read(path)
        .map_err(|e| ModelManagerError::Download(format!("读取文件失败：{e}")))?;
    Ok(sha256_hex(&data))
}

/// 计算字节数组的 SHA256（小写 hex）。
pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_empty() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn paths_have_ppocr_v6_suffix() {
        // 便携模式：model_root = current_exe 同级 models/，测试 binary 位于 target/deps，
        // 前缀随运行位置变化，故仅断言后缀（ppocr/v6/{tier}/...）。
        let det = det_model_path(Tier::Small).unwrap();
        assert!(
            det.ends_with(r"ppocr\v6\small\det.onnx") || det.ends_with("ppocr/v6/small/det.onnx")
        );
        let dict = dict_path(Tier::Medium).unwrap();
        assert!(
            dict.ends_with(r"ppocr\v6\medium\dict.txt")
                || dict.ends_with("ppocr/v6/medium/dict.txt")
        );
    }

    #[test]
    fn tier_variant_mapping() {
        assert_eq!(tier_variant(Tier::Medium), "medium");
        assert_eq!(tier_variant(Tier::Small), "small");
    }

    #[test]
    fn verify_files_detects_missing() {
        // 自包含（tempdir）：rec 文件缺失 → 返回 ModelNotFound。
        // 替代旧的环境耦合测试（旧测试依赖全局 %APPDATA% 路径与本机下载状态，已下载即失败）。
        let tmp = tempfile::tempdir().unwrap();
        let det = tmp.path().join("det.onnx");
        std::fs::write(&det, b"det").unwrap();
        let files = vec![
            ("det", det),
            ("rec", tmp.path().join("rec.onnx")), // 故意不创建
        ];
        let err = verify_files(&files, &[]).unwrap_err();
        assert!(matches!(err, ModelManagerError::ModelNotFound { .. }));
    }

    #[test]
    fn verify_files_checks_sha256() {
        let tmp = tempfile::tempdir().unwrap();
        let det = tmp.path().join("det.onnx");
        let rec = tmp.path().join("rec.onnx");
        let dict = tmp.path().join("dict.txt");
        std::fs::write(&det, b"det-bytes").unwrap();
        std::fs::write(&rec, b"rec-bytes").unwrap();
        std::fs::write(&dict, b"dict-bytes").unwrap();
        let files = vec![
            ("det", det.clone()),
            ("rec", rec.clone()),
            ("dict", dict.clone()),
        ];

        let good = [("det".to_string(), sha256_hex(b"det-bytes"))];
        assert!(verify_files(&files, &good).is_ok());

        let bad = [("det".to_string(), "deadbeef".to_string())];
        assert!(verify_files(&files, &bad).is_err());
    }
}
