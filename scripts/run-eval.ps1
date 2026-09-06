param(
    [string]$Tasks = "",
    [int]$Repeats = 1,
    [int]$Timeout = 240,
    [switch]$SkipBuild,
    [switch]$OpenReport
)

$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$binary = Join-Path $repoRoot "rust\target\release\ncx.exe"
$reportDir = Join-Path $repoRoot "evals\reports"
$report = Join-Path $reportDir ("baseline_{0:yyyyMMdd_HHmmss}.json" -f (Get-Date))

function Step([string]$Name, [scriptblock]$Action) {
    Write-Host ""
    Write-Host "==> $Name" -ForegroundColor Cyan
    & $Action
}

if (-not $SkipBuild -or -not (Test-Path -LiteralPath $binary)) {
    Step "Build Rust Agent" {
        & cargo build --manifest-path (Join-Path $repoRoot "rust\Cargo.toml") --workspace --release
    }
}
if (-not (Test-Path -LiteralPath $binary)) {
    throw "Rust Agent binary not found: $binary"
}

New-Item -ItemType Directory -Force -Path $reportDir | Out-Null
Step "Replay benchmark tasks" {
    $args = @("-m", "evals.harness", "benchmark", "--agent", "nanocodex",
              "--repeats", "$Repeats", "--timeout", "$Timeout", "--output", $report)
    if (-not [string]::IsNullOrWhiteSpace($Tasks)) {
        $args += @("--tasks", $Tasks)
    }
    Push-Location $repoRoot
    try { & python @args } finally { Pop-Location }
}

Step "Generate compaction fixtures" {
    Push-Location $repoRoot
    try {
        & python -m evals.harness generate --count 50 --mutate --output (Join-Path $repoRoot "evals\generated")
    } finally { Pop-Location }
}

Write-Host ""
Write-Host "Report: $report" -ForegroundColor Green
if ($OpenReport) {
    Invoke-Item -LiteralPath $report
}
