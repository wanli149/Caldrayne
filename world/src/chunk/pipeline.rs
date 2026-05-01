use super::runtime::{apply_rtsim_resource_thinning, finalize_world_runtime_chunk};
use crate::{
    BlockGen, Canvas, CanvasInfo, IndexRef, World, block, layer, sim,
    util::{Grid, seed_expan},
};
use common::{
    calendar::Calendar,
    generation::{ChunkSupplement, EntitySpawn},
    resources::TimeOfDay,
    rtsim::TerrainResource,
    terrain::{Block, BlockKind, SpriteKind, TerrainChunk, TerrainChunkMeta, TerrainChunkSize},
    vol::{RectVolSize, WriteVol},
};
use enum_map::EnumMap;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use vek::{Vec2, Vec3};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChunkGenerationMode {
    StaticSnapshot,
    RuntimeFinalized,
}

pub(crate) enum ChunkGenerationOutput {
    StaticSnapshot(TerrainChunk),
    RuntimeFinalized {
        chunk: TerrainChunk,
        supplement: ChunkSupplement,
    },
}

pub(crate) struct ChunkBaseBuild<'a> {
    chunk_pos: Vec2<i32>,
    chunk_wpos2d: Vec2<i32>,
    chunk_center_wpos2d: Vec2<i32>,
    grid_border: i32,
    zcache_grid: Grid<Option<block::ZCache<'a>>>,
    chunk: TerrainChunk,
}

pub(crate) struct StaticChunkArtifacts {
    entity_spawns: Vec<EntitySpawn>,
    rtsim_resource_blocks: Vec<Vec3<i32>>,
}

struct RuntimeChunkArtifacts {
    supplement: ChunkSupplement,
    rtsim_resource_blocks: Vec<Vec3<i32>>,
}

struct RuntimeChunkThinningState {
    supplement: ChunkSupplement,
    rtsim_resource_blocks: Vec<Vec3<i32>>,
    runtime_rng: ChaCha8Rng,
}

struct RuntimeFinalizeContext<'a> {
    time: Option<&'a (TimeOfDay, Calendar)>,
    runtime_rng: ChaCha8Rng,
    rtsim_resource_fractions: Option<EnumMap<TerrainResource, f32>>,
}

pub(crate) struct WorldRuntimeContext<'a> {
    time: Option<&'a (TimeOfDay, Calendar)>,
}

impl<'a> WorldRuntimeContext<'a> {
    fn new(time: Option<&'a (TimeOfDay, Calendar)>) -> Self { Self { time } }

    fn calendar(&self) -> Option<&'a Calendar> { self.time.map(|(_, calendar)| calendar) }

    fn time(&self) -> Option<&'a (TimeOfDay, Calendar)> { self.time }
}

pub(crate) struct ChunkGenerationContext<'a> {
    world_runtime: WorldRuntimeContext<'a>,
    rtsim_resource_fractions: Option<EnumMap<TerrainResource, f32>>,
}

impl<'a> ChunkGenerationContext<'a> {
    pub(crate) fn static_snapshot(time: Option<&'a (TimeOfDay, Calendar)>) -> Self {
        Self {
            world_runtime: WorldRuntimeContext::new(time),
            rtsim_resource_fractions: None,
        }
    }

    pub(crate) fn runtime_finalized(
        time: Option<&'a (TimeOfDay, Calendar)>,
        rtsim_resource_fractions: Option<EnumMap<TerrainResource, f32>>,
    ) -> Self {
        Self {
            world_runtime: WorldRuntimeContext::new(time),
            rtsim_resource_fractions,
        }
    }

    fn calendar(&self) -> Option<&'a Calendar> { self.world_runtime.calendar() }
}

impl StaticChunkArtifacts {
    fn into_runtime_artifacts(self) -> RuntimeChunkArtifacts {
        RuntimeChunkArtifacts {
            supplement: ChunkSupplement {
                entity_spawns: self.entity_spawns,
                rtsim_max_resources: Default::default(),
            },
            rtsim_resource_blocks: self.rtsim_resource_blocks,
        }
    }
}

