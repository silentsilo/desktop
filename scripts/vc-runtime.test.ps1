# Exercises Assert-NoVcRuntime against stand-in files, without the signing
# token or a build.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\vc-runtime.test.ps1

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "vc-runtime.ps1")

$dir = Join-Path $env:TEMP "silentsilo-vc-runtime-test"

$failures = 0
function Check($name, $condition) {
    if ($condition) { Write-Host "  PASS  $name" -ForegroundColor Green }
    else { Write-Host "  FAIL  $name" -ForegroundColor Red; $script:failures++ }
}

function Throws([scriptblock]$Body) {
    try { & $Body | Out-Null; $false } catch { $true }
}

# A file whose bytes hold these import names among binary noise.
function Exe([string]$Name, [string[]]$Imports) {
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir | Out-Null }
    $path = Join-Path $dir $Name
    $bytes = [System.Collections.Generic.List[byte]]::new()
    $bytes.AddRange([byte[]](0x4D, 0x5A, 0x90, 0x00))
    foreach ($import in $Imports) {
        $bytes.AddRange([System.Text.Encoding]::ASCII.GetBytes($import))
        $bytes.AddRange([byte[]](0x00, 0xFF))
    }
    [System.IO.File]::WriteAllBytes($path, $bytes.ToArray())
    $path
}

Write-Host "`n== Imports =="
$clean = Exe "clean.exe" @("KERNEL32.dll", "api-ms-win-crt-runtime-l1-1-0.dll", "ntdll.dll")
Check "a clean exe passes" (-not (Throws { Assert-NoVcRuntime @($clean) }))
Check "VCRUNTIME140.dll: refused" (Throws { Assert-NoVcRuntime @((Exe "vc.exe" @("KERNEL32.dll", "VCRUNTIME140.dll"))) })
Check "vcruntime140_1.dll: refused" (Throws { Assert-NoVcRuntime @((Exe "vc1.exe" @("vcruntime140_1.dll"))) })
Check "MSVCP140.dll: refused" (Throws { Assert-NoVcRuntime @((Exe "cpp.exe" @("MSVCP140.dll"))) })
Check "one bad file among clean ones: refused" (Throws { Assert-NoVcRuntime @($clean, (Exe "late.exe" @("VCRUNTIME140.dll"))) })
Check "no files: passes" (-not (Throws { Assert-NoVcRuntime @() }))
Check "a missing file: refused" (Throws { Assert-NoVcRuntime @((Join-Path $dir "absent.exe")) })
$message = try { Assert-NoVcRuntime @((Exe "named.exe" @("VCRUNTIME140.dll"))); "" } catch { $_.Exception.Message }
Check "the refusal names the DLL" ($message -match "vcruntime140\.dll")

if (Test-Path $dir) { Remove-Item $dir -Recurse -Force }
Write-Host ""
if ($failures -gt 0) { Write-Host "$failures failed" -ForegroundColor Red; exit 1 }
Write-Host "all checks passed" -ForegroundColor Green
