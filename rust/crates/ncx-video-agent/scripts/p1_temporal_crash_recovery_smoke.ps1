param(
    [string]$RustWorkspace = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path,
    [string]$WorkflowId = "video-agent-p1-probe-$([guid]::NewGuid().ToString('N'))",
    [string]$TaskQueue = "video-agent-p1-probe",
    [string]$ShotId = "shot_01",
    [string]$Protoc = $env:PROTOC,
    [string]$LogDir = (Join-Path $env:TEMP "ncx-video-agent-temporal-smoke"),
    [int]$WorkerWarmupSeconds = 4
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

if (-not (Test-TcpPort -HostName "127.0.0.1" -Port 7233)) {
    throw "Temporal dev server is not reachable at 127.0.0.1:7233. Start it with: temporal server start-dev"
}

New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
$resolvedProtoc = Resolve-Protoc -Requested $Protoc

$oldTaskQueue = $env:P1_TEMPORAL_TASK_QUEUE
$oldWorkflowId = $env:P1_TEMPORAL_WORKFLOW_ID
$oldShotId = $env:P1_TEMPORAL_SHOT_ID
$oldProtoc = $env:PROTOC
$worker1 = $null
$worker2 = $null
$pushedLocation = $false

try {
    $env:P1_TEMPORAL_TASK_QUEUE = $TaskQueue
    $env:P1_TEMPORAL_WORKFLOW_ID = $WorkflowId
    $env:P1_TEMPORAL_SHOT_ID = $ShotId
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
        throw "first worker exited before workflow start. Logs: $worker1Out $worker1Err"
    }

    Invoke-ProbeCommand -ProbeExe $probeExe -Mode "start" -LogPath (Join-Path $LogDir "$WorkflowId-start.log") | Out-Null

    Stop-StartedProcess -Process $worker1

    $worker2 = Start-Process -FilePath $probeExe -ArgumentList @("worker") -WorkingDirectory $RustWorkspace -PassThru -WindowStyle Hidden -RedirectStandardOutput $worker2Out -RedirectStandardError $worker2Err
    Start-Sleep -Seconds $WorkerWarmupSeconds
    if ($worker2.HasExited) {
        throw "second worker exited before approval signal. Logs: $worker2Out $worker2Err"
    }

    Invoke-ProbeCommand -ProbeExe $probeExe -Mode "signal" -LogPath (Join-Path $LogDir "$WorkflowId-signal.log") | Out-Null
    $resultOutput = Invoke-ProbeCommand -ProbeExe $probeExe -Mode "result" -LogPath (Join-Path $LogDir "$WorkflowId-result.log")

    $expected = "${ShotId}:dry-temporal-job-${ShotId}:approved"
    if ($resultOutput -notmatch [regex]::Escape($expected)) {
        throw "Temporal crash-recovery smoke returned unexpected result. Expected substring: $expected. Output: $resultOutput"
    }

    Write-Host "PASS P1 Temporal crash-recovery smoke"
    Write-Host "workflow_id: $WorkflowId"
    Write-Host "task_queue: $TaskQueue"
    Write-Host "logs: $LogDir"
}
finally {
    Stop-StartedProcess -Process $worker1
    Stop-StartedProcess -Process $worker2
    if ($pushedLocation) {
        Pop-Location
    }
    $env:P1_TEMPORAL_TASK_QUEUE = $oldTaskQueue
    $env:P1_TEMPORAL_WORKFLOW_ID = $oldWorkflowId
    $env:P1_TEMPORAL_SHOT_ID = $oldShotId
    $env:PROTOC = $oldProtoc
}
