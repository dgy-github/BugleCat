param(
    [Parameter(Mandatory = $true)]
    [string]$SourceOutDir
)

$ErrorActionPreference = "Stop"

function Fail {
    param([string]$Message)
    throw "P1 trace verifier self-test failed: $Message"
}

function Get-ErrorText {
    param($ErrorRecord)

    $parts = @()
    if ($ErrorRecord.Exception -and $ErrorRecord.Exception.Message) {
        $parts += $ErrorRecord.Exception.Message
    }
    if ($ErrorRecord.ErrorDetails -and $ErrorRecord.ErrorDetails.Message) {
        $parts += $ErrorRecord.ErrorDetails.Message
    }
    if ($ErrorRecord.FullyQualifiedErrorId) {
        $parts += $ErrorRecord.FullyQualifiedErrorId
    }
    return ($parts -join "`n")
}

function Invoke-VerifierExpectFailure {
    param(
        [string]$OutDir,
        [string]$ExpectedText,
        [string]$CaseName,
        [switch]$AllowLocalDryRun
    )

    $accepted = $false
    $text = ""
    try {
        if ($AllowLocalDryRun) {
            $output = & (Join-Path $PSScriptRoot "p1_verify_rough_cut_trace.ps1") -OutDir $OutDir -AllowLocalDryRun 2>&1
        }
        else {
            $output = & (Join-Path $PSScriptRoot "p1_verify_rough_cut_trace.ps1") -OutDir $OutDir 2>&1
        }
        $text = ($output | Out-String)
        $accepted = $true
    }
    catch {
        $text = Get-ErrorText -ErrorRecord $_
    }
    if ($accepted) {
        Fail "verifier unexpectedly accepted ${CaseName}"
    }
    if (-not $text.Contains($ExpectedText)) {
        Fail "verifier failed ${CaseName} for an unexpected reason: $text"
    }
}

function Invoke-VerifierExpectSuccess {
    param(
        [string]$OutDir,
        [string]$CaseName,
        [switch]$AllowLocalDryRun
    )

    $global:LASTEXITCODE = 0
    if ($AllowLocalDryRun) {
        $output = & (Join-Path $PSScriptRoot "p1_verify_rough_cut_trace.ps1") -OutDir $OutDir -AllowLocalDryRun 2>&1
    }
    else {
        $output = & (Join-Path $PSScriptRoot "p1_verify_rough_cut_trace.ps1") -OutDir $OutDir 2>&1
    }
    $text = ($output | Out-String)
    if ($LASTEXITCODE -ne 0 -or -not $?) {
        Fail "verifier unexpectedly rejected ${CaseName}: $text"
    }
}

function Get-Sha256ContentHash {
    param([string]$Path)
    $hash = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
    return "sha256:$hash"
}

function Write-ShotTracesFromProjectTrace {
    param(
        [string]$Scratch,
        [object]$Trace
    )

    foreach ($shot in @($Trace.shots)) {
        $shotTracePath = Join-Path $Scratch "trace_$($shot.shot_id).json"
        [pscustomobject]@{
            project_id = $Trace.project_id
            shot_id = $shot.shot_id
            jobs = @($shot.jobs)
            artifacts = @($shot.artifacts)
            validations = @($shot.validations)
        } | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $shotTracePath -Encoding UTF8
    }
}

