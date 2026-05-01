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
mod chunk;
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

pub(crate) use self::chunk::pipeline::{
    ChunkGenerationContext, ChunkGenerationMode, ChunkGenerationOutput,
};
use crate::{
    column::ColumnGen, index::Index, layer::spot::SpotGenerate, site::SiteKind, util::Sampler,
};
use common::{
    assets::{self, BoxedError, FileAsset, load_ron},
    calendar::Calendar,
    comp::Content,
    generation::ChunkSupplement,
    lod,
    map::{Marker, MarkerKind},
    resources::TimeOfDay,
    rtsim::TerrainResource,
    spiral::Spiral2d,
    spot::Spot,
    terrain::{
        BiomeKind, CoordinateConversions, SpriteKind, TerrainChunk, TerrainChunkSize, TerrainGrid,
    },
    vol::RectVolSize,
};
use common_base::prof_span;
use common_net::msg::{WorldMapMsg, world_msg};
use enum_map::EnumMap;
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
    fn default_chunk_output_for_missing_runtime_chunk_product(
        &self,
        generation_mode: ChunkGenerationMode,
    ) -> ChunkGenerationOutput {
        match generation_mode {
            ChunkGenerationMode::StaticSnapshot => ChunkGenerationOutput::StaticSnapshot(
                self.sim().default_chunk_for_missing_world_bounds(),
            ),
            ChunkGenerationMode::RuntimeFinalized => ChunkGenerationOutput::RuntimeFinalized {
                chunk: self.sim().default_chunk_for_missing_world_bounds(),
                supplement: ChunkSupplement::default(),
            },
        }
    }

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

    pub fn query_chunk_key_aabr(&self) -> Aabr<i32> { self.sim.query_chunk_key_aabr() }

    pub fn runtime_chunk_product_key_aabr(&self) -> Aabr<i32> {
        self.sim.runtime_chunk_product_key_aabr()
    }

    pub fn runtime_topology_descriptor(&self) -> world_msg::RuntimeTopologyDescriptor {
        self.sim.runtime_topology_descriptor()
    }

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
        chunk::pipeline::generate_chunk_with_mode(
            self,
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
        chunk::pipeline::generate_chunk_with_mode(
            self,
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
                        .with_z(self.sim().alt_approx_or(wpos2d, sim::ApproxFallback::Zero) as i16),
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
    use crate::{
        CONFIG,
        recipe::RecipeManifestV1,
        sim::{
            CompatMode, FileOpts, GenOpts, LoadLegacyMode, LoadOrGenerateSidecarlessMode,
            WorldFile, WorldMap_0_7_0, WorldOpts,
        },
    };
    use bincode::{config::legacy, serde::encode_into_std_write};
    use rayon::ThreadPoolBuilder;
    use std::{
        fs,
        io::BufWriter,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };
    use vek::Vec2;

    fn unique_managed_world_target(tag: &str) -> (String, PathBuf) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let base = std::env::temp_dir().join(format!("caldrayne-world-{tag}-{unique}"));
        (
            base.to_string_lossy().into_owned(),
            base.with_extension("bin"),
        )
    }

    fn recipe_sidecar_path_for_map_path(map_path: &Path) -> PathBuf {
        let mut sidecar_path = map_path.as_os_str().to_owned();
        sidecar_path.push(".recipe.ron");
        PathBuf::from(sidecar_path)
    }

    fn write_matching_world_file(map_path: &Path) {
        let opts = GenOpts::default();
        let map_cell_count = 1usize << (opts.x_lg + opts.y_lg);
        let world_file = WorldFile::Veloren0_7_0(WorldMap_0_7_0 {
            map_size_lg: Vec2::new(opts.x_lg, opts.y_lg),
            continent_scale_hack: opts.scale,
            alt: vec![0.0; map_cell_count].into_boxed_slice(),
            basement: vec![0.0; map_cell_count].into_boxed_slice(),
        });

        if let Some(parent) = map_path.parent() {
            fs::create_dir_all(parent).expect("managed world parent should be creatable");
        }

        let file = fs::File::create(map_path).expect("managed world file should be writable");
        let mut writer = BufWriter::new(file);
        encode_into_std_write(&world_file, &mut writer, legacy())
            .expect("managed world file should serialize");
    }

    fn write_matching_recipe_sidecar(map_path: &Path, world_seed: u32) {
        let rendered_manifest = ron::ser::to_string_pretty(
            &RecipeManifestV1::record_only(world_seed, &GenOpts::default(), true),
            ron::ser::PrettyConfig::default(),
        )
        .expect("recipe sidecar should serialize");
        fs::write(
            recipe_sidecar_path_for_map_path(map_path),
            rendered_manifest,
        )
        .expect("recipe sidecar should be writable");
    }

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

    #[test]
    fn generate_returns_structured_error_for_enforced_asset_recipe_reject() {
        let pool = ThreadPoolBuilder::new().build().unwrap();

        let result = World::generate(
            42,
            WorldOpts {
                world_file: FileOpts::LoadAsset("world.map.veloren_0_16_0_0".to_owned()),
                compat_mode: CompatMode::Enforce,
                ..WorldOpts::default()
            },
            &pool,
            &|_| {},
        );

        match result {
            Err(Error::CompatEnforce { audit }) => {
                assert_eq!(audit.entry.as_str(), "load_asset");
                assert_eq!(audit.resolution.as_str(), "reject");
                assert_eq!(audit.failure_kind.as_str(), "missing_input");
                assert_eq!(audit.failure_subject.as_str(), "recipe");
            },
            Ok(_) => panic!("expected compat enforce error, got successful world generation"),
            Err(other) => panic!("expected compat enforce error, got {other}"),
        }
    }

    #[test]
    fn generate_returns_structured_error_for_missing_legacy_import_in_record_mode() {
        let pool = ThreadPoolBuilder::new().build().unwrap();
        let missing_path = std::env::temp_dir().join(format!(
            "caldrayne-world-legacy-import-missing-{}.bin",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos()
        ));
        let _ = fs::remove_file(&missing_path);

        let result = World::generate(
            42,
            WorldOpts {
                world_file: FileOpts::LoadLegacy(missing_path.clone()),
                compat_mode: CompatMode::Record,
                ..WorldOpts::default()
            },
            &pool,
            &|_| {},
        );

        match result {
            Err(Error::CompatEnforce { audit }) => {
                assert_eq!(audit.entry.as_str(), "load_legacy");
                assert_eq!(audit.resolution.as_str(), "reject");
                assert_eq!(audit.failure_kind.as_str(), "missing_input");
                assert_eq!(audit.failure_subject.as_str(), "world");
            },
            Ok(_) => panic!("expected compat import rejection, got successful world generation"),
            Err(other) => panic!("expected compat import rejection, got {other}"),
        }

        let _ = fs::remove_file(missing_path);
    }

    #[test]
    fn generate_returns_structured_error_when_load_legacy_mode_denies_import() {
        let pool = ThreadPoolBuilder::new().build().unwrap();
        let missing_path = std::env::temp_dir().join(format!(
            "caldrayne-world-legacy-import-denied-{}.bin",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos()
        ));

        let result = World::generate(
            42,
            WorldOpts {
                world_file: FileOpts::LoadLegacy(missing_path),
                compat_mode: CompatMode::Record,
                load_legacy_mode: LoadLegacyMode::Deny,
                ..WorldOpts::default()
            },
            &pool,
            &|_| {},
        );

        match result {
            Err(Error::CompatEnforce { audit }) => {
                assert_eq!(audit.entry.as_str(), "load_legacy");
                assert_eq!(audit.resolution.as_str(), "reject");
                assert_eq!(audit.failure_kind.as_str(), "policy_denied");
                assert_eq!(audit.failure_subject.as_str(), "options");
            },
            Ok(_) => panic!("expected load_legacy gate rejection, got successful world generation"),
            Err(other) => panic!("expected load_legacy gate rejection, got {other}"),
        }
    }

    #[test]
    fn generate_returns_structured_error_when_sidecarless_load_or_generate_mode_denies_reuse() {
        let pool = ThreadPoolBuilder::new().build().unwrap();
        let (name, map_path) = unique_managed_world_target("managed-sidecarless-denied");
        let sidecar_path = recipe_sidecar_path_for_map_path(&map_path);
        let _ = fs::remove_file(&map_path);
        let _ = fs::remove_file(&sidecar_path);
        write_matching_world_file(&map_path);

        let result = World::generate(
            42,
            WorldOpts {
                world_file: FileOpts::LoadOrGenerate {
                    name,
                    opts: GenOpts::default(),
                    overwrite: false,
                },
                compat_mode: CompatMode::Record,
                load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode::Deny,
                ..WorldOpts::default()
            },
            &pool,
            &|_| {},
        );

        match result {
            Err(Error::CompatEnforce { audit }) => {
                assert_eq!(audit.entry.as_str(), "load_or_generate");
                assert_eq!(audit.resolution.as_str(), "reject");
                assert_eq!(audit.failure_kind.as_str(), "policy_denied");
                assert_eq!(audit.failure_subject.as_str(), "options");
            },
            Ok(_) => {
                panic!(
                    "expected sidecarless load_or_generate gate rejection, got successful world \
                     generation"
                )
            },
            Err(other) => {
                panic!("expected sidecarless load_or_generate gate rejection, got {other}")
            },
        }

        let _ = fs::remove_file(map_path);
        let _ = fs::remove_file(sidecar_path);
    }

    #[test]
    fn generate_loads_strict_world_with_recipe_sidecar_under_deny_posture() {
        let pool = ThreadPoolBuilder::new().build().unwrap();
        let (_, map_path) = unique_managed_world_target("strict-load-deny-clear");
        let sidecar_path = recipe_sidecar_path_for_map_path(&map_path);
        let _ = fs::remove_file(&map_path);
        let _ = fs::remove_file(&sidecar_path);
        write_matching_world_file(&map_path);
        write_matching_recipe_sidecar(&map_path, 42);

        let (world, _) = World::generate(
            42,
            WorldOpts {
                world_file: FileOpts::Load(map_path.clone()),
                compat_mode: CompatMode::Record,
                load_legacy_mode: LoadLegacyMode::Deny,
                load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode::Deny,
                ..WorldOpts::default()
            },
            &pool,
            &|_| {},
        )
        .expect("strict Load(path) should remain admitted under deny posture");

        assert_eq!(world.sim().compat_audit().entry.as_str(), "load");
        assert_eq!(
            world.sim().compat_audit().decision.as_str(),
            "loaded_existing"
        );
        assert_eq!(world.sim().compat_audit().failure_kind.as_str(), "none");
        assert_eq!(world.sim().load_legacy_mode().as_str(), "deny");
        assert_eq!(
            world.sim().load_or_generate_sidecarless_mode().as_str(),
            "deny"
        );
        assert!(!world.sim().managed_recipe_sidecar_missing());

        let _ = fs::remove_file(map_path);
        let _ = fs::remove_file(sidecar_path);
    }

    #[test]
    fn default_chunk_output_for_missing_runtime_chunk_product_preserves_bounded_ocean_product() {
        let (world, _) = World::empty();

        match world.default_chunk_output_for_missing_runtime_chunk_product(
            super::ChunkGenerationMode::StaticSnapshot,
        ) {
            super::ChunkGenerationOutput::StaticSnapshot(chunk) => {
                assert_eq!(chunk.get_min_z(), CONFIG.sea_level as i32);
            },
            super::ChunkGenerationOutput::RuntimeFinalized { .. } => {
                panic!("static snapshot default chunk output should not produce runtime output")
            },
        }

        match world.default_chunk_output_for_missing_runtime_chunk_product(
            super::ChunkGenerationMode::RuntimeFinalized,
        ) {
            super::ChunkGenerationOutput::RuntimeFinalized { chunk, supplement } => {
                assert_eq!(chunk.get_min_z(), CONFIG.sea_level as i32);
                assert!(supplement.entity_spawns.is_empty());
            },
            super::ChunkGenerationOutput::StaticSnapshot(_) => {
                panic!("runtime default chunk output should not produce static snapshot output")
            },
        }
    }

    #[test]
    fn bounded_runtime_chunk_product_domain_is_stricter_than_query_domain() {
        let (world, _) = World::empty();

        let query_chunk_key_aabr = world.query_chunk_key_aabr();
        let runtime_chunk_product_key_aabr = world.runtime_chunk_product_key_aabr();

        assert_eq!(query_chunk_key_aabr.min, Vec2::zero());
        assert_eq!(runtime_chunk_product_key_aabr.min, Vec2::one());
        assert!(query_chunk_key_aabr.max.x > runtime_chunk_product_key_aabr.max.x);
        assert!(query_chunk_key_aabr.max.y > runtime_chunk_product_key_aabr.max.y);
    }

    #[test]
    fn public_chunk_generation_falls_back_to_default_output_outside_runtime_product_domain() {
        let (world, index) = World::empty();
        let runtime_chunk_product_key_aabr = world.runtime_chunk_product_key_aabr();
        let chunk_pos = Vec2::new(
            runtime_chunk_product_key_aabr.min.x - 1,
            runtime_chunk_product_key_aabr.min.y,
        );

        let static_chunk = world
            .generate_chunk_static_snapshot(index.as_index_ref(), chunk_pos, || false, None)
            .expect(
                "static chunk generation should fall back cleanly outside runtime product domain",
            );
        assert_eq!(static_chunk.get_min_z(), CONFIG.sea_level as i32);

        let (runtime_chunk, supplement) = world
            .generate_chunk(index.as_index_ref(), chunk_pos, None, || false, None)
            .expect(
                "runtime chunk generation should fall back cleanly outside runtime product domain",
            );
        assert_eq!(runtime_chunk.get_min_z(), CONFIG.sea_level as i32);
        assert!(supplement.entity_spawns.is_empty());
        assert!(
            supplement
                .rtsim_max_resources
                .values()
                .all(|count| *count == 0)
        );
    }
}
