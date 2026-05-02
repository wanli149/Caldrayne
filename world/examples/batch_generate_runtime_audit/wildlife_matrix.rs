use std::collections::BTreeSet;

use common::{
    calendar::{Calendar, CalendarEvent},
    generation::ChunkSupplement,
    resources::TimeOfDay,
    spiral::Spiral2d,
    terrain::{BiomeKind, TerrainChunkSize},
    vol::RectVolSize,
};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use serde::Serialize;
use vek::Vec2;
use veldr_world::{
    IndexRef, World,
    layer::wildlife,
    sim::{AquaticFaunaSummary, AquaticSpawnPotential, SimChunk},
    util::{Sampler, seed_expan},
};

const FIXED_NIGHT_TIME_SECONDS: f64 = 22.0 * 3600.0;
const WILDLIFE_SCORE_SAMPLE_STRIDE: usize = 4;
const MAX_BUCKET_CANDIDATES: usize = 32;
const PRIMARY_WILDLIFE_BUCKETS: [&str; 8] = [
    "forest",
    "jungle",
    "snowland",
    "taiga",
    "desert",
    "grassland_or_savannah",
    "ocean",
    "river",
];
const FALLBACK_WILDLIFE_BUCKETS: [&str; 2] = ["fallback_general", "fallback_any"];
const EXCLUDED_WILDLIFE_BIOMES: [&str; 2] = ["Void", "Lake"];
const AQUATIC_FAUNA_BUCKETS: [&str; 4] = [
    "freshwater_fauna",
    "coastal_fauna",
    "shelf_fauna",
    "pelagic_fauna",
];

pub fn build_wildlife_runtime_matrix(
    run_id: &str,
    world: &World,
    index_ref: IndexRef,
    gen_opts: &crate::GenOpts,
    sample_chunks: usize,
) -> AuditWildlifeRuntimeMatrixFile {
    let target_chunks = sample_chunks.max(1);
    let size = world.sim().get_size();
    let edge_margin_chunks = wildlife_probe_edge_margin(size);
    let aquatic_fauna_samples = select_aquatic_fauna_chunks(world, size, edge_margin_chunks)
        .into_iter()
        .map(|selected| build_aquatic_fauna_sample(world, selected))
        .collect();
    let sampled_chunks =
        select_wildlife_chunks(world, index_ref, target_chunks, edge_margin_chunks)
            .into_iter()
            .map(|selected| build_wildlife_chunk_sample(world, index_ref, selected))
            .collect();

    AuditWildlifeRuntimeMatrixFile {
        run_id: run_id.to_owned(),
        seed: world.sim().seed,
        gen_opts: gen_opts.clone(),
        recipe: crate::build_recipe_summary(world),
        sample_chunks: target_chunks,
        runtime_audit_mode: "wildlife_runtime_matrix_v2".to_owned(),
        strict_determinism: true,
        runtime_chunk_contract: AuditWildlifeRuntimeContractSummary {
            base_chunk_entry:
                "world.generate_chunk_static_snapshot(time=fixed)+layer::wildlife::apply_wildlife_supplement(...)"
                    .to_owned(),
            includes_world_runtime_finalize: false,
            includes_time_context: true,
            includes_rtsim_resource_thinning: false,
            wildlife_only_supplement: true,
        },
        contexts: AuditWildlifeRuntimeContextsSummary {
            baseline_night: WildlifeAuditContext::baseline_night().summary(),
            halloween_night: WildlifeAuditContext::halloween_night().summary(),
        },
        aquatic_fauna_sampling_contract: AuditAquaticFaunaSamplingContractSummary {
            sampler_id: "center_spiral_aquatic_fauna_buckets_v1".to_owned(),
            edge_margin_chunks,
            bucket_coverage_mode: "best_effort_primary_buckets".to_owned(),
            bucket_order: AquaticFaunaSelectionBucket::ordered_labels()
                .into_iter()
                .map(str::to_owned)
                .collect(),
            distinct_chunks: true,
            selection_contract:
                "deterministic first interior chunk per fauna bucket using WorldSim aquatic \
                 fauna summary"
                    .to_owned(),
        },
        sampling_contract: AuditWildlifeSamplingContractSummary {
            sampler_id: "center_spiral_biome_plus_aquatic_buckets_v2".to_owned(),
            edge_margin_chunks,
            requires_site_free_chunks: true,
            requires_waypoint_free_chunks: true,
            requires_positive_spawn_rate: true,
            excluded_biomes: EXCLUDED_WILDLIFE_BIOMES
                .into_iter()
                .map(str::to_owned)
                .collect(),
            bucket_coverage_mode: "best_effort_primary_then_fallback".to_owned(),
            bucket_order: WildlifeSelectionBucket::ordered_labels()
                .into_iter()
                .map(str::to_owned)
                .collect(),
            fallback_bucket_order: FALLBACK_WILDLIFE_BUCKETS
                .into_iter()
                .map(str::to_owned)
                .collect(),
            max_bucket_candidates: MAX_BUCKET_CANDIDATES,
            score_sample_stride: WILDLIFE_SCORE_SAMPLE_STRIDE,
            selection_score_contract:
                "deterministic_density_signal=sum(requestable_density*spawn_rate) over \
                 fixed-stride columns"
                    .to_owned(),
        },
        aquatic_fauna_samples,
        sampled_chunks,
    }
}