#[expect(clippy::result_unit_err)]
pub(crate) fn generate_chunk_with_mode(
    world: &World,
    index: IndexRef,
    chunk_pos: Vec2<i32>,
    generation_context: ChunkGenerationContext<'_>,
    mut should_abort: impl FnMut() -> bool,
    generation_mode: ChunkGenerationMode,
) -> Result<ChunkGenerationOutput, ()> {
    let calendar = generation_context.calendar();
    let mut sampler = world.sample_blocks();

    let generation_anchor = match world.sim.generation_chunk_anchor(chunk_pos) {
        Some(generation_anchor) => generation_anchor,
        None => {
            return Ok(
                world.default_chunk_output_for_missing_runtime_chunk_product(generation_mode)
            );
        },
    };
    let (base_build, static_artifacts) = build_chunk_static_stage(
        world,
        index,
        chunk_pos,
        generation_anchor.base_z,
        generation_anchor.sim_chunk,
        &mut sampler,
        calendar,
        &mut should_abort,
    )?;
    Ok(finalize_chunk_generation_mode(
        world,
        generation_mode,
        base_build,
        generation_anchor.sim_chunk,
        static_artifacts,
        index,
        generation_context,
    ))
}

#[expect(clippy::result_unit_err)]
fn build_chunk_static_stage<'a>(
    world: &World,
    index: IndexRef<'a>,
    chunk_pos: Vec2<i32>,
    base_z: i32,
    sim_chunk: &sim::SimChunk,
    sampler: &mut BlockGen<'a>,
    calendar: Option<&'a Calendar>,
    should_abort: &mut impl FnMut() -> bool,
) -> Result<(ChunkBaseBuild<'a>, StaticChunkArtifacts), ()> {
    let mut base_build = build_base_chunk_volume(
        world,
        index,
        chunk_pos,
        base_z,
        sim_chunk,
        sampler,
        calendar,
        should_abort,
    )?;
    let static_artifacts = apply_static_passes_and_extract_artifacts(
        world,
        &mut base_build,
        sim_chunk,
        index,
        calendar,
    );

    Ok((base_build, static_artifacts))
}

fn finalize_chunk_generation_mode<'a>(
    world: &World,
    generation_mode: ChunkGenerationMode,
    mut base_build: ChunkBaseBuild<'a>,
    sim_chunk: &sim::SimChunk,
    static_artifacts: StaticChunkArtifacts,
    index: IndexRef<'a>,
    generation_context: ChunkGenerationContext<'_>,
) -> ChunkGenerationOutput {
    match generation_mode {
        ChunkGenerationMode::RuntimeFinalized => {
            let supplement = run_runtime_finalizers(
                world,
                &mut base_build,
                sim_chunk,
                static_artifacts,
                index,
                generation_context,
            );
            ChunkGenerationOutput::RuntimeFinalized {
                chunk: base_build.chunk,
                supplement,
            }
        },
        ChunkGenerationMode::StaticSnapshot => {
            base_build.chunk.defragment();
            ChunkGenerationOutput::StaticSnapshot(base_build.chunk)
        },
    }
}

