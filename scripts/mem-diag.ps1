# 内存诊断压测：经 single-instance argv 触发后端 OCR 管线 + 读 [mem_diag] 日志 + CSV 报告。
#
# 绕过 GUI/鼠标：直接 `snaptext.exe --diag-ocr` 让已运行实例在 Rust 内跑完整
# capture → crop → OCR → translate 流程（见 ocr_translate.rs::diag_run_ocr_pipeline）。
# 比模拟鼠标可靠（选区窗 show 有异步时序，鼠标事件可能丢失）。
#
# 用法（SnapText 已运行 + OCR 模型已下载 + 翻译 Key 已配时）：
#   .\scripts\mem-diag.ps1                            # 默认：全屏 30 次
#   .\scripts\mem-diag.ps1 -Iterations 50             # 50 次
#   .\scripts\mem-diag.ps1 -Scenario random           # 随机尺寸（验证动态 shape 扩容）
#   .\scripts\mem-diag.ps1 -Scenario random -Iterations 50
#
# 前置：
#   1. SnapText 已运行（脚本会自动定位 exe，经 single-instance 触发已运行实例）
#   2. OCR 模型已下载 + auto_ocr 无关（诊断命令直接调 run_ocr，不经前端开关）
#   3. 翻译 API Key 已配（否则管线在翻译步报错，但 OCR 打点仍有效）
#
# 输出：
#   - 控制台实时进度 + 最终判读指南
#   - CSV 报告到 %TEMP%\snaptext-mem-diag-<timestamp>.csv

param(
    [int]$Iterations = 30,
    [int]$StartDelaySec = 3,
    # 场景：full=全屏(验证 ort arena 线性增长) / random=随机尺寸(验证动态 shape 扩容)
    [ValidateSet("full", "random")]
    [string]$Scenario = "full",
    # 每次管线间隔（毫秒），需足够长让 OCR+翻译跑完（medium 档 OCR ~3s）
    [int]$StepDelayMs = 5000
)

$ErrorActionPreference = "Stop"
$logPath = Join-Path $env:APPDATA "SnapText\logs\snaptext.log"
$csvPath = Join-Path $env:TEMP "snaptext-mem-diag-$(Get-Date -Format 'yyyyMMdd-HHmmss').csv"

Write-Host "=== 内存诊断压测（argv 触发，绕过 GUI）===" -ForegroundColor Cyan
Write-Host "场景: $Scenario | 次数: $Iterations | 步长: ${StepDelayMs}ms"
Write-Host "日志: $logPath"
Write-Host "CSV:  $csvPath"
Write-Host ""

# 定位 snaptext.exe：优先已安装版，回退 target debug/release。
function Find-SnapTextExe {
    $candidates = @(
        (Join-Path $env:LOCALAPPDATA "SnapText\snaptext.exe"),
        (Join-Path $PSScriptRoot "..\target\release\snaptext.exe"),
        (Join-Path $PSScriptRoot "..\target\debug\snaptext.exe")
    )
    foreach ($p in $candidates) {
        $full = if (Test-Path $p) { (Resolve-Path $p).Path } else { $null }
        if ($full) { return $full }
    }
    return $null
}

$exePath = Find-SnapTextExe
if (-not $exePath) {
    Write-Error "未找到 snaptext.exe。请先 cargo run -p snaptext 或安装 SnapText。"
}
Write-Host "exe: $exePath"

# 确认 SnapText 已运行（single-instance 插件要求已有实例在跑，否则 argv 无接收方）。
$proc = Get-Process -Name snaptext -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $proc) {
    Write-Error "SnapText 未运行。single-instance 需已有实例接收 --diag-ocr。请先启动 SnapText。"
}

# 记录压测开始时的日志行数，之后只解析新增行（避免旧数据干扰）。
$startLogLine = 0
if (Test-Path $logPath) {
    $startLogLine = (Get-Content $logPath | Measure-Object -Line).Lines
}

# 从日志尾部解析最新的 mem_diag 记录。返回 hashtable 或 $null。
function Get-LatestMemDiag {
    param([string]$NodePattern)
    if (-not (Test-Path $logPath)) { return $null }
    $lines = Get-Content $logPath | Select-Object -Skip $startLogLine
    $matched = $lines | Where-Object { $_ -match "mem_diag" -and $_ -match "node=$NodePattern" }
    $last = $matched | Select-Object -Last 1
    if (-not $last) { return $null }
    # 解析 key=value 对（tracing 默认 fmt 输出空格分隔）。
    $result = @{}
    foreach ($token in $last -split '\s+') {
        if ($token -match '^(\w+)=(.+)$') {
            $result[$Matches[1]] = $Matches[2]
        }
    }
    return $result
}