#[derive(Serialize)]
pub struct AuditWildlifeRuntimeMatrixFile {
    pub run_id: String,
    pub seed: u32,
    pub gen_opts: crate::GenOpts,
    pub recipe: crate::AuditRecipeSummary,
    pub sample_chunks: usize,
    pub runtime_audit_mode: String,
    pub strict_determinism: bool,
    pub runtime_chunk_contract: AuditWildlifeRuntimeContractSummary,
    pub contexts: AuditWildlifeRuntimeContextsSummary,
    pub aquatic_fauna_sampling_contract: AuditAquaticFaunaSamplingContractSummary,
    pub sampling_contract: AuditWildlifeSamplingContractSummary,
    pub aquatic_fauna_samples: Vec<AuditAquaticFaunaSample>,
    pub sampled_chunks: Vec<AuditWildlifeRuntimeSample>,
}

#[derive(Serialize)]
pub struct AuditWildlifeRuntimeContractSummary {
    pub base_chunk_entry: String,
    pub includes_world_runtime_finalize: bool,
    pub includes_time_context: bool,
    pub includes_rtsim_resource_thinning: bool,
    pub wildlife_only_supplement: bool,
}

#[derive(Serialize)]
pub struct AuditWildlifeRuntimeContextsSummary {
    pub baseline_night: AuditWildlifeContextSummary,
    pub halloween_night: AuditWildlifeContextSummary,
}

#[derive(Serialize)]
pub struct AuditWildlifeContextSummary {
    pub variant_mode: String,
    pub time_of_day_seconds: f64,
    pub day_period: String,
    pub calendar_events: Vec<String>,
}

#[derive(Serialize)]
pub struct AuditWildlifeSamplingContractSummary {
    pub sampler_id: String,
    pub edge_margin_chunks: i32,
    pub requires_site_free_chunks: bool,
    pub requires_waypoint_free_chunks: bool,
    pub requires_positive_spawn_rate: bool,
    pub excluded_biomes: Vec<String>,
    pub bucket_coverage_mode: String,
    pub bucket_order: Vec<String>,
    pub fallback_bucket_order: Vec<String>,
    pub max_bucket_candidates: usize,
    pub score_sample_stride: usize,
    pub selection_score_contract: String,
}

#[derive(Serialize)]
pub struct AuditAquaticFaunaSamplingContractSummary {
    pub sampler_id: String,
    pub edge_margin_chunks: i32,
    pub bucket_coverage_mode: String,
    pub bucket_order: Vec<String>,
    pub distinct_chunks: bool,
    pub selection_contract: String,
}

