mod wildlife_matrix;

use std::{collections::BTreeMap, fs, path::PathBuf};

use common::{
    calendar::{Calendar, CalendarEvent},
    generation::{ChunkSupplement, EntityInfo, EntitySpawn},
    resources::TimeOfDay,
    rtsim::TerrainResource,
    spiral::Spiral2d,
    terrain::{Block, BlockKind, SpriteKind, TerrainChunk, TerrainChunkSize},
    vol::{IntoVolIterator, ReadVol, RectVolSize, WriteVol},
};
use enum_map::EnumMap;
use serde::{Deserialize, Serialize};
use vek::{Rgb, Vec2, Vec3};

pub use wildlife_matrix::{AuditWildlifeRuntimeMatrixFile, build_wildlife_runtime_matrix};

const FIXED_OVERLAY_FIXTURE_PATH: &str =
    "server/tests/data/terrain_overlay/fixed_overlay_fixture_v1.ron";
const FIXED_RTSIM_RESOURCE_FIXTURE_PATH: &str =
    "world/tests/data/worldgen_runtime/rtsim_resource_thinning_fixture_v1.ron";
const FIXED_NIGHT_TIME_SECONDS: f64 = 22.0 * 3600.0;

#[derive(Serialize)]
pub struct AuditChunkRuntimeMatrixFile {
    pub run_id: String,
    pub seed: u32,
    pub gen_opts: super::GenOpts,
    pub recipe: super::AuditRecipeSummary,
    pub sample_chunks: usize,
    pub runtime_audit_mode: String,
    pub strict_determinism: bool,
    pub runtime_chunk_contract: AuditRuntimeChunkContractSummary,
    pub contexts: AuditRuntimeContextsSummary,
    pub fixed_overlay_fixture: AuditFixedOverlayFixtureSummary,
    pub rtsim_resource_sampling_contract: AuditRtsimResourceSamplingContractSummary,
    pub rtsim_resource_fixture: AuditRtsimResourceFixtureSummary,
    pub rtsim_resource_samples: Vec<AuditRtsimResourceSample>,
    pub sampled_chunks: Vec<AuditChunkRuntimeSample>,
}

#[derive(Serialize)]
pub struct AuditRuntimeChunkContractSummary {
    pub base_chunk_entry: String,
    pub fixed_context_entry: String,
    pub rtsim_full_density_entry: String,
    pub rtsim_fixed_fixture_entry: String,
    pub includes_world_runtime_finalize: bool,
    pub includes_time_context: bool,
    pub includes_rtsim_resource_thinning: bool,
}

#[derive(Serialize)]
pub struct AuditRuntimeContextsSummary {
    pub baseline_night: AuditRuntimeContextSummary,
    pub halloween_night: AuditRuntimeContextSummary,
}

#[derive(Serialize)]
pub struct AuditRuntimeContextSummary {
    pub variant_mode: String,
    pub time_of_day_seconds: f64,
    pub day_period: String,
    pub calendar_events: Vec<String>,
}

#[derive(Serialize)]
pub struct AuditFixedOverlayFixtureSummary {
    pub fixture_id: String,
    pub contract_path: String,
    pub operation_count: usize,
}

#[derive(Serialize)]
pub struct AuditRtsimResourceSamplingContractSummary {
    pub sampler_id: String,
    pub edge_margin_chunks: i32,
    pub max_probes: usize,
    pub distinct_chunks: bool,
    pub best_effort: bool,
    pub context_variant: String,
    pub selection_contract: String,
}

#[derive(Serialize)]
pub struct AuditRtsimResourceFixtureSummary {
    pub fixture_id: String,
    pub contract_path: String,
    pub context_variant: String,
    pub fractions: BTreeMap<String, f32>,
}

