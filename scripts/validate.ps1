# Run from Windows PowerShell or PowerShell 7. This does not install packages,
# alter credentials, publish releases, or launch agent prompts.
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    foreach ($tool in @('cargo', 'node')) {
        if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
            throw "$tool is not on PATH. Install the prerequisites described in README.md."
        }
    }
    $nodeVersion = (& node --version).TrimStart('v')
    if ($nodeVersion -notmatch '^\d+\.\d+\.\d+$' -or [version]$nodeVersion -lt [version]'22.19.0') {
        throw "Pi requires stable Node 22.19.0 or newer; found $nodeVersion."
    }
    function Invoke-Checked {
        param([string]$Executable, [string[]]$Arguments)
        & $Executable @Arguments
        if ($LASTEXITCODE -ne 0) {
            throw "$Executable $($Arguments -join ' ') failed with exit code $LASTEXITCODE."
        }
    }
    Invoke-Checked -Executable 'node' -Arguments @('scripts/verify-source.mjs')
    Invoke-Checked -Executable 'cargo' -Arguments @('fmt', '--all', '--', '--check')
    Invoke-Checked -Executable 'cargo' -Arguments @('check', '--locked', '--all-targets')
    Invoke-Checked -Executable 'cargo' -Arguments @('test', '--locked', '--all-targets')
    $tests = @(Get-ChildItem -Path bridge -Filter '*.test.mjs' | Sort-Object Name | ForEach-Object FullName)
    Invoke-Checked -Executable 'node' -Arguments (@('--test') + $tests)
    Invoke-Checked -Executable 'cargo' -Arguments @('clippy', '--locked', '--all-targets', '--all-features', '--', '-D', 'warnings')
} finally {
    Pop-Location
}
