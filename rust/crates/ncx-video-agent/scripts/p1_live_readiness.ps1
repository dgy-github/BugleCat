param(
    [string]$RustWorkspace = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path,
    [string]$Protoc = $env:PROTOC,
    [string]$FastTextModel = $env:FASTTEXT_LID_MODEL,
    [switch]$StartTemporalIfNeeded,
    [string]$TemporalExe = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path "target\tools\temporal-cli\extract\temporal.exe"),
    [string]$LogDir = (Join-Path $env:TEMP "ncx-video-agent-live-readiness")
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

function Stop-StartedProcess {
    param([System.Diagnostics.Process]$Process)

    if ($Process -and -not $Process.HasExited) {
        Stop-Process -Id $Process.Id -Force
        $Process.WaitForExit()
    }
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

$resolvedProtoc = Resolve-Protoc -Requested $Protoc
$resolvedFastText = Resolve-FastTextModel -Requested $FastTextModel
$server = $null
$pushedLocation = $false
$oldProtoc = $env:PROTOC
$oldFastText = $env:FASTTEXT_LID_MODEL
$oldLidFtz = $env:LID_176_FTZ

try {
    if ($resolvedProtoc) {
        $env:PROTOC = $resolvedProtoc
    }
    else {
        throw "Could not find protoc.exe. Set PROTOC or build once to fetch protoc-bin-vendored."
    }

    if ($resolvedFastText) {
        $env:FASTTEXT_LID_MODEL = $resolvedFastText
        $env:LID_176_FTZ = $resolvedFastText
    }
    else {
        throw "Could not find fastText lid.176 model. Run p1_fetch_fasttext_lid.ps1 first."
    }

    if (-not (Test-TcpPort -HostName "127.0.0.1" -Port 7233)) {
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
        Write-Host "Started temporary Temporal dev server for readiness check. logs: $LogDir"
    }

    Push-Location $RustWorkspace
    $pushedLocation = $true

    Invoke-Checked "Temporal probe compile check" {
        cargo check -p ncx-video-agent --features temporal --bin p1_temporal_probe
    }

    Invoke-Checked "P1 non-paid environment smoke" {
        cargo run -p ncx-video-agent --bin p1_smoke --quiet
    }

    Write-Host "PASS P1 live readiness preflight"
    Write-Host "rust_workspace: $RustWorkspace"
    Write-Host "protoc: $resolvedProtoc"
    Write-Host "fasttext_model: $resolvedFastText"
    Write-Host "temporal: reachable at 127.0.0.1:7233"
    Write-Host "note: no Seedance job was submitted; run paid smokes only with explicit opt-in."
}
finally {
    if ($pushedLocation) {
        Pop-Location
    }
    Stop-StartedProcess -Process $server
    $env:PROTOC = $oldProtoc
    $env:FASTTEXT_LID_MODEL = $oldFastText
    $env:LID_176_FTZ = $oldLidFtz
}
