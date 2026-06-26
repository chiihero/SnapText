//! PP-OCRv6 模型下载器（ModelScope `greatv/oar-ocr`）。
//!
//! 下载 det + rec + dict 三件套。oar-ocr 0.7+ 的 v6 模型托管在 ModelScope
//! （非 GitHub Releases，Releases 仅 v3-v5）。ModelScope 国内直连可达，无需加速镜像。
//! 流式下载，每 ~100KB 回调进度，写入临时 `.part`，完成后原子 rename。
//! `extra_mirrors` 为可选镜像前缀（拼为 `{mirror}/{filename}`），主源失败时尝试。

use std::path::Path;

use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

use crate::config::Tier;
use crate::error::ModelManagerError;
use crate::model_manager::{model_files, tier_variant};

/// ModelScope 上 oar-ocr 官方仓库（PP-OCRv6 ONNX + 字典）。
const MODELSCOPE_BASE: &str = "https://www.modelscope.cn/models/greatv/oar-ocr/resolve/master";

/// 角色 → ModelScope 文件名。det/rec 随 v6 尺寸（medium/small），dict 固定。
fn release_filename(role: &str, variant: &str) -> String {
    match role {
        "det" => format!("pp-ocrv6_{variant}_det.onnx"),
        "rec" => format!("pp-ocrv6_{variant}_rec.onnx"),
        "dict" => "ppocrv6_dict.txt".to_string(),
        other => unreachable!("未知模型角色：{other}"),
    }
}

/// 角色 → 候选下载 URL（ModelScope 主源 → 额外镜像）。
///
/// `extra_mirrors` 为完整路径前缀（如自建镜像），拼为 `{mirror}/{filename}`。
/// 默认空：ModelScope 国内直连可达，v6 无需 GitHub 加速。
pub fn candidate_urls(tier: Tier, role: &str, extra_mirrors: &[String]) -> Vec<String> {
    let variant = tier_variant(tier);
    let filename = release_filename(role, variant);
    let mut urls = vec![format!("{MODELSCOPE_BASE}/{filename}")];
    for mirror in extra_mirrors {
        urls.push(format!("{mirror}/{filename}"));
    }
    urls
}

/// 下载指定档位的全部模型文件（det + rec + dict）。
///
/// `on_progress(role, received_bytes, total_bytes)` 在每 ~100KB 与完成时回调。
pub async fn download_models(
    tier: Tier,
    client: &reqwest::Client,
    extra_mirrors: &[String],
    on_progress: impl Fn(&str, u64, Option<u64>),
) -> Result<(), ModelManagerError> {
    let files = model_files(tier)?;
    for (role, dest) in files {
        download_role(tier, role, &dest, client, extra_mirrors, &on_progress).await?;
    }
    Ok(())
}

async fn download_role(
    tier: Tier,
    role: &str,
    dest: &Path,
    client: &reqwest::Client,
    extra_mirrors: &[String],
    on_progress: &dyn Fn(&str, u64, Option<u64>),
) -> Result<(), ModelManagerError> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| ModelManagerError::Download(format!("创建目录失败：{e}")))?;
    }
    let urls = candidate_urls(tier, role, extra_mirrors);
    let mut errors = Vec::new();
    for url in &urls {
        match download_url(url, dest, client, role, on_progress).await {
            Ok(()) => return Ok(()),
            Err(e) => errors.push(format!("{url}: {e}")),
        }
    }
    Err(ModelManagerError::Download(format!(
        "所有源下载 {role} 失败：{}",
        errors.join("; ")
    )))
}

