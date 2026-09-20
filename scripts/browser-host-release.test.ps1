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
# The Firefox id moved to the release list once AMO claimed it, so the dev
# list has none. The fixtures below use a stand-in for the dev-id rules.
$devFirefoxId = "dev-only@silentsilo.com"
# A stand-in for an id our AMO submission holds.
$firefoxId = "claimed@silentsilo.com"
$dir = Join-Path $env:TEMP "silentsilo-host-plan-test"

$failures = 0
function Check($name, $condition) {
    if ($condition) { Write-Host "  PASS  $name" -ForegroundColor Green }
    else { Write-Host "  FAIL  $name" -ForegroundColor Red; $script:failures++ }
}

function Plan([string[]]$Chrome, [string[]]$Edge, [string[]]$Firefox = @()) {
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir | Out-Null }
    $path = Join-Path $dir "allowed-origins.json"
    @{ chrome_web_store = @($Chrome); edge_add_ons = @($Edge); firefox_add_ons = @($Firefox) } |
        ConvertTo-Json | Set-Content $path
    # A fixture dev list, so the dev-id rules are exercised whatever the real
    # one holds today: the Firefox id left it when AMO claimed it.
    $devPath = Join-Path $dir "allowed-origins.dev.json"
    @{ allowed_origins = @($devId); firefox_add_ons = @($devFirefoxId) } |
        ConvertTo-Json | Set-Content $devPath
    Get-BrowserHostPlan -OriginsPath $path -DevPath $devPath
}

function Throws([scriptblock]$Body) {
    try { & $Body | Out-Null; $false } catch { $true }
}

Write-Host "`n== The lists in this tree =="
$real = Get-BrowserHostPlan -OriginsPath $realOrigins -DevPath $realDev
Write-Host "  plan: $real"
Check "ships or leaves out, never refuses" ($real -in @("ship", "leave-out"))
$devJson = Get-Content $realDev -Raw | ConvertFrom-Json
$relJson = Get-Content $realOrigins -Raw | ConvertFrom-Json
$realDevIds = @($devJson.allowed_origins) + @($devJson.firefox_add_ons)
$realLists = @($relJson.chrome_web_store) + @($relJson.edge_add_ons) + @($relJson.firefox_add_ons)
$shared = @($realDevIds | Where-Object { $realLists -contains $_ })
Check "the release lists hold no dev id" ($shared.Count -eq 0)

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
Check "the Firefox id only: shipped" ((Plan @() @() @($firefoxId)) -eq "ship")
Check "a GUID Firefox id: shipped" ((Plan @() @() @("{daf44bf7-a45e-4450-979c-91cf07434c3d}")) -eq "ship")
Check "all three: shipped" ((Plan @($storeId) @($storeId) @($firefoxId)) -eq "ship")
Check "the dev id as a Firefox id: refused" (Throws { Plan @() @() @($devId) })
Check "a Firefox id without a domain: refused" (Throws { Plan @() @() @("browser@") })
Check "a Firefox wildcard: refused" (Throws { Plan @() @() @("*") })
Check "a Firefox id over 80 characters: refused" (Throws { Plan @() @() @(("a" * 70) + "@silentsilo.com") })
Check "an empty Firefox id: refused" (Throws { Plan @() @() @("") })
Check "the dev id beside the Firefox id: refused" (Throws { Plan @($devId) @() @($firefoxId) })
Check "the real dev list holds no Firefox id" (@((Get-Content $realDev -Raw | ConvertFrom-Json).firefox_add_ons).Count -eq 0)
Check "the dev Firefox id: refused" (Throws { Plan @() @() @($devFirefoxId) })
Check "the dev Firefox id beside a store id: refused" (Throws { Plan @($storeId) @() @($firefoxId, $devFirefoxId) })

if (Test-Path $dir) { Remove-Item $dir -Recurse -Force }
Write-Host ""
if ($failures -gt 0) { Write-Host "$failures failed" -ForegroundColor Red; exit 1 }
Write-Host "all checks passed" -ForegroundColor Green
