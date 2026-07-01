param(
    [string]$RustWorkspace = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path,
    [string]$Protoc = $env:PROTOC,
    [string]$FastTextModel = $env:FASTTEXT_LID_MODEL,
    [string]$TemporalExe = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path "target\tools\temporal-cli\extract\temporal.exe"),
    [string]$LogDir = (Join-Path $env:TEMP "ncx-video-agent-p1-acceptance"),
    [string]$DryRunOutDir = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path "target\p1_dry_run_out_current"),
    [string]$LiveOutDir = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path "target\p1_seedance_tos_smoke_out"),
    [string]$TemporalLiveOutDir = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path "target\p1_temporal_live_seedance_tos_out"),
    [switch]$StartTemporalIfNeeded,
    [switch]$LocalOnly,
    [switch]$RunPaidLive
)

$ErrorActionPreference = "Stop"

function Test-TcpPort {
    param(
        [string]$HostName,
        [int]$Port
    )

    $client = [System.Net.Sockets.TcpClient]::new()
    try {
        $connect = $client.ConnectAsync($HostName, $Port)
        if (-not $connect.Wait(2000)) {
            return $false
        }
        return $client.Connected
    }
    finally {
        $client.Dispose()
    }
}

function Resolve-Protoc {
    param([string]$Requested)

    if ($Requested -and (Test-Path -LiteralPath $Requested -PathType Leaf)) {
        return (Resolve-Path -LiteralPath $Requested).Path
    }

    $registry = Join-Path $env:USERPROFILE ".cargo\registry\src"
    if (-not (Test-Path -LiteralPath $registry)) {
        return $null
    }

    $candidate = Get-ChildItem -LiteralPath $registry -Recurse -Filter protoc.exe -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match "protoc-bin-vendored-win32" } |
        Select-Object -First 1
    if ($candidate) {
        return $candidate.FullName
    }
    return $null
}

function Resolve-FastTextModel {
    param([string]$Requested)

    if ($Requested -and (Test-Path -LiteralPath $Requested -PathType Leaf)) {
        return (Resolve-Path -LiteralPath $Requested).Path
    }

    foreach ($envName in @("LID_176_BIN", "LID_176_FTZ")) {
        $value = [Environment]::GetEnvironmentVariable($envName)
        if ($value -and (Test-Path -LiteralPath $value -PathType Leaf)) {
            return (Resolve-Path -LiteralPath $value).Path
        }
    }

    $defaultFtz = Join-Path $RustWorkspace "target\tools\fasttext\lid.176.ftz"
    if (Test-Path -LiteralPath $defaultFtz -PathType Leaf) {
        return (Resolve-Path -LiteralPath $defaultFtz).Path
    }

    $defaultBin = Join-Path $RustWorkspace "target\tools\fasttext\lid.176.bin"
    if (Test-Path -LiteralPath $defaultBin -PathType Leaf) {
        return (Resolve-Path -LiteralPath $defaultBin).Path
    }

    return $null
}

function Invoke-Checked {
    param(
        [string]$Label,
        [scriptblock]$Block
    )

    Write-Host "==> $Label"
    $global:LASTEXITCODE = 0
    & $Block
    if (-not $?) {
        throw "$Label failed"
    }
    if ($LASTEXITCODE -ne 0) {
        throw "$Label failed with exit code $LASTEXITCODE"
    }
}

function Stop-StartedProcess {
    param([System.Diagnostics.Process]$Process)

    if ($Process -and -not $Process.HasExited) {
        Stop-Process -Id $Process.Id -Force
        $Process.WaitForExit()
    }
}

function Start-TemporalIfRequired {
    if (Test-TcpPort -HostName "127.0.0.1" -Port 7233) {
        Write-Host "Temporal dev server already reachable at 127.0.0.1:7233"
        return $null
    }

    if (-not $StartTemporalIfNeeded) {
        throw "Temporal dev server is not reachable at 127.0.0.1:7233. Start it or pass -StartTemporalIfNeeded."
    }
    if (-not (Test-Path -LiteralPath $TemporalExe -PathType Leaf)) {
        throw "Temporal CLI not found: $TemporalExe"
    }

    New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
    $serverOut = Join-Path $LogDir "temporal-server.out.log"
    $serverErr = Join-Path $LogDir "temporal-server.err.log"
    $server = Start-Process -FilePath $TemporalExe -ArgumentList @("server", "start-dev", "--ip", "127.0.0.1") -WorkingDirectory $RustWorkspace -PassThru -WindowStyle Hidden -RedirectStandardOutput $serverOut -RedirectStandardError $serverErr
    $deadline = (Get-Date).AddSeconds(60)
    while (-not (Test-TcpPort -HostName "127.0.0.1" -Port 7233)) {
        if ($server.HasExited) {
            throw "Temporal dev server exited early. Logs: $serverOut $serverErr"
        }
        if ((Get-Date) -ge $deadline) {
            throw "Temporal dev server did not open 127.0.0.1:7233 within 60s. Logs: $serverOut $serverErr"
        }
        Start-Sleep -Milliseconds 500
    }
    Write-Host "Started temporary Temporal dev server. logs: $LogDir"
    return $server
}

