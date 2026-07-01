param(
    [Parameter(Mandatory = $true)]
    [string]$OutDir,
    [switch]$AllowLocalDryRun,
    [double]$MinDurationSeconds = 0.1
)

$ErrorActionPreference = "Stop"

function Fail {
    param([string]$Message)
    throw "P1 output verification failed: $Message"
}

function Require-File {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        Fail "missing file: $Path"
    }
    $item = Get-Item -LiteralPath $Path
    if ($item.Length -le 0) {
        Fail "empty file: $Path"
    }
    return $item
}

function Assert-SameFile {
    param(
        [string]$Actual,
        [string]$Expected,
        [string]$Description
    )

    Require-File -Path $Actual | Out-Null
    Require-File -Path $Expected | Out-Null
    $actualResolved = (Resolve-Path -LiteralPath $Actual).Path
    $expectedResolved = (Resolve-Path -LiteralPath $Expected).Path
    if (-not [string]::Equals($actualResolved, $expectedResolved, [StringComparison]::OrdinalIgnoreCase)) {
        Fail "$Description must point to $expectedResolved, got $actualResolved"
    }
}

function Read-JsonFile {
    param([string]$Path)
    Require-File -Path $Path | Out-Null
    try {
        return (Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json)
    }
    catch {
        Fail "invalid JSON in ${Path}: $($_.Exception.Message)"
    }
}