function Reset-Scratch {
    param(
        [string]$Source,
        [string]$Scratch
    )

    Remove-Item -LiteralPath $Scratch -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path $Scratch | Out-Null
    Copy-Item -Path (Join-Path $Source "*") -Destination $Scratch -Recurse -Force

    function Convert-CopiedPath {
        param([object]$Value)

        if (-not $Value) {
            return $Value
        }
        $text = [string]$Value
        $leaf = Split-Path -Leaf $text
        if ($text -like "*_rough_cut_work*") {
            return (Join-Path (Join-Path $Scratch "_rough_cut_work") $leaf)
        }
        if ($text -like "*\text\*" -or $text -like "*/text/*") {
            return (Join-Path (Join-Path $Scratch "text") $leaf)
        }
        return (Join-Path $Scratch $leaf)
    }

    $scratchTracePath = Join-Path $Scratch "trace.json"
    $trace = Get-Content -Raw -LiteralPath $scratchTracePath | ConvertFrom-Json
    $roughArtifacts = @($trace.project_artifacts | Where-Object { $_.kind -eq "rough_cut" })
    if ($roughArtifacts.Count -eq 1) {
        $roughArtifacts[0].params.assembly_manifest = Join-Path $Scratch "assembly_manifest.json"
        $roughArtifacts[0].params.failed_shots = Join-Path $Scratch "failed_shots.json"
        foreach ($validation in @($roughArtifacts[0].validations)) {
            if ($validation.layers -and
                $validation.layers.PSObject.Properties.Name -contains "media_l0" -and
                $validation.layers.media_l0.probe) {
                $roughPath = Join-Path $Scratch "rough_cut.mp4"
                $validation.layers.media_l0.probe.path = $roughPath
                $validation.layers.media_l0.probe.size_bytes = (Get-Item -LiteralPath $roughPath).Length
            }
        }
    }
    foreach ($shot in @($trace.shots)) {
        foreach ($validation in @($shot.validations)) {
            if ($validation.layers -and
                $validation.layers.PSObject.Properties.Name -contains "media_l0" -and
                $validation.layers.media_l0.probe) {
                $path = Convert-CopiedPath -Value $validation.layers.media_l0.probe.path
                $validation.layers.media_l0.probe.path = $path
                $validation.layers.media_l0.probe.size_bytes = (Get-Item -LiteralPath $path).Length
            }
        }
    }
    $trace | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $scratchTracePath -Encoding UTF8

    foreach ($shotTraceFile in Get-ChildItem -LiteralPath $Scratch -Filter "trace_shot_*.json" -File -ErrorAction SilentlyContinue) {
        $shotTrace = Get-Content -Raw -LiteralPath $shotTraceFile.FullName | ConvertFrom-Json
        foreach ($validation in @($shotTrace.validations)) {
            if ($validation.layers -and
                $validation.layers.PSObject.Properties.Name -contains "media_l0" -and
                $validation.layers.media_l0.probe) {
                $path = Convert-CopiedPath -Value $validation.layers.media_l0.probe.path
                $validation.layers.media_l0.probe.path = $path
                $validation.layers.media_l0.probe.size_bytes = (Get-Item -LiteralPath $path).Length
            }
        }
        $shotTrace | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $shotTraceFile.FullName -Encoding UTF8
    }

    $manifestPath = Join-Path $Scratch "assembly_manifest.json"
    $manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
    foreach ($row in @($manifest.shots)) {
        if ($row.clip_path) {
            $row.clip_path = Convert-CopiedPath -Value $row.clip_path
        }
        if ($row.assembled_clip_path) {
            $row.assembled_clip_path = Convert-CopiedPath -Value $row.assembled_clip_path
        }
        if ($row.subtitle_path) {
            $row.subtitle_path = Convert-CopiedPath -Value $row.subtitle_path
        }
        if ($row.audio_path) {
            $row.audio_path = Convert-CopiedPath -Value $row.audio_path
        }
        if ($row.rerun_context) {
            if ($row.rerun_context.PSObject.Properties.Name -contains "tts_audio_path" -and $row.rerun_context.tts_audio_path) {
                $row.rerun_context.tts_audio_path = Convert-CopiedPath -Value $row.rerun_context.tts_audio_path
            }
        }
    }
    $manifest | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $manifestPath -Encoding UTF8
}