#[derive(Serialize)]
pub struct AuditAquaticFaunaSample {
    pub chunk_pos: [i32; 2],
    pub selection_bucket: String,
    pub biome: String,
    pub alt: f32,
    pub water_alt: f32,
    pub near_water: bool,
    pub aquatic_spawn_potential: AuditAquaticSpawnPotentialSummary,
    pub aquatic_fauna: AuditAquaticFaunaSummary,
}

#[derive(Serialize)]
pub struct AuditAquaticFaunaSummary {
    pub freshwater_fauna: bool,
    pub coastal_fauna: bool,
    pub shelf_fauna: bool,
    pub pelagic_fauna: bool,
}

#[derive(Serialize)]
pub struct AuditWildlifeRuntimeSample {
    pub chunk_pos: [i32; 2],
    pub selection_bucket: String,
    pub selection_score: f32,
    pub biome: String,
    pub alt: f32,
    pub temp: f32,
    pub humidity: f32,
    pub tree_density: f32,
    pub contains_river: bool,
    pub near_water: bool,
    pub aquatic_spawn_potential: AuditAquaticSpawnPotentialSummary,
    pub baseline_night: AuditWildlifeRuntimeVariant,
    pub halloween_night: AuditWildlifeRuntimeVariant,
}

#[derive(Serialize)]
pub struct AuditAquaticSpawnPotentialSummary {
    pub freshwater_shoreline: bool,
    pub river_channel: bool,
    pub lake_water: bool,
    pub coastal_shoreline: bool,
    pub submerged_freshwater: bool,
    pub submerged_marine: bool,
    pub open_ocean: bool,
}

#[derive(Serialize)]
pub struct AuditWildlifeRuntimeVariant {
    pub variant_mode: String,
    pub expected_spawn_score: f32,
    pub entity_signature_count: usize,
    pub entity_signatures: Vec<String>,
}

struct WildlifeAuditContext {
    variant_mode: &'static str,
    time_of_day: TimeOfDay,
    calendar_events: &'static [CalendarEvent],
}

impl WildlifeAuditContext {
    fn baseline_night() -> Self {
        Self {
            variant_mode: "baseline_night",
            time_of_day: TimeOfDay::new(FIXED_NIGHT_TIME_SECONDS),
            calendar_events: &[],
        }
    }

    fn halloween_night() -> Self {
        Self {
            variant_mode: "halloween_night",
            time_of_day: TimeOfDay::new(FIXED_NIGHT_TIME_SECONDS),
            calendar_events: &[CalendarEvent::Halloween],
        }
    }

    fn calendar(&self) -> Calendar { Calendar::from_events(self.calendar_events.to_vec()) }

    fn summary(&self) -> AuditWildlifeContextSummary {
        AuditWildlifeContextSummary {
            variant_mode: self.variant_mode.to_owned(),
            time_of_day_seconds: self.time_of_day.0,
            day_period: format!("{:?}", self.time_of_day.day_period()),
            calendar_events: self
                .calendar_events
                .iter()
                .map(|event| format!("{event:?}"))
                .collect(),
        }
    }
}

struct SelectedWildlifeChunk {
    chunk_pos: Vec2<i32>,
    selection_bucket: String,
    baseline_expected_score: f32,
    halloween_expected_score: f32,
    selection_score: f32,
}

struct SelectedAquaticFaunaChunk {
    chunk_pos: Vec2<i32>,
    selection_bucket: String,
}

#[derive(Copy, Clone)]
enum WildlifeSelectionBucket {
    Forest,
    Jungle,
    Snowland,
    Taiga,
    Desert,
    GrasslandSavannah,
    Ocean,
    River,
}

impl WildlifeSelectionBucket {
    fn ordered() -> [Self; 8] {
        [
            Self::Forest,
            Self::Jungle,
            Self::Snowland,
            Self::Taiga,
            Self::Desert,
            Self::GrasslandSavannah,
            Self::Ocean,
            Self::River,
        ]
    }

