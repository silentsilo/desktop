# Whether an exe this release ships needs the Visual C++ runtime. Separated
# from build-release-local.ps1 so the check can be run without the signing
# token (vc-runtime.test.ps1).
#
# Dot-source it. It defines a function and does nothing else.

<#
.SYNOPSIS
Throws when any of the files names a vcruntime or msvcp DLL.

.DESCRIPTION
The Visual C++ runtime is not part of Windows, and the browser host and the
extractor once needed it while the app, linked statically by tauri-build,
did not: fine on this machine, broken on one that never installed the
redistributable. Read from the import table's names, which sit in the file
as plain ASCII.
#>
function Assert-NoVcRuntime {
    param([string[]]$Paths)
    foreach ($path in $Paths) {
        $text = [System.Text.Encoding]::ASCII.GetString([System.IO.File]::ReadAllBytes($path))
        $found = [regex]::Matches($text, '(?i)\b(vcruntime|msvcp)\d+[a-z_0-9]*\.dll') |
            ForEach-Object { $_.Value.ToLower() } | Sort-Object -Unique
        if ($found) {
            throw "$path needs $($found -join ', '), which a clean Windows does not have. Link the C runtime statically (see crates/silentsilo-browser-host/build.rs)."
        }
    }
}
