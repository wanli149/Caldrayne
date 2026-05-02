param(
    [string]$CorpusConfig,
    [string]$BaselineDir,
    [string]$CorpusRoot,
    [string]$BaselineRoot,
    [string]$OutputRoot,
    [int]$SampleChunks = 0,
    [bool]$HeavyWorldBlockStatistics = $false,
    [int]$HeavyWorldBlockStatisticsChunkBudget = 0
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ($PSVersionTable.PSEdition -ne "Core" -or $PSVersionTable.PSVersion.Major -lt 7) {
    $pwshCommand = Get-Command pwsh -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($null -eq $pwshCommand) {
        throw "tools/worldgen-audit-verify.ps1 requires PowerShell 7+ (pwsh). Windows PowerShell 5.1 can hang and produce unstable verifier output, and no pwsh executable was found on PATH."
    }

    $pwshVersionOutput = & $pwshCommand.Source -NoProfile -Command '$PSVersionTable.PSVersion.Major'
    if ($LASTEXITCODE -ne 0) {
        throw "tools/worldgen-audit-verify.ps1 could not verify the pwsh version at '$($pwshCommand.Source)'."
    }

    try {
        $pwshVersionMajor = [int](@($pwshVersionOutput) | Select-Object -Last 1)
    } catch {
        throw "tools/worldgen-audit-verify.ps1 could not parse the pwsh version reported by '$($pwshCommand.Source)'."
    }

    if ($pwshVersionMajor -lt 7) {
        throw "tools/worldgen-audit-verify.ps1 requires PowerShell 7+ (pwsh). The pwsh executable found on PATH is version $pwshVersionMajor."
    }

    $forwardedArgs = [System.Collections.Generic.List[string]]::new()
    foreach ($entry in $PSBoundParameters.GetEnumerator()) {
        $forwardedArgs.Add("-$($entry.Key)")
        $forwardedArgs.Add([string]$entry.Value)
    }

    & $pwshCommand.Source -NoProfile -File $PSCommandPath @forwardedArgs
    exit $LASTEXITCODE
}

function ConvertTo-BoolOrNull {
    param([Parameter(Mandatory = $true)][AllowNull()]$Value)

    if ($null -eq $Value) {
        return $null
    }

    if ($Value -is [bool]) {
        return [bool]$Value
    }

    $stringValue = [string]$Value
    if ([string]::IsNullOrWhiteSpace($stringValue)) {
        return $null
    }

    return [System.Convert]::ToBoolean($stringValue)
}

function Resolve-RepoPath {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RepoRoot,
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    if ([System.IO.Path]::IsPathRooted($Path)) {
        if (Test-Path -LiteralPath $Path) {
            return (Resolve-Path -LiteralPath $Path).Path
        }

        return [System.IO.Path]::GetFullPath($Path)
    }

    $candidatePath = Join-Path $RepoRoot $Path
    if (Test-Path -LiteralPath $candidatePath) {
        return (Resolve-Path -LiteralPath $candidatePath).Path
    }

    return [System.IO.Path]::GetFullPath($candidatePath)
}

function Normalize-PreviewMetrics {
    param([Parameter(Mandatory = $true)][string]$Path)

    $json = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
    return [pscustomobject]@{
        seed = $json.seed
        gen_opts = $json.gen_opts
        recipe = $json.recipe
        world_generate_time_contract = $json.world_generate_time_contract
        world_generate_ms = $json.world_generate_ms
        dimensions_lg = $json.dimensions_lg
        chunk_dimensions = $json.chunk_dimensions
        max_height = $json.max_height
        site_markers = $json.site_markers
        possible_starting_sites = $json.possible_starting_sites
        starting_site_profile_contract = $json.starting_site_profile_contract
        starting_site_scoring_contract = $json.starting_site_scoring_contract
        starting_site_candidates = @(
            $json.starting_site_candidates | ForEach-Object {
                [pscustomobject]@{
                    rank = $_.rank
                    selected = $_.selected
                    profile = [pscustomobject]@{
                        site_id = $_.profile.site_id
                        name = $_.profile.name
                        site_kind = $_.profile.site_kind
                        center_biome = $_.profile.center_biome
                        center_chunk = $_.profile.center_chunk
                        plot_count = $_.profile.plot_count
                        biome_factor = $_.profile.biome_factor
                    }
                    score = $_.score
                }
            }
        )
        poi_markers = $json.poi_markers
        sim = $json.sim
    }
}

function Normalize-ChunkStats {
    param([Parameter(Mandatory = $true)][string]$Path)

    $json = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
    $sampleChunks = if ($null -ne $json.sample_chunks) {
        [int]$json.sample_chunks
    } else {
        @($json.sampled_chunks).Count
    }
    return [pscustomobject]@{
        seed = $json.seed
        gen_opts = $json.gen_opts
        recipe = $json.recipe
        sample_chunks = $sampleChunks
        chunk_audit_mode = $json.chunk_audit_mode
        strict_determinism = $json.strict_determinism
        sampled_chunks = @(
            $json.sampled_chunks | ForEach-Object {
                [pscustomobject]@{
                    chunk_pos = $_.chunk_pos
                    min_z = $_.min_z
                    max_z = $_.max_z
                    sub_chunks = $_.sub_chunks
                    name = $_.name
                    biome = $_.biome
                    alt = $_.alt
                    tree_density = $_.tree_density
                    contains_river = $_.contains_river
                    near_water = $_.near_water
                    temp = $_.temp
                    humidity = $_.humidity
                    rockiness = $_.rockiness
                    cliff_height = $_.cliff_height
                    block_total = $_.block_total
                    non_air_blocks = $_.non_air_blocks
                    sprite_total = $_.sprite_total
                    block_kind_counts = $_.block_kind_counts
                    sprite_kind_counts = $_.sprite_kind_counts
                }
            }
        )
    }
}

function Normalize-RuntimeMatrix {
    param([Parameter(Mandatory = $true)][string]$Path)

    $json = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
    $sampleChunks = if ($null -ne $json.sample_chunks) {
        [int]$json.sample_chunks
    } else {
        @($json.sampled_chunks).Count
    }
    return [pscustomobject]@{
        seed = $json.seed
        gen_opts = $json.gen_opts
        recipe = $json.recipe
        sample_chunks = $sampleChunks
        runtime_audit_mode = $json.runtime_audit_mode
        strict_determinism = $json.strict_determinism
        runtime_chunk_contract = $json.runtime_chunk_contract
        contexts = $json.contexts
        fixed_overlay_fixture = $json.fixed_overlay_fixture
        rtsim_resource_sampling_contract = $json.rtsim_resource_sampling_contract
        rtsim_resource_fixture = $json.rtsim_resource_fixture
        rtsim_resource_samples = @(
            $json.rtsim_resource_samples | ForEach-Object {
                [pscustomobject]@{
                    chunk_pos = $_.chunk_pos
                    selection_score = $_.selection_score
                    resource_kind_count = $_.resource_kind_count
                    baseline_night_full_density_runtime_chunk = $_.baseline_night_full_density_runtime_chunk
                    baseline_night_full_density_runtime_supplement = [pscustomobject]@{
                        entity_signature_count = $_.baseline_night_full_density_runtime_supplement.entity_signature_count
                        entity_signatures = @($_.baseline_night_full_density_runtime_supplement.entity_signatures | Sort-Object)
                        rtsim_max_resources = $_.baseline_night_full_density_runtime_supplement.rtsim_max_resources
                    }
                    baseline_night_full_density_rtsim_resource_block_counts = $_.baseline_night_full_density_rtsim_resource_block_counts
                    baseline_night_thinned_runtime_chunk = $_.baseline_night_thinned_runtime_chunk
                    baseline_night_thinned_runtime_supplement = [pscustomobject]@{
                        entity_signature_count = $_.baseline_night_thinned_runtime_supplement.entity_signature_count
                        entity_signatures = @($_.baseline_night_thinned_runtime_supplement.entity_signatures | Sort-Object)
                        rtsim_max_resources = $_.baseline_night_thinned_runtime_supplement.rtsim_max_resources
                    }
                    baseline_night_thinned_rtsim_resource_block_counts = $_.baseline_night_thinned_rtsim_resource_block_counts
                }
            }
        )
        sampled_chunks = @(
            $json.sampled_chunks | ForEach-Object {
                [pscustomobject]@{
                    chunk_pos = $_.chunk_pos
                    base_runtime_chunk = $_.base_runtime_chunk
                    base_runtime_supplement = [pscustomobject]@{
                        entity_signature_count = $_.base_runtime_supplement.entity_signature_count
                        entity_signatures = @($_.base_runtime_supplement.entity_signatures | Sort-Object)
                    }
                    empty_overlay_runtime_chunk = $_.empty_overlay_runtime_chunk
                    fixed_overlay_runtime_chunk = $_.fixed_overlay_runtime_chunk
                    baseline_night_runtime_chunk = $_.baseline_night_runtime_chunk
                    baseline_night_runtime_supplement = [pscustomobject]@{
                        entity_signature_count = $_.baseline_night_runtime_supplement.entity_signature_count
                        entity_signatures = @($_.baseline_night_runtime_supplement.entity_signatures | Sort-Object)
                    }
                    halloween_night_runtime_chunk = $_.halloween_night_runtime_chunk
                    halloween_night_runtime_supplement = [pscustomobject]@{
                        entity_signature_count = $_.halloween_night_runtime_supplement.entity_signature_count
                        entity_signatures = @($_.halloween_night_runtime_supplement.entity_signatures | Sort-Object)
                    }
                }
            }
        )
    }
}

function Normalize-WildlifeRuntimeMatrix {
    param([Parameter(Mandatory = $true)][string]$Path)

    $json = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
    $sampleChunks = if ($null -ne $json.sample_chunks) {
        [int]$json.sample_chunks
    } else {
        @($json.sampled_chunks).Count
    }
    return [pscustomobject]@{
        seed = $json.seed
        gen_opts = $json.gen_opts
        recipe = $json.recipe
        sample_chunks = $sampleChunks
        runtime_audit_mode = $json.runtime_audit_mode
        strict_determinism = $json.strict_determinism
        runtime_chunk_contract = $json.runtime_chunk_contract
        contexts = $json.contexts
        aquatic_fauna_sampling_contract = $json.aquatic_fauna_sampling_contract
        sampling_contract = $json.sampling_contract
        aquatic_fauna_samples = @(
            $json.aquatic_fauna_samples | ForEach-Object {
                [pscustomobject]@{
                    chunk_pos = $_.chunk_pos
                    selection_bucket = $_.selection_bucket
                    biome = $_.biome
                    alt = $_.alt
                    water_alt = $_.water_alt
                    near_water = $_.near_water
                    aquatic_spawn_potential = $_.aquatic_spawn_potential
                    aquatic_fauna = $_.aquatic_fauna
                }
            }
        )
        sampled_chunks = @(
            $json.sampled_chunks | ForEach-Object {
                [pscustomobject]@{
                    chunk_pos = $_.chunk_pos
                    selection_bucket = $_.selection_bucket
                    selection_score = $_.selection_score
                    biome = $_.biome
                    alt = $_.alt
                    temp = $_.temp
                    humidity = $_.humidity
                    tree_density = $_.tree_density
                    contains_river = $_.contains_river
                    near_water = $_.near_water
                    aquatic_spawn_potential = $_.aquatic_spawn_potential
                    baseline_night = [pscustomobject]@{
                        variant_mode = $_.baseline_night.variant_mode
                        expected_spawn_score = $_.baseline_night.expected_spawn_score
                        entity_signature_count = $_.baseline_night.entity_signature_count
                        entity_signatures = $_.baseline_night.entity_signatures
                    }
                    halloween_night = [pscustomobject]@{
                        variant_mode = $_.halloween_night.variant_mode
                        expected_spawn_score = $_.halloween_night.expected_spawn_score
                        entity_signature_count = $_.halloween_night.entity_signature_count
                        entity_signatures = $_.halloween_night.entity_signatures
                    }
                }
            }
        )
    }
}

function Normalize-WorldBlockStatistics {
    param([Parameter(Mandatory = $true)][string]$Path)

    $json = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
    return [pscustomobject]@{
        schema_version = $json.schema_version
        contract = $json.contract
        comparability = $json.comparability
        input_contract = $json.input_contract
        runtime_chunk_entry = $json.runtime_chunk_entry
        selection_contract = $json.selection_contract
        strict_determinism = [bool]$json.strict_determinism
        requested_chunk_budget = [int]$json.requested_chunk_budget
        sampled_chunk_count = [int]$json.sampled_chunk_count
        map_size_chunks = $json.map_size_chunks
        world_recipe_hash = $json.world_recipe_hash
        chunk_recipe_hash = $json.chunk_recipe_hash
        topology_id = $json.topology_id
        chunk_pass_version = $json.chunk_pass_version
        aggregate = [pscustomobject]@{
            block_total = $json.aggregate.block_total
            non_air_blocks = $json.aggregate.non_air_blocks
            sprite_total = $json.aggregate.sprite_total
            block_kind_counts = $json.aggregate.block_kind_counts
            sprite_kind_counts = $json.aggregate.sprite_kind_counts
        }
        sampled_chunks = @(
            $json.sampled_chunks | ForEach-Object {
                [pscustomobject]@{
                    selection_rank = $_.selection_rank
                    chunk_pos = $_.chunk_pos
                    min_z = $_.min_z
                    max_z = $_.max_z
                    height = $_.height
                    block_total = $_.block_total
                    non_air_blocks = $_.non_air_blocks
                    sprite_total = $_.sprite_total
                    block_kind_counts = $_.block_kind_counts
                    sprite_kind_counts = $_.sprite_kind_counts
                }
            }
        )
    }
}

function Write-NormalizedJson {
    param(
        [Parameter(Mandatory = $true)]
        [object]$Value,
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    $json = $Value | ConvertTo-Json -Depth 100
    Set-Content -LiteralPath $Path -Value $json -NoNewline
}

function Read-JsonFile {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $true)]
        [string]$Context
    )

    if (-not (Test-Path -LiteralPath $Path)) {
        throw "$Context does not exist: $Path"
    }

    $json = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
    if ($null -eq $json) {
        throw "$Context is empty or could not be parsed: $Path"
    }

    return $json
}