function Convert-ScratchToLiveLike {
    param([string]$Scratch)

    $scratchTracePath = Join-Path $Scratch "trace.json"
    $roundtrip = Join-Path $Scratch "seedance_tos_roundtrip.mp4"
    Copy-Item -LiteralPath (Join-Path $Scratch "shot_01.mp4") -Destination $roundtrip -Force
    $roundtripHash = Get-Sha256ContentHash -Path $roundtrip
    $roundtripItem = Get-Item -LiteralPath $roundtrip

    $trace = Get-Content -Raw -LiteralPath $scratchTracePath | ConvertFrom-Json
    $roughArtifacts = @($trace.project_artifacts | Where-Object { $_.kind -eq "rough_cut" })
    if ($roughArtifacts.Count -ne 1) {
        Fail "source trace should start with exactly one rough_cut artifact, found $($roughArtifacts.Count)"
    }
    $roughArtifacts[0].tos_key = "tos://bucket/live/rough_cut.mp4"

    foreach ($shot in @($trace.shots)) {
        foreach ($job in @($shot.jobs)) {
            $job.provider = "ark"
            $job.model = "doubao-seedance-2-0-fast-260128"
            $job.status = "provider_succeeded"
        }
        foreach ($artifact in @($shot.artifacts | Where-Object { $_.kind -eq "video" })) {
            $artifact.tos_key = "tos://bucket/live/$($artifact.id).mp4"
            $artifact.content_hash = $roundtripHash
        }
        foreach ($validation in @($shot.validations)) {
            $validation.stage = "seedance_media_l0"
            if (-not $validation.layers) {
                $validation | Add-Member -MemberType NoteProperty -Name "layers" -Value ([pscustomobject]@{})
            }
            if ($validation.layers.PSObject.Properties.Name -notcontains "tos_roundtrip_path") {
                $validation.layers | Add-Member -MemberType NoteProperty -Name "tos_roundtrip_path" -Value $roundtrip
            }
            else {
                $validation.layers.tos_roundtrip_path = $roundtrip
            }
            if ($validation.layers.PSObject.Properties.Name -contains "dry_run") {
                $validation.layers.PSObject.Properties.Remove("dry_run")
            }
            if ($validation.layers.PSObject.Properties.Name -contains "media_l0" -and $validation.layers.media_l0.probe) {
                $validation.layers.media_l0.probe.path = $roundtrip
                $validation.layers.media_l0.probe.size_bytes = $roundtripItem.Length
            }
        }
    }
    Write-ShotTracesFromProjectTrace -Scratch $Scratch -Trace $trace
    $trace | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $scratchTracePath -Encoding UTF8
}

function Convert-ScratchToPartialDelivery {
    param([string]$Scratch)

    $failedShotId = "shot_02"
    $scratchTracePath = Join-Path $Scratch "trace.json"
    $trace = Get-Content -Raw -LiteralPath $scratchTracePath | ConvertFrom-Json
    $failedShot = @($trace.shots | Where-Object { $_.shot_id -eq $failedShotId })
    if ($failedShot.Count -ne 1) {
        Fail "source trace should have exactly one $failedShotId for partial delivery self-test"
    }
    foreach ($job in @($failedShot[0].jobs)) {
        $job.status = "failed"
        $job.failure_reason = "simulated provider failure for partial delivery self-test"
    }
    $failedShot[0].artifacts = @()
    $failedShot[0].validations = @()
    $trace | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $scratchTracePath -Encoding UTF8

    $shotTracePath = Join-Path $Scratch "trace_$failedShotId.json"
    $shotTrace = Get-Content -Raw -LiteralPath $shotTracePath | ConvertFrom-Json
    foreach ($job in @($shotTrace.jobs)) {
        $job.status = "failed"
        $job.failure_reason = "simulated provider failure for partial delivery self-test"
    }
    $shotTrace.artifacts = @()
    $shotTrace.validations = @()
    $shotTrace | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $shotTracePath -Encoding UTF8

    $failedRows = @(
        [pscustomobject]@{
            shot_id = $failedShotId
            reason = "simulated provider failure for partial delivery self-test"
            rerun_context = [pscustomobject]@{
                shot_text_spec = [pscustomobject]@{
                    duration_s = 0.4
                    visual_prompt = "retry partial delivery shot"
                }
            }
        }
    )
    $failedRows | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath (Join-Path $Scratch "failed_shots.json") -Encoding UTF8

    $manifestPath = Join-Path $Scratch "assembly_manifest.json"
    $manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
    $manifestRows = @($manifest.shots)
    $failedRowsInManifest = @($manifestRows | Where-Object { $_.shot_id -eq $failedShotId })
    if ($failedRowsInManifest.Count -ne 1) {
        Fail "assembly manifest should have exactly one $failedShotId row"
    }
    $failedRowsInManifest[0].clip_path = $null
    $failedRowsInManifest[0].assembled_clip_path = $null
    $failedRowsInManifest[0].subtitle_burned_in = $false
    $failedRowsInManifest[0].audio_muxed = $false
    $failedRowsInManifest[0].silent_audio_inserted = $false
    $failedRowsInManifest[0].assembly_notes = @("simulated failed shot for partial delivery self-test")
    $failedRowsInManifest[0].rerun_context = [pscustomobject]@{
        shot_text_spec = [pscustomobject]@{
            duration_s = 0.4
            visual_prompt = "retry partial delivery shot"
        }
    }
    $manifest | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $manifestPath -Encoding UTF8
}

