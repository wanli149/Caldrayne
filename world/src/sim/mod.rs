mod compat;
mod diffusion;
mod erosion;
mod file_ops;
mod load_content;
mod location;
mod map;
pub(crate) mod marine_semantics;
pub(crate) mod site_suitability;
pub(crate) mod subterranean_semantics;
mod topology;
mod util;
mod way;

// Reexports
pub use self::{
    compat::{CompatMode, LoadLegacyMode, LoadOrGenerateSidecarlessMode},
    diffusion::diffusion,
    location::Location,
    map::{sample_pos, sample_wpos},
    marine_semantics::AquaticSpawnPotential,
    util::get_horizon_map,
    way::{Path, Way},
};
use self::{
    erosion::Compute,
    marine_semantics::{
        AquaticFaunaProfile, MarineEcologyProfile, WaterAccessClass, WaterBodyKind,
    },
    topology::WorldTopology,
};
pub(crate) use self::{
    erosion::{
        Alt, RiverData, RiverKind, do_erosion, fill_sinks, get_lakes, get_multi_drainage,
        get_multi_rec, get_rivers,
    },
    util::{
        InverseCdf, cdf_irwin_hall, downhill, get_oceans, map_edge_factor, uniform_noise, uphill,
    },
};

use crate::{
    CONFIG, IndexRef,
    all::{Environment, ForestKind, TreeAttr},
    block::BlockGen,
    civ::{Place, PointOfInterest},
    column::ColumnGen,
    recipe::{
        CompatAuditV1, CompatFailureDetailV1, CompatFailureKindV1, CompatFailureSubjectV1,
        RecipeManifestV1, TopologyId,
    },
    site::Site,
    util::{
        CARDINALS, DHashSet, FastNoise, FastNoise2d, LOCALITY, NEIGHBORS, RandomField, Sampler,
        StructureGen2d, seed_expan,
    },
};
use bincode::{
    config::legacy,
    serde::{decode_from_std_read, encode_into_std_write},
};
use bitvec::prelude::BitBox;
use common::{
    assets::{AssetExt, BoxedError, FileAsset, load_bincode_legacy},
    calendar::Calendar,
    grid::Grid,
    lottery::Lottery,
    resources::MapKind,
    spiral::Spiral2d,
    spot::Spot,
    store::{Id, Store},
    terrain::{
        BiomeKind, CoordinateConversions, MapSizeLg, TerrainChunk, TerrainChunkSize,
        map::MapConfig, uniform_idx_as_vec2, vec2_as_uniform_idx,
    },
    vol::RectVolSize,
};
use common_base::prof_span;
use common_net::msg::{WorldMapMsg, world_msg};
use noise::{
    BasicMulti, Billow, Fbm, HybridMulti, MultiFractal, NoiseFn, Perlin, RidgedMulti, SuperSimplex,
    core::worley::distance_functions,
};
use num::{Float, Signed, traits::FloatConst};
use rand::{RngExt, SeedableRng};
use rand_chacha::ChaChaRng;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    f32,
    fs::File,
    io::{BufReader, BufWriter},
    ops::{Add, Div, Mul, Neg, Sub},
    path::PathBuf,
    sync::Arc,
};
use strum::IntoEnumIterator;
use tracing::{debug, error, info, warn};
use vek::*;

/// Default base two logarithm of the world size, in chunks, per dimension.
///
/// Currently, our default map dimensions are 2^10 × 2^10 chunks,
/// mostly for historical reasons.  It is likely that we will increase this
/// default at some point.
const DEFAULT_WORLD_CHUNKS_LG: MapSizeLg =
    if let Ok(map_size_lg) = MapSizeLg::new(Vec2 { x: 10, y: 10 }) {
        map_size_lg
    } else {
        panic!("Default world chunk size does not satisfy required invariants.");
    };

/// A structure that holds cached noise values and cumulative distribution
/// functions for the input that led to those values.  See the definition of
/// InverseCdf for a description of how to interpret the types of its fields.
struct GenCdf {
    humid_base: InverseCdf,
    temp_base: InverseCdf,
    chaos: InverseCdf,
    alt: Box<[Alt]>,
    basement: Box<[Alt]>,
    water_alt: Box<[f32]>,
    dh: Box<[isize]>,
    /// NOTE: Until we hit 4096 × 4096, this should suffice since integers with
    /// an absolute value under 2^24 can be exactly represented in an f32.
    flux: Box<[Compute]>,
    pure_flux: InverseCdf<Compute>,
    alt_no_water: InverseCdf,
    rivers: Box<[RiverData]>,
}

struct PostErosionHydrologyCore {
    water_alt: Box<[f32]>,
    dh: Box<[isize]>,
    flux: Box<[Compute]>,
    rivers: Box<[RiverData]>,
    max_height: f32,
}

struct PostErosionDrainageState {
    is_ocean: BitBox,
    dh: Box<[isize]>,
    indirection: Box<[i32]>,
    water_alt_pos: Box<[u32]>,
    flux: Box<[Compute]>,
    max_height: f32,
}

struct PreErosionFields {
    chaos: InverseCdf,
    alt_old: InverseCdf,
    is_ocean: BitBox,
    uplift_uniform: InverseCdf<f64>,
}

struct GenerationTunables {
    continent_scale: f64,
    rock_lacunarity: f64,
    uplift_scale: f64,
}

impl GenerationTunables {
    fn new(gen_opts: &GenOpts) -> Self {
        Self {
            continent_scale: gen_opts.scale
                * 5_000.0f64
                    .div(32.0)
                    .mul(TerrainChunkSize::RECT_SIZE.x as f64),
            rock_lacunarity: 2.0,
            uplift_scale: 128.0,
        }
    }
}

struct PreErosionParams {
    grid_scale: f64,
    continent_scale: f64,
    uplift_turb_scale: f64,
    turb_wposf_div: f64,
    min_epsilon: f64,
    max_epsilon: f64,
    max_erosion_per_delta_t: f64,
    n_steps: usize,
    n_small_steps: usize,
    n_post_load_steps: usize,
    rock_strength_div_factor: f64,
}

impl PreErosionParams {
    fn new(
        map_size_lg: MapSizeLg,
        gen_opts: &GenOpts,
        continent_scale: f64,
        uplift_scale: f64,
    ) -> Self {
        // Suppose the old world has grid spacing Δx' = Δy', new Δx = Δy.
        // We define grid_scale such that Δx = height_scale * Δx' ⇒
        //  grid_scale = Δx / Δx'.
        let grid_scale = 1.0f64 / (4.0 / gen_opts.scale)/*1.0*/;

        let n_approx = 1.0;
        let max_erosion_per_delta_t = 64.0 * grid_scale.powf(n_approx);
        let map_size_chunks_len_f64 = map_size_lg.chunks().map(f64::from).product();
        let min_epsilon = 1.0 / map_size_chunks_len_f64.max(f64::EPSILON * 0.5);
        let max_epsilon = (1.0 - 1.0 / map_size_chunks_len_f64).min(1.0 - f64::EPSILON * 0.5);

        Self {
            grid_scale,
            continent_scale,
            uplift_turb_scale: uplift_scale / 4.0,
            turb_wposf_div: 8.0,
            min_epsilon,
            max_epsilon,
            max_erosion_per_delta_t,
            n_steps: (100.0 * gen_opts.erosion_quality) as usize,
            n_small_steps: 0,
            n_post_load_steps: 0,
            rock_strength_div_factor: (2.0 * TerrainChunkSize::RECT_SIZE.x as f64) / 8.0,
        }
    }

    fn k_fs_scale(&self, theta: f32, n: f32) -> f64 {
        self.grid_scale.powf(-2.0 * (theta * n) as f64)
    }

    fn k_da_scale(&self, q: f64) -> f64 { self.grid_scale.powf(-2.0 * q) }

    fn height_scale(&self, n: f32) -> Alt { self.grid_scale.powf(n as f64) as Alt }

    fn time_scale(&self, n: f32) -> f64 { self.grid_scale.powf(n as f64) }

    fn alpha_scale(&self, n: f32) -> f32 { self.height_scale(n).recip() as f32 }

    fn k_d_scale(&self, n: f32) -> f64 { self.grid_scale.powi(2) / self.time_scale(n) }

    fn epsilon_0_scale(&self, n: f32) -> f32 {
        (self.height_scale(n) / self.time_scale(n) as Alt) as f32
    }

    fn erosion_factor(&self, x: f64) -> f64 {
        (x - self.min_epsilon) / (self.max_epsilon - self.min_epsilon)
    }

    fn remap_uplift_uniform(&self, x: f64) -> f64 {
        self.erosion_factor(
            x.mul(self.max_epsilon - self.min_epsilon)
                .add(self.min_epsilon),
        )
    }
}

struct UpliftRockSample {
    uheight: f64,
    rock_strength: f64,
}

struct PreErosionSetup {
    params: PreErosionParams,
    fields: PreErosionFields,
}

struct WorldGenerationStart {
    generation_tunables: GenerationTunables,
    rng: ChaChaRng,
    gen_ctx: GenCtx,
    pre_erosion_setup: PreErosionSetup,
}

impl PreErosionSetup {
    fn model<'a>(&'a self, map_size_lg: MapSizeLg, gen_ctx: &'a GenCtx) -> PreErosionModel<'a> {
        PreErosionModel {
            map_size_lg,
            pre_erosion_params: &self.params,
            gen_ctx,
            alt_old: &self.fields.alt_old,
            is_ocean: &self.fields.is_ocean,
            uplift_uniform: &self.fields.uplift_uniform,
        }
    }

    fn into_chaos(self) -> InverseCdf { self.fields.chaos }
}

struct PreErosionModel<'a> {
    map_size_lg: MapSizeLg,
    pre_erosion_params: &'a PreErosionParams,
    gen_ctx: &'a GenCtx,
    alt_old: &'a InverseCdf,
    is_ocean: &'a BitBox,
    uplift_uniform: &'a InverseCdf<f64>,
}

impl<'a> PreErosionModel<'a> {
    fn terrain_n(_posi: usize) -> f32 { 1.0 }

    fn theta(&self, _posi: usize) -> f32 { 0.4 }

    fn is_ocean(&self, posi: usize) -> bool { self.is_ocean[posi] }

    fn old_height(&self, posi: usize) -> f32 {
        self.alt_old[posi].1
            * CONFIG.mountain_scale
            * self.pre_erosion_params.height_scale(Self::terrain_n(posi)) as f32
    }

    fn kf(&self, posi: usize) -> f64 {
        let kf_scale_i = self
            .pre_erosion_params
            .k_fs_scale(self.theta(posi), Self::terrain_n(posi));
        if self.is_ocean(posi) {
            return 1.0e-4 * kf_scale_i;
        }

        let kf_i = 1.0e-6;
        kf_i * kf_scale_i
    }

    fn kd(&self, posi: usize) -> f64 {
        let kd_scale_i = self.pre_erosion_params.k_d_scale(Self::terrain_n(posi));
        if self.is_ocean(posi) {
            let kd_i = 1.0e-2 / 4.0;
            return kd_i * kd_scale_i;
        }

        let kd_i = 1.0e-2 / 4.0;
        kd_i * kd_scale_i
    }

    fn g(&self, posi: usize) -> f32 {
        if map_edge_factor(self.map_size_lg, posi) == 0.0 {
            return 0.0;
        }

        1.0
    }

    fn weathering_logit(x: f64) -> f64 { x.ln() - (-x).ln_1p() }

    fn weathering_log_odds(x: f64, center: f64) -> f64 {
        Self::weathering_logit(x) - Self::weathering_logit(center)
    }

    fn weathering_logistic_cdf(x: f64) -> f64 {
        let logistic_2_base = 3.0f64.sqrt() * std::f64::consts::FRAC_2_PI;
        (x / logistic_2_base).tanh() * 0.5 + 0.5
    }

    fn weathering_strength(&self, posi: usize) -> f64 {
        let UpliftRockSample {
            uheight,
            rock_strength,
        } = self.sample_uplift_rock_sample(posi);
        let center = 0.4;
        let dmin = center - 0.05;
        let dmax = center + 0.05;
        Self::weathering_logistic_cdf(
            1.0 * Self::weathering_logit(rock_strength.clamp(1e-7, 1.0f64 - 1e-7))
                + 1.0 * Self::weathering_log_odds(uheight.clamp(dmin, dmax), center),
        )
    }

    fn epsilon_0(&self, posi: usize) -> f32 {
        let epsilon_0_scale_i = self
            .pre_erosion_params
            .epsilon_0_scale(Self::terrain_n(posi));
        if self.is_ocean(posi) {
            let epsilon_0_i = 2.078e-3 / 4.0;
            return epsilon_0_i * epsilon_0_scale_i;
        }

        let ustrength = self.weathering_strength(posi);
        let epsilon_0_i = ((1.0 - ustrength) * (2.078e-3 - 5.3e-5) + 5.3e-5) as f32 / 4.0;
        epsilon_0_i * epsilon_0_scale_i
    }

    fn alpha(&self, posi: usize) -> f32 {
        let alpha_scale_i = self.pre_erosion_params.alpha_scale(Self::terrain_n(posi));
        if self.is_ocean(posi) {
            return 3.7e-2 * alpha_scale_i;
        }

        let ustrength = self.weathering_strength(posi);
        let alpha_i = (ustrength * (4.2e-2 - 1.6e-2) + 1.6e-2) as f32;
        alpha_i * alpha_scale_i
    }

    fn uplift(&self, posi: usize) -> f64 {
        if self.is_ocean(posi) {
            return 0.0;
        }
        let height = self
            .pre_erosion_params
            .remap_uplift_uniform(self.uplift_uniform[posi].1);
        assert!(height >= 0.0);
        assert!(height <= 1.0);
        height * self.pre_erosion_params.max_erosion_per_delta_t
    }

    fn alt(&self, posi: usize) -> f32 {
        if self.is_ocean(posi) {
            self.old_height(posi)
        } else {
            (self.old_height(posi) as f64 / CONFIG.mountain_scale as f64) as f32 - 0.5
        }
    }

    fn sample_uplift_rock_sample(&self, posi: usize) -> UpliftRockSample {
        let wposf = (uniform_idx_as_vec2(self.map_size_lg, posi)
            * TerrainChunkSize::RECT_SIZE.map(|e| e as i32))
        .map(|e| e as f64);
        let turb_wposf = wposf
            .mul(5_000.0 / self.pre_erosion_params.continent_scale)
            .div(TerrainChunkSize::RECT_SIZE.map(|e| e as f64))
            .div(self.pre_erosion_params.turb_wposf_div);
        let turb = Vec2::new(
            self.gen_ctx.turb_x_nz.get(turb_wposf.into_array()),
            self.gen_ctx.turb_y_nz.get(turb_wposf.into_array()),
        ) * self.pre_erosion_params.uplift_turb_scale
            * TerrainChunkSize::RECT_SIZE.map(|e| e as f64);
        let turb_wposf = wposf + turb;
        let uheight = self
            .gen_ctx
            .uplift_nz
            .get(turb_wposf.into_array())
            .clamp(-1.0, 1.0)
            .mul(0.5)
            .add(0.5);
        let wposf3 = Vec3::new(
            wposf.x,
            wposf.y,
            uheight
                * CONFIG.mountain_scale as f64
                * self.pre_erosion_params.rock_strength_div_factor,
        );
        let rock_strength = self
            .gen_ctx
            .rock_strength_nz
            .get(wposf3.into_array())
            .clamp(-1.0, 1.0)
            .mul(0.5)
            .add(0.5);

        UpliftRockSample {
            uheight,
            rock_strength,
        }
    }
}

struct PostWaterCdfFields {
    pure_flux: InverseCdf<Compute>,
    alt_no_water: InverseCdf,
    temp_base: InverseCdf,
    humid_base: InverseCdf,
}

struct GeneratedWorldParts {
    seed: u32,
    map_size_lg: MapSizeLg,
    max_height: f32,
    chunks: Vec<SimChunk>,
    gen_ctx: GenCtx,
    rng: ChaChaRng,
    calendar: Option<Calendar>,
    compat_mode: CompatMode,
    load_legacy_mode: LoadLegacyMode,
    load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode,
    compat_audit: CompatAuditV1,
    managed_recipe_sidecar_missing: bool,
    recipe_manifest: RecipeManifestV1,
}

struct GeneratedWorldFinalizeInputs {
    world_parts_inputs: GeneratedWorldPartsInputs,
    seed_elements: bool,
}

struct GeneratedWorldFinalizeBuilderInputs {
    seed: u32,
    map_size_lg: MapSizeLg,
    gen_ctx: GenCtx,
    post_erosion_chunk_inputs: PostErosionChunkInputs,
    rng: ChaChaRng,
    calendar: Option<Calendar>,
    compat_mode: CompatMode,
    load_legacy_mode: LoadLegacyMode,
    load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode,
    compat_audit: CompatAuditV1,
    managed_recipe_sidecar_missing: bool,
    recipe_manifest: RecipeManifestV1,
    seed_elements: bool,
}

struct WorldLoadBootstrapRequest {
    seed: u32,
    seed_elements: bool,
    world_file: FileOpts,
    compat_mode: CompatMode,
    load_legacy_mode: LoadLegacyMode,
    load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode,
}

struct GeneratedWorldFinalizePreparationRequest<'a> {
    load_bootstrap: WorldLoadBootstrap,
    world_file: FileOpts,
    calendar: Option<Calendar>,
    compat_mode: CompatMode,
    load_legacy_mode: LoadLegacyMode,
    load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode,
    threadpool: &'a rayon::ThreadPool,
    stage_report: &'a dyn Fn(WorldSimStage),
}

struct GeneratedWorldFinalizeBuilderPreparationRequest<'a> {
    load_bootstrap: WorldLoadBootstrap,
    world_file: FileOpts,
    calendar: Option<Calendar>,
    compat_mode: CompatMode,
    load_legacy_mode: LoadLegacyMode,
    load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode,
    threadpool: &'a rayon::ThreadPool,
    stage_report: &'a dyn Fn(WorldSimStage),
}

struct GeneratedWorldPartsInputs {
    seed: u32,
    map_size_lg: MapSizeLg,
    gen_ctx: GenCtx,
    post_erosion_chunk_inputs: PostErosionChunkInputs,
    rng: ChaChaRng,
    calendar: Option<Calendar>,
    compat_mode: CompatMode,
    load_legacy_mode: LoadLegacyMode,
    load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode,
    compat_audit: CompatAuditV1,
    managed_recipe_sidecar_missing: bool,
    recipe_manifest: RecipeManifestV1,
}

struct GenerationChunkInputsRequest<'a> {
    parsed_world_file: Option<ModernMap>,
    map_size_lg: MapSizeLg,
    gen_opts: &'a GenOpts,
    world_file: FileOpts,
    fresh: bool,
    recipe_manifest: &'a RecipeManifestV1,
    gen_ctx: &'a GenCtx,
    pre_erosion_setup: PreErosionSetup,
    threadpool: &'a rayon::ThreadPool,
    stage_report: &'a dyn Fn(WorldSimStage),
}

struct PostErosionChunkInputs {
    max_height: f32,
    gen_cdf: GenCdf,
}

impl GeneratedWorldFinalizeInputs {
    fn into_parts(self) -> GeneratedWorldParts { self.world_parts_inputs.into_parts() }
}

impl GeneratedWorldPartsInputs {
    fn into_parts(self) -> GeneratedWorldParts {
        let Self {
            seed,
            map_size_lg,
            gen_ctx,
            post_erosion_chunk_inputs,
            rng,
            calendar,
            compat_mode,
            load_legacy_mode,
            load_or_generate_sidecarless_mode,
            compat_audit,
            managed_recipe_sidecar_missing,
            recipe_manifest,
        } = self;
        let PostErosionChunkInputs {
            max_height,
            gen_cdf,
        } = post_erosion_chunk_inputs;
        let chunks = WorldSim::build_sim_chunks(map_size_lg, &gen_ctx, &gen_cdf);

        GeneratedWorldParts {
            seed,
            map_size_lg,
            max_height,
            chunks,
            gen_ctx,
            rng,
            calendar,
            compat_mode,
            load_legacy_mode,
            load_or_generate_sidecarless_mode,
            compat_audit,
            managed_recipe_sidecar_missing,
            recipe_manifest,
        }
    }
}

struct ErosionProgressReporter<'a> {
    last: Option<(std::time::Instant, f64)>,
    all_samples: std::time::Duration,
    sample_count: u32,
    stage_report: &'a dyn Fn(WorldSimStage),
}

impl<'a> ErosionProgressReporter<'a> {
    fn new(stage_report: &'a dyn Fn(WorldSimStage)) -> Self {
        Self {
            last: None,
            all_samples: std::time::Duration::default(),
            sample_count: 0,
            stage_report,
        }
    }

    fn report(&mut self, progress: f64) {
        let now = std::time::Instant::now();
        let estimate = if let Some((last_instant, last_progress)) = self.last {
            if last_progress > progress {
                None
            } else {
                if last_progress < progress {
                    let sample = now
                        .duration_since(last_instant)
                        .div_f64(progress - last_progress);
                    self.all_samples += sample;
                    self.sample_count += 1;
                }

                Some((self.all_samples / self.sample_count).mul_f64(100.0 - progress))
            }
        } else {
            None
        };
        self.last = Some((now, progress));
        (self.stage_report)(WorldSimStage::Erosion { progress, estimate });
    }
}

pub(crate) struct GenCtx {
    pub turb_x_nz: SuperSimplex,
    pub turb_y_nz: SuperSimplex,
    pub chaos_nz: RidgedMulti<Perlin>,
    pub alt_nz: util::HybridMulti<Perlin>,
    pub hill_nz: SuperSimplex,
    pub temp_nz: Fbm<Perlin>,
    // Humidity noise
    pub humid_nz: Billow<Perlin>,
    // Small amounts of noise for simulating rough terrain.
    pub small_nz: BasicMulti<Perlin>,
    pub rock_nz: HybridMulti<Perlin>,
    pub tree_nz: BasicMulti<Perlin>,

    // TODO: unused, remove??? @zesterer
    pub _cave_0_nz: SuperSimplex,
    pub _cave_1_nz: SuperSimplex,

    pub structure_gen: StructureGen2d,
    pub _big_structure_gen: StructureGen2d,
    pub _region_gen: StructureGen2d,

    pub _fast_turb_x_nz: FastNoise,
    pub _fast_turb_y_nz: FastNoise,