function Get-ValueKind {
    param([object]$Value)

    if ($null -eq $Value) {
        return "null"
    }

    if (
        $Value -is [System.Management.Automation.PSCustomObject] -or
        $Value -is [System.Collections.IDictionary]
    ) {
        return "object"
    }

    if ($Value -is [System.Collections.IEnumerable] -and -not ($Value -is [string])) {
        return "array"
    }

    return "primitive"
}

function Get-ObjectPropertyNames {
    param(
        [Parameter(Mandatory = $true)]
        [object]$Value
    )

    if ($Value -is [System.Collections.IDictionary]) {
        return @($Value.Keys | ForEach-Object { [string]$_ })
    }

    return @($Value.PSObject.Properties | ForEach-Object { $_.Name })
}

function Test-ObjectPropertyExists {
    param(
        [Parameter(Mandatory = $true)]
        [object]$Value,
        [Parameter(Mandatory = $true)]
        [string]$Name
    )

    if ($Value -is [System.Collections.IDictionary]) {
        return @($Value.Keys | ForEach-Object { [string]$_ }) -contains $Name
    }

    return @($Value.PSObject.Properties | ForEach-Object { $_.Name }) -contains $Name
}

function Get-ObjectPropertyValue {
    param(
        [Parameter(Mandatory = $true)]
        [object]$Value,
        [Parameter(Mandatory = $true)]
        [string]$Name
    )

    if ($Value -is [System.Collections.IDictionary]) {
        return $Value[$Name]
    }

    return $Value.$Name
}

function Get-RequiredObjectPropertyValue {
    param(
        [Parameter(Mandatory = $true)]
        [object]$Value,
        [Parameter(Mandatory = $true)]
        [string]$Name,
        [Parameter(Mandatory = $true)]
        [string]$Context
    )

    if (-not (Test-ObjectPropertyExists -Value $Value -Name $Name)) {
        throw "Missing required property '$Name' in $Context"
    }

    return (Get-ObjectPropertyValue -Value $Value -Name $Name)
}

function Get-OptionalObjectPropertyValue {
    param(
        [Parameter(Mandatory = $true)]
        [object]$Value,
        [Parameter(Mandatory = $true)]
        [string]$Name
    )

    if (-not (Test-ObjectPropertyExists -Value $Value -Name $Name)) {
        return $null
    }

    return (Get-ObjectPropertyValue -Value $Value -Name $Name)
}

function Get-RequiredStringPropertyValue {
    param(
        [Parameter(Mandatory = $true)]
        [object]$Value,
        [Parameter(Mandatory = $true)]
        [string]$Name,
        [Parameter(Mandatory = $true)]
        [string]$Context
    )

    $propertyValue = Get-RequiredObjectPropertyValue -Value $Value -Name $Name -Context $Context
    if ($null -eq $propertyValue) {
        throw "Property '$Name' in $Context must not be null"
    }

    $stringValue = [string]$propertyValue
    if ([string]::IsNullOrWhiteSpace($stringValue)) {
        throw "Property '$Name' in $Context must not be empty"
    }

    return $stringValue
}

function Get-OptionalStringPropertyValue {
    param(
        [Parameter(Mandatory = $true)]
        [object]$Value,
        [Parameter(Mandatory = $true)]
        [string]$Name
    )

    $propertyValue = Get-OptionalObjectPropertyValue -Value $Value -Name $Name
    if ($null -eq $propertyValue) {
        return $null
    }

    $stringValue = [string]$propertyValue
    if ([string]::IsNullOrWhiteSpace($stringValue)) {
        return $null
    }

    return $stringValue
}

function Resolve-RunOwnedContractPath {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RunDirPath,
        [Parameter(Mandatory = $true)]
        [string]$ContractPath,
        [Parameter(Mandatory = $true)]
        [string]$Context
    )

    if ([string]::IsNullOrWhiteSpace($ContractPath)) {
        throw "$Context path must not be empty"
    }

    $resolvedRunDirPath = [System.IO.Path]::GetFullPath($RunDirPath)
    $resolvedContractPath = if ([System.IO.Path]::IsPathRooted($ContractPath)) {
        [System.IO.Path]::GetFullPath($ContractPath)
    } else {
        [System.IO.Path]::GetFullPath((Join-Path $resolvedRunDirPath $ContractPath))
    }

    $directorySeparator = [System.IO.Path]::DirectorySeparatorChar
    $altDirectorySeparator = [System.IO.Path]::AltDirectorySeparatorChar
    $runRootPrefix = if (
        $resolvedRunDirPath.EndsWith($directorySeparator) -or
        $resolvedRunDirPath.EndsWith($altDirectorySeparator)
    ) {
        $resolvedRunDirPath
    } else {
        $resolvedRunDirPath + $directorySeparator
    }

    if (
        $resolvedContractPath -cne $resolvedRunDirPath -and
        -not $resolvedContractPath.StartsWith($runRootPrefix, [System.StringComparison]::OrdinalIgnoreCase)
    ) {
        throw "$Context path '$resolvedContractPath' escapes run directory '$resolvedRunDirPath'"
    }

    return $resolvedContractPath
}

function Get-ArtifactContractScope {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ArtifactPath
    )

    $normalizedPath = Normalize-PathForSummary -Path $ArtifactPath
    $segments = @($normalizedPath -split "/" | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    if ($segments.Count -eq 0) {
        return $null
    }

    return $segments[0]
}

function Convert-VolatileFieldToMatcher {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ArtifactPath,
        [Parameter(Mandatory = $true)]
        [string]$Field
    )

    $artifactScope = Get-ArtifactContractScope -ArtifactPath $ArtifactPath
    $normalizedField = Normalize-PathForSummary -Path $Field
    if (
        [string]::IsNullOrWhiteSpace($artifactScope) -or
        [string]::IsNullOrWhiteSpace($normalizedField)
    ) {
        return [pscustomobject]@{
            success = $false
            field = $normalizedField
            artifact_scope = $artifactScope
            diff_path_pattern = $null
            reason = "empty_scope_or_field"
        }
    }

    $segments = @($normalizedField -split "/" | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    if ($segments.Count -lt 2) {
        return [pscustomobject]@{
            success = $false
            field = $normalizedField
            artifact_scope = $artifactScope
            diff_path_pattern = $null
            reason = "missing_scope_or_path"
        }
    }

    if ($segments[0] -cne $artifactScope) {
        return [pscustomobject]@{
            success = $false
            field = $normalizedField
            artifact_scope = $artifactScope
            diff_path_pattern = $null
            reason = "artifact_scope_mismatch"
        }
    }

    $diffPathPattern = '^\$'
    foreach ($segment in $segments[1..($segments.Count - 1)]) {
        if ($segment -cmatch '^(?<name>[^\[\]]+)\[\*\]$') {
            $arrayName = $Matches["name"]
            if ([string]::IsNullOrWhiteSpace($arrayName)) {
                return [pscustomobject]@{
                    success = $false
                    field = $normalizedField
                    artifact_scope = $artifactScope
                    diff_path_pattern = $null
                    reason = "empty_array_segment"
                }
            }

            $diffPathPattern += '\.' + [System.Text.RegularExpressions.Regex]::Escape($arrayName)
            if ($arrayName -ceq "sampled_chunks" -or $arrayName -ceq "rtsim_resource_samples") {
                $diffPathPattern += '\[(?:\d+|chunk_pos=-?\d+,-?\d+)\]'
            } else {
                $diffPathPattern += '\[\d+\]'
            }
            continue
        }

        if ($segment.Contains("[") -or $segment.Contains("]")) {
            return [pscustomobject]@{
                success = $false
                field = $normalizedField
                artifact_scope = $artifactScope
                diff_path_pattern = $null
                reason = "unsupported_array_selector"
            }
        }

        $diffPathPattern += '\.' + [System.Text.RegularExpressions.Regex]::Escape($segment)
    }

    return [pscustomobject]@{
        success = $true
        field = $normalizedField
        artifact_scope = $artifactScope
        diff_path_pattern = $diffPathPattern + '$'
        reason = $null
    }
}

function Test-VolatileDiffPath {
    param(
        [Parameter(Mandatory = $true)]
        [object]$Matcher,
        [Parameter(Mandatory = $true)]
        [string]$DiffPath
    )

    $pattern = Get-OptionalStringPropertyValue -Value $Matcher -Name "diff_path_pattern"
    if ([string]::IsNullOrWhiteSpace($pattern)) {
        return $false
    }

    return [System.Text.RegularExpressions.Regex]::IsMatch($DiffPath, $pattern)
}