    fn ordered_labels() -> [&'static str; 8] { PRIMARY_WILDLIFE_BUCKETS }

    fn label(self) -> &'static str {
        match self {
            Self::Forest => "forest",
            Self::Jungle => "jungle",
            Self::Snowland => "snowland",
            Self::Taiga => "taiga",
            Self::Desert => "desert",
            Self::GrasslandSavannah => "grassland_or_savannah",
            Self::Ocean => "ocean",
            Self::River => "river",
        }
    }

    fn matches(self, chunk: &SimChunk, aquatic: AquaticSpawnPotential) -> bool {
        let biome = chunk.get_biome();
        match self {
            Self::Forest => {
                biome == BiomeKind::Forest && chunk.tree_density > 0.35 && !chunk.river.near_water()
            },
            Self::Jungle => biome == BiomeKind::Jungle,
            Self::Snowland => biome == BiomeKind::Snowland,
            Self::Taiga => biome == BiomeKind::Taiga,
            Self::Desert => biome == BiomeKind::Desert,
            Self::GrasslandSavannah => {
                matches!(biome, BiomeKind::Grassland | BiomeKind::Savannah)
                    && !chunk.river.near_water()
            },
            Self::Ocean => aquatic.open_ocean,
            Self::River => aquatic.river_channel || aquatic.freshwater_shoreline,
        }
    }
}

#[derive(Copy, Clone)]
enum AquaticFaunaSelectionBucket {
    Freshwater,
    Coastal,
    Shelf,
    Pelagic,
}

impl AquaticFaunaSelectionBucket {
    fn ordered() -> [Self; 4] { [Self::Freshwater, Self::Coastal, Self::Shelf, Self::Pelagic] }

    fn ordered_labels() -> [&'static str; 4] { AQUATIC_FAUNA_BUCKETS }

    fn label(self) -> &'static str {
        match self {
            Self::Freshwater => "freshwater_fauna",
            Self::Coastal => "coastal_fauna",
            Self::Shelf => "shelf_fauna",
            Self::Pelagic => "pelagic_fauna",
        }
    }

    fn matches(self, fauna: AquaticFaunaSummary) -> bool {
        match self {
            Self::Freshwater => fauna.freshwater_fauna,
            Self::Coastal => fauna.coastal_fauna,
            Self::Shelf => fauna.shelf_fauna,
            Self::Pelagic => fauna.pelagic_fauna,
        }
    }
}

fn build_wildlife_chunk_sample(
    world: &World,
    index_ref: IndexRef,
    selected: SelectedWildlifeChunk,
) -> AuditWildlifeRuntimeSample {
    let sim_chunk = world
        .sim()
        .get(selected.chunk_pos)
        .expect("wildlife audit chunk selection should stay within world bounds");
    let aquatic_spawn_potential = world
        .sim()
        .aquatic_spawn_potential(selected.chunk_pos)
        .expect("wildlife audit chunk selection should stay within world bounds");

    AuditWildlifeRuntimeSample {
        chunk_pos: [selected.chunk_pos.x, selected.chunk_pos.y],
        selection_bucket: selected.selection_bucket,
        selection_score: selected.selection_score,
        biome: format!("{:?}", sim_chunk.get_biome()),
        alt: sim_chunk.alt,
        temp: sim_chunk.temp,
        humidity: sim_chunk.humidity,
        tree_density: sim_chunk.tree_density,
        contains_river: sim_chunk.river.is_river(),
        near_water: sim_chunk.river.near_water(),
        aquatic_spawn_potential: AuditAquaticSpawnPotentialSummary {
            freshwater_shoreline: aquatic_spawn_potential.freshwater_shoreline,
            river_channel: aquatic_spawn_potential.river_channel,
            lake_water: aquatic_spawn_potential.lake_water,
            coastal_shoreline: aquatic_spawn_potential.coastal_shoreline,
            submerged_freshwater: aquatic_spawn_potential.submerged_freshwater,
            submerged_marine: aquatic_spawn_potential.submerged_marine,
            open_ocean: aquatic_spawn_potential.open_ocean,
        },
        baseline_night: build_wildlife_variant(
            world,
            index_ref,
            selected.chunk_pos,
            sim_chunk,
            WildlifeAuditContext::baseline_night(),
            selected.baseline_expected_score,
        ),
        halloween_night: build_wildlife_variant(
            world,
            index_ref,
            selected.chunk_pos,
            sim_chunk,
            WildlifeAuditContext::halloween_night(),
            selected.halloween_expected_score,
        ),
    }
}