#[derive(Serialize)]
pub struct AuditRtsimResourceSample {
    pub chunk_pos: [i32; 2],
    pub selection_score: usize,
    pub resource_kind_count: usize,
    pub baseline_night_full_density_runtime_chunk: AuditChunkRuntimeVariant,
    pub baseline_night_full_density_runtime_supplement: AuditRtsimRuntimeSupplementSummary,
    pub baseline_night_full_density_rtsim_resource_block_counts: BTreeMap<String, u64>,
    pub baseline_night_thinned_runtime_chunk: AuditChunkRuntimeVariant,
    pub baseline_night_thinned_runtime_supplement: AuditRtsimRuntimeSupplementSummary,
    pub baseline_night_thinned_rtsim_resource_block_counts: BTreeMap<String, u64>,
}

#[derive(Serialize)]
pub struct AuditChunkRuntimeSample {
    pub chunk_pos: [i32; 2],
    pub base_runtime_chunk: AuditChunkRuntimeVariant,
    pub base_runtime_supplement: AuditChunkRuntimeSupplementSummary,
    pub empty_overlay_runtime_chunk: AuditChunkRuntimeVariant,
    pub fixed_overlay_runtime_chunk: AuditChunkRuntimeVariant,
    pub baseline_night_runtime_chunk: AuditChunkRuntimeVariant,
    pub baseline_night_runtime_supplement: AuditChunkRuntimeSupplementSummary,
    pub halloween_night_runtime_chunk: AuditChunkRuntimeVariant,
    pub halloween_night_runtime_supplement: AuditChunkRuntimeSupplementSummary,
}

#[derive(Serialize)]
pub struct AuditChunkRuntimeVariant {
    pub variant_mode: String,
    pub overlay_blocks_applied: usize,
    pub overlay_operations_skipped: usize,
    pub min_z: i32,
    pub max_z: i32,
    pub sub_chunks: usize,
    pub block_total: u64,
    pub non_air_blocks: u64,
    pub sprite_total: u64,
    pub block_kind_counts: BTreeMap<String, u64>,
    pub sprite_kind_counts: BTreeMap<String, u64>,
}

#[derive(Serialize)]
pub struct AuditChunkRuntimeSupplementSummary {
    pub entity_signature_count: usize,
    pub entity_signatures: Vec<String>,
}

#[derive(Serialize)]
pub struct AuditRtsimRuntimeSupplementSummary {
    pub entity_signature_count: usize,
    pub entity_signatures: Vec<String>,
    pub rtsim_max_resources: BTreeMap<String, usize>,
}

#[derive(Deserialize)]
struct FixedOverlayFixture {
    fixture_id: String,
    operations: Vec<FixedOverlayOperation>,
}

#[derive(Deserialize)]
struct FixedRtsimResourceFixture {
    fixture_id: String,
    context_variant: String,
    fractions: FixedRtsimResourceFractions,
}

#[derive(Deserialize)]
struct FixedRtsimResourceFractions {
    grass: f32,
    flower: f32,
    fruit: f32,
    vegetable: f32,
    mushroom: f32,
    loot: f32,
    plant: f32,
    stone: f32,
    wood: f32,
    gem: f32,
    ore: f32,
}

impl FixedRtsimResourceFractions {
    fn to_enum_map(&self) -> EnumMap<TerrainResource, f32> {
        EnumMap::from_fn(|resource| match resource {
            TerrainResource::Grass => self.grass,
            TerrainResource::Flower => self.flower,
            TerrainResource::Fruit => self.fruit,
            TerrainResource::Vegetable => self.vegetable,
            TerrainResource::Mushroom => self.mushroom,
            TerrainResource::Loot => self.loot,
            TerrainResource::Plant => self.plant,
            TerrainResource::Stone => self.stone,
            TerrainResource::Wood => self.wood,
            TerrainResource::Gem => self.gem,
            TerrainResource::Ore => self.ore,
        })
    }

