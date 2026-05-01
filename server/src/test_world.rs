use common::{
    calendar::Calendar,
    generation::ChunkSupplement,
    resources::TimeOfDay,
    rtsim::TerrainResource,
    terrain::{
        Block, BlockKind, MapSizeLg, SpriteKind, TerrainChunk, TerrainChunkMeta, TerrainChunkSize,
    },
    vol::RectVolSize,
};
use common_net::msg::world_msg;
use enum_map::EnumMap;
use rand::{prelude::*, rngs::SmallRng};
use std::time::Duration;
use vek::*;

const DEFAULT_WORLD_CHUNKS_LG: MapSizeLg =
    if let Ok(map_size_lg) = MapSizeLg::new(Vec2 { x: 8, y: 8 }) {
        map_size_lg
    } else {
        panic!("Default world chunk size does not satisfy required invariants.");
    };

pub struct World;

#[derive(Clone)]
pub struct IndexOwned;

#[derive(Clone, Copy)]
#[expect(dead_code)]
pub struct IndexRef<'a>(&'a IndexOwned);

impl IndexOwned {
    pub fn reload_if_changed<R>(&mut self, _reload: impl FnOnce(&mut Self) -> R) -> Option<R> {
        None
    }

    pub fn as_index_ref(&self) -> IndexRef<'_> { IndexRef(self) }
}

impl World {
    pub fn generate(_seed: u32) -> (Self, IndexOwned) { (Self, IndexOwned) }

    pub fn tick(&self, _dt: Duration) {}

    #[inline(always)]
    pub const fn map_size_lg(&self) -> MapSizeLg { DEFAULT_WORLD_CHUNKS_LG }

    pub fn query_chunk_key_aabr(&self) -> Aabr<i32> {
        Aabr {
            min: Vec2::zero(),
            max: self.map_size_lg().chunks().map(i32::from) - 1,
        }
    }

    pub fn runtime_chunk_product_key_aabr(&self) -> Aabr<i32> {
        Aabr {
            min: Vec2::one(),
            max: self.map_size_lg().chunks().map(i32::from) - Vec2::new(3, 3),
        }
    }

    pub fn runtime_topology_descriptor(&self) -> world_msg::RuntimeTopologyDescriptor {
        world_msg::RuntimeTopologyDescriptor {
            topology_id: "bounded_plane_v1".to_owned(),
            query_chunk_key_aabr: self.query_chunk_key_aabr(),
            runtime_chunk_product_key_aabr: self.runtime_chunk_product_key_aabr(),
            missing_world_bounds_policy:
                world_msg::MissingWorldBoundsPolicy::BoundedOceanDefaultChunk,
        }
    }

    pub fn default_chunk_for_missing_world_bounds(&self) -> TerrainChunk { TerrainChunk::water(0) }

    pub fn generate_oob_chunk(&self) -> TerrainChunk {
        self.default_chunk_for_missing_world_bounds()
    }

    pub fn generate_chunk(
        &self,
        _index: IndexRef,
        chunk_pos: Vec2<i32>,
        _rtsim_resources: Option<EnumMap<TerrainResource, f32>>,
        // TODO: misleading name
        mut _should_continue: impl FnMut() -> bool,
        _time: Option<(TimeOfDay, Calendar)>,
    ) -> Result<(TerrainChunk, ChunkSupplement), ()> {
        let (x, y) = chunk_pos.map(|e| e.to_le_bytes()).into_tuple();
        let mut rng = SmallRng::from_seed([
            x[0], x[1], x[2], x[3], y[0], y[1], y[2], y[3], x[0], x[1], x[2], x[3], y[0], y[1],
            y[2], y[3], x[0], x[1], x[2], x[3], y[0], y[1], y[2], y[3], x[0], x[1], x[2], x[3],
            y[0], y[1], y[2], y[3],
        ]);
        let height = rng.random::<i32>() % 8;

        let supplement = ChunkSupplement::default();

        Ok((
            TerrainChunk::new(
                if rng.random::<u8>() < 64 { height } else { 0 },
                Block::new(BlockKind::Grass, Rgb::new(11, 102, 35)),
                Block::air(SpriteKind::Empty),
                TerrainChunkMeta::void(),
            ),
            supplement,
        ))
    }

    pub fn get_center(&self) -> Vec2<u32> {
        // FIXME: Assumes that TerrainChunkSize::RECT_SIZE.x ==
        // TerrainChunkSize::RECT_SIZE.y
        DEFAULT_WORLD_CHUNKS_LG.chunks().as_::<u32>() / 2 * TerrainChunkSize::RECT_SIZE.x
    }

    pub fn get_location_name(&self, _index: IndexRef, _wpos2d: Vec2<i32>) -> Option<String> {
        // Test world has no locations
        None
    }
}