fn build_aquatic_fauna_sample(
    world: &World,
    selected: SelectedAquaticFaunaChunk,
) -> AuditAquaticFaunaSample {
    let sim_chunk = world
        .sim()
        .get(selected.chunk_pos)
        .expect("aquatic fauna audit chunk selection should stay within world bounds");
    let aquatic_spawn_potential = world
        .sim()
        .aquatic_spawn_potential(selected.chunk_pos)
        .expect("aquatic fauna audit chunk selection should stay within world bounds");
    let aquatic_fauna = world
        .sim()
        .aquatic_fauna_summary(selected.chunk_pos)
        .expect("aquatic fauna audit chunk selection should stay within world bounds");

    AuditAquaticFaunaSample {
        chunk_pos: [selected.chunk_pos.x, selected.chunk_pos.y],
        selection_bucket: selected.selection_bucket,
        biome: format!("{:?}", sim_chunk.get_biome()),
        alt: sim_chunk.alt,
        water_alt: sim_chunk.water_alt,
        near_water: sim_chunk.river.near_water(),
        aquatic_spawn_potential: AuditAquaticSpawnPotentialSummary {
            freshwater_shoreline: aquatic_spawn_potential.freshwater_shoreline,
            river_channel: aquatic_spawn_potential.river_channel,
            lake_water: aquatic_spawn_potential.lake_water,
            coastal_shoreline: aquatic_spawn_potential.coastal_shoreline,
            submerged_freshwater: aquatic_spawn_potential.submerged_freshwater,
            submerged_marine: aquatic_spawn_potential.submerged_marine,
            open_ocean: aquatic_spawn_potential.open_ocean,
        },
        aquatic_fauna: AuditAquaticFaunaSummary {
            freshwater_fauna: aquatic_fauna.freshwater_fauna,
            coastal_fauna: aquatic_fauna.coastal_fauna,
            shelf_fauna: aquatic_fauna.shelf_fauna,
            pelagic_fauna: aquatic_fauna.pelagic_fauna,
        },
    }
}

fn build_wildlife_variant(
    world: &World,
    index_ref: IndexRef,
    chunk_pos: Vec2<i32>,
    sim_chunk: &SimChunk,
    context: WildlifeAuditContext,
    expected_spawn_score: f32,
) -> AuditWildlifeRuntimeVariant {
    let calendar = context.calendar();
    let time_context = (context.time_of_day, calendar.clone());
    let static_chunk = world
        .generate_chunk_static_snapshot(index_ref, chunk_pos, || false, Some(time_context.clone()))
        .expect("wildlife audit static chunk generation should succeed");

    let chunk_wpos2d = chunk_pos * TerrainChunkSize::RECT_SIZE.map(|edge| edge as i32);
    let column_sampler = world.sample_columns();
    let mut columns = Vec::with_capacity(
        TerrainChunkSize::RECT_SIZE.x as usize * TerrainChunkSize::RECT_SIZE.y as usize,
    );
    for y in 0..TerrainChunkSize::RECT_SIZE.y as i32 {
        for x in 0..TerrainChunkSize::RECT_SIZE.x as i32 {
            columns.push(column_sampler.get((
                chunk_wpos2d + Vec2::new(x, y),
                index_ref,
                Some(&calendar),
            )));
        }
    }
    let mut supplement = ChunkSupplement::default();
    let runtime_rng_seed = seed_expan::diffuse_mult(&[
        world.sim().seed,
        chunk_pos.x as u32,
        chunk_pos.y as u32,
        context.time_of_day.day().to_bits() as u32,
        (context.time_of_day.day().to_bits() >> 32) as u32,
        runtime_calendar_mask(&calendar),
        0x5255_4E54,
    ]);
    let mut runtime_rng = ChaCha8Rng::from_seed(seed_expan::rng_state(runtime_rng_seed));

    wildlife::apply_wildlife_supplement(
        &mut runtime_rng,
        chunk_wpos2d,
        |offs| {
            let width = TerrainChunkSize::RECT_SIZE.x as usize;
            let idx = offs.y.max(0) as usize * width + offs.x.max(0) as usize;
            columns.get(idx).and_then(Option::as_ref)
        },
        &static_chunk,
        index_ref,
        sim_chunk,
        &mut supplement,
        Some(&time_context),
    );

    let entity_signatures = super::summarize_entity_spawns(&supplement.entity_spawns);
    AuditWildlifeRuntimeVariant {
        variant_mode: context.variant_mode.to_owned(),
        expected_spawn_score,
        entity_signature_count: entity_signatures.len(),
        entity_signatures,
    }
}

