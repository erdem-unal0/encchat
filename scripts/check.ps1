# One-shot verification for encchat: format, lint, test, and build the web bundle.
#
# Usage:  powershell -ExecutionPolicy Bypass -File scripts/check.ps1
#
# Windows blocks .ps1 files under the default execution policy, hence the
# explicit -ExecutionPolicy Bypass above.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root

try {
    $cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
    if (-not (Test-Path $cargo)) { $cargo = 'cargo' }

    Write-Host '== cargo fmt --check ==' -ForegroundColor Cyan
    & $cargo fmt --all -- --check

    Write-Host '== cargo clippy ==' -ForegroundColor Cyan
    & $cargo clippy --all-targets -- -D warnings

    Write-Host '== cargo test ==' -ForegroundColor Cyan
    & $cargo test

    if (Get-Command npm.cmd -ErrorAction SilentlyContinue) {
        Write-Host '== web ==' -ForegroundColor Cyan
        Push-Location web
        try {
            & npm.cmd run build:wasm
            & npm.cmd run test:wasm
            & npm.cmd run build
        } finally {
            Pop-Location
        }
    } else {
        Write-Host 'npm.cmd not found; skipping the web build.' -ForegroundColor Yellow
    }

    Write-Host ''
    Write-Host 'All checks passed.' -ForegroundColor Green
} finally {
    Pop-Location
}