    pub _town_gen: StructureGen2d,
    pub river_seed: RandomField,
    pub rock_strength_nz: Fbm<Perlin>,
    pub uplift_nz: util::Worley,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct GenOpts {
    pub x_lg: u32,
    pub y_lg: u32,
    pub scale: f64,
    pub map_kind: MapKind,
    pub erosion_quality: f32,
}

impl Default for GenOpts {
    fn default() -> Self {
        Self {
            x_lg: 10,
            y_lg: 10,
            scale: 2.0,
            map_kind: MapKind::Square,
            erosion_quality: 1.0,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum FileOpts {
    /// If set, generate the world map and do not try to save to or load from
    /// file (default).
    Generate(GenOpts),
    /// If set, generate the world map and save the world file (path is created
    /// the same way screenshot paths are).
    Save(PathBuf, GenOpts),
    /// Combination of Save and Load.
    /// Load map if exists or generate the world map and save the
    /// world file.
    LoadOrGenerate {
        name: String,
        #[serde(default)]
        opts: GenOpts,
        #[serde(default)]
        overwrite: bool,
    },
    /// If set, explicitly import a legacy or sidecarless external world file
    /// from this path using weak compat inference instead of strict sidecar
    /// enforcement. This option is transitional and may be removed once the
    /// compat-import tail is retired.
    LoadLegacy(PathBuf),
    /// If set, load the world file from this path (errors if path not found).
    Load(PathBuf),
    /// If set, look for  the world file at this asset specifier (errors if
    /// asset is not found).
    ///
    /// NOTE: Could stand to merge this with `Load` and construct an enum that
    /// can handle either a PathBuf or an asset specifier, at some point.
    LoadAsset(String),
}

impl Default for FileOpts {
    fn default() -> Self { Self::Generate(GenOpts::default()) }
}

struct FileLoadContent {
    parsed_world_file: Option<ModernMap>,
    loaded_recipe_manifest: Option<RecipeManifestV1>,
    map_size_lg: MapSizeLg,
    gen_opts: GenOpts,
    compat_audit: CompatAuditV1,
    managed_recipe_sidecar_missing: bool,
}

struct WorldLoadBootstrap {
    parsed_world_file: Option<ModernMap>,
    map_size_lg: MapSizeLg,
    gen_opts: GenOpts,
    compat_audit: CompatAuditV1,
    managed_recipe_sidecar_missing: bool,
    recipe_manifest: RecipeManifestV1,
    fresh: bool,
    effective_seed: u32,
    effective_seed_elements: bool,
}

struct LoadedMapContent {
    map: ModernMap,
    recipe_manifest: Option<RecipeManifestV1>,
    inferred_gen_opts: Option<GenOpts>,
}

impl FileOpts {
    #[cfg(test)]
    fn load_content(
        &self,
        compat_mode: CompatMode,
        world_seed: u32,
        seed_elements: bool,
    ) -> Result<FileLoadContent, compat::CompatResolveError> {
        self.load_content_with_policy_modes(
            compat_mode,
            LoadLegacyMode::Allow,
            LoadOrGenerateSidecarlessMode::Allow,
            world_seed,
            seed_elements,
        )
    }

    #[cfg(test)]
    fn load_content_with_legacy_mode(
        &self,
        compat_mode: CompatMode,
        load_legacy_mode: LoadLegacyMode,
        world_seed: u32,
        seed_elements: bool,
    ) -> Result<FileLoadContent, compat::CompatResolveError> {
        self.load_content_with_policy_modes(
            compat_mode,
            load_legacy_mode,
            LoadOrGenerateSidecarlessMode::Allow,
            world_seed,
            seed_elements,
        )
    }

    fn load_content_with_policy_modes(
        &self,
        compat_mode: CompatMode,
        load_legacy_mode: LoadLegacyMode,
        load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode,
        world_seed: u32,
        seed_elements: bool,
    ) -> Result<FileLoadContent, compat::CompatResolveError> {
        if matches!(self, Self::LoadLegacy(_)) && matches!(load_legacy_mode, LoadLegacyMode::Deny) {
            warn!(
                load_legacy_mode = %load_legacy_mode.as_str(),
                "LoadLegacy(path) is disabled by configured compat-import gate"
            );
            return Err(compat::CompatResolveError {
                audit: CompatAuditV1::reject(
                    compat::entry_kind(self),
                    CompatFailureKindV1::PolicyDenied,
                    CompatFailureSubjectV1::Options,
                    CompatFailureDetailV1::default(),
                ),
            });
        }

        let requested_gen_opts = self.gen_opts();
        let requested_recipe_manifest = requested_gen_opts
            .as_ref()
            .map(|gen_opts| RecipeManifestV1::record_only(world_seed, gen_opts, seed_elements));
        let compat_resolution = compat::resolve(
            compat_mode,
            compat::entry_kind(self),
            self.try_load_map_raw(requested_recipe_manifest.as_ref()),
        )?;
        load_content::build_file_load_content(
            self,
            requested_gen_opts,
            compat_resolution,
            load_or_generate_sidecarless_mode,
        )
    }

    fn gen_opts(&self) -> Option<GenOpts> {
        match self {
            Self::Generate(opts) | Self::Save(_, opts) | Self::LoadOrGenerate { opts, .. } => {
                Some(opts.clone())
            },
            _ => None,
        }
    }

    // TODO: this should return Option so that caller can choose fallback
    fn map_size(&self) -> MapSizeLg {
        match self {
            Self::Generate(opts) | Self::Save(_, opts) | Self::LoadOrGenerate { opts, .. } => {
                MapSizeLg::new(Vec2 {
                    x: opts.x_lg,
                    y: opts.y_lg,
                })
                .unwrap_or_else(|e| {
                    warn!("World size does not satisfy invariants: {:?}", e);
                    DEFAULT_WORLD_CHUNKS_LG
                })
            },
            _ => DEFAULT_WORLD_CHUNKS_LG,
        }
    }

    fn basic_load_failure(kind: CompatFailureKindV1) -> compat::RawCompatFailure {
        compat::RawCompatFailure::new(kind)
    }

    fn structured_load_failure(
        kind: CompatFailureKindV1,
        subject: CompatFailureSubjectV1,
        detail: CompatFailureDetailV1,
    ) -> compat::RawCompatFailure {
        compat::RawCompatFailure::structured(kind, subject, detail)
    }

    fn load_or_generate_contract_outcome(
        overwrite: bool,
        failure: compat::RawCompatFailure,
    ) -> compat::RawLoadOutcome<LoadedMapContent> {
        if overwrite {
            compat::RawLoadOutcome::Failed(failure)
        } else {
            compat::RawLoadOutcome::Rejected(failure)
        }
    }

    fn recipe_sidecar_path_for_map_path(map_path: &std::path::Path) -> PathBuf {
        file_ops::recipe_sidecar_path_for_map_path(map_path)
    }

    fn recipe_sidecar_path(&self) -> Option<PathBuf> {
        self.map_path()
            .map(|path| Self::recipe_sidecar_path_for_map_path(path.as_path()))
    }

    fn load_recipe_sidecar(
        map_path: &std::path::Path,
    ) -> Result<Option<RecipeManifestV1>, compat::RawCompatFailure> {
        file_ops::load_recipe_sidecar(map_path)
    }

    // Built-in asset manifests describe the runtime contract we enforce for
    // read-only asset loads; they are not retroactive provenance for arbitrary
    // third-party or historical asset worlds. In particular, the default asset
    // keeps its longstanding runtime biome seed while using the asset's
    // recorded world-shape gen opts.
    fn load_asset_recipe_manifest(
        specifier: &str,
        map: &ModernMap,
    ) -> Result<RecipeManifestV1, compat::RawCompatFailure> {
        file_ops::load_asset_recipe_manifest(specifier, map)
    }

    fn recipe_manifest_world_contract_failure(
        map: &ModernMap,
        recipe_manifest: &RecipeManifestV1,
    ) -> compat::RawCompatFailure {
        file_ops::recipe_manifest_world_contract_failure(map, recipe_manifest)
    }

    fn inferred_gen_opts_from_map(map: &ModernMap) -> GenOpts {
        file_ops::inferred_gen_opts_from_map(map)
    }

    // TODO: This should probably return a Result, so that caller can choose
    // whether to log error
    fn try_load_map_raw(
        &self,
        requested_recipe_manifest: Option<&RecipeManifestV1>,
    ) -> compat::RawLoadOutcome<LoadedMapContent> {
        let map = match self {
            Self::LoadLegacy(path) => {
                let file = match File::open(path) {
                    Ok(file) => file,
                    Err(e) => {
                        warn!(?e, ?path, "Couldn't read path for maps");
                        return compat::RawLoadOutcome::Rejected(Self::structured_load_failure(
                            CompatFailureKindV1::MissingInput,
                            CompatFailureSubjectV1::World,
                            CompatFailureDetailV1::default(),
                        ));
                    },
                };

                let mut modern_reader = BufReader::new(file);
                match decode_from_std_read::<WorldFile, _, _>(&mut modern_reader, legacy()) {
                    Ok(map) => match map {
                        WorldFile::Veloren0_7_0(map) => {
                            warn!(
                                ?path,
                                "LoadLegacy(path) imported modern world file through explicit \
                                 compat import path"
                            );
                            Ok(LoadedMapContent {
                                inferred_gen_opts: Some(Self::inferred_gen_opts_from_map(&map)),
                                map,
                                recipe_manifest: None,
                            })
                        },
                        WorldFile::Veloren0_5_0(map) => match map.into_modern() {
                            Ok(map) => Ok(LoadedMapContent {
                                inferred_gen_opts: Some(Self::inferred_gen_opts_from_map(&map)),
                                map,
                                recipe_manifest: None,
                            }),
                            Err(e) => {
                                warn!(
                                    ?path,
                                    ?e,
                                    "LoadLegacy(path) parsed a legacy world file, but it failed \
                                     explicit compat import validation"
                                );
                                return compat::RawLoadOutcome::Rejected(
                                    Self::structured_load_failure(
                                        CompatFailureKindV1::InvalidWorld,
                                        CompatFailureSubjectV1::World,
                                        CompatFailureDetailV1::default(),
                                    ),
                                );
                            },
                        },
                    },
                    Err(modern_err) => {
                        let file = match File::open(path) {
                            Ok(file) => file,
                            Err(e) => {
                                warn!(?e, ?path, "Couldn't reopen path for legacy compat import");
                                return compat::RawLoadOutcome::Rejected(
                                    Self::structured_load_failure(
                                        CompatFailureKindV1::MissingInput,
                                        CompatFailureSubjectV1::World,
                                        CompatFailureDetailV1::default(),
                                    ),
                                );
                            },
                        };

                        let mut legacy_reader = BufReader::new(file);
                        let map: WorldFileLegacy =
                            match decode_from_std_read(&mut legacy_reader, legacy()) {
                                Ok(map) => map,
                                Err(legacy_err) => {
                                    warn!(
                                        ?path,
                                        ?modern_err,
                                        ?legacy_err,
                                        "LoadLegacy(path) could not parse modern or legacy world \
                                         file"
                                    );
                                    return compat::RawLoadOutcome::Rejected(
                                        Self::structured_load_failure(
                                            CompatFailureKindV1::ParseError,
                                            CompatFailureSubjectV1::World,
                                            CompatFailureDetailV1::default(),
                                        ),
                                    );
                                },
                            };

                        match map.into_modern() {
                            Ok(map) => Ok(LoadedMapContent {
                                inferred_gen_opts: Some(Self::inferred_gen_opts_from_map(&map)),
                                map,
                                recipe_manifest: None,
                            }),
                            Err(e) => {
                                warn!(
                                    ?path,
                                    ?e,
                                    "LoadLegacy(path) parsed a legacy world file, but it failed \
                                     explicit compat import validation"
                                );
                                return compat::RawLoadOutcome::Rejected(
                                    Self::structured_load_failure(
                                        CompatFailureKindV1::InvalidWorld,
                                        CompatFailureSubjectV1::World,
                                        CompatFailureDetailV1::default(),
                                    ),
                                );
                            },
                        }
                    },
                }
            },
            Self::Load(path) => {
                let file = match File::open(path) {
                    Ok(file) => file,
                    Err(e) => {
                        warn!(?e, ?path, "Couldn't read path for maps");
                        return compat::RawLoadOutcome::Failed(Self::basic_load_failure(
                            CompatFailureKindV1::MissingInput,
                        ));
                    },
                };

                let mut reader = BufReader::new(file);
                let map: WorldFile = match decode_from_std_read(&mut reader, legacy()) {
                    Ok(map) => map,
                    Err(e) => {
                        warn!(
                            ?e,
                            "Couldn't parse modern map.  Maybe you meant to try a legacy load?"
                        );
                        return compat::RawLoadOutcome::Failed(Self::basic_load_failure(
                            CompatFailureKindV1::ParseError,
                        ));
                    },
                };

                let map = match map {
                    WorldFile::Veloren0_7_0(map) => map,
                    WorldFile::Veloren0_5_0(_) => {
                        let failure = Self::structured_load_failure(
                            CompatFailureKindV1::InvalidWorld,
                            CompatFailureSubjectV1::World,
                            CompatFailureDetailV1::legacy_world_version(),
                        );
                        warn!(
                            ?path,
                            compat_failure = %CompatFailureKindV1::InvalidWorld.as_str(),
                            compat_subject = %CompatFailureSubjectV1::World.as_str(),
                            "Load(path) found a legacy world file version; refusing strict modern load"
                        );
                        return compat::RawLoadOutcome::Rejected(failure);
                    },
                };

                let stored_recipe_manifest = match Self::load_recipe_sidecar(path.as_path()) {
                    Ok(Some(recipe_manifest)) => recipe_manifest,
                    Ok(None) => {
                        let failure = Self::structured_load_failure(
                            CompatFailureKindV1::MissingInput,
                            CompatFailureSubjectV1::Recipe,
                            CompatFailureDetailV1::default(),
                        );
                        warn!(
                            ?path,
                            recipe_sidecar_path =
                                ?Self::recipe_sidecar_path_for_map_path(path.as_path()),
                            compat_failure = %CompatFailureKindV1::MissingInput.as_str(),
                            compat_subject = %CompatFailureSubjectV1::Recipe.as_str(),
                            "Load(path) requires adjacent recipe sidecar; use LoadLegacy(path) for explicit compat import of sidecarless worlds"
                        );
                        return compat::RawLoadOutcome::Rejected(failure);
                    },
                    Err(failure) => {
                        return compat::RawLoadOutcome::Rejected(failure);
                    },
                };

                let failure =
                    Self::recipe_manifest_world_contract_failure(&map, &stored_recipe_manifest);
                if failure.detail.world_size_mismatch || failure.detail.world_scale_mismatch {
                    let stored_gen_opts = &stored_recipe_manifest.world_recipe.gen_opts;
                    warn!(
                        ?path,
                        recipe_sidecar_path =
                            ?Self::recipe_sidecar_path_for_map_path(path.as_path()),
                        stored_world_recipe_hash = %stored_recipe_manifest.world_recipe_hash,
                        stored_topology_id = %stored_recipe_manifest.world_recipe.topology_id.as_str(),
                        world_file_size = ?map.map_size_lg,
                        recipe_size = ?Vec2::new(stored_gen_opts.x_lg, stored_gen_opts.y_lg),
                        world_file_scale = map.continent_scale_hack,
                        recipe_scale = stored_gen_opts.scale,
                        "Load(path) found recipe sidecar that does not match the external world file"
                    );
                    return compat::RawLoadOutcome::Rejected(failure);
                }

                Ok(LoadedMapContent {
                    inferred_gen_opts: None,
                    map,
                    recipe_manifest: Some(stored_recipe_manifest),
                })
            },
            Self::LoadAsset(specifier) => {
                let map = match WorldFile::load_owned(specifier) {
                    Ok(map) => map,
                    Err(err) => {
                        let failure_kind = match err.reason().downcast_ref::<std::io::Error>() {
                            Some(e) if e.kind() == std::io::ErrorKind::NotFound => {
                                warn!(?e, ?specifier, "Couldn't find asset specifier for maps");
                                CompatFailureKindV1::MissingInput
                            },
                            Some(e) => {
                                warn!(?e, ?specifier, "Couldn't read asset specifier for maps");
                                CompatFailureKindV1::ParseError
                            },
                            None => {
                                warn!(
                                    ?err,
                                    ?specifier,
                                    "Couldn't parse modern asset map.  Maybe you meant to try a \
                                     legacy load?"
                                );
                                CompatFailureKindV1::ParseError
                            },
                        };
                        return compat::RawLoadOutcome::Rejected(Self::structured_load_failure(
                            failure_kind,
                            CompatFailureSubjectV1::World,
                            CompatFailureDetailV1::default(),
                        ));
                    },
                };

                let map = match map {
                    WorldFile::Veloren0_7_0(map) => map,
                    WorldFile::Veloren0_5_0(_) => {
                        let failure = Self::structured_load_failure(
                            CompatFailureKindV1::InvalidWorld,
                            CompatFailureSubjectV1::World,
                            CompatFailureDetailV1::legacy_world_version(),
                        );
                        warn!(
                            ?specifier,
                            compat_failure = %CompatFailureKindV1::InvalidWorld.as_str(),
                            compat_subject = %CompatFailureSubjectV1::World.as_str(),
                            "LoadAsset(asset) found a legacy world file version; refusing strict asset load"
                        );
                        return compat::RawLoadOutcome::Rejected(failure);
                    },
                };

                let stored_recipe_manifest = match Self::load_asset_recipe_manifest(specifier, &map)
                {
                    Ok(recipe_manifest) => recipe_manifest,
                    Err(failure) => return compat::RawLoadOutcome::Rejected(failure),
                };

                Ok(LoadedMapContent {
                    inferred_gen_opts: None,
                    map,
                    recipe_manifest: Some(stored_recipe_manifest),
                })
            },
            Self::LoadOrGenerate {
                opts, overwrite, ..
            } => {
                // `unwrap` is safe here, because LoadOrGenerate has its path
                // always defined
                let path = self.map_path().unwrap();

                let file = match File::open(&path) {
                    Ok(file) => file,
                    Err(e) => {
                        warn!(?e, ?path, "Couldn't find needed map. Generating...");
                        return compat::RawLoadOutcome::Failed(Self::basic_load_failure(
                            CompatFailureKindV1::MissingInput,
                        ));
                    },
                };

                let mut reader = BufReader::new(file);
                let map: WorldFile = match decode_from_std_read(&mut reader, legacy()) {
                    Ok(map) => map,
                    Err(e) => {
                        warn!(
                            ?e,
                            "Couldn't parse modern map.  Maybe you meant to try a legacy load?"
                        );
                        return compat::RawLoadOutcome::Failed(Self::basic_load_failure(
                            CompatFailureKindV1::ParseError,
                        ));
                    },
                };

                // FIXME:
                // We check if we need to generate new map by comparing gen opts.
                // But we also have another generation paramater that currently
                // passed outside and used for both worldsim and worldgen.
                //
                // Ideally, we need to figure out how we want to use seed, i. e.
                // moving worldgen seed to gen opts and use different sim seed from
                // server config or grab sim seed from world file.
                //
                // NOTE: we intentionally use pattern-matching here to get
                // options, so that when gen opts get another field, compiler
                // will force you to update following logic
                let GenOpts {
                    x_lg, y_lg, scale, ..
                } = opts;
                let map = match map {
                    WorldFile::Veloren0_7_0(map) => map,
                    WorldFile::Veloren0_5_0(_) => {
                        let failure = Self::structured_load_failure(
                            CompatFailureKindV1::InvalidWorld,
                            CompatFailureSubjectV1::World,
                            CompatFailureDetailV1::legacy_world_version(),
                        );
                        warn!(
                            ?path,
                            overwrite = *overwrite,
                            compat_failure = %CompatFailureKindV1::InvalidWorld.as_str(),
                            compat_subject = %CompatFailureSubjectV1::World.as_str(),
                            "LoadOrGenerate found a legacy world file version; refusing to reuse it silently"
                        );
                        return Self::load_or_generate_contract_outcome(*overwrite, failure);
                    },
                };
                let mut loaded_recipe_manifest = None;

                if let Some(requested_recipe_manifest) = requested_recipe_manifest {
                    match Self::load_recipe_sidecar(path.as_path()) {
                        Ok(Some(stored_recipe_manifest)) => {
                            if stored_recipe_manifest.world_recipe_hash
                                != requested_recipe_manifest.world_recipe_hash
                            {
                                let failure = Self::structured_load_failure(
                                    CompatFailureKindV1::OptionMismatch,
                                    CompatFailureSubjectV1::Recipe,
                                    CompatFailureDetailV1::default(),
                                );
                                if *overwrite {
                                    warn!(
                                        ?path,
                                        recipe_sidecar_path = ?Self::recipe_sidecar_path_for_map_path(path.as_path()),
                                        overwrite = *overwrite,
                                        stored_world_seed = stored_recipe_manifest.world_recipe.world_seed,
                                        requested_world_seed = requested_recipe_manifest.world_recipe.world_seed,
                                        stored_world_recipe_hash = %stored_recipe_manifest.world_recipe_hash,
                                        requested_world_recipe_hash = %requested_recipe_manifest.world_recipe_hash,
                                        stored_topology_id = %stored_recipe_manifest.world_recipe.topology_id.as_str(),
                                        requested_topology_id = %requested_recipe_manifest.world_recipe.topology_id.as_str(),
                                        stored_preset_id = %stored_recipe_manifest.world_recipe.preset_id.as_str(),
                                        requested_preset_id = %requested_recipe_manifest.world_recipe.preset_id.as_str(),
                                        "LoadOrGenerate recipe sidecar mismatch; regenerating because overwrite=true"
                                    );
                                } else {
                                    warn!(
                                        ?path,
                                        recipe_sidecar_path = ?Self::recipe_sidecar_path_for_map_path(path.as_path()),
                                        overwrite = *overwrite,
                                        stored_world_seed = stored_recipe_manifest.world_recipe.world_seed,
                                        requested_world_seed = requested_recipe_manifest.world_recipe.world_seed,
                                        stored_world_recipe_hash = %stored_recipe_manifest.world_recipe_hash,
                                        requested_world_recipe_hash = %requested_recipe_manifest.world_recipe_hash,
                                        stored_topology_id = %stored_recipe_manifest.world_recipe.topology_id.as_str(),
                                        requested_topology_id = %requested_recipe_manifest.world_recipe.topology_id.as_str(),
                                        stored_preset_id = %stored_recipe_manifest.world_recipe.preset_id.as_str(),
                                        requested_preset_id = %requested_recipe_manifest.world_recipe.preset_id.as_str(),
                                        "LoadOrGenerate recipe sidecar mismatch; rejecting because overwrite=false"
                                    );
                                }

                                return Self::load_or_generate_contract_outcome(
                                    *overwrite, failure,
                                );
                            }

                            loaded_recipe_manifest = Some(stored_recipe_manifest);
                        },
                        Ok(None) => {
                            debug!(
                                ?path,
                                recipe_sidecar_path = ?Self::recipe_sidecar_path_for_map_path(path.as_path()),
                                "LoadOrGenerate map has no recipe sidecar; using legacy option compare"
                            );
                        },
                        Err(failure) => {
                            return Self::load_or_generate_contract_outcome(*overwrite, failure);
                        },
                    }
                }

                let requested_size = Vec2::new(*x_lg, *y_lg);
                let world_scale_mismatch = map.continent_scale_hack != *scale;
                let world_size_mismatch = map.map_size_lg != requested_size;
                if world_scale_mismatch || world_size_mismatch {
                    let failure = Self::structured_load_failure(
                        CompatFailureKindV1::OptionMismatch,
                        CompatFailureSubjectV1::Options,
                        CompatFailureDetailV1::option_mismatch(
                            world_size_mismatch,
                            world_scale_mismatch,
                        ),
                    );
                    if *overwrite {
                        warn!(
                            ?path,
                            requested_size = ?requested_size,
                            stored_size = ?map.map_size_lg,
                            requested_scale = *scale,
                            stored_scale = map.continent_scale_hack,
                            "Specified options don't correspond to the loaded map; regenerating because overwrite=true"
                        );
                    } else {
                        warn!(
                            ?path,
                            requested_size = ?requested_size,
                            stored_size = ?map.map_size_lg,
                            requested_scale = *scale,
                            stored_scale = map.continent_scale_hack,
                            "Specified options don't correspond to the loaded map; rejecting because overwrite=false"
                        );
                    }

                    return Self::load_or_generate_contract_outcome(*overwrite, failure);
                }

                Ok(LoadedMapContent {
                    inferred_gen_opts: loaded_recipe_manifest
                        .is_none()
                        .then(|| Self::inferred_gen_opts_from_map(&map)),
                    map,
                    recipe_manifest: loaded_recipe_manifest,
                })
            },
            Self::Generate(_) | Self::Save(_, _) => {
                return compat::RawLoadOutcome::GenerateRequested;
            },
        };

        match map {
            Ok(map) => compat::RawLoadOutcome::Loaded(map),
            Err(e) => {
                match e {
                    WorldFileError::WorldSizeInvalid => {
                        warn!("World size of map is invalid.");
                    },
                }
                compat::RawLoadOutcome::Failed(Self::basic_load_failure(
                    CompatFailureKindV1::InvalidWorld,
                ))
            },
        }
    }

    fn map_path(&self) -> Option<PathBuf> {
        // TODO: Work out a nice bincode file extension.
        match self {
            Self::Save(path, _) => Some(PathBuf::from(&path)),
            Self::LoadOrGenerate { name, .. } => {
                const MAP_DIR: &str = "./maps";
                let file_name = format!("{}.bin", name);
                Some(std::path::Path::new(MAP_DIR).join(file_name))
            },
            _ => None,
        }
    }

    fn save(&self, map: &WorldFile, recipe_manifest: &RecipeManifestV1) {
        let path = if let Some(path) = self.map_path() {
            path
        } else {
            return;
        };

        // Check if folder exists and create it if it does not
        let map_dir = path.parent().expect("failed to get map directory");
        if !map_dir.exists()
            && let Err(e) = std::fs::create_dir_all(map_dir)
        {
            warn!(?e, ?map_dir, "Couldn't create folder for map");
            return;
        }

        let file = match File::create(path.clone()) {
            Ok(file) => file,
            Err(e) => {
                warn!(?e, ?path, "Couldn't create file for maps");
                return;
            },
        };

        let mut writer = BufWriter::new(file);
        if let Err(e) = encode_into_std_write(map, &mut writer, legacy()) {
            warn!(?e, "Couldn't write map");
            return;
        }
        if let Some(sidecar_path) = self.recipe_sidecar_path() {
            let rendered_recipe_manifest = match ron::ser::to_string_pretty(
                recipe_manifest,
                ron::ser::PrettyConfig::default(),
            ) {
                Ok(rendered_recipe_manifest) => rendered_recipe_manifest,
                Err(e) => {
                    warn!(?e, ?sidecar_path, "Couldn't serialize world recipe sidecar");
                    return;
                },
            };

            if let Err(e) = std::fs::write(&sidecar_path, rendered_recipe_manifest) {
                warn!(?e, ?sidecar_path, "Couldn't write world recipe sidecar");
            }
        }
        if let Ok(p) = std::fs::canonicalize(path) {
            info!("Map saved at {}", p.to_string_lossy());
        }
    }
}

pub struct WorldOpts {
    /// Set to false to disable seeding elements during worldgen.
    pub seed_elements: bool,
    pub world_file: FileOpts,
    pub calendar: Option<Calendar>,
    pub compat_mode: CompatMode,
    /// Controls whether the transitional `LoadLegacy(path)` compat-import
    /// entry remains admitted or is rejected before any file parsing.
    pub load_legacy_mode: LoadLegacyMode,
    /// Controls whether managed `LoadOrGenerate` may still reuse an existing
    /// world when the adjacent recipe sidecar is missing.
    pub load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode,
}

impl Default for WorldOpts {
    fn default() -> Self {
        Self {
            seed_elements: true,
            world_file: Default::default(),
            calendar: None,
            compat_mode: CompatMode::Record,
            load_legacy_mode: LoadLegacyMode::Allow,
            load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode::Allow,
        }
    }
}

/// LEGACY: Remove when people stop caring.
#[derive(Serialize, Deserialize)]
#[repr(C)]
pub struct WorldFileLegacy {
    /// Saved altitude height map.
    pub alt: Box<[Alt]>,
    /// Saved basement height map.
    pub basement: Box<[Alt]>,
}

/// Version of the world map intended for use in Veloren 0.5.0.
#[derive(Serialize, Deserialize)]
#[repr(C)]
pub struct WorldMap_0_5_0 {
    /// Saved altitude height map.
    pub alt: Box<[Alt]>,
    /// Saved basement height map.
    pub basement: Box<[Alt]>,
}

/// Version of the world map intended for use in Veloren 0.7.0.
#[derive(Serialize, Deserialize)]
#[repr(C)]
pub struct WorldMap_0_7_0 {
    /// Saved map size.
    pub map_size_lg: Vec2<u32>,
    /// Saved continent_scale hack, to try to better approximate the correct
    /// seed according to varying map size.
    ///
    /// TODO: Remove when generating new maps becomes more principled.
    pub continent_scale_hack: f64,
    /// Saved altitude height map.
    pub alt: Box<[Alt]>,
    /// Saved basement height map.
    pub basement: Box<[Alt]>,
}

/// Errors when converting a map to the most recent type (currently,
/// shared by the various map types, but at some point we might switch to
/// version-specific errors if it feels worthwhile).
#[derive(Debug)]
pub enum WorldFileError {
    /// Map size was invalid, and it can't be converted to a valid one.
    WorldSizeInvalid,
}

/// WORLD MAP.
///
/// A way to store certain components between runs of map generation.  Only
/// intended for development purposes--no attempt is made to detect map
/// invalidation or make sure that the map is synchronized with updates to
/// noise-rs, changes to other parameters, etc.
///
/// The map is versioned to enable format detection between versions of Veloren,
/// so that when we update the map format we don't break existing maps (or at
/// least, we will try hard not to break maps between versions; if we can't
/// avoid it, we can at least give a reasonable error message).
///
/// NOTE: We rely somewhat heavily on the implementation specifics of bincode
/// to make sure this is backwards compatible.  When adding new variants here,
/// Be very careful to make sure tha the old variants are preserved in the
/// correct order and with the correct names and indices, and make sure to keep
/// the #[repr(u32)]!
///
/// All non-legacy versions of world files should (ideally) fit in this format.
/// Since the format contains a version and is designed to be extensible
/// backwards-compatibly, the only reason not to use this forever would be if we
/// decided to move away from BinCode, or store data across multiple files (or
/// something else weird I guess).
///
/// Update this when you add a new map version.
#[derive(Serialize, Deserialize)]
#[repr(u32)]
pub enum WorldFile {
    Veloren0_5_0(WorldMap_0_5_0) = 0,
    Veloren0_7_0(WorldMap_0_7_0) = 1,
}

impl FileAsset for WorldFile {
    const EXTENSION: &'static str = "bin";

    fn from_bytes(bytes: Cow<[u8]>) -> Result<Self, BoxedError> { load_bincode_legacy(&bytes) }
}

/// Data for the most recent map type.  Update this when you add a new map
/// version.
pub type ModernMap = WorldMap_0_7_0;

/// The default world map.
///
/// TODO: Consider using some naming convention to automatically change this
/// with changing versions, or at least keep it in a constant somewhere that's
/// easy to change.
// Generation parameters:
//
// gen_opts: (
//     erosion_quality: 1.0,
//     map_kind: Circle,
//     scale: 2.157574498096227,
//     x_lg: 10,
//     y_lg: 10,
// )
// seed: 3582734543
//
// The biome seed can found below
pub const DEFAULT_WORLD_MAP: &str = "world.map.veloren_0_18_0_0";
/// This is *not* the seed used to generate the default map, this seed was used
/// to generate a better set of biomes on it as the original ones were
/// unsuitable.
///
/// See DEFAULT_WORLD_MAP to get the original worldgen parameters.
pub const DEFAULT_WORLD_SEED: u32 = 130626853;

fn default_world_asset_gen_opts() -> GenOpts {
    GenOpts {
        x_lg: 10,
        y_lg: 10,
        scale: 2.157574498096227,
        map_kind: MapKind::Circle,
        erosion_quality: 1.0,
    }
}

impl WorldFileLegacy {
    #[inline]
    /// Idea: each map type except the latest knows how to transform
    /// into the the subsequent map version, and each map type including the
    /// latest exposes an "into_modern()" method that converts this map type
    /// to the modern map type.  Thus, to migrate a map from an old format to a
    /// new format, we just need to transform the old format to the
    /// subsequent map version, and then call .into_modern() on that--this
    /// should construct a call chain that ultimately ends up with a modern
    /// version.
    pub fn into_modern(self) -> Result<ModernMap, WorldFileError> {
        // NOTE: At this point, we assume that any remaining legacy maps were 1024 ×
        // 1024.
        if self.alt.len() != self.basement.len() || self.alt.len() != 1024 * 1024 {
            return Err(WorldFileError::WorldSizeInvalid);
        }

        let map = WorldMap_0_5_0 {
            alt: self.alt,
            basement: self.basement,
        };

        map.into_modern()
    }
}

impl WorldMap_0_5_0 {
    #[inline]
    pub fn into_modern(self) -> Result<ModernMap, WorldFileError> {
        let pow_size = (self.alt.len().trailing_zeros()) / 2;
        let two_coord_size = 1 << (2 * pow_size);
        if self.alt.len() != self.basement.len() || self.alt.len() != two_coord_size {
            return Err(WorldFileError::WorldSizeInvalid);
        }

        // The recommended continent scale for maps from version 0.5.0 is (in all
        // existing cases) just 1.0 << (f64::from(pow_size) - 10.0).
        let continent_scale_hack = (f64::from(pow_size) - 10.0).exp2();

        let map = WorldMap_0_7_0 {
            map_size_lg: Vec2::new(pow_size, pow_size),
            continent_scale_hack,
            alt: self.alt,
            basement: self.basement,
        };

        map.into_modern()
    }
}

impl WorldMap_0_7_0 {
    #[inline]
    pub fn into_modern(self) -> Result<ModernMap, WorldFileError> {
        if self.alt.len() != self.basement.len()
            || self.alt.len() != (1 << (self.map_size_lg.x + self.map_size_lg.y))
            || self.continent_scale_hack <= 0.0
        {
            return Err(WorldFileError::WorldSizeInvalid);
        }

        Ok(self)
    }
}

impl WorldFile {
    /// Turns map data from the latest version into a versioned WorldFile ready
    /// for serialization. Whenever a new map is updated, just change the
    /// variant we construct here to make sure we're using the latest map
    /// version.
    pub fn new(map: ModernMap) -> Self { WorldFile::Veloren0_7_0(map) }

    #[inline]
    /// Turns a WorldFile into the latest version.  Whenever a new map version
    /// is added, just add it to this match statement.
    pub fn into_modern(self) -> Result<ModernMap, WorldFileError> {
        match self {
            WorldFile::Veloren0_5_0(map) => map.into_modern(),
            WorldFile::Veloren0_7_0(map) => map.into_modern(),
        }
    }
}

#[derive(Debug)]
pub enum WorldSimStage {
    // TODO: Add more stages
    Erosion {
        progress: f64,
        estimate: Option<std::time::Duration>,
    },
}

pub struct WorldSim {
    pub seed: u32,
    /// Base 2 logarithm of the map size.
    map_size_lg: MapSizeLg,
    /// Maximum height above sea level of any chunk in the map (not including
    /// post-erosion warping, cliffs, and other things like that).
    pub max_height: f32,
    pub(crate) chunks: Vec<SimChunk>,
    //TODO: remove or use this property
    pub(crate) _locations: Vec<Location>,

    pub(crate) gen_ctx: GenCtx,
    pub rng: ChaChaRng,

    pub(crate) calendar: Option<Calendar>,
    pub(crate) compat_mode: CompatMode,
    pub(crate) load_legacy_mode: LoadLegacyMode,
    pub(crate) load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode,
    pub(crate) compat_audit: CompatAuditV1,
    pub(crate) managed_recipe_sidecar_missing: bool,
    pub(crate) recipe_manifest: RecipeManifestV1,
    topology: WorldTopology,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AquaticFaunaSummary {
    pub freshwater_fauna: bool,
    pub coastal_fauna: bool,
    pub shelf_fauna: bool,
    pub pelagic_fauna: bool,
}

impl AquaticFaunaSummary {
    fn from_profile(profile: AquaticFaunaProfile) -> Self {
        Self {
            freshwater_fauna: profile.freshwater_fauna,
            coastal_fauna: profile.coastal_fauna,
            shelf_fauna: profile.shelf_fauna,
            pelagic_fauna: profile.pelagic_fauna,
        }
    }
}

#[derive(Clone, Copy)]
struct ChunkAquaticSemantics {
    aquatic_spawn_potential: AquaticSpawnPotential,
    marine_ecology_profile: MarineEcologyProfile,
}

pub(crate) struct ChunkGenerationAnchor<'a> {
    pub(crate) base_z: i32,
    pub(crate) sim_chunk: &'a SimChunk,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ApproxFallback {
    Zero,
    SeaLevel,
}

impl ApproxFallback {
    const fn value(self) -> f32 {
        match self {
            Self::Zero => 0.0,
            Self::SeaLevel => CONFIG.sea_level,
        }
    }
}

impl WorldSim {
    pub fn empty() -> Self {
        let gen_ctx = GenCtx {
            turb_x_nz: SuperSimplex::new(0),
            turb_y_nz: SuperSimplex::new(0),
            chaos_nz: RidgedMulti::new(0),
            hill_nz: SuperSimplex::new(0),
            alt_nz: util::HybridMulti::new(0),
            temp_nz: Fbm::new(0),

            small_nz: BasicMulti::new(0),
            rock_nz: HybridMulti::new(0),
            tree_nz: BasicMulti::new(0),
            _cave_0_nz: SuperSimplex::new(0),
            _cave_1_nz: SuperSimplex::new(0),

            structure_gen: StructureGen2d::new(0, 24, 10),
            _big_structure_gen: StructureGen2d::new(0, 768, 512),
            _region_gen: StructureGen2d::new(0, 400, 96),
            humid_nz: Billow::new(0),

            _fast_turb_x_nz: FastNoise::new(0),
            _fast_turb_y_nz: FastNoise::new(0),

            _town_gen: StructureGen2d::new(0, 2048, 1024),
            river_seed: RandomField::new(0),
            rock_strength_nz: Fbm::new(0),
            uplift_nz: util::Worley::new(0),
        };
        Self {
            seed: 0,
            map_size_lg: MapSizeLg::new(Vec2::one()).unwrap(),
            max_height: 0.0,
            chunks: vec![SimChunk {
                chaos: 0.0,
                alt: 0.0,
                basement: 0.0,
                water_alt: 0.0,
                downhill: None,
                flux: 0.0,
                temp: 0.0,
                humidity: 0.0,
                rockiness: 0.0,
                tree_density: 0.0,
                forest_kind: ForestKind::Dead,
                spawn_rate: 0.0,
                river: RiverData::default(),
                surface_veg: 0.0,
                sites: vec![],
                place: None,
                poi: None,
                path: Default::default(),
                cliff_height: 0.0,
                spot: None,
                contains_waypoint: false,
            }],
            _locations: Vec::new(),
            gen_ctx,
            rng: rand_chacha::ChaCha20Rng::from_seed([0; 32]),
            calendar: None,
            compat_mode: CompatMode::Record,
            load_legacy_mode: LoadLegacyMode::Allow,
            load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode::Allow,
            compat_audit: CompatAuditV1::default(),
            managed_recipe_sidecar_missing: false,
            recipe_manifest: RecipeManifestV1::default(),
            topology: WorldTopology::new(
                TopologyId::BoundedPlaneV1,
                MapSizeLg::new(Vec2::one()).unwrap(),
            ),
        }
    }

    pub fn generate(
        seed: u32,
        opts: WorldOpts,
        threadpool: &rayon::ThreadPool,
        stage_report: &dyn Fn(WorldSimStage),
    ) -> Result<Self, crate::Error> {
        prof_span!("WorldSim::generate");
        let seed_elements = opts.seed_elements;
        let calendar = opts.calendar; // separate lifetime of elements
        let world_file = opts.world_file;
        let compat_mode = opts.compat_mode;
        let load_legacy_mode = opts.load_legacy_mode;
        let load_or_generate_sidecarless_mode = opts.load_or_generate_sidecarless_mode;
        let load_bootstrap = Self::prepare_world_load_bootstrap(WorldLoadBootstrapRequest {
            seed,
            seed_elements,
            world_file: world_file.clone(),
            compat_mode,
            load_legacy_mode,
            load_or_generate_sidecarless_mode,
        })?;

        Ok(Self::prepare_and_finalize_generated_world(
            GeneratedWorldFinalizePreparationRequest {
                load_bootstrap,
                world_file,
                calendar,
                compat_mode,
                load_legacy_mode,
                load_or_generate_sidecarless_mode,
                threadpool,
                stage_report,
            },
        ))
    }

    fn prepare_pre_erosion_setup(
        map_size_lg: MapSizeLg,
        gen_opts: &GenOpts,
        gen_ctx: &GenCtx,
        generation_tunables: &GenerationTunables,
        threadpool: &rayon::ThreadPool,
    ) -> PreErosionSetup {
        let params = PreErosionParams::new(
            map_size_lg,
            gen_opts,
            generation_tunables.continent_scale,
            generation_tunables.uplift_scale,
        );
        let fields =
            Self::build_pre_erosion_fields(map_size_lg, gen_opts, gen_ctx, &params, threadpool);

        PreErosionSetup { params, fields }
    }

    fn prepare_generation_start(
        effective_seed: u32,
        map_size_lg: MapSizeLg,
        gen_opts: &GenOpts,
        threadpool: &rayon::ThreadPool,
    ) -> WorldGenerationStart {
        let generation_tunables = GenerationTunables::new(gen_opts);

        info!("Starting world generation");

        let (rng, gen_ctx) = Self::init_gen_ctx(
            effective_seed,
            generation_tunables.continent_scale,
            generation_tunables.rock_lacunarity,
            generation_tunables.uplift_scale,
        );
        let pre_erosion_setup = Self::prepare_pre_erosion_setup(
            map_size_lg,
            gen_opts,
            &gen_ctx,
            &generation_tunables,
            threadpool,
        );

        WorldGenerationStart {
            generation_tunables,
            rng,
            gen_ctx,
            pre_erosion_setup,
        }
    }

    fn prepare_world_load_bootstrap(
        request: WorldLoadBootstrapRequest,
    ) -> Result<WorldLoadBootstrap, crate::Error> {
        let WorldLoadBootstrapRequest {
            seed,
            seed_elements,
            world_file,
            compat_mode,
            load_legacy_mode,
            load_or_generate_sidecarless_mode,
        } = request;
        Self::resolve_world_load_bootstrap_with_observability(
            &world_file,
            seed,
            seed_elements,
            compat_mode,
            load_legacy_mode,
            load_or_generate_sidecarless_mode,
        )
    }

    fn resolve_world_load_bootstrap_with_observability(
        world_file: &FileOpts,
        seed: u32,
        seed_elements: bool,
        compat_mode: CompatMode,
        load_legacy_mode: LoadLegacyMode,
        load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode,
    ) -> Result<WorldLoadBootstrap, crate::Error> {
        match world_file.load_content_with_policy_modes(
            compat_mode,
            load_legacy_mode,
            load_or_generate_sidecarless_mode,
            seed,
            seed_elements,
        ) {
            Ok(file_load_content) => Ok(Self::record_world_load_bootstrap_observability(
                load_legacy_mode,
                load_or_generate_sidecarless_mode,
                load_content::build_world_load_bootstrap(seed, seed_elements, file_load_content),
            )),
            Err(err) => {
                Self::record_world_load_contract_rejection(
                    compat_mode,
                    load_legacy_mode,
                    load_or_generate_sidecarless_mode,
                    &err.audit,
                );
                Err(crate::Error::CompatEnforce { audit: err.audit })
            },
        }
    }

    fn record_world_load_bootstrap_observability(
        load_legacy_mode: LoadLegacyMode,
        load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode,
        load_bootstrap: WorldLoadBootstrap,
    ) -> WorldLoadBootstrap {
        Self::record_world_load_contract_observability(
            load_legacy_mode,
            load_or_generate_sidecarless_mode,
            &load_bootstrap.compat_audit,
            load_bootstrap.managed_recipe_sidecar_missing,
            load_bootstrap.fresh,
            &load_bootstrap.recipe_manifest,
        );

        load_bootstrap
    }

    fn record_world_load_contract_observability(
        load_legacy_mode: LoadLegacyMode,
        load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode,
        compat_audit: &CompatAuditV1,
        managed_recipe_sidecar_missing: bool,
        fresh: bool,
        recipe_manifest: &RecipeManifestV1,
    ) {
        info!(
            load_legacy_mode = %load_legacy_mode.as_str(),
            load_or_generate_sidecarless_mode =
                %load_or_generate_sidecarless_mode.as_str(),
            compat_entry = %compat_audit.entry.as_str(),
            compat_decision = %compat_audit.decision.as_str(),
            compat_failure = %compat_audit.failure_kind.as_str(),
            compat_resolution = %compat_audit.resolution.as_str(),
            compat_subject = %compat_audit.failure_subject.as_str(),
            compat_legacy_world_version = compat_audit.failure_detail.legacy_world_version,
            compat_world_size_mismatch = compat_audit.failure_detail.world_size_mismatch,
            compat_world_scale_mismatch = compat_audit.failure_detail.world_scale_mismatch,
            compat_managed_recipe_sidecar_missing = managed_recipe_sidecar_missing,
            fresh,
            "recorded world file compatibility audit"
        );
        if compat_audit.is_strict_load_contract_gap() {
            warn!(
                compat_entry = %compat_audit.entry.as_str(),
                compat_failure = %compat_audit.failure_kind.as_str(),
                "strict load path fell back to generation; C1 keeps this behavior observable before enforce"
            );
        }
        if managed_recipe_sidecar_missing {
            warn!(
                compat_entry = %compat_audit.entry.as_str(),
                "managed world loaded without an adjacent recipe sidecar; runtime recipe contract remains inferred from legacy option compare"
            );
        }
        info!(
            world_recipe_hash = %recipe_manifest.world_recipe_hash,
            chunk_recipe_hash = %recipe_manifest.chunk_recipe_hash,
            topology_id = %recipe_manifest.world_recipe.topology_id.as_str(),
            preset_id = %recipe_manifest.world_recipe.preset_id.as_str(),
            config_hash = %recipe_manifest
                .world_recipe
                .config_hash
                .as_deref()
                .unwrap_or("unrecorded"),
            asset_hash = %recipe_manifest
                .world_recipe
                .asset_hash
                .as_deref()
                .unwrap_or("unrecorded"),
            "recorded world recipe manifest"
        );
    }

    fn record_world_load_contract_rejection(
        compat_mode: CompatMode,
        load_legacy_mode: LoadLegacyMode,
        load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode,
        compat_audit: &CompatAuditV1,
    ) {
        error!(
            compat_mode = %compat_mode.as_str(),
            load_legacy_mode = %load_legacy_mode.as_str(),
            load_or_generate_sidecarless_mode =
                %load_or_generate_sidecarless_mode.as_str(),
            compat_entry = %compat_audit.entry.as_str(),
            compat_decision = %compat_audit.decision.as_str(),
            compat_failure = %compat_audit.failure_kind.as_str(),
            compat_resolution = %compat_audit.resolution.as_str(),
            compat_subject = %compat_audit.failure_subject.as_str(),
            compat_legacy_world_version = compat_audit.failure_detail.legacy_world_version,
            compat_world_size_mismatch = compat_audit.failure_detail.world_size_mismatch,
            compat_world_scale_mismatch = compat_audit.failure_detail.world_scale_mismatch,
            "compat contract rejected world load"
        );
    }

    fn prepare_generation_chunk_inputs(
        request: GenerationChunkInputsRequest<'_>,
    ) -> PostErosionChunkInputs {
        let GenerationChunkInputsRequest {
            parsed_world_file,
            map_size_lg,
            gen_opts,
            world_file,
            fresh,
            recipe_manifest,
            gen_ctx,
            pre_erosion_setup,
            threadpool,
            stage_report,
        } = request;

        // Perform some erosion.
        let mut erosion_reporter = ErosionProgressReporter::new(stage_report);
        let report_erosion: &mut dyn FnMut(f64) =
            &mut |progress: f64| erosion_reporter.report(progress);
        let pre_erosion_model = pre_erosion_setup.model(map_size_lg, gen_ctx);

        let (alt, basement) = Self::materialize_heightfields(
            parsed_world_file,
            gen_opts,
            world_file,
            fresh,
            recipe_manifest,
            &pre_erosion_model,
            threadpool,
            report_erosion,
        );
        let chaos = pre_erosion_setup.into_chaos();

        Self::prepare_post_erosion_chunk_inputs(
            map_size_lg,
            gen_opts.scale,
            gen_ctx,
            chaos,
            alt,
            basement,
            threadpool,
        )
    }

    fn prepare_generated_world_finalize_inputs(
        request: GeneratedWorldFinalizePreparationRequest<'_>,
    ) -> GeneratedWorldFinalizeInputs {
        let builder_inputs = Self::prepare_generated_world_finalize_builder_inputs(
            GeneratedWorldFinalizeBuilderPreparationRequest {
                load_bootstrap: request.load_bootstrap,
                world_file: request.world_file,
                calendar: request.calendar,
                compat_mode: request.compat_mode,
                load_legacy_mode: request.load_legacy_mode,
                load_or_generate_sidecarless_mode: request.load_or_generate_sidecarless_mode,
                threadpool: request.threadpool,
                stage_report: request.stage_report,
            },
        );

        Self::build_generated_world_finalize_inputs(builder_inputs)
    }

    fn prepare_generated_world_finalize_builder_inputs(
        request: GeneratedWorldFinalizeBuilderPreparationRequest<'_>,
    ) -> GeneratedWorldFinalizeBuilderInputs {
        let GeneratedWorldFinalizeBuilderPreparationRequest {
            load_bootstrap,
            world_file,
            calendar,
            compat_mode,
            load_legacy_mode,
            load_or_generate_sidecarless_mode,
            threadpool,
            stage_report,
        } = request;
        let WorldLoadBootstrap {
            parsed_world_file,
            map_size_lg,
            gen_opts,
            compat_audit,
            managed_recipe_sidecar_missing,
            recipe_manifest,
            fresh,
            effective_seed,
            effective_seed_elements,
        } = load_bootstrap;
        let WorldGenerationStart {
            generation_tunables: _generation_tunables,
            rng,
            gen_ctx,
            pre_erosion_setup,
        } = Self::prepare_generation_start(effective_seed, map_size_lg, &gen_opts, threadpool);
        let post_erosion_chunk_inputs =
            Self::prepare_generation_chunk_inputs(GenerationChunkInputsRequest {
                parsed_world_file,
                map_size_lg,
                gen_opts: &gen_opts,
                world_file,
                fresh,
                recipe_manifest: &recipe_manifest,
                gen_ctx: &gen_ctx,
                pre_erosion_setup,
                threadpool,
                stage_report,
            });

        GeneratedWorldFinalizeBuilderInputs {
            seed: effective_seed,
            map_size_lg,
            gen_ctx,
            post_erosion_chunk_inputs,
            rng,
            calendar,
            compat_mode,
            load_legacy_mode,
            load_or_generate_sidecarless_mode,
            compat_audit,
            managed_recipe_sidecar_missing,
            recipe_manifest,
            seed_elements: effective_seed_elements,
        }
    }

    fn build_generated_world_finalize_inputs(
        inputs: GeneratedWorldFinalizeBuilderInputs,
    ) -> GeneratedWorldFinalizeInputs {
        let GeneratedWorldFinalizeBuilderInputs {
            seed,
            map_size_lg,
            gen_ctx,
            post_erosion_chunk_inputs,
            rng,
            calendar,
            compat_mode,
            load_legacy_mode,
            load_or_generate_sidecarless_mode,
            compat_audit,
            managed_recipe_sidecar_missing,
            recipe_manifest,
            seed_elements,
        } = inputs;

        GeneratedWorldFinalizeInputs {
            world_parts_inputs: GeneratedWorldPartsInputs {
                seed,
                map_size_lg,
                gen_ctx,
                post_erosion_chunk_inputs,
                rng,
                calendar,
                compat_mode,
                load_legacy_mode,
                load_or_generate_sidecarless_mode,
                compat_audit,
                managed_recipe_sidecar_missing,
                recipe_manifest,
            },
            seed_elements,
        }
    }

    fn prepare_and_finalize_generated_world(
        request: GeneratedWorldFinalizePreparationRequest<'_>,
    ) -> Self {
        let finalize_inputs = Self::prepare_generated_world_finalize_inputs(request);
        Self::finalize_world_from_chunk_inputs(finalize_inputs)
    }

    fn prepare_post_erosion_chunk_inputs(
        map_size_lg: MapSizeLg,
        continent_scale_hack: f64,
        gen_ctx: &GenCtx,
        chaos: InverseCdf,
        alt: Box<[Alt]>,
        basement: Box<[Alt]>,
        threadpool: &rayon::ThreadPool,
    ) -> PostErosionChunkInputs {
        let post_erosion_hydrology = Self::build_post_erosion_hydrology_core(
            map_size_lg,
            continent_scale_hack,
            &alt,
            threadpool,
        );
        let post_water_cdf_fields = Self::build_post_water_cdf_fields(
            map_size_lg,
            gen_ctx,
            &alt,
            &post_erosion_hydrology.flux,
            &post_erosion_hydrology.rivers,
            threadpool,
        );
        let PostErosionHydrologyCore {
            water_alt,
            dh,
            flux,
            rivers,
            max_height,
        } = post_erosion_hydrology;
        let PostWaterCdfFields {
            pure_flux,
            alt_no_water,
            temp_base,
            humid_base,
        } = post_water_cdf_fields;
        let gen_cdf = GenCdf {
            humid_base,
            temp_base,
            chaos,
            alt,
            basement,
            water_alt,
            dh,
            flux,
            pure_flux,
            alt_no_water,
            rivers,
        };

        PostErosionChunkInputs {
            max_height,
            gen_cdf,
        }
    }

    fn build_pre_erosion_fields(
        map_size_lg: MapSizeLg,
        gen_opts: &GenOpts,
        gen_ctx: &GenCtx,
        pre_erosion_params: &PreErosionParams,
        threadpool: &rayon::ThreadPool,
    ) -> PreErosionFields {
        let (alt_base, chaos) =
            Self::build_base_alt_and_chaos_fields(map_size_lg, gen_opts, gen_ctx, threadpool);
        let alt_old = Self::build_alt_old_field(map_size_lg, gen_ctx, &alt_base, &chaos);
        let (is_ocean, uplift_uniform) =
            Self::build_ocean_and_uplift_fields(map_size_lg, pre_erosion_params, &alt_old);

        PreErosionFields {
            chaos,
            alt_old,
            is_ocean,
            uplift_uniform,
        }
    }

    fn build_base_alt_and_chaos_fields(
        map_size_lg: MapSizeLg,
        gen_opts: &GenOpts,
        gen_ctx: &GenCtx,
        threadpool: &rayon::ThreadPool,
    ) -> (InverseCdf<f64>, InverseCdf) {
        // No NaNs in these uniform vectors, since the original noise value always
        // returns Some.
        let ((alt_base, _), (chaos, _)) = threadpool.join(
            || {
                uniform_noise(map_size_lg, |_, wposf| {
                    match gen_opts.map_kind {
                        MapKind::Square => {
                            // "Base" of the chunk, to be multiplied by CONFIG.mountain_scale
                            // (multiplied value is from -0.35 *
                            // (CONFIG.mountain_scale * 1.05) to
                            // 0.35 * (CONFIG.mountain_scale * 0.95), but value here is from -0.3675
                            // to 0.3325).
                            Some(
                                (gen_ctx
                                    .alt_nz
                                    .get((wposf.div(10_000.0)).into_array())
                                    .clamp(-1.0, 1.0))
                                .sub(0.05)
                                .mul(0.35),
                            )
                        },
                        MapKind::Circle => {
                            let world_sizef = map_size_lg.chunks().map(|e| e as f64)
                                * TerrainChunkSize::RECT_SIZE.map(|e| e as f64);
                            Some(
                                (gen_ctx
                                    .alt_nz
                                    .get((wposf.div(5_000.0 * gen_opts.scale)).into_array())
                                    .clamp(-1.0, 1.0))
                                .add(
                                    0.2 - ((wposf / world_sizef) * 2.0 - 1.0)
                                        .magnitude_squared()
                                        .powf(0.75)
                                        .clamped(0.0, 1.0)
                                        .powf(1.0)
                                        * 0.6,
                                )
                                .mul(0.5),
                            )
                        },
                    }
                })
            },
            || {
                uniform_noise(map_size_lg, |_, wposf| {
                    // From 0 to 1.6, but the distribution before the max is from -1 and 1.6, so
                    // there is a 50% chance that hill will end up at 0.3 or
                    // lower, and probably a very high change it will be exactly
                    // 0.
                    let hill = (0.0f64
                        + gen_ctx
                            .hill_nz
                            .get(
                                (wposf
                                    .mul(32.0)
                                    .div(TerrainChunkSize::RECT_SIZE.map(|e| e as f64))
                                    .div(1_500.0))
                                .into_array(),
                            )
                            .clamp(-1.0, 1.0)
                            .mul(1.0)
                        + gen_ctx
                            .hill_nz
                            .get(
                                (wposf
                                    .mul(32.0)
                                    .div(TerrainChunkSize::RECT_SIZE.map(|e| e as f64))
                                    .div(400.0))
                                .into_array(),
                            )
                            .clamp(-1.0, 1.0)
                            .mul(0.3))
                    .add(0.3)
                    .max(0.0);

                    // chaos produces a value in [0.12, 1.32].  It is a meta-level factor intended
                    // to reflect how "chaotic" the region is--how much weird
                    // stuff is going on on this terrain.
                    Some(
                        ((gen_ctx
                            .chaos_nz
                            .get((wposf.div(3_000.0)).into_array())
                            .clamp(-1.0, 1.0))
                        .add(1.0)
                        .mul(0.5)
                        // [0, 1] * [0.4, 1] = [0, 1] (but probably towards the lower end)
                        .mul(
                            (gen_ctx
                                .chaos_nz
                                .get((wposf.div(6_000.0)).into_array())
                                .clamp(-1.0, 1.0))
                            .abs()
                                .clamp(0.4, 1.0),
                        )
                        // Chaos is always increased by a little when we're on a hill (but remember
                        // that hill is 0.3 or less about 50% of the time).
                        // [0, 1] + 0.2 * [0, 1.6] = [0, 1.32]
                        .add(0.2 * hill)
                        // We can't have *no* chaos!
                        .max(0.12)) as f32,
                    )
                })
            },
        );
        (alt_base, chaos)
    }

    fn build_alt_old_field(
        map_size_lg: MapSizeLg,
        gen_ctx: &GenCtx,
        alt_base: &InverseCdf<f64>,
        chaos: &InverseCdf,
    ) -> InverseCdf {
        // We ignore sea level because we actually want to be relative to sea level here
        // and want things in CONFIG.mountain_scale units, but otherwise this is
        // a correct altitude calculation.  Note that this is using the
        // "unadjusted" temperature.
        //
        // No NaNs in these uniform vectors, since the original noise value always
        // returns Some.
        let (alt_old, _) = uniform_noise(map_size_lg, |posi, wposf| {
            // This is the extension upwards from the base added to some extra noise from -1
            // to 1.
            //
            // The extra noise is multiplied by alt_main (the mountain part of the
            // extension) powered to 0.8 and clamped to [0.15, 1], to get a
            // value between [-1, 1] again.
            //
            // The sides then receive the sequence (y * 0.3 + 1.0) * 0.4, so we have
            // [-1*1*(1*0.3+1)*0.4, 1*(1*0.3+1)*0.4] = [-0.52, 0.52].
            //
            // Adding this to alt_main thus yields a value between -0.4 (if alt_main = 0 and
            // gen_ctx = -1, 0+-1*(0*.3+1)*0.4) and 1.52 (if alt_main = 1 and gen_ctx = 1).
            // Most of the points are above 0.
            //
            // Next, we add again by a sin of alt_main (between [-1, 1])^pow, getting
            // us (after adjusting for sign) another value between [-1, 1], and then this is
            // multiplied by 0.045 to get [-0.045, 0.045], which is added to [-0.4, 0.52] to
            // get [-0.445, 0.565].
            let alt_main = {
                // Extension upwards from the base.  A positive number from 0 to 1 curved to be
                // maximal at 0.  Also to be multiplied by CONFIG.mountain_scale.
                let alt_main = (gen_ctx
                    .alt_nz
                    .get((wposf.div(2_000.0)).into_array())
                    .clamp(-1.0, 1.0))
                .abs()
                .powf(1.35);

                fn spring(x: f64, pow: f64) -> f64 { x.abs().powf(pow) * x.signum() }

                0.0 + alt_main
                    + (gen_ctx
                        .small_nz
                        .get(
                            (wposf
                                .mul(32.0)
                                .div(TerrainChunkSize::RECT_SIZE.map(|e| e as f64))
                                .div(300.0))
                            .into_array(),
                        )
                        .clamp(-1.0, 1.0))
                    .mul(alt_main.powf(0.8).max(/* 0.25 */ 0.15))
                    .mul(0.3)
                    .add(1.0)
                    .mul(0.4)
                    + spring(alt_main.abs().sqrt().min(0.75).mul(60.0).sin(), 4.0).mul(0.045)
            };

            // Now we can compute the final altitude using chaos.
            // We multiply by chaos clamped to [0.1, 1.32] to get a value between [0.03,
            // 2.232] for alt_pre, then multiply by CONFIG.mountain_scale and
            // add to the base and sea level to get an adjusted value, then
            // multiply the whole thing by map_edge_factor (TODO: compute final
            // bounds).
            //
            // [-.3675, .3325] + [-0.445, 0.565] * [0.12, 1.32]^1.2
            // ~ [-.3675, .3325] + [-0.445, 0.565] * [0.07, 1.40]
            // = [-.3675, .3325] + ([-0.5785, 0.7345])
            // = [-0.946, 1.067]
            Some(
                ((alt_base[posi].1 + alt_main.mul((chaos[posi].1 as f64).powf(1.2)))
                    .mul(map_edge_factor(map_size_lg, posi) as f64)
                    .add(
                        (CONFIG.sea_level as f64)
                            .div(CONFIG.mountain_scale as f64)
                            .mul(map_edge_factor(map_size_lg, posi) as f64),
                    )
                    .sub((CONFIG.sea_level as f64).div(CONFIG.mountain_scale as f64)))
                    as f32,
            )
        });
        alt_old
    }

    fn build_ocean_and_uplift_fields(
        map_size_lg: MapSizeLg,
        pre_erosion_params: &PreErosionParams,
        alt_old: &InverseCdf,
    ) -> (BitBox, InverseCdf<f64>) {
        // Calculate oceans.
        let is_ocean = get_oceans(map_size_lg, |posi: usize| alt_old[posi].1);
        // NOTE: Uncomment if you want oceans to exclusively be on the border of the
        // map.
        /* let is_ocean = (0..map_size_lg.chunks())
        .into_par_iter()
        .map(|i| map_edge_factor(map_size_lg, i) == 0.0)
        .collect::<Vec<_>>(); */
        let is_ocean_fn = |posi: usize| is_ocean[posi];
        let old_height = |posi: usize| {
            alt_old[posi].1
                * CONFIG.mountain_scale
                * pre_erosion_params.height_scale(PreErosionModel::terrain_n(posi)) as f32
        };

        // NOTE: Needed if you wish to use the distance to the point defining the Worley
        // cell, not just the value within that cell.
        // let uplift_nz_dist = gen_ctx.uplift_nz.clone().enable_range(true);

        // Recalculate altitudes without oceans.
        // NaNs in these uniform vectors wherever is_ocean_fn returns true.
        let (alt_old_no_ocean, _) = uniform_noise(map_size_lg, |posi, _| {
            if is_ocean_fn(posi) {
                None
            } else {
                Some(old_height(posi))
            }
        });
        let (uplift_uniform, _) = uniform_noise(map_size_lg, |posi, _wposf| {
            if is_ocean_fn(posi) {
                None
            } else {
                let oheight = alt_old_no_ocean[posi].0 as f64 - 0.5;
                let height = (oheight + 0.5).powi(2);
                Some(height)
            }
        });

        (is_ocean, uplift_uniform)
    }

    fn init_gen_ctx(
        seed: u32,
        continent_scale: f64,
        rock_lacunarity: f64,
        uplift_scale: f64,
    ) -> (ChaChaRng, GenCtx) {
        let mut rng = ChaChaRng::from_seed(seed_expan::rng_state(seed));

        // NOTE: Changing order will significantly change WorldGen, so try not to!
        let gen_ctx = GenCtx {
            turb_x_nz: SuperSimplex::new(rng.random()),
            turb_y_nz: SuperSimplex::new(rng.random()),
            chaos_nz: RidgedMulti::new(rng.random()).set_octaves(7).set_frequency(
                RidgedMulti::<Perlin>::DEFAULT_FREQUENCY * (5_000.0 / continent_scale),
            ),
            hill_nz: SuperSimplex::new(rng.random()),
            alt_nz: util::HybridMulti::new(rng.random())
                .set_octaves(8)
                .set_frequency(10_000.0 / continent_scale)
                // persistence = lacunarity^(-(1.0 - fractal increment))
                .set_lacunarity(util::HybridMulti::<Perlin>::DEFAULT_LACUNARITY)
                .set_persistence(util::HybridMulti::<Perlin>::DEFAULT_LACUNARITY.powi(-1))
                .set_offset(0.0),
            temp_nz: Fbm::new(rng.random())
                .set_octaves(6)
                .set_persistence(0.5)
                .set_frequency(1.0 / (((1 << 6) * 64) as f64))
                .set_lacunarity(2.0),

            small_nz: BasicMulti::new(rng.random()).set_octaves(2),
            rock_nz: HybridMulti::new(rng.random()).set_persistence(0.3),
            tree_nz: BasicMulti::new(rng.random())
                .set_octaves(12)
                .set_persistence(0.75),
            _cave_0_nz: SuperSimplex::new(rng.random()),
            _cave_1_nz: SuperSimplex::new(rng.random()),

            structure_gen: StructureGen2d::new(rng.random(), 24, 10),
            _big_structure_gen: StructureGen2d::new(rng.random(), 768, 512),
            _region_gen: StructureGen2d::new(rng.random(), 400, 96),
            humid_nz: Billow::new(rng.random())
                .set_octaves(9)
                .set_persistence(0.4)
                .set_frequency(0.2),

            _fast_turb_x_nz: FastNoise::new(rng.random()),
            _fast_turb_y_nz: FastNoise::new(rng.random()),

            _town_gen: StructureGen2d::new(rng.random(), 2048, 1024),
            river_seed: RandomField::new(rng.random()),
            rock_strength_nz: Fbm::new(rng.random())
                .set_octaves(10)
                .set_lacunarity(rock_lacunarity)
                // persistence = lacunarity^(-(1.0 - fractal increment))
                // NOTE: In paper, fractal increment is roughly 0.25.
                .set_persistence(rock_lacunarity.powf(-0.75))
                .set_frequency(
                    1.0 * (5_000.0 / continent_scale)
                        / (2.0 * TerrainChunkSize::RECT_SIZE.x as f64 * 2.0.powi(10 - 1)),
                ),
            uplift_nz: util::Worley::new(rng.random())
                .set_frequency(1.0 / (TerrainChunkSize::RECT_SIZE.x as f64 * uplift_scale))
                .set_distance_function(distance_functions::euclidean),
        };

        (rng, gen_ctx)
    }

    #[allow(clippy::too_many_arguments)]
    fn materialize_heightfields(
        parsed_world_file: Option<ModernMap>,
        gen_opts: &GenOpts,
        world_file: FileOpts,
        fresh: bool,
        recipe_manifest: &RecipeManifestV1,
        model: &PreErosionModel<'_>,
        threadpool: &rayon::ThreadPool,
        report_erosion: &mut dyn FnMut(f64),
    ) -> (Box<[Alt]>, Box<[Alt]>) {
        let map_size_lg = model.map_size_lg;
        let (alt, basement) = if let Some(map) = parsed_world_file {
            (map.alt, map.basement)
        } else {
            Self::generate_heightfields_from_model(model, threadpool, report_erosion)
        };

        let (alt, basement) = Self::persist_and_normalize_heightfields(
            map_size_lg,
            gen_opts,
            world_file,
            fresh,
            recipe_manifest,
            alt,
            basement,
        );

        Self::apply_post_load_erosion_if_needed(alt, basement, model, threadpool, report_erosion)
    }

    fn generate_heightfields_from_model(
        model: &PreErosionModel<'_>,
        threadpool: &rayon::ThreadPool,
        report_erosion: &mut dyn FnMut(f64),
    ) -> (Box<[Alt]>, Box<[Alt]>) {
        let (alt, basement) = Self::run_primary_erosion_cycle(model, threadpool, report_erosion);

        // Quick "small scale" erosion cycle in order to lower extreme angles.
        Self::run_followup_erosion_cycle(
            alt,
            basement,
            model.pre_erosion_params.n_small_steps,
            model,
            threadpool,
            report_erosion,
        )
    }

    fn run_primary_erosion_cycle(
        model: &PreErosionModel<'_>,
        threadpool: &rayon::ThreadPool,
        report_erosion: &mut dyn FnMut(f64),
    ) -> (Box<[Alt]>, Box<[Alt]>) {
        do_erosion(
            model.map_size_lg,
            model.pre_erosion_params.max_erosion_per_delta_t as f32,
            model.pre_erosion_params.n_steps,
            &model.gen_ctx.river_seed,
            // varying conditions
            &model.gen_ctx.rock_strength_nz,
            // initial conditions
            &|posi| model.alt(posi),
            &|posi| model.alt(posi),
            &|posi| model.is_ocean(posi),
            // empirical constants
            &|posi| model.uplift(posi),
            &|posi| PreErosionModel::terrain_n(posi),
            &|posi| model.theta(posi),
            &|posi| model.kf(posi),
            &|posi| model.kd(posi),
            &|posi| model.g(posi),
            &|posi| model.epsilon_0(posi),
            &|posi| model.alpha(posi),
            // scaling factors
            &|n| model.pre_erosion_params.height_scale(n),
            model.pre_erosion_params.k_d_scale(1.0),
            &|q| model.pre_erosion_params.k_da_scale(q),
            threadpool,
            report_erosion,
        )
    }

    fn run_followup_erosion_cycle(
        alt: Box<[Alt]>,
        basement: Box<[Alt]>,
        n_steps: usize,
        model: &PreErosionModel<'_>,
        threadpool: &rayon::ThreadPool,
        report_erosion: &mut dyn FnMut(f64),
    ) -> (Box<[Alt]>, Box<[Alt]>) {
        do_erosion(
            model.map_size_lg,
            1.0f32,
            n_steps,
            &model.gen_ctx.river_seed,
            &model.gen_ctx.rock_strength_nz,
            |posi| alt[posi] as f32,
            |posi| basement[posi] as f32,
            |posi| model.is_ocean(posi),
            |posi| model.uplift(posi) * (1.0 / model.pre_erosion_params.max_erosion_per_delta_t),
            |posi| PreErosionModel::terrain_n(posi),
            |posi| model.theta(posi),
            |posi| model.kf(posi),
            |posi| model.kd(posi),
            |posi| model.g(posi),
            |posi| model.epsilon_0(posi),
            |posi| model.alpha(posi),
            |n| model.pre_erosion_params.height_scale(n),
            model.pre_erosion_params.k_d_scale(1.0),
            |q| model.pre_erosion_params.k_da_scale(q),
            threadpool,
            report_erosion,
        )
    }

    fn persist_and_normalize_heightfields(
        map_size_lg: MapSizeLg,
        gen_opts: &GenOpts,
        world_file: FileOpts,
        fresh: bool,
        recipe_manifest: &RecipeManifestV1,
        alt: Box<[Alt]>,
        basement: Box<[Alt]>,
    ) -> (Box<[Alt]>, Box<[Alt]>) {
        // Save map, if necessary.
        // NOTE: We wll always save a map with latest version.
        let map = WorldFile::new(ModernMap {
            continent_scale_hack: gen_opts.scale,
            map_size_lg: map_size_lg.vec(),
            alt,
            basement,
        });
        if fresh {
            world_file.save(&map, recipe_manifest);
        }

        // Skip validation--we just performed a no-op conversion for this map, so it had
        // better be valid!
        let ModernMap {
            continent_scale_hack: _,
            map_size_lg: _,
            alt,
            basement,
        } = map.into_modern().unwrap();

        (alt, basement)
    }

    fn apply_post_load_erosion_if_needed(
        alt: Box<[Alt]>,
        basement: Box<[Alt]>,
        model: &PreErosionModel<'_>,
        threadpool: &rayon::ThreadPool,
        report_erosion: &mut dyn FnMut(f64),
    ) -> (Box<[Alt]>, Box<[Alt]>) {
        if model.pre_erosion_params.n_post_load_steps == 0 {
            (alt, basement)
        } else {
            Self::run_followup_erosion_cycle(
                alt,
                basement,
                model.pre_erosion_params.n_post_load_steps,
                model,
                threadpool,
                report_erosion,
            )
        }
    }

    fn build_post_erosion_hydrology_core(
        map_size_lg: MapSizeLg,
        continent_scale_hack: f64,
        alt: &[Alt],
        threadpool: &rayon::ThreadPool,
    ) -> PostErosionHydrologyCore {
        let drainage_state = Self::build_post_erosion_drainage_state(map_size_lg, alt, threadpool);
        Self::materialize_surface_water_and_rivers(
            map_size_lg,
            continent_scale_hack,
            alt,
            drainage_state,
        )
    }

    fn build_post_erosion_drainage_state(
        map_size_lg: MapSizeLg,
        alt: &[Alt],
        threadpool: &rayon::ThreadPool,
    ) -> PostErosionDrainageState {
        let is_ocean = get_oceans(map_size_lg, |posi| alt[posi]);
        let is_ocean_fn = |posi: usize| is_ocean[posi];
        let mut dh = downhill(map_size_lg, |posi| alt[posi], is_ocean_fn);
        let (boundary_len, indirection, water_alt_pos, maxh) =
            get_lakes(map_size_lg, |posi| alt[posi], &mut dh);
        debug!(?maxh, "Max height");
        let (mrec, mstack, mwrec) = {
            let mut wh = vec![0.0; map_size_lg.chunks_len()];
            get_multi_rec(
                map_size_lg,
                |posi| alt[posi],
                &dh,
                &water_alt_pos,
                &mut wh,
                usize::from(map_size_lg.chunks().x),
                usize::from(map_size_lg.chunks().y),
                TerrainChunkSize::RECT_SIZE.x as Compute,
                TerrainChunkSize::RECT_SIZE.y as Compute,
                maxh,
                threadpool,
            )
        };
        let flux_old = get_multi_drainage(map_size_lg, &mstack, &mrec, &mwrec, boundary_len);

        PostErosionDrainageState {
            is_ocean,
            dh,
            indirection,
            water_alt_pos,
            flux: flux_old,
            max_height: maxh as f32,
        }
    }

    fn materialize_surface_water_and_rivers(
        map_size_lg: MapSizeLg,
        continent_scale_hack: f64,
        alt: &[Alt],
        drainage_state: PostErosionDrainageState,
    ) -> PostErosionHydrologyCore {
        let PostErosionDrainageState {
            is_ocean,
            dh,
            indirection,
            water_alt_pos,
            flux,
            max_height,
        } = drainage_state;
        let is_ocean_fn = |posi: usize| is_ocean[posi];
        // let flux_rivers = get_drainage(map_size_lg, &water_alt_pos, &dh,
        // boundary_len); TODO: Make rivers work with multi-direction flux as
        // well.
        let flux_rivers = flux.clone();

        let water_height_initial = |chunk_idx| {
            let indirection_idx = indirection[chunk_idx];
            // Find the lake this point is flowing into.
            let lake_idx = if indirection_idx < 0 {
                chunk_idx
            } else {
                indirection_idx as usize
            };
            let chunk_water_alt = if dh[lake_idx] < 0 {
                // This is either a boundary node (dh[chunk_idx] == -2, i.e. water is at sea
                // level) or part of a lake that flows directly into the ocean.
                // In the former case, water is at sea level so we just return
                // 0.0.  In the latter case, the lake bottom must have been a
                // boundary node in the first place--meaning this node flows directly
                // into the ocean.  In that case, its lake bottom is ocean, meaning its water is
                // also at sea level.  Thus, we return 0.0 in both cases.
                0.0
            } else {
                // This chunk is draining into a body of water that isn't the ocean (i.e., a
                // lake). Then we just need to find the pass height of the
                // surrounding lake in order to figure out the initial water
                // height (which fill_sinks will then extend to make
                // sure it fills the entire basin).

                // Find the height of "our" side of the pass (the part of it that drains into
                // this chunk's lake).
                let pass_idx = -indirection[lake_idx] as usize;
                let pass_height_i = alt[pass_idx];
                // Find the pass this lake is flowing into (i.e. water at the lake bottom gets
                // pushed towards the point identified by pass_idx).
                let neighbor_pass_idx = dh[pass_idx/*lake_idx*/];
                // Find the height of the pass into which our lake is flowing.
                let pass_height_j = alt[neighbor_pass_idx as usize];
                // Find the maximum of these two heights.
                // Use the pass height as the initial water altitude.
                pass_height_i.max(pass_height_j) /*pass_height*/
            };
            // Use the maximum of the pass height and chunk height as the parameter to
            // fill_sinks.
            let chunk_alt = alt[chunk_idx];
            chunk_alt.max(chunk_water_alt)
        };

        // NOTE: If for for some reason you need to avoid the expensive `fill_sinks`
        // step here, and we haven't yet replaced it with a faster version, you
        // may comment out this line and replace it with the commented-out code
        // below; however, there are no guarantees that this
        // will work correctly.
        let water_alt = fill_sinks(map_size_lg, water_height_initial, is_ocean_fn);
        /* let water_alt = (0..map_size_lg.chunks_len())
        .into_par_iter()
        .map(|posi| water_height_initial(posi))
        .collect::<Vec<_>>(); */

        let rivers = get_rivers(
            map_size_lg,
            continent_scale_hack,
            &water_alt_pos,
            &water_alt,
            &dh,
            &indirection,
            &flux_rivers,
        );

        let water_alt = indirection
            .par_iter()
            .enumerate()
            .map(|(chunk_idx, &indirection_idx)| {
                // Find the lake this point is flowing into.
                let lake_idx = if indirection_idx < 0 {
                    chunk_idx
                } else {
                    indirection_idx as usize
                };
                if dh[lake_idx] < 0 {
                    // This is either a boundary node (dh[chunk_idx] == -2, i.e. water is at sea
                    // level) or part of a lake that flows directly into the
                    // ocean.  In the former case, water is at sea level so we
                    // just return 0.0.  In the latter case, the lake bottom must
                    // have been a boundary node in the first place--meaning this node flows
                    // directly into the ocean.  In that case, its lake bottom
                    // is ocean, meaning its water is also at sea level.  Thus,
                    // we return 0.0 in both cases.
                    0.0
                } else {
                    // This is not flowing into the ocean, so we can use the existing water_alt.
                    water_alt[chunk_idx] as f32
                }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();

        PostErosionHydrologyCore {
            water_alt,
            dh,
            flux,
            rivers,
            max_height,
        }
    }

    fn build_post_water_cdf_fields(
        map_size_lg: MapSizeLg,
        gen_ctx: &GenCtx,
        alt: &[Alt],
        flux: &[Compute],
        rivers: &[RiverData],
        threadpool: &rayon::ThreadPool,
    ) -> PostWaterCdfFields {
        let pure_water = Self::build_pure_water_mask(map_size_lg, rivers);
        Self::build_masked_post_water_cdf_fields(
            map_size_lg,
            gen_ctx,
            alt,
            flux,
            &pure_water,
            threadpool,
        )
    }

    fn build_pure_water_mask(map_size_lg: MapSizeLg, rivers: &[RiverData]) -> Box<[bool]> {
        let is_underwater = |chunk_idx: usize| match rivers[chunk_idx].river_kind {
            Some(RiverKind::Ocean) | Some(RiverKind::Lake { .. }) => true,
            Some(RiverKind::River { .. }) => false, // TODO: inspect width
            None => false,
        };

        // Check whether any tiles around this tile are not water (since Lerp will
        // ensure that they are included).
        let pure_water = |posi: usize| {
            let pos = uniform_idx_as_vec2(map_size_lg, posi);
            for x in pos.x - 1..(pos.x + 1) + 1 {
                for y in pos.y - 1..(pos.y + 1) + 1 {
                    if x >= 0
                        && y >= 0
                        && x < map_size_lg.chunks().x as i32
                        && y < map_size_lg.chunks().y as i32
                    {
                        let posi = vec2_as_uniform_idx(map_size_lg, Vec2::new(x, y));
                        if !is_underwater(posi) {
                            return false;
                        }
                    }
                }
            }
            true
        };
        (0..map_size_lg.chunks_len())
            .into_par_iter()
            .map(pure_water)
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }

    fn build_masked_post_water_cdf_fields(
        map_size_lg: MapSizeLg,
        gen_ctx: &GenCtx,
        alt: &[Alt],
        flux: &[Compute],
        pure_water: &[bool],
        threadpool: &rayon::ThreadPool,
    ) -> PostWaterCdfFields {
        // NaNs in these uniform vectors wherever pure_water() returns true.
        let (((alt_no_water, _), (pure_flux, _)), ((temp_base, _), (humid_base, _))) = threadpool
            .join(
                || {
                    threadpool.join(
                        || {
                            uniform_noise(map_size_lg, |posi, _| {
                                if pure_water[posi] {
                                    None
                                } else {
                                    // A version of alt that is uniform over *non-water* (or
                                    // land-adjacent water) chunks.
                                    Some(alt[posi] as f32)
                                }
                            })
                        },
                        || {
                            uniform_noise(map_size_lg, |posi, _| {
                                if pure_water[posi] {
                                    None
                                } else {
                                    Some(flux[posi])
                                }
                            })
                        },
                    )
                },
                || {
                    threadpool.join(
                        || {
                            uniform_noise(map_size_lg, |posi, wposf| {
                                if pure_water[posi] {
                                    None
                                } else {
                                    // -1 to 1.
                                    Some(gen_ctx.temp_nz.get((wposf).into_array()) as f32)
                                }
                            })
                        },
                        || {
                            uniform_noise(map_size_lg, |posi, wposf| {
                                // Check whether any tiles around this tile are water.
                                if pure_water[posi] {
                                    None
                                } else {
                                    // 0 to 1, hopefully.
                                    Some(
                                        (gen_ctx.humid_nz.get(wposf.div(1024.0).into_array())
                                            as f32)
                                            .add(1.0)
                                            .mul(0.5),
                                    )
                                }
                            })
                        },
                    )
                },
            );

        PostWaterCdfFields {
            pure_flux,
            alt_no_water,
            temp_base,
            humid_base,
        }
    }

    fn build_sim_chunks(
        map_size_lg: MapSizeLg,
        gen_ctx: &GenCtx,
        gen_cdf: &GenCdf,
    ) -> Vec<SimChunk> {
        (0..map_size_lg.chunks_len())
            .into_par_iter()
            .map(|i| SimChunk::generate(map_size_lg, i, gen_ctx, gen_cdf))
            .collect::<Vec<_>>()
    }

    fn finalize_world_from_chunk_inputs(inputs: GeneratedWorldFinalizeInputs) -> Self {
        let seed_elements = inputs.seed_elements;
        let parts = inputs.into_parts();
        Self::finalize_world_from_parts_with_postprocess(parts, seed_elements)
    }

    fn finalize_world_from_parts(parts: GeneratedWorldParts) -> Self {
        let GeneratedWorldParts {
            seed,
            map_size_lg,
            max_height,
            chunks,
            gen_ctx,
            rng,
            calendar,
            compat_mode,
            load_legacy_mode,
            load_or_generate_sidecarless_mode,
            compat_audit,
            managed_recipe_sidecar_missing,
            recipe_manifest,
        } = parts;

        Self {
            seed,
            map_size_lg,
            max_height,
            chunks,
            _locations: Vec::new(),
            gen_ctx,
            rng,
            calendar,
            compat_mode,
            load_legacy_mode,
            load_or_generate_sidecarless_mode,
            compat_audit,
            managed_recipe_sidecar_missing,
            topology: WorldTopology::new(recipe_manifest.world_recipe.topology_id, map_size_lg),
            recipe_manifest,
        }
    }

    fn finalize_world_from_parts_with_postprocess(
        parts: GeneratedWorldParts,
        seed_elements: bool,
    ) -> Self {
        let mut this = Self::finalize_world_from_parts(parts);
        this.run_generation_postprocesses(seed_elements);

        this
    }

    fn run_generation_postprocesses(&mut self, seed_elements: bool) {
        self.generate_cliffs();

        if seed_elements {
            self.seed_elements();
        }
    }

    #[inline(always)]
    pub const fn map_size_lg(&self) -> MapSizeLg { self.map_size_lg }

    pub const fn compat_mode(&self) -> CompatMode { self.compat_mode }

    pub const fn load_legacy_mode(&self) -> LoadLegacyMode { self.load_legacy_mode }

    pub const fn load_or_generate_sidecarless_mode(&self) -> LoadOrGenerateSidecarlessMode {
        self.load_or_generate_sidecarless_mode
    }

    pub const fn compat_audit(&self) -> CompatAuditV1 { self.compat_audit }

    pub const fn managed_recipe_sidecar_missing(&self) -> bool {
        self.managed_recipe_sidecar_missing
    }

    pub fn recipe_manifest(&self) -> &RecipeManifestV1 { &self.recipe_manifest }

    pub(crate) const fn topology(&self) -> WorldTopology { self.topology }

    pub fn get_size(&self) -> Vec2<u32> { self.map_size_lg().chunks().map(u32::from) }

    pub fn get_aabr(&self) -> Aabr<i32> { self.topology.chunk_aabr() }

    pub fn query_chunk_key_aabr(&self) -> Aabr<i32> { self.topology.chunk_key_aabr() }

    pub fn runtime_chunk_product_key_aabr(&self) -> Aabr<i32> {
        self.topology.runtime_chunk_product_key_aabr()
    }

    pub(crate) fn runtime_topology_descriptor(&self) -> world_msg::RuntimeTopologyDescriptor {
        world_msg::RuntimeTopologyDescriptor {
            topology_id: self
                .recipe_manifest
                .world_recipe
                .topology_id
                .as_str()
                .to_owned(),
            query_chunk_key_aabr: self.query_chunk_key_aabr(),
            runtime_chunk_product_key_aabr: self.runtime_chunk_product_key_aabr(),
            missing_world_bounds_policy:
                world_msg::MissingWorldBoundsPolicy::BoundedOceanDefaultChunk,
        }
    }

    pub(crate) fn default_chunk_for_missing_world_bounds(&self) -> TerrainChunk {
        self.topology.default_chunk_kind().build()
    }

    pub fn generate_oob_chunk(&self) -> TerrainChunk {
        self.default_chunk_for_missing_world_bounds()
    }

    pub fn approx_chunk_terrain_normal(&self, chunk_pos: Vec2<i32>) -> Option<Vec3<f32>> {
        let curr_chunk = self.get(chunk_pos)?;
        let downhill_chunk_pos = curr_chunk.downhill?.wpos_to_cpos();
        let downhill_chunk = self.get(downhill_chunk_pos)?;
        // special case if chunks are flat
        if (curr_chunk.alt - downhill_chunk.alt) == 0. {
            return Some(Vec3::unit_z());
        }
        let curr = chunk_pos.cpos_to_wpos_center().as_().with_z(curr_chunk.alt);
        let down = downhill_chunk_pos
            .cpos_to_wpos_center()
            .as_()
            .with_z(downhill_chunk.alt);
        let downwards = curr - down;
        let flat = downwards.with_z(down.z);
        let mut res = downwards.cross(flat).cross(downwards);
        res.normalize();
        Some(res)
    }

    /// Draw a map of the world based on chunk information.  Returns a buffer of
    /// u32s.
    pub fn get_map(&self, index: IndexRef, calendar: Option<&Calendar>) -> WorldMapMsg {
        prof_span!("WorldSim::get_map");
        let mut map_config = MapConfig::orthographic(
            self.map_size_lg(),
            core::ops::RangeInclusive::new(CONFIG.sea_level, CONFIG.sea_level + self.max_height),
        );
        // Build a horizon map.
        let scale_angle = |angle: Alt| {
            (/* 0.0.max( */angle /* ) */
                .atan()
                * <Alt as FloatConst>::FRAC_2_PI()
                * 255.0)
                .floor() as u8
        };
        let scale_height = |height: Alt| {
            (/* 0.0.max( */height/*)*/ as Alt * 255.0 / self.max_height as Alt).floor() as u8
        };

        let samples_data = {
            prof_span!("samples data");
            let column_sample = ColumnGen::new(self);
            (0..self.map_size_lg().chunks_len())
                .into_par_iter()
                .map_init(
                    || Box::new(BlockGen::new(ColumnGen::new(self))),
                    |_block_gen, posi| {
                        let sample = column_sample.get(
                            (
                                uniform_idx_as_vec2(self.map_size_lg(), posi) * TerrainChunkSize::RECT_SIZE.map(|e| e as i32),
                                index,
                                calendar,
                            )
                        )?;
                        // sample.water_level = CONFIG.sea_level.max(sample.water_level);

                        Some(sample)
                    },
                )
                /* .map(|posi| {
                    let mut sample = column_sample.get(
                        uniform_idx_as_vec2(self.map_size_lg(), posi) * TerrainChunkSize::RECT_SIZE.map(|e| e as i32),
                    );
                }) */
                .collect::<Vec<_>>()
                .into_boxed_slice()
        };

        let horizons = get_horizon_map(
            self.map_size_lg(),
            self.topology.chunk_aabr(),
            CONFIG.sea_level,
            CONFIG.sea_level + self.max_height,
            |posi| {
                /* let chunk = &self.chunks[posi];
                chunk.alt.max(chunk.water_alt) as Alt */
                let sample = samples_data[posi].as_ref();
                sample
                    .map(|s| s.alt.max(s.water_level))
                    .unwrap_or(CONFIG.sea_level)
            },
            |a| scale_angle(a.into()),
            |h| scale_height(h.into()),
        )
        .unwrap();

        let mut v = vec![0u32; self.map_size_lg().chunks_len()];
        let mut alts = vec![0u32; self.map_size_lg().chunks_len()];
        // TODO: Parallelize again.
        map_config.is_shaded = false;

        map_config.generate(
            |pos| sample_pos(&map_config, self, index, Some(&samples_data), pos),
            |pos| sample_wpos(&map_config, self, pos),
            |pos, (r, g, b, _a)| {
                // We currently ignore alpha and replace it with the height at pos, scaled to
                // u8.
                let alt = sample_wpos(
                    &map_config,
                    self,
                    pos.map(|e| e as i32) * TerrainChunkSize::RECT_SIZE.map(|e| e as i32),
                );
                let a = 0; //(alt.min(1.0).max(0.0) * 255.0) as u8;

                // NOTE: Safe by invariants on map_size_lg.
                let posi = (pos.y << self.map_size_lg().vec().x) | pos.x;
                v[posi] = u32::from_le_bytes([r, g, b, a]);
                alts[posi] = (((alt.clamp(0.0, 1.0) * 8191.0) as u32) & 0x1FFF) << 3;
            },
        );
        WorldMapMsg {
            dimensions_lg: self.map_size_lg().vec(),
            max_height: self.max_height,
            rgba: Grid::from_raw(self.get_size().map(|e| e as i32), v),
            alt: Grid::from_raw(self.get_size().map(|e| e as i32), alts),
            horizons,
            sites: Vec::new(),                   // Will be substituted later
            pois: Vec::new(),                    // Will be substituted later
            possible_starting_sites: Vec::new(), // Will be substituted later
            runtime_topology: self.runtime_topology_descriptor(),
            default_chunk: Arc::new(self.default_chunk_for_missing_world_bounds()),
        }
    }

    pub fn generate_cliffs(&mut self) {
        let mut rng = self.rng.clone();

        for _ in 0..self.get_size().product() / 10 {
            let mut pos = self.get_size().map(|e| rng.random_range(0..e) as i32);

            let mut cliffs = DHashSet::default();
            let mut cliff_path = Vec::new();

            for _ in 0..64 {
                if self.get_gradient_approx(pos).is_some_and(|g| g > 1.5) {
                    if !cliffs.insert(pos) {
                        break;
                    }
                    cliff_path.push((pos, 0.0));

                    pos += CARDINALS
                        .iter()
                        .copied()
                        .max_by_key(|rpos| {
                            self.get_gradient_approx(pos + rpos)
                                .map_or(0, |g| (g * 1000.0) as i32)
                        })
                        .unwrap(); // Can't fail
                } else {
                    break;
                }
            }

            for cliff in cliffs {
                Spiral2d::new()
                    .take((4usize * 2 + 1).pow(2))
                    .for_each(|rpos| {
                        let dist = rpos.map(|e| e as f32).magnitude();
                        if let Some(c) = self.get_mut(cliff + rpos) {
                            let warp = 1.0 / (1.0 + dist);
                            if !c.river.near_water() {
                                c.tree_density *= 1.0 - warp;
                                c.cliff_height = Lerp::lerp(44.0, 0.0, -1.0 + dist / 3.5);
                            }
                        }
                    });
            }
        }
    }

    /// Prepare the world for simulation
    pub fn seed_elements(&mut self) {
        let mut rng = self.rng.clone();

        let cell_size = 16;
        let grid_size = self.map_size_lg().chunks().map(usize::from) / cell_size;
        let loc_count = 100;

        let mut loc_grid = vec![None; grid_size.product()];
        let mut locations = Vec::new();

        // Seed the world with some locations
        (0..loc_count).for_each(|_| {
            let cell_pos = Vec2::new(
                (self.rng.random::<u64>() as usize) % grid_size.x,
                (self.rng.random::<u64>() as usize) % grid_size.y,
            );
            let wpos = (cell_pos * cell_size + cell_size / 2)
                .map2(TerrainChunkSize::RECT_SIZE, |e, sz: u32| {
                    e as i32 * sz as i32 + sz as i32 / 2
                });

            locations.push(Location::generate(wpos, &mut rng));

            loc_grid[cell_pos.y * grid_size.x + cell_pos.x] = Some(locations.len() - 1);
        });

        // Find neighbours
        let mut loc_clone = locations
            .iter()
            .map(|l| l.center)
            .enumerate()
            .collect::<Vec<_>>();
        // NOTE: We assume that usize is 8 or fewer bytes.
        (0..locations.len()).for_each(|i| {
            let pos = locations[i].center.map(|e| e as i64);

            loc_clone.sort_by_key(|(_, l)| l.map(|e| e as i64).distance_squared(pos));

            loc_clone.iter().skip(1).take(2).for_each(|(j, _)| {
                locations[i].neighbours.insert(*j as u64);
                locations[*j].neighbours.insert(i as u64);
            });
        });

        // Simulate invasion!
        let invasion_cycles = 25;
        (0..invasion_cycles).for_each(|_| {
            (0..grid_size.y).for_each(|j| {
                (0..grid_size.x).for_each(|i| {
                    if loc_grid[j * grid_size.x + i].is_none() {
                        const R_COORDS: [i32; 5] = [-1, 0, 1, 0, -1];
                        let idx = (self.rng.random::<u64>() % 4) as usize;
                        let new_i = i as i32 + R_COORDS[idx];
                        let new_j = j as i32 + R_COORDS[idx + 1];
                        if new_i >= 0 && new_j >= 0 {
                            let loc = Vec2::new(new_i as usize, new_j as usize);
                            loc_grid[j * grid_size.x + i] =
                                loc_grid.get(loc.y * grid_size.x + loc.x).cloned().flatten();
                        }
                    }
                });
            });
        });

        // Place the locations onto the world
        /*
        let gen = StructureGen2d::new(self.seed, cell_size as u32, cell_size as u32 / 2);

        self.chunks
            .par_iter_mut()
            .enumerate()
            .for_each(|(ij, chunk)| {
                let chunk_pos = uniform_idx_as_vec2(self.map_size_lg(), ij);
                let i = chunk_pos.x as usize;
                let j = chunk_pos.y as usize;
                let block_pos = Vec2::new(
                    chunk_pos.x * TerrainChunkSize::RECT_SIZE.x as i32,
                    chunk_pos.y * TerrainChunkSize::RECT_SIZE.y as i32,
                );
                let _cell_pos = Vec2::new(i / cell_size, j / cell_size);

                // Find the distance to each region
                let near = gen.get(chunk_pos);
                let mut near = near
                    .iter()
                    .map(|(pos, seed)| RegionInfo {
                        chunk_pos: *pos,
                        block_pos: pos
                            .map2(TerrainChunkSize::RECT_SIZE, |e, sz: u32| e * sz as i32),
                        dist: (pos - chunk_pos).map(|e| e as f32).magnitude(),
                        seed: *seed,
                    })
                    .collect::<Vec<_>>();

                // Sort regions based on distance
                near.sort_by(|a, b| a.dist.partial_cmp(&b.dist).unwrap());

                let nearest_cell_pos = near[0].chunk_pos;
                if nearest_cell_pos.x >= 0 && nearest_cell_pos.y >= 0 {
                    let nearest_cell_pos = nearest_cell_pos.map(|e| e as usize) / cell_size;
                    chunk.location = loc_grid
                        .get(nearest_cell_pos.y * grid_size.x + nearest_cell_pos.x)
                        .cloned()
                        .unwrap_or(None)
                        .map(|loc_idx| LocationInfo { loc_idx, near });
                }
            });
        */

        // Create waypoints
        const WAYPOINT_EVERY: usize = 16;
        let this = &self;
        let waypoints = (0..this.map_size_lg().chunks().x)
            .step_by(WAYPOINT_EVERY)
            .flat_map(|i| {
                (0..this.map_size_lg().chunks().y)
                    .step_by(WAYPOINT_EVERY)
                    .map(move |j| (i, j))
            })
            .collect::<Vec<_>>()
            .into_par_iter()
            .filter_map(|(i, j)| {
                let mut pos = Vec2::new(i as i32, j as i32);
                let mut chunk = this.get(pos)?;

                if chunk.is_underwater() {
                    return None;
                }
                // Slide the waypoints down hills
                const MAX_ITERS: usize = 64;
                for _ in 0..MAX_ITERS {
                    let downhill_pos = match chunk.downhill {
                        Some(downhill) => {
                            downhill.map2(TerrainChunkSize::RECT_SIZE, |e, sz: u32| e / (sz as i32))
                        },
                        None => return Some(pos),
                    };

                    let new_chunk = this.get(downhill_pos)?;
                    const SLIDE_THRESHOLD: f32 = 5.0;
                    if new_chunk.river.near_water() || new_chunk.alt + SLIDE_THRESHOLD < chunk.alt {
                        break;
                    } else {
                        chunk = new_chunk;
                        pos = downhill_pos;
                    }
                }
                Some(pos)
            })
            .collect::<Vec<_>>();

        for waypoint in waypoints {
            self.get_mut(waypoint).map(|sc| sc.contains_waypoint = true);
        }

        self.rng = rng;
        self._locations = locations;
    }

    pub fn get(&self, chunk_pos: Vec2<i32>) -> Option<&SimChunk> {
        self.topology
            .chunk_index(chunk_pos)
            .map(|index| &self.chunks[index])
    }

    fn chunk_aquatic_semantics(&self, chunk_pos: Vec2<i32>) -> Option<ChunkAquaticSemantics> {
        let chunk = self.get(chunk_pos)?;
        let water_body_kind = WaterBodyKind::from_chunk(chunk);
        let is_submerged = chunk.water_alt > chunk.alt;
        let marine_adjacent =
            marine_semantics::marine_adjacency_at_site(self, chunk_pos, chunk.alt);
        let water_access_class = WaterAccessClass::from_semantic_facts(
            water_body_kind,
            is_submerged,
            chunk.river.near_water(),
            chunk.water_alt,
            marine_adjacent,
        );

        Some(ChunkAquaticSemantics {
            aquatic_spawn_potential: AquaticSpawnPotential::from_semantic_facts(
                water_body_kind,
                water_access_class,
            ),
            marine_ecology_profile: MarineEcologyProfile::from_world_facts(
                chunk.water_alt,
                marine_adjacent,
                chunk.get_biome(),
                chunk.alt,
            ),
        })
    }

    pub fn aquatic_spawn_potential(&self, chunk_pos: Vec2<i32>) -> Option<AquaticSpawnPotential> {
        Some(
            self.chunk_aquatic_semantics(chunk_pos)?
                .aquatic_spawn_potential,
        )
    }

    pub fn aquatic_fauna_summary(&self, chunk_pos: Vec2<i32>) -> Option<AquaticFaunaSummary> {
        let semantics = self.chunk_aquatic_semantics(chunk_pos)?;
        Some(AquaticFaunaSummary::from_profile(
            AquaticFaunaProfile::from_profiles(
                semantics.aquatic_spawn_potential,
                semantics.marine_ecology_profile,
            ),
        ))
    }

    pub fn get_gradient_approx(&self, chunk_pos: Vec2<i32>) -> Option<f32> {
        let a = self.get(chunk_pos)?;
        if let Some(downhill) = a.downhill {
            let b = self.get(downhill.wpos_to_cpos())?;
            Some((a.alt - b.alt).abs() / TerrainChunkSize::RECT_SIZE.x as f32)
        } else {
            Some(0.0)
        }
    }

    pub(crate) fn gradient_approx_or(&self, chunk_pos: Vec2<i32>, fallback: ApproxFallback) -> f32 {
        self.get_gradient_approx(chunk_pos)
            .unwrap_or(fallback.value())
    }

    /// Get the altitude of the surface, could be water or ground.
    pub fn get_surface_alt_approx(&self, wpos: Vec2<i32>) -> f32 {
        self.surface_alt_approx_or(wpos, ApproxFallback::SeaLevel)
    }

    pub(crate) fn surface_alt_approx_or(&self, wpos: Vec2<i32>, fallback: ApproxFallback) -> f32 {
        self.get_interpolated(wpos, |chunk| chunk.alt)
            .zip(self.get_interpolated(wpos, |chunk| chunk.water_alt))
            .map(|(alt, water_alt)| alt.max(water_alt))
            .unwrap_or(fallback.value())
    }

    pub fn get_alt_approx(&self, wpos: Vec2<i32>) -> Option<f32> {
        self.get_interpolated(wpos, |chunk| chunk.alt)
    }

    pub(crate) fn alt_approx_or(&self, wpos: Vec2<i32>, fallback: ApproxFallback) -> f32 {
        self.get_alt_approx(wpos).unwrap_or(fallback.value())
    }

    pub(crate) fn chunk_pos_at_wpos(&self, wpos: Vec2<i32>) -> Vec2<i32> {
        wpos.map2(TerrainChunkSize::RECT_SIZE, |e, sz: u32| {
            e.div_euclid(sz as i32)
        })
    }

    pub(crate) fn map_sample_alt_or(
        &self,
        wpos: Vec2<i32>,
        is_basement: bool,
        is_water: bool,
        fallback: ApproxFallback,
    ) -> f32 {
        self.get_wpos(wpos)
            .map(|chunk| {
                if is_basement {
                    chunk.basement
                } else {
                    chunk.alt
                }
                .max(if is_water {
                    chunk.water_alt
                } else {
                    -f32::INFINITY
                })
            })
            .unwrap_or(fallback.value())
    }

    pub fn get_wpos(&self, wpos: Vec2<i32>) -> Option<&SimChunk> {
        self.get(self.chunk_pos_at_wpos(wpos))
    }

    pub fn get_mut(&mut self, chunk_pos: Vec2<i32>) -> Option<&mut SimChunk> {
        self.topology
            .chunk_index(chunk_pos)
            .map(|index| &mut self.chunks[index])
    }

    pub fn get_base_z(&self, chunk_pos: Vec2<i32>) -> Option<f32> {
        const LOCAL_GRID_RADIUS: i32 = 3;
        if !self.topology.contains_runtime_chunk_product_key(chunk_pos) {
            return None;
        }

        self.topology
            .local_chunks(chunk_pos, LOCAL_GRID_RADIUS)
            .flat_map(|neighbor_pos| {
                let neighbor_chunk = self.get(neighbor_pos);
                let river_kind = neighbor_chunk.and_then(|c| c.river.river_kind);
                let has_water = river_kind.is_some() && river_kind != Some(RiverKind::Ocean);
                if (neighbor_pos - chunk_pos).reduce_partial_max() <= 1 || has_water {
                    neighbor_chunk.map(|c| c.get_base_z())
                } else {
                    None
                }
            })
            .fold(None, |a: Option<f32>, x| a.map(|a| a.min(x)).or(Some(x)))
    }

    pub(crate) fn generation_chunk_anchor(
        &self,
        chunk_pos: Vec2<i32>,
    ) -> Option<ChunkGenerationAnchor<'_>> {
        let base_z = self.get_base_z(chunk_pos)? as i32;
        let sim_chunk = self.get(chunk_pos)?;
        Some(ChunkGenerationAnchor { base_z, sim_chunk })
    }

    pub fn get_interpolated<T, F>(&self, pos: Vec2<i32>, mut f: F) -> Option<T>
    where
        T: Copy + Default + Add<Output = T> + Mul<f32, Output = T>,
        F: FnMut(&SimChunk) -> T,
    {
        let pos = pos.as_::<f64>().wpos_to_cpos();
        let sample_chunk = |offset| {
            self.topology
                .interpolation_chunk(pos, offset)
                .and_then(|chunk_pos| self.get(chunk_pos))
        };

        let cubic = |a: T, b: T, c: T, d: T, x: f32| -> T {
            let x2 = x * x;

            // Catmull-Rom splines
            let co0 = a * -0.5 + b * 1.5 + c * -1.5 + d * 0.5;
            let co1 = a + b * -2.5 + c * 2.0 + d * -0.5;
            let co2 = a * -0.5 + c * 0.5;
            let co3 = b;

            co0 * x2 * x + co1 * x2 + co2 * x + co3
        };

        let mut x = [T::default(); 4];

        for (x_idx, j) in (-1..3).enumerate() {
            let y0 = f(sample_chunk(Vec2::new(j, -1))?);
            let y1 = f(sample_chunk(Vec2::new(j, 0))?);
            let y2 = f(sample_chunk(Vec2::new(j, 1))?);
            let y3 = f(sample_chunk(Vec2::new(j, 2))?);

            x[x_idx] = cubic(y0, y1, y2, y3, pos.y.fract() as f32);
        }

        Some(cubic(x[0], x[1], x[2], x[3], pos.x.fract() as f32))
    }

    /// M. Steffen splines.
    ///
    /// A more expensive cubic interpolation function that can preserve
    /// monotonicity between points.  This is useful if you rely on relative
    /// differences between endpoints being preserved at all interior
    /// points.  For example, we use this with riverbeds (and water
    /// height on along rivers) to maintain the invariant that the rivers always
    /// flow downhill at interior points (not just endpoints), without
    /// needing to flatten out the river.
    pub fn get_interpolated_monotone<T, F>(&self, pos: Vec2<i32>, mut f: F) -> Option<T>
    where
        T: Copy + Default + Signed + Float + Add<Output = T> + Mul<f32, Output = T>,
        F: FnMut(&SimChunk) -> T,
    {
        // See http://articles.adsabs.harvard.edu/cgi-bin/nph-iarticle_query?1990A%26A...239..443S&defaultprint=YES&page_ind=0&filetype=.pdf
        //
        // Note that these are only guaranteed monotone in one dimension; fortunately,
        // that is sufficient for our purposes.
        let pos = pos.as_::<f64>().wpos_to_cpos();
        let sample_chunk = |offset| {
            self.topology
                .interpolation_chunk(pos, offset)
                .and_then(|chunk_pos| self.get(chunk_pos))
        };

        let secant = |b: T, c: T| c - b;

        let parabola = |a: T, c: T| -a * 0.5 + c * 0.5;

        let slope = |_a: T, _b: T, _c: T, s_a: T, s_b: T, p_b: T| {
            // ((b - a).signum() + (c - b).signum()) * s
            (s_a.signum() + s_b.signum()) * (s_a.abs().min(s_b.abs()).min(p_b.abs() * 0.5))
        };

        let cubic = |a: T, b: T, c: T, d: T, x: f32| -> T {
            // Compute secants.
            let s_a = secant(a, b);
            let s_b = secant(b, c);
            let s_c = secant(c, d);
            // Computing slopes from parabolas.
            let p_b = parabola(a, c);
            let p_c = parabola(b, d);
            // Get slopes (setting distance between neighbors to 1.0).
            let slope_b = slope(a, b, c, s_a, s_b, p_b);
            let slope_c = slope(b, c, d, s_b, s_c, p_c);
            let x2 = x * x;

            // Interpolating splines.
            let co0 = slope_b + slope_c - s_b * 2.0;
            // = a * -0.5 + c * 0.5 + b * -0.5 + d * 0.5 - 2 * (c - b)
            // = a * -0.5 + b * 1.5 - c * 1.5 + d * 0.5;
            let co1 = s_b * 3.0 - slope_b * 2.0 - slope_c;
            // = (3.0 * (c - b) - 2.0 * (a * -0.5 + c * 0.5) - (b * -0.5 + d * 0.5))
            // = a + b * -2.5 + c * 2.0 + d * -0.5;
            let co2 = slope_b;
            // = a * -0.5 + c * 0.5;
            let co3 = b;

            co0 * x2 * x + co1 * x2 + co2 * x + co3
        };

        let mut x = [T::default(); 4];

        for (x_idx, j) in (-1..3).enumerate() {
            let y0 = f(sample_chunk(Vec2::new(j, -1))?);
            let y1 = f(sample_chunk(Vec2::new(j, 0))?);
            let y2 = f(sample_chunk(Vec2::new(j, 1))?);
            let y3 = f(sample_chunk(Vec2::new(j, 2))?);

            x[x_idx] = cubic(y0, y1, y2, y3, pos.y.fract() as f32);
        }

        Some(cubic(x[0], x[1], x[2], x[3], pos.x.fract() as f32))
    }

    /// Bilinear interpolation.
    ///
    /// Linear interpolation in both directions (i.e. quadratic interpolation).
    pub fn get_interpolated_bilinear<T, F>(&self, pos: Vec2<i32>, mut f: F) -> Option<T>
    where
        T: Copy + Default + Signed + Float + Add<Output = T> + Mul<f32, Output = T>,
        F: FnMut(&SimChunk) -> T,
    {
        // (i) Find downhill for all four points.
        // (ii) Compute distance from each downhill point and do linear interpolation on
        // their heights. (iii) Compute distance between each neighboring point
        // and do linear interpolation on       their distance-interpolated
        // heights.

        // See http://articles.adsabs.harvard.edu/cgi-bin/nph-iarticle_query?1990A%26A...239..443S&defaultprint=YES&page_ind=0&filetype=.pdf
        //
        // Note that these are only guaranteed monotone in one dimension; fortunately,
        // that is sufficient for our purposes.
        let pos = pos.as_::<f64>().wpos_to_cpos();
        let sample_chunk = |offset| {
            self.topology
                .interpolation_chunk(pos, offset)
                .and_then(|chunk_pos| self.get(chunk_pos))
        };

        // Orient the chunk in the direction of the most downhill point of the four.  If
        // there is no "most downhill" point, then we don't care.
        let p0 = sample_chunk(Vec2::new(0, 0))?;
        let y0 = f(p0);

        let p1 = sample_chunk(Vec2::new(1, 0))?;
        let y1 = f(p1);

        let p2 = sample_chunk(Vec2::new(0, 1))?;
        let y2 = f(p2);

        let p3 = sample_chunk(Vec2::new(1, 1))?;
        let y3 = f(p3);

        let z0 = y0
            .mul(1.0 - pos.x.fract() as f32)
            .mul(1.0 - pos.y.fract() as f32);
        let z1 = y1.mul(pos.x.fract() as f32).mul(1.0 - pos.y.fract() as f32);
        let z2 = y2.mul(1.0 - pos.x.fract() as f32).mul(pos.y.fract() as f32);
        let z3 = y3.mul(pos.x.fract() as f32).mul(pos.y.fract() as f32);

        Some(z0 + z1 + z2 + z3)
    }

    pub fn get_nearest_ways<'a, M: Clone + Lerp<Output = M>>(
        &'a self,
        wpos: Vec2<i32>,
        get_way: &'a impl Fn(&SimChunk) -> Option<(Way, M)>,
    ) -> impl Iterator<Item = NearestWaysData<M, impl FnOnce() -> Vec2<f32>>> + 'a {
        let chunk_pos = self.chunk_pos_at_wpos(wpos);
        let get_chunk_centre = |chunk_pos: Vec2<i32>| {
            chunk_pos.map2(TerrainChunkSize::RECT_SIZE, |e, sz: u32| {
                e * sz as i32 + sz as i32 / 2
            })
        };

        LOCALITY
            .iter()
            .filter_map(move |ctrl| {
                let (way, meta) = get_way(self.get(chunk_pos + *ctrl)?)?;
                let ctrl_pos = get_chunk_centre(chunk_pos + *ctrl).map(|e| e as f32)
                    + way.offset.map(|e| e as f32);

                let chunk_connections = way.neighbors.count_ones();
                if chunk_connections == 0 {
                    return None;
                }

                let (start_pos, start_idx, start_meta) = if chunk_connections != 2 {
                    (ctrl_pos, None, meta.clone())
                } else {
                    let (start_idx, start_rpos) = NEIGHBORS
                        .iter()
                        .copied()
                        .enumerate()
                        .find(|(i, _)| way.neighbors & (1 << *i as u8) != 0)
                        .unwrap();
                    let start_pos_chunk = chunk_pos + *ctrl + start_rpos;
                    let (start_way, start_meta) = get_way(self.get(start_pos_chunk)?)?;
                    (
                        get_chunk_centre(start_pos_chunk).map(|e| e as f32)
                            + start_way.offset.map(|e| e as f32),
                        Some(start_idx),
                        start_meta,
                    )
                };

                Some(
                    NEIGHBORS
                        .iter()
                        .enumerate()
                        .filter(move |(i, _)| {
                            way.neighbors & (1 << *i as u8) != 0 && Some(*i) != start_idx
                        })
                        .filter_map(move |(i, end_rpos)| {
                            let end_pos_chunk = chunk_pos + *ctrl + end_rpos;
                            let (end_way, end_meta) = get_way(self.get(end_pos_chunk)?)?;
                            let end_pos = get_chunk_centre(end_pos_chunk).map(|e| e as f32)
                                + end_way.offset.map(|e| e as f32);

                            let bez = QuadraticBezier2 {
                                start: (start_pos + ctrl_pos) / 2.0,
                                ctrl: ctrl_pos,
                                end: (end_pos + ctrl_pos) / 2.0,
                            };
                            let nearest_interval = bez
                                .binary_search_point_by_steps(wpos.map(|e| e as f32), 16, 0.001)
                                .0
                                .clamped(0.0, 1.0);
                            let pos = bez.evaluate(nearest_interval);
                            let dist_sqrd = pos.distance_squared(wpos.map(|e| e as f32));
                            let meta = if nearest_interval < 0.5 {
                                Lerp::lerp(start_meta.clone(), meta.clone(), 0.5 + nearest_interval)
                            } else {
                                Lerp::lerp(meta.clone(), end_meta, nearest_interval - 0.5)
                            };
                            Some(NearestWaysData {
                                i,
                                dist_sqrd,
                                pos,
                                meta,
                                bezier: bez,
                                calc_tangent: move || {
                                    bez.evaluate_derivative(nearest_interval).normalized()
                                },
                            })
                        }),
                )
            })
            .flatten()
    }

    /// Return the distance to the nearest way in blocks, along with the
    /// closest point on the way, the way metadata, and the tangent vector
    /// of that way.
    pub fn get_nearest_way<M: Clone + Lerp<Output = M>>(
        &self,
        wpos: Vec2<i32>,
        get_way: impl Fn(&SimChunk) -> Option<(Way, M)>,
    ) -> Option<(f32, Vec2<f32>, M, Vec2<f32>)> {
        let get_way = &get_way;
        self.get_nearest_ways(wpos, get_way)
            .min_by_key(|NearestWaysData { dist_sqrd, .. }| (dist_sqrd * 1024.0) as i32)
            .map(
                |NearestWaysData {
                     dist_sqrd,
                     pos,
                     meta,
                     calc_tangent,
                     ..
                 }| (dist_sqrd.sqrt(), pos, meta, calc_tangent()),
            )
    }

    fn supports_nearest_path_query(&self, wpos: Vec2<i32>) -> bool { self.get_wpos(wpos).is_some() }

    fn get_nearest_path_best_effort(
        &self,
        wpos: Vec2<i32>,
    ) -> Option<(f32, Vec2<f32>, Path, Vec2<f32>)> {
        self.get_nearest_way(wpos, |chunk| Some(chunk.path))
    }

    /// Best-effort path proximity query.
    ///
    /// On bounded topologies this keeps the historical behavior where a query
    /// just outside the world can still match an in-bounds border path if the
    /// LOCALITY scan reaches valid chunks.
    pub fn get_nearest_path(&self, wpos: Vec2<i32>) -> Option<(f32, Vec2<f32>, Path, Vec2<f32>)> {
        self.get_nearest_path_best_effort(wpos)
    }

    /// Path proximity query gated by the topology-valid sim query domain.
    ///
    /// This is the strict sibling of `get_nearest_path(...)`: it rejects wpos
    /// requests that do not resolve to a valid sim chunk under the active
    /// topology before running the historical best-effort path search.
    pub fn get_nearest_path_if_queryable(
        &self,
        wpos: Vec2<i32>,
    ) -> Option<(f32, Vec2<f32>, Path, Vec2<f32>)> {
        if !self.supports_nearest_path_query(wpos) {
            return None;
        }

        self.get_nearest_path_best_effort(wpos)
    }

    /// Create a [`Lottery<Option<ForestKind>>`] that generates [`ForestKind`]s
    /// according to the conditions at the given position. If no or fewer
    /// trees are appropriate for the conditions, `None` may be generated.
    pub fn make_forest_lottery(&self, wpos: Vec2<i32>) -> Lottery<Option<ForestKind>> {
        let chunk = if let Some(chunk) = self.get_wpos(wpos) {
            chunk
        } else {
            return Lottery::from(vec![(1.0, None)]);
        };
        let env = chunk.get_environment();
        Lottery::from(
            ForestKind::iter()
                .enumerate()
                .map(|(i, fk)| {
                    const CLUSTER_SIZE: f64 = 48.0;
                    let nz = (FastNoise2d::new(i as u32 * 37)
                        .get(wpos.map(|e| e as f64) / CLUSTER_SIZE)
                        + 1.0)
                        / 2.0;
                    (fk.proclivity(&env) * nz, Some(fk))
                })
                .chain(std::iter::once((0.001, None)))
                .collect::<Vec<_>>(),
        )
    }

    /// WARNING: Not currently used by the tree layer. Needs to be reworked.
    /// Return an iterator over candidate tree positions (note that only some of
    /// these will become trees since environmental parameters may forbid
    /// them spawning).
    pub fn get_near_trees(&self, wpos: Vec2<i32>) -> impl Iterator<Item = TreeAttr> + '_ {
        // Deterministic based on wpos
        self.gen_ctx
            .structure_gen
            .get(wpos)
            .into_iter()
            .filter_map(move |(wpos, seed)| {
                let lottery = self.make_forest_lottery(wpos);
                Some(TreeAttr {
                    pos: wpos,
                    seed,
                    scale: 1.0,
                    forest_kind: *lottery.choose_seeded(seed).as_ref()?,
                    inhabited: false,
                })
            })
    }

    pub fn get_area_trees(
        &self,
        wpos_min: Vec2<i32>,
        wpos_max: Vec2<i32>,
    ) -> impl Iterator<Item = TreeAttr> + '_ {
        self.gen_ctx
            .structure_gen
            .iter(wpos_min, wpos_max)
            .filter_map(move |(wpos, seed)| {
                let lottery = self.make_forest_lottery(wpos);
                Some(TreeAttr {
                    pos: wpos,
                    seed,
                    scale: 1.0,
                    forest_kind: *lottery.choose_seeded(seed).as_ref()?,
                    inhabited: false,
                })
            })
    }
}

#[derive(Debug)]
pub struct SimChunk {
    pub chaos: f32,
    pub alt: f32,
    pub basement: f32,
    pub water_alt: f32,
    pub downhill: Option<Vec2<i32>>,
    pub flux: f32,
    pub temp: f32,
    pub humidity: f32,
    pub rockiness: f32,
    pub tree_density: f32,
    pub forest_kind: ForestKind,
    pub spawn_rate: f32,
    pub river: RiverData,
    pub surface_veg: f32,