fn select_aquatic_fauna_chunks(
    world: &World,
    size: Vec2<u32>,
    edge_margin_chunks: i32,
) -> Vec<SelectedAquaticFaunaChunk> {
    let probe_order = wildlife_probe_order(size);
    let mut selected = Vec::new();
    let mut seen = BTreeSet::new();

    for bucket in AquaticFaunaSelectionBucket::ordered() {
        let Some(candidate) = select_aquatic_fauna_bucket_candidate(
            world,
            size,
            edge_margin_chunks,
            &probe_order,
            &seen,
            bucket,
        ) else {
            continue;
        };
        seen.insert((candidate.chunk_pos.x, candidate.chunk_pos.y));
        selected.push(candidate);
    }

    selected
}

fn select_wildlife_chunks(
    world: &World,
    index_ref: IndexRef,
    target_chunks: usize,
    edge_margin_chunks: i32,
) -> Vec<SelectedWildlifeChunk> {
    let size = world.sim().get_size();
    let probe_order = wildlife_probe_order(size);
    let mut selected = Vec::new();
    let mut seen = BTreeSet::new();

    for bucket in WildlifeSelectionBucket::ordered() {
        if selected.len() >= target_chunks {
            break;
        }

        if let Some(best_candidate) = select_bucket_candidate(
            world,
            index_ref,
            size,
            edge_margin_chunks,
            &probe_order,
            &seen,
            bucket,
        ) {
            seen.insert((best_candidate.chunk_pos.x, best_candidate.chunk_pos.y));
            selected.push(best_candidate);
        }
    }

    fill_wildlife_chunk_fallbacks(
        &mut selected,
        &mut seen,
        target_chunks,
        &probe_order,
        world,
        index_ref,
        size,
        edge_margin_chunks,
        |chunk| is_general_wildlife_candidate(chunk),
        "fallback_general",
    );
    fill_wildlife_chunk_fallbacks(
        &mut selected,
        &mut seen,
        target_chunks,
        &probe_order,
        world,
        index_ref,
        size,
        edge_margin_chunks,
        |_| true,
        "fallback_any",
    );

    selected
}

fn select_aquatic_fauna_bucket_candidate(
    world: &World,
    size: Vec2<u32>,
    edge_margin_chunks: i32,
    probe_order: &[Vec2<i32>],
    seen: &BTreeSet<(i32, i32)>,
    bucket: AquaticFaunaSelectionBucket,
) -> Option<SelectedAquaticFaunaChunk> {
    for chunk_pos in probe_order.iter().copied() {
        if seen.contains(&(chunk_pos.x, chunk_pos.y))
            || !is_interior_probe_candidate(size, edge_margin_chunks, chunk_pos)
        {
            continue;
        }

        let Some(aquatic_fauna) = world.sim().aquatic_fauna_summary(chunk_pos) else {
            continue;
        };
        if !bucket.matches(aquatic_fauna) {
            continue;
        }

        return Some(SelectedAquaticFaunaChunk {
            chunk_pos,
            selection_bucket: bucket.label().to_owned(),
        });
    }

    None
}