$source = (Resolve-Path -LiteralPath $SourceOutDir).Path
$tracePath = Join-Path $source "trace.json"
if (-not (Test-Path -LiteralPath $tracePath -PathType Leaf)) {
    Fail "source trace.json is missing: $tracePath"
}

$scratch = Join-Path $env:TEMP ("ncx-video-agent-trace-verifier-selftest-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $scratch | Out-Null

try {
    Reset-Scratch -Source $source -Scratch $scratch

    $scratchTracePath = Join-Path $scratch "trace.json"
    $trace = Get-Content -Raw -LiteralPath $scratchTracePath | ConvertFrom-Json
    $roughArtifacts = @($trace.project_artifacts | Where-Object { $_.kind -eq "rough_cut" })
    if ($roughArtifacts.Count -ne 1) {
        Fail "source trace should start with exactly one rough_cut artifact, found $($roughArtifacts.Count)"
    }

    $duplicate = $roughArtifacts[0] | ConvertTo-Json -Depth 32 | ConvertFrom-Json
    $duplicate.id = "$($duplicate.id)_duplicate"
    foreach ($validation in @($duplicate.validations)) {
        $validation.id = "$($validation.id)_duplicate"
        $validation.artifact_id = $duplicate.id
    }
    $trace.project_artifacts = @($trace.project_artifacts) + @($duplicate)
    $trace | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $scratchTracePath -Encoding UTF8
    Invoke-VerifierExpectFailure `
        -OutDir $scratch `
        -ExpectedText "exactly one project rough_cut artifact" `
        -CaseName "duplicate rough_cut artifacts" `
        -AllowLocalDryRun

    Reset-Scratch -Source $source -Scratch $scratch
    $trace = Get-Content -Raw -LiteralPath $scratchTracePath | ConvertFrom-Json
    $shots = @($trace.shots)
    if ($shots.Count -eq 0 -or @($shots[0].jobs).Count -eq 0) {
        Fail "source trace should have at least one shot job"
    }
    $trace.shots[0].jobs[0].status = "provider_running"
    $trace | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $scratchTracePath -Encoding UTF8
    Invoke-VerifierExpectFailure `
        -OutDir $scratch `
        -ExpectedText "non-terminal generation job status" `
        -CaseName "non-terminal generation job status" `
        -AllowLocalDryRun

    Reset-Scratch -Source $source -Scratch $scratch
    $trace = Get-Content -Raw -LiteralPath $scratchTracePath | ConvertFrom-Json
    $roughArtifacts = @($trace.project_artifacts | Where-Object { $_.kind -eq "rough_cut" })
    if ($roughArtifacts.Count -ne 1) {
        Fail "source trace should start with exactly one rough_cut artifact, found $($roughArtifacts.Count)"
    }
    $roughArtifacts[0].params.PSObject.Properties.Remove("assembly_manifest")
    $trace | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $scratchTracePath -Encoding UTF8
    Invoke-VerifierExpectFailure `
        -OutDir $scratch `
        -ExpectedText "rough_cut params missing assembly_manifest" `
        -CaseName "missing rough_cut assembly manifest pointer" `
        -AllowLocalDryRun

    Reset-Scratch -Source $source -Scratch $scratch
    $trace = Get-Content -Raw -LiteralPath $scratchTracePath | ConvertFrom-Json
    $roughArtifacts = @($trace.project_artifacts | Where-Object { $_.kind -eq "rough_cut" })
    if ($roughArtifacts.Count -ne 1) {
        Fail "source trace should start with exactly one rough_cut artifact, found $($roughArtifacts.Count)"
    }
    $roughArtifacts[0].params.assembly_manifest = $scratchTracePath
    $trace | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $scratchTracePath -Encoding UTF8
    Invoke-VerifierExpectFailure `
        -OutDir $scratch `
        -ExpectedText "rough_cut params assembly_manifest must point to" `
        -CaseName "rough_cut assembly manifest pointer to wrong existing file" `
        -AllowLocalDryRun

    Reset-Scratch -Source $source -Scratch $scratch
    $trace = Get-Content -Raw -LiteralPath $scratchTracePath | ConvertFrom-Json
    $roughArtifacts = @($trace.project_artifacts | Where-Object { $_.kind -eq "rough_cut" })
    if ($roughArtifacts.Count -ne 1) {
        Fail "source trace should start with exactly one rough_cut artifact, found $($roughArtifacts.Count)"
    }
    $roughArtifacts[0].content_hash = "sha256:$("0" * 64)"
    $trace | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $scratchTracePath -Encoding UTF8
    Invoke-VerifierExpectFailure `
        -OutDir $scratch `
        -ExpectedText "content_hash must match local rough_cut sha256" `
        -CaseName "rough_cut content_hash mismatch" `
        -AllowLocalDryRun

    Reset-Scratch -Source $source -Scratch $scratch
    $trace = Get-Content -Raw -LiteralPath $scratchTracePath | ConvertFrom-Json
    $roughValidation = @($trace.project_artifacts |
        Where-Object { $_.kind -eq "rough_cut" } |
        ForEach-Object { @($_.validations) } |
        Where-Object { $_.stage -eq "rough_cut_media_l0" })[0]
    $roughValidation.layers.media_l0.probe.size_bytes = 1
    $trace | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $scratchTracePath -Encoding UTF8
    Invoke-VerifierExpectFailure `
        -OutDir $scratch `
        -ExpectedText "media_l0 probe size_bytes mismatch" `
        -CaseName "rough_cut media_l0 size mismatch" `
        -AllowLocalDryRun

    Reset-Scratch -Source $source -Scratch $scratch
    $trace = Get-Content -Raw -LiteralPath $scratchTracePath | ConvertFrom-Json
    $videoArtifacts = @($trace.shots | ForEach-Object { @($_.artifacts) } | Where-Object { $_.kind -eq "video" })
    if ($videoArtifacts.Count -eq 0) {
        Fail "source trace should have video artifacts for local-size self-test"
    }
    $videoArtifacts[0].content_hash = "local-size-1"
    $trace | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $scratchTracePath -Encoding UTF8
    $shotTracePath = Join-Path $scratch "trace_shot_01.json"
    $shotTrace = Get-Content -Raw -LiteralPath $shotTracePath | ConvertFrom-Json
    $shotTrace.artifacts[0].content_hash = "local-size-1"
    $shotTrace | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $shotTracePath -Encoding UTF8
    Invoke-VerifierExpectFailure `
        -OutDir $scratch `
        -ExpectedText "content_hash must match probed file size" `
        -CaseName "local video artifact local-size mismatch" `
        -AllowLocalDryRun

    Reset-Scratch -Source $source -Scratch $scratch
    $shotTracePath = Join-Path $scratch "trace_shot_01.json"
    $shotTrace = Get-Content -Raw -LiteralPath $shotTracePath | ConvertFrom-Json
    $shotTrace.jobs[0].id = "$($shotTrace.jobs[0].id)_mismatch"
    $shotTrace | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $shotTracePath -Encoding UTF8
    Invoke-VerifierExpectFailure `
        -OutDir $scratch `
        -ExpectedText "trace_shot_01.json jobs mismatch" `
        -CaseName "shot trace job content mismatch" `
        -AllowLocalDryRun

    Reset-Scratch -Source $source -Scratch $scratch
    $shotTracePath = Join-Path $scratch "trace_shot_01.json"
    $shotTrace = Get-Content -Raw -LiteralPath $shotTracePath | ConvertFrom-Json
    $shotTrace.artifacts[0].id = "$($shotTrace.artifacts[0].id)_mismatch"
    $shotTrace | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $shotTracePath -Encoding UTF8
    Invoke-VerifierExpectFailure `
        -OutDir $scratch `
        -ExpectedText "trace_shot_01.json artifacts mismatch" `
        -CaseName "shot trace artifact content mismatch" `
        -AllowLocalDryRun

    Reset-Scratch -Source $source -Scratch $scratch
    $trace = Get-Content -Raw -LiteralPath $scratchTracePath | ConvertFrom-Json
    $trace.shots[0].validations[0].artifact_id = "missing_artifact"
    $trace | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $scratchTracePath -Encoding UTF8
    Write-ShotTracesFromProjectTrace -Scratch $scratch -Trace $trace
    Invoke-VerifierExpectFailure `
        -OutDir $scratch `
        -ExpectedText "references unknown artifact missing_artifact" `
        -CaseName "shot validation references unknown artifact" `
        -AllowLocalDryRun

    Reset-Scratch -Source $source -Scratch $scratch
    $trace = Get-Content -Raw -LiteralPath $scratchTracePath | ConvertFrom-Json
    $duplicateValidation = $trace.shots[0].validations[0].PSObject.Copy()
    $duplicateValidation.id = "$($duplicateValidation.id)_duplicate"
    $trace.shots[0].validations = @($trace.shots[0].validations) + @($duplicateValidation)
    $trace | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $scratchTracePath -Encoding UTF8
    Write-ShotTracesFromProjectTrace -Scratch $scratch -Trace $trace
    Invoke-VerifierExpectFailure `
        -OutDir $scratch `
        -ExpectedText "duplicate pass validation" `
        -CaseName "duplicate pass validation for same artifact stage" `
        -AllowLocalDryRun

    Reset-Scratch -Source $source -Scratch $scratch
    $trace = Get-Content -Raw -LiteralPath $scratchTracePath | ConvertFrom-Json
    $briefValidation = @($trace.project_artifacts |
        Where-Object { $_.id -eq "artifact_agent_brief" } |
        ForEach-Object { @($_.validations) } |
        Where-Object { $_.stage -eq "brief_self_check" })[0]
    $briefValidation.layers.PSObject.Properties.Remove("node_contract")
    $trace | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $scratchTracePath -Encoding UTF8
    Invoke-VerifierExpectFailure `
        -OutDir $scratch `
        -ExpectedText "missing node_contract evidence" `
        -CaseName "agent validation missing node contract evidence" `
        -AllowLocalDryRun

    Reset-Scratch -Source $source -Scratch $scratch
    $trace = Get-Content -Raw -LiteralPath $scratchTracePath | ConvertFrom-Json
    $chaptersValidation = @($trace.project_artifacts |
        Where-Object { $_.id -eq "artifact_agent_chapters" } |
        ForEach-Object { @($_.validations) } |
        Where-Object { $_.stage -eq "chapters_self_check" })[0]
    $chaptersValidation.layers.node_contract.context_packet.params |
        Add-Member -MemberType NoteProperty -Name "chain_of_thought" -Value "hidden scratchpad" -Force
    $trace | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $scratchTracePath -Encoding UTF8
    Invoke-VerifierExpectFailure `
        -OutDir $scratch `
        -ExpectedText "forbidden reasoning/history field" `
        -CaseName "agent context packet reasoning leak" `
        -AllowLocalDryRun

    Reset-Scratch -Source $source -Scratch $scratch
    Convert-ScratchToPartialDelivery -Scratch $scratch
    Invoke-VerifierExpectSuccess `
        -OutDir $scratch `
        -CaseName "synthetic partial delivery trace" `
        -AllowLocalDryRun

    Reset-Scratch -Source $source -Scratch $scratch
    $failedRows = @(
        [pscustomobject]@{
            shot_id = "missing_shot"
            reason = "synthetic unknown failed shot"
            rerun_context = [pscustomobject]@{
                shot_text_spec = [pscustomobject]@{
                    duration_s = 0.4
                    visual_prompt = "unknown shot should not be accepted"
                }
            }
        }
    )
    $failedRows | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath (Join-Path $scratch "failed_shots.json") -Encoding UTF8
    Invoke-VerifierExpectFailure `
        -OutDir $scratch `
        -ExpectedText "failed_shots.json row references unknown shot" `
        -CaseName "failed_shots row references unknown shot" `
        -AllowLocalDryRun

    Reset-Scratch -Source $source -Scratch $scratch
    Convert-ScratchToPartialDelivery -Scratch $scratch
    $failedPath = Join-Path $scratch "failed_shots.json"
    $failedRows = @((Get-Content -Raw -LiteralPath $failedPath | ConvertFrom-Json))
    $failedRows = @($failedRows) + @($failedRows[0].PSObject.Copy())
    $failedRows | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $failedPath -Encoding UTF8
    Invoke-VerifierExpectFailure `
        -OutDir $scratch `
        -ExpectedText "duplicate failed_shots.json row" `
        -CaseName "duplicate failed_shots row" `
        -AllowLocalDryRun

    Reset-Scratch -Source $source -Scratch $scratch
    $failedRows = @(
        [pscustomobject]@{
            shot_id = "shot_01"
            reason = "synthetic inconsistent failed shot"
            rerun_context = [pscustomobject]@{
                shot_text_spec = [pscustomobject]@{
                    duration_s = 0.4
                    visual_prompt = "delivered shot should not be listed failed"
                }
            }
        }
    )
    $failedRows | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath (Join-Path $scratch "failed_shots.json") -Encoding UTF8
    Invoke-VerifierExpectFailure `
        -OutDir $scratch `
        -ExpectedText "is listed in failed_shots.json but still has video artifact" `
        -CaseName "delivered shot listed as failed" `
        -AllowLocalDryRun

    Reset-Scratch -Source $source -Scratch $scratch
    $manifestPath = Join-Path $scratch "assembly_manifest.json"
    $manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
    $manifest.shots = @($manifest.shots | Where-Object { $_.shot_id -ne "shot_02" })
    $manifest | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $manifestPath -Encoding UTF8
    Invoke-VerifierExpectFailure `
        -OutDir $scratch `
        -ExpectedText "assembly_manifest missing row for shot shot_02" `
        -CaseName "assembly manifest missing delivered shot" `
        -AllowLocalDryRun

    Reset-Scratch -Source $source -Scratch $scratch
    $manifestPath = Join-Path $scratch "assembly_manifest.json"
    $manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
    $extraManifestRow = [pscustomobject]@{
        shot_id = "missing_shot"
        clip_path = $null
        assembled_clip_path = $null
        subtitle_burned_in = $false
        audio_muxed = $false
        silent_audio_inserted = $false
        assembly_notes = @("unknown shot should not be accepted")
        rerun_context = [pscustomobject]@{}
    }
    $manifest.shots = @($manifest.shots) + @($extraManifestRow)
    $manifest | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $manifestPath -Encoding UTF8
    Invoke-VerifierExpectFailure `
        -OutDir $scratch `
        -ExpectedText "assembly_manifest row references unknown shot" `
        -CaseName "assembly manifest unknown shot" `
        -AllowLocalDryRun

    Reset-Scratch -Source $source -Scratch $scratch
    $manifestPath = Join-Path $scratch "assembly_manifest.json"
    $manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
    $manifest.shots = @($manifest.shots) + @($manifest.shots[0].PSObject.Copy())
    $manifest | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $manifestPath -Encoding UTF8
    Invoke-VerifierExpectFailure `
        -OutDir $scratch `
        -ExpectedText "duplicate assembly_manifest row" `
        -CaseName "duplicate assembly manifest row" `
        -AllowLocalDryRun

    Reset-Scratch -Source $source -Scratch $scratch
    $manifestPath = Join-Path $scratch "assembly_manifest.json"
    $manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
    $deliveredRows = @($manifest.shots | Where-Object { $_.shot_id -eq "shot_01" })
    if ($deliveredRows.Count -ne 1) {
        Fail "source manifest should have exactly one shot_01 row"
    }
    $deliveredRows[0].assembled_clip_path = $null
    $manifest | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $manifestPath -Encoding UTF8
    Invoke-VerifierExpectFailure `
        -OutDir $scratch `
        -ExpectedText "delivered shot shot_01 assembly_manifest missing assembled_clip_path" `
        -CaseName "delivered manifest row missing assembled clip" `
        -AllowLocalDryRun

    Reset-Scratch -Source $source -Scratch $scratch
    Convert-ScratchToLiveLike -Scratch $scratch
    Invoke-VerifierExpectSuccess `
        -OutDir $scratch `
        -CaseName "synthetic live roundtrip trace"

    Reset-Scratch -Source $source -Scratch $scratch
    Convert-ScratchToLiveLike -Scratch $scratch
    $trace = Get-Content -Raw -LiteralPath $scratchTracePath | ConvertFrom-Json
    $seedanceValidations = @($trace.shots | ForEach-Object { @($_.validations) } | Where-Object { $_.stage -eq "seedance_media_l0" })
    if ($seedanceValidations.Count -eq 0) {
        Fail "synthetic live trace should have seedance_media_l0 validations"
    }
    $seedanceValidations[0].layers.PSObject.Properties.Remove("tos_roundtrip_path")
    $trace | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $scratchTracePath -Encoding UTF8
    Write-ShotTracesFromProjectTrace -Scratch $scratch -Trace $trace
    Invoke-VerifierExpectFailure `
        -OutDir $scratch `
        -ExpectedText "missing tos_roundtrip_path" `
        -CaseName "live seedance validation missing TOS roundtrip path"

    Reset-Scratch -Source $source -Scratch $scratch
    Convert-ScratchToLiveLike -Scratch $scratch
    $trace = Get-Content -Raw -LiteralPath $scratchTracePath | ConvertFrom-Json
    $seedanceValidations = @($trace.shots | ForEach-Object { @($_.validations) } | Where-Object { $_.stage -eq "seedance_media_l0" })
    if ($seedanceValidations.Count -eq 0) {
        Fail "synthetic live trace should have seedance_media_l0 validations"
    }
    $seedanceValidations[0].layers.tos_roundtrip_path = $scratchTracePath
    $trace | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $scratchTracePath -Encoding UTF8
    Write-ShotTracesFromProjectTrace -Scratch $scratch -Trace $trace
    Invoke-VerifierExpectFailure `
        -OutDir $scratch `
        -ExpectedText "live seedance_media_l0 tos_roundtrip_path must point to" `
        -CaseName "live seedance validation TOS roundtrip path points to wrong file"

    Reset-Scratch -Source $source -Scratch $scratch
    Convert-ScratchToLiveLike -Scratch $scratch
    $trace = Get-Content -Raw -LiteralPath $scratchTracePath | ConvertFrom-Json
    $videoArtifacts = @($trace.shots | ForEach-Object { @($_.artifacts) } | Where-Object { $_.kind -eq "video" })
    if ($videoArtifacts.Count -eq 0) {
        Fail "synthetic live trace should have video artifacts"
    }
    $videoArtifacts[0].content_hash = "sha256:$("1" * 64)"
    $trace | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $scratchTracePath -Encoding UTF8
    Write-ShotTracesFromProjectTrace -Scratch $scratch -Trace $trace
    Invoke-VerifierExpectFailure `
        -OutDir $scratch `
        -ExpectedText "seedance_tos_roundtrip" `
        -CaseName "live video artifact content_hash mismatch"

    Write-Host "PASS P1 trace verifier negative self-test"
}
finally {
    Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction SilentlyContinue
}
