@echo off
REM SnapText 开发启动器：启动前端 Vite + Rust 后端，打开应用窗口
cd /d "%~dp0.."

echo ============================================
echo   SnapText 开发模式启动器
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

echo [4/4] 启动 npm run tauri dev ...
echo       首次运行需编译 Rust，请耐心等待...
echo --------------------------------------------
call npm run tauri dev
echo --------------------------------------------
echo [完成] tauri dev 已退出，退出码 = %errorlevel%

:end
echo.
echo 按任意键关闭...
pause >nul
