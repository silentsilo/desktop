# The platform map that goes into latest.json, which is the one file every
# installed client reads. Separated from build-release-local.ps1 so it can be
# exercised without the signing token: a test that mirrors this logic instead
# of calling it drifts, and then reassures about code nobody runs.
#
# Dot-source it. It defines functions and does nothing else.

<#
.SYNOPSIS
Builds the `platforms` map for latest.json.

.DESCRIPTION
Windows comes from this machine, because its bytes only exist here: the
Authenticode signature changes the installer, so the updater signature has to
be taken after it. That is the reason the installer and latest.json come from
the same place.

Every other platform is built and signed by CI and is already attached to the
draft, so its entry is read from there rather than rebuilt. That does not bend
the rule: nothing re-signs those bytes afterwards, so the signature on the
draft covers exactly what ships.

Keys carry the installer, because the updater asks for
`{os}-{arch}-{installer}` before `{os}-{arch}`. A .deb client and an AppImage
client need different files, and a bare `linux-x86_64` would hand one of them
something it cannot install. macOS is the exception: one universal archive
answers both architectures, and the updater never asks for `darwin-universal`.

A platform missing from the draft is skipped, not invented. That is a
legitimate release (Windows only, or a platform not shipping yet) and also
what a failed CI job looks like, so it is reported rather than swallowed.
#>
function New-ManifestPlatforms {
    param(
        [Parameter(Mandatory)][string]$Tag,
        [Parameter(Mandatory)][string]$Slug,
        # Where the Windows installer and its .sig already are, and where the
        # signatures pulled off the draft will land.
        [Parameter(Mandatory)][string]$OutDir,
        [Parameter(Mandatory)][string]$SetupName
    )

    $platforms = [ordered]@{
        "windows-x86_64" = [ordered]@{
            signature = (Get-Content (Join-Path $OutDir "$SetupName.sig") -Raw).Trim()
            url       = "https://github.com/$Slug/releases/download/$Tag/$SetupName"
        }
    }

    # Patterns rather than literal names: the bundler puts the version in most
    # of these and not in the macOS archive, which is its choice, not ours.
    $fromDraft = @(
        @{ Keys = @("linux-x86_64-deb");                Pattern = "*_amd64.deb" }
        @{ Keys = @("linux-x86_64-appimage");           Pattern = "*_amd64.AppImage" }
        @{ Keys = @("darwin-aarch64", "darwin-x86_64"); Pattern = "*.app.tar.gz" }
    )

    $assets = @()
    try {
        $assets = (gh release view $Tag --json assets | ConvertFrom-Json).assets.name
    }
    catch {
        Write-Host "  could not read the draft's assets: $_" -ForegroundColor Yellow
    }

    foreach ($entry in $fromDraft) {
        $asset = $assets |
            Where-Object { $_ -like $entry.Pattern -and $_ -notlike "*.sig" } |
            Select-Object -First 1
        if (-not $asset) {
            Write-Host ("  {0,-40} no asset matching {1}, skipped" -f ($entry.Keys -join ", "), $entry.Pattern) -ForegroundColor Yellow
            continue
        }
        if (-not ($assets | Where-Object { $_ -eq "$asset.sig" })) {
            throw "$asset is on the draft but $asset.sig is not. An entry without its signature is an update the app refuses."
        }
        # Downloaded rather than assumed: the signature has to be the one
        # sitting next to the file people will actually download.
        gh release download $Tag --pattern "$asset.sig" --dir $OutDir --clobber
        if (-not $?) { throw "downloading $asset.sig from the draft failed" }
        $signature = (Get-Content (Join-Path $OutDir "$asset.sig") -Raw).Trim()
        if (-not $signature) { throw "$asset.sig is empty" }

        foreach ($key in $entry.Keys) {
            $platforms[$key] = [ordered]@{
                signature = $signature
                url       = "https://github.com/$Slug/releases/download/$Tag/$asset"
            }
        }
        Write-Host ("  {0,-40} {1}" -f ($entry.Keys -join ", "), $asset) -ForegroundColor Gray
    }

    return $platforms
}

<#
.SYNOPSIS
Whether one version ranks above another, the way the updater ranks them.

.DESCRIPTION
Semver precedence, prereleases included: 1.2.0-rc.1 ranks above 1.1.0 and
below 1.2.0, and rc.10 above rc.9. Build metadata is ignored. Written here
rather than taken from the `semver` package, which is not a dependency of
this repository and without `-p` refused every prerelease.
#>
function Test-VersionAbove {
    param(
        [Parameter(Mandatory)][string]$Version,
        [Parameter(Mandatory)][string]$Than
    )

    function Split-Version([string]$v) {
        $m = [regex]::Match($v, '^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$')
        if (-not $m.Success) { throw "not a version: $v" }
        $pre = if ($m.Groups[4].Success) { $m.Groups[4].Value -split '\.' } else { @() }
        return @{
            Core = @([long]$m.Groups[1].Value, [long]$m.Groups[2].Value, [long]$m.Groups[3].Value)
            Pre  = $pre
        }
    }

    $a = Split-Version $Version
    $b = Split-Version $Than
    for ($i = 0; $i -lt 3; $i++) {
        if ($a.Core[$i] -ne $b.Core[$i]) { return $a.Core[$i] -gt $b.Core[$i] }
    }
    # A release ranks above any prerelease of it.
    if ($a.Pre.Count -eq 0) { return $b.Pre.Count -gt 0 }
    if ($b.Pre.Count -eq 0) { return $false }
    for ($i = 0; $i -lt [Math]::Min($a.Pre.Count, $b.Pre.Count); $i++) {
        $x = $a.Pre[$i]; $y = $b.Pre[$i]
        $xNum = $x -match '^\d+$'; $yNum = $y -match '^\d+$'
        if ($xNum -and $yNum) {
            if ([long]$x -ne [long]$y) { return [long]$x -gt [long]$y }
        }
        elseif ($xNum -ne $yNum) {
            # Numeric identifiers rank below alphanumeric ones.
            return $yNum
        }
        elseif ($x -cne $y) {
            return [string]::CompareOrdinal($x, $y) -gt 0
        }
    }
    return $a.Pre.Count -gt $b.Pre.Count
}
