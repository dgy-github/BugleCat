param(
    [string]$RustWorkspace = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
)

$ErrorActionPreference = "Stop"

function Fail {
    param([string]$Message)
    throw "P1 paid preflight self-test failed: $Message"
}

function Invoke-ExpectFailure {
    param(
        [string]$Label,
        [scriptblock]$Block,
        [string]$ExpectedText,
        [string]$ForbiddenText = ""
    )

    $text = ""
    $accepted = $false
    $global:LASTEXITCODE = 0
    $oldEap = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        $output = & $Block 2>&1
        $text = ($output | Out-String)
        $accepted = ($LASTEXITCODE -eq 0 -and $?)
    }
    catch {
        $text = ($_ | Out-String)
    }
    finally {
        $ErrorActionPreference = $oldEap
    }

    if ($accepted) {
        Fail "$Label unexpectedly succeeded"
    }
    if (-not $text.Contains($ExpectedText)) {
        Fail "$Label failed for an unexpected reason: $text"
    }
    if ($ForbiddenText -and $text.Contains($ForbiddenText)) {
        Fail "$Label printed forbidden text '$ForbiddenText': $text"
    }
}

function Set-ProcessEnv {
    param(
        [string]$Name,
        [AllowNull()][string]$Value
    )
    [Environment]::SetEnvironmentVariable($Name, $Value, "Process")
}

$tosEnvNames = @(
    "TOS_ACCESS_KEY_ID",
    "TOS_ACCESS_KEY",
    "AWS_ACCESS_KEY_ID",
    "TOS_SECRET_ACCESS_KEY",
    "TOS_SECRET_KEY",
    "AWS_SECRET_ACCESS_KEY",
    "TOS_ENDPOINT",
    "AWS_ENDPOINT_URL",
    "TOS_BUCKET",
    "S3_BUCKET",
    "TOS_REGION",
    "AWS_REGION",
    "AWS_DEFAULT_REGION"
)

$saved = @{}
foreach ($name in $tosEnvNames) {
    $saved[$name] = [Environment]::GetEnvironmentVariable($name, "Process")
}
$oldAllowRealArk = $env:P1_TEMPORAL_ALLOW_REAL_ARK
$pushedLocation = $false
$completed = $false

try {
    foreach ($name in $tosEnvNames) {
        Set-ProcessEnv -Name $name -Value $null
    }

    Push-Location $RustWorkspace
    $pushedLocation = $true

    $directOut = Join-Path $env:TEMP ("ncx-video-agent-paid-preflight-direct-" + [guid]::NewGuid().ToString("N"))
    Invoke-ExpectFailure `
        -Label "direct paid Seedance/TOS smoke without TOS credentials" `
        -ExpectedText "TOS access key missing" `
        -ForbiddenText "submitted Seedance task" `
        -Block {
            cargo run -p ncx-video-agent --bin p1_seedance_tos_smoke -- --submit-real-ark-job $directOut
        }
    foreach ($fileName in @("seedance_tos_roundtrip.mp4", "rough_cut.mp4", "trace.json")) {
        $path = Join-Path $directOut $fileName
        if (Test-Path -LiteralPath $path) {
            Fail "direct paid preflight created forbidden output before TOS was configured: $path"
        }
    }
    if (Test-Path -LiteralPath $directOut) {
        Fail "direct paid preflight created output directory before TOS was configured: $directOut"
    }

    $env:P1_TEMPORAL_ALLOW_REAL_ARK = "1"
    $temporalOut = Join-Path $env:TEMP ("ncx-video-agent-paid-preflight-temporal-" + [guid]::NewGuid().ToString("N"))
    Invoke-ExpectFailure `
        -Label "Temporal paid Seedance/TOS smoke without TOS credentials" `
        -ExpectedText "Missing TOS access key env" `
        -ForbiddenText "Started P1 live Seedance/TOS workflow" `
        -Block {
            & (Join-Path $PSScriptRoot "p1_temporal_live_seedance_recovery_smoke.ps1") -RustWorkspace $RustWorkspace -OutDir $temporalOut
        }
    if (Test-Path -LiteralPath $temporalOut) {
        Fail "Temporal paid preflight created output directory before TOS was configured: $temporalOut"
    }

    $completed = $true
}
finally {
    if ($pushedLocation) {
        Pop-Location
    }
    foreach ($name in $tosEnvNames) {
        Set-ProcessEnv -Name $name -Value $saved[$name]
    }
    $env:P1_TEMPORAL_ALLOW_REAL_ARK = $oldAllowRealArk
}

if ($completed) {
    foreach ($name in $tosEnvNames) {
        $current = [Environment]::GetEnvironmentVariable($name, "Process")
        if ($current -ne $saved[$name]) {
            Fail "environment variable $name was not restored after self-test"
        }
    }
    if ($env:P1_TEMPORAL_ALLOW_REAL_ARK -ne $oldAllowRealArk) {
        Fail "environment variable P1_TEMPORAL_ALLOW_REAL_ARK was not restored after self-test"
    }
    Write-Host "PASS P1 paid preflight self-test"
}
