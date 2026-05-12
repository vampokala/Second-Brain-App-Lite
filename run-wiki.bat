@echo off
setlocal EnableDelayedExpansion

:: Always run from the directory that contains this script (cmd shortcuts, other CWD)
cd /d "%~dp0" || exit /b 1

:: Prune stale Cargo/npm log-like files under src-tauri\target (see scripts\cleanup-old-target-logs.ps1).
if not "%RUN_WIKI_SKIP_TARGET_CLEANUP%"=="1" (
    powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\cleanup-old-target-logs.ps1"
)

set BROWSER_URL=http://localhost:1420

:: ── Dependencies (npm + optional Rust prefetch) ───────────────────────────────

call "%~dp0scripts\install-deps.bat" --allow-partial-npm
if errorlevel 1 exit /b 1

:: ── Tauri desktop mode (requires Rust / cargo) ────────────────────────────────

where cargo >nul 2>&1
if %ERRORLEVEL% equ 0 (
    for /f "tokens=*" %%v in ('cargo --version 2^>nul') do set CARGO_VER=%%v
    echo !CARGO_VER! detected.

    echo Starting Second Brain Lite ^(Tauri desktop mode^)...
    echo.

    call npm run tauri:dev
    set TAURI_EXIT=!ERRORLEVEL!

    if !TAURI_EXIT! equ 0 exit /b 0

    :: User interrupt (Ctrl+C) — do not fall back
    if "!TAURI_EXIT!"=="-1073741510" exit /b 130
    if "!TAURI_EXIT!"=="3221225786" exit /b 130
    if !TAURI_EXIT! equ 130 exit /b 130
    if !TAURI_EXIT! equ 143 exit /b 143

    :: Rust compile failure — fix code; do not mask with browser
    if !TAURI_EXIT! equ 101 (
        echo.
        echo Rust build failed ^(exit 101^). Fix the errors above.
        exit /b 101
    )

    echo.
    echo WARNING: Tauri exited with code !TAURI_EXIT!.
    echo   Falling back to browser mode ^(often caused by firewall, WebView, or localhost^).
    goto :browser_mode

) else (
    echo.
    echo Rust / cargo not found -- Tauri desktop mode is unavailable.
    echo   Install Rust at https://rustup.rs for the full desktop experience.
    echo.
    echo Starting browser mode...
    goto :browser_mode
)

:browser_mode
echo.
echo Launching in browser mode ^-^> %BROWSER_URL%
echo Press Ctrl+C to stop.
echo.

start "" cmd /c "timeout /t 2 /nobreak >nul 2>&1 && start "" %BROWSER_URL%"

call npm run dev
if %ERRORLEVEL% neq 0 (
    echo.
    echo ERROR: Failed to start the development server.
    echo   Make sure port 1420 is not already in use.
    pause
    exit /b 1
)
