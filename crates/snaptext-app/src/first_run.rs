//! 首次启动：模型缺失时下载（同步阻塞 main，简化版；DU-11 完整下载 UI 留后续增强）。

use std::time::Duration;

use anyhow::Result;
use snaptext_core::config::Tier;
use snaptext_core::model_manager::{self, downloader};

/// 确保指定档位模型就绪：缺失则从默认源下载（ModelScope）。
///
/// 同步阻塞（main 启动阶段，eframe 尚未进入事件循环）。下载进度打到 stderr。
pub fn ensure_models(tier: Tier) -> Result<()> {
    if model_manager::is_models_ready(tier) {
        return Ok(());
    }
    eprintln!(
        "首次启动：正在下载 PP-OCRv6 {:?} 模型（来源 ModelScope）...",
        tier
    );
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        downloader::download_models(tier, &client, &[], |role, received, total| {
            if let Some(t) = total {
                eprintln!("\t{role}: {received}/{t} 字节");
            } else {
                eprintln!("\t{role}: {received} 字节");
            }
        })
        .await
        .map_err(anyhow::Error::from)
    })?;
    eprintln!("模型下载完成。");
    Ok(())
}