    fn to_summary_map(&self) -> BTreeMap<String, f32> {
        let mut summarized = BTreeMap::new();
        summarized.insert("Grass".to_owned(), self.grass);
        summarized.insert("Flower".to_owned(), self.flower);
        summarized.insert("Fruit".to_owned(), self.fruit);
        summarized.insert("Vegetable".to_owned(), self.vegetable);
        summarized.insert("Mushroom".to_owned(), self.mushroom);
        summarized.insert("Loot".to_owned(), self.loot);
        summarized.insert("Plant".to_owned(), self.plant);
        summarized.insert("Stone".to_owned(), self.stone);
        summarized.insert("Wood".to_owned(), self.wood);
        summarized.insert("Gem".to_owned(), self.gem);
        summarized.insert("Ore".to_owned(), self.ore);
        summarized
    }
}

#[derive(Deserialize)]
struct FixedOverlayOperation {
    rel_xy: [u32; 2],
    target: FixedOverlayTarget,
    block: FixedOverlayBlock,
}

#[derive(Deserialize)]
enum FixedOverlayTarget {
    TopMostNonAir,
}

#[derive(Deserialize)]
enum FixedOverlayBlock {
    Solid { kind: BlockKind, color: [u8; 3] },
    Air { sprite: SpriteKind },
    Water { sprite: SpriteKind },
}

#[derive(Default)]
struct OverlayApplySummary {
    applied: usize,
    skipped: usize,
}

struct RuntimeAuditContext {
    context_id: &'static str,
    chunk_variant_mode: &'static str,
    time_of_day: TimeOfDay,
    calendar_events: &'static [CalendarEvent],
}

fn resolve_fixed_rtsim_resource_context(
    fixture: &FixedRtsimResourceFixture,
) -> RuntimeAuditContext {
    let context = RuntimeAuditContext::baseline_night();
    assert_eq!(
        fixture.context_variant, context.context_id,
        "fixed rtsim resource fixture context_variant must stay on {}",
        context.context_id
    );
    context
}

impl RuntimeAuditContext {
    fn baseline_night() -> Self {
        Self {
            context_id: "baseline_night",
            chunk_variant_mode: "baseline_night_runtime_chunk",
            time_of_day: TimeOfDay::new(FIXED_NIGHT_TIME_SECONDS),
            calendar_events: &[],
        }
    }

    fn halloween_night() -> Self {
        Self {
            context_id: "halloween_night",
            chunk_variant_mode: "halloween_night_runtime_chunk",
            time_of_day: TimeOfDay::new(FIXED_NIGHT_TIME_SECONDS),
            calendar_events: &[CalendarEvent::Halloween],
        }
    }

    fn time_context(&self) -> (TimeOfDay, Calendar) { (self.time_of_day, self.calendar()) }

    fn calendar(&self) -> Calendar { Calendar::from_events(self.calendar_events.to_vec()) }