    pub sites: Vec<Id<Site>>,
    pub place: Option<Id<Place>>,
    pub poi: Option<Id<PointOfInterest>>,

    pub path: (Way, Path),
    pub cliff_height: f32,
    pub spot: Option<Spot>,

    pub contains_waypoint: bool,
}

#[derive(Copy, Clone)]
pub struct RegionInfo {
    pub chunk_pos: Vec2<i32>,
    pub block_pos: Vec2<i32>,
    pub dist: f32,
    pub seed: u32,
}

pub struct NearestWaysData<M, F: FnOnce() -> Vec2<f32>> {
    pub i: usize,
    pub dist_sqrd: f32,
    pub pos: Vec2<f32>,
    pub meta: M,
    pub bezier: QuadraticBezier2<f32>,
    pub calc_tangent: F,
}

impl SimChunk {
    fn environment_near_water_value(alt: f32, water_alt: f32, river: &RiverData) -> f32 {
        let water_access = WaterAccessClass::from_world_facts(
            WaterBodyKind::from_river_data(river),
            water_alt > alt,
            river.near_water(),
            water_alt,
        );

        if matches!(
            water_access,
            WaterAccessClass::FreshwaterShoreline | WaterAccessClass::FreshwaterSubmerged
        ) || alt < CONFIG.sea_level + 6.0
        {
            1.0
        } else {
            0.0
        }
    }

