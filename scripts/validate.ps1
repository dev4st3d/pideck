# Final checks for the native terminal manager.
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
Push-Location $projectRoot
try {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw 'Cargo is not on PATH. Install the Rust prerequisites described in README.md.'
    }
    function Invoke-Cargo {
        param([string[]]$Arguments)
        & cargo @Arguments
        if ($LASTEXITCODE -ne 0) {
            throw "cargo $($Arguments -join ' ') failed with exit code $LASTEXITCODE."
        }
    }
    Invoke-Cargo -Arguments @('fmt', '--all', '--', '--check')
    Invoke-Cargo -Arguments @('check', '--locked', '--all-targets')
    Invoke-Cargo -Arguments @('test', '--locked', '--all-targets')
    Invoke-Cargo -Arguments @('clippy', '--locked', '--all-targets', '--all-features', '--', '-D', 'warnings')
} finally {
    Pop-Location
}
