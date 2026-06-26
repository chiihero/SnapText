# DU-12 稳定性压测：模拟热键 + 鼠标连续框选，验证 0 崩溃 + 内存增长 < 50MB。
#
# 用法（SnapText 已运行时）：.\scripts\stress-test.ps1 -Iterations 100
# 注：需以管理员/SnapText 同会话运行，使用 Win32 API 模拟按键与鼠标。
# 验收（DESIGN §9）：连续 100 次框选：0 崩溃，内存增长 < 50MB。

param(
    [int]$Iterations = 100,
    [int]$StartDelaySec = 3
)

$ErrorActionPreference = "Stop"
Write-Host "DU-12 稳定性压测：$Iterations 次框选（$StartDelaySec 秒后开始，请确保 SnapText 已运行）"
Start-Sleep -Seconds $StartDelaySec

Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class Win32 {
    [DllImport("user32.dll")] public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, UIntPtr dwExtraInfo);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint dwFlags, uint dx, uint dy, uint cButtons, UIntPtr dwExtraInfo);
    public const byte VK_CONTROL = 0x11, VK_MENU = 0x12, VK_Q = 0x51;
    public const uint KEYEVENTF_KEYUP = 0x0002;
    public const uint MOUSEEVENTF_LEFTDOWN = 0x0002, MOUSEEVENTF_LEFTUP = 0x0004;
}
"@

$proc = Get-Process -Name snaptext -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $proc) { Write-Error "未找到 snaptext 进程，请先 cargo run -p snaptext-app"; exit 1 }

$initialMem = $proc.WorkingSet64
for ($i = 1; $i -le $Iterations; $i++) {
    # 触发 Ctrl+Alt+Q
    [Win32]::keybd_event([Win32]::VK_CONTROL, 0, 0, [UIntPtr]::Zero)
    [Win32]::keybd_event([Win32]::VK_MENU, 0, 0, [UIntPtr]::Zero)
    [Win32]::keybd_event([Win32]::VK_Q, 0, 0, [UIntPtr]::Zero)
    [Win32]::keybd_event([Win32]::VK_Q, 0, [Win32]::KEYEVENTF_KEYUP, [UIntPtr]::Zero)
    [Win32]::keybd_event([Win32]::VK_MENU, 0, [Win32]::KEYEVENTF_KEYUP, [UIntPtr]::Zero)
    [Win32]::keybd_event([Win32]::VK_CONTROL, 0, [Win32]::KEYEVENTF_KEYUP, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 600

    # 框选矩形（左上 → 右下拖拽）
    [Win32]::SetCursorPos(300, 300)
    [Win32]::mouse_event([Win32]::MOUSEEVENTF_LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 150
    [Win32]::SetCursorPos(700, 500)
    [Win32]::mouse_event([Win32]::MOUSEEVENTF_LEFTUP, 0, 0, 0, [UIntPtr]::Zero)

    # 进程存活检查
    $proc = Get-Process -Id $proc.Id -ErrorAction SilentlyContinue
    if (-not $proc) { Write-Error "第 $i 次框选后进程消失（崩溃）"; exit 1 }
    if ($i % 10 -eq 0) {
        $deltaMB = [math]::Round(($proc.WorkingSet64 - $initialMem) / 1MB, 1)
        Write-Host "  已完成 $i / $Iterations，内存增量 $deltaMB MB"
    }
    Start-Sleep -Milliseconds 400
}

$deltaMB = [math]::Round(($proc.WorkingSet64 - $initialMem) / 1MB, 1)
Write-Host "完成 $Iterations 次框选：0 崩溃，内存增量 $deltaMB MB（验收 < 50MB）"
