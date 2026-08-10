# Everything CI runs, plus the suites that need a real bucket, share or SFTP
# account, against containers on this machine.
#
# Those suites skip themselves when no endpoint is configured, which is the
# right default (nobody should need Docker to run `cargo test`) and also
# means they never ran here: on Windows they were exercised only by CI,
# after a push. This script closes that gap before one.
#
#   .\scripts\test-local.ps1          # start backends, run everything
#   .\scripts\test-local.ps1 -Stop    # tear the containers down again
[CmdletBinding()]
param(
    [switch]$Stop,
    # Skips the frontend half when only the Rust side is being iterated on.
    [switch]$RustOnly
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$compose = Join-Path $root 'docker-compose.test.yml'

if ($Stop) {
    docker compose -f $compose down
    return
}

docker compose -f $compose up -d

Write-Host 'Waiting for MinIO...'
$ready = $false
foreach ($attempt in 1..30) {
    try {
        Invoke-WebRequest -UseBasicParsing -TimeoutSec 2 `
            -Uri 'http://127.0.0.1:9000/minio/health/live' | Out-Null
        $ready = $true
        break
    } catch {
        Start-Sleep -Seconds 2
    }
}
if (-not $ready) { throw 'MinIO did not come up; check `docker compose logs minio`.' }

# The bucket the S3 suites expect. Created through mc rather than the SDK so
# a credentials problem shows up here rather than as a puzzling test failure.
docker run --rm --network host `
    -e MC_HOST_local=http://silentsilo:silentsilo123@127.0.0.1:9000 `
    minio/mc mb --ignore-existing local/vault-test | Out-Null

$env:SILENTSILO_TEST_S3_ENDPOINT = 'http://127.0.0.1:9000'
$env:SILENTSILO_TEST_S3_KEY = 'silentsilo'
$env:SILENTSILO_TEST_S3_SECRET = 'silentsilo123'
$env:SILENTSILO_TEST_S3_BUCKET = 'vault-test'
$env:SILENTSILO_TEST_WEBDAV_URL = 'http://127.0.0.1:8088'
$env:SILENTSILO_TEST_SFTP_HOST = '127.0.0.1'
$env:SILENTSILO_TEST_SFTP_PORT = '2222'
# Running this script means asking for those suites, so a suite that decides
# to skip itself anyway (a typo above, a container that died mid-run) has to
# fail instead of printing a line nobody reads.
$env:SILENTSILO_TEST_REQUIRE_BACKENDS = '1'

function Invoke-Step {
    param([string]$Name, [scriptblock]$Body)
    Write-Host "`n=== $Name ===" -ForegroundColor Cyan
    & $Body
    if ($LASTEXITCODE -ne 0) { throw "$Name failed" }
}

if (-not $RustOnly) {
    Invoke-Step 'typecheck' { npm run typecheck }
    Invoke-Step 'lint' { npm run lint }
    Invoke-Step 'frontend tests' { npm test }
    Invoke-Step 'frontend build' { npm run build }
}

Invoke-Step 'fmt' { cargo fmt --all -- --check }
Invoke-Step 'clippy' { cargo clippy --all-targets -- -D warnings }
# With the endpoints set, this run includes the suites that would otherwise
# skip: the S3 client, the storage contract on every backend, and the sync
# tests that need a real bucket.
Invoke-Step 'cargo test (with real backends)' { cargo test --all }
Invoke-Step 'cargo check' { cargo check --all }

Write-Host "`nAll green. The containers are still up; ./scripts/test-local.ps1 -Stop clears them." -ForegroundColor Green
