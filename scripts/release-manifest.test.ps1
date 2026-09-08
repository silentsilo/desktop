# Exercises New-ManifestPlatforms against a stubbed draft, so the map that
# goes into latest.json can be checked without the signing token or a real
# release. It calls the shipping function rather than a copy of it, which is
# the point: a test that mirrors logic drifts away from it and then reassures
# about code nobody runs.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\release-manifest.test.ps1

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "release-manifest.ps1")

$tag = "v1.1.0"
$slug = "silentsilo/desktop"
$out = Join-Path $env:TEMP "silentsilo-manifest-test"
$setupName = "SilentSilo_1.1.0_x64-setup.exe"

function Reset-Fixture {
    param([string[]]$Assets)
    if (Test-Path $out) { Remove-Item $out -Recurse -Force }
    New-Item -ItemType Directory -Path $out | Out-Null
    Set-Content (Join-Path $out "$setupName.sig") "windows-signature" -NoNewline
    $script:fakeAssets = $Assets
}

# Stands in for the CLI. `release view` answers with the fixture's asset list,
# `release download` writes a signature file the way gh would.
function gh {
    $a = $args
    if ($a[0] -eq "release" -and $a[1] -eq "view") {
        return (@{ assets = @($script:fakeAssets | ForEach-Object { @{ name = $_ } }) } | ConvertTo-Json -Depth 5)
    }
    if ($a[0] -eq "release" -and $a[1] -eq "download") {
        $pattern = $a[($a.IndexOf("--pattern") + 1)]
        $dir = $a[($a.IndexOf("--dir") + 1)]
        Set-Content (Join-Path $dir $pattern) "signature-for-$pattern" -NoNewline
        return
    }
    throw "unexpected gh call: $($a -join ' ')"
}

function Build { New-ManifestPlatforms -Tag $tag -Slug $slug -OutDir $out -SetupName $setupName }

$failures = 0
function Check($name, $condition) {
    if ($condition) { Write-Host "  PASS  $name" -ForegroundColor Green }
    else { Write-Host "  FAIL  $name" -ForegroundColor Red; $script:failures++ }
}

Write-Host "`n== A Windows-only draft, which is every release so far =="
Reset-Fixture @("$setupName", "$setupName.sig", "silentsilo-extract-windows-x86_64.exe")
$p = Build
Check "exactly one platform, unchanged from before the other two existed" ($p.Keys.Count -eq 1)
Check "and it is windows-x86_64" ($p.Keys -contains "windows-x86_64")
Check "its signature is the one made on this machine" ($p["windows-x86_64"].signature -eq "windows-signature")
Check "its url points at the setup under this tag" ($p["windows-x86_64"].url -eq "https://github.com/$slug/releases/download/$tag/$setupName")

Write-Host "`n== A draft carrying all three platforms =="
Reset-Fixture @(
    "$setupName", "$setupName.sig",
    "SilentSilo_1.1.0_amd64.deb", "SilentSilo_1.1.0_amd64.deb.sig",
    "SilentSilo_1.1.0_amd64.AppImage", "SilentSilo_1.1.0_amd64.AppImage.sig",
    "SilentSilo.app.tar.gz", "SilentSilo.app.tar.gz.sig",
    "SilentSilo_1.1.0_universal.dmg"
)
$p = Build
# Five, not four: macOS needs one key per architecture over the one archive.
Check "five entries" ($p.Keys.Count -eq 5)
Check "the deb key exists" ($p.Keys -contains "linux-x86_64-deb")
Check "the appimage key exists" ($p.Keys -contains "linux-x86_64-appimage")
Check "both mac architectures exist" (($p.Keys -contains "darwin-aarch64") -and ($p.Keys -contains "darwin-x86_64"))
Check "no bare linux-x86_64, which would serve one format the other's file" (-not ($p.Keys -contains "linux-x86_64"))
Check "the deb url points at the deb" ($p["linux-x86_64-deb"].url -like "*/SilentSilo_1.1.0_amd64.deb")
Check "the appimage url points at the AppImage" ($p["linux-x86_64-appimage"].url -like "*/SilentSilo_1.1.0_amd64.AppImage")
Check "both mac keys share the one universal archive" ($p["darwin-aarch64"].url -eq $p["darwin-x86_64"].url -and $p["darwin-aarch64"].url -like "*/SilentSilo.app.tar.gz")
Check "the dmg is never offered to the updater" (-not ($p.Values.url -like "*.dmg"))
Check "each signature came from its own file" ($p["linux-x86_64-deb"].signature -eq "signature-for-SilentSilo_1.1.0_amd64.deb.sig")
Check "every url sits under this tag" (@($p.Values | Where-Object { $_.url -notlike "*/download/$tag/*" }).Count -eq 0)

Write-Host "`n== A draft where Linux built but its signature never arrived =="
Reset-Fixture @("$setupName", "$setupName.sig", "SilentSilo_1.1.0_amd64.deb")
$threw = $false
try { Build | Out-Null } catch { $threw = $true }
Check "refuses, rather than shipping an entry the app would reject" $threw

Write-Host "`n== Only macOS present, the Linux job having failed =="
Reset-Fixture @("$setupName", "$setupName.sig", "SilentSilo.app.tar.gz", "SilentSilo.app.tar.gz.sig")
$p = Build
Check "windows plus the two mac keys, nothing invented for Linux" ($p.Keys.Count -eq 3)

if (Test-Path $out) { Remove-Item $out -Recurse -Force }
Write-Host ""
if ($failures -gt 0) { Write-Host "$failures failed" -ForegroundColor Red; exit 1 }
Write-Host "all checks passed" -ForegroundColor Green
