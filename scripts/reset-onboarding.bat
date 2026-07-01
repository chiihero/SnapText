@echo off
REM SnapText dev tool: reset onboarding so the guide shows again on next launch.
REM
REM Usage: double-click this file. Keeps API keys and downloaded models,
REM only flips onboarding_completed back to false in config.toml.
cd /d "%~dp0.."

echo ============================================
echo   SnapText - Reset Onboarding
echo ============================================
echo.

set "CFG=%APPDATA%\SnapText\config.toml"

if not exist "%CFG%" (
    echo [INFO] config.toml does not exist - already first-run state.
    echo        Path: %CFG%
    goto :end
)

powershell -NoProfile -ExecutionPolicy Bypass -Command "$p=$env:APPDATA+'\SnapText\config.toml'; $c=[IO.File]::ReadAllText($p); if($c -match 'onboarding_completed\s*=\s*true'){ $c=$c -replace 'onboarding_completed\s*=\s*true','onboarding_completed = false'; [IO.File]::WriteAllText($p,$c); Write-Host '[OK] onboarding_completed set to false' -ForegroundColor Green } else { Write-Host '[INFO] already false or field missing - no change needed' -ForegroundColor Yellow }"

echo.
echo Run "scripts\dev.bat" (dev launcher) to see the onboarding guide again.
echo.

:end
echo.
echo Press any key to close...
pause >nul
