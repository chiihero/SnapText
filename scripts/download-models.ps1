# DU-13 离线下载 PP-OCRv6 模型到 %APPDATA%\SnapText\models\ppocr\v6\{tier}\
# 用法：.\scripts\download-models.ps1 -Tier small   # 或 medium
#
# 默认源：ModelScope（greatv/oar-ocr）。v6 模型仅 ModelScope 有（GitHub Releases 仅 v3-v5）。

param(
    [ValidateSet("small", "medium")][string]$Tier = "small",
    [string]$Mirror = ""   # 可选镜像前缀，拼为 {Mirror}/{filename}
)

$ErrorActionPreference = "Stop"
$base = "https://www.modelscope.cn/models/greatv/oar-ocr/resolve/master"
if ($Mirror) { $base = $Mirror }

$destDir = Join-Path $env:APPDATA "SnapText\models\ppocr\v6\$Tier"
New-Item -ItemType Directory -Force -Path $destDir | Out-Null

$files = @{
    "det.onnx"  = "pp-ocrv6_${Tier}_det.onnx"
    "rec.onnx"  = "pp-ocrv6_${Tier}_rec.onnx"
    "dict.txt"  = "ppocrv6_dict.txt"
}

foreach ($k in $files.Keys) {
    $url = "$base/$($files[$k])"
    $dest = Join-Path $destDir $k
    if (Test-Path $dest) { Write-Host "已存在 $dest，跳过"; continue }
    Write-Host "下载 $url -> $dest"
    # ModelScope WAF 对 .onnx 模型文件拦截非浏览器 UA（同 downloader.rs），须伪装。
    Invoke-WebRequest -Uri $url -OutFile $dest -Headers @{ "User-Agent" = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36" }
}
Write-Host "模型下载完成：$destDir"