fn fill_wildlife_chunk_fallbacks(
    selected: &mut Vec<SelectedWildlifeChunk>,
    seen: &mut BTreeSet<(i32, i32)>,
    target_chunks: usize,
    probe_order: &[Vec2<i32>],
    world: &World,
    index_ref: IndexRef,
    size: Vec2<u32>,
    edge_margin_chunks: i32,
    matches_chunk: impl Fn(&SimChunk) -> bool,
    label: &str,
) {
    for chunk_pos in probe_order.iter().copied() {
        if selected.len() >= target_chunks {
            return;
        }
        if seen.contains(&(chunk_pos.x, chunk_pos.y)) {
            continue;
        }
        let Some(chunk) = world.sim().get(chunk_pos) else {
            continue;
        };
        if is_primary_probe_candidate(size, edge_margin_chunks, chunk_pos, chunk)
            && matches_chunk(chunk)
        {
            let baseline_expected_score = estimate_wildlife_score(
                world,
                index_ref,
                chunk_pos,
                chunk,
                WildlifeAuditContext::baseline_night(),
            );
            let halloween_expected_score = estimate_wildlife_score(
                world,
                index_ref,
                chunk_pos,
                chunk,
                WildlifeAuditContext::halloween_night(),
            );
            seen.insert((chunk_pos.x, chunk_pos.y));
            selected.push(SelectedWildlifeChunk {
                chunk_pos,
                selection_bucket: label.to_owned(),
                baseline_expected_score,
                halloween_expected_score,
                selection_score: baseline_expected_score + halloween_expected_score,
            });
        }
    }
}

fn select_bucket_candidate(
    world: &World,
    index_ref: IndexRef,
    size: Vec2<u32>,
    edge_margin_chunks: i32,
    probe_order: &[Vec2<i32>],
    seen: &BTreeSet<(i32, i32)>,
    bucket: WildlifeSelectionBucket,
) -> Option<SelectedWildlifeChunk> {
    let mut best_candidate = None;
    let mut examined = 0usize;

    for chunk_pos in probe_order.iter().copied() {
        if examined >= MAX_BUCKET_CANDIDATES {
            break;
        }
        if seen.contains(&(chunk_pos.x, chunk_pos.y)) {
            continue;
        }
        let Some(chunk) = world.sim().get(chunk_pos) else {
            continue;
        };
        let Some(aquatic_spawn_potential) = world.sim().aquatic_spawn_potential(chunk_pos) else {
            continue;
        };
        if !is_primary_probe_candidate(size, edge_margin_chunks, chunk_pos, chunk)
            || !bucket.matches(chunk, aquatic_spawn_potential)
        {
            continue;
        }

        let baseline_expected_score = estimate_wildlife_score(
            world,
            index_ref,
            chunk_pos,
            chunk,
            WildlifeAuditContext::baseline_night(),
        );
        let halloween_expected_score = estimate_wildlife_score(
            world,
            index_ref,
            chunk_pos,
            chunk,
            WildlifeAuditContext::halloween_night(),
        );
        let selection_score = baseline_expected_score + halloween_expected_score;
        let candidate = SelectedWildlifeChunk {
            chunk_pos,
            selection_bucket: bucket.label().to_owned(),
            baseline_expected_score,
            halloween_expected_score,
            selection_score,
        };
        let replace_best = best_candidate
            .as_ref()
            .is_none_or(|best: &SelectedWildlifeChunk| selection_score > best.selection_score);
        if replace_best {
            best_candidate = Some(candidate);
        }
        examined += 1;
    }

    best_candidate
}

