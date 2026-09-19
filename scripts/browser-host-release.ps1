# Whether a release ships the browser extension's host. Separated from
# build-release-local.ps1 so the decision can be run without the signing
# token (browser-host-release.test.ps1). The same rule is
# `release_verdict` in crates/silentsilo-browser-host.
#
# Dot-source it. It defines a function and does nothing else.

<#
.SYNOPSIS
Returns "ship" or "leave-out", or throws when the lists are unfit.

.DESCRIPTION
All three store lists empty (Chrome Web Store, Edge Add-ons, Firefox
Add-ons): "leave-out". The extension has no store listing yet, and a desktop
release must not wait for one, so it goes out without the host. The NSIS
hooks register nothing when the host is absent, and the app hides its
toggle. Any one list is enough to ship; the installer then registers only
the browsers whose list has an id (the host's --registers).

A Chromium entry that is the development id (its key is public, so anyone
can build an extension that carries it) or not exactly
chrome-extension://<32 letters a-p>/ (a wildcard, say) throws. So does a
Firefox entry that is not an add-on id as MDN defines it (name@domain of at
most 80 characters, or a GUID in braces). Otherwise "ship".
#>
function Get-BrowserHostPlan {
    param(
        [Parameter(Mandatory)][string]$OriginsPath,
        [Parameter(Mandatory)][string]$DevPath
    )
    $origins = Get-Content $OriginsPath -Raw | ConvertFrom-Json
    $chromiumIds = @(@($origins.chrome_web_store) + @($origins.edge_add_ons) | Where-Object { $null -ne $_ })
    $firefoxIds = @(@($origins.firefox_add_ons) | Where-Object { $null -ne $_ })
    $devIds = @((Get-Content $DevPath -Raw | ConvertFrom-Json).allowed_origins)
    if ($chromiumIds.Count -eq 0 -and $firefoxIds.Count -eq 0) {
        return "leave-out"
    }
    foreach ($id in $chromiumIds) {
        if ($devIds -contains $id) { throw "allowed-origins.json holds the development id $id." }
        if ($id -cnotmatch '^chrome-extension://[a-p]{32}/$') {
            throw "allowed-origins.json has a malformed entry '$id'."
        }
    }
    foreach ($id in $firefoxIds) {
        $email = $id.Length -le 80 -and $id -cmatch '^[a-zA-Z0-9._-]*@[a-zA-Z0-9._-]+$'
        $guid = $id -cmatch '^\{[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\}$'
        if (-not ($email -or $guid)) {
            throw "allowed-origins.json has a malformed Firefox id '$id'."
        }
    }
    return "ship"
}
