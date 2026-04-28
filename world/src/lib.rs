#![expect(
    clippy::option_map_unit_fn,
    clippy::blocks_in_conditions,
    clippy::identity_op,
    clippy::needless_pass_by_ref_mut //until we find a better way for specs
)]
#![expect(clippy::branches_sharing_code)] // TODO: evaluate
#![deny(clippy::clone_on_ref_ptr)]
#![feature(option_zip)]
#![cfg_attr(feature = "simd", feature(portable_simd))]

mod all;
mod block;
pub mod canvas;
pub mod civ;
mod column;
pub mod config;
pub mod index;
pub mod land;
pub mod layer;
pub mod pathfinding;
pub mod recipe;
pub mod sim;
pub mod sim2;
pub mod site;
pub mod util;

// Reexports
pub use crate::{
    canvas::{Canvas, CanvasInfo},
    config::{CONFIG, Features},
    land::Land,
    layer::PathLocals,
};
pub use block::BlockGen;
use civ::WorldCivStage;
pub use column::ColumnSample;
pub use common::terrain::site::{DungeonKindMeta, SettlementKindMeta};
pub use index::{IndexOwned, IndexRef};
use sim::WorldSimStage;

use crate::{
    column::ColumnGen,
    index::Index,
    layer::spot::SpotGenerate,
    site::{SiteKind, SpawnRules},
    util::{Grid, Sampler, seed_expan},
};
use common::{
    assets::{self, BoxedError, FileAsset, load_ron},
    calendar::Calendar,
    comp::Content,
    generation::{ChunkSupplement, EntityInfo, EntitySpawn, SpecialEntity},
    lod,
    map::{Marker, MarkerKind},
    resources::TimeOfDay,
    rtsim::TerrainResource,
    spiral::Spiral2d,
    spot::Spot,
    terrain::{
        BiomeKind, Block, BlockKind, CoordinateConversions, SpriteKind, TerrainChunk,
        TerrainChunkMeta, TerrainChunkSize, TerrainGrid,
    },
    vol::{ReadVol, RectVolSize, WriteVol},
};
use common_base::prof_span;
use common_net::msg::{WorldMapMsg, world_msg};
use enum_map::EnumMap;
use rand::{RngExt, prelude::*};
use rand_chacha::ChaCha8Rng;
use serde::Deserialize;
use std::{borrow::Cow, fmt, time::Duration};
use vek::*;

#[cfg(all(feature = "be-dyn-lib", feature = "use-dyn-lib"))]
compile_error!("Can't use both \"be-dyn-lib\" and \"use-dyn-lib\" features at once");

#[cfg(feature = "use-dyn-lib")]
use {common_dynlib::LoadedLib, lazy_static::lazy_static, std::sync::Arc, std::sync::Mutex};

#[cfg(feature = "use-dyn-lib")]
lazy_static! {
    pub static ref LIB: Arc<Mutex<Option<LoadedLib>>> =
        common_dynlib::init("veloren-world", "world", &[]);
}

#[cfg(feature = "use-dyn-lib")]
pub fn init() { lazy_static::initialize(&LIB); }

#[derive(Debug)]
pub enum Error {
    Other(String),
    CompatEnforce { audit: recipe::CompatAuditV1 },
}

impl Error {
    pub const fn compat_audit(&self) -> Option<recipe::CompatAuditV1> {
        match self {
            Self::CompatEnforce { audit } => Some(*audit),
            Self::Other(_) => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Other(message) => f.write_str(message),
            Self::CompatEnforce { audit } => write!(
                f,
                "world compat enforce rejected load: entry={}, decision={}, failure={}",
                audit.entry.as_str(),
                audit.decision.as_str(),
                audit.failure_kind.as_str()
            ),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug)]
pub enum WorldGenerateStage {
    WorldSimGenerate(WorldSimStage),
    WorldCivGenerate(WorldCivStage),
    EconomySimulation,
    SpotGeneration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ChunkGenerationMode {
    StaticSnapshot,
    RuntimeFinalized,
}

enum ChunkGenerationOutput {
    StaticSnapshot(TerrainChunk),
    RuntimeFinalized {
        chunk: TerrainChunk,
        supplement: ChunkSupplement,
    },
}

struct ChunkBaseBuild<'a> {
    chunk_pos: Vec2<i32>,
    chunk_wpos2d: Vec2<i32>,
    chunk_center_wpos2d: Vec2<i32>,
    grid_border: i32,
    zcache_grid: Grid<Option<block::ZCache<'a>>>,
    chunk: TerrainChunk,
}

struct StaticChunkArtifacts {
    entity_spawns: Vec<EntitySpawn>,
    rtsim_resource_blocks: Vec<Vec3<i32>>,
}

struct RuntimeChunkArtifacts {
    supplement: ChunkSupplement,
    rtsim_resource_blocks: Vec<Vec3<i32>>,
}

struct WorldRuntimeContext<'a> {
    time: Option<&'a (TimeOfDay, Calendar)>,
}

