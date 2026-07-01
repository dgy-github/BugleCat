param(
    [ValidateSet("bin", "ftz")]
    [string]$Format = "bin",
    [string]$OutDir = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\target\tools')).Path 'fasttext')
)

$ErrorActionPreference = "Stop"

$fileName = "lid.176.$Format"
$url = "https://dl.fbaipublicfiles.com/fasttext/supervised-models/$fileName"
$target = Join-Path $OutDir $fileName

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

if (Test-Path -LiteralPath $target) {
    $existing = Get-Item -LiteralPath $target
    if ($existing.Length -gt 0) {
        Write-Host "fastText model already exists: $target"
        Write-Host "Use this for smoke:"
        if ($Format -eq "bin") {
            Write-Host "`$env:LID_176_BIN = '$target'"
        } else {
            Write-Host "`$env:LID_176_FTZ = '$target'"
        }
        exit 0
    }
}

Write-Host "Downloading official fastText language-identification model:"
Write-Host $url
Invoke-WebRequest -Uri $url -OutFile $target

$downloaded = Get-Item -LiteralPath $target
if ($downloaded.Length -le 0) {
    throw "Downloaded model is empty: $target"
}

Write-Host "Downloaded: $target"
Write-Host "Size bytes: $($downloaded.Length)"
Write-Host "Use this for smoke:"
if ($Format -eq "bin") {
    Write-Host "`$env:LID_176_BIN = '$target'"
} else {
    Write-Host "`$env:LID_176_FTZ = '$target'"
}