    fn build_environment(
        humidity: f32,
        temp: f32,
        alt: f32,
        water_alt: f32,
        river: &RiverData,
    ) -> Environment {
        Environment {
            humid: humidity,
            temp,
            near_water: Self::environment_near_water_value(alt, water_alt, river),
        }
    }

    fn generate(map_size_lg: MapSizeLg, posi: usize, gen_ctx: &GenCtx, gen_cdf: &GenCdf) -> Self {
        let pos = uniform_idx_as_vec2(map_size_lg, posi);
        let wposf = (pos * TerrainChunkSize::RECT_SIZE.map(|e| e as i32)).map(|e| e as f64);

        let (_, chaos) = gen_cdf.chaos[posi];
        let alt_pre = gen_cdf.alt[posi] as f32;
        let basement_pre = gen_cdf.basement[posi] as f32;
        let water_alt_pre = gen_cdf.water_alt[posi];
        let downhill_pre = gen_cdf.dh[posi];
        let flux = gen_cdf.flux[posi] as f32;
        let river = gen_cdf.rivers[posi].clone();

        // Can have NaNs in non-uniform part where pure_water returned true.  We just
        // test one of the four in order to find out whether this is the case.
        let (flux_uniform, /* flux_non_uniform */ _) = gen_cdf.pure_flux[posi];
        let (alt_uniform, _) = gen_cdf.alt_no_water[posi];
        let (temp_uniform, _) = gen_cdf.temp_base[posi];
        let (humid_uniform, _) = gen_cdf.humid_base[posi];

        /* // Vertical difference from the equator (NOTE: "uniform" with much lower granularity than
        // other uniform quantities, but hopefully this doesn't matter *too* much--if it does, we
        // can always add a small x component).
        //
        // Not clear that we want this yet, let's see.
        let latitude_uniform = (pos.y as f32 / f32::from(self.map_size_lg().chunks().y)).sub(0.5).mul(2.0);

        // Even less granular--if this matters we can make the sign affect the quantity slightly.
        let abs_lat_uniform = latitude_uniform.abs(); */

        // We also correlate temperature negatively with altitude and absolute latitude,
        // using different weighting than we use for humidity.
        const TEMP_WEIGHTS: [f32; 3] = [/* 1.5, */ 1.0, 2.0, 1.0];
        let temp = cdf_irwin_hall(
            &TEMP_WEIGHTS,
            [
                temp_uniform,
                1.0 - alt_uniform, /* 1.0 - abs_lat_uniform*/
                (gen_ctx.rock_nz.get((wposf.div(50000.0)).into_array()) as f32 * 2.5 + 1.0) * 0.5,
            ],
        )
        // Convert to [-1, 1]
        .sub(0.5)
        .mul(2.0);

        // Take the weighted average of our randomly generated base humidity, and the
        // calculated water flux over this point in order to compute humidity.
        const HUMID_WEIGHTS: [f32; 3] = [1.0, 1.0, 0.75];
        let humidity = cdf_irwin_hall(&HUMID_WEIGHTS, [humid_uniform, flux_uniform, 1.0]);
        // Moisture evaporates more in hot places
        let humidity = humidity
            * (1.0
                - (temp - CONFIG.tropical_temp)
                    .max(0.0)
                    .div(1.0 - CONFIG.tropical_temp))
            .max(0.0);

        let mut alt = CONFIG.sea_level.add(alt_pre);
        let basement = CONFIG.sea_level.add(basement_pre);
        let water_alt = CONFIG.sea_level.add(water_alt_pre);
        let (downhill, _gradient) = if downhill_pre == -2 {
            (None, 0.0)
        } else if downhill_pre < 0 {
            panic!("Uh... shouldn't this never, ever happen?");
        } else {
            (
                Some(
                    uniform_idx_as_vec2(map_size_lg, downhill_pre as usize)
                        * TerrainChunkSize::RECT_SIZE.map(|e| e as i32)
                        + TerrainChunkSize::RECT_SIZE.map(|e| e as i32 / 2),
                ),
                (alt_pre - gen_cdf.alt[downhill_pre as usize] as f32).abs()
                    / TerrainChunkSize::RECT_SIZE.x as f32,
            )
        };

        // Logistic regression.  Make sure x ∈ (0, 1).
        let logit = |x: f64| x.ln() - x.neg().ln_1p();
        // 0.5 + 0.5 * tanh(ln(1 / (1 - 0.1) - 1) / (2 * (sqrt(3)/pi)))
        let logistic_2_base = 3.0f64.sqrt().mul(std::f64::consts::FRAC_2_PI);
        // Assumes μ = 0, σ = 1
        let logistic_cdf = |x: f64| x.div(logistic_2_base).tanh().mul(0.5).add(0.5);

        let is_underwater = match river.river_kind {
            Some(RiverKind::Ocean) | Some(RiverKind::Lake { .. }) => true,
            Some(RiverKind::River { .. }) => false, // TODO: inspect width
            None => false,
        };
        let river_xy = Vec2::new(river.velocity.x, river.velocity.y).magnitude();
        let river_slope = river.velocity.z / river_xy;
        match river.river_kind {
            Some(RiverKind::River { cross_section }) => {
                if cross_section.x >= 0.5 && cross_section.y >= CONFIG.river_min_height {
                    /* println!(
                        "Big area! Pos area: {:?}, River data: {:?}, slope: {:?}",
                        wposf, river, river_slope
                    ); */
                }
                if river_slope.abs() >= 0.25 && cross_section.x >= 1.0 {
                    let pos_area = wposf;
                    let river_data = &river;
                    debug!(?pos_area, ?river_data, ?river_slope, "Big waterfall!",);
                }
            },
            Some(RiverKind::Lake { .. }) => {
                // Forces lakes to be downhill from the land around them, and adds some noise to
                // the lake bed to make sure it's not too flat.
                let lake_bottom_nz = (gen_ctx.small_nz.get((wposf.div(20.0)).into_array()) as f32)
                    .clamp(-1.0, 1.0)
                    .mul(3.0);
                alt = alt.min(water_alt - 5.0) + lake_bottom_nz;
            },
            _ => {},
        }

        // No trees in the ocean, with zero humidity (currently), or directly on
        // bedrock.
        let tree_density = if is_underwater {
            0.0
        } else {
            let tree_density = Lerp::lerp(
                -1.5,
                2.5,
                gen_ctx.tree_nz.get((wposf.div(1024.0)).into_array()) * 0.5 + 0.5,
            )
            .clamp(0.0, 1.0);
            // Tree density should go (by a lot) with humidity.
            if humidity <= 0.0 || tree_density <= 0.0 {
                0.0
            } else if humidity >= 1.0 || tree_density >= 1.0 {
                1.0
            } else {
                // Weighted logit sum.
                logistic_cdf(logit(tree_density))
            }
            // rescale to (-0.95, 0.95)
            .sub(0.5)
            .add(0.5)
        } as f32;
        const MIN_TREE_HUM: f32 = 0.15;
        let tree_density = tree_density
            // Tree density increases exponentially with humidity...
            .mul((humidity - MIN_TREE_HUM).max(0.0).mul(1.0 + MIN_TREE_HUM) / temp.max(0.75))
            // Places that are *too* wet (like marshes) also get fewer trees because the ground isn't stable enough for
            // them.
            //.mul((1.0 - flux * 0.05/*(humidity - 0.9).max(0.0) / 0.1*/).max(0.0))
            .mul(0.25 + flux * 0.05)
            // ...but is ultimately limited by available sunlight (and our tree generation system)
            .min(1.0);

        // Add geologically short timescale undulation to the world for various reasons
        let alt =
            // Don't add undulation to rivers, mainly because this could accidentally result in rivers flowing uphill
            if river.near_water() {
                alt
            } else {
                // Sand dunes (formed over a short period of time, so we don't care about erosion sim)
                let warp = Vec2::new(
                    gen_ctx.turb_x_nz.get(wposf.div(350.0).into_array()) as f32,
                    gen_ctx.turb_y_nz.get(wposf.div(350.0).into_array()) as f32,
                ) * 200.0;
                const DUNE_SCALE: f32 = 24.0;
                const DUNE_LEN: f32 = 96.0;
                const DUNE_DIR: Vec2<f32> = Vec2::new(1.0, 1.0);
                let dune_dist = (wposf.map(|e| e as f32) + warp)
                    .div(DUNE_LEN)
                    .mul(DUNE_DIR.normalized())
                    .sum();
                let dune_nz = 0.5 - dune_dist.sin().abs() + 0.5 * (dune_dist + 0.5).sin().abs();
                let dune = dune_nz * DUNE_SCALE * (temp - 0.75).clamped(0.0, 0.25) * 4.0;

                // Trees bind to soil and their roots result in small accumulating undulations over geologically short
                // periods of time. Forest floors are generally significantly bumpier than that of deforested areas.
                // This is particularly pronounced in high-humidity areas.
                let soil_nz = gen_ctx.hill_nz.get(wposf.div(96.0).into_array()) as f32;
                let soil_nz = (soil_nz + 1.0) * 0.5;
                const SOIL_SCALE: f32 = 16.0;
                let soil = soil_nz * SOIL_SCALE * tree_density.sqrt() * humidity.sqrt();

                let warp_factor = ((alt - CONFIG.sea_level) / 16.0).clamped(0.0, 1.0);

                let warp = (dune + soil) * warp_factor;

                // Prevent warping pushing the altitude underwater
                if alt + warp < water_alt {
                    alt
                } else {
                    alt + warp
                }
            };

        Self {
            chaos,
            flux,
            alt,
            basement: basement.min(alt),
            water_alt,
            downhill,
            temp,
            humidity,
            rockiness: if true {
                (gen_ctx.rock_nz.get((wposf.div(1024.0)).into_array()) as f32)
                    //.add(if river.near_river() { 20.0 } else { 0.0 })
                    .sub(0.1)
                    .mul(1.3)
                    .max(0.0)
            } else {
                0.0
            },
            tree_density,
            forest_kind: {
                let env = Self::build_environment(humidity, temp, alt, water_alt, &river);

                ForestKind::iter()
                    .max_by_key(|fk| (fk.proclivity(&env) * 10000.0) as u32)
                    .unwrap() // Can't fail
            },
            spawn_rate: 1.0,
            river,
            surface_veg: 1.0,

            sites: Vec::new(),
            place: None,
            poi: None,
            path: Default::default(),
            cliff_height: 0.0,
            spot: None,

            contains_waypoint: false,
        }
    }

