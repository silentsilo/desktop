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
Both store lists empty: "leave-out". The extension has no store listing yet,
and a desktop release must not wait for one, so it goes out without the host.
The NSIS hooks register nothing when the host is absent, and the app hides
its toggle.

Any entry that is the development id (its key is public, so anyone can build
an extension that carries it) or not exactly chrome-extension://<32 letters
a-p>/ (a wildcard, say) throws. Otherwise "ship".
#>
function Get-BrowserHostPlan {
    param(
        [Parameter(Mandatory)][string]$OriginsPath,
        [Parameter(Mandatory)][string]$DevPath
    )
    $origins = Get-Content $OriginsPath -Raw | ConvertFrom-Json
    $storeIds = @(@($origins.chrome_web_store) + @($origins.edge_add_ons) | Where-Object { $null -ne $_ })
    $devIds = @((Get-Content $DevPath -Raw | ConvertFrom-Json).allowed_origins)
    if ($storeIds.Count -eq 0) {
        return "leave-out"
    }
    foreach ($id in $storeIds) {
        if ($devIds -contains $id) { throw "allowed-origins.json holds the development id $id." }
        if ($id -cnotmatch '^chrome-extension://[a-p]{32}/$') {
            throw "allowed-origins.json has a malformed entry '$id'."
        }
    }
    return "ship"
}
