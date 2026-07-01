param(
    [string]$RustWorkspace = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path,
    [string]$WorkflowId = "video-agent-p1-live-$([guid]::NewGuid().ToString('N'))",
    [string]$TaskQueue = "video-agent-p1-probe",
    [string]$OutDir = (Join-Path $env:TEMP "ncx-video-agent-temporal-live-$([guid]::NewGuid().ToString('N'))"),
    [string]$Protoc = $env:PROTOC,
    [string]$LogDir = (Join-Path $env:TEMP "ncx-video-agent-temporal-live-smoke"),
    [int]$WorkerWarmupSeconds = 4,
    [int]$PollMarkerTimeoutSeconds = 900
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

    if ($Requested -and (Test-Path -LiteralPath $Requested)) {
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

function Invoke-ProbeCommand {
    param(
        [string]$ProbeExe,
        [string]$Mode,
        [string]$LogPath
    )

    $oldEap = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    $output = & $ProbeExe $Mode 2>&1
    $exitCode = $LASTEXITCODE
    $ErrorActionPreference = $oldEap
    $output | Set-Content -LiteralPath $LogPath
    if ($exitCode -ne 0) {
        throw "p1_temporal_probe $Mode failed with exit code $exitCode. Log: $LogPath"
    }
    return ($output -join "`n")
}

function Stop-StartedProcess {
    param([System.Diagnostics.Process]$Process)

    if ($Process -and -not $Process.HasExited) {
        Stop-Process -Id $Process.Id -Force
        $Process.WaitForExit()
    }
}

function Test-AnyEnv {
    param([string[]]$Names)

    foreach ($name in $Names) {
        $value = [Environment]::GetEnvironmentVariable($name)
        if ($value -and $value.Trim()) {
            return $true
        }
    }
    return $false
}

$allow = $env:P1_TEMPORAL_ALLOW_REAL_ARK
if (-not ($allow -and ($allow.Trim().ToLowerInvariant() -in @("1", "true", "yes")))) {
    throw "This smoke submits a paid real Seedance job. Set P1_TEMPORAL_ALLOW_REAL_ARK=1 to continue."
}

if (-not (Test-AnyEnv -Names @("TOS_ACCESS_KEY_ID", "TOS_ACCESS_KEY", "AWS_ACCESS_KEY_ID"))) {
    throw "Missing TOS access key env: TOS_ACCESS_KEY_ID, TOS_ACCESS_KEY, or AWS_ACCESS_KEY_ID"
}
if (-not (Test-AnyEnv -Names @("TOS_SECRET_ACCESS_KEY", "TOS_SECRET_KEY", "AWS_SECRET_ACCESS_KEY"))) {
    throw "Missing TOS secret key env: TOS_SECRET_ACCESS_KEY, TOS_SECRET_KEY, or AWS_SECRET_ACCESS_KEY"
}
if (-not (Test-AnyEnv -Names @("TOS_ENDPOINT", "AWS_ENDPOINT_URL"))) {
    throw "Missing TOS endpoint env: TOS_ENDPOINT or AWS_ENDPOINT_URL"
}
if (-not (Test-AnyEnv -Names @("TOS_BUCKET", "S3_BUCKET"))) {
    throw "Missing TOS bucket env: TOS_BUCKET or S3_BUCKET"
}

if (-not (Test-TcpPort -HostName "127.0.0.1" -Port 7233)) {
    throw "Temporal dev server is not reachable at 127.0.0.1:7233. Start it with: temporal server start-dev"
}

New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$resolvedProtoc = Resolve-Protoc -Requested $Protoc

$oldTaskQueue = $env:P1_TEMPORAL_TASK_QUEUE
$oldLiveWorkflowId = $env:P1_TEMPORAL_LIVE_WORKFLOW_ID
$oldLiveOutDir = $env:P1_TEMPORAL_LIVE_OUT_DIR
$oldAllowRealArk = $env:P1_TEMPORAL_ALLOW_REAL_ARK
$oldProtoc = $env:PROTOC
$worker1 = $null
$worker2 = $null
$pushedLocation = $false

try {
    $env:P1_TEMPORAL_TASK_QUEUE = $TaskQueue
    $env:P1_TEMPORAL_LIVE_WORKFLOW_ID = $WorkflowId
    $env:P1_TEMPORAL_LIVE_OUT_DIR = $OutDir
    $env:P1_TEMPORAL_ALLOW_REAL_ARK = "1"
    if ($resolvedProtoc) {
        $env:PROTOC = $resolvedProtoc
    }

    Push-Location $RustWorkspace
    $pushedLocation = $true

    & cargo build -p ncx-video-agent --features temporal --bin p1_temporal_probe
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build for p1_temporal_probe failed with exit code $LASTEXITCODE"
    }
    $probeExe = Join-Path $RustWorkspace "target\debug\p1_temporal_probe.exe"
    if (-not (Test-Path -LiteralPath $probeExe)) {
        throw "built probe executable not found: $probeExe"
    }

    $worker1Out = Join-Path $LogDir "$WorkflowId-worker-1.out.log"
    $worker1Err = Join-Path $LogDir "$WorkflowId-worker-1.err.log"
    $worker2Out = Join-Path $LogDir "$WorkflowId-worker-2.out.log"
    $worker2Err = Join-Path $LogDir "$WorkflowId-worker-2.err.log"

    $worker1 = Start-Process -FilePath $probeExe -ArgumentList @("worker") -WorkingDirectory $RustWorkspace -PassThru -WindowStyle Hidden -RedirectStandardOutput $worker1Out -RedirectStandardError $worker1Err
    Start-Sleep -Seconds $WorkerWarmupSeconds
    if ($worker1.HasExited) {
        throw "first worker exited before live workflow start. Logs: $worker1Out $worker1Err"
    }

    Invoke-ProbeCommand -ProbeExe $probeExe -Mode "live-start" -LogPath (Join-Path $LogDir "$WorkflowId-start.log") | Out-Null

    $pollMarker = Join-Path $OutDir "temporal_live_poll_marker.json"
    $deadline = (Get-Date).AddSeconds($PollMarkerTimeoutSeconds)
    while (-not (Test-Path -LiteralPath $pollMarker)) {
        if ($worker1.HasExited) {
            throw "first worker exited before live poll marker was written. Logs: $worker1Out $worker1Err"
        }
        if ((Get-Date) -ge $deadline) {
            throw "live poll marker was not written within $PollMarkerTimeoutSeconds seconds: $pollMarker"
        }
        Start-Sleep -Seconds 2
    }

    $pollState = Get-Content -Raw -LiteralPath $pollMarker | ConvertFrom-Json
    if ($pollState.kind -ne "running") {
        throw "Expected live poll marker kind=running before killing worker, got kind=$($pollState.kind). Marker: $pollMarker"
    }

    Stop-StartedProcess -Process $worker1

    $worker2 = Start-Process -FilePath $probeExe -ArgumentList @("worker") -WorkingDirectory $RustWorkspace -PassThru -WindowStyle Hidden -RedirectStandardOutput $worker2Out -RedirectStandardError $worker2Err
    Start-Sleep -Seconds $WorkerWarmupSeconds
    if ($worker2.HasExited) {
        throw "second worker exited before live result. Logs: $worker2Out $worker2Err"
    }

    $resultOutput = Invoke-ProbeCommand -ProbeExe $probeExe -Mode "live-result" -LogPath (Join-Path $LogDir "$WorkflowId-result.log")
    if ($resultOutput -notmatch "rough_cut=" -or $resultOutput -notmatch "trace=" -or $resultOutput -notmatch "shot_trace=" -or $resultOutput -notmatch "rough_cut_tos=") {
        throw "Temporal live workflow returned unexpected result: $resultOutput"
    }

    $roughCut = Join-Path $OutDir "rough_cut.mp4"
    $trace = Join-Path $OutDir "trace.json"
    $shotTrace = Join-Path $OutDir "trace_shot_01.json"
    if (-not (Test-Path -LiteralPath $roughCut)) {
        throw "rough_cut.mp4 was not produced: $roughCut"
    }
    if (-not (Test-Path -LiteralPath $trace)) {
        throw "trace.json was not produced: $trace"
    }
    if (-not (Test-Path -LiteralPath $shotTrace)) {
        throw "trace_shot_01.json was not produced: $shotTrace"
    }

    $verifyScript = Join-Path $PSScriptRoot "p1_verify_rough_cut_trace.ps1"
    & $verifyScript -OutDir $OutDir
    if ($LASTEXITCODE -ne 0) {
        throw "P1 rough-cut trace verification failed for live output: $OutDir"
    }

    Write-Host "PASS P1 Temporal live Seedance/TOS recovery smoke"
    Write-Host "workflow_id: $WorkflowId"
    Write-Host "task_queue: $TaskQueue"
    Write-Host "out_dir: $OutDir"
    Write-Host "rough_cut: $roughCut"
    Write-Host "trace: $trace"
    Write-Host "shot_trace: $shotTrace"
    Write-Host "poll_marker: $pollMarker"
    Write-Host "logs: $LogDir"
}
finally {
    Stop-StartedProcess -Process $worker1
    Stop-StartedProcess -Process $worker2
    if ($pushedLocation) {
        Pop-Location
    }
    $env:P1_TEMPORAL_TASK_QUEUE = $oldTaskQueue
    $env:P1_TEMPORAL_LIVE_WORKFLOW_ID = $oldLiveWorkflowId
    $env:P1_TEMPORAL_LIVE_OUT_DIR = $oldLiveOutDir
    $env:P1_TEMPORAL_ALLOW_REAL_ARK = $oldAllowRealArk
    $env:PROTOC = $oldProtoc
}