async fn download_url(
    url: &str,
    dest: &Path,
    client: &reqwest::Client,
    role: &str,
    on_progress: &dyn Fn(&str, u64, Option<u64>),
) -> Result<(), ModelManagerError> {
    // ModelScope WAF 对模型文件(.onnx)拦截非浏览器 UA（reqwest 默认 UA → 403），
    // 须伪装浏览器 UA（字典等小文件不受限，download_small_full_real 实测确认）。
    let resp = client
        .get(url)
        .header("accept", "*/*")
        .header(
            "user-agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        )
        .send()
        .await
        .map_err(|e| ModelManagerError::Download(format!("请求失败：{e}")))?;
    if !resp.status().is_success() {
        return Err(ModelManagerError::Download(format!(
            "HTTP {}",
            resp.status()
        )));
    }
    let total = resp.content_length();
    let tmp = dest.with_extension("part");
    // 写入 .part 临时文件；成功后原子 rename 为目标文件。
    // 任何中途失败（流读取 / 写入 / flush / rename）都清理残留 .part，避免磁盘碎片堆积。
    let outcome: Result<u64, ModelManagerError> = async {
        let mut file = tokio::fs::File::create(&tmp)
            .await
            .map_err(|e| ModelManagerError::Download(format!("创建临时文件失败：{e}")))?;
        let mut stream = resp.bytes_stream();
        let mut received: u64 = 0;
        let mut since_last_cb: u64 = 0;
        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|e| ModelManagerError::Download(format!("读取流失败：{e}")))?;
            file.write_all(&chunk)
                .await
                .map_err(|e| ModelManagerError::Download(format!("写入失败：{e}")))?;
            let n = chunk.len() as u64;
            received += n;
            since_last_cb += n;
            if since_last_cb >= 100 * 1024 {
                since_last_cb = 0;
                on_progress(role, received, total);
            }
        }
        file.flush()
            .await
            .map_err(|e| ModelManagerError::Download(format!("flush 失败：{e}")))?;
        drop(file);
        tokio::fs::rename(&tmp, dest)
            .await
            .map_err(|e| ModelManagerError::Download(format!("重命名失败：{e}")))?;
        Ok(received)
    }
    .await;
    match outcome {
        Ok(received) => {
            on_progress(role, received, total);
            Ok(())
        }
        Err(e) => {
            let _ = tokio::fs::remove_file(&tmp).await; // 清理残留 .part
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_urls_small_det() {
        let urls = candidate_urls(Tier::Small, "det", &[]);
        assert_eq!(
            urls[0],
            "https://www.modelscope.cn/models/greatv/oar-ocr/resolve/master/pp-ocrv6_small_det.onnx"
        );
    }

    #[test]
    fn candidate_urls_medium_rec() {
        let urls = candidate_urls(Tier::Medium, "rec", &[]);
        assert!(urls[0].ends_with("pp-ocrv6_medium_rec.onnx"));
    }

    #[test]
    fn candidate_urls_dict_fixed() {
        let urls = candidate_urls(Tier::Small, "dict", &[]);
        assert!(urls[0].ends_with("ppocrv6_dict.txt"));
    }

    #[test]
    fn candidate_urls_append_mirror() {
        let urls = candidate_urls(
            Tier::Small,
            "det",
            &["https://my-mirror.example/oar-ocr".to_string()],
        );
        assert_eq!(urls.len(), 2);
        assert!(urls[1].starts_with("https://my-mirror.example/oar-ocr/"));
        assert!(urls[1].ends_with("pp-ocrv6_small_det.onnx"));
    }

    #[tokio::test]
    #[ignore = "需要网络访问 GitHub Releases"]
    async fn download_small_full_real() {
        // 真实验证：下载 small 全套（det+rec+dict）到临时目录。
        let client = reqwest::Client::new();
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path();
        for (role, variant_path) in [
            ("det", "det.onnx"),
            ("rec", "rec.onnx"),
            ("dict", "dict.txt"),
        ] {
            let dest = base.join(variant_path);
            download_role(Tier::Small, role, &dest, &client, &[], &|_, _, _| {})
                .await
                .unwrap_or_else(|e| panic!("下载 {role} 失败：{e}"));
            assert!(dest.exists());
        }
        assert!(base.join("det.onnx").metadata().unwrap().len() > 1_000_000);
        assert!(base.join("rec.onnx").metadata().unwrap().len() > 1_000_000);
        assert!(base.join("dict.txt").metadata().unwrap().len() > 1_000);
    }
}