$resolvedProtoc = Resolve-Protoc -Requested $Protoc
$resolvedFastText = Resolve-FastTextModel -Requested $FastTextModel
$server = $null
$pushedLocation = $false
$oldProtoc = $env:PROTOC
$oldFastText = $env:FASTTEXT_LID_MODEL
$oldLidFtz = $env:LID_176_FTZ
$oldAllowRealArk = $env:P1_TEMPORAL_ALLOW_REAL_ARK

try {
    if (-not $resolvedProtoc) {
        throw "Could not find protoc.exe. Set PROTOC or build once to fetch protoc-bin-vendored."
    }
    if (-not $resolvedFastText) {
        throw "Could not find fastText lid.176 model. Run p1_fetch_fasttext_lid.ps1 first."
    }
    $env:PROTOC = $resolvedProtoc
    $env:FASTTEXT_LID_MODEL = $resolvedFastText
    $env:LID_176_FTZ = $resolvedFastText

    Push-Location $RustWorkspace
    $pushedLocation = $true

    Invoke-Checked "Rust formatting check" {
        cargo fmt -p ncx-video-agent -- --check
    }
    Invoke-Checked "P1 unit tests" {
        cargo test -p ncx-video-agent --quiet
    }
    Invoke-Checked "P1 binary check" {
        cargo check -p ncx-video-agent --bins
    }
    Invoke-Checked "P1 Temporal binary check" {
        cargo check -p ncx-video-agent --features temporal --bin p1_temporal_probe
    }
    Invoke-Checked "P1 deterministic dry-run" {
        cargo run -p ncx-video-agent --bin p1_dry_run --quiet -- $DryRunOutDir
    }
    Invoke-Checked "P1 dry-run rough-cut trace verifier" {
        & (Join-Path $PSScriptRoot "p1_verify_rough_cut_trace.ps1") -OutDir $DryRunOutDir -AllowLocalDryRun
    }
    Invoke-Checked "P1 trace verifier negative self-test" {
        & (Join-Path $PSScriptRoot "p1_verify_rough_cut_trace_selftest.ps1") -SourceOutDir $DryRunOutDir
    }
    Invoke-Checked "P1 paid preflight safety self-test" {
        & (Join-Path $PSScriptRoot "p1_paid_preflight_selftest.ps1") -RustWorkspace $RustWorkspace
    }

    $server = Start-TemporalIfRequired

    Invoke-Checked "P1 Temporal crash-recovery smoke" {
        & (Join-Path $PSScriptRoot "p1_temporal_crash_recovery_smoke.ps1") -RustWorkspace $RustWorkspace -Protoc $resolvedProtoc
    }
    Invoke-Checked "P1 Temporal dry-run recovery smoke" {
        & (Join-Path $PSScriptRoot "p1_temporal_dry_run_recovery_smoke.ps1") -RustWorkspace $RustWorkspace -Protoc $resolvedProtoc
    }

    if ($LocalOnly) {
        Write-Host "PASS P1 local acceptance gates"
        Write-Host "note: -LocalOnly skips live TOS readiness and paid Seedance/TOS proof; full P1 is not complete from this mode alone."
        return
    }

    Invoke-Checked "P1 live readiness preflight" {
        & (Join-Path $PSScriptRoot "p1_live_readiness.ps1") -RustWorkspace $RustWorkspace -Protoc $resolvedProtoc -FastTextModel $resolvedFastText
    }

    if (-not $RunPaidLive) {
        Write-Host "PASS P1 non-paid acceptance gates and live readiness"
        Write-Host "note: paid Seedance/TOS proof was skipped; rerun with -RunPaidLive for full P1 completion evidence."
        return
    }

    $env:P1_TEMPORAL_ALLOW_REAL_ARK = "1"
    Invoke-Checked "P1 paid Seedance/TOS smoke" {
        cargo run -p ncx-video-agent --bin p1_seedance_tos_smoke -- --submit-real-ark-job $LiveOutDir
    }
    Invoke-Checked "P1 paid Seedance/TOS output verifier" {
        & (Join-Path $PSScriptRoot "p1_verify_rough_cut_trace.ps1") -OutDir $LiveOutDir
    }
    Invoke-Checked "P1 paid Temporal live Seedance/TOS recovery smoke" {
        & (Join-Path $PSScriptRoot "p1_temporal_live_seedance_recovery_smoke.ps1") -RustWorkspace $RustWorkspace -Protoc $resolvedProtoc -OutDir $TemporalLiveOutDir
    }

    Write-Host "PASS P1 full paid acceptance"
    Write-Host "seedance_tos_out: $LiveOutDir"
    Write-Host "temporal_live_out: $TemporalLiveOutDir"
}
finally {
    if ($pushedLocation) {
        Pop-Location
    }
    Stop-StartedProcess -Process $server
    $env:PROTOC = $oldProtoc
    $env:FASTTEXT_LID_MODEL = $oldFastText
    $env:LID_176_FTZ = $oldLidFtz
    $env:P1_TEMPORAL_ALLOW_REAL_ARK = $oldAllowRealArk
}
