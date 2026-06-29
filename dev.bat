@echo off
REM SnapText dev launcher: starts frontend Vite + Rust backend, opens app window
cd /d "%~dp0"

echo ============================================
echo   SnapText Dev Launcher
echo   CWD: %cd%
echo ============================================
echo.

echo [1/4] Checking Node.js ...
where node >nul 2>nul
if errorlevel 1 (
    echo [ERROR] Node.js not found. Install it from https://nodejs.org
    goto :end
)
echo       Node OK
echo.

echo [2/4] Checking Rust (cargo) ...
where cargo >nul 2>nul
if errorlevel 1 (
    echo [ERROR] Rust not found. Install it from https://rustup.rs
    goto :end
)
echo       Cargo OK
echo.

echo [3/4] Checking frontend deps (node_modules) ...
if not exist "node_modules" (
    echo       node_modules missing, running npm install ...
    call npm install
    if errorlevel 1 (
        echo [ERROR] npm install failed
        goto :end
    )
) else (
    echo       deps exist, skip
)
echo.

echo [4/4] Starting npm run tauri dev ...
echo       First run compiles Rust, please wait...
echo --------------------------------------------
call npm run tauri dev
echo --------------------------------------------
echo [DONE] tauri dev exited, code = %errorlevel%

:end
echo.
echo Press any key to close...
pause >nul