#[expect(clippy::result_unit_err)]
fn build_base_chunk_volume<'a>(
    world: &World,
    index: IndexRef<'a>,
    chunk_pos: Vec2<i32>,
    base_z: i32,
    sim_chunk: &sim::SimChunk,
    sampler: &mut BlockGen<'a>,
    calendar: Option<&'a Calendar>,
    should_abort: &mut impl FnMut() -> bool,
) -> Result<ChunkBaseBuild<'a>, ()> {
    let chunk_wpos2d = chunk_pos * TerrainChunkSize::RECT_SIZE.map(|e| e as i32);
    let chunk_center_wpos2d = chunk_wpos2d + TerrainChunkSize::RECT_SIZE.map(|e| e as i32 / 2);
    let grid_border = 4;
    let zcache_grid = Grid::populate_from(
        TerrainChunkSize::RECT_SIZE.map(|e| e as i32) + grid_border * 2,
        |offs| sampler.get_z_cache(chunk_wpos2d - grid_border + offs, index, calendar),
    );

    let air = Block::air(SpriteKind::Empty);
    let stone = Block::new(
        BlockKind::Rock,
        zcache_grid
            .get(grid_border + TerrainChunkSize::RECT_SIZE.map(|e| e as i32) / 2)
            .and_then(|zcache| zcache.as_ref())
            .map(|zcache| zcache.sample.stone_col)
            .unwrap_or_else(|| index.colors.deep_stone_color.into()),
    );
    let meta = TerrainChunkMeta::new(
        sim_chunk.get_location_name(&index.sites, &world.civs.pois, chunk_center_wpos2d),
        sim_chunk.get_biome(),
        sim_chunk.alt,
        sim_chunk.tree_density,
        sim_chunk.river.is_river(),
        sim_chunk.river.near_water(),
        sim_chunk.river.velocity,
        sim_chunk.temp,
        sim_chunk.humidity,
        sim_chunk
            .sites
            .iter()
            .filter(|id| {
                index.sites[**id]
                    .origin
                    .as_::<f32>()
                    .distance_squared(chunk_center_wpos2d.as_::<f32>())
                    <= index.sites[**id].radius().powi(2)
            })
            .min_by_key(|id| {
                index.sites[**id]
                    .origin
                    .as_::<i64>()
                    .distance_squared(chunk_center_wpos2d.as_::<i64>())
            })
            .map(|id| index.sites[*id].meta().unwrap_or_default()),
        world.sim.approx_chunk_terrain_normal(chunk_pos),
        sim_chunk.rockiness,
        sim_chunk.cliff_height,
    );

    let mut chunk = TerrainChunk::new(base_z, stone, air, meta);

    for y in 0..TerrainChunkSize::RECT_SIZE.y as i32 {
        for x in 0..TerrainChunkSize::RECT_SIZE.x as i32 {
            if should_abort() {
                return Err(());
            };

            let offs = Vec2::new(x, y);

            let z_cache = match zcache_grid.get(grid_border + offs) {
                Some(Some(z_cache)) => z_cache,
                _ => continue,
            };

            let (min_z, max_z) = z_cache.get_z_limits();

            (base_z..min_z as i32).for_each(|z| {
                let _ = chunk.set(Vec3::new(x, y, z), stone);
            });

            (min_z as i32..max_z as i32).for_each(|z| {
                let lpos = Vec3::new(x, y, z);
                let wpos = Vec3::from(chunk_wpos2d) + lpos;

                if let Some(block) = sampler.get_with_z_cache(wpos, Some(z_cache)) {
                    let _ = chunk.set(lpos, block);
                }
            });
        }
    }

    Ok(ChunkBaseBuild {
        chunk_pos,
        chunk_wpos2d,
        chunk_center_wpos2d,
        grid_border,
        zcache_grid,
        chunk,
    })
}

fn apply_static_passes_and_extract_artifacts<'a>(
    world: &World,
    base_build: &mut ChunkBaseBuild<'a>,
    sim_chunk: &sim::SimChunk,
    index: IndexRef<'a>,
    calendar: Option<&'a Calendar>,
) -> StaticChunkArtifacts {
    let static_rng_seed = seed_expan::diffuse_mult(&[
        world.sim.seed,
        base_build.chunk_pos.x as u32,
        base_build.chunk_pos.y as u32,
        0x5354_4154,
    ]);
    let mut static_rng = ChaCha8Rng::from_seed(seed_expan::rng_state(static_rng_seed));

    let mut canvas = Canvas {
        info: CanvasInfo {
            chunk_pos: base_build.chunk_pos,
            wpos: base_build.chunk_wpos2d,
            column_grid: &base_build.zcache_grid,
            column_grid_border: base_build.grid_border,
            chunks: &world.sim,
            index,
            chunk: sim_chunk,
            calendar,
        },
        chunk: &mut base_build.chunk,
        entity_spawns: Vec::new(),
        rtsim_resource_blocks: Vec::new(),
    };

    if index.features.train_tracks {
        layer::apply_trains_to(
            &mut canvas,
            &world.sim,
            sim_chunk,
            base_build.chunk_center_wpos2d,
        );
    }

    if index.features.caverns {
        layer::apply_caverns_to(&mut canvas, &mut static_rng);
    }
    if index.features.caves {
        layer::apply_caves_to(&mut canvas, &mut static_rng);
    }
    if index.features.rocks {
        layer::apply_rocks_to(&mut canvas, &mut static_rng);
    }
    if index.features.shrubs {
        layer::apply_shrubs_to(&mut canvas, &mut static_rng);
    }
    if index.features.trees {
        layer::apply_trees_to(&mut canvas, &mut static_rng, calendar);
    }
    if index.features.scatter {
        layer::apply_scatter_to(&mut canvas, &mut static_rng, calendar);
    }
    if index.features.paths {
        layer::apply_paths_to(&mut canvas);
    }
    if index.features.spots {
        layer::apply_spots_to(&mut canvas, &mut static_rng);
    }

    sim_chunk
        .sites
        .iter()
        .for_each(|site| index.sites[*site].render(&mut canvas, &mut static_rng));

    StaticChunkArtifacts {
        rtsim_resource_blocks: std::mem::take(&mut canvas.rtsim_resource_blocks),
        entity_spawns: std::mem::take(&mut canvas.entity_spawns),
    }
}

