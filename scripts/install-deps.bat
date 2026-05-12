@echo off
setlocal EnableDelayedExpansion

:: Install npm packages and optionally prefetch Rust crates.
::   scripts\install-deps.bat
::   scripts\install-deps.bat --npm-only
::   scripts\install-deps.bat --rust-only
::   scripts\install-deps.bat --allow-partial-npm

set "REPO_ROOT=%~dp0.."
cd /d "%REPO_ROOT%" || exit /b 1

set DO_NPM=1
set DO_RUST=1
set ALLOW_PARTIAL=0

:parse
if "%~1"=="" goto done_parse
if /i "%~1"=="--npm-only" (set DO_NPM=1&set DO_RUST=0&shift&goto parse)
if /i "%~1"=="--rust-only" (set DO_NPM=0&set DO_RUST=1&shift&goto parse)
if /i "%~1"=="--allow-partial-npm" (set ALLOW_PARTIAL=1&shift&goto parse)
echo Unknown option: %~1
exit /b 1
:done_parse

if not defined SB_LITE_NPM_RETRIES set SB_LITE_NPM_RETRIES=3

if "!DO_NPM!"=="1" (
    where node >nul 2>&1
    if !ERRORLEVEL! neq 0 (
        echo ERROR: Node.js is not installed. https://nodejs.org
        exit /b 1
    )
    where npm >nul 2>&1
    if !ERRORLEVEL! neq 0 (
        echo ERROR: npm is not available.
        exit /b 1
    )
    for /f "tokens=*" %%v in ('node -v 2^>nul') do echo Node %%v detected.

    set NPM_OK=0
    for /l %%i in (1,1,!SB_LITE_NPM_RETRIES!) do (
        if !NPM_OK! equ 0 (
            echo Installing npm packages ^(attempt %%i/!SB_LITE_NPM_RETRIES!^)...
            call npm install --no-fund --no-audit
            if !ERRORLEVEL! equ 0 (set NPM_OK=1) else (
                if %%i lss !SB_LITE_NPM_RETRIES! (
                    echo   Install failed -- retrying in 3s...
                    timeout /t 3 /nobreak >nul 2>&1
                )
            )
        )
    )
    if !NPM_OK! neq 1 (
        echo.
        echo ERROR: npm install failed after !SB_LITE_NPM_RETRIES! attempts.
        if exist "node_modules\" (
            if "!ALLOW_PARTIAL!"=="1" (
                echo   Continuing with existing node_modules ^(--allow-partial-npm^).
            ) else (
                echo   Fix network/firewall or run with --allow-partial-npm from run-wiki.
                exit /b 1
            )
        ) else (
            echo   No node_modules -- cannot continue.
            exit /b 1
        )
    )
)

if "!DO_RUST!"=="1" (
    if exist "src-tauri\Cargo.toml" (
        where cargo >nul 2>&1
        if !ERRORLEVEL! equ 0 (
            echo Prefetching Rust crates ^(cargo fetch^)...
            cargo fetch --manifest-path src-tauri\Cargo.toml
            if !ERRORLEVEL! neq 0 (
                echo WARNING: cargo fetch failed ^(network/firewall/proxy^).
            )
        ) else (
            echo Skipping Rust prefetch: cargo not on PATH.
        )
    )
)

echo Dependency install step finished.
exit /b 0
