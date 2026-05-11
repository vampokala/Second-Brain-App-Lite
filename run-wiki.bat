@echo off
setlocal EnableDelayedExpansion

set BROWSER_URL=http://localhost:1420

:: ── Node.js check ─────────────────────────────────────────────────────────────

where node >nul 2>&1
if %ERRORLEVEL% neq 0 (
    echo ERROR: Node.js is not installed.
    echo   Download it from https://nodejs.org ^(LTS version recommended^).
    pause
    exit /b 1
)

for /f "tokens=*" %%v in ('node -v 2^>nul') do set NODE_VER=%%v
echo Node %NODE_VER% detected.

:: ── npm install ───────────────────────────────────────────────────────────────

if not exist "node_modules\" (
    echo Installing npm packages...
    call npm install
    if !ERRORLEVEL! neq 0 (
        echo.
        echo ERROR: npm install failed.
        echo   Check your network connection or proxy settings and try again.
        pause
        exit /b 1
    )
) else (
    echo Dependencies up to date, skipping npm install.
)

:: ── Tauri desktop mode (requires Rust / cargo) ────────────────────────────────

where cargo >nul 2>&1
if %ERRORLEVEL% equ 0 (
    for /f "tokens=*" %%v in ('cargo --version 2^>nul') do set CARGO_VER=%%v
    echo !CARGO_VER! detected.
    echo Starting Second Brain Lite ^(Tauri desktop mode^)...
    echo.

    call npm run tauri:dev
    set TAURI_EXIT=!ERRORLEVEL!

    :: Exit 0 = user closed the window normally — don't fall back
    if !TAURI_EXIT! equ 0 exit /b 0

    echo.
    echo WARNING: Tauri exited with code !TAURI_EXIT!.
    echo   This can happen due to a firewall blocking localhost, a missing
    echo   WebView runtime, or a network error while downloading Rust crates.
    echo.
    echo Falling back to browser mode...
    goto :browser_mode

) else (
    echo.
    echo Rust / cargo not found -- Tauri desktop mode is unavailable.
    echo   Install Rust at https://rustup.rs for the full desktop experience.
    echo.
    echo Falling back to browser mode...
    goto :browser_mode
)

:browser_mode
echo.
echo Launching in browser mode ^-^> %BROWSER_URL%
echo Press Ctrl+C to stop.
echo.

:: Open browser after a short delay
start "" cmd /c "timeout /t 2 >nul && start %BROWSER_URL%"

call npm run dev
if %ERRORLEVEL% neq 0 (
    echo.
    echo ERROR: Failed to start the development server.
    echo   Make sure port 1420 is not already in use.
    pause
    exit /b 1
)