    pub fn is_underwater(&self) -> bool {
        self.water_alt > self.alt || self.river.river_kind.is_some()
    }

    pub fn get_base_z(&self) -> f32 { self.alt - self.chaos * 50.0 - 16.0 }

    pub fn get_biome(&self) -> BiomeKind {
        let savannah_hum_temp = [0.05..0.55, 0.3..1.6];
        let taiga_hum_temp = [0.2..1.4, -0.7..-0.3];
        match WaterBodyKind::from_river_data(&self.river) {
            WaterBodyKind::Ocean => BiomeKind::Ocean,
            WaterBodyKind::Lake => BiomeKind::Lake,
            WaterBodyKind::River | WaterBodyKind::DryLand if self.temp < CONFIG.snow_temp => {
                BiomeKind::Snowland
            },
            WaterBodyKind::River | WaterBodyKind::DryLand
                if self.alt > 500.0 && self.chaos > 0.3 && self.tree_density < 0.6 =>
            {
                BiomeKind::Mountain
            },
            WaterBodyKind::River | WaterBodyKind::DryLand
                if self.temp > CONFIG.desert_temp && self.humidity < CONFIG.desert_hum =>
            {
                BiomeKind::Desert
            },
            WaterBodyKind::River | WaterBodyKind::DryLand
                if self.tree_density > 0.65 && self.humidity > 0.65 && self.temp > 0.45 =>
            {
                BiomeKind::Jungle
            },
            WaterBodyKind::River | WaterBodyKind::DryLand
                if savannah_hum_temp[0].contains(&self.humidity)
                    && savannah_hum_temp[1].contains(&self.temp) =>
            {
                BiomeKind::Savannah
            },
            WaterBodyKind::River | WaterBodyKind::DryLand
                if taiga_hum_temp[0].contains(&self.humidity)
                    && taiga_hum_temp[1].contains(&self.temp) =>
            {
                BiomeKind::Taiga
            },
            WaterBodyKind::River | WaterBodyKind::DryLand if self.tree_density > 0.4 => {
                BiomeKind::Forest
            },
            WaterBodyKind::River | WaterBodyKind::DryLand => BiomeKind::Grassland,
        }
    }

