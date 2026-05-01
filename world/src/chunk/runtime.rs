use crate::{IndexRef, block, layer, sim, site::SpawnRules, util::Grid};
use common::{
    calendar::Calendar,
    generation::{ChunkSupplement, EntityInfo, EntitySpawn, SpecialEntity},
    resources::TimeOfDay,
    rtsim::TerrainResource,
    terrain::{Block, TerrainChunk, TerrainChunkSize},
    vol::{ReadVol, RectVolSize, WriteVol},
};
use enum_map::EnumMap;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;
use vek::{Vec2, Vec3};

pub(crate) fn finalize_world_runtime_chunk(
    chunk: &mut TerrainChunk,
    sim_chunk: &sim::SimChunk,
    supplement: &mut ChunkSupplement,
    runtime_rng: &mut ChaCha8Rng,
    chunk_wpos2d: Vec2<i32>,
    zcache_grid: &Grid<Option<block::ZCache<'_>>>,
    grid_border: i32,
    index: IndexRef<'_>,
    time: Option<&(TimeOfDay, Calendar)>,
) {
    apply_world_runtime_supplement(
        chunk,
        supplement,
        runtime_rng,
        chunk_wpos2d,
        zcache_grid,
        grid_border,
        index,
        sim_chunk,
        time,
    );

    // World-owned runtime finalize stops after supplement expansion and chunk
    // compaction. Server-side rtsim thinning is applied as a distinct tail step.
    chunk.defragment();
}

fn apply_world_runtime_supplement(
    chunk: &TerrainChunk,
    supplement: &mut ChunkSupplement,
    runtime_rng: &mut ChaCha8Rng,
    chunk_wpos2d: Vec2<i32>,
    zcache_grid: &Grid<Option<block::ZCache<'_>>>,
    grid_border: i32,
    index: IndexRef<'_>,
    sim_chunk: &sim::SimChunk,
    time: Option<&(TimeOfDay, Calendar)>,
) {
    let sample_get = |offs| {
        zcache_grid
            .get(grid_border + offs)
            .and_then(Option::as_ref)
            .map(|zc| &zc.sample)
    };

    let gen_entity_pos = |runtime_rng: &mut ChaCha8Rng| {
        let lpos2d =
            TerrainChunkSize::RECT_SIZE.map(|sz| runtime_rng.random::<u32>().rem_euclid(sz) as i32);
        let mut lpos = Vec3::new(
            lpos2d.x,
            lpos2d.y,
            sample_get(lpos2d).map(|s| s.alt as i32 - 32).unwrap_or(0),
        );

        while let Some(block) = chunk.get(lpos).ok().copied().filter(Block::is_solid) {
            lpos.z += block.solid_height().ceil() as i32;
        }

        (Vec3::from(chunk_wpos2d) + lpos).map(|e: i32| e as f32) + 0.5
    };

    if sim_chunk.contains_waypoint {
        let waypoint_pos = gen_entity_pos(runtime_rng);
        if sim_chunk
            .sites
            .iter()
            .map(|site| index.sites[*site].spawn_rules(waypoint_pos.xy().as_()))
            .fold(SpawnRules::default(), |a, b| a.combine(b))
            .waypoints
        {
            supplement.add_entity_spawn(EntitySpawn::Entity(Box::new(
                EntityInfo::at(waypoint_pos).into_special(SpecialEntity::Waypoint),
            )));
        }
    }

    // Apply layer supplement
    layer::wildlife::apply_wildlife_supplement(
        runtime_rng,
        chunk_wpos2d,
        sample_get,
        chunk,
        index,
        sim_chunk,
        supplement,
        time,
    );

    // Apply site supplementary information
    sim_chunk.sites.iter().for_each(|site| {
        index.sites[*site].apply_supplement(runtime_rng, chunk_wpos2d, supplement)
    });
}

pub(crate) fn apply_rtsim_resource_thinning(
    chunk: &mut TerrainChunk,
    mut rtsim_resource_blocks: Vec<Vec3<i32>>,
    rtsim_resource_fractions: Option<EnumMap<TerrainResource, f32>>,
    runtime_rng: &mut ChaCha8Rng,
    chunk_wpos2d: Vec2<i32>,
) -> EnumMap<TerrainResource, usize> {
    let mut rtsim_max_resources = EnumMap::default();

    // Before we finish, we check candidate rtsim resource blocks, deduplicating
    // positions and only keeping those that actually do have resources.
    // Although this looks potentially very expensive, only blocks that are rtsim
    // resources (i.e: a relatively small number of sprites) are processed here.
    if let Some(rtsim_resource_fractions) = rtsim_resource_fractions {
        rtsim_resource_blocks.sort_unstable_by_key(|pos| pos.into_array());
        rtsim_resource_blocks.dedup();
        for wpos in rtsim_resource_blocks.iter().copied() {
            let _ = chunk.map(wpos - chunk_wpos2d.with_z(0), |block| {
                if let Some(res) = block.get_rtsim_resource() {
                    // Note: this represents the upper limit, not the actual number spanwed, so
                    // we increment this before deciding whether we're going to spawn the
                    // resource.
                    rtsim_max_resources[res] += 1;

                    debug_assert!(
                        0.0 <= rtsim_resource_fractions[res]
                            && rtsim_resource_fractions[res] <= 1.0,
                        "The rtsim resource {res:?} has the value '{}', which is not in the \
                         expected range of 0.0..=1.0. When registering a block with the sprite \
                         `{:?}`, with the damage `{:?}`.",
                        rtsim_resource_fractions[res],
                        block.get_sprite(),
                        block.get_attr::<common::terrain::sprite::Damage>().ok(),
                    );

                    // Throw a dice to determine whether this resource should actually spawn
                    // TODO: Don't throw a dice, try to generate the *exact* correct number
                    if runtime_rng.random_bool(rtsim_resource_fractions[res].clamp(0.0, 1.0) as f64)
                    {
                        block
                    } else {
                        block.into_vacant()
                    }
                } else {
                    block
                }
            });
        }
    }

    rtsim_max_resources
}
