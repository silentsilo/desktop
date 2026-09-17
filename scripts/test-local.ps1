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

# Working copies go under target\, not under the real %LOCALAPPDATA%.
#
# Several tests build a VaultPaths over a tempdir and call ensure_work_dir,
# and the work directory is derived from the silo path rather than kept
# inside it, so it lands in whatever `work_base()` answers. Left to itself
# that is %LOCALAPPDATA%\SilentSilo\work\open, where a fresh uuid-named
# directory per run accumulates forever next to the ones a real silo uses.
# Debug builds only, which is what a test run is; a release build has no
# switch that moves plaintext anywhere.
#
# Core does the same through a committed .cargo\config.toml. Here that file
# is gitignored (it is where the local [patch] at a core checkout goes), so
# it is set per run instead. `cargo test` typed by hand still writes to the
# real one; see README, "Dev".
$workBase = Join-Path $root 'target\test-work'
$previousWorkBase = $env:SILENTSILO_TEST_WORK_BASE
$env:SILENTSILO_TEST_WORK_BASE = $workBase
try {
    Invoke-Step 'cargo test' { cargo test --all --locked }
    Invoke-Step 'cargo check' { cargo check --all --locked }
}
finally {
    $env:SILENTSILO_TEST_WORK_BASE = $previousWorkBase
    if (Test-Path $workBase) { Remove-Item -Recurse -Force $workBase }
}

Write-Host "`nAll green." -ForegroundColor Green