    pub fn near_cliffs(&self) -> bool { self.cliff_height > 0.0 }

    pub fn get_environment(&self) -> Environment {
        Self::build_environment(
            self.humidity,
            self.temp,
            self.alt,
            self.water_alt,
            &self.river,
        )
    }

    pub fn get_location_name(
        &self,
        index_sites: &Store<crate::site::Site>,
        civs_pois: &Store<PointOfInterest>,
        wpos2d: Vec2<i32>,
    ) -> Option<String> {
        self.sites
            .iter()
            .filter(|id| {
                index_sites[**id].origin.distance_squared(wpos2d) as f32
                    <= index_sites[**id].radius().powi(2)
            })
            .min_by_key(|id| index_sites[**id].origin.distance_squared(wpos2d))
            .and_then(|id| Some(index_sites[*id].name()?.to_string()))
            .or_else(|| self.poi.map(|poi| civs_pois[poi].name.clone()))
    }
}

#[cfg(test)]
mod compat_tests {
    use super::{
        CompatAuditV1, CompatMode, DEFAULT_WORLD_MAP, DEFAULT_WORLD_SEED, FileOpts, GenOpts,
        GeneratedWorldFinalizeBuilderInputs, GeneratedWorldFinalizeBuilderPreparationRequest,
        GeneratedWorldFinalizeInputs, GeneratedWorldFinalizePreparationRequest,
        GeneratedWorldPartsInputs, GenerationChunkInputsRequest, GenerationTunables,
        LoadLegacyMode, LoadOrGenerateSidecarlessMode, WorldFile, WorldLoadBootstrap,
        WorldLoadBootstrapRequest, WorldMap_0_5_0, WorldMap_0_7_0, WorldSim,
        default_world_asset_gen_opts,
    };
    use crate::recipe::{
        CompatDecisionV1, CompatFailureKindV1, CompatFailureSubjectV1, RecipeManifestV1,
    };
    use bincode::{config::legacy, serde::encode_into_std_write};
    use common::terrain::MapSizeLg;
    use rayon::ThreadPoolBuilder;
    use std::{
        fs::{self, File},
        io::BufWriter,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };
    use vek::Vec2;

