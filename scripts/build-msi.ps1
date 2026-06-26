# DU-13 调用 cargo-wix 生成 MSI 安装包。
#
# 前置：
#   1. 安装 WiX Toolset v3（https://wixtoolset.org/）并加入 PATH
#   2. cargo install cargo-wix
# 用法：.\scripts\build-msi.ps1
# 产出：target\wix\snaptext-<version>-x86_64.msi

$ErrorActionPreference = "Stop"

Write-Host "检查 cargo-wix..."
cargo wix --version | Out-Null
if ($LASTEXITCODE -ne 0) {
    Write-Error "未安装 cargo-wix。请先：cargo install cargo-wix，并安装 WiX Toolset v3。"
    exit 1
}

Write-Host "构建 release 二进制..."
cargo build --release -p snaptext-app
if ($LASTEXITCODE -ne 0) { Write-Error "release 构建失败"; exit 1 }

Write-Host "生成 MSI（cargo wix）..."
cargo wix -p snaptext-app --nocapture
if ($LASTEXITCODE -ne 0) { Write-Error "MSI 生成失败"; exit 1 }

Write-Host "完成。MSI 在 target\wix\ 下。"
