param(
    [ValidateSet("x86_64-pc-windows-msvc")]
    [string]$Target = "x86_64-pc-windows-msvc",
    [string]$OutputDir = "releases",
    [string]$Version = "",
    [switch]$SkipTests,
    [switch]$SkipTauri,
    [switch]$NoNpmCi
)

$ErrorActionPreference = "Stop"

function Invoke-Step {
    param(
        [string]$Name,
        [scriptblock]$Block
    )
    Write-Host ""
    Write-Host "==> $Name"
    & $Block
}

function Read-WorkspaceVersion {
    param([string]$RepoRoot)
    $cargoToml = Get-Content -Raw -Path (Join-Path $RepoRoot "rust\Cargo.toml")
    $match = [regex]::Match($cargoToml, '(?ms)^\[workspace\.package\].*?^version\s*=\s*"([^"]+)"')
    if (-not $match.Success) {
        throw "Could not read workspace.package.version from rust\Cargo.toml"
    }
    $match.Groups[1].Value
}

function Add-ArtifactRecord {
    param(
        [System.Collections.ArrayList]$Artifacts,
        [string]$Path,
        [string]$OutputDir
    )
    $item = Get-Item -LiteralPath $Path
    $hash = Get-FileHash -Algorithm SHA256 -LiteralPath $item.FullName
    $relativePath = "$OutputDir/$($item.Name)" -replace "\\", "/"
    [void]$Artifacts.Add([ordered]@{
        name = $item.Name
        path = $relativePath
        bytes = $item.Length
        sha256 = $hash.Hash.ToLowerInvariant()
    })
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if ([string]::IsNullOrWhiteSpace($Version)) {
    $Version = Read-WorkspaceVersion -RepoRoot $repoRoot
}

$outRoot = Join-Path $repoRoot $OutputDir
$stageRoot = Join-Path $outRoot "_stage"
$releaseName = "nanocodex-$Version-$Target"
$stageDir = Join-Path $stageRoot $releaseName
$zipPath = Join-Path $outRoot "$releaseName.zip"
$manifestPath = Join-Path $outRoot "release-manifest.json"
$sumsPath = Join-Path $outRoot "SHA256SUMS.txt"
$artifacts = [System.Collections.ArrayList]::new()

New-Item -ItemType Directory -Force -Path $outRoot | Out-Null
Remove-Item -Recurse -Force -LiteralPath $stageDir -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $stageDir | Out-Null

$rustRoot = Join-Path $repoRoot "rust"
if (-not $SkipTests) {
    Invoke-Step "Rust tests ($Target)" {
        Push-Location $rustRoot
        try {
            & cargo test --workspace --target $Target
        } finally {
            Pop-Location
        }
    }
}

Invoke-Step "Rust release build ($Target)" {
    Push-Location $rustRoot
    try {
        & cargo build --release --workspace --target $Target
    } finally {
        Pop-Location
    }
}

$cliExe = Join-Path $rustRoot "target\$Target\release\ncx.exe"
if (-not (Test-Path -LiteralPath $cliExe)) {
    throw "CLI binary not found: $cliExe"
}

Invoke-Step "Stage CLI package" {
    Copy-Item -LiteralPath $cliExe -Destination (Join-Path $stageDir "ncx.exe") -Force
    foreach ($file in @("README.md", "README.zh-CN.md", "LICENSE", "config.example.toml", "mcp.example.toml")) {
        $source = Join-Path $repoRoot $file
        if (Test-Path -LiteralPath $source) {
            Copy-Item -LiteralPath $source -Destination $stageDir -Force
        }
    }
    Remove-Item -LiteralPath $zipPath -Force -ErrorAction SilentlyContinue
    Compress-Archive -Path (Join-Path $stageDir "*") -DestinationPath $zipPath -Force
    Add-ArtifactRecord -Artifacts $artifacts -Path $zipPath -OutputDir $OutputDir
}

if (-not $SkipTauri) {
    $guiRoot = Join-Path $rustRoot "gui"
    Invoke-Step "Tauri NSIS installer" {
        Push-Location $guiRoot
        try {
            if (-not $NoNpmCi) {
                if (Test-Path -LiteralPath (Join-Path $guiRoot "package-lock.json")) {
                    & npm.cmd ci
                } else {
                    & npm.cmd install
                }
            }
            & npm.cmd run tauri:installer
        } finally {
            Pop-Location
        }
    }

    $nsisDir = Join-Path $guiRoot "src-tauri\target\$Target\release\bundle\nsis"
    $installers = @(Get-ChildItem -LiteralPath $nsisDir -Filter "*.exe" -File -ErrorAction SilentlyContinue)
    if ($installers.Count -eq 0) {
        throw "No NSIS installer found under $nsisDir"
    }
    foreach ($installer in $installers) {
        $dest = Join-Path $outRoot $installer.Name
        Copy-Item -LiteralPath $installer.FullName -Destination $dest -Force
        Add-ArtifactRecord -Artifacts $artifacts -Path $dest -OutputDir $OutputDir
    }
}

$commit = ""
try {
    $commit = (& git -C $repoRoot rev-parse --short HEAD).Trim()
} catch {
    $commit = "unknown"
}

$hashLines = foreach ($artifact in $artifacts) {
    "$($artifact.sha256)  $($artifact.name)"
}
Set-Content -Path $sumsPath -Value $hashLines -Encoding utf8

$manifest = [ordered]@{
    package = "nanocodex"
    version = $Version
    target = $Target
    commit = $commit
    built_at = (Get-Date).ToUniversalTime().ToString("o")
    artifacts = @($artifacts)
}
$manifest | ConvertTo-Json -Depth 5 | Set-Content -Path $manifestPath -Encoding utf8

Write-Host ""
Write-Host "Release artifacts written to $outRoot"
foreach ($artifact in $artifacts) {
    Write-Host " - $($artifact.name) ($($artifact.bytes) bytes, sha256 $($artifact.sha256))"
}
Write-Host " - SHA256SUMS.txt"
Write-Host " - release-manifest.json"
