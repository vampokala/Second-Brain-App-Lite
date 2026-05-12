# Prune build/debug log-like files older than N days under src-tauri\target and root npm-debug logs.
# Removes: *.log, Cargo build stderr, build-script output under debug|release\build, npm-debug.log*.
# Called from run-wiki.bat. Set RUN_WIKI_SKIP_TARGET_CLEANUP=1 to disable. RUN_WIKI_CLEAN_TARGET_DAYS overrides age.

param(
    [string]$RepoRoot = (Split-Path -Parent $PSScriptRoot),
    [int]$Days = 2
)

if ($env:RUN_WIKI_SKIP_TARGET_CLEANUP -eq '1') { exit 0 }

if ($null -ne $env:RUN_WIKI_CLEAN_TARGET_DAYS -and $env:RUN_WIKI_CLEAN_TARGET_DAYS -match '^\d+$') {
    $Days = [int]$env:RUN_WIKI_CLEAN_TARGET_DAYS
}

$cutoff = (Get-Date).AddDays(-$Days)
$removed = 0

function Remove-OldLogFiles {
    param([string]$Directory)
    if (-not (Test-Path -LiteralPath $Directory)) { return }
    Get-ChildItem -LiteralPath $Directory -Recurse -File -ErrorAction SilentlyContinue |
        Where-Object {
            $_.LastWriteTime -lt $cutoff -and (
                $_.Extension -eq '.log' -or
                $_.Name -eq 'stderr' -or
                ($_.Name -eq 'output' -and (
                    $_.FullName -match '[/\\]debug[/\\]build[/\\]' -or
                    $_.FullName -match '[/\\]release[/\\]build[/\\]'
                ))
            )
        } |
        ForEach-Object {
            Remove-Item -LiteralPath $_.FullName -Force -ErrorAction SilentlyContinue
            if (-not (Test-Path -LiteralPath $_.FullName)) { $script:removed++ }
        }
}

$targetDir = Join-Path $RepoRoot 'src-tauri\target'
Remove-OldLogFiles -Directory $targetDir

Get-ChildItem -LiteralPath $RepoRoot -File -ErrorAction SilentlyContinue |
    Where-Object {
        $_.LastWriteTime -lt $cutoff -and $_.Name -like 'npm-debug.log*'
    } |
    ForEach-Object {
        Remove-Item -LiteralPath $_.FullName -Force -ErrorAction SilentlyContinue
        if (-not (Test-Path -LiteralPath $_.FullName)) { $script:removed++ }
    }

if ($removed -gt 0) {
    Write-Host "Cleaned $removed stale log file(s) (older than $Days days) under target/ and repo root."
}
