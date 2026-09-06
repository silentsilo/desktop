# Everything CI runs for the desktop application, in one command.
#
# The suites that need a real bucket, share or SFTP account live in
# silentsilo/core along with the crates they exercise; run
# `scripts\test-local.ps1` there for those.
#
#   .\scripts\test-local.ps1              # the whole sequence
#   .\scripts\test-local.ps1 -RustOnly    # skip the frontend half
[CmdletBinding()]
param(
    # Skips the frontend half when only the Rust side is being iterated on.
    [switch]$RustOnly
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Invoke-Step {
    param([string]$Name, [scriptblock]$Body)
    Write-Host "`n=== $Name ===" -ForegroundColor Cyan
    & $Body
    if ($LASTEXITCODE -ne 0) { throw "$Name failed" }
}

# A `[patch]` pointing Cargo at a local core checkout is the normal way to
# work on both repositories at once, and it rewrites Cargo.lock. Saying so
# here means a green run with a patch in place is not mistaken for a green
# run against the pinned tag, which is what CI will build.
if (Test-Path (Join-Path $root '.cargo\config.toml')) {
    Write-Host "`n.cargo\config.toml is present: the core crates come from a local checkout, not from the pinned tag." -ForegroundColor Yellow
}
else {
    Invoke-Step 'lockfile' { node scripts\check-lockfile.mjs }
}

if (-not $RustOnly) {
    Invoke-Step 'typecheck' { npm run typecheck }
    Invoke-Step 'lint' { npm run lint }
    Invoke-Step 'frontend tests' { npm test }
    Invoke-Step 'frontend build' { npm run build }
}

Invoke-Step 'fmt' { cargo fmt --all -- --check }
Invoke-Step 'clippy' { cargo clippy --all-targets --locked -- -D warnings }
Invoke-Step 'cargo test' { cargo test --all --locked }
Invoke-Step 'cargo check' { cargo check --all --locked }

Write-Host "`nAll green." -ForegroundColor Green