    const TEST_WORLD_SEED: u32 = 42;
    const TEST_SEED_ELEMENTS: bool = true;

    struct TempLoadOrGenerateTarget {
        file_path: PathBuf,
    }

    impl TempLoadOrGenerateTarget {
        fn new(tag: &str) -> (String, Self) {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos();
            let base = std::env::temp_dir().join(format!("caldrayne-world-{tag}-{unique}"));
            let file_path = base.with_extension("bin");

            (base.to_string_lossy().into_owned(), Self { file_path })
        }

        fn write_world_file(&self, world_file: &WorldFile) {
            if let Some(parent) = self.file_path.parent() {
                fs::create_dir_all(parent).expect("temp world parent should be creatable");
            }
            let file = File::create(&self.file_path).expect("temp world file should be writable");
            let mut writer = BufWriter::new(file);
            encode_into_std_write(world_file, &mut writer, legacy())
                .expect("temp world file should serialize");
        }

        fn recipe_sidecar_path(&self) -> PathBuf {
            FileOpts::recipe_sidecar_path_for_map_path(self.file_path.as_path())
        }

        fn write_recipe_manifest(&self, recipe_manifest: &RecipeManifestV1) {
            let sidecar_path = self.recipe_sidecar_path();
            let rendered_recipe_manifest =
                ron::ser::to_string_pretty(recipe_manifest, ron::ser::PrettyConfig::default())
                    .expect("recipe manifest should serialize");
            fs::write(sidecar_path, rendered_recipe_manifest)
                .expect("recipe sidecar should be writable");
        }

        fn write_invalid_recipe_sidecar(&self) {
            fs::write(self.recipe_sidecar_path(), "not valid ron")
                .expect("invalid recipe sidecar should be writable");
        }
    }

    impl Drop for TempLoadOrGenerateTarget {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.file_path);
            let _ = fs::remove_file(self.recipe_sidecar_path());
        }
    }

    fn load_or_generate_opts(name: String, overwrite: bool) -> FileOpts {
        FileOpts::LoadOrGenerate {
            name,
            opts: GenOpts::default(),
            overwrite,
        }
    }

    fn load_legacy_opts(path: PathBuf) -> FileOpts { FileOpts::LoadLegacy(path) }

    fn load_path_opts(path: PathBuf) -> FileOpts { FileOpts::Load(path) }

    fn load_asset_opts(specifier: &str) -> FileOpts { FileOpts::LoadAsset(specifier.to_owned()) }

    fn legacy_world_file() -> WorldFile {
        WorldFile::Veloren0_5_0(WorldMap_0_5_0 {
            alt: vec![0.0].into_boxed_slice(),
            basement: vec![0.0].into_boxed_slice(),
        })
    }

    fn invalid_legacy_world_file() -> WorldFile {
        WorldFile::Veloren0_5_0(WorldMap_0_5_0 {
            alt: vec![0.0; 3].into_boxed_slice(),
            basement: vec![0.0; 3].into_boxed_slice(),
        })
    }

    fn mismatched_world_file() -> WorldFile {
        WorldFile::Veloren0_7_0(WorldMap_0_7_0 {
            map_size_lg: Vec2::new(9, 9),
            continent_scale_hack: 3.0,
            alt: vec![0.0].into_boxed_slice(),
            basement: vec![0.0].into_boxed_slice(),
        })
    }

    fn matching_world_file() -> WorldFile {
        let opts = GenOpts::default();
        let map_cell_count = 1usize << (opts.x_lg + opts.y_lg);
        WorldFile::Veloren0_7_0(WorldMap_0_7_0 {
            map_size_lg: Vec2::new(opts.x_lg, opts.y_lg),
            continent_scale_hack: opts.scale,
            alt: vec![0.0; map_cell_count].into_boxed_slice(),
            basement: vec![0.0; map_cell_count].into_boxed_slice(),
        })
    }

    fn recipe_manifest_for_seed(world_seed: u32) -> RecipeManifestV1 {
        RecipeManifestV1::record_only(world_seed, &GenOpts::default(), TEST_SEED_ELEMENTS)
    }

    fn assert_same_path(lhs: &Path, rhs: &Path) {
        assert_eq!(
            lhs, rhs,
            "expected temp load_or_generate path to stay stable during the test"
        );
    }

    #[test]
    fn save_writes_recipe_sidecar_for_managed_worlds() {
        let (_, temp_target) = TempLoadOrGenerateTarget::new("recipe-sidecar-save");
        let recipe_manifest = recipe_manifest_for_seed(TEST_WORLD_SEED);
        let save_opts = FileOpts::Save(temp_target.file_path.clone(), GenOpts::default());

        save_opts.save(&matching_world_file(), &recipe_manifest);

        let sidecar_file =
            File::open(temp_target.recipe_sidecar_path()).expect("recipe sidecar should exist");
        let stored_recipe_manifest: RecipeManifestV1 =
            ron::de::from_reader(sidecar_file).expect("recipe sidecar should deserialize");

        assert_eq!(
            stored_recipe_manifest.world_recipe_hash,
            recipe_manifest.world_recipe_hash
        );
        assert_eq!(
            stored_recipe_manifest.chunk_recipe_hash,
            recipe_manifest.chunk_recipe_hash
        );
    }

    #[test]
    fn load_path_legacy_world_rejects_strict_modern_load() {
        let (_, temp_target) = TempLoadOrGenerateTarget::new("load-path-legacy-reject");
        temp_target.write_world_file(&legacy_world_file());

        let err = match load_path_opts(temp_target.file_path.clone()).load_content(
            CompatMode::Record,
            TEST_WORLD_SEED,
            TEST_SEED_ELEMENTS,
        ) {
            Ok(_) => panic!("Load(path) should reject legacy world versions"),
            Err(err) => err,
        };

        assert!(err.audit.is_rejected());
        assert_eq!(err.audit.failure_kind, CompatFailureKindV1::InvalidWorld);
        assert_eq!(err.audit.failure_subject, CompatFailureSubjectV1::World);
        assert!(err.audit.failure_detail.legacy_world_version);
    }

    #[test]
    fn load_path_requires_recipe_sidecar() {
        let (_, temp_target) = TempLoadOrGenerateTarget::new("load-path-sidecar-missing");
        temp_target.write_world_file(&matching_world_file());

        let err = match load_path_opts(temp_target.file_path.clone()).load_content(
            CompatMode::Record,
            TEST_WORLD_SEED,
            TEST_SEED_ELEMENTS,
        ) {
            Ok(_) => panic!("Load(path) should reject when recipe sidecar is missing"),
            Err(err) => err,
        };

        assert!(err.audit.is_rejected());
        assert_eq!(err.audit.failure_kind, CompatFailureKindV1::MissingInput);
        assert_eq!(err.audit.failure_subject, CompatFailureSubjectV1::Recipe);
    }

    #[test]
    fn load_path_loads_existing_world_with_recipe_sidecar() {
        let (_, temp_target) = TempLoadOrGenerateTarget::new("load-path-sidecar-match");
        let recipe_manifest = recipe_manifest_for_seed(TEST_WORLD_SEED + 17);
        temp_target.write_world_file(&matching_world_file());
        temp_target.write_recipe_manifest(&recipe_manifest);

        let content = load_path_opts(temp_target.file_path.clone())
            .load_content(CompatMode::Record, TEST_WORLD_SEED, TEST_SEED_ELEMENTS)
            .expect("Load(path) should accept matching strict world + sidecar");

        assert!(content.parsed_world_file.is_some());
        assert_eq!(
            content.compat_audit.decision,
            CompatDecisionV1::LoadedExisting
        );
        assert_eq!(
            content
                .loaded_recipe_manifest
                .as_ref()
                .expect("loaded recipe manifest should be present")
                .world_recipe
                .world_seed,
            recipe_manifest.world_recipe.world_seed
        );
        assert_eq!(
            content.gen_opts.x_lg,
            recipe_manifest.world_recipe.gen_opts.x_lg
        );
        assert_eq!(
            content.gen_opts.y_lg,
            recipe_manifest.world_recipe.gen_opts.y_lg
        );
        assert_eq!(
            content.gen_opts.scale,
            recipe_manifest.world_recipe.gen_opts.scale
        );
    }

    #[test]
    fn load_path_rejects_recipe_sidecar_world_mismatch() {
        let (_, temp_target) = TempLoadOrGenerateTarget::new("load-path-sidecar-mismatch");
        temp_target.write_world_file(&mismatched_world_file());
        temp_target.write_recipe_manifest(&recipe_manifest_for_seed(TEST_WORLD_SEED));

        let err = match load_path_opts(temp_target.file_path.clone()).load_content(
            CompatMode::Record,
            TEST_WORLD_SEED,
            TEST_SEED_ELEMENTS,
        ) {
            Ok(_) => panic!("Load(path) should reject when sidecar and world file disagree"),
            Err(err) => err,
        };

        assert!(err.audit.is_rejected());
        assert_eq!(err.audit.failure_kind, CompatFailureKindV1::OptionMismatch);
        assert_eq!(err.audit.failure_subject, CompatFailureSubjectV1::Recipe);
        assert!(err.audit.failure_detail.world_size_mismatch);
        assert!(err.audit.failure_detail.world_scale_mismatch);
    }

    #[test]
    fn load_path_rejects_corrupt_recipe_sidecar() {
        let (_, temp_target) = TempLoadOrGenerateTarget::new("load-path-sidecar-corrupt");
        temp_target.write_world_file(&matching_world_file());
        temp_target.write_invalid_recipe_sidecar();

        let err = match load_path_opts(temp_target.file_path.clone()).load_content(
            CompatMode::Record,
            TEST_WORLD_SEED,
            TEST_SEED_ELEMENTS,
        ) {
            Ok(_) => panic!("Load(path) should reject corrupt recipe sidecars"),
            Err(err) => err,
        };

        assert!(err.audit.is_rejected());
        assert_eq!(err.audit.failure_kind, CompatFailureKindV1::ParseError);
        assert_eq!(err.audit.failure_subject, CompatFailureSubjectV1::Recipe);
    }

    #[test]
    fn load_path_rejects_internally_invalid_recipe_sidecar() {
        let (_, temp_target) = TempLoadOrGenerateTarget::new("load-path-sidecar-invalid");
        let mut recipe_manifest = recipe_manifest_for_seed(TEST_WORLD_SEED);
        recipe_manifest.world_recipe_hash = "tampered-world-recipe-hash".to_owned();
        temp_target.write_world_file(&matching_world_file());
        temp_target.write_recipe_manifest(&recipe_manifest);

        let err = match load_path_opts(temp_target.file_path.clone()).load_content(
            CompatMode::Record,
            TEST_WORLD_SEED,
            TEST_SEED_ELEMENTS,
        ) {
            Ok(_) => panic!("Load(path) should reject internally inconsistent recipe sidecars"),
            Err(err) => err,
        };

        assert!(err.audit.is_rejected());
        assert_eq!(err.audit.failure_kind, CompatFailureKindV1::InvalidWorld);
        assert_eq!(err.audit.failure_subject, CompatFailureSubjectV1::Recipe);
    }

    #[test]
    fn load_legacy_imports_sidecarless_modern_world_with_inferred_gen_opts() {
        let (_, temp_target) = TempLoadOrGenerateTarget::new("load-legacy-modern-import");
        temp_target.write_world_file(&mismatched_world_file());

        let content = load_legacy_opts(temp_target.file_path.clone())
            .load_content(CompatMode::Record, TEST_WORLD_SEED, TEST_SEED_ELEMENTS)
            .expect("LoadLegacy(path) should import sidecarless modern worlds explicitly");

        assert!(content.parsed_world_file.is_some());
        assert!(content.loaded_recipe_manifest.is_none());
        assert_eq!(
            content.compat_audit.decision,
            CompatDecisionV1::LoadedExisting
        );
        assert_eq!(content.gen_opts.x_lg, 9);
        assert_eq!(content.gen_opts.y_lg, 9);
        assert_eq!(content.gen_opts.scale, 3.0);
    }

    #[test]
    fn load_legacy_rejects_missing_file_without_fallback() {
        let (_, temp_target) = TempLoadOrGenerateTarget::new("load-legacy-missing");

        let err = match load_legacy_opts(temp_target.file_path.clone()).load_content(
            CompatMode::Record,
            TEST_WORLD_SEED,
            TEST_SEED_ELEMENTS,
        ) {
            Ok(_) => panic!("LoadLegacy(path) should reject a missing compat-import target"),
            Err(err) => err,
        };

        assert!(err.audit.is_rejected());
        assert_eq!(err.audit.failure_kind, CompatFailureKindV1::MissingInput);
        assert_eq!(err.audit.failure_subject, CompatFailureSubjectV1::World);
    }

    #[test]
    fn load_legacy_rejects_invalid_legacy_world_without_fallback() {
        let (_, temp_target) = TempLoadOrGenerateTarget::new("load-legacy-invalid-world");
        temp_target.write_world_file(&invalid_legacy_world_file());

        let err = match load_legacy_opts(temp_target.file_path.clone()).load_content(
            CompatMode::Record,
            TEST_WORLD_SEED,
            TEST_SEED_ELEMENTS,
        ) {
            Ok(_) => panic!("LoadLegacy(path) should reject invalid compat-import inputs"),
            Err(err) => err,
        };

        assert!(err.audit.is_rejected());
        assert_eq!(err.audit.failure_kind, CompatFailureKindV1::InvalidWorld);
        assert_eq!(err.audit.failure_subject, CompatFailureSubjectV1::World);
    }

    #[test]
    fn load_legacy_rejects_when_compat_import_gate_is_closed() {
        let (_, temp_target) = TempLoadOrGenerateTarget::new("load-legacy-gate-closed");
        temp_target.write_world_file(&matching_world_file());

        let err = match load_legacy_opts(temp_target.file_path.clone())
            .load_content_with_legacy_mode(
                CompatMode::Record,
                LoadLegacyMode::Deny,
                TEST_WORLD_SEED,
                TEST_SEED_ELEMENTS,
            ) {
            Ok(_) => panic!("LoadLegacy(path) should reject when compat import gate is closed"),
            Err(err) => err,
        };

        assert!(err.audit.is_rejected());
        assert_eq!(err.audit.failure_kind, CompatFailureKindV1::PolicyDenied);
        assert_eq!(err.audit.failure_subject, CompatFailureSubjectV1::Options);
    }

    #[test]
    fn load_asset_default_world_uses_fixed_recipe_manifest() {
        let requested_seed = TEST_WORLD_SEED + 91;
        let default_asset_gen_opts = default_world_asset_gen_opts();

        let content = load_asset_opts(DEFAULT_WORLD_MAP)
            .load_content(CompatMode::Record, requested_seed, false)
            .expect("LoadAsset(default asset) should accept the fixed asset contract");

        assert!(content.parsed_world_file.is_some());
        assert_eq!(
            content.compat_audit.decision,
            CompatDecisionV1::LoadedExisting
        );

        let recipe_manifest = content
            .loaded_recipe_manifest
            .as_ref()
            .expect("default asset should surface a fixed recipe manifest");
        assert_eq!(recipe_manifest.world_recipe.world_seed, DEFAULT_WORLD_SEED);
        assert!(recipe_manifest.world_recipe.seed_elements);
        assert_eq!(
            recipe_manifest.world_recipe.gen_opts.x_lg,
            default_asset_gen_opts.x_lg
        );
        assert_eq!(
            recipe_manifest.world_recipe.gen_opts.y_lg,
            default_asset_gen_opts.y_lg
        );
        assert_eq!(
            recipe_manifest.world_recipe.gen_opts.scale,
            default_asset_gen_opts.scale
        );
        assert_eq!(content.gen_opts.x_lg, default_asset_gen_opts.x_lg);
        assert_eq!(content.gen_opts.y_lg, default_asset_gen_opts.y_lg);
        assert_eq!(content.gen_opts.scale, default_asset_gen_opts.scale);
    }

    #[test]
    fn load_asset_missing_specifier_rejects_without_fallback() {
        let err = match load_asset_opts("world.map.does_not_exist").load_content(
            CompatMode::Record,
            TEST_WORLD_SEED,
            TEST_SEED_ELEMENTS,
        ) {
            Ok(_) => panic!("LoadAsset(asset) should reject a missing asset specifier"),
            Err(err) => err,
        };

        assert!(err.audit.is_rejected());
        assert_eq!(err.audit.failure_kind, CompatFailureKindV1::MissingInput);
        assert_eq!(err.audit.failure_subject, CompatFailureSubjectV1::World);
    }

    #[test]
    fn load_asset_requires_fixed_recipe_manifest() {
        let err = match load_asset_opts("world.map.veloren_0_16_0_0").load_content(
            CompatMode::Record,
            TEST_WORLD_SEED,
            TEST_SEED_ELEMENTS,
        ) {
            Ok(_) => {
                panic!("LoadAsset(asset) should reject built-in assets without a fixed manifest")
            },
            Err(err) => err,
        };

        assert!(err.audit.is_rejected());
        assert_eq!(err.audit.failure_kind, CompatFailureKindV1::MissingInput);
        assert_eq!(err.audit.failure_subject, CompatFailureSubjectV1::Recipe);
    }

    #[test]
    fn prepare_generation_start_shapes_tunables_and_pre_erosion_setup() {
        let gen_opts = GenOpts::default();
        let map_size_lg = MapSizeLg::new(Vec2::new(gen_opts.x_lg, gen_opts.y_lg))
            .expect("default gen opts should map to valid world size");
        let threadpool = ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .expect("threadpool should build");

        let start = WorldSim::prepare_generation_start(
            TEST_WORLD_SEED,
            map_size_lg,
            &gen_opts,
            &threadpool,
        );
        let expected_tunables = GenerationTunables::new(&gen_opts);

        assert_eq!(
            start.generation_tunables.continent_scale,
            expected_tunables.continent_scale
        );
        assert_eq!(
            start.generation_tunables.rock_lacunarity,
            expected_tunables.rock_lacunarity
        );
        assert_eq!(
            start.generation_tunables.uplift_scale,
            expected_tunables.uplift_scale
        );

        let pre_erosion_model = start.pre_erosion_setup.model(map_size_lg, &start.gen_ctx);
        assert_eq!(pre_erosion_model.map_size_lg.chunks(), map_size_lg.chunks());
    }

    #[test]
    fn prepare_generation_chunk_inputs_shapes_loaded_world_into_post_erosion_inputs() {
        let gen_opts = GenOpts::default();
        let map_size_lg = MapSizeLg::new(Vec2::new(gen_opts.x_lg, gen_opts.y_lg))
            .expect("default gen opts should map to valid world size");
        let threadpool = ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .expect("threadpool should build");
        let start = WorldSim::prepare_generation_start(
            TEST_WORLD_SEED,
            map_size_lg,
            &gen_opts,
            &threadpool,
        );
        let recipe_manifest = RecipeManifestV1::record_only(TEST_WORLD_SEED, &gen_opts, true);
        let loaded_world = matching_world_file()
            .into_modern()
            .expect("fixture should be modern");

        let post = WorldSim::prepare_generation_chunk_inputs(GenerationChunkInputsRequest {
            parsed_world_file: Some(loaded_world),
            map_size_lg,
            gen_opts: &gen_opts,
            world_file: FileOpts::Generate(gen_opts.clone()),
            fresh: false,
            recipe_manifest: &recipe_manifest,
            gen_ctx: &start.gen_ctx,
            pre_erosion_setup: start.pre_erosion_setup,
            threadpool: &threadpool,
            stage_report: &|_| {},
        });

        assert!(post.max_height.is_finite());
        assert!(post.max_height >= 0.0);
        assert_eq!(post.gen_cdf.alt.len(), post.gen_cdf.basement.len());
        assert_eq!(
            post.gen_cdf.alt.len(),
            map_size_lg.chunks().map(|e| e as usize).product()
        );
    }

    #[test]
    fn generated_world_finalize_inputs_into_parts_preserves_seed_elements_as_finalize_gate() {
        let gen_opts = GenOpts::default();
        let map_size_lg = MapSizeLg::new(Vec2::new(gen_opts.x_lg, gen_opts.y_lg))
            .expect("default gen opts should map to valid world size");
        let threadpool = ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .expect("threadpool should build");
        let start = WorldSim::prepare_generation_start(
            TEST_WORLD_SEED,
            map_size_lg,
            &gen_opts,
            &threadpool,
        );
        let recipe_manifest = RecipeManifestV1::record_only(TEST_WORLD_SEED, &gen_opts, true);
        let loaded_world = matching_world_file()
            .into_modern()
            .expect("fixture should be modern");
        let post_erosion_chunk_inputs =
            WorldSim::prepare_generation_chunk_inputs(GenerationChunkInputsRequest {
                parsed_world_file: Some(loaded_world),
                map_size_lg,
                gen_opts: &gen_opts,
                world_file: FileOpts::Generate(gen_opts.clone()),
                fresh: false,
                recipe_manifest: &recipe_manifest,
                gen_ctx: &start.gen_ctx,
                pre_erosion_setup: start.pre_erosion_setup,
                threadpool: &threadpool,
                stage_report: &|_| {},
            });

        let inputs = GeneratedWorldFinalizeInputs {
            world_parts_inputs: GeneratedWorldPartsInputs {
                seed: TEST_WORLD_SEED,
                map_size_lg,
                gen_ctx: start.gen_ctx,
                post_erosion_chunk_inputs,
                rng: start.rng,
                calendar: None,
                compat_mode: CompatMode::Record,
                load_legacy_mode: LoadLegacyMode::Allow,
                load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode::Allow,
                compat_audit: CompatAuditV1::default(),
                managed_recipe_sidecar_missing: false,
                recipe_manifest: recipe_manifest.clone(),
            },
            seed_elements: true,
        };

        assert!(inputs.seed_elements);
        let parts = inputs.into_parts();
        assert_eq!(parts.seed, TEST_WORLD_SEED);
        assert_eq!(parts.map_size_lg.chunks(), map_size_lg.chunks());
        assert_eq!(
            parts.recipe_manifest.world_recipe_hash,
            recipe_manifest.world_recipe_hash
        );
        assert_eq!(
            parts.chunks.len(),
            map_size_lg.chunks().map(|e| e as usize).product()
        );
    }

    #[test]
    fn build_generated_world_finalize_inputs_keeps_finalize_payload_packing_at_single_boundary() {
        let gen_opts = GenOpts::default();
        let map_size_lg = MapSizeLg::new(Vec2::new(gen_opts.x_lg, gen_opts.y_lg))
            .expect("default gen opts should map to valid world size");
        let threadpool = ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .expect("threadpool should build");
        let start = WorldSim::prepare_generation_start(
            TEST_WORLD_SEED,
            map_size_lg,
            &gen_opts,
            &threadpool,
        );
        let recipe_manifest = RecipeManifestV1::record_only(TEST_WORLD_SEED, &gen_opts, true);
        let loaded_world = matching_world_file()
            .into_modern()
            .expect("fixture should be modern");
        let post_erosion_chunk_inputs =
            WorldSim::prepare_generation_chunk_inputs(GenerationChunkInputsRequest {
                parsed_world_file: Some(loaded_world),
                map_size_lg,
                gen_opts: &gen_opts,
                world_file: FileOpts::Generate(gen_opts.clone()),
                fresh: false,
                recipe_manifest: &recipe_manifest,
                gen_ctx: &start.gen_ctx,
                pre_erosion_setup: start.pre_erosion_setup,
                threadpool: &threadpool,
                stage_report: &|_| {},
            });

        let finalize_inputs =
            WorldSim::build_generated_world_finalize_inputs(GeneratedWorldFinalizeBuilderInputs {
                seed: TEST_WORLD_SEED,
                map_size_lg,
                gen_ctx: start.gen_ctx,
                post_erosion_chunk_inputs,
                rng: start.rng,
                calendar: None,
                compat_mode: CompatMode::Record,
                load_legacy_mode: LoadLegacyMode::Allow,
                load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode::Allow,
                compat_audit: CompatAuditV1::default(),
                managed_recipe_sidecar_missing: false,
                recipe_manifest: recipe_manifest.clone(),
                seed_elements: true,
            });

        assert!(finalize_inputs.seed_elements);
        assert_eq!(
            finalize_inputs
                .world_parts_inputs
                .recipe_manifest
                .world_recipe_hash,
            recipe_manifest.world_recipe_hash
        );
        assert_eq!(
            finalize_inputs.world_parts_inputs.map_size_lg.chunks(),
            map_size_lg.chunks()
        );
    }

    #[test]
    fn prepare_generated_world_finalize_builder_inputs_keeps_bootstrap_to_builder_boundary_single_step()
     {
        let gen_opts = GenOpts::default();
        let threadpool = ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .expect("threadpool should build");
        let recipe_manifest = RecipeManifestV1::record_only(TEST_WORLD_SEED, &gen_opts, true);
        let load_bootstrap = WorldLoadBootstrap {
            parsed_world_file: Some(
                matching_world_file()
                    .into_modern()
                    .expect("fixture should be modern"),
            ),
            map_size_lg: MapSizeLg::new(Vec2::new(gen_opts.x_lg, gen_opts.y_lg))
                .expect("default gen opts should map to valid world size"),
            gen_opts: gen_opts.clone(),
            compat_audit: CompatAuditV1::default(),
            managed_recipe_sidecar_missing: false,
            recipe_manifest: recipe_manifest.clone(),
            fresh: false,
            effective_seed: TEST_WORLD_SEED,
            effective_seed_elements: true,
        };

        let builder_inputs = WorldSim::prepare_generated_world_finalize_builder_inputs(
            GeneratedWorldFinalizeBuilderPreparationRequest {
                load_bootstrap,
                world_file: FileOpts::Generate(gen_opts),
                calendar: None,
                compat_mode: CompatMode::Record,
                load_legacy_mode: LoadLegacyMode::Allow,
                load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode::Allow,
                threadpool: &threadpool,
                stage_report: &|_| {},
            },
        );

        assert!(builder_inputs.seed_elements);
        assert_eq!(
            builder_inputs.recipe_manifest.world_recipe_hash,
            recipe_manifest.world_recipe_hash
        );
    }

    #[test]
    fn prepare_generated_world_finalize_inputs_keeps_bootstrap_to_finalize_at_single_boundary() {
        let gen_opts = GenOpts::default();
        let threadpool = ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .expect("threadpool should build");
        let recipe_manifest = RecipeManifestV1::record_only(TEST_WORLD_SEED, &gen_opts, true);
        let load_bootstrap = WorldLoadBootstrap {
            parsed_world_file: Some(
                matching_world_file()
                    .into_modern()
                    .expect("fixture should be modern"),
            ),
            map_size_lg: MapSizeLg::new(Vec2::new(gen_opts.x_lg, gen_opts.y_lg))
                .expect("default gen opts should map to valid world size"),
            gen_opts: gen_opts.clone(),
            compat_audit: CompatAuditV1::default(),
            managed_recipe_sidecar_missing: false,
            recipe_manifest: recipe_manifest.clone(),
            fresh: false,
            effective_seed: TEST_WORLD_SEED,
            effective_seed_elements: true,
        };

        let finalize_inputs = WorldSim::prepare_generated_world_finalize_inputs(
            GeneratedWorldFinalizePreparationRequest {
                load_bootstrap,
                world_file: FileOpts::Generate(gen_opts),
                calendar: None,
                compat_mode: CompatMode::Record,
                load_legacy_mode: LoadLegacyMode::Allow,
                load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode::Allow,
                threadpool: &threadpool,
                stage_report: &|_| {},
            },
        );

        assert!(finalize_inputs.seed_elements);
        assert_eq!(
            finalize_inputs
                .world_parts_inputs
                .recipe_manifest
                .world_recipe_hash,
            recipe_manifest.world_recipe_hash
        );
    }

    #[test]
    fn finalize_world_from_parts_with_postprocess_preserves_finalize_gate() {
        let gen_opts = GenOpts::default();
        let map_size_lg = MapSizeLg::new(Vec2::new(gen_opts.x_lg, gen_opts.y_lg))
            .expect("default gen opts should map to valid world size");
        let threadpool = ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .expect("threadpool should build");
        let start = WorldSim::prepare_generation_start(
            TEST_WORLD_SEED,
            map_size_lg,
            &gen_opts,
            &threadpool,
        );
        let recipe_manifest = RecipeManifestV1::record_only(TEST_WORLD_SEED, &gen_opts, false);
        let post_erosion_chunk_inputs =
            WorldSim::prepare_generation_chunk_inputs(GenerationChunkInputsRequest {
                parsed_world_file: Some(
                    matching_world_file()
                        .into_modern()
                        .expect("fixture should be modern"),
                ),
                map_size_lg,
                gen_opts: &gen_opts,
                world_file: FileOpts::Generate(gen_opts.clone()),
                fresh: false,
                recipe_manifest: &recipe_manifest,
                gen_ctx: &start.gen_ctx,
                pre_erosion_setup: start.pre_erosion_setup,
                threadpool: &threadpool,
                stage_report: &|_| {},
            });
        let parts = GeneratedWorldPartsInputs {
            seed: TEST_WORLD_SEED,
            map_size_lg,
            gen_ctx: start.gen_ctx,
            post_erosion_chunk_inputs,
            rng: start.rng,
            calendar: None,
            compat_mode: CompatMode::Record,
            load_legacy_mode: LoadLegacyMode::Allow,
            load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode::Allow,
            compat_audit: CompatAuditV1::default(),
            managed_recipe_sidecar_missing: false,
            recipe_manifest: recipe_manifest.clone(),
        }
        .into_parts();

        let world = WorldSim::finalize_world_from_parts_with_postprocess(parts, false);

        assert_eq!(
            world.recipe_manifest().world_recipe_hash,
            recipe_manifest.world_recipe_hash
        );
        assert_eq!(world.map_size_lg().chunks(), map_size_lg.chunks());
    }

    #[test]
    fn prepare_and_finalize_generated_world_keeps_prepare_finalize_boundary_single_step() {
        let gen_opts = GenOpts::default();
        let threadpool = ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .expect("threadpool should build");
        let recipe_manifest = RecipeManifestV1::record_only(TEST_WORLD_SEED, &gen_opts, true);
        let load_bootstrap = WorldLoadBootstrap {
            parsed_world_file: Some(
                matching_world_file()
                    .into_modern()
                    .expect("fixture should be modern"),
            ),
            map_size_lg: MapSizeLg::new(Vec2::new(gen_opts.x_lg, gen_opts.y_lg))
                .expect("default gen opts should map to valid world size"),
            gen_opts: gen_opts.clone(),
            compat_audit: CompatAuditV1::default(),
            managed_recipe_sidecar_missing: false,
            recipe_manifest: recipe_manifest.clone(),
            fresh: false,
            effective_seed: TEST_WORLD_SEED,
            effective_seed_elements: true,
        };

        let world = WorldSim::prepare_and_finalize_generated_world(
            GeneratedWorldFinalizePreparationRequest {
                load_bootstrap,
                world_file: FileOpts::Generate(gen_opts),
                calendar: None,
                compat_mode: CompatMode::Record,
                load_legacy_mode: LoadLegacyMode::Allow,
                load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode::Allow,
                threadpool: &threadpool,
                stage_report: &|_| {},
            },
        );

        assert_eq!(
            world.recipe_manifest().world_recipe_hash,
            recipe_manifest.world_recipe_hash
        );
        assert_eq!(world.compat_mode(), CompatMode::Record);
        assert_eq!(world.load_legacy_mode(), LoadLegacyMode::Allow);
        assert_eq!(
            world.load_or_generate_sidecarless_mode(),
            LoadOrGenerateSidecarlessMode::Allow
        );
    }

    #[test]
    fn prepare_world_load_bootstrap_keeps_load_to_bootstrap_boundary_single_step() {
        let gen_opts = GenOpts::default();
        let bootstrap = WorldSim::prepare_world_load_bootstrap(WorldLoadBootstrapRequest {
            seed: TEST_WORLD_SEED,
            seed_elements: true,
            world_file: FileOpts::Generate(gen_opts.clone()),
            compat_mode: CompatMode::Record,
            load_legacy_mode: LoadLegacyMode::Allow,
            load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode::Allow,
        })
        .expect("generate contract should remain bootstrap-able");

        assert!(bootstrap.parsed_world_file.is_none());
        assert!(bootstrap.fresh);
        assert_eq!(bootstrap.effective_seed, TEST_WORLD_SEED);
        assert_eq!(
            bootstrap.recipe_manifest.world_recipe_hash,
            RecipeManifestV1::record_only(TEST_WORLD_SEED, &gen_opts, true).world_recipe_hash
        );
    }

    #[test]
    fn prepare_world_load_bootstrap_preserves_rejection_audit_on_gate_denial() {
        let (name, temp_target) = TempLoadOrGenerateTarget::new("bootstrap-recipe-sidecar-gate");
        temp_target.write_world_file(&matching_world_file());

        let err = match WorldSim::prepare_world_load_bootstrap(WorldLoadBootstrapRequest {
            seed: TEST_WORLD_SEED,
            seed_elements: TEST_SEED_ELEMENTS,
            world_file: load_or_generate_opts(name, false),
            compat_mode: CompatMode::Record,
            load_legacy_mode: LoadLegacyMode::Allow,
            load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode::Deny,
        }) {
            Ok(_) => {
                panic!(
                    "prepare_world_load_bootstrap should preserve gate denial for sidecarless \
                     managed reuse"
                )
            },
            Err(err) => err,
        };

        match err {
            crate::Error::CompatEnforce { audit } => {
                assert!(audit.is_rejected());
                assert_eq!(audit.failure_kind, CompatFailureKindV1::PolicyDenied);
                assert_eq!(audit.failure_subject, CompatFailureSubjectV1::Options);
            },
            other => panic!("unexpected bootstrap error: {other}"),
        }
    }

    #[test]
    fn load_or_generate_legacy_world_rejects_without_overwrite() {
        let (name, temp_target) = TempLoadOrGenerateTarget::new("legacy-reject");
        let expected_path = temp_target.file_path.clone();
        temp_target.write_world_file(&legacy_world_file());

        let file_opts = load_or_generate_opts(name, false);
        assert_same_path(
            &expected_path,
            &file_opts
                .map_path()
                .expect("load_or_generate path should always exist"),
        );

        let err =
            match file_opts.load_content(CompatMode::Record, TEST_WORLD_SEED, TEST_SEED_ELEMENTS) {
                Ok(_) => panic!("legacy load_or_generate world should reject without overwrite"),
                Err(err) => err,
            };

        assert!(err.audit.is_rejected());
        assert_eq!(err.audit.failure_kind, CompatFailureKindV1::InvalidWorld);
        assert_eq!(err.audit.failure_subject, CompatFailureSubjectV1::World);
        assert!(err.audit.failure_detail.legacy_world_version);
    }

    #[test]
    fn load_or_generate_option_mismatch_rejects_without_overwrite() {
        let (name, temp_target) = TempLoadOrGenerateTarget::new("mismatch-reject");
        temp_target.write_world_file(&mismatched_world_file());

        let err = match load_or_generate_opts(name, false).load_content(
            CompatMode::Record,
            TEST_WORLD_SEED,
            TEST_SEED_ELEMENTS,
        ) {
            Ok(_) => panic!("existing world mismatch should reject when overwrite=false"),
            Err(err) => err,
        };

        assert!(err.audit.is_rejected());
        assert_eq!(err.audit.failure_kind, CompatFailureKindV1::OptionMismatch);
        assert_eq!(err.audit.failure_subject, CompatFailureSubjectV1::Options);
        assert!(err.audit.failure_detail.world_size_mismatch);
        assert!(err.audit.failure_detail.world_scale_mismatch);
    }

    #[test]
    fn load_or_generate_option_mismatch_recovers_with_overwrite() {
        let (name, temp_target) = TempLoadOrGenerateTarget::new("mismatch-overwrite");
        temp_target.write_world_file(&mismatched_world_file());

        let content = load_or_generate_opts(name, true)
            .load_content(CompatMode::Record, TEST_WORLD_SEED, TEST_SEED_ELEMENTS)
            .expect("overwrite=true mismatch should stay recoverable");

        assert!(content.parsed_world_file.is_none());
        assert_eq!(
            content.compat_audit.decision,
            CompatDecisionV1::FallbackGenerate
        );
        assert!(!content.compat_audit.is_rejected());
        assert_eq!(
            content.compat_audit.failure_subject,
            CompatFailureSubjectV1::Options
        );
        assert!(content.compat_audit.failure_detail.world_size_mismatch);
        assert!(content.compat_audit.failure_detail.world_scale_mismatch);
    }

    #[test]
    fn load_or_generate_matching_recipe_sidecar_loads_existing_world() {
        let (name, temp_target) = TempLoadOrGenerateTarget::new("recipe-match-load");
        temp_target.write_world_file(&matching_world_file());
        temp_target.write_recipe_manifest(&recipe_manifest_for_seed(TEST_WORLD_SEED));

        let content = load_or_generate_opts(name, false)
            .load_content(CompatMode::Record, TEST_WORLD_SEED, TEST_SEED_ELEMENTS)
            .expect("matching managed world recipe should load existing world");

        assert!(content.parsed_world_file.is_some());
        assert_eq!(
            content.compat_audit.decision,
            CompatDecisionV1::LoadedExisting
        );
        assert_eq!(
            content.compat_audit.failure_subject,
            CompatFailureSubjectV1::None
        );
        assert!(!content.managed_recipe_sidecar_missing);
    }

    #[test]
    fn load_or_generate_missing_recipe_sidecar_falls_back_to_legacy_option_compare() {
        let (name, temp_target) = TempLoadOrGenerateTarget::new("recipe-sidecar-missing");
        temp_target.write_world_file(&matching_world_file());

        let content = load_or_generate_opts(name, false)
            .load_content(CompatMode::Record, TEST_WORLD_SEED, TEST_SEED_ELEMENTS)
            .expect("missing recipe sidecar should still fall back to legacy option compare");

        assert!(content.parsed_world_file.is_some());
        assert_eq!(
            content.compat_audit.decision,
            CompatDecisionV1::LoadedExisting
        );
        assert_eq!(
            content.compat_audit.failure_subject,
            CompatFailureSubjectV1::None
        );
        assert!(content.managed_recipe_sidecar_missing);
    }

    #[test]
    fn load_or_generate_missing_recipe_sidecar_rejects_when_gate_is_closed() {
        let (name, temp_target) = TempLoadOrGenerateTarget::new("recipe-sidecar-gate-closed");
        temp_target.write_world_file(&matching_world_file());

        let err = match load_or_generate_opts(name, false).load_content_with_policy_modes(
            CompatMode::Record,
            LoadLegacyMode::Allow,
            LoadOrGenerateSidecarlessMode::Deny,
            TEST_WORLD_SEED,
            TEST_SEED_ELEMENTS,
        ) {
            Ok(_) => {
                panic!(
                    "sidecarless managed LoadOrGenerate should reject when its admission gate is \
                     closed"
                )
            },
            Err(err) => err,
        };

        assert!(err.audit.is_rejected());
        assert_eq!(err.audit.failure_kind, CompatFailureKindV1::PolicyDenied);
        assert_eq!(err.audit.failure_subject, CompatFailureSubjectV1::Options);
    }

    #[test]
    fn load_or_generate_recipe_mismatch_rejects_without_overwrite() {
        let (name, temp_target) = TempLoadOrGenerateTarget::new("recipe-mismatch-reject");
        temp_target.write_world_file(&matching_world_file());
        temp_target.write_recipe_manifest(&recipe_manifest_for_seed(TEST_WORLD_SEED + 1));

        let err = match load_or_generate_opts(name, false).load_content(
            CompatMode::Record,
            TEST_WORLD_SEED,
            TEST_SEED_ELEMENTS,
        ) {
            Ok(_) => panic!("managed world recipe mismatch should reject when overwrite=false"),
            Err(err) => err,
        };

        assert!(err.audit.is_rejected());
        assert_eq!(err.audit.failure_kind, CompatFailureKindV1::OptionMismatch);
        assert_eq!(err.audit.failure_subject, CompatFailureSubjectV1::Recipe);
        assert!(!err.audit.failure_detail.world_size_mismatch);
        assert!(!err.audit.failure_detail.world_scale_mismatch);
    }

    #[test]
    fn load_or_generate_recipe_mismatch_recovers_with_overwrite() {
        let (name, temp_target) = TempLoadOrGenerateTarget::new("recipe-mismatch-overwrite");
        temp_target.write_world_file(&matching_world_file());
        temp_target.write_recipe_manifest(&recipe_manifest_for_seed(TEST_WORLD_SEED + 1));

        let content = load_or_generate_opts(name, true)
            .load_content(CompatMode::Record, TEST_WORLD_SEED, TEST_SEED_ELEMENTS)
            .expect("managed world recipe mismatch should stay recoverable when overwrite=true");

        assert!(content.parsed_world_file.is_none());
        assert_eq!(
            content.compat_audit.decision,
            CompatDecisionV1::FallbackGenerate
        );
        assert!(!content.compat_audit.is_rejected());
        assert_eq!(
            content.compat_audit.failure_subject,
            CompatFailureSubjectV1::Recipe
        );
        assert!(!content.compat_audit.failure_detail.world_size_mismatch);
        assert!(!content.compat_audit.failure_detail.world_scale_mismatch);
    }

    #[test]
    fn load_or_generate_corrupt_recipe_sidecar_rejects_without_overwrite() {
        let (name, temp_target) = TempLoadOrGenerateTarget::new("recipe-sidecar-parse-reject");
        temp_target.write_world_file(&matching_world_file());
        temp_target.write_invalid_recipe_sidecar();

        let err = match load_or_generate_opts(name, false).load_content(
            CompatMode::Record,
            TEST_WORLD_SEED,
            TEST_SEED_ELEMENTS,
        ) {
            Ok(_) => {
                panic!("corrupt managed world recipe sidecar should reject when overwrite=false")
            },
            Err(err) => err,
        };

        assert!(err.audit.is_rejected());
        assert_eq!(err.audit.failure_kind, CompatFailureKindV1::ParseError);
        assert_eq!(err.audit.failure_subject, CompatFailureSubjectV1::Recipe);
    }

    #[test]
    fn load_or_generate_corrupt_recipe_sidecar_recovers_with_overwrite() {
        let (name, temp_target) = TempLoadOrGenerateTarget::new("recipe-sidecar-parse-overwrite");
        temp_target.write_world_file(&matching_world_file());
        temp_target.write_invalid_recipe_sidecar();

        let content = load_or_generate_opts(name, true)
            .load_content(CompatMode::Record, TEST_WORLD_SEED, TEST_SEED_ELEMENTS)
            .expect(
                "corrupt managed world recipe sidecar should stay recoverable when overwrite=true",
            );

        assert!(content.parsed_world_file.is_none());
        assert_eq!(
            content.compat_audit.decision,
            CompatDecisionV1::FallbackGenerate
        );
        assert!(!content.compat_audit.is_rejected());
        assert_eq!(
            content.compat_audit.failure_kind,
            CompatFailureKindV1::ParseError
        );
        assert_eq!(
            content.compat_audit.failure_subject,
            CompatFailureSubjectV1::Recipe
        );
    }
}