    fn summary(&self) -> AuditRuntimeContextSummary {
        AuditRuntimeContextSummary {
            variant_mode: self.context_id.to_owned(),
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

pub fn build_runtime_chunk_matrix(
    run_id: &str,
    world: &super::World,
    index_ref: super::IndexRef,
    gen_opts: &super::GenOpts,
    sample_chunks: usize,
) -> AuditChunkRuntimeMatrixFile {
    let fixture = load_fixed_overlay_fixture();
    let rtsim_fixture = load_fixed_rtsim_resource_fixture();
    let rtsim_context = resolve_fixed_rtsim_resource_context(&rtsim_fixture);
    let rtsim_sample_target = sample_chunks.max(1);
    let rtsim_edge_margin_chunks = runtime_probe_edge_margin(world.sim().get_size());
    let rtsim_max_probes = rtsim_sample_target.max(1) * 64;
    let sampled_chunks = super::sampled_chunk_positions(world.sim().get_size(), sample_chunks)
        .into_iter()
        .map(|chunk_pos| build_runtime_chunk_sample(world, index_ref, chunk_pos, &fixture))
        .collect();
    let rtsim_resource_samples = build_rtsim_resource_samples(
        world,
        index_ref,
        rtsim_sample_target,
        rtsim_edge_margin_chunks,
        rtsim_max_probes,
        &rtsim_context,
        &rtsim_fixture,
    );

    AuditChunkRuntimeMatrixFile {
        run_id: run_id.to_owned(),
        seed: world.sim().seed,
        gen_opts: gen_opts.clone(),
        recipe: super::build_recipe_summary(world),
        sample_chunks,
        runtime_audit_mode: "sampled_runtime_overlay_matrix_v3".to_owned(),
        strict_determinism: true,
        runtime_chunk_contract: AuditRuntimeChunkContractSummary {
            base_chunk_entry: "world.generate_chunk(rtsim_resource_fractions=None,time=None)"
                .to_owned(),
            fixed_context_entry: "world.generate_chunk(rtsim_resource_fractions=None,\
                                  time=baseline_night|halloween_night)"
                .to_owned(),
            rtsim_full_density_entry:
                "world.generate_chunk(rtsim_resource_fractions=fixed_full_density,\
                 time=baseline_night)"
                    .to_owned(),
            rtsim_fixed_fixture_entry:
                "world.generate_chunk(rtsim_resource_fractions=fixed_fixture,time=baseline_night)"
                    .to_owned(),
            includes_world_runtime_finalize: true,
            includes_time_context: true,
            includes_rtsim_resource_thinning: true,
        },
        contexts: AuditRuntimeContextsSummary {
            baseline_night: RuntimeAuditContext::baseline_night().summary(),
            halloween_night: RuntimeAuditContext::halloween_night().summary(),
        },
        fixed_overlay_fixture: AuditFixedOverlayFixtureSummary {
            fixture_id: fixture.fixture_id.clone(),
            contract_path: FIXED_OVERLAY_FIXTURE_PATH.to_owned(),
            operation_count: fixture.operations.len(),
        },
        rtsim_resource_sampling_contract: AuditRtsimResourceSamplingContractSummary {
            sampler_id: "center_spiral_nonempty_rtsim_resource_chunks_v1".to_owned(),
            edge_margin_chunks: rtsim_edge_margin_chunks,
            max_probes: rtsim_max_probes,
            distinct_chunks: true,
            best_effort: true,
            context_variant: rtsim_context.context_id.to_owned(),
            selection_contract: "deterministic best-effort top interior chunks ranked by \
                                 full-density baseline-night rtsim_max_resources upper bound, \
                                 then resource-kind count, then chunk_pos"
                .to_owned(),
        },
        rtsim_resource_fixture: AuditRtsimResourceFixtureSummary {
            fixture_id: rtsim_fixture.fixture_id.clone(),
            contract_path: FIXED_RTSIM_RESOURCE_FIXTURE_PATH.to_owned(),
            context_variant: rtsim_context.context_id.to_owned(),
            fractions: rtsim_fixture.fractions.to_summary_map(),
        },
        rtsim_resource_samples,
        sampled_chunks,
    }
}

fn build_runtime_chunk_sample(
    world: &super::World,
    index_ref: super::IndexRef,
    chunk_pos: Vec2<i32>,
    fixture: &FixedOverlayFixture,
) -> AuditChunkRuntimeSample {
    let (base_runtime_chunk, base_runtime_supplement) = world
        .generate_chunk(index_ref, chunk_pos, None, || false, None)
        .expect("runtime audit chunk generation should succeed");
    let empty_overlay_runtime_chunk = base_runtime_chunk.clone();
    let mut fixed_overlay_runtime_chunk = base_runtime_chunk.clone();
    let empty_overlay_summary = OverlayApplySummary::default();
    let fixed_overlay_summary =
        apply_fixed_overlay_fixture(&mut fixed_overlay_runtime_chunk, fixture);
    let baseline_night_runtime = build_contextual_runtime_variant(
        world,
        index_ref,
        chunk_pos,
        RuntimeAuditContext::baseline_night(),
    );
    let halloween_night_runtime = build_contextual_runtime_variant(
        world,
        index_ref,
        chunk_pos,
        RuntimeAuditContext::halloween_night(),
    );

    AuditChunkRuntimeSample {
        chunk_pos: [chunk_pos.x, chunk_pos.y],
        base_runtime_chunk: summarize_runtime_variant(
            "base_runtime_chunk",
            &base_runtime_chunk,
            OverlayApplySummary::default(),
        ),
        base_runtime_supplement: summarize_runtime_supplement(&base_runtime_supplement),
        empty_overlay_runtime_chunk: summarize_runtime_variant(
            "empty_overlay_runtime_chunk",
            &empty_overlay_runtime_chunk,
            empty_overlay_summary,
        ),
        fixed_overlay_runtime_chunk: summarize_runtime_variant(
            "fixed_overlay_runtime_chunk",
            &fixed_overlay_runtime_chunk,
            fixed_overlay_summary,
        ),
        baseline_night_runtime_chunk: baseline_night_runtime.chunk,
        baseline_night_runtime_supplement: baseline_night_runtime.supplement,
        halloween_night_runtime_chunk: halloween_night_runtime.chunk,
        halloween_night_runtime_supplement: halloween_night_runtime.supplement,
    }
}

struct AuditContextualRuntimeSample {
    chunk: AuditChunkRuntimeVariant,
    supplement: AuditChunkRuntimeSupplementSummary,
    rtsim_max_resources: BTreeMap<String, usize>,
    rtsim_resource_block_counts: BTreeMap<String, u64>,
}

struct SelectedRtsimResourceSample {
    chunk_pos: Vec2<i32>,
    selection_score: usize,
    resource_kind_count: usize,
    full_density_runtime: AuditContextualRuntimeSample,
}

fn build_contextual_runtime_variant(
    world: &super::World,
    index_ref: super::IndexRef,
    chunk_pos: Vec2<i32>,
    context: RuntimeAuditContext,
) -> AuditContextualRuntimeSample {
    let (runtime_chunk, supplement) = world
        .generate_chunk(
            index_ref,
            chunk_pos,
            None,
            || false,
            Some(context.time_context()),
        )
        .expect("runtime audit fixed-context chunk generation should succeed");
    AuditContextualRuntimeSample {
        chunk: summarize_runtime_variant(
            context.chunk_variant_mode,
            &runtime_chunk,
            OverlayApplySummary::default(),
        ),
        supplement: summarize_runtime_supplement(&supplement),
        rtsim_max_resources: summarize_rtsim_max_resources(&supplement.rtsim_max_resources),
        rtsim_resource_block_counts: summarize_runtime_chunk_resources(&runtime_chunk),
    }
}

fn summarize_runtime_variant(
    variant_mode: &str,
    chunk: &TerrainChunk,
    overlay_summary: OverlayApplySummary,
) -> AuditChunkRuntimeVariant {
    let volume = super::summarize_chunk_volume(chunk);
    AuditChunkRuntimeVariant {
        variant_mode: variant_mode.to_owned(),
        overlay_blocks_applied: overlay_summary.applied,
        overlay_operations_skipped: overlay_summary.skipped,
        min_z: chunk.get_min_z(),
        max_z: chunk.get_max_z(),
        sub_chunks: chunk.sub_chunks_len(),
        block_total: volume.block_total,
        non_air_blocks: volume.non_air_blocks,
        sprite_total: volume.sprite_total,
        block_kind_counts: volume.block_kind_counts,
        sprite_kind_counts: volume.sprite_kind_counts,
    }
}

fn summarize_runtime_supplement(
    supplement: &ChunkSupplement,
) -> AuditChunkRuntimeSupplementSummary {
    let entity_signatures = summarize_entity_spawns(&supplement.entity_spawns);
    AuditChunkRuntimeSupplementSummary {
        entity_signature_count: entity_signatures.len(),
        entity_signatures,
    }
}

fn summarize_rtsim_runtime_supplement(
    supplement: &AuditChunkRuntimeSupplementSummary,
    rtsim_max_resources: &BTreeMap<String, usize>,
) -> AuditRtsimRuntimeSupplementSummary {
    AuditRtsimRuntimeSupplementSummary {
        entity_signature_count: supplement.entity_signature_count,
        entity_signatures: supplement.entity_signatures.clone(),
        rtsim_max_resources: rtsim_max_resources.clone(),
    }
}

fn summarize_rtsim_max_resources(
    rtsim_max_resources: &enum_map::EnumMap<TerrainResource, usize>,
) -> BTreeMap<String, usize> {
    let mut summarized = BTreeMap::new();
    for resource in [
        TerrainResource::Grass,
        TerrainResource::Flower,
        TerrainResource::Fruit,
        TerrainResource::Vegetable,
        TerrainResource::Mushroom,
        TerrainResource::Loot,
        TerrainResource::Plant,
        TerrainResource::Stone,
        TerrainResource::Wood,
        TerrainResource::Gem,
        TerrainResource::Ore,
    ] {
        let count = rtsim_max_resources[resource];
        if count > 0 {
            summarized.insert(format!("{resource:?}"), count);
        }
    }
    summarized
}

fn load_fixed_overlay_fixture() -> FixedOverlayFixture {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("world crate should live under repo root")
        .to_path_buf();
    let fixture_path = repo_root.join(FIXED_OVERLAY_FIXTURE_PATH);
    let fixture_text = fs::read_to_string(&fixture_path).unwrap_or_else(|error| {
        panic!(
            "failed to read fixed overlay fixture {}: {error}",
            fixture_path.display()
        )
    });
    ron::from_str(&fixture_text).unwrap_or_else(|error| {
        panic!(
            "failed to parse fixed overlay fixture {}: {error}",
            fixture_path.display()
        )
    })
}

fn load_fixed_rtsim_resource_fixture() -> FixedRtsimResourceFixture {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("world crate should live under repo root")
        .to_path_buf();
    let fixture_path = repo_root.join(FIXED_RTSIM_RESOURCE_FIXTURE_PATH);
    let fixture_text = fs::read_to_string(&fixture_path).unwrap_or_else(|error| {
        panic!(
            "failed to read fixed rtsim resource fixture {}: {error}",
            fixture_path.display()
        )
    });
    ron::from_str(&fixture_text).unwrap_or_else(|error| {
        panic!(
            "failed to parse fixed rtsim resource fixture {}: {error}",
            fixture_path.display()
        )
    })
}

fn build_rtsim_resource_samples(
    world: &super::World,
    index_ref: super::IndexRef,
    target_samples: usize,
    edge_margin_chunks: i32,
    max_probes: usize,
    context: &RuntimeAuditContext,
    fixture: &FixedRtsimResourceFixture,
) -> Vec<AuditRtsimResourceSample> {
    let size = world.sim().get_size();
    let full_density_fractions = fixed_full_density_rtsim_fractions();
    let fixed_fixture_fractions = fixture.fractions.to_enum_map();
    let mut candidates = Vec::new();
    let mut probes = 0usize;

    for chunk_pos in runtime_probe_order(size) {
        if probes >= max_probes {
            break;
        }
        probes += 1;

        if !is_interior_probe_candidate(size, edge_margin_chunks, chunk_pos) {
            continue;
        }

        let full_density_runtime = build_rtsim_runtime_variant(
            world,
            index_ref,
            chunk_pos,
            &full_density_fractions,
            context.time_context(),
            "baseline_night_full_density_runtime_chunk",
        );
        let selection_score = full_density_runtime
            .rtsim_max_resources
            .values()
            .copied()
            .sum::<usize>();
        let resource_kind_count = full_density_runtime.rtsim_max_resources.len();
        if selection_score == 0 {
            continue;
        }

        candidates.push(SelectedRtsimResourceSample {
            chunk_pos,
            selection_score,
            resource_kind_count,
            full_density_runtime,
        });
    }

    candidates.sort_by(|left, right| {
        right
            .selection_score
            .cmp(&left.selection_score)
            .then_with(|| right.resource_kind_count.cmp(&left.resource_kind_count))
            .then_with(|| left.chunk_pos.x.cmp(&right.chunk_pos.x))
            .then_with(|| left.chunk_pos.y.cmp(&right.chunk_pos.y))
    });

    candidates
        .into_iter()
        .take(target_samples)
        .map(|selected| {
            let fixed_fixture_runtime = build_rtsim_runtime_variant(
                world,
                index_ref,
                selected.chunk_pos,
                &fixed_fixture_fractions,
                context.time_context(),
                "baseline_night_thinned_runtime_chunk",
            );

            AuditRtsimResourceSample {
                chunk_pos: [selected.chunk_pos.x, selected.chunk_pos.y],
                selection_score: selected.selection_score,
                resource_kind_count: selected.resource_kind_count,
                baseline_night_full_density_runtime_chunk: selected.full_density_runtime.chunk,
                baseline_night_full_density_runtime_supplement: summarize_rtsim_runtime_supplement(
                    &selected.full_density_runtime.supplement,
                    &selected.full_density_runtime.rtsim_max_resources,
                ),
                baseline_night_full_density_rtsim_resource_block_counts: selected
                    .full_density_runtime
                    .rtsim_resource_block_counts,
                baseline_night_thinned_runtime_chunk: fixed_fixture_runtime.chunk,
                baseline_night_thinned_runtime_supplement: summarize_rtsim_runtime_supplement(
                    &fixed_fixture_runtime.supplement,
                    &fixed_fixture_runtime.rtsim_max_resources,
                ),
                baseline_night_thinned_rtsim_resource_block_counts: fixed_fixture_runtime
                    .rtsim_resource_block_counts,
            }
        })
        .collect()
}

fn build_rtsim_runtime_variant(
    world: &super::World,
    index_ref: super::IndexRef,
    chunk_pos: Vec2<i32>,
    rtsim_resource_fractions: &EnumMap<TerrainResource, f32>,
    time_context: (TimeOfDay, Calendar),
    variant_mode: &str,
) -> AuditContextualRuntimeSample {
    let (runtime_chunk, supplement) = world
        .generate_chunk(
            index_ref,
            chunk_pos,
            Some(*rtsim_resource_fractions),
            || false,
            Some(time_context),
        )
        .expect("runtime audit rtsim chunk generation should succeed");
    AuditContextualRuntimeSample {
        chunk: summarize_runtime_variant(
            variant_mode,
            &runtime_chunk,
            OverlayApplySummary::default(),
        ),
        supplement: summarize_runtime_supplement(&supplement),
        rtsim_max_resources: summarize_rtsim_max_resources(&supplement.rtsim_max_resources),
        rtsim_resource_block_counts: summarize_runtime_chunk_resources(&runtime_chunk),
    }
}

fn fixed_full_density_rtsim_fractions() -> EnumMap<TerrainResource, f32> {
    EnumMap::from_fn(|_| 1.0)
}

fn summarize_runtime_chunk_resources(chunk: &TerrainChunk) -> BTreeMap<String, u64> {
    let lo = Vec3::new(0, 0, chunk.get_min_z());
    let hi = TerrainChunkSize::RECT_SIZE.as_().with_z(chunk.get_max_z());
    let mut summarized = BTreeMap::new();

    for (_, block) in chunk.vol_iter(lo, hi) {
        if let Some(resource) = block.get_rtsim_resource() {
            *summarized.entry(format!("{resource:?}")).or_insert(0) += 1;
        }
    }

    summarized
}

fn runtime_probe_order(size: Vec2<u32>) -> Vec<Vec2<i32>> {
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

fn runtime_probe_edge_margin(size: Vec2<u32>) -> i32 {
    size.x.min(size.y).clamp(32, 512) as i32 / 32
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

fn apply_fixed_overlay_fixture(
    chunk: &mut TerrainChunk,
    fixture: &FixedOverlayFixture,
) -> OverlayApplySummary {
    let mut summary = OverlayApplySummary::default();
    for operation in &fixture.operations {
        if apply_overlay_operation(chunk, operation) {
            summary.applied += 1;
        } else {
            summary.skipped += 1;
        }
    }
    summary
}

fn apply_overlay_operation(chunk: &mut TerrainChunk, operation: &FixedOverlayOperation) -> bool {
    let rel_xy = Vec2::new(operation.rel_xy[0] as i32, operation.rel_xy[1] as i32);
    if rel_xy.x < 0
        || rel_xy.y < 0
        || rel_xy.x >= TerrainChunkSize::RECT_SIZE.x as i32
        || rel_xy.y >= TerrainChunkSize::RECT_SIZE.y as i32
    {
        return false;
    }

    let Some(target) = resolve_overlay_target(chunk, rel_xy, &operation.target) else {
        return false;
    };

    let block = match &operation.block {
        FixedOverlayBlock::Solid { kind, color } => {
            Block::new(*kind, Rgb::new(color[0], color[1], color[2]))
        },
        FixedOverlayBlock::Air { sprite } => Block::air(*sprite),
        FixedOverlayBlock::Water { sprite } => Block::water(*sprite),
    };

    chunk.set(target, block).is_ok()
}

fn resolve_overlay_target(
    chunk: &TerrainChunk,
    rel_xy: Vec2<i32>,
    target: &FixedOverlayTarget,
) -> Option<Vec3<i32>> {
    match target {
        FixedOverlayTarget::TopMostNonAir => {
            for z in (chunk.get_min_z()..chunk.get_max_z()).rev() {
                let pos = rel_xy.with_z(z);
                if chunk
                    .get(pos)
                    .is_ok_and(|block| block.kind() != BlockKind::Air)
                {
                    return Some(pos);
                }
            }
            None
        },
    }
}

pub(super) fn summarize_entity_spawns(entity_spawns: &[EntitySpawn]) -> Vec<String> {
    let mut entity_signatures = Vec::new();
    for entity_spawn in entity_spawns {
        match entity_spawn {
            EntitySpawn::Entity(entity) => collect_entity_signature(entity, &mut entity_signatures),
            EntitySpawn::Group(group) => {
                for entity in group {
                    collect_entity_signature(entity, &mut entity_signatures);
                }
            },
        }
    }
    entity_signatures.sort();
    entity_signatures
}

fn collect_entity_signature(entity: &EntityInfo, entity_signatures: &mut Vec<String>) {
    entity_signatures.push(format!(
        "pos_bits={},{},{};body={:?};alignment={:?};scale_bits={};inventory={};pets={};rider={};\
         special={};agency={};no_flee={}",
        entity.pos.x.to_bits(),
        entity.pos.y.to_bits(),
        entity.pos.z.to_bits(),
        entity.body,
        entity.alignment,
        entity.scale.to_bits(),
        entity.inventory.len(),
        entity.pets.len(),
        entity.rider.is_some(),
        entity.special_entity.is_some(),
        entity.has_agency,
        entity.no_flee,
    ));

    for pet in &entity.pets {
        collect_entity_signature(pet, entity_signatures);
    }

    if let Some(rider) = &entity.rider {
        collect_entity_signature(rider, entity_signatures);
    }
}