impl<'a> WorldRuntimeContext<'a> {
    fn new(time: Option<&'a (TimeOfDay, Calendar)>) -> Self { Self { time } }

    fn calendar(&self) -> Option<&'a Calendar> { self.time.map(|(_, calendar)| calendar) }

    fn time(&self) -> Option<&'a (TimeOfDay, Calendar)> { self.time }
}

struct ChunkGenerationContext<'a> {
    world_runtime: WorldRuntimeContext<'a>,
    rtsim_resource_fractions: Option<EnumMap<TerrainResource, f32>>,
}

impl<'a> ChunkGenerationContext<'a> {
    fn static_snapshot(time: Option<&'a (TimeOfDay, Calendar)>) -> Self {
        Self {
            world_runtime: WorldRuntimeContext::new(time),
            rtsim_resource_fractions: None,
        }
    }

    fn runtime_finalized(
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

pub struct World {
    sim: sim::WorldSim,
    civs: civ::Civs,
}

const STARTING_SITE_COUNT: usize = 5;
const OPTIMAL_STARTER_TOWN_SIZE: f32 = 30.0;

#[derive(Clone, Copy, Debug)]
pub struct StartingSiteScoreBreakdown {
    pub base_kind_score: f32,
    pub size_score: f32,
    pub position_score: f32,
    pub biome_score: f32,
    pub final_score: f32,
}

#[derive(Clone, Debug)]
pub struct StartingSiteProfile {
    pub site_id: world_msg::SiteId,
    pub name: String,
    pub site_kind: Option<SiteKind>,
    pub settlement_kind: Option<SettlementKindMeta>,
    pub center_biome: Option<BiomeKind>,
    pub center_near_water: Option<bool>,
    pub center: Vec2<i32>,
    pub plot_count: usize,
    pub biome_factor: f32,
}

#[derive(Clone, Debug)]
pub struct StartingSiteCandidate {
    pub profile: StartingSiteProfile,
    pub score: StartingSiteScoreBreakdown,
}

#[derive(Clone, Debug)]
pub struct StartingSiteSelection {
    pub candidates: Vec<StartingSiteCandidate>,
}

impl StartingSiteSelection {
    pub fn selected_site_ids(&self) -> Vec<world_msg::SiteId> {
        self.candidates
            .iter()
            .filter(|candidate| candidate.score.base_kind_score > 0.0)
            .map(|candidate| candidate.profile.site_id)
            .take(STARTING_SITE_COUNT)
            .collect()
    }
}

#[derive(Deserialize)]
pub struct Colors {
    pub deep_stone_color: (u8, u8, u8),
    pub block: block::Colors,
    pub column: column::Colors,
    pub layer: layer::Colors,
}

impl FileAsset for Colors {
    const EXTENSION: &'static str = "ron";

    fn from_bytes(bytes: Cow<[u8]>) -> Result<Self, BoxedError> { load_ron(&bytes) }
}

fn starting_site_settlement_kind(site_kind: Option<SiteKind>) -> Option<SettlementKindMeta> {
    match site_kind.and_then(|kind| kind.meta()) {
        Some(common::terrain::SiteKindMeta::Settlement(settlement_kind)) => Some(settlement_kind),
        _ => None,
    }
}

fn starting_site_base_kind_score(
    site_kind: Option<SiteKind>,
    settlement_kind: Option<SettlementKindMeta>,
) -> f32 {
    match site_kind {
        Some(SiteKind::Refactor) => 2.0,
        _ if settlement_kind.is_some() => 1.0,
        _ => 0.0,
    }
}

fn starting_site_size_score(plots: usize) -> f32 {
    let plots = plots as f32;
    if plots > OPTIMAL_STARTER_TOWN_SIZE {
        1.0 + (1.0 / (1.0 + ((plots - OPTIMAL_STARTER_TOWN_SIZE) / 15.0).powi(3)))
    } else {
        (2.05 / (1.0 + ((OPTIMAL_STARTER_TOWN_SIZE - plots) / 15.0).powi(5))) - 0.05
    }
    .max(0.01)
}

fn starting_site_position_score(center: Vec2<i32>, world_size: Vec2<u32>) -> f32 {
    (10.0
        / (1.0
            + (center
                .map2(world_size, |e, sz| (e as f32 / sz as f32 - 0.5).abs() * 2.0)
                .reduce_partial_max())
            .powi(6)
                * 25.0))
        .max(0.02)
}

impl World {
    pub fn empty() -> (Self, IndexOwned) {
        let index = Index::new(0);
        (
            Self {
                sim: sim::WorldSim::empty(),
                civs: civ::Civs::default(),
            },
            IndexOwned::new(index),
        )
    }

    pub fn generate(
        seed: u32,
        opts: sim::WorldOpts,
        threadpool: &rayon::ThreadPool,
        report_stage: &(dyn Fn(WorldGenerateStage) + Send + Sync),
    ) -> Result<(Self, IndexOwned), Error> {
        prof_span!("World::generate");
        // NOTE: Generating index first in order to quickly fail if the color manifest
        // is broken.
        threadpool.install(|| -> Result<(Self, IndexOwned), Error> {
            let mut index = Index::new(seed);
            let calendar = opts.calendar.clone();

            let mut sim = sim::WorldSim::generate(seed, opts, threadpool, &|stage| {
                report_stage(WorldGenerateStage::WorldSimGenerate(stage))
            })?;

            let civs =
                civ::Civs::generate(seed, &mut sim, &mut index, calendar.as_ref(), &|stage| {
                    report_stage(WorldGenerateStage::WorldCivGenerate(stage))
                });

            report_stage(WorldGenerateStage::EconomySimulation);
            sim2::simulate(&mut index, &mut sim);

            report_stage(WorldGenerateStage::SpotGeneration);
            Spot::generate(&mut sim);

            Ok((Self { sim, civs }, IndexOwned::new(index)))
        })
    }

    pub fn sim(&self) -> &sim::WorldSim { &self.sim }

    pub fn civs(&self) -> &civ::Civs { &self.civs }

    pub fn tick(&self, _dt: Duration) {
        // TODO
    }

    fn starting_site_biome_score(&self, center: Vec2<i32>) -> f32 {
        let mut chunk_scores = 2.0;
        for (chunk, distance) in Spiral2d::with_radius(10).filter_map(|rel_pos| {
            let chunk_pos = center + rel_pos * 2;
            self.sim()
                .get(chunk_pos)
                .zip(Some(rel_pos.as_::<f32>().magnitude()))
        }) {
            let weight = 1.0 / (distance * std::f32::consts::TAU + 1.0);
            let chunk_difficulty =
                20.0 / (20.0 + chunk.get_biome().difficulty().pow(4) as f32 / 5.0);

            chunk_scores *= 1.0 - weight + chunk_difficulty * weight;
        }

        chunk_scores
    }

    pub fn starting_site_profiles(&self, index: IndexRef) -> Vec<StartingSiteProfile> {
        let mut profiles = self
            .civs()
            .sites
            .iter()
            .filter_map(|(_, civ_site)| {
                let site_idx = civ_site.site_tmp?;
                let site = &index.sites[site_idx];
                let (center_biome, center_near_water) = match self.sim().get(civ_site.center) {
                    Some(chunk) => (Some(chunk.get_biome()), Some(chunk.river.near_water())),
                    None => (None, None),
                };

                Some(StartingSiteProfile {
                    site_id: site_idx.id(),
                    name: index.sites[site_idx].name().unwrap_or("").to_string(),
                    site_kind: site.kind,
                    settlement_kind: starting_site_settlement_kind(site.kind),
                    center_biome,
                    center_near_water,
                    center: civ_site.center,
                    plot_count: site.plots().len(),
                    biome_factor: self.starting_site_biome_score(civ_site.center),
                })
            })
            .collect::<Vec<_>>();

        profiles.sort_by_key(|profile| profile.site_id);
        profiles
    }

    fn score_starting_site_profile(
        profile: StartingSiteProfile,
        world_size: Vec2<u32>,
    ) -> StartingSiteCandidate {
        let base_kind_score =
            starting_site_base_kind_score(profile.site_kind, profile.settlement_kind);
        let size_score = if base_kind_score > 0.0 {
            starting_site_size_score(profile.plot_count)
        } else {
            1.0
        };
        let position_score = if base_kind_score > 0.0 {
            starting_site_position_score(profile.center, world_size)
        } else {
            1.0
        };
        let biome_score = if base_kind_score > 0.0 {
            profile.biome_factor
        } else {
            1.0
        };
        let final_score = base_kind_score * size_score * position_score * biome_score;

        StartingSiteCandidate {
            profile,
            score: StartingSiteScoreBreakdown {
                base_kind_score,
                size_score,
                position_score,
                biome_score,
                final_score,
            },
        }
    }

    fn score_starting_site_profiles(
        profiles: Vec<StartingSiteProfile>,
        world_size: Vec2<u32>,
    ) -> Vec<StartingSiteCandidate> {
        let mut candidates = profiles
            .into_iter()
            .map(|profile| Self::score_starting_site_profile(profile, world_size))
            .collect::<Vec<_>>();

        candidates.sort_by(|a, b| {
            b.score
                .final_score
                .total_cmp(&a.score.final_score)
                .then_with(|| a.profile.site_id.cmp(&b.profile.site_id))
        });
        candidates
    }

    pub fn starting_site_selection(&self, index: IndexRef) -> StartingSiteSelection {
        StartingSiteSelection {
            candidates: Self::score_starting_site_profiles(
                self.starting_site_profiles(index),
                self.sim().get_size(),
            ),
        }
    }

    pub fn starting_site_candidates(&self, index: IndexRef) -> Vec<StartingSiteCandidate> {
        self.starting_site_selection(index).candidates
    }

    pub fn get_map_data(&self, index: IndexRef, threadpool: &rayon::ThreadPool) -> WorldMapMsg {
        prof_span!("World::get_map_data");
        threadpool.install(|| {
            let starting_site_selection = self.starting_site_selection(index);
            WorldMapMsg {
                pois: self
                    .civs()
                    .pois
                    .iter()
                    .map(|(_, poi)| world_msg::PoiInfo {
                        name: poi.name.clone(),
                        kind: match &poi.kind {
                            civ::PoiKind::Peak(alt) => world_msg::PoiKind::Peak(*alt),
                            civ::PoiKind::Biome(size) => world_msg::PoiKind::Lake(*size),
                        },
                        wpos: poi.loc * TerrainChunkSize::RECT_SIZE.map(|e| e as i32),
                    })
                    .collect(),
                sites: self
                    .civs()
                    .sites
                    .values()
                    .filter_map(|site| Some((site.kind.marker()?, site)))
                    .map(|(marker, site)| {
                        Marker::at(
                            (site.center * TerrainChunkSize::RECT_SIZE.map(|e| e as i32)).as_(),
                        )
                        .with_kind(marker)
                        .with_site_id(site.site_tmp.map(|i| i.id()))
                        .with_label(site.site_tmp.map(|id| {
                            Content::Plain(index.sites[id].name().unwrap_or("").to_string())
                        }))
                    })
                    .chain(
                        layer::cave::surface_entrances(&Land::from_sim(self.sim()), index)
                            .map(|wpos| Marker::at(wpos.as_()).with_kind(MarkerKind::Cave)),
                    )
                    .collect(),
                possible_starting_sites: starting_site_selection.selected_site_ids(),
                ..self.sim.get_map(index, self.sim().calendar.as_ref())
            }
        })
    }

    pub fn sample_columns(
        &self,
    ) -> impl Sampler<
        '_,
        Index = (Vec2<i32>, IndexRef<'_>, Option<&'_ Calendar>),
        Sample = Option<ColumnSample<'_>>,
    > + '_ {
        ColumnGen::new(&self.sim)
    }

    pub fn sample_blocks(&self) -> BlockGen<'_> { BlockGen::new(ColumnGen::new(&self.sim)) }

    /// Find a position that's accessible to a player at the given world
    /// position by searching blocks vertically.
    ///
    /// If `ascending` is `true`, we try to find the highest accessible position
    /// instead of the lowest.
    pub fn find_accessible_pos(
        &self,
        index: IndexRef,
        spawn_wpos: Vec2<i32>,
        ascending: bool,
    ) -> Vec3<f32> {
        let chunk_pos = TerrainGrid::chunk_key(spawn_wpos);

        // Unwrapping because generate_chunk only returns err when should_abort evals
        // to true
        let tc = self
            .generate_chunk_static_snapshot(index, chunk_pos, || false, None)
            .unwrap();

        tc.find_accessible_pos(spawn_wpos, ascending)
    }

    #[expect(clippy::result_unit_err)]
    pub fn generate_chunk_static_snapshot(
        &self,
        index: IndexRef,
        chunk_pos: Vec2<i32>,
        should_abort: impl FnMut() -> bool,
        time: Option<(TimeOfDay, Calendar)>,
    ) -> Result<TerrainChunk, ()> {
        self.generate_chunk_with_mode(
            index,
            chunk_pos,
            ChunkGenerationContext::static_snapshot(time.as_ref()),
            should_abort,
            ChunkGenerationMode::StaticSnapshot,
        )
        .map(|output| match output {
            ChunkGenerationOutput::StaticSnapshot(chunk) => chunk,
            ChunkGenerationOutput::RuntimeFinalized { .. } => {
                unreachable!("static snapshot generation should not produce runtime output")
            },
        })
    }

    #[expect(clippy::result_unit_err)]
    pub fn generate_chunk(
        &self,
        index: IndexRef,
        chunk_pos: Vec2<i32>,
        rtsim_resource_fractions: Option<EnumMap<TerrainResource, f32>>,
        should_abort: impl FnMut() -> bool,
        time: Option<(TimeOfDay, Calendar)>,
    ) -> Result<(TerrainChunk, ChunkSupplement), ()> {
        self.generate_chunk_with_mode(
            index,
            chunk_pos,
            ChunkGenerationContext::runtime_finalized(time.as_ref(), rtsim_resource_fractions),
            should_abort,
            ChunkGenerationMode::RuntimeFinalized,
        )
        .map(|output| match output {
            ChunkGenerationOutput::RuntimeFinalized { chunk, supplement } => (chunk, supplement),
            ChunkGenerationOutput::StaticSnapshot(_) => {
                unreachable!("runtime chunk generation should not produce a static snapshot")
            },
        })
    }

    #[expect(clippy::result_unit_err)]
    fn generate_chunk_with_mode(
        &self,
        index: IndexRef,
        chunk_pos: Vec2<i32>,
        generation_context: ChunkGenerationContext<'_>,
        mut should_abort: impl FnMut() -> bool,
        generation_mode: ChunkGenerationMode,
    ) -> Result<ChunkGenerationOutput, ()> {
        let calendar = generation_context.calendar();

        let mut sampler = self.sample_blocks();

        let (base_z, sim_chunk) = match self
            .sim
            /*.get_interpolated(
                chunk_pos.map2(chunk_size2d, |e, sz: u32| e * sz as i32 + sz as i32 / 2),
                |chunk| chunk.get_base_z(),
            )
            .and_then(|base_z| self.sim.get(chunk_pos).map(|sim_chunk| (base_z, sim_chunk))) */
            .get_base_z(chunk_pos)
        {
            Some(base_z) => (base_z as i32, self.sim.get(chunk_pos).unwrap()),
            // Some((base_z, sim_chunk)) => (base_z as i32, sim_chunk),
            None => {
                // NOTE: This is necessary in order to generate a handful of chunks at the
                // edges of the map.
                return Ok(match generation_mode {
                    ChunkGenerationMode::StaticSnapshot => {
                        ChunkGenerationOutput::StaticSnapshot(self.sim().generate_oob_chunk())
                    },
                    ChunkGenerationMode::RuntimeFinalized => {
                        ChunkGenerationOutput::RuntimeFinalized {
                            chunk: self.sim().generate_oob_chunk(),
                            supplement: ChunkSupplement::default(),
                        }
                    },
                });
            },
        };
        let (base_build, static_artifacts) = self.build_chunk_static_stage(
            index,
            chunk_pos,
            base_z,
            sim_chunk,
            &mut sampler,
            calendar,
            &mut should_abort,
        )?;
        Ok(self.finalize_chunk_generation_mode(
            generation_mode,
            base_build,
            sim_chunk,
            static_artifacts,
            index,
            generation_context,
        ))
    }

    #[expect(clippy::result_unit_err)]
    fn build_chunk_static_stage<'a>(
        &self,
        index: IndexRef<'a>,
        chunk_pos: Vec2<i32>,
        base_z: i32,
        sim_chunk: &sim::SimChunk,
        sampler: &mut BlockGen<'a>,
        calendar: Option<&'a Calendar>,
        should_abort: &mut impl FnMut() -> bool,
    ) -> Result<(ChunkBaseBuild<'a>, StaticChunkArtifacts), ()> {
        let mut base_build = self.build_base_chunk_volume(
            index,
            chunk_pos,
            base_z,
            sim_chunk,
            sampler,
            calendar,
            should_abort,
        )?;
        let static_artifacts = self.apply_static_passes_and_extract_artifacts(
            &mut base_build,
            sim_chunk,
            index,
            calendar,
        );

        Ok((base_build, static_artifacts))
    }