#[cfg(test)]
mod environment_tests {
    use super::{ApproxFallback, Path, RiverData, RiverKind, SimChunk, Way, WorldSim};
    use crate::{all::ForestKind, config::CONFIG};
    use common::{
        spot::Spot,
        terrain::{BiomeKind, TerrainChunkSize},
        vol::RectVolSize,
    };
    use vek::{Vec2, Vec3};

    fn river_data(kind: Option<RiverKind>) -> RiverData {
        RiverData {
            velocity: Vec3::zero(),
            spline_derivative: Vec2::zero(),
            river_kind: kind,
            neighbor_rivers: Vec::new(),
        }
    }

    #[test]
    fn environment_near_water_counts_freshwater_influence() {
        let lake = river_data(Some(RiverKind::Lake {
            neighbor_pass_pos: Vec2::zero(),
        }));
        assert_eq!(
            SimChunk::environment_near_water_value(
                CONFIG.sea_level + 20.0,
                CONFIG.sea_level,
                &lake
            ),
            1.0
        );

        let river = river_data(Some(RiverKind::River {
            cross_section: Vec2::one(),
        }));
        assert_eq!(
            SimChunk::environment_near_water_value(
                CONFIG.sea_level + 20.0,
                CONFIG.sea_level,
                &river,
            ),
            1.0
        );
    }

    #[test]
    fn environment_near_water_preserves_coastal_low_altitude_bias() {
        let dry_land = river_data(None);
        assert_eq!(
            SimChunk::environment_near_water_value(
                CONFIG.sea_level + 5.0,
                CONFIG.sea_level,
                &dry_land,
            ),
            1.0
        );
        assert_eq!(
            SimChunk::environment_near_water_value(
                CONFIG.sea_level + 20.0,
                CONFIG.sea_level,
                &dry_land,
            ),
            0.0
        );
    }

    fn sim_chunk_with_river(kind: Option<RiverKind>) -> SimChunk {
        SimChunk {
            chaos: 0.0,
            alt: 200.0,
            basement: 200.0,
            water_alt: CONFIG.sea_level,
            downhill: None,
            flux: 0.0,
            temp: 0.0,
            humidity: 0.5,
            rockiness: 0.0,
            tree_density: 0.5,
            forest_kind: ForestKind::Oak,
            spawn_rate: 1.0,
            river: river_data(kind),
            surface_veg: 1.0,
            sites: Vec::new(),
            place: None,
            poi: None,
            path: Default::default(),
            cliff_height: 0.0,
            spot: Option::<Spot>::None,
            contains_waypoint: false,
        }
    }

    #[test]
    fn get_biome_uses_water_body_prefix_but_keeps_river_on_land_ladder() {
        let ocean = sim_chunk_with_river(Some(RiverKind::Ocean));
        assert_eq!(ocean.get_biome(), BiomeKind::Ocean);

        let lake = sim_chunk_with_river(Some(RiverKind::Lake {
            neighbor_pass_pos: Vec2::zero(),
        }));
        assert_eq!(lake.get_biome(), BiomeKind::Lake);

        let river = sim_chunk_with_river(Some(RiverKind::River {
            cross_section: Vec2::one(),
        }));
        assert_eq!(river.get_biome(), BiomeKind::Forest);
    }

    #[test]
    fn alt_approx_or_uses_named_fallbacks_for_oob_queries() {
        let sim = WorldSim::empty();
        let oob_wpos = Vec2::new(4096, 4096);

        assert_eq!(sim.alt_approx_or(oob_wpos, ApproxFallback::Zero), 0.0);
        assert_eq!(
            sim.alt_approx_or(oob_wpos, ApproxFallback::SeaLevel),
            CONFIG.sea_level
        );
    }

    #[test]
    fn gradient_approx_or_uses_named_fallbacks_for_oob_queries() {
        let sim = WorldSim::empty();
        let oob_chunk = Vec2::new(4096, 4096);

        assert_eq!(sim.gradient_approx_or(oob_chunk, ApproxFallback::Zero), 0.0);
        assert_eq!(
            sim.gradient_approx_or(oob_chunk, ApproxFallback::SeaLevel),
            CONFIG.sea_level
        );
    }

    #[test]
    fn map_sample_alt_or_uses_named_sea_level_fallback_for_oob_queries() {
        let sim = WorldSim::empty();
        let oob_wpos = Vec2::new(4096, 4096);

        assert_eq!(
            sim.map_sample_alt_or(oob_wpos, false, true, ApproxFallback::SeaLevel),
            CONFIG.sea_level
        );
    }

    fn empty_test_sim_chunk() -> SimChunk {
        SimChunk {
            chaos: 0.0,
            alt: 0.0,
            basement: 0.0,
            water_alt: 0.0,
            downhill: None,
            flux: 0.0,
            temp: 0.0,
            humidity: 0.0,
            rockiness: 0.0,
            tree_density: 0.0,
            forest_kind: ForestKind::Dead,
            spawn_rate: 0.0,
            river: RiverData::default(),
            surface_veg: 0.0,
            sites: vec![],
            place: None,
            poi: None,
            path: Default::default(),
            cliff_height: 0.0,
            spot: None,
            contains_waypoint: false,
        }
    }

    #[test]
    fn nearest_path_queryable_gate_preserves_best_effort_border_pull() {
        let mut sim = WorldSim::empty();
        sim.chunks = vec![
            empty_test_sim_chunk(),
            empty_test_sim_chunk(),
            empty_test_sim_chunk(),
            empty_test_sim_chunk(),
        ];
        sim.get_mut(Vec2::new(0, 0)).unwrap().path = (
            Way {
                offset: Vec2::zero(),
                neighbors: 1 << 0,
            },
            Path::default(),
        );
        sim.get_mut(Vec2::new(1, 0)).unwrap().path = (
            Way {
                offset: Vec2::zero(),
                neighbors: 1 << 4,
            },
            Path::default(),
        );

        let just_left_of_world = Vec2::new(-1, TerrainChunkSize::RECT_SIZE.y as i32 / 2);
        let in_world = Vec2::new(0, TerrainChunkSize::RECT_SIZE.y as i32 / 2);

        assert!(sim.get_nearest_path(just_left_of_world).is_some());
        assert!(
            sim.get_nearest_path_if_queryable(just_left_of_world)
                .is_none()
        );
        assert!(sim.get_nearest_path(in_world).is_some());
        assert!(sim.get_nearest_path_if_queryable(in_world).is_some());
    }
}
