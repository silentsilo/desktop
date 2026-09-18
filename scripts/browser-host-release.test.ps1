# Exercises Get-BrowserHostPlan against fixture lists, and against the real
# ones, without the signing token or a build.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\browser-host-release.test.ps1

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "browser-host-release.ps1")

$repoRoot = Split-Path -Parent $PSScriptRoot
$realOrigins = Join-Path $repoRoot "crates\silentsilo-browser-host\allowed-origins.json"
$realDev = Join-Path $repoRoot "crates\silentsilo-browser-host\allowed-origins.dev.json"
$devId = @((Get-Content $realDev -Raw | ConvertFrom-Json).allowed_origins)[0]
$storeId = "chrome-extension://bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/"
$dir = Join-Path $env:TEMP "silentsilo-host-plan-test"

$failures = 0
function Check($name, $condition) {
    if ($condition) { Write-Host "  PASS  $name" -ForegroundColor Green }
    else { Write-Host "  FAIL  $name" -ForegroundColor Red; $script:failures++ }
}

function Plan([string[]]$Chrome, [string[]]$Edge) {
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir | Out-Null }
    $path = Join-Path $dir "allowed-origins.json"
    @{ chrome_web_store = @($Chrome); edge_add_ons = @($Edge) } | ConvertTo-Json | Set-Content $path
    Get-BrowserHostPlan -OriginsPath $path -DevPath $realDev
}

function Throws([scriptblock]$Body) {
    try { & $Body | Out-Null; $false } catch { $true }
}

Write-Host "`n== The lists in this tree =="
$real = Get-BrowserHostPlan -OriginsPath $realOrigins -DevPath $realDev
Write-Host "  plan: $real"
Check "ships or leaves out, never refuses" ($real -in @("ship", "leave-out"))

Write-Host "`n== Fixtures =="
Check "both empty: left out" ((Plan @() @()) -eq "leave-out")
Check "a Chrome id: shipped" ((Plan @($storeId) @()) -eq "ship")
Check "an Edge id only: shipped" ((Plan @() @($storeId)) -eq "ship")
Check "the dev id: refused" (Throws { Plan @($storeId, $devId) @() })
Check "the dev id alone: refused" (Throws { Plan @() @($devId) })
Check "a wildcard: refused" (Throws { Plan @("chrome-extension://*/") @() })
Check "upper case: refused" (Throws { Plan @($storeId.ToUpper()) @() })
Check "no trailing slash: refused" (Throws { Plan @($storeId.TrimEnd('/')) @() })
Check "an empty string: refused" (Throws { Plan @("") @() })

if (Test-Path $dir) { Remove-Item $dir -Recurse -Force }
Write-Host ""
if ($failures -gt 0) { Write-Host "$failures failed" -ForegroundColor Red; exit 1 }
Write-Host "all checks passed" -ForegroundColor Green