fn run_runtime_finalizers<'a>(
    world: &World,
    base_build: &mut ChunkBaseBuild<'a>,
    sim_chunk: &sim::SimChunk,
    static_artifacts: StaticChunkArtifacts,
    index: IndexRef<'a>,
    generation_context: ChunkGenerationContext<'_>,
) -> ChunkSupplement {
    let runtime_finalize_context =
        prepare_runtime_finalize_context(world, base_build, generation_context);
    let runtime_artifacts = static_artifacts.into_runtime_artifacts();
    let RuntimeFinalizeContext {
        time,
        runtime_rng,
        rtsim_resource_fractions,
    } = runtime_finalize_context;
    let runtime_chunk_thinning_state = finalize_runtime_chunk_artifacts(
        base_build,
        sim_chunk,
        runtime_artifacts,
        index,
        time,
        runtime_rng,
    );
    finish_runtime_chunk_supplement(
        base_build,
        runtime_chunk_thinning_state,
        rtsim_resource_fractions,
    )
}

fn prepare_runtime_finalize_context<'a>(
    world: &World,
    base_build: &ChunkBaseBuild<'_>,
    generation_context: ChunkGenerationContext<'a>,
) -> RuntimeFinalizeContext<'a> {
    let ChunkGenerationContext {
        world_runtime,
        rtsim_resource_fractions,
    } = generation_context;
    let time = world_runtime.time();
    let runtime_time_bits = time
        .as_ref()
        .map(|(time_of_day, _)| time_of_day.day().to_bits())
        .unwrap_or_default();
    let runtime_calendar_mask = time
        .as_ref()
        .map(|(_, calendar)| {
            calendar
                .events()
                .fold(0u32, |mask, event| mask | (1u32 << (*event as u32)))
        })
        .unwrap_or_default();
    let runtime_rng_seed = seed_expan::diffuse_mult(&[
        world.sim.seed,
        base_build.chunk_pos.x as u32,
        base_build.chunk_pos.y as u32,
        runtime_time_bits as u32,
        (runtime_time_bits >> 32) as u32,
        runtime_calendar_mask,
        0x5255_4E54,
    ]);
    let runtime_rng = ChaCha8Rng::from_seed(seed_expan::rng_state(runtime_rng_seed));

    RuntimeFinalizeContext {
        time,
        runtime_rng,
        rtsim_resource_fractions,
    }
}

fn finalize_runtime_chunk_artifacts<'a>(
    base_build: &mut ChunkBaseBuild<'a>,
    sim_chunk: &sim::SimChunk,
    runtime_artifacts: RuntimeChunkArtifacts,
    index: IndexRef<'a>,
    time: Option<&(TimeOfDay, Calendar)>,
    mut runtime_rng: ChaCha8Rng,
) -> RuntimeChunkThinningState {
    let RuntimeChunkArtifacts {
        mut supplement,
        rtsim_resource_blocks,
    } = runtime_artifacts;
    finalize_world_runtime_chunk(
        &mut base_build.chunk,
        sim_chunk,
        &mut supplement,
        &mut runtime_rng,
        base_build.chunk_wpos2d,
        &base_build.zcache_grid,
        base_build.grid_border,
        index,
        time,
    );

    RuntimeChunkThinningState {
        supplement,
        rtsim_resource_blocks,
        runtime_rng,
    }
}

fn finish_runtime_chunk_supplement(
    base_build: &mut ChunkBaseBuild<'_>,
    runtime_chunk_thinning_state: RuntimeChunkThinningState,
    rtsim_resource_fractions: Option<EnumMap<TerrainResource, f32>>,
) -> ChunkSupplement {
    let RuntimeChunkThinningState {
        mut supplement,
        rtsim_resource_blocks,
        mut runtime_rng,
    } = runtime_chunk_thinning_state;
    supplement.rtsim_max_resources = apply_rtsim_resource_thinning(
        &mut base_build.chunk,
        rtsim_resource_blocks,
        rtsim_resource_fractions,
        &mut runtime_rng,
        base_build.chunk_wpos2d,
    );

    supplement
}
