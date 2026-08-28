[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

function Get-Sha256([string]$Path) {
    $algorithm = [System.Security.Cryptography.SHA256]::Create()
    try {
        $stream = [System.IO.File]::OpenRead($Path)
        try {
            $bytes = $algorithm.ComputeHash($stream)
        } finally {
            $stream.Dispose()
        }
    } finally {
        $algorithm.Dispose()
    }
    return -join ($bytes | ForEach-Object { $_.ToString('X2') })
}

$guiRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$workspaceRoot = (Resolve-Path (Join-Path $guiRoot '..\..')).Path
$rustRoot = Join-Path $workspaceRoot 'rust'
$stageRoot = Join-Path $guiRoot 'src-tauri\forge-runtime'
$allowedRoot = (Resolve-Path (Join-Path $guiRoot 'src-tauri')).Path
$stageFull = [System.IO.Path]::GetFullPath($stageRoot)
if (-not $stageFull.StartsWith($allowedRoot + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing to stage outside src-tauri: $stageFull"
}

$pythonVersion = '3.13.7'
$pythonArchive = "python-$pythonVersion-embed-amd64.zip"
$pythonUrl = "https://www.python.org/ftp/python/$pythonVersion/$pythonArchive"
$pythonSha256 = 'F6CCA216A359BE84797CABB54149CE5E062AFB16CC7567EB7FC51CACB2D86B65'
$cacheRoot = Join-Path $rustRoot 'target-codex-check\forge-package-cache'
$archivePath = Join-Path $cacheRoot $pythonArchive
New-Item -ItemType Directory -Force -Path $cacheRoot | Out-Null
if (-not (Test-Path -LiteralPath $archivePath)) {
    Invoke-WebRequest -Uri $pythonUrl -OutFile $archivePath -UseBasicParsing
}
$actualHash = Get-Sha256 $archivePath
if ($actualHash -ne $pythonSha256) {
    throw "Embedded Python archive hash mismatch: $actualHash"
}

if (Test-Path -LiteralPath $stageFull) {
    Remove-Item -LiteralPath $stageFull -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $stageFull | Out-Null
$pythonRoot = Join-Path $stageFull 'python'
Expand-Archive -LiteralPath $archivePath -DestinationPath $pythonRoot

$trainRoot = Join-Path $stageFull 'train'
$benchRoot = Join-Path $stageFull 'bench'
New-Item -ItemType Directory -Force -Path $trainRoot, $benchRoot | Out-Null
$trainFiles = @(
    'forge.py', 'evaluator.py', 'genome.py', 'pareto.py', 'process_control.py',
    'splits.py', 'teacher.py', 'viz.py'
)
foreach ($name in $trainFiles) {
    Copy-Item -LiteralPath (Join-Path $workspaceRoot "train\$name") -Destination $trainRoot
}
Copy-Item -LiteralPath (Join-Path $workspaceRoot 'bench\run.py') -Destination $benchRoot
Copy-Item -LiteralPath (Join-Path $workspaceRoot 'bench\tasks') -Destination $benchRoot -Recurse

$sidecarTarget = Join-Path $rustRoot 'target-codex-check\forge-sidecar'
$env:CARGO_INCREMENTAL = '0'
$env:CARGO_TARGET_DIR = $sidecarTarget
& cargo build -p ncx-cli --release --manifest-path (Join-Path $rustRoot 'Cargo.toml')
if ($LASTEXITCODE -ne 0) { throw "ncx sidecar build failed with exit code $LASTEXITCODE" }
$sidecar = Join-Path $sidecarTarget 'release\ncx.exe'
if (-not (Test-Path -LiteralPath $sidecar)) { throw "ncx sidecar was not produced" }
New-Item -ItemType Directory -Force -Path (Join-Path $stageFull 'bin') | Out-Null
Copy-Item -LiteralPath $sidecar -Destination (Join-Path $stageFull 'bin\ncx.exe')

$pythonExe = Join-Path $pythonRoot 'python.exe'
$forgeScript = Join-Path $trainRoot 'forge.py'
$previousNoBytecode = $env:PYTHONDONTWRITEBYTECODE
$env:PYTHONDONTWRITEBYTECODE = '1'
& $pythonExe $forgeScript --help | Out-Null
$smokeExit = $LASTEXITCODE
if ($null -eq $previousNoBytecode) {
    Remove-Item Env:PYTHONDONTWRITEBYTECODE
} else {
    $env:PYTHONDONTWRITEBYTECODE = $previousNoBytecode
}
if ($smokeExit -ne 0) { throw "embedded Python cannot load Forge" }

$manifest = [ordered]@{
    schema = 'buglecat-forge-runtime/v1'
    pythonVersion = $pythonVersion
    pythonSha256 = Get-Sha256 $pythonExe
    ncxSha256 = Get-Sha256 (Join-Path $stageFull 'bin\ncx.exe')
    forgeSha256 = Get-Sha256 $forgeScript
}
$manifestPath = Join-Path $stageFull 'manifest.json'
$manifestJson = $manifest | ConvertTo-Json
[IO.File]::WriteAllText($manifestPath, $manifestJson, (New-Object Text.UTF8Encoding($false)))
Write-Host "Forge runtime staged at $stageFull"