function Resolve-VolatileFieldContracts {
    param(
        [Parameter(Mandatory = $true)]
        [System.Collections.IDictionary]$ArtifactContractPaths,
        [object[]]$DeclaredVolatileFields
    )

    $artifactMatchers = @{}
    $artifactDeclaredFields = @{}
    foreach ($artifactName in $ArtifactContractPaths.Keys) {
        $artifactMatchers[$artifactName] = [System.Collections.ArrayList]::new()
        $artifactDeclaredFields[$artifactName] = [System.Collections.ArrayList]::new()
    }

    $recognizedFields = [System.Collections.ArrayList]::new()
    $unrecognizedFields = [System.Collections.ArrayList]::new()
    foreach ($declaredFieldValue in @($DeclaredVolatileFields)) {
        $declaredField = if ($null -eq $declaredFieldValue) {
            ""
        } else {
            Normalize-PathForSummary -Path ([string]$declaredFieldValue)
        }

        if ([string]::IsNullOrWhiteSpace($declaredField)) {
            [void]$unrecognizedFields.Add($declaredField)
            continue
        }

        $matchedArtifact = $false
        foreach ($artifactName in $ArtifactContractPaths.Keys) {
            $matcher = Convert-VolatileFieldToMatcher `
                -ArtifactPath ([string]$ArtifactContractPaths[$artifactName]) `
                -Field $declaredField
            if (-not $matcher.success) {
                continue
            }

            [void]$artifactMatchers[$artifactName].Add($matcher)
            [void]$artifactDeclaredFields[$artifactName].Add($declaredField)
            $matchedArtifact = $true
        }

        if ($matchedArtifact) {
            [void]$recognizedFields.Add($declaredField)
        } else {
            [void]$unrecognizedFields.Add($declaredField)
        }
    }

    return [pscustomobject]@{
        recognized_fields = @($recognizedFields | Sort-Object -Unique)
        unrecognized_fields = @($unrecognizedFields | Sort-Object -Unique)
        artifact_matchers = $artifactMatchers
        artifact_declared_fields = [pscustomobject]@{
            preview_metrics = @($artifactDeclaredFields["preview_metrics"] | Sort-Object -Unique)
            chunk_stats = @($artifactDeclaredFields["chunk_stats"] | Sort-Object -Unique)
            runtime_matrix = @($artifactDeclaredFields["runtime_matrix"] | Sort-Object -Unique)
            wildlife_runtime_matrix = @($artifactDeclaredFields["wildlife_runtime_matrix"] | Sort-Object -Unique)
            warnings = @($artifactDeclaredFields["warnings"] | Sort-Object -Unique)
        }
    }
}

function Remove-VolatileDifferences {
    param(
        [AllowEmptyCollection()]
        [object[]]$Differences,
        [object[]]$VolatileFieldMatchers
    )

    if ($null -eq $VolatileFieldMatchers -or @($VolatileFieldMatchers).Count -eq 0) {
        return ,([object[]]@($Differences))
    }

    $filteredDifferences = [System.Collections.ArrayList]::new()
    foreach ($difference in @($Differences)) {
        $differencePath = Get-OptionalStringPropertyValue -Value $difference -Name "path"
        if ([string]::IsNullOrWhiteSpace($differencePath)) {
            [void]$filteredDifferences.Add($difference)
            continue
        }

        $isVolatileDifference = $false
        foreach ($matcher in @($VolatileFieldMatchers)) {
            if (Test-VolatileDiffPath -Matcher $matcher -DiffPath $differencePath) {
                $isVolatileDifference = $true
                break
            }
        }

        if (-not $isVolatileDifference) {
            [void]$filteredDifferences.Add($difference)
        }
    }

    return ,([object[]]@($filteredDifferences))
}

function Resolve-ArtifactComparisonExecution {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ArtifactName,
        [AllowNull()]
        [string]$Comparability
    )

    if ([string]::IsNullOrWhiteSpace($Comparability)) {
        return [pscustomobject]@{
            artifact = $ArtifactName
            comparability = $null
            execution_status = "not_comparable"
            gates_overall_status = $false
            execution_reason = "comparability_not_declared"
        }
    }

    switch ($Comparability) {
        "comparable_for_future_diff" {
            return [pscustomobject]@{
                artifact = $ArtifactName
                comparability = $Comparability
                execution_status = "compared"
                gates_overall_status = $true
                execution_reason = $null
            }
        }
        "static_chunk_strict_comparable" {
            return [pscustomobject]@{
                artifact = $ArtifactName
                comparability = $Comparability
                execution_status = "compared"
                gates_overall_status = $true
                execution_reason = $null
            }
        }
        "runtime_chunk_strict_comparable" {
            return [pscustomobject]@{
                artifact = $ArtifactName
                comparability = $Comparability
                execution_status = "compared"
                gates_overall_status = $true
                execution_reason = $null
            }
        }
        "bounded_runtime_chunk_block_statistics_non_gating" {
            return [pscustomobject]@{
                artifact = $ArtifactName
                comparability = $Comparability
                execution_status = "compared"
                gates_overall_status = $false
                execution_reason = $null
            }
        }
        "sample_based_non_strict" {
            return [pscustomobject]@{
                artifact = $ArtifactName
                comparability = $Comparability
                execution_status = "not_comparable"
                gates_overall_status = $false
                execution_reason = "sample_based_non_strict"
            }
        }
        default {
            throw "Unsupported $ArtifactName comparability '$Comparability'"
        }
    }
}

