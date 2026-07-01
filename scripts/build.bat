@echo off
REM SnapText 正式打包脚本：构建生产安装包 (nsis / msi)
cd /d "%~dp0.."

echo ============================================
echo   SnapText 正式打包脚本
echo   工作目录: %cd%
echo ============================================
echo.

echo [1/4] 检查 Node.js ...
where node >nul 2>nul
if errorlevel 1 (
    echo [错误] 未找到 Node.js，请从 https://nodejs.org 安装
    goto :end
)
echo       Node 正常
echo.

echo [2/4] 检查 Rust (cargo) ...
where cargo >nul 2>nul
if errorlevel 1 (
    echo [错误] 未找到 Rust，请从 https://rustup.rs 安装
    goto :end
)
echo       Cargo 正常
echo.

echo [3/4] 检查前端依赖 (node_modules) ...
if not exist "node_modules" (
    echo       缺少 node_modules，正在执行 npm install ...
    call npm install
    if errorlevel 1 (
        echo [错误] npm install 失败
        goto :end
    )
) else (
    echo       依赖已存在，跳过
)
echo.

echo [4/4] 启动 npm run tauri build ...
echo       将先编译前端 (vite build) 再编译 Rust 并生成安装包，请耐心等待...
echo --------------------------------------------
call npm run tauri build
set BUILD_CODE=%errorlevel%
echo --------------------------------------------

if %BUILD_CODE% neq 0 (
    echo [错误] 打包失败，退出码 = %BUILD_CODE%
    goto :end
)

echo.
echo [完成] 打包成功！安装包位于:
echo       src-tauri\target\release\bundle\nsis\
echo       src-tauri\target\release\bundle\msi\
echo.

:end
echo 按任意键关闭...
pause >nul