function Get-FfprobeDuration {
    param([string]$Path)
    Require-File -Path $Path | Out-Null
    $output = & ffprobe -v error -show_entries format=duration -of default=noprint_wrappers=1:nokey=1 $Path 2>&1
    if ($LASTEXITCODE -ne 0) {
        Fail "ffprobe failed for ${Path}: $($output -join "`n")"
    }
    $duration = 0.0
    if (-not [double]::TryParse(($output -join "").Trim(), [Globalization.NumberStyles]::Float, [Globalization.CultureInfo]::InvariantCulture, [ref]$duration)) {
        Fail "ffprobe duration was not numeric for ${Path}: $output"
    }
    if ($duration -lt $MinDurationSeconds) {
        Fail "duration too short for ${Path}: $duration seconds"
    }
    return $duration
}

function Test-ArtifactUri {
    param(
        [string]$Uri,
        [string]$Description
    )
    if ([string]::IsNullOrWhiteSpace($Uri)) {
        Fail "$Description has empty uri"
    }
    if ($Uri.StartsWith("tos://")) {
        return
    }
    if ($AllowLocalDryRun -and $Uri.StartsWith("local://")) {
        return
    }
    Fail "$Description must use tos:// URI in live mode: $Uri"
}

function Test-ContentHash {
    param(
        [string]$Hash,
        [string]$Description
    )
    if ([string]::IsNullOrWhiteSpace($Hash)) {
        Fail "$Description has empty content_hash"
    }
    if ($Hash.StartsWith("sha256:")) {
        return
    }
    if ($AllowLocalDryRun -and $Hash.StartsWith("local-size-")) {
        return
    }
    Fail "$Description has non-live content_hash: $Hash"
}

function Get-Sha256ContentHash {
    param([string]$Path)
    Require-File -Path $Path | Out-Null
    $hash = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
    return "sha256:$hash"
}

function Assert-PassingValidation {
    param(
        [object[]]$Validations,
        [string]$ArtifactId,
        [string]$Stage
    )
    $matches = @($Validations | Where-Object {
        $_.artifact_id -eq $ArtifactId -and $_.stage -eq $Stage -and $_.verdict -eq "pass"
    })
    if ($matches.Count -eq 0) {
        Fail "artifact $ArtifactId has no pass validation for stage $Stage"
    }
}

function New-ArtifactIdSet {
    param([object[]]$Artifacts)

    $set = @{}
    foreach ($artifact in $Artifacts) {
        $id = [string]$artifact.id
        if ([string]::IsNullOrWhiteSpace($id)) {
            Fail "artifact missing id"
        }
        if ($set.ContainsKey($id)) {
            Fail "duplicate artifact id $id"
        }
        $set[$id] = $true
    }
    return $set
}

function Assert-ValidationRecordsIntegrity {
    param(
        [object[]]$Validations,
        [hashtable]$ArtifactIds,
        [string]$Description
    )

    $validationIds = @{}
    $passKeys = @{}
    foreach ($validation in $Validations) {
        foreach ($name in @("id", "artifact_id", "stage", "gate_version", "verdict", "confidence", "layers")) {
            Assert-ObjectProperty -Object $validation -Name $name -Description "$Description validation"
        }

        $id = [string]$validation.id
        if ([string]::IsNullOrWhiteSpace($id)) {
            Fail "$Description has validation with empty id"
        }
        if ($validationIds.ContainsKey($id)) {
            Fail "$Description has duplicate validation id $id"
        }
        $validationIds[$id] = $true

        $artifactId = [string]$validation.artifact_id
        if ([string]::IsNullOrWhiteSpace($artifactId)) {
            Fail "$Description validation $id has empty artifact_id"
        }
        if (-not $ArtifactIds.ContainsKey($artifactId)) {
            Fail "$Description validation $id references unknown artifact $artifactId"
        }

        $stage = [string]$validation.stage
        if ([string]::IsNullOrWhiteSpace($stage)) {
            Fail "$Description validation $id has empty stage"
        }
        if ([string]::IsNullOrWhiteSpace([string]$validation.gate_version)) {
            Fail "$Description validation $id has empty gate_version"
        }
        $verdict = [string]$validation.verdict
        if (@("pass", "repair", "escalate") -notcontains $verdict) {
            Fail "$Description validation $id has invalid verdict $verdict"
        }
        $confidence = [double]$validation.confidence
        if ($confidence -lt 0 -or $confidence -gt 1) {
            Fail "$Description validation $id has out-of-range confidence $confidence"
        }
        if ($null -eq $validation.layers) {
            Fail "$Description validation $id has null layers"
        }
        if ($verdict -eq "pass") {
            $passKey = "$artifactId|$stage"
            if ($passKeys.ContainsKey($passKey)) {
                Fail "$Description has duplicate pass validation for artifact $artifactId stage $stage"
            }
            $passKeys[$passKey] = $true
        }
    }
}

function Get-PassingValidations {
    param(
        [object[]]$Validations,
        [string]$ArtifactId,
        [string]$Stage,
        [string]$Description
    )

    $matches = @($Validations | Where-Object {
        $_.artifact_id -eq $ArtifactId -and $_.stage -eq $Stage -and $_.verdict -eq "pass"
    })
    if ($matches.Count -eq 0) {
        Fail "$Description has no pass validation for stage $Stage"
    }
    return $matches
}

function Assert-MediaL0Validation {
    param(
        [object]$Validation,
        [string]$Description,
        [string]$ExpectedPath = $null
    )

    if ($null -eq $Validation.layers -or $Validation.layers.PSObject.Properties.Name -notcontains "media_l0") {
        Fail "$Description missing media_l0 layers"
    }
    $media = $Validation.layers.media_l0
    if ($media.passed -ne $true) {
        Fail "$Description media_l0 did not pass"
    }
    if (@($media.reasons).Count -gt 0) {
        Fail "$Description media_l0 pass has reasons"
    }
    if ($null -eq $media.probe) {
        Fail "$Description media_l0 missing probe"
    }

    $probe = $media.probe
    $probePath = [string]$probe.path
    if ([string]::IsNullOrWhiteSpace($probePath)) {
        Fail "$Description media_l0 probe missing path"
    }
    if ($ExpectedPath) {
        Assert-SameFile -Actual $probePath -Expected $ExpectedPath -Description "$Description media_l0 probe path"
    }
    else {
        Require-File -Path $probePath | Out-Null
    }

    $item = Get-Item -LiteralPath $probePath
    $sizeBytes = [int64]$probe.size_bytes
    if ($sizeBytes -ne $item.Length) {
        Fail "$Description media_l0 probe size_bytes mismatch: probe=$sizeBytes actual=$($item.Length)"
    }
    if ([double]$probe.duration_s -le 0) {
        Fail "$Description media_l0 probe duration_s must be positive"
    }
    if ([int64]$probe.video_streams -lt 1) {
        Fail "$Description media_l0 probe has no video stream"
    }
    if ([int64]$probe.width -lt 1 -or [int64]$probe.height -lt 1) {
        Fail "$Description media_l0 probe has invalid dimensions"
    }
    return $probePath
}

function Assert-LocalSizeContentHash {
    param(
        [string]$Hash,
        [string]$Path,
        [string]$Description
    )

    if (-not $Hash.StartsWith("local-size-")) {
        Fail "$Description local dry-run content_hash must use local-size-N, got $Hash"
    }
    $expected = "local-size-$((Get-Item -LiteralPath $Path).Length)"
    if (-not [string]::Equals($Hash, $expected, [StringComparison]::OrdinalIgnoreCase)) {
        Fail "$Description content_hash must match probed file size $expected, got $Hash"
    }
}

function ConvertTo-JsonText {
    param([object]$Value)
    if ($null -eq $Value) {
        return ""
    }
    return ($Value | ConvertTo-Json -Depth 32 -Compress)
}

function Assert-ContainsNoTextOverlays {
    param(
        [object]$Params,
        [string]$Description
    )

    $text = (ConvertTo-JsonText -Value $Params).ToLowerInvariant()
    if (-not $text.Contains("no text overlays")) {
        Fail "$Description does not carry no text overlays generation constraint"
    }
}

function Assert-CompletedJobStatus {
    param(
        [object]$Job,
        [string]$ShotId,
        [switch]$AllowFailed
    )

    $status = [string]$Job.status
    if ([string]::IsNullOrWhiteSpace($status)) {
        Fail "shot $ShotId has job without status"
    }
    if (@("provider_succeeded", "settled") -contains $status) {
        return
    }
    if ($AllowFailed -and @("failed", "submit_failed") -contains $status) {
        if ([string]::IsNullOrWhiteSpace([string]$Job.failure_reason)) {
            Fail "failed shot $ShotId has terminal failed job without failure_reason"
        }
        return
    }
    Fail "shot $ShotId has non-terminal generation job status for $($Job.id): $status"
}

function Assert-ObjectProperty {
    param(
        [object]$Object,
        [string]$Name,
        [string]$Description
    )

    if ($null -eq $Object -or $Object.PSObject.Properties.Name -notcontains $Name) {
        Fail "$Description missing $Name"
    }
}

function Assert-S12JobTraceFields {
    param(
        [object]$Job,
        [string]$ShotId
    )

    foreach ($name in @("id", "attempt", "provider", "model", "params", "cost", "latency_ms", "failure_reason")) {
        Assert-ObjectProperty -Object $Job -Name $name -Description "S12 trace job for shot $ShotId"
    }
    if ([string]::IsNullOrWhiteSpace([string]$Job.model)) {
        Fail "S12 trace job for shot $ShotId has empty model"
    }
    if ([string]::IsNullOrWhiteSpace([string]$Job.provider)) {
        Fail "S12 trace job for shot $ShotId has empty provider"
    }
    if ($null -eq $Job.params) {
        Fail "S12 trace job for shot $ShotId has null params"
    }
    if ($null -eq $Job.latency_ms) {
        Fail "S12 trace job for shot $ShotId missing concrete latency_ms"
    }
    $latency = [int64]$Job.latency_ms
    if ($latency -lt 0) {
        Fail "S12 trace job for shot $ShotId has negative latency_ms: $latency"
    }
    $cost = [double]$Job.cost
    if ($cost -lt 0) {
        Fail "S12 trace job for shot $ShotId has negative cost: $cost"
    }
    $attempt = [int64]$Job.attempt
    if ($attempt -lt 0) {
        Fail "S12 trace job for shot $ShotId has negative attempt: $attempt"
    }
    $status = [string]$Job.status
    if ($status.Contains("fail") -and [string]::IsNullOrWhiteSpace([string]$Job.failure_reason)) {
        Fail "S12 trace job for shot $ShotId has failed status without failure_reason"
    }
}

function Assert-JsonEquivalent {
    param(
        [object]$Actual,
        [object]$Expected,
        [string]$Description
    )

    $actualText = ConvertTo-JsonText -Value $Actual
    $expectedText = ConvertTo-JsonText -Value $Expected
    if (-not [string]::Equals($actualText, $expectedText, [StringComparison]::Ordinal)) {
        Fail "$Description mismatch"
    }
}

function Assert-ShotTraceFile {
    param(
        [string]$OutDir,
        [string]$ProjectId,
        [object]$Shot
    )

    $shotId = [string]$Shot.shot_id
    $shotTracePath = Join-Path $OutDir "trace_$shotId.json"
    $shotTrace = Read-JsonFile -Path $shotTracePath
    if ($shotTrace.project_id -ne $ProjectId) {
        Fail "shot trace $shotTracePath project_id mismatch: $($shotTrace.project_id)"
    }
    if ($shotTrace.shot_id -ne $shotId) {
        Fail "shot trace $shotTracePath shot_id mismatch: $($shotTrace.shot_id)"
    }
    if (@($shotTrace.jobs).Count -ne @($Shot.jobs).Count) {
        Fail "shot trace $shotTracePath job count mismatch"
    }
    foreach ($job in @($shotTrace.jobs)) {
        Assert-S12JobTraceFields -Job $job -ShotId $shotId
    }
    Assert-JsonEquivalent -Actual @($shotTrace.jobs) -Expected @($Shot.jobs) -Description "shot trace $shotTracePath jobs"
    if (@($shotTrace.artifacts).Count -ne @($Shot.artifacts).Count) {
        Fail "shot trace $shotTracePath artifact count mismatch"
    }
    Assert-JsonEquivalent -Actual @($shotTrace.artifacts) -Expected @($Shot.artifacts) -Description "shot trace $shotTracePath artifacts"
    if (@($shotTrace.validations).Count -ne @($Shot.validations).Count) {
        Fail "shot trace $shotTracePath validation count mismatch"
    }
    Assert-JsonEquivalent -Actual @($shotTrace.validations) -Expected @($Shot.validations) -Description "shot trace $shotTracePath validations"
}

function Assert-BudgetTrace {
    param(
        [object]$Budget,
        [double]$JobCostTotal
    )

    if ($null -eq $Budget) {
        Fail "trace.json missing budget summary"
    }
    foreach ($name in @("budget_total", "budget_reserved", "budget_spent", "job_cost_total", "job_reserved_total")) {
        Assert-ObjectProperty -Object $Budget -Name $name -Description "trace budget summary"
        $value = [double]$Budget.$name
        if ($value -lt -0.000001) {
            Fail "trace budget summary $name is negative: $value"
        }
    }
    $spent = [double]$Budget.budget_spent
    $traceJobCost = [double]$Budget.job_cost_total
    if ([Math]::Abs($spent - $traceJobCost) -gt 0.000001) {
        Fail "trace budget_spent must equal DB job_cost_total: spent=$spent job_cost_total=$traceJobCost"
    }
    if ([Math]::Abs($traceJobCost - $JobCostTotal) -gt 0.000001) {
        Fail "trace budget job_cost_total must equal exported job costs: budget=$traceJobCost trace_jobs=$JobCostTotal"
    }
    $reserved = [double]$Budget.budget_reserved
    $jobReserved = [double]$Budget.job_reserved_total
    if ([Math]::Abs($reserved - $jobReserved) -gt 0.000001) {
        Fail "trace budget_reserved must equal unsettled job_reserved_total: reserved=$reserved job_reserved_total=$jobReserved"
    }
}

function Get-FailedShotMap {
    param([object[]]$FailedRows)

    $map = @{}
    foreach ($row in $FailedRows) {
        $shotId = [string]$row.shot_id
        if ([string]::IsNullOrWhiteSpace($shotId)) {
            Fail "failed_shots.json row missing shot_id"
        }
        if ([string]::IsNullOrWhiteSpace([string]$row.reason)) {
            Fail "failed_shots.json row for $shotId missing reason"
        }
        if ($null -eq $row.rerun_context) {
            Fail "failed_shots.json row for $shotId missing rerun_context"
        }
        if ($map.ContainsKey($shotId)) {
            Fail "duplicate failed_shots.json row for $shotId"
        }
        $map[$shotId] = $row
    }
    return $map
}

function Assert-FailedShotManifest {
    param(
        [object[]]$ManifestRows,
        [string]$ShotId
    )

    $rows = @($ManifestRows | Where-Object { $_.shot_id -eq $ShotId })
    if ($rows.Count -ne 1) {
        Fail "failed shot $ShotId must have exactly one assembly_manifest row, found $($rows.Count)"
    }
    $row = $rows[0]
    if ($row.assembled_clip_path) {
        Fail "failed shot $ShotId must not have assembled_clip_path"
    }
    if (-not @($row.assembly_notes).Count) {
        Fail "failed shot $ShotId missing assembly_notes"
    }
    if ($null -eq $row.rerun_context) {
        Fail "failed shot $ShotId manifest row missing rerun_context"
    }
}

function Get-ManifestShotMap {
    param(
        [object[]]$ManifestRows,
        [hashtable]$TraceShotIds
    )

    $map = @{}
    foreach ($row in $ManifestRows) {
        $shotId = [string]$row.shot_id
        if ([string]::IsNullOrWhiteSpace($shotId)) {
            Fail "assembly_manifest.json row missing shot_id"
        }
        if ($map.ContainsKey($shotId)) {
            Fail "duplicate assembly_manifest row for shot $shotId"
        }
        if (-not $TraceShotIds.ContainsKey($shotId)) {
            Fail "assembly_manifest row references unknown shot $shotId"
        }
        $map[$shotId] = $row
    }

    foreach ($shotId in $TraceShotIds.Keys) {
        if (-not $map.ContainsKey($shotId)) {
            Fail "assembly_manifest missing row for shot $shotId"
        }
    }

    return $map
}

function Assert-DeliveredShotManifest {
    param(
        [object]$ManifestRow,
        [string]$ShotId
    )

    if (-not $ManifestRow.assembled_clip_path) {
        Fail "delivered shot $ShotId assembly_manifest missing assembled_clip_path"
    }
    Require-File -Path ([string]$ManifestRow.assembled_clip_path) | Out-Null
    if (-not $ManifestRow.clip_path) {
        Fail "delivered shot $ShotId assembly_manifest missing clip_path"
    }
    Require-File -Path ([string]$ManifestRow.clip_path) | Out-Null

    if ($ManifestRow.subtitle_burned_in -eq $true) {
        if (-not $ManifestRow.subtitle_path) {
            Fail "delivered shot $ShotId burned subtitles but has no subtitle_path"
        }
        Require-File -Path ([string]$ManifestRow.subtitle_path) | Out-Null
    }
    elseif ($ManifestRow.subtitle_path) {
        Fail "delivered shot $ShotId has subtitle_path but subtitle_burned_in is not true"
    }

    if ($ManifestRow.audio_muxed -eq $true) {
        if (-not $ManifestRow.audio_path) {
            Fail "delivered shot $ShotId muxed audio but has no audio_path"
        }
        Require-File -Path ([string]$ManifestRow.audio_path) | Out-Null
    }
    elseif ($ManifestRow.audio_path) {
        Fail "delivered shot $ShotId has audio_path but audio_muxed is not true"
    }

    if ($ManifestRow.audio_muxed -eq $true -and $ManifestRow.silent_audio_inserted -eq $true) {
        Fail "delivered shot $ShotId cannot both mux audio and insert silent audio"
    }
}

function Test-HasNonEmptyProperty {
    param(
        [object]$Object,
        [string]$Name
    )

    if ($null -eq $Object -or $Object.PSObject.Properties.Name -notcontains $Name) {
        return $false
    }
    $value = $Object.$Name
    if ($null -eq $value) {
        return $false
    }
    return @($value).Count -gt 0
}

$ForbiddenContextKeys = @(
    "reasoning",
    "thought",
    "thoughts",
    "chain_of_thought",
    "cot",
    "conversation",
    "conversation_history",
    "messages",
    "prompt_history",
    "scratchpad"
)

function Assert-NoForbiddenContextKeys {
    param(
        [object]$Value,
        [string]$Path = '$'
    )

    if ($null -eq $Value) {
        return
    }

    if ($Value -is [System.Array]) {
        for ($i = 0; $i -lt @($Value).Count; $i++) {
            Assert-NoForbiddenContextKeys -Value @($Value)[$i] -Path "$Path.$i"
        }
        return
    }

    if ($Value -is [pscustomobject]) {
        foreach ($prop in $Value.PSObject.Properties) {
            $name = [string]$prop.Name
            if ($ForbiddenContextKeys -contains $name.Trim().ToLowerInvariant()) {
                Fail "Context Packet contains forbidden reasoning/history field at $Path.$name"
            }
            Assert-NoForbiddenContextKeys -Value $prop.Value -Path "$Path.$name"
        }
    }
}

function Assert-LocalAgentNodeContract {
    param(
        [object]$Artifact,
        [string]$Stage,
        [string]$ExpectedNodeId,
        [string]$ExpectedReasoningMode,
        [bool]$ExpectedJudgmentOrPlanning,
        [string]$ExpectedContextStage,
        [string[]]$ExpectedUpstreamArtifactIds
    )

    $passes = Get-PassingValidations -Validations @($Artifact.validations) -ArtifactId ([string]$Artifact.id) -Stage $Stage -Description "agent artifact $($Artifact.id)"
    $validation = $passes[0]
    if (-not $validation.layers -or $validation.layers.PSObject.Properties.Name -notcontains "node_contract") {
        Fail "agent artifact $($Artifact.id) validation $Stage missing node_contract evidence"
    }
    $contract = $validation.layers.node_contract
    if ([string]$contract.node_id -ne $ExpectedNodeId) {
        Fail "agent artifact $($Artifact.id) expected node_id $ExpectedNodeId, got $($contract.node_id)"
    }
    if ([string]$contract.kind -ne "agent") {
        Fail "agent artifact $($Artifact.id) expected kind agent, got $($contract.kind)"
    }
    if ([string]$contract.reasoning_mode -ne $ExpectedReasoningMode) {
        Fail "agent artifact $($Artifact.id) expected reasoning_mode $ExpectedReasoningMode, got $($contract.reasoning_mode)"
    }
    if (@($contract.tools).Count -ne 0) {
        Fail "agent artifact $($Artifact.id) node_contract tools must be empty"
    }
    if ($contract.PSObject.Properties.Name -notcontains "judgment_or_planning") {
        Fail "agent artifact $($Artifact.id) node_contract missing judgment_or_planning"
    }
    if ([bool]$contract.judgment_or_planning -ne $ExpectedJudgmentOrPlanning) {
        Fail "agent artifact $($Artifact.id) expected judgment_or_planning $ExpectedJudgmentOrPlanning, got $($contract.judgment_or_planning)"
    }
    if (-not $contract.context_packet) {
        Fail "agent artifact $($Artifact.id) node_contract missing context_packet"
    }
    $packet = $contract.context_packet
    if ([string]$packet.stage -ne $ExpectedContextStage) {
        Fail "agent artifact $($Artifact.id) expected context stage $ExpectedContextStage, got $($packet.stage)"
    }
    $actualUpstream = @($packet.upstream_artifact_ids | ForEach-Object { [string]$_ })
    if ($actualUpstream.Count -ne $ExpectedUpstreamArtifactIds.Count) {
        Fail "agent artifact $($Artifact.id) upstream_artifact_ids mismatch: expected $($ExpectedUpstreamArtifactIds -join ',') got $($actualUpstream -join ',')"
    }
    for ($i = 0; $i -lt $ExpectedUpstreamArtifactIds.Count; $i++) {
        if ($actualUpstream[$i] -ne $ExpectedUpstreamArtifactIds[$i]) {
            Fail "agent artifact $($Artifact.id) upstream_artifact_ids mismatch at index ${i}: expected $($ExpectedUpstreamArtifactIds[$i]) got $($actualUpstream[$i])"
        }
    }
    Assert-NoForbiddenContextKeys -Value $packet.params -Path '$.context_packet.params'
}

function Assert-LocalAgentArtifact {
    param(
        [object[]]$ProjectArtifacts,
        [string]$Id,
        [string]$Kind,
        [string]$Stage
    )

    $artifact = @($ProjectArtifacts | Where-Object { $_.id -eq $Id -and $_.kind -eq $Kind })
    if ($artifact.Count -ne 1) {
        Fail "local dry-run expected exactly one $Kind agent artifact $Id, found $($artifact.Count)"
    }
    Assert-PassingValidation -Validations @($artifact[0].validations) -ArtifactId $Id -Stage $Stage
    return $artifact[0]
}

$resolvedOutDir = (Resolve-Path -LiteralPath $OutDir).Path
$roughCut = Join-Path $resolvedOutDir "rough_cut.mp4"
$failedShots = Join-Path $resolvedOutDir "failed_shots.json"
$manifest = Join-Path $resolvedOutDir "assembly_manifest.json"
$tracePath = Join-Path $resolvedOutDir "trace.json"
$roundtrip = Join-Path $resolvedOutDir "seedance_tos_roundtrip.mp4"

$roughDuration = Get-FfprobeDuration -Path $roughCut
$failedJson = Read-JsonFile -Path $failedShots
$manifestJson = Read-JsonFile -Path $manifest
$trace = Read-JsonFile -Path $tracePath

if (-not $trace.project_id) {
    Fail "trace.json missing project_id"
}

$dbFiles = @(Get-ChildItem -LiteralPath $resolvedOutDir -Filter "video_agent*.sqlite" -File -ErrorAction SilentlyContinue)
if ($dbFiles.Count -eq 0) {
    Fail "no video_agent*.sqlite database found in output dir"
}

$projectArtifacts = @($trace.project_artifacts)
$projectArtifactIds = New-ArtifactIdSet -Artifacts $projectArtifacts
foreach ($artifact in $projectArtifacts) {
    $artifactId = [string]$artifact.id
    $validations = @($artifact.validations)
    if ($validations.Count -eq 0) {
        Fail "project artifact $artifactId has no validations"
    }
    $singleArtifactIds = @{}
    $singleArtifactIds[$artifactId] = $true
    Assert-ValidationRecordsIntegrity -Validations $validations -ArtifactIds $singleArtifactIds -Description "project artifact $artifactId"
}
$roughArtifacts = @($projectArtifacts | Where-Object { $_.kind -eq "rough_cut" })
if ($roughArtifacts.Count -ne 1) {
    Fail "trace must have exactly one project rough_cut artifact, found $($roughArtifacts.Count)"
}
$roughArtifact = $roughArtifacts[0]
Test-ArtifactUri -Uri ([string]$roughArtifact.tos_key) -Description "rough_cut artifact"
Test-ContentHash -Hash ([string]$roughArtifact.content_hash) -Description "rough_cut artifact"
$expectedRoughHash = Get-Sha256ContentHash -Path $roughCut
if (-not [string]::Equals(([string]$roughArtifact.content_hash), $expectedRoughHash, [StringComparison]::OrdinalIgnoreCase)) {
    Fail "rough_cut artifact content_hash must match local rough_cut sha256 $expectedRoughHash, got $($roughArtifact.content_hash)"
}
Assert-PassingValidation -Validations @($roughArtifact.validations) -ArtifactId ([string]$roughArtifact.id) -Stage "rough_cut_media_l0"
$roughPasses = Get-PassingValidations -Validations @($roughArtifact.validations) -ArtifactId ([string]$roughArtifact.id) -Stage "rough_cut_media_l0" -Description "rough_cut artifact $($roughArtifact.id)"
foreach ($validation in $roughPasses) {
    Assert-MediaL0Validation -Validation $validation -Description "rough_cut artifact $($roughArtifact.id)" -ExpectedPath $roughCut | Out-Null
}

$roughParams = $roughArtifact.params
if (-not $roughParams) {
    Fail "rough_cut artifact missing params"
}
if (-not $roughParams.assembly_manifest) {
    Fail "rough_cut params missing assembly_manifest"
}
Assert-SameFile -Actual ([string]$roughParams.assembly_manifest) -Expected $manifest -Description "rough_cut params assembly_manifest"
if (-not $roughParams.failed_shots) {
    Fail "rough_cut params missing failed_shots"
}
Assert-SameFile -Actual ([string]$roughParams.failed_shots) -Expected $failedShots -Description "rough_cut params failed_shots"
if ($roughParams.partial_delivery -ne $true) {
    Fail "rough_cut params partial_delivery must be true"
}

if (-not $manifestJson.shots) {
    Fail "assembly_manifest.json missing shots"
}

$shots = @($trace.shots)
if ($shots.Count -eq 0) {
    Fail "trace has no shot records"
}
$traceShotIds = @{}
foreach ($shot in $shots) {
    $shotId = [string]$shot.shot_id
    if ([string]::IsNullOrWhiteSpace($shotId)) {
        Fail "trace shot missing shot_id"
    }
    if ($traceShotIds.ContainsKey($shotId)) {
        Fail "duplicate trace shot $shotId"
    }
    $traceShotIds[$shotId] = $true
}
$manifestRows = @($manifestJson.shots)
$manifestShotMap = Get-ManifestShotMap -ManifestRows $manifestRows -TraceShotIds $traceShotIds
$failedRows = @($failedJson)
$failedShotMap = Get-FailedShotMap -FailedRows $failedRows
foreach ($failedShotId in $failedShotMap.Keys) {
    if (-not $traceShotIds.ContainsKey($failedShotId)) {
        Fail "failed_shots.json row references unknown shot $failedShotId"
    }
}

$roundtripHash = $null
if (Test-Path -LiteralPath $roundtrip -PathType Leaf) {
    Get-FfprobeDuration -Path $roundtrip | Out-Null
    $roundtripHash = Get-Sha256ContentHash -Path $roundtrip
}
elseif (-not $AllowLocalDryRun) {
    Fail "live output missing seedance_tos_roundtrip.mp4"
}

if ($AllowLocalDryRun) {
    $briefArtifact = Assert-LocalAgentArtifact -ProjectArtifacts $projectArtifacts -Id "artifact_agent_brief" -Kind "brief" -Stage "brief_self_check"
    Assert-LocalAgentNodeContract -Artifact $briefArtifact -Stage "brief_self_check" -ExpectedNodeId "requirements" -ExpectedReasoningMode "single_structured" -ExpectedJudgmentOrPlanning $true -ExpectedContextStage "brief" -ExpectedUpstreamArtifactIds @()
    $chaptersArtifact = Assert-LocalAgentArtifact -ProjectArtifacts $projectArtifacts -Id "artifact_agent_chapters" -Kind "chapters" -Stage "chapters_self_check"
    Assert-LocalAgentNodeContract -Artifact $chaptersArtifact -Stage "chapters_self_check" -ExpectedNodeId "script_chapters" -ExpectedReasoningMode "bounded_generate_critic" -ExpectedJudgmentOrPlanning $true -ExpectedContextStage "chapters" -ExpectedUpstreamArtifactIds @("artifact_agent_brief")
    $storyboardArtifact = Assert-LocalAgentArtifact -ProjectArtifacts $projectArtifacts -Id "artifact_agent_shots" -Kind "storyboard" -Stage "shots_self_check"
    Assert-LocalAgentNodeContract -Artifact $storyboardArtifact -Stage "shots_self_check" -ExpectedNodeId "storyboard" -ExpectedReasoningMode "single_structured" -ExpectedJudgmentOrPlanning $true -ExpectedContextStage "shots" -ExpectedUpstreamArtifactIds @("artifact_agent_chapters")
    $assetsArtifact = Assert-LocalAgentArtifact -ProjectArtifacts $projectArtifacts -Id "artifact_agent_assets" -Kind "assets" -Stage "assets_self_check"
    Assert-LocalAgentNodeContract -Artifact $assetsArtifact -Stage "assets_self_check" -ExpectedNodeId "visual_assets" -ExpectedReasoningMode "single_structured" -ExpectedJudgmentOrPlanning $false -ExpectedContextStage "assets" -ExpectedUpstreamArtifactIds @("artifact_agent_shots")

    $storyboardShots = @($storyboardArtifact.params.shots)
    if ($storyboardShots.Count -eq 0) {
        Fail "local dry-run storyboard artifact has no shots"
    }
    $heroCount = 0
    foreach ($storyShot in $storyboardShots) {
        if (-not $storyShot.shot_id) {
            Fail "local dry-run storyboard shot missing shot_id"
        }
        if ($storyShot.is_hero -eq $true) {
            $heroCount += 1
        }
        if ($null -eq $storyShot.is_hero) {
            Fail "local dry-run storyboard shot $($storyShot.shot_id) missing is_hero"
        }
        if (@("hero", "standard", "filler") -notcontains ([string]$storyShot.tier)) {
            Fail "local dry-run storyboard shot $($storyShot.shot_id) has invalid tier: $($storyShot.tier)"
        }
        if (-not $storyShot.continuity_in -or -not $storyShot.continuity_out) {
            Fail "local dry-run storyboard shot $($storyShot.shot_id) missing continuity fields"
        }
    }
    if ($heroCount -lt 1) {
        Fail "local dry-run storyboard did not mark any hero shot"
    }

    $subtitleRows = @($manifestRows | Where-Object { $_.subtitle_burned_in -eq $true -and $_.subtitle_path })
    if ($subtitleRows.Count -eq 0) {
        Fail "local dry-run manifest has no subtitle burn-in evidence"
    }
    foreach ($row in $subtitleRows) {
        if (-not (Test-Path -LiteralPath ([string]$row.subtitle_path) -PathType Leaf)) {
            Fail "subtitle path from manifest is missing: $($row.subtitle_path)"
        }
    }
    $ttsRows = @($manifestRows | Where-Object { Test-HasNonEmptyProperty -Object $_.rerun_context -Name "tts_requests" })
    if ($ttsRows.Count -eq 0) {
        Fail "local dry-run manifest has no TTS request evidence"
    }
    foreach ($row in $ttsRows) {
        if ($row.audio_muxed -ne $true) {
            Fail "TTS-request shot $($row.shot_id) did not mux a post-production audio track"
        }
        if (-not $row.audio_path) {
            Fail "TTS-request shot $($row.shot_id) missing audio_path"
        }
        if (-not (Test-Path -LiteralPath ([string]$row.audio_path) -PathType Leaf)) {
            Fail "TTS-request audio path is missing: $($row.audio_path)"
        }
    }
}

$videoArtifactCount = 0
$jobCount = 0
$jobCostTotal = 0.0
$roundtripMatchedArtifact = $false
foreach ($shot in $shots) {
    $shotId = [string]$shot.shot_id
    $isFailedShot = $failedShotMap.ContainsKey($shotId)
    $manifestRow = $manifestShotMap[$shotId]
    $shotArtifacts = @($shot.artifacts)
    $shotArtifactIds = New-ArtifactIdSet -Artifacts $shotArtifacts
    $shotValidations = @($shot.validations)
    if ($shotArtifacts.Count -gt 0 -and $shotValidations.Count -eq 0) {
        Fail "shot $shotId has artifacts but no validations"
    }
    Assert-ValidationRecordsIntegrity -Validations $shotValidations -ArtifactIds $shotArtifactIds -Description "shot $shotId"
    $jobs = @($shot.jobs)
    if ($jobs.Count -eq 0) {
        Fail "shot $shotId has no jobs"
    }
    $hasFailedTerminalJob = $false
    foreach ($job in $jobs) {
        $jobCount += 1
        $jobCostTotal += [double]$job.cost
        Assert-S12JobTraceFields -Job $job -ShotId $shotId
        Assert-ContainsNoTextOverlays -Params $job.params -Description "job $($job.id) for shot $shotId"
        if (@("failed", "submit_failed") -contains ([string]$job.status)) {
            $hasFailedTerminalJob = $true
        }
        if (-not $AllowLocalDryRun) {
            if ($job.provider -ne "ark") {
                Fail "live shot $shotId has non-ARK provider: $($job.provider)"
            }
            if (-not ([string]$job.model).Contains("seedance")) {
                Fail "live shot $shotId has non-Seedance model: $($job.model)"
            }
        }
        Assert-CompletedJobStatus -Job $job -ShotId $shotId -AllowFailed:$isFailedShot
    }
    Assert-ShotTraceFile -OutDir $resolvedOutDir -ProjectId ([string]$trace.project_id) -Shot $shot

    $videoArtifacts = @($shotArtifacts | Where-Object { $_.kind -eq "video" })
    if ($isFailedShot -and $videoArtifacts.Count -gt 0) {
        Fail "shot $shotId is listed in failed_shots.json but still has video artifact"
    }
    if ($videoArtifacts.Count -eq 0) {
        if (-not $isFailedShot) {
            Fail "shot $shotId has no video artifact"
        }
        if (-not $hasFailedTerminalJob) {
            Fail "failed shot $shotId has no terminal failed job"
        }
        Assert-FailedShotManifest -ManifestRows @($manifestJson.shots) -ShotId $shotId
        continue
    }
    Assert-DeliveredShotManifest -ManifestRow $manifestRow -ShotId $shotId
    foreach ($artifact in $videoArtifacts) {
        $videoArtifactCount += 1
        Test-ArtifactUri -Uri ([string]$artifact.tos_key) -Description "video artifact $($artifact.id)"
        Test-ContentHash -Hash ([string]$artifact.content_hash) -Description "video artifact $($artifact.id)"
        $pass = @($shotValidations | Where-Object {
            $_.artifact_id -eq $artifact.id -and $_.verdict -eq "pass"
        })
        if ($pass.Count -eq 0) {
            Fail "video artifact $($artifact.id) has no pass validation"
        }
        if ($AllowLocalDryRun) {
            $dryRunPass = @($pass | Where-Object { $_.stage -eq "dry_run_l0" })
            if ($dryRunPass.Count -eq 0) {
                Fail "local video artifact $($artifact.id) has no pass dry_run_l0 validation"
            }
            foreach ($validation in $dryRunPass) {
                $probePath = Assert-MediaL0Validation -Validation $validation -Description "local video artifact $($artifact.id)"
                Assert-LocalSizeContentHash -Hash ([string]$artifact.content_hash) -Path $probePath -Description "local video artifact $($artifact.id)"
            }
        }
        else {
            $seedancePass = @($pass | Where-Object { $_.stage -eq "seedance_media_l0" })
            if ($seedancePass.Count -eq 0) {
                Fail "live video artifact $($artifact.id) has no pass seedance_media_l0 validation"
            }
            foreach ($validation in $seedancePass) {
                $roundtripPath = [string]$validation.layers.tos_roundtrip_path
                if ([string]::IsNullOrWhiteSpace($roundtripPath)) {
                    Fail "live seedance_media_l0 validation for $($artifact.id) missing tos_roundtrip_path"
                }
                Assert-SameFile -Actual $roundtripPath -Expected $roundtrip -Description "live seedance_media_l0 tos_roundtrip_path"
                Assert-MediaL0Validation -Validation $validation -Description "live video artifact $($artifact.id)" -ExpectedPath $roundtrip | Out-Null
                if (-not [string]::Equals(([string]$artifact.content_hash), $roundtripHash, [StringComparison]::OrdinalIgnoreCase)) {
                    Fail "live video artifact $($artifact.id) content_hash must match seedance_tos_roundtrip sha256 $roundtripHash, got $($artifact.content_hash)"
                }
                $roundtripMatchedArtifact = $true
            }
        }
    }
}

Assert-BudgetTrace -Budget $trace.budget -JobCostTotal $jobCostTotal

if (-not $AllowLocalDryRun -and -not $roundtripMatchedArtifact) {
    Fail "live seedance_tos_roundtrip.mp4 content hash $roundtripHash did not match any passed video artifact"
}

Write-Host "PASS P1 rough-cut trace verification"
Write-Host "out_dir: $resolvedOutDir"
Write-Host "project_id: $($trace.project_id)"
Write-Host "rough_cut: $roughCut"
Write-Host "rough_cut_duration_s: $roughDuration"
Write-Host "trace: $tracePath"
Write-Host "db_files: $($dbFiles.Count)"
Write-Host "shots: $($shots.Count)"
Write-Host "jobs: $jobCount"
Write-Host "video_artifacts: $videoArtifactCount"
Write-Host "failed_shots_count: $(@($failedJson).Count)"