function Convert-ToExecutedArtifactDiff {
    param(
        [Parameter(Mandatory = $true)]
        [object]$RawDiff,
        [Parameter(Mandatory = $true)]
        [object]$Execution
    )

    $rawSchemaVersion = Get-RequiredStringPropertyValue `
        -Value $RawDiff `
        -Name "schema_version" `
        -Context "artifact diff"
    $executedSchemaVersion = switch ($rawSchemaVersion) {
        "worldgen_compare_artifact_diff_v1" { "worldgen_compare_artifact_diff_v2" }
        "worldgen_compare_text_diff_v1" { "worldgen_compare_text_diff_v2" }
        default { $rawSchemaVersion }
    }

    $rawStatus = Get-RequiredStringPropertyValue -Value $RawDiff -Name "status" -Context "artifact diff"
    $status = if ($Execution.execution_status -ceq "skipped") {
        "skipped"
    } else {
        $rawStatus
    }
    $match = if ($Execution.gates_overall_status) {
        switch ($status) {
            "match" { $true }
            "mismatch" { $false }
            "baseline_missing" { $false }
            "skipped" { $null }
            default { $null }
        }
    } else {
        $null
    }
    $differenceCount = if ($status -ceq "skipped") {
        0
    } else {
        [int]$RawDiff.difference_count
    }
    $differences = if ($status -ceq "skipped") {
        ,([object[]]@())
    } else {
        ,([object[]]@($RawDiff.differences))
    }

    $executedDiff = [ordered]@{
        schema_version = $executedSchemaVersion
        artifact = $RawDiff.artifact
        status = $status
        execution_status = $Execution.execution_status
        gates_overall_status = [bool]$Execution.gates_overall_status
        comparability = $Execution.comparability
        match = $match
        actual = $RawDiff.actual
        baseline = $RawDiff.baseline
        difference_count = $differenceCount
        differences = $differences
    }
    if (-not [string]::IsNullOrWhiteSpace($Execution.execution_reason)) {
        $executedDiff.execution_reason = $Execution.execution_reason
    }

    return [pscustomobject]$executedDiff
}

function Resolve-OverallComparisonStatus {
    param(
        [Parameter(Mandatory = $true)]
        [object[]]$ArtifactDiffs
    )

    $gatingArtifactDiffs = @($ArtifactDiffs | Where-Object { [bool]$_.gates_overall_status })
    if ($gatingArtifactDiffs.Count -eq 0) {
        return "skipped"
    }

    if (@($gatingArtifactDiffs | Where-Object { $_.status -ceq "baseline_missing" }).Count -gt 0) {
        return "baseline_missing"
    }

    if (@($gatingArtifactDiffs | Where-Object { $_.status -ceq "mismatch" }).Count -gt 0) {
        return "mismatch"
    }

    if (@($gatingArtifactDiffs | Where-Object { $_.status -ceq "skipped" }).Count -gt 0) {
        return "skipped"
    }

    return "match"
}

function Add-DiffRecord {
    param(
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [System.Collections.ArrayList]$Diffs,
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $true)]
        [string]$Kind,
        [object]$BaselineValue,
        [object]$ActualValue,
        [string]$BaselineKind,
        [string]$ActualKind,
        [string]$Detail
    )

    $record = [ordered]@{
        path = $Path
        kind = $Kind
    }

    if ($PSBoundParameters.ContainsKey("BaselineKind") -and -not [string]::IsNullOrWhiteSpace($BaselineKind)) {
        $record.baseline_kind = $BaselineKind
    }

    if ($PSBoundParameters.ContainsKey("ActualKind") -and -not [string]::IsNullOrWhiteSpace($ActualKind)) {
        $record.actual_kind = $ActualKind
    }

    if ($PSBoundParameters.ContainsKey("BaselineValue")) {
        $record.baseline = $BaselineValue
    }

    if ($PSBoundParameters.ContainsKey("ActualValue")) {
        $record.actual = $ActualValue
    }

    if ($PSBoundParameters.ContainsKey("Detail") -and -not [string]::IsNullOrWhiteSpace($Detail)) {
        $record.detail = $Detail
    }

    [void]$Diffs.Add([pscustomobject]$record)
}

function Test-IsChunkPositionKeyedArrayPath {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    return $Path -ceq "$.sampled_chunks" -or $Path -ceq "$.rtsim_resource_samples"
}

function Try-GetChunkPositionKey {
    param(
        [Parameter(Mandatory = $true)]
        [object]$Value
    )

    if ((Get-ValueKind $Value) -ne "object") {
        return $null
    }

    if (-not (Test-ObjectPropertyExists -Value $Value -Name "chunk_pos")) {
        return $null
    }

    $chunkPos = @(Get-ObjectPropertyValue -Value $Value -Name "chunk_pos")
    if ($chunkPos.Count -ne 2) {
        return $null
    }

    try {
        $x = [int]$chunkPos[0]
        $y = [int]$chunkPos[1]
    } catch {
        return $null
    }

    return "{0},{1}" -f $x, $y
}

function Convert-ToChunkPositionKeyedMap {
    param(
        [Parameter(Mandatory = $true)]
        [object[]]$Items
    )

    $keyedItems = @{}
    $duplicateKeys = [System.Collections.ArrayList]::new()
    foreach ($item in $Items) {
        $chunkKey = Try-GetChunkPositionKey -Value $item
        if ($null -eq $chunkKey) {
            return [pscustomobject]@{
                success = $false
                keyed_items = @{}
                duplicate_keys = @()
            }
        }

        if ($keyedItems.ContainsKey($chunkKey)) {
            [void]$duplicateKeys.Add($chunkKey)
            continue
        }

        $keyedItems[$chunkKey] = $item
    }

    return [pscustomobject]@{
        success = $true
        keyed_items = $keyedItems
        duplicate_keys = @($duplicateKeys | Sort-Object -Unique)
    }
}

function Compare-NormalizedValue {
    param(
        [Parameter(Mandatory = $true)]
        [AllowNull()]
        [object]$Actual,
        [Parameter(Mandatory = $true)]
        [AllowNull()]
        [object]$Baseline,
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [System.Collections.ArrayList]$Diffs
    )

    $actualKind = Get-ValueKind $Actual
    $baselineKind = Get-ValueKind $Baseline

    if ($actualKind -ne $baselineKind) {
        Add-DiffRecord -Diffs $Diffs -Path $Path -Kind "type_mismatch" -BaselineKind $baselineKind -ActualKind $actualKind
        return
    }

    switch ($actualKind) {
        "null" {
            return
        }
        "primitive" {
            $actualComparable = $Actual | ConvertTo-Json -Depth 20 -Compress
            $baselineComparable = $Baseline | ConvertTo-Json -Depth 20 -Compress
            if ($actualComparable -cne $baselineComparable) {
                Add-DiffRecord -Diffs $Diffs -Path $Path -Kind "value_mismatch" -BaselineValue $Baseline -ActualValue $Actual
            }
            return
        }
        "array" {
            $actualItems = @($Actual)
            $baselineItems = @($Baseline)
            if (Test-IsChunkPositionKeyedArrayPath -Path $Path) {
                $actualChunkMap = Convert-ToChunkPositionKeyedMap -Items $actualItems
                $baselineChunkMap = Convert-ToChunkPositionKeyedMap -Items $baselineItems

                if ($actualChunkMap.success -and $baselineChunkMap.success) {
                    foreach ($duplicateKey in $actualChunkMap.duplicate_keys) {
                        Add-DiffRecord `
                            -Diffs $Diffs `
                            -Path ("{0}[chunk_pos={1}]" -f $Path, $duplicateKey) `
                            -Kind "duplicate_key_in_actual" `
                            -ActualValue $duplicateKey `
                            -Detail "sampled_chunks must have unique chunk_pos keys"
                    }

                    foreach ($duplicateKey in $baselineChunkMap.duplicate_keys) {
                        Add-DiffRecord `
                            -Diffs $Diffs `
                            -Path ("{0}[chunk_pos={1}]" -f $Path, $duplicateKey) `
                            -Kind "duplicate_key_in_baseline" `
                            -BaselineValue $duplicateKey `
                            -Detail "sampled_chunks must have unique chunk_pos keys"
                    }

                    $allChunkKeys = @(
                        @($actualChunkMap.keyed_items.Keys + $baselineChunkMap.keyed_items.Keys) |
                            Sort-Object -Unique
                    )
                    foreach ($chunkKey in $allChunkKeys) {
                        $itemPath = "{0}[chunk_pos={1}]" -f $Path, $chunkKey
                        $actualHasChunk = $actualChunkMap.keyed_items.ContainsKey($chunkKey)
                        $baselineHasChunk = $baselineChunkMap.keyed_items.ContainsKey($chunkKey)

                        if (-not $actualHasChunk) {
                            Add-DiffRecord `
                                -Diffs $Diffs `
                                -Path $itemPath `
                                -Kind "missing_in_actual" `
                                -BaselineValue $baselineChunkMap.keyed_items[$chunkKey]
                            continue
                        }

                        if (-not $baselineHasChunk) {
                            Add-DiffRecord `
                                -Diffs $Diffs `
                                -Path $itemPath `
                                -Kind "missing_in_baseline" `
                                -ActualValue $actualChunkMap.keyed_items[$chunkKey]
                            continue
                        }

                        Compare-NormalizedValue `
                            -Actual $actualChunkMap.keyed_items[$chunkKey] `
                            -Baseline $baselineChunkMap.keyed_items[$chunkKey] `
                            -Path $itemPath `
                            -Diffs $Diffs
                    }
                    return
                }
            }

            if ($actualItems.Count -ne $baselineItems.Count) {
                Add-DiffRecord `
                    -Diffs $Diffs `
                    -Path $Path `
                    -Kind "array_length_mismatch" `
                    -BaselineValue $baselineItems.Count `
                    -ActualValue $actualItems.Count `
                    -Detail "baseline_count/actual_count"
            }

            $maxItemCount = [Math]::Max($actualItems.Count, $baselineItems.Count)
            for ($index = 0; $index -lt $maxItemCount; $index++) {
                $itemPath = "{0}[{1}]" -f $Path, $index
                if ($index -ge $actualItems.Count) {
                    Add-DiffRecord -Diffs $Diffs -Path $itemPath -Kind "missing_in_actual" -BaselineValue $baselineItems[$index]
                    continue
                }

                if ($index -ge $baselineItems.Count) {
                    Add-DiffRecord -Diffs $Diffs -Path $itemPath -Kind "missing_in_baseline" -ActualValue $actualItems[$index]
                    continue
                }

                Compare-NormalizedValue -Actual $actualItems[$index] -Baseline $baselineItems[$index] -Path $itemPath -Diffs $Diffs
            }
            return
        }
        "object" {
            $actualPropertyNames = @(Get-ObjectPropertyNames -Value $Actual)
            $baselinePropertyNames = @(Get-ObjectPropertyNames -Value $Baseline)
            $allPropertyNames = @((@($actualPropertyNames + $baselinePropertyNames) | Sort-Object -Unique))

            foreach ($propertyName in $allPropertyNames) {
                $propertyPath = "{0}.{1}" -f $Path, $propertyName
                $actualHasProperty = Test-ObjectPropertyExists -Value $Actual -Name $propertyName
                $baselineHasProperty = Test-ObjectPropertyExists -Value $Baseline -Name $propertyName

                if (-not $actualHasProperty) {
                    Add-DiffRecord `
                        -Diffs $Diffs `
                        -Path $propertyPath `
                        -Kind "missing_in_actual" `
                        -BaselineValue (Get-ObjectPropertyValue -Value $Baseline -Name $propertyName)
                    continue
                }

                if (-not $baselineHasProperty) {
                    Add-DiffRecord `
                        -Diffs $Diffs `
                        -Path $propertyPath `
                        -Kind "missing_in_baseline" `
                        -ActualValue (Get-ObjectPropertyValue -Value $Actual -Name $propertyName)
                    continue
                }

                Compare-NormalizedValue `
                    -Actual (Get-ObjectPropertyValue -Value $Actual -Name $propertyName) `
                    -Baseline (Get-ObjectPropertyValue -Value $Baseline -Name $propertyName) `
                    -Path $propertyPath `
                    -Diffs $Diffs
            }
            return
        }
        default {
            throw "Unsupported diff value kind '$actualKind' at path '$Path'"
        }
    }
}

function Build-StructuredArtifactDiff {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ArtifactName,
        [Parameter(Mandatory = $true)]
        [string]$ActualPath,
        [Parameter(Mandatory = $true)]
        [string]$BaselinePath,
        [object[]]$VolatileFieldMatchers
    )

    if (-not (Test-Path -LiteralPath $BaselinePath)) {
        return [pscustomobject]@{
            schema_version = "worldgen_compare_artifact_diff_v1"
            artifact = $ArtifactName
            status = "baseline_missing"
            match = $false
            actual = $ActualPath
            baseline = $BaselinePath
            difference_count = 0
            differences = [object[]]@()
        }
    }

    $actual = Get-Content -LiteralPath $ActualPath -Raw | ConvertFrom-Json
    $baseline = Get-Content -LiteralPath $BaselinePath -Raw | ConvertFrom-Json
    $differences = [System.Collections.ArrayList]::new()
    Compare-NormalizedValue -Actual $actual -Baseline $baseline -Path '$' -Diffs $differences
    $filteredDifferences = Remove-VolatileDifferences `
        -Differences @($differences) `
        -VolatileFieldMatchers $VolatileFieldMatchers

    return [pscustomobject]@{
        schema_version = "worldgen_compare_artifact_diff_v1"
        artifact = $ArtifactName
        status = if ($filteredDifferences.Count -eq 0) { "match" } else { "mismatch" }
        match = $filteredDifferences.Count -eq 0
        actual = $ActualPath
        baseline = $BaselinePath
        difference_count = $filteredDifferences.Count
        differences = [object[]]@($filteredDifferences)
    }
}

function Build-StructuredArtifactDiffAllowingMissingActual {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ArtifactName,
        [Parameter(Mandatory = $true)]
        [string]$ActualPath,
        [Parameter(Mandatory = $true)]
        [string]$BaselinePath,
        [object[]]$VolatileFieldMatchers
    )

    if (-not (Test-Path -LiteralPath $ActualPath)) {
        return [pscustomobject]@{
            schema_version = "worldgen_compare_artifact_diff_v1"
            artifact = $ArtifactName
            status = "actual_missing"
            match = $false
            actual = $ActualPath
            baseline = $BaselinePath
            difference_count = 0
            differences = [object[]]@()
        }
    }

    return Build-StructuredArtifactDiff `
        -ArtifactName $ArtifactName `
        -ActualPath $ActualPath `
        -BaselinePath $BaselinePath `
        -VolatileFieldMatchers $VolatileFieldMatchers
}

function Build-TextArtifactDiff {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ArtifactName,
        [Parameter(Mandatory = $true)]
        [string]$ActualPath,
        [Parameter(Mandatory = $true)]
        [string]$BaselinePath
    )

    if (-not (Test-Path -LiteralPath $BaselinePath)) {
        return [pscustomobject]@{
            schema_version = "worldgen_compare_text_diff_v1"
            artifact = $ArtifactName
            status = "baseline_missing"
            match = $false
            actual = $ActualPath
            baseline = $BaselinePath
            difference_count = 0
            differences = [object[]]@()
        }
    }

    $actualLines = @(
        if ((Get-Item -LiteralPath $ActualPath).Length -eq 0) { @() } else { Get-Content -LiteralPath $ActualPath }
    )
    $baselineLines = @(
        if ((Get-Item -LiteralPath $BaselinePath).Length -eq 0) { @() } else { Get-Content -LiteralPath $BaselinePath }
    )

    $differences = [System.Collections.ArrayList]::new()
    $maxLineCount = [Math]::Max($actualLines.Count, $baselineLines.Count)
    for ($index = 0; $index -lt $maxLineCount; $index++) {
        $linePath = "line[{0}]" -f ($index + 1)
        if ($index -ge $actualLines.Count) {
            Add-DiffRecord -Diffs $differences -Path $linePath -Kind "missing_in_actual" -BaselineValue $baselineLines[$index]
            continue
        }

        if ($index -ge $baselineLines.Count) {
            Add-DiffRecord -Diffs $differences -Path $linePath -Kind "missing_in_baseline" -ActualValue $actualLines[$index]
            continue
        }

        if ($actualLines[$index] -cne $baselineLines[$index]) {
            Add-DiffRecord -Diffs $differences -Path $linePath -Kind "line_mismatch" -BaselineValue $baselineLines[$index] -ActualValue $actualLines[$index]
        }
    }

    return [pscustomobject]@{
        schema_version = "worldgen_compare_text_diff_v1"
        artifact = $ArtifactName
        status = if ($differences.Count -eq 0) { "match" } else { "mismatch" }
        match = $differences.Count -eq 0
        actual = $ActualPath
        baseline = $BaselinePath
        difference_count = $differences.Count
        differences = [object[]]@($differences)
    }
}

function Get-RelativeChildPath {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RootPath,
        [Parameter(Mandatory = $true)]
        [string]$ChildPath
    )

    $resolvedRootPath = Resolve-RepoPath -RepoRoot $RootPath -Path "."
    $resolvedChildPath = Resolve-RepoPath -RepoRoot $resolvedRootPath -Path $ChildPath
    $directorySeparator = [System.IO.Path]::DirectorySeparatorChar
    $altDirectorySeparator = [System.IO.Path]::AltDirectorySeparatorChar
    if (
        $resolvedRootPath.EndsWith($directorySeparator) -or
        $resolvedRootPath.EndsWith($altDirectorySeparator)
    ) {
        $rootPrefix = $resolvedRootPath
    } else {
        $rootPrefix = $resolvedRootPath + $directorySeparator
    }

    if (-not $resolvedChildPath.StartsWith($rootPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Path '$resolvedChildPath' is not inside root '$resolvedRootPath'"
    }

    return $resolvedChildPath.Substring($rootPrefix.Length)
}

function Normalize-PathForSummary {
    param([Parameter(Mandatory = $true)][string]$Path)

    return ($Path -replace "\\", "/")
}

function Get-CaseTier {
    param(
        [Parameter(Mandatory = $true)]
        [string]$CorpusRootPath,
        [Parameter(Mandatory = $true)]
        [string]$RelativeCasePath
    )

    $segments = @($RelativeCasePath -split "[\\/]+" | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    if ($segments.Count -gt 1) {
        return $segments[0].ToLowerInvariant()
    }

    return (Split-Path -Path $CorpusRootPath -Leaf).ToLowerInvariant()
}

function Resolve-CaseSampleChunks {
    param(
        [Parameter(Mandatory = $true)]
        [int]$ExplicitSampleChunks
    )

    if ($ExplicitSampleChunks -gt 0) {
        return $ExplicitSampleChunks
    }

    return 0
}

function Resolve-HeavyWorldBlockStatisticsPrecheck {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RunDirPath,
        [Parameter(Mandatory = $true)]
        [string]$BaselineDirPath,
        [Parameter(Mandatory = $true)]
        [object]$HeavyContract
    )

    $lane = Get-RequiredStringPropertyValue `
        -Value $HeavyContract `
        -Name "lane" `
        -Context "compare status heavy_opt_in"
    $executionMode = Get-RequiredStringPropertyValue `
        -Value $HeavyContract `
        -Name "execution_mode" `
        -Context "compare status heavy_opt_in"
    $inputParityContract = Get-RequiredStringPropertyValue `
        -Value $HeavyContract `
        -Name "input_parity_contract" `
        -Context "compare status heavy_opt_in"
    $comparability = Get-RequiredStringPropertyValue `
        -Value $HeavyContract `
        -Name "comparability" `
        -Context "compare status heavy_opt_in"
    $worldBinContractPath = Get-RequiredStringPropertyValue `
        -Value $HeavyContract `
        -Name "world_bin" `
        -Context "compare status heavy_opt_in"
    $recipeSidecarContractPath = Get-RequiredStringPropertyValue `
        -Value $HeavyContract `
        -Name "recipe_sidecar" `
        -Context "compare status heavy_opt_in"
    $precheckArtifactContractPath = Get-RequiredStringPropertyValue `
        -Value $HeavyContract `
        -Name "precheck_artifact" `
        -Context "compare status heavy_opt_in"
    $outputArtifactContractPath = Get-RequiredStringPropertyValue `
        -Value $HeavyContract `
        -Name "output_artifact" `
        -Context "compare status heavy_opt_in"
    $baselineArtifactContractPath = Get-RequiredStringPropertyValue `
        -Value $HeavyContract `
        -Name "baseline_artifact" `
        -Context "compare status heavy_opt_in"
    $requestedChunkBudgetRaw = Get-RequiredObjectPropertyValue `
        -Value $HeavyContract `
        -Name "requested_chunk_budget" `
        -Context "compare status heavy_opt_in"
    try {
        $requestedChunkBudget = [int]$requestedChunkBudgetRaw
    } catch {
        throw "Property 'requested_chunk_budget' in compare status heavy_opt_in must be an integer"
    }

    $contractNotesValue = Get-OptionalObjectPropertyValue -Value $HeavyContract -Name "notes"
    $contractNotes = if ($null -eq $contractNotesValue) {
        @()
    } else {
        @($contractNotesValue) | ForEach-Object { [string]$_ }
    }

    $resolvedWorldBinPath = Resolve-RunOwnedContractPath `
        -RunDirPath $RunDirPath `
        -ContractPath $worldBinContractPath `
        -Context "world_block_statistics heavy world.bin"
    $resolvedRecipeSidecarPath = Resolve-RunOwnedContractPath `
        -RunDirPath $RunDirPath `
        -ContractPath $recipeSidecarContractPath `
        -Context "world_block_statistics heavy recipe sidecar"
    $resolvedPrecheckArtifactPath = Resolve-RunOwnedContractPath `
        -RunDirPath $RunDirPath `
        -ContractPath $precheckArtifactContractPath `
        -Context "world_block_statistics heavy precheck artifact"
    $resolvedOutputArtifactPath = Resolve-RunOwnedContractPath `
        -RunDirPath $RunDirPath `
        -ContractPath $outputArtifactContractPath `
        -Context "world_block_statistics heavy output artifact"
    $resolvedBaselineArtifactPath = [System.IO.Path]::GetFullPath(
        (Join-Path $BaselineDirPath $baselineArtifactContractPath)
    )
    $worldBinExists = Test-Path -LiteralPath $resolvedWorldBinPath
    $recipeSidecarExists = Test-Path -LiteralPath $resolvedRecipeSidecarPath
    $precheckArtifactExists = Test-Path -LiteralPath $resolvedPrecheckArtifactPath
    $outputArtifactExists = Test-Path -LiteralPath $resolvedOutputArtifactPath
    $baselineExists = Test-Path -LiteralPath $resolvedBaselineArtifactPath
    $precheckStatus = $null
    if ($precheckArtifactExists) {
        $precheckStatus = Read-JsonFile `
            -Path $resolvedPrecheckArtifactPath `
            -Context "world_block_statistics heavy precheck artifact"
    }
    $strictReloadStatus = if ($null -eq $precheckStatus) {
        $null
    } else {
        Get-OptionalStringPropertyValue -Value $precheckStatus -Name "strict_reload_status"
    }
    $compatEntry = if ($null -eq $precheckStatus) {
        $null
    } else {
        Get-OptionalStringPropertyValue -Value $precheckStatus -Name "compat_entry"
    }
    $compatDecision = if ($null -eq $precheckStatus) {
        $null
    } else {
        Get-OptionalStringPropertyValue -Value $precheckStatus -Name "compat_decision"
    }
    $compatFailure = if ($null -eq $precheckStatus) {
        $null
    } else {
        Get-OptionalStringPropertyValue -Value $precheckStatus -Name "compat_failure"
    }
    $precheckDetail = if ($null -eq $precheckStatus) {
        $null
    } else {
        Get-OptionalStringPropertyValue -Value $precheckStatus -Name "detail"
    }

    $summaryNotes = [System.Collections.Generic.List[string]]::new()
    foreach ($note in $contractNotes) {
        $summaryNotes.Add($note)
    }
    if (-not $worldBinExists) {
        $summaryNotes.Add(
            "precheck stopped before any heavy execution because the run-owned world.bin contract path is missing"
        )
    } elseif (-not $recipeSidecarExists) {
        $summaryNotes.Add(
            "precheck stopped before any heavy execution because the run-owned recipe sidecar contract path is missing"
        )
    } elseif (-not $precheckArtifactExists) {
        $summaryNotes.Add(
            "precheck output is missing even though the heavy lane was requested; audit did not record strict reload parity"
        )
    } elseif ($strictReloadStatus -cne "passed") {
        $summaryNotes.Add(
            "the emitted heavy precheck artifact did not report a passed strict reload parity status"
        )
    } elseif (-not $outputArtifactExists) {
        $summaryNotes.Add(
            "strict reload parity passed, but the bounded heavy/world_block_statistics.normalized.json artifact was not emitted"
        )
    } elseif (-not $baselineExists) {
        $summaryNotes.Add(
            "bounded heavy compare surface was emitted, but the heavy baseline artifact is missing"
        )
    } else {
        $summaryNotes.Add(
            "saved-world parity, bounded output emission, and baseline presence all passed; the heavy diff remains non-gating in this tranche"
        )
    }
    if (-not [string]::IsNullOrWhiteSpace($precheckDetail)) {
        $summaryNotes.Add($precheckDetail)
    }

    return [pscustomobject]@{
        requested = $true
        lane = $lane
        execution_mode = $executionMode
        input_parity_contract = $inputParityContract
        comparability = $comparability
        requested_chunk_budget = $requestedChunkBudget
        world_bin = $resolvedWorldBinPath
        world_bin_exists = $worldBinExists
        recipe_sidecar = $resolvedRecipeSidecarPath
        recipe_sidecar_exists = $recipeSidecarExists
        precheck_artifact = $resolvedPrecheckArtifactPath
        precheck_artifact_exists = $precheckArtifactExists
        strict_reload_status = $strictReloadStatus
        compat_entry = $compatEntry
        compat_decision = $compatDecision
        compat_failure = $compatFailure
        output_artifact = $resolvedOutputArtifactPath
        output_artifact_exists = $outputArtifactExists
        baseline_artifact = $resolvedBaselineArtifactPath
        baseline_exists = $baselineExists
        gates_overall_status = $false
        status = if (-not $worldBinExists) {
            "world_bin_missing"
        } elseif (-not $recipeSidecarExists) {
            "recipe_sidecar_missing"
        } elseif (-not $precheckArtifactExists) {
            "precheck_missing"
        } elseif ($strictReloadStatus -cne "passed") {
            "precheck_failed"
        } elseif (-not $outputArtifactExists) {
            "output_artifact_missing"
        } elseif (-not $baselineExists) {
            "baseline_missing"
        } else {
            "precheck_passed"
        }
        notes = @($summaryNotes)
    }
}

function Invoke-AuditVerifyCase {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RepoRoot,
        [Parameter(Mandatory = $true)]
        [string]$CorpusConfig,
        [Parameter(Mandatory = $true)]
        [string]$BaselineDir,
        [Parameter(Mandatory = $true)]
        [string]$OutputRootPath,
        [Parameter(Mandatory = $true)]
        [int]$CaseSampleChunks,
        [Parameter(Mandatory = $true)]
        [bool]$EnableHeavyWorldBlockStatistics,
        [Parameter(Mandatory = $true)]
        [int]$HeavyWorldBlockStatisticsChunkBudget
    )

    $corpusConfigPath = Resolve-RepoPath -RepoRoot $RepoRoot -Path $CorpusConfig
    if (-not (Test-Path -LiteralPath $corpusConfigPath)) {
        throw "Corpus config does not exist: $corpusConfigPath"
    }

    $baselineDirPath = Resolve-RepoPath -RepoRoot $RepoRoot -Path $BaselineDir

    $existingRuns = @{}
    if (Test-Path -LiteralPath $OutputRootPath) {
        Get-ChildItem -LiteralPath $OutputRootPath -Directory | ForEach-Object {
            $existingRuns[$_.Name] = $true
        }
    } else {
        New-Item -ItemType Directory -Path $OutputRootPath -Force | Out-Null
    }

    Push-Location $RepoRoot
    try {
        $cargoArgs = @(
            "run",
            "-p",
            "veldr-world",
            "--features",
            "cli",
            "--example",
            "batch_generate",
            "--",
            "--no-progress",
            "audit",
            $corpusConfigPath,
            "--output-path",
            $OutputRootPath
        )
        if ($CaseSampleChunks -gt 0) {
            $cargoArgs += @("--sample-chunks", $CaseSampleChunks)
        }
        if ($EnableHeavyWorldBlockStatistics) {
            $cargoArgs += @("--heavy-world-block-statistics")
            if ($HeavyWorldBlockStatisticsChunkBudget -gt 0) {
                $cargoArgs += @(
                    "--heavy-world-block-statistics-chunk-budget",
                    $HeavyWorldBlockStatisticsChunkBudget
                )
            }
        }
        & cargo @cargoArgs | Out-Host
        if ($LASTEXITCODE -ne 0) {
            throw "cargo run failed with exit code $LASTEXITCODE for corpus config '$corpusConfigPath'"
        }
    } finally {
        Pop-Location
    }

    $runDir = Get-ChildItem -LiteralPath $OutputRootPath -Directory |
        Where-Object { -not $existingRuns.ContainsKey($_.Name) } |
        Sort-Object Name -Descending |
        Select-Object -First 1

    if ($null -eq $runDir) {
        $runDir = Get-ChildItem -LiteralPath $OutputRootPath -Directory |
            Sort-Object Name -Descending |
            Select-Object -First 1
    }

    if ($null -eq $runDir) {
        throw "No audit run directory was produced under $OutputRootPath"
    }

    $compareStatusPath = Resolve-RunOwnedContractPath `
        -RunDirPath $runDir.FullName `
        -ContractPath "compare/status.json" `
        -Context "compare status"
    $compareStatus = Read-JsonFile -Path $compareStatusPath -Context "Compare status"
    $compareStatusSchemaVersion = Get-RequiredStringPropertyValue `
        -Value $compareStatus `
        -Name "schema_version" `
        -Context "compare status"
    if ($compareStatusSchemaVersion -cne "worldgen_compare_status_v2") {
        throw "Unsupported compare status schema version '$compareStatusSchemaVersion' in $compareStatusPath"
    }
    $compareMode = Get-RequiredStringPropertyValue `
        -Value $compareStatus `
        -Name "compare_mode" `
        -Context "compare status"
    if ($compareMode -cne "single_run_only_v1") {
        throw "Unsupported compare mode '$compareMode' in $compareStatusPath"
    }
    $compareDiffDir = Resolve-RunOwnedContractPath `
        -RunDirPath $runDir.FullName `
        -ContractPath (Get-RequiredStringPropertyValue -Value $compareStatus -Name "diff_dir" -Context "compare status") `
        -Context "compare diff directory"
    $compareArtifacts = Get-RequiredObjectPropertyValue `
        -Value $compareStatus `
        -Name "artifacts" `
        -Context "compare status"
    $compareComparability = Get-OptionalObjectPropertyValue -Value $compareStatus -Name "comparability"
    $compareComparabilityObject = if ($null -eq $compareComparability) {
        [pscustomobject]@{}
    } else {
        $compareComparability
    }
    $volatileFieldsValue = Get-OptionalObjectPropertyValue -Value $compareStatus -Name "volatile_fields"
    $compareVolatileFields = if ($null -eq $volatileFieldsValue) {
        @()
    } else {
        @($volatileFieldsValue) | ForEach-Object { [string]$_ }
    }
    $deferredHeavyArtifactsValue = Get-OptionalObjectPropertyValue `
        -Value $compareStatus `
        -Name "deferred_heavy_artifacts"
    $deferredHeavyArtifacts = if ($null -eq $deferredHeavyArtifactsValue) {
        @()
    } else {
        @($deferredHeavyArtifactsValue) | ForEach-Object { [string]$_ }
    }
    $heavyOptInContract = Get-OptionalObjectPropertyValue -Value $compareStatus -Name "heavy_opt_in"
    if ($EnableHeavyWorldBlockStatistics -and $null -eq $heavyOptInContract) {
        throw "Heavy world_block_statistics opt-in was requested, but compare/status.json did not declare a heavy_opt_in contract."
    }
    $heavyWorldBlockStatisticsSummary = if ($null -eq $heavyOptInContract) {
        $null
    } else {
        Resolve-HeavyWorldBlockStatisticsPrecheck `
            -RunDirPath $runDir.FullName `
            -BaselineDirPath $baselineDirPath `
            -HeavyContract $heavyOptInContract
    }

    New-Item -ItemType Directory -Path $compareDiffDir -Force | Out-Null

    $previewArtifactContractPath = Get-RequiredStringPropertyValue `
        -Value $compareArtifacts `
        -Name "preview_metrics" `
        -Context "compare status artifacts"
    $chunkArtifactContractPath = Get-RequiredStringPropertyValue `
        -Value $compareArtifacts `
        -Name "chunk_stats" `
        -Context "compare status artifacts"
    $runtimeMatrixArtifactContractPath = Get-RequiredStringPropertyValue `
        -Value $compareArtifacts `
        -Name "runtime_matrix" `
        -Context "compare status artifacts"
    $wildlifeRuntimeMatrixArtifactContractPath = Get-RequiredStringPropertyValue `
        -Value $compareArtifacts `
        -Name "wildlife_runtime_matrix" `
        -Context "compare status artifacts"
    $warningsArtifactContractPath = Get-RequiredStringPropertyValue `
        -Value $compareArtifacts `
        -Name "warnings" `
        -Context "compare status artifacts"
    $volatileFieldContracts = Resolve-VolatileFieldContracts `
        -ArtifactContractPaths ([ordered]@{
            preview_metrics = $previewArtifactContractPath
            chunk_stats = $chunkArtifactContractPath
            runtime_matrix = $runtimeMatrixArtifactContractPath
            wildlife_runtime_matrix = $wildlifeRuntimeMatrixArtifactContractPath
            warnings = $warningsArtifactContractPath
        }) `
        -DeclaredVolatileFields $compareVolatileFields
    $recognizedVolatileFields = @($volatileFieldContracts.recognized_fields)
    $unrecognizedVolatileFields = @($volatileFieldContracts.unrecognized_fields)
    $previewDeclaredVolatileFields = @($volatileFieldContracts.artifact_declared_fields.preview_metrics)
    $chunkDeclaredVolatileFields = @($volatileFieldContracts.artifact_declared_fields.chunk_stats)
    $runtimeMatrixDeclaredVolatileFields = @($volatileFieldContracts.artifact_declared_fields.runtime_matrix)
    $wildlifeRuntimeMatrixDeclaredVolatileFields = @($volatileFieldContracts.artifact_declared_fields.wildlife_runtime_matrix)
    $warningsDeclaredVolatileFields = @($volatileFieldContracts.artifact_declared_fields.warnings)

    $previewArtifactSourcePath = Resolve-RunOwnedContractPath `
        -RunDirPath $runDir.FullName `
        -ContractPath $previewArtifactContractPath `
        -Context "preview_metrics artifact"
    $chunkArtifactSourcePath = Resolve-RunOwnedContractPath `
        -RunDirPath $runDir.FullName `
        -ContractPath $chunkArtifactContractPath `
        -Context "chunk_stats artifact"
    $runtimeMatrixArtifactSourcePath = Resolve-RunOwnedContractPath `
        -RunDirPath $runDir.FullName `
        -ContractPath $runtimeMatrixArtifactContractPath `
        -Context "runtime_matrix artifact"
    $wildlifeRuntimeMatrixArtifactSourcePath = Resolve-RunOwnedContractPath `
        -RunDirPath $runDir.FullName `
        -ContractPath $wildlifeRuntimeMatrixArtifactContractPath `
        -Context "wildlife_runtime_matrix artifact"
    $warningsArtifactSourcePath = Resolve-RunOwnedContractPath `
        -RunDirPath $runDir.FullName `
        -ContractPath $warningsArtifactContractPath `
        -Context "warnings artifact"

    $actualPreviewPath = Join-Path $compareDiffDir "preview_metrics.normalized.json"
    $actualChunkPath = Join-Path $compareDiffDir "chunk_stats.normalized.json"
    $actualRuntimeMatrixPath = Join-Path $compareDiffDir "runtime_matrix.normalized.json"
    $actualWildlifeRuntimeMatrixPath = Join-Path $compareDiffDir "wildlife_runtime_matrix.normalized.json"
    $actualWarningsPath = Join-Path $compareDiffDir "warnings.txt"
    $previewDiffPath = Join-Path $compareDiffDir "preview_metrics.diff.json"
    $chunkDiffPath = Join-Path $compareDiffDir "chunk_stats.diff.json"
    $runtimeMatrixDiffPath = Join-Path $compareDiffDir "runtime_matrix.diff.json"
    $wildlifeRuntimeMatrixDiffPath = Join-Path $compareDiffDir "wildlife_runtime_matrix.diff.json"
    $warningsDiffPath = Join-Path $compareDiffDir "warnings.diff.json"
    $summaryPath = Join-Path $compareDiffDir "summary.json"

    $normalizedPreviewMetrics = Normalize-PreviewMetrics -Path $previewArtifactSourcePath
    $normalizedChunkStats = Normalize-ChunkStats -Path $chunkArtifactSourcePath
    $normalizedRuntimeMatrix = Normalize-RuntimeMatrix -Path $runtimeMatrixArtifactSourcePath
    $normalizedWildlifeRuntimeMatrix = Normalize-WildlifeRuntimeMatrix -Path $wildlifeRuntimeMatrixArtifactSourcePath
    Write-NormalizedJson -Value $normalizedPreviewMetrics -Path $actualPreviewPath
    Write-NormalizedJson -Value $normalizedChunkStats -Path $actualChunkPath
    Write-NormalizedJson -Value $normalizedRuntimeMatrix -Path $actualRuntimeMatrixPath
    Write-NormalizedJson -Value $normalizedWildlifeRuntimeMatrix -Path $actualWildlifeRuntimeMatrixPath
    Copy-Item -LiteralPath $warningsArtifactSourcePath -Destination $actualWarningsPath -Force

    $baselinePreviewPath = Join-Path $baselineDirPath "preview_metrics.normalized.json"
    $baselineChunkPath = Join-Path $baselineDirPath "chunk_stats.normalized.json"
    $baselineRuntimeMatrixPath = Join-Path $baselineDirPath "runtime_matrix.normalized.json"
    $baselineWildlifeRuntimeMatrixPath = Join-Path $baselineDirPath "wildlife_runtime_matrix.normalized.json"
    $baselineWarningsPath = Join-Path $baselineDirPath "warnings.txt"

    $previewComparability = Get-OptionalStringPropertyValue `
        -Value $compareComparabilityObject `
        -Name "preview_metrics"
    $chunkComparability = Get-OptionalStringPropertyValue `
        -Value $compareComparabilityObject `
        -Name "chunk_stats"
    $runtimeMatrixComparability = Get-OptionalStringPropertyValue `
        -Value $compareComparabilityObject `
        -Name "runtime_matrix"
    $wildlifeRuntimeMatrixComparability = Get-OptionalStringPropertyValue `
        -Value $compareComparabilityObject `
        -Name "wildlife_runtime_matrix"
    $warningsComparability = Get-OptionalStringPropertyValue `
        -Value $compareComparabilityObject `
        -Name "warnings"
    $previewExecution = Resolve-ArtifactComparisonExecution `
        -ArtifactName "preview_metrics" `
        -Comparability $previewComparability
    $chunkExecution = Resolve-ArtifactComparisonExecution `
        -ArtifactName "chunk_stats" `
        -Comparability $chunkComparability
    $runtimeMatrixExecution = Resolve-ArtifactComparisonExecution `
        -ArtifactName "runtime_matrix" `
        -Comparability $runtimeMatrixComparability
    $wildlifeRuntimeMatrixExecution = Resolve-ArtifactComparisonExecution `
        -ArtifactName "wildlife_runtime_matrix" `
        -Comparability $wildlifeRuntimeMatrixComparability
    $warningsExecution = Resolve-ArtifactComparisonExecution `
        -ArtifactName "warnings" `
        -Comparability $warningsComparability

    $previewRawDiff = Build-StructuredArtifactDiff `
        -ArtifactName "preview_metrics" `
        -ActualPath $actualPreviewPath `
        -BaselinePath $baselinePreviewPath `
        -VolatileFieldMatchers @($volatileFieldContracts.artifact_matchers["preview_metrics"])
    $chunkRawDiff = Build-StructuredArtifactDiff `
        -ArtifactName "chunk_stats" `
        -ActualPath $actualChunkPath `
        -BaselinePath $baselineChunkPath `
        -VolatileFieldMatchers @($volatileFieldContracts.artifact_matchers["chunk_stats"])
    $runtimeMatrixRawDiff = Build-StructuredArtifactDiff `
        -ArtifactName "runtime_matrix" `
        -ActualPath $actualRuntimeMatrixPath `
        -BaselinePath $baselineRuntimeMatrixPath `
        -VolatileFieldMatchers @($volatileFieldContracts.artifact_matchers["runtime_matrix"])
    $wildlifeRuntimeMatrixRawDiff = Build-StructuredArtifactDiff `
        -ArtifactName "wildlife_runtime_matrix" `
        -ActualPath $actualWildlifeRuntimeMatrixPath `
        -BaselinePath $baselineWildlifeRuntimeMatrixPath `
        -VolatileFieldMatchers @($volatileFieldContracts.artifact_matchers["wildlife_runtime_matrix"])
    $warningsRawDiff = Build-TextArtifactDiff `
        -ArtifactName "warnings" `
        -ActualPath $actualWarningsPath `
        -BaselinePath $baselineWarningsPath
    $previewDiff = Convert-ToExecutedArtifactDiff -RawDiff $previewRawDiff -Execution $previewExecution
    $chunkDiff = Convert-ToExecutedArtifactDiff -RawDiff $chunkRawDiff -Execution $chunkExecution
    $runtimeMatrixDiff = Convert-ToExecutedArtifactDiff -RawDiff $runtimeMatrixRawDiff -Execution $runtimeMatrixExecution
    $wildlifeRuntimeMatrixDiff = Convert-ToExecutedArtifactDiff -RawDiff $wildlifeRuntimeMatrixRawDiff -Execution $wildlifeRuntimeMatrixExecution
    $warningsDiff = Convert-ToExecutedArtifactDiff -RawDiff $warningsRawDiff -Execution $warningsExecution
    $artifactDiffs = @($previewDiff, $chunkDiff, $runtimeMatrixDiff, $wildlifeRuntimeMatrixDiff, $warningsDiff)
    if ($null -ne $heavyWorldBlockStatisticsSummary) {
        $actualHeavyWorldBlockStatisticsPath = Join-Path $compareDiffDir "world_block_statistics.normalized.json"
        $heavyWorldBlockStatisticsDiffPath = Join-Path $compareDiffDir "world_block_statistics.diff.json"
        if ([bool]$heavyWorldBlockStatisticsSummary.output_artifact_exists) {
            $normalizedHeavyWorldBlockStatistics = Normalize-WorldBlockStatistics `
                -Path $heavyWorldBlockStatisticsSummary.output_artifact
            Write-NormalizedJson `
                -Value $normalizedHeavyWorldBlockStatistics `
                -Path $actualHeavyWorldBlockStatisticsPath
        }
        $heavyWorldBlockStatisticsExecution = Resolve-ArtifactComparisonExecution `
            -ArtifactName "world_block_statistics" `
            -Comparability ([string]$heavyWorldBlockStatisticsSummary.comparability)
        $heavyWorldBlockStatisticsRawDiff = Build-StructuredArtifactDiffAllowingMissingActual `
            -ArtifactName "world_block_statistics" `
            -ActualPath $actualHeavyWorldBlockStatisticsPath `
            -BaselinePath ([string]$heavyWorldBlockStatisticsSummary.baseline_artifact) `
            -VolatileFieldMatchers @()
        $heavyWorldBlockStatisticsDiff = Convert-ToExecutedArtifactDiff `
            -RawDiff $heavyWorldBlockStatisticsRawDiff `
            -Execution $heavyWorldBlockStatisticsExecution
        Write-NormalizedJson `
            -Value $heavyWorldBlockStatisticsDiff `
            -Path $heavyWorldBlockStatisticsDiffPath
        $heavyWorldBlockStatisticsSummary | Add-Member `
            -NotePropertyName actual `
            -NotePropertyValue $actualHeavyWorldBlockStatisticsPath `
            -Force
        $heavyWorldBlockStatisticsSummary | Add-Member `
            -NotePropertyName diff `
            -NotePropertyValue $heavyWorldBlockStatisticsDiffPath `
            -Force
        $heavyWorldBlockStatisticsSummary | Add-Member `
            -NotePropertyName execution_status `
            -NotePropertyValue $heavyWorldBlockStatisticsDiff.execution_status `
            -Force
        $heavyWorldBlockStatisticsSummary | Add-Member `
            -NotePropertyName diff_status `
            -NotePropertyValue $heavyWorldBlockStatisticsDiff.status `
            -Force
        $heavyWorldBlockStatisticsSummary | Add-Member `
            -NotePropertyName difference_count `
            -NotePropertyValue ([int]$heavyWorldBlockStatisticsDiff.difference_count) `
            -Force
        $heavyWorldBlockStatisticsSummary | Add-Member `
            -NotePropertyName match `
            -NotePropertyValue $heavyWorldBlockStatisticsDiff.match `
            -Force
        if ([string]$heavyWorldBlockStatisticsSummary.status -ceq "precheck_passed") {
            $heavyWorldBlockStatisticsSummary.status = [string]$heavyWorldBlockStatisticsDiff.status
        }
    }

    $previewMatch = $previewDiff.match
    $chunkMatch = $chunkDiff.match
    $runtimeMatrixMatch = $runtimeMatrixDiff.match
    $wildlifeRuntimeMatrixMatch = $wildlifeRuntimeMatrixDiff.match
    $warningsMatch = $warningsDiff.match

    Write-NormalizedJson -Value $previewDiff -Path $previewDiffPath
    Write-NormalizedJson -Value $chunkDiff -Path $chunkDiffPath
    Write-NormalizedJson -Value $runtimeMatrixDiff -Path $runtimeMatrixDiffPath
    Write-NormalizedJson -Value $wildlifeRuntimeMatrixDiff -Path $wildlifeRuntimeMatrixDiffPath
    Write-NormalizedJson -Value $warningsDiff -Path $warningsDiffPath

    $resolvedCaseSampleChunks = [int]$normalizedChunkStats.sample_chunks
    $missingBaselineFiles = @(
        @($artifactDiffs) |
            Where-Object { $_.status -ceq "baseline_missing" } |
            ForEach-Object { [string]$_.baseline }
    )
    $gatingMissingBaselineFiles = @(
        @($artifactDiffs) |
            Where-Object { [bool]$_.gates_overall_status -and $_.status -ceq "baseline_missing" } |
            ForEach-Object { [string]$_.baseline }
    )
    $overallStatus = Resolve-OverallComparisonStatus -ArtifactDiffs $artifactDiffs

    $summary = [pscustomobject]@{
        schema_version = "worldgen_compare_diff_summary_v6"
        diff_contract = "pairwise_artifact_diff_v1"
        corpus_config = $CorpusConfig
        baseline_dir = $BaselineDir
        run_dir = $runDir.FullName
        sample_chunks = $resolvedCaseSampleChunks
        diff_generated = $true
        status_contract = [pscustomobject]@{
            status_path = $compareStatusPath
            status_schema_version = $compareStatusSchemaVersion
            compare_mode = $compareMode
            diff_dir = $compareDiffDir
            volatile_fields = $compareVolatileFields
            deferred_heavy_artifacts = $deferredHeavyArtifacts
            recognized_volatile_fields = $recognizedVolatileFields
            unrecognized_volatile_fields = $unrecognizedVolatileFields
        }
        heavy_lanes = if ($null -eq $heavyWorldBlockStatisticsSummary) {
            [pscustomobject]@{}
        } else {
            [pscustomobject]@{
                world_block_statistics = $heavyWorldBlockStatisticsSummary
            }
        }
        baseline_missing = $gatingMissingBaselineFiles.Count -gt 0
        missing_baseline_files = $missingBaselineFiles
        gating_missing_baseline_files = $gatingMissingBaselineFiles
        compared_artifacts = [pscustomobject]@{
            preview_metrics = [pscustomobject]@{
                source = $previewArtifactSourcePath
                actual = $actualPreviewPath
                baseline = $baselinePreviewPath
                comparability = $previewComparability
                execution_status = $previewDiff.execution_status
                gates_overall_status = $previewDiff.gates_overall_status
                declared_volatile_fields = $previewDeclaredVolatileFields
                match = $previewMatch
                diff = $previewDiffPath
                diff_status = $previewDiff.status
                difference_count = $previewDiff.difference_count
            }
            chunk_stats = [pscustomobject]@{
                source = $chunkArtifactSourcePath
                actual = $actualChunkPath
                baseline = $baselineChunkPath
                comparability = $chunkComparability
                execution_status = $chunkDiff.execution_status
                gates_overall_status = $chunkDiff.gates_overall_status
                declared_volatile_fields = $chunkDeclaredVolatileFields
                match = $chunkMatch
                diff = $chunkDiffPath
                diff_status = $chunkDiff.status
                difference_count = $chunkDiff.difference_count
            }
            runtime_matrix = [pscustomobject]@{
                source = $runtimeMatrixArtifactSourcePath
                actual = $actualRuntimeMatrixPath
                baseline = $baselineRuntimeMatrixPath
                comparability = $runtimeMatrixComparability
                execution_status = $runtimeMatrixDiff.execution_status
                gates_overall_status = $runtimeMatrixDiff.gates_overall_status
                declared_volatile_fields = $runtimeMatrixDeclaredVolatileFields
                match = $runtimeMatrixMatch
                diff = $runtimeMatrixDiffPath
                diff_status = $runtimeMatrixDiff.status
                difference_count = $runtimeMatrixDiff.difference_count
            }
            wildlife_runtime_matrix = [pscustomobject]@{
                source = $wildlifeRuntimeMatrixArtifactSourcePath
                actual = $actualWildlifeRuntimeMatrixPath
                baseline = $baselineWildlifeRuntimeMatrixPath
                comparability = $wildlifeRuntimeMatrixComparability
                execution_status = $wildlifeRuntimeMatrixDiff.execution_status
                gates_overall_status = $wildlifeRuntimeMatrixDiff.gates_overall_status
                declared_volatile_fields = $wildlifeRuntimeMatrixDeclaredVolatileFields
                match = $wildlifeRuntimeMatrixMatch
                diff = $wildlifeRuntimeMatrixDiffPath
                diff_status = $wildlifeRuntimeMatrixDiff.status
                difference_count = $wildlifeRuntimeMatrixDiff.difference_count
            }
            warnings = [pscustomobject]@{
                source = $warningsArtifactSourcePath
                actual = $actualWarningsPath
                baseline = $baselineWarningsPath
                comparability = $warningsComparability
                execution_status = $warningsDiff.execution_status
                gates_overall_status = $warningsDiff.gates_overall_status
                declared_volatile_fields = $warningsDeclaredVolatileFields
                match = $warningsMatch
                diff = $warningsDiffPath
                diff_status = $warningsDiff.status
                difference_count = $warningsDiff.difference_count
            }
        }
        overall_status = $overallStatus
    }

    Write-NormalizedJson -Value $summary -Path $summaryPath

    return [pscustomobject]@{
        schema_version = $summary.schema_version
        corpus_config = $summary.corpus_config
        baseline_dir = $summary.baseline_dir
        run_dir = $summary.run_dir
        sample_chunks = $resolvedCaseSampleChunks
        diff_generated = $summary.diff_generated
        baseline_missing = $summary.baseline_missing
        missing_baseline_files = $summary.missing_baseline_files
        compared_artifacts = $summary.compared_artifacts
        heavy_lanes = $summary.heavy_lanes
        heavy_world_block_statistics_enabled = $EnableHeavyWorldBlockStatistics
        heavy_world_block_statistics_chunk_budget = $HeavyWorldBlockStatisticsChunkBudget
        overall_status = $summary.overall_status
        summary_path = $summaryPath
    }
}

$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")).Path
$isSingleCaseMode = -not [string]::IsNullOrWhiteSpace($CorpusConfig) -or -not [string]::IsNullOrWhiteSpace($BaselineDir)
$isRootMode = -not [string]::IsNullOrWhiteSpace($CorpusRoot) -or -not [string]::IsNullOrWhiteSpace($BaselineRoot)

if ($isSingleCaseMode -and $isRootMode) {
    throw "Choose either single-case mode (-CorpusConfig/-BaselineDir) or root mode (-CorpusRoot/-BaselineRoot), not both."
}

if (-not $isSingleCaseMode -and -not $isRootMode) {
    throw "Provide either -CorpusConfig/-BaselineDir for single-case mode or -CorpusRoot/-BaselineRoot for root mode."
}

if ($isSingleCaseMode -and ([string]::IsNullOrWhiteSpace($CorpusConfig) -or [string]::IsNullOrWhiteSpace($BaselineDir))) {
    throw "Single-case mode requires both -CorpusConfig and -BaselineDir."
}

if ($isRootMode -and ([string]::IsNullOrWhiteSpace($CorpusRoot) -or [string]::IsNullOrWhiteSpace($BaselineRoot))) {
    throw "Root mode requires both -CorpusRoot and -BaselineRoot."
}

if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
    $resolvedOutputRoot = Join-Path $repoRoot "target/worldgen-audit-ci"
} elseif ([System.IO.Path]::IsPathRooted($OutputRoot)) {
    $resolvedOutputRoot = [System.IO.Path]::GetFullPath($OutputRoot)
} else {
    $resolvedOutputRoot = [System.IO.Path]::GetFullPath((Join-Path $repoRoot $OutputRoot))
}

if ($isSingleCaseMode) {
    $singleCaseSampleChunks = Resolve-CaseSampleChunks -ExplicitSampleChunks $SampleChunks
    $singleCaseSummary = Invoke-AuditVerifyCase `
        -RepoRoot $repoRoot `
        -CorpusConfig $CorpusConfig `
        -BaselineDir $BaselineDir `
        -OutputRootPath $resolvedOutputRoot `
        -CaseSampleChunks $singleCaseSampleChunks `
        -EnableHeavyWorldBlockStatistics $HeavyWorldBlockStatistics `
        -HeavyWorldBlockStatisticsChunkBudget $HeavyWorldBlockStatisticsChunkBudget

    if ($singleCaseSummary.overall_status -ne "match") {
        throw "Worldgen audit baseline verification failed with status '$($singleCaseSummary.overall_status)'. Summary: $($singleCaseSummary.summary_path)"
    }

    return
}

$corpusRootPath = Resolve-RepoPath -RepoRoot $repoRoot -Path $CorpusRoot
if (-not (Test-Path -LiteralPath $corpusRootPath)) {
    throw "Corpus root does not exist: $corpusRootPath"
}

$baselineRootPath = Resolve-RepoPath -RepoRoot $repoRoot -Path $BaselineRoot
New-Item -ItemType Directory -Path $resolvedOutputRoot -Force | Out-Null

$caseFiles = @(
    Get-ChildItem -LiteralPath $corpusRootPath -Recurse -File -Filter "*.ron" |
        Sort-Object FullName
)

if ($caseFiles.Count -eq 0) {
    throw "No corpus cases were found under $corpusRootPath"
}

$caseResults = @()
foreach ($caseFile in $caseFiles) {
    $relativeCasePath = Get-RelativeChildPath -RootPath $corpusRootPath -ChildPath $caseFile.FullName
    $relativeCaseDir = Split-Path -Path $relativeCasePath -Parent
    $relativeCaseName = [System.IO.Path]::GetFileNameWithoutExtension($relativeCasePath)
    $relativeCaseStem = if ([string]::IsNullOrWhiteSpace($relativeCaseDir)) {
        $relativeCaseName
    } else {
        Join-Path $relativeCaseDir $relativeCaseName
    }
    $caseTier = Get-CaseTier -CorpusRootPath $corpusRootPath -RelativeCasePath $relativeCasePath
    $caseSampleChunks = Resolve-CaseSampleChunks -ExplicitSampleChunks $SampleChunks
    $caseBaselineDir = Join-Path $baselineRootPath $relativeCaseStem
    $caseOutputRoot = Join-Path $resolvedOutputRoot $relativeCaseStem

    try {
        $caseSummary = Invoke-AuditVerifyCase `
            -RepoRoot $repoRoot `
            -CorpusConfig $caseFile.FullName `
            -BaselineDir $caseBaselineDir `
            -OutputRootPath $caseOutputRoot `
            -CaseSampleChunks $caseSampleChunks `
            -EnableHeavyWorldBlockStatistics $HeavyWorldBlockStatistics `
            -HeavyWorldBlockStatisticsChunkBudget $HeavyWorldBlockStatisticsChunkBudget

        $caseResults += [pscustomobject]@{
            case = Normalize-PathForSummary -Path $relativeCasePath
            tier = $caseTier
            sample_chunks = $caseSummary.sample_chunks
            corpus_config = $caseSummary.corpus_config
            baseline_dir = $caseSummary.baseline_dir
            run_dir = $caseSummary.run_dir
            summary_path = $caseSummary.summary_path
            heavy_lanes = $caseSummary.heavy_lanes
            overall_status = $caseSummary.overall_status
            missing_baseline_files = $caseSummary.missing_baseline_files
            error_message = $null
        }
    } catch {
        $caseResults += [pscustomobject]@{
            case = Normalize-PathForSummary -Path $relativeCasePath
            tier = $caseTier
            sample_chunks = $caseSampleChunks
            corpus_config = $caseFile.FullName
            baseline_dir = $caseBaselineDir
            run_dir = $null
            summary_path = $null
            heavy_lanes = [pscustomobject]@{}
            overall_status = "error"
            missing_baseline_files = @()
            error_message = $_.Exception.Message
        }
    }
}

$statusCountsMap = [ordered]@{}
foreach ($status in "match", "skipped", "baseline_missing", "mismatch", "error") {
    $statusCountsMap[$status] = @($caseResults | Where-Object { $_.overall_status -eq $status }).Count
}

$heavyLaneStatusCountsMap = [ordered]@{}
foreach (
    $status in
    "not_requested",
    "precheck_missing",
    "precheck_failed",
    "baseline_missing",
    "actual_missing",
    "match",
    "mismatch",
    "skipped",
    "error"
) {
    $heavyLaneStatusCountsMap[$status] = 0
}
foreach ($caseResult in $caseResults) {
    $heavyStatus = "not_requested"
    $heavyLane = $null
    if ($null -ne $caseResult.heavy_lanes) {
        $heavyLane = Get-OptionalObjectPropertyValue `
            -Value $caseResult.heavy_lanes `
            -Name "world_block_statistics"
    }
    if ($null -ne $heavyLane) {
        $resolvedHeavyStatus = Get-OptionalStringPropertyValue -Value $heavyLane -Name "status"
        if (-not [string]::IsNullOrWhiteSpace($resolvedHeavyStatus)) {
            $heavyStatus = $resolvedHeavyStatus
        }
    } elseif ($caseResult.overall_status -eq "error" -and [bool]$HeavyWorldBlockStatistics) {
        $heavyStatus = "error"
    }

    if (-not $heavyLaneStatusCountsMap.Contains($heavyStatus)) {
        $heavyLaneStatusCountsMap[$heavyStatus] = 0
    }
    $heavyLaneStatusCountsMap[$heavyStatus] += 1
}

$rootOverallStatus = if ($statusCountsMap["error"] -gt 0) {
    "error"
} elseif ($statusCountsMap["baseline_missing"] -gt 0) {
    "baseline_missing"
} elseif ($statusCountsMap["mismatch"] -gt 0) {
    "mismatch"
} elseif ($statusCountsMap["skipped"] -gt 0) {
    "skipped"
} else {
    "match"
}

$rootSummaryPath = Join-Path $resolvedOutputRoot "summary.json"
$heavyInvocationContract = [pscustomobject]@{
    enabled = [bool]$HeavyWorldBlockStatistics
    requested_chunk_budget = [int]$HeavyWorldBlockStatisticsChunkBudget
    mode = if ([bool]$HeavyWorldBlockStatistics) {
        "opt_in_non_gating"
    } else {
        "disabled"
    }
}
$rootSummary = [pscustomobject]@{
    schema_version = "worldgen_compare_root_summary_v3"
    corpus_root = $CorpusRoot
    baseline_root = $BaselineRoot
    output_root = $resolvedOutputRoot
    heavy_world_block_statistics = $heavyInvocationContract
    case_count = $caseResults.Count
    status_counts = [pscustomobject]$statusCountsMap
    heavy_lane_status_counts = [pscustomobject]$heavyLaneStatusCountsMap
    cases = $caseResults
    overall_status = $rootOverallStatus
}

Write-NormalizedJson -Value $rootSummary -Path $rootSummaryPath

$githubStepSummaryPath = $env:GITHUB_STEP_SUMMARY
if (-not [string]::IsNullOrWhiteSpace($githubStepSummaryPath)) {
    $heavyMatchCount = [int]$heavyLaneStatusCountsMap["match"]
    $heavyMismatchCount = [int]$heavyLaneStatusCountsMap["mismatch"]
    $heavyPrecheckFailedCount = [int]$heavyLaneStatusCountsMap["precheck_failed"]
    $heavyPrecheckMissingCount = [int]$heavyLaneStatusCountsMap["precheck_missing"]
    $heavyBaselineMissingCount = [int]$heavyLaneStatusCountsMap["baseline_missing"]
    $heavyActualMissingCount = [int]$heavyLaneStatusCountsMap["actual_missing"]
    $heavyErrorCount = [int]$heavyLaneStatusCountsMap["error"]

    @"
## Worldgen audit verifier result
- overall status: `$rootOverallStatus`
- case count: `$($caseResults.Count)`
- root summary: `$rootSummaryPath`
- heavy world_block_statistics requested: `$([bool]$HeavyWorldBlockStatistics)`
- heavy world_block_statistics chunk budget: `$([int]$HeavyWorldBlockStatisticsChunkBudget)`
- heavy lane status counts: `match=$heavyMatchCount`, `mismatch=$heavyMismatchCount`, `precheck_failed=$heavyPrecheckFailedCount`, `precheck_missing=$heavyPrecheckMissingCount`, `baseline_missing=$heavyBaselineMissingCount`, `actual_missing=$heavyActualMissingCount`, `error=$heavyErrorCount`
- heavy lane remains opt-in and non-gating; root verdict still follows the default gating artifacts
"@ | Out-File -FilePath $githubStepSummaryPath -Encoding utf8 -Append
}

if ($rootOverallStatus -ne "match") {
    throw "Worldgen audit root verification failed with status '$rootOverallStatus'. Summary: $rootSummaryPath"
}