fn wildlife_probe_order(size: Vec2<u32>) -> Vec<Vec2<i32>> {
    let center = size.map(|axis| axis as i32 / 2);
    let radius = size.x.max(size.y) as i32;
    Spiral2d::with_radius(radius)
        .filter_map(|offset| {
            let pos = center + offset;
            (pos.x >= 0 && pos.y >= 0 && pos.x < size.x as i32 && pos.y < size.y as i32)
                .then_some(pos)
        })
        .collect()
}

fn wildlife_probe_edge_margin(size: Vec2<u32>) -> i32 {
    size.x.min(size.y).clamp(32, 512) as i32 / 32
}

fn is_primary_probe_candidate(
    size: Vec2<u32>,
    edge_margin_chunks: i32,
    chunk_pos: Vec2<i32>,
    chunk: &SimChunk,
) -> bool {
    is_interior_probe_candidate(size, edge_margin_chunks, chunk_pos)
        && chunk.spawn_rate > 0.0
        && chunk.sites.is_empty()
        && !chunk.contains_waypoint
        && !matches!(chunk.get_biome(), BiomeKind::Void | BiomeKind::Lake)
}

fn is_interior_probe_candidate(
    size: Vec2<u32>,
    edge_margin_chunks: i32,
    chunk_pos: Vec2<i32>,
) -> bool {
    let x_limit = size.x as i32;
    let y_limit = size.y as i32;
    if x_limit <= edge_margin_chunks * 2 || y_limit <= edge_margin_chunks * 2 {
        return true;
    }

    chunk_pos.x >= edge_margin_chunks
        && chunk_pos.y >= edge_margin_chunks
        && chunk_pos.x < x_limit - edge_margin_chunks
        && chunk_pos.y < y_limit - edge_margin_chunks
}

fn is_general_wildlife_candidate(chunk: &SimChunk) -> bool {
    matches!(
        chunk.get_biome(),
        BiomeKind::Forest
            | BiomeKind::Jungle
            | BiomeKind::Snowland
            | BiomeKind::Taiga
            | BiomeKind::Desert
            | BiomeKind::Grassland
            | BiomeKind::Savannah
            | BiomeKind::Ocean
    ) || chunk.river.is_river()
}

fn runtime_calendar_mask(calendar: &Calendar) -> u32 {
    calendar
        .events()
        .fold(0u32, |mask, event| mask | (1u32 << (*event as u32)))
}

fn estimate_wildlife_score(
    world: &World,
    index_ref: IndexRef,
    chunk_pos: Vec2<i32>,
    sim_chunk: &SimChunk,
    context: WildlifeAuditContext,
) -> f32 {
    let calendar = context.calendar();
    let chunk_wpos2d = chunk_pos * TerrainChunkSize::RECT_SIZE.map(|edge| edge as i32);
    let column_sampler = world.sample_columns();
    let mut score = 0.0f32;

    for y in (0..TerrainChunkSize::RECT_SIZE.y as i32).step_by(WILDLIFE_SCORE_SAMPLE_STRIDE) {
        for x in (0..TerrainChunkSize::RECT_SIZE.x as i32).step_by(WILDLIFE_SCORE_SAMPLE_STRIDE) {
            let Some(col_sample) =
                column_sampler.get((chunk_wpos2d + Vec2::new(x, y), index_ref, Some(&calendar)))
            else {
                continue;
            };
            let runtime_gate = wildlife::WildlifeRuntimeGate::from_column_sample(&col_sample);

            for (entry, get_density) in &index_ref.wildlife_spawns {
                let density =
                    get_density(sim_chunk, &col_sample) * index_ref.features.wildlife_density;
                let Some(scaled_density) = runtime_gate.scaled_density(density) else {
                    continue;
                };
                if entry
                    .read()
                    .0
                    .request(
                        context.time_of_day.day_period(),
                        Some(&calendar),
                        runtime_gate.is_underwater,
                        runtime_gate.is_ice,
                    )
                    .is_some()
                {
                    score += scaled_density;
                }
            }
        }
    }

    score * (WILDLIFE_SCORE_SAMPLE_STRIDE * WILDLIFE_SCORE_SAMPLE_STRIDE) as f32
}