    fn finalize_chunk_generation_mode<'a>(
        &self,
        generation_mode: ChunkGenerationMode,
        mut base_build: ChunkBaseBuild<'a>,
        sim_chunk: &sim::SimChunk,
        static_artifacts: StaticChunkArtifacts,
        index: IndexRef<'a>,
        generation_context: ChunkGenerationContext<'_>,
    ) -> ChunkGenerationOutput {
        match generation_mode {
            ChunkGenerationMode::RuntimeFinalized => {
                let supplement = self.run_runtime_finalizers(
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
                // Static snapshot callers compare only deterministic chunk facts and stop
                // before runtime supplement / rtsim finalize mutate the returned
                // value contract.
                base_build.chunk.defragment();
                ChunkGenerationOutput::StaticSnapshot(base_build.chunk)
            },
        }
    }

    #[expect(clippy::result_unit_err)]
    fn build_base_chunk_volume<'a>(
        &self,
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
            sim_chunk.get_location_name(&index.sites, &self.civs.pois, chunk_center_wpos2d),
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
            self.sim.approx_chunk_terrain_normal(chunk_pos),
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
        &self,
        base_build: &mut ChunkBaseBuild<'a>,
        sim_chunk: &sim::SimChunk,
        index: IndexRef<'a>,
        calendar: Option<&'a Calendar>,
    ) -> StaticChunkArtifacts {
        let static_rng_seed = seed_expan::diffuse_mult(&[
            self.sim.seed,
            base_build.chunk_pos.x as u32,
            base_build.chunk_pos.y as u32,
            0x5354_4154,
        ]);
        let mut static_rng = ChaCha8Rng::from_seed(seed_expan::rng_state(static_rng_seed));

        // Apply layers (paths, caves, etc.)
        let mut canvas = Canvas {
            info: CanvasInfo {
                chunk_pos: base_build.chunk_pos,
                wpos: base_build.chunk_wpos2d,
                column_grid: &base_build.zcache_grid,
                column_grid_border: base_build.grid_border,
                chunks: &self.sim,
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
                &self.sim,
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
        // layer::apply_coral_to(&mut canvas);

        // Apply site generation
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
        &self,
        base_build: &mut ChunkBaseBuild<'a>,
        sim_chunk: &sim::SimChunk,
        static_artifacts: StaticChunkArtifacts,
        index: IndexRef<'a>,
        generation_context: ChunkGenerationContext<'_>,
    ) -> ChunkSupplement {
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
            self.sim.seed,
            base_build.chunk_pos.x as u32,
            base_build.chunk_pos.y as u32,
            runtime_time_bits as u32,
            (runtime_time_bits >> 32) as u32,
            runtime_calendar_mask,
            0x5255_4E54,
        ]);
        let mut runtime_rng = ChaCha8Rng::from_seed(seed_expan::rng_state(runtime_rng_seed));
        let RuntimeChunkArtifacts {
            mut supplement,
            rtsim_resource_blocks,
        } = static_artifacts.into_runtime_artifacts();
        Self::finalize_world_runtime_chunk(
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
        supplement.rtsim_max_resources = Self::apply_rtsim_resource_thinning(
            &mut base_build.chunk,
            rtsim_resource_blocks,
            rtsim_resource_fractions,
            &mut runtime_rng,
            base_build.chunk_wpos2d,
        );

        supplement
    }

    fn finalize_world_runtime_chunk(
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
        Self::apply_world_runtime_supplement(
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
            let lpos2d = TerrainChunkSize::RECT_SIZE
                .map(|sz| runtime_rng.random::<u32>().rem_euclid(sz) as i32);
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

    fn apply_rtsim_resource_thinning(
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
                             expected range of 0.0..=1.0. When registering a block with the \
                             sprite `{:?}`, with the damage `{:?}`.",
                            rtsim_resource_fractions[res],
                            block.get_sprite(),
                            block.get_attr::<common::terrain::sprite::Damage>().ok(),
                        );

                        // Throw a dice to determine whether this resource should actually spawn
                        // TODO: Don't throw a dice, try to generate the *exact* correct number
                        if runtime_rng
                            .random_bool(rtsim_resource_fractions[res].clamp(0.0, 1.0) as f64)
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

    // Zone coordinates
    pub fn get_lod_zone(&self, pos: Vec2<i32>, index: IndexRef) -> lod::Zone {
        let min_wpos = pos.map(lod::to_wpos);
        let max_wpos = (pos + 1).map(lod::to_wpos);

        let mut objects = Vec::new();

        // Add trees
        prof_span!(guard, "add trees");
        objects.extend(
            &mut self
                .sim()
                .get_area_trees(min_wpos, max_wpos)
                .filter_map(|attr| {
                    ColumnGen::new(self.sim())
                        .get((attr.pos, index, self.sim().calendar.as_ref()))
                        .filter(|col| layer::tree::tree_valid_at(attr.pos, col, None, attr.seed))
                        .zip(Some(attr))
                })
                .filter_map(|(col, tree)| {
                    Some(lod::Object {
                        kind: match tree.forest_kind {
                            all::ForestKind::Dead => lod::ObjectKind::Dead,
                            all::ForestKind::Pine => lod::ObjectKind::Pine,
                            all::ForestKind::Mangrove => lod::ObjectKind::Mangrove,
                            all::ForestKind::Acacia => lod::ObjectKind::Acacia,
                            all::ForestKind::Birch => lod::ObjectKind::Birch,
                            all::ForestKind::Redwood => lod::ObjectKind::Redwood,
                            all::ForestKind::Baobab => lod::ObjectKind::Baobab,
                            all::ForestKind::Frostpine => lod::ObjectKind::Frostpine,
                            all::ForestKind::Palm => lod::ObjectKind::Palm,
                            _ => lod::ObjectKind::GenericTree,
                        },
                        pos: {
                            let rpos = tree.pos - min_wpos;
                            if rpos.is_any_negative() {
                                return None;
                            } else {
                                rpos.map(|e| e as i16).with_z(col.alt as i16)
                            }
                        },
                        flags: lod::InstFlags::empty()
                            | if col.snow_cover {
                                lod::InstFlags::SNOW_COVERED
                            } else {
                                lod::InstFlags::empty()
                            }
                            // Apply random rotation
                            | lod::InstFlags::from_bits(((tree.seed % 4) as u8) << 2).expect("This shouldn't set unknown bits"),
                        color: {
                            let field = crate::util::RandomField::new(tree.seed);
                            let lerp = field.get_f32(Vec3::from(tree.pos)) * 0.8 + 0.1;
                            let sblock = tree.forest_kind.leaf_block();

                            crate::all::leaf_color(index, tree.seed, lerp, &sblock)
                                .unwrap_or(Rgb::black())
                        },
                    })
                }),
        );
        drop(guard);

        // Add structures
        objects.extend(
            index
                .sites
                .iter()
                .filter(|(_, site)| {
                    site.origin
                        .map2(min_wpos.zip(max_wpos), |e, (min, max)| e >= min && e < max)
                        .reduce_and()
                })
                .flat_map(|(_, site)| {
                    site.plots().filter_map(|plot| match &plot.kind {
                        site::plot::PlotKind::House(h) => Some((
                            site.tile_wpos(plot.root_tile),
                            h.roof_color(),
                            lod::ObjectKind::House,
                        )),
                        site::plot::PlotKind::GiantTree(t) => Some((
                            site.tile_wpos(plot.root_tile),
                            t.leaf_color(),
                            lod::ObjectKind::GiantTree,
                        )),
                        site::plot::PlotKind::Haniwa(_) => Some((
                            site.tile_wpos(plot.root_tile),
                            Rgb::black(),
                            lod::ObjectKind::Haniwa,
                        )),
                        site::plot::PlotKind::DesertCityMultiPlot(_) => Some((
                            site.tile_wpos(plot.root_tile),
                            Rgb::black(),
                            lod::ObjectKind::Desert,
                        )),
                        site::plot::PlotKind::DesertCityArena(_) => Some((
                            site.tile_wpos(plot.root_tile),
                            Rgb::black(),
                            lod::ObjectKind::Arena,
                        )),
                        site::plot::PlotKind::SavannahHut(_)
                        | site::plot::PlotKind::SavannahWorkshop(_) => Some((
                            site.tile_wpos(plot.root_tile),
                            Rgb::black(),
                            lod::ObjectKind::SavannahHut,
                        )),
                        site::plot::PlotKind::SavannahAirshipDock(_) => Some((
                            site.tile_wpos(plot.root_tile),
                            Rgb::black(),
                            lod::ObjectKind::SavannahAirshipDock,
                        )),
                        site::plot::PlotKind::TerracottaPalace(_) => Some((
                            site.tile_wpos(plot.root_tile),
                            Rgb::black(),
                            lod::ObjectKind::TerracottaPalace,
                        )),
                        site::plot::PlotKind::TerracottaHouse(_) => Some((
                            site.tile_wpos(plot.root_tile),
                            Rgb::black(),
                            lod::ObjectKind::TerracottaHouse,
                        )),
                        site::plot::PlotKind::TerracottaYard(_) => Some((
                            site.tile_wpos(plot.root_tile),
                            Rgb::black(),
                            lod::ObjectKind::TerracottaYard,
                        )),
                        site::plot::PlotKind::AirshipDock(_) => Some((
                            site.tile_wpos(plot.root_tile),
                            Rgb::black(),
                            lod::ObjectKind::AirshipDock,
                        )),
                        site::plot::PlotKind::CoastalHouse(_) => Some((
                            site.tile_wpos(plot.root_tile),
                            Rgb::black(),
                            lod::ObjectKind::CoastalHouse,
                        )),
                        site::plot::PlotKind::CoastalWorkshop(_) => Some((
                            site.tile_wpos(plot.root_tile),
                            Rgb::black(),
                            lod::ObjectKind::CoastalWorkshop,
                        )),
                        site::plot::PlotKind::CoastalAirshipDock(_) => Some((
                            site.tile_wpos(plot.root_tile),
                            Rgb::black(),
                            lod::ObjectKind::CoastalAirshipDock,
                        )),
                        site::plot::PlotKind::DesertCityAirshipDock(_) => Some((
                            site.tile_wpos(plot.root_tile),
                            Rgb::black(),
                            lod::ObjectKind::DesertCityAirshipDock,
                        )),
                        site::plot::PlotKind::CliffTownAirshipDock(_) => Some((
                            site.tile_wpos(plot.root_tile),
                            Rgb::black(),
                            lod::ObjectKind::CliffTownAirshipDock,
                        )),
                        _ => None,
                    })
                })
                .filter_map(|(wpos2d, color, model)| {
                    ColumnGen::new(self.sim())
                        .get((wpos2d, index, self.sim().calendar.as_ref()))
                        .zip(Some((wpos2d, color, model)))
                })
                .map(|(column, (wpos2d, color, model))| lod::Object {
                    kind: model,
                    pos: (wpos2d - min_wpos)
                        .map(|e| e as i16)
                        .with_z(self.sim().get_alt_approx(wpos2d).unwrap_or(0.0) as i16),
                    flags: if column.snow_cover {
                        lod::InstFlags::SNOW_COVERED
                    } else {
                        lod::InstFlags::empty()
                    },
                    color,
                }),
        );

        lod::Zone { objects }
    }

    // determine waypoint name
    pub fn get_location_name(&self, index: IndexRef, wpos2d: Vec2<i32>) -> Option<String> {
        let chunk_pos = wpos2d.wpos_to_cpos();
        let sim_chunk = self.sim.get(chunk_pos)?;
        sim_chunk.get_location_name(&index.sites, &self.civs.pois, wpos2d)
    }
}

#[cfg(test)]
mod tests {
    use super::{Error, World};
    use crate::sim::{CompatMode, FileOpts, WorldOpts};
    use rayon::ThreadPoolBuilder;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn generate_returns_structured_error_for_enforced_missing_load() {
        let pool = ThreadPoolBuilder::new().build().unwrap();
        let missing_path = std::env::temp_dir().join(format!(
            "caldrayne-world-missing-{}.bin",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos()
        ));
        let _ = fs::remove_file(&missing_path);

        let result = World::generate(
            42,
            WorldOpts {
                world_file: FileOpts::Load(missing_path.clone()),
                compat_mode: CompatMode::Enforce,
                ..WorldOpts::default()
            },
            &pool,
            &|_| {},
        );

        match result {
            Err(Error::CompatEnforce { audit }) => {
                assert_eq!(audit.entry.as_str(), "load");
                assert_eq!(audit.decision.as_str(), "fallback_generate");
                assert_eq!(audit.failure_kind.as_str(), "missing_input");
            },
            Ok(_) => panic!("expected compat enforce error, got successful world generation"),
            Err(other) => panic!("expected compat enforce error, got {other}"),
        }

        let _ = fs::remove_file(missing_path);
    }
}
