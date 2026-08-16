param(
    [switch]$NoDownload
)

$ErrorActionPreference = "Stop"

$protocVersion = "35.1"
$protocSha256 = "5D3FF218D7D91EEA95F7569BCB5A98F3030F8996D44151279D9772EDCFF76082"
$protocUrl = "https://github.com/protocolbuffers/protobuf/releases/download/v$protocVersion/protoc-$protocVersion-win64.zip"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$rustRoot = Join-Path $repoRoot "rust"
$toolRoot = Join-Path $rustRoot "target\tools"
$archivePath = Join-Path $toolRoot "protoc-$protocVersion-win64.zip"
$installRoot = Join-Path $toolRoot "protoc-$protocVersion"
$cachedProtoc = Join-Path $installRoot "bin\protoc.exe"

function Invoke-Native {
    param(
        [string]$FilePath,
        [string[]]$Arguments
    )

    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$FilePath exited with code $LASTEXITCODE"
    }
}

function Resolve-Protoc {
    if (-not [string]::IsNullOrWhiteSpace($env:PROTOC)) {
        if (-not (Test-Path -LiteralPath $env:PROTOC -PathType Leaf)) {
            throw "PROTOC does not point to a file: $env:PROTOC"
        }
        return (Resolve-Path -LiteralPath $env:PROTOC).Path
    }

    if (Test-Path -LiteralPath $cachedProtoc -PathType Leaf) {
        return (Resolve-Path -LiteralPath $cachedProtoc).Path
    }

    $pathProtoc = Get-Command "protoc.exe" -ErrorAction SilentlyContinue
    if ($null -ne $pathProtoc) {
        return $pathProtoc.Source
    }

    if ($NoDownload) {
        throw "protoc was not found in PROTOC, rust/target/tools, or PATH"
    }

    New-Item -ItemType Directory -Force -Path $toolRoot | Out-Null
    Invoke-WebRequest -UseBasicParsing $protocUrl -OutFile $archivePath
    $actualHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $archivePath).Hash
    if ($actualHash -ne $protocSha256) {
        throw "protoc archive SHA-256 mismatch: expected $protocSha256, got $actualHash"
    }
    Expand-Archive -LiteralPath $archivePath -DestinationPath $installRoot -Force
    if (-not (Test-Path -LiteralPath $cachedProtoc -PathType Leaf)) {
        throw "protoc archive did not contain bin/protoc.exe"
    }
    (Resolve-Path -LiteralPath $cachedProtoc).Path
}

$env:PROTOC = Resolve-Protoc
Invoke-Native -FilePath $env:PROTOC -Arguments @("--version")

Push-Location $rustRoot
try {
    Invoke-Native -FilePath "cargo" -Arguments @("fmt", "--all", "--", "--check")
    Invoke-Native -FilePath "cargo" -Arguments @(
        "clippy", "--workspace", "--all-targets", "--all-features", "--", "-D", "warnings"
    )
    Invoke-Native -FilePath "cargo" -Arguments @("test", "--workspace", "--all-features")
} finally {
    Pop-Location
}
