param(
    [Parameter(Mandatory=$true)][string]$Executable,
    [Parameter(ValueFromRemainingArguments=$true)][string[]]$RemainingArguments
)
$ErrorActionPreference = 'Stop'
$previousKey = $env:NCX_JEV_API_KEY
$previousMode = $env:NCX_JEV_MODE
$env:NCX_JEV_API_KEY = [Environment]::GetEnvironmentVariable('NCX_JEV_API_KEY', 'User')
if ([string]::IsNullOrWhiteSpace($env:NCX_JEV_API_KEY)) {
    throw 'NCX_JEV_API_KEY is not configured for this Windows user.'
}
$env:NCX_JEV_MODE = 'shadow'
try {
    & $Executable @RemainingArguments
    exit $LASTEXITCODE
} finally {
    $env:NCX_JEV_API_KEY = $previousKey
    $env:NCX_JEV_MODE = $previousMode
}