Write-Host "等待 ${StartDelaySec}s 后开始..."
Start-Sleep -Seconds $StartDelaySec

# CSV 表头。
"iteration,ocr_before_main_priv_mb,ocr_after_main_priv_mb,ocr_after_children_priv_mb,ocr_after_children_count,ocr_delta_mb" |
    Out-File -FilePath $csvPath -Encoding utf8

# 采基线（压测前的最新 ocr 记录，可能为 null 如果还没跑过 OCR）。
$baseline = Get-LatestMemDiag "ocr_after"
$baselinePriv = if ($baseline) { $baseline.main_priv_mb } else { "N/A" }
Write-Host "基线 main_priv_mb: $baselinePriv MB"
Write-Host ""

# 主循环。
for ($i = 1; $i -le $Iterations; $i++) {
    # 构造 argv：random 场景每次随机尺寸；full 场景纯 --diag-ocr（全屏）。
    if ($Scenario -eq "random") {
        $w = Get-Random -Minimum 400 -Maximum 1600
        $h = Get-Random -Minimum 300 -Maximum 1000
        $arg = "--diag-ocr=${w}x${h}"
    } else {
        $arg = "--diag-ocr"
    }

    # 经 single-instance 触发已运行实例跑诊断管线。-NoNewWindow -Wait:$false 不阻塞。
    Start-Process -FilePath $exePath -ArgumentList $arg -NoNewWindow

    # 等待管线完成（OCR+翻译 ~3-5s，diag 打点在管线内写入日志）。
    Start-Sleep -Milliseconds $StepDelayMs

    # 进程存活检查。
    $proc = Get-Process -Id $proc.Id -ErrorAction SilentlyContinue
    if (-not $proc) { Write-Error "第 $i 次管线后进程消失（崩溃）" }

    # 从日志解析本次的 ocr_before / ocr_after 内存。
    $ocrBefore = Get-LatestMemDiag "ocr_before"
    $ocrAfter = Get-LatestMemDiag "ocr_after"
    $beforePriv = if ($ocrBefore) { $ocrBefore.main_priv_mb } else { "?" }
    $afterPriv = if ($ocrAfter) { $ocrAfter.main_priv_mb } else { "?" }
    $childrenPriv = if ($ocrAfter) { $ocrAfter.children_priv_mb } else { "?" }
    $childrenCount = if ($ocrAfter) { $ocrAfter.children } else { "?" }

    # 计算 delta（仅当能解析出数字时）。
    $delta = "?"
    if ($beforePriv -match '^\d+$' -and $afterPriv -match '^\d+$') {
        $delta = [int]$afterPriv - [int]$beforePriv
    }

    # 写 CSV。
    "$i,$beforePriv,$afterPriv,$childrenPriv,$childrenCount,$delta" |
        Out-File -FilePath $csvPath -Append -Encoding utf8

    # 控制台进度。
    Write-Host ("  [{0,3}/{1}] ocr: {2}->{3} MB (delta {4}) | children_priv={5}MB ({6} procs)" `
            -f $i, $Iterations, $beforePriv, $afterPriv, $delta, $childrenPriv, $childrenCount)
}

# 总结。
Write-Host ""
Write-Host "=== 诊断总结 ===" -ForegroundColor Cyan
$final = Get-LatestMemDiag "ocr_after"
if ($final) {
    Write-Host ("主进程私有字节: {0} -> {1} MB" -f $baselinePriv, $final.main_priv_mb)
    Write-Host ("子进程私有字节: {0} MB（{1} 个子进程）" -f $final.children_priv_mb, $final.children)
    if ($baselinePriv -match '^\d+$') {
        $totalDelta = [int]$final.main_priv_mb - [int]$baselinePriv
        Write-Host ("主进程总增量: {0} MB（{1} 次管线）" -f $totalDelta, $Iterations)
        Write-Host ""
        Write-Host "判读指南:" -ForegroundColor Yellow
        Write-Host "  - main_priv 线性增长  -> ort arena 渐进扩容（方案：配 arena 或重建 session）"
        Write-Host "  - main_priv 平台期    -> arena 非主因，查 children 或其他"
        Write-Host "  - children_priv 涨     -> WebView2 渲染内存累积（方案：前端 canvas/img 释放）"
        Write-Host "  - random 场景比 full 涨更多 -> 动态 shape 扩容假说成立"
    }
} else {
    Write-Host "未在日志中找到 ocr_after 记录——可能 OCR 模型未就绪或管线出错。" -ForegroundColor Yellow
    Write-Host "检查日志: $logPath（grep '诊断管线失败' 或 'OCR 模型未就绪'）"
}
Write-Host ""
Write-Host "CSV 报告: $csvPath"
Write-Host "完整日志: $logPath（grep mem_diag 查看全部打点）"
