mod batch_generate_runtime_audit;
#[allow(dead_code)]
#[path = "world_block_statistics.rs"]
mod world_block_statistics_example;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, create_dir_all},
    io::Write,
    ops::RangeInclusive,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use clap::{Parser, Subcommand};
use common::{
    resources::MapKind,
    terrain::{
        BlockKind, CoordinateConversions, TerrainChunkSize,
        map::{MapConfig, MapSample},
        uniform_idx_as_vec2,
    },
    vol::{IntoVolIterator, RectVolSize},
};
use image::{DynamicImage, GenericImage, ImageEncoder, codecs::png::PngEncoder};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use rand::{RngExt, rng};
use rayon::ThreadPool;
use serde::{Deserialize, Serialize};
use serde_json::to_writer_pretty;
use tracing::{Level, Span, debug, error, info, info_span};
use tracing_subscriber::EnvFilter;
use vek::{Aabr, Rgb, Vec2, Vec3};
use veldr_world::{
    CONFIG, IndexOwned, IndexRef, World, WorldGenerateStage,
    sim::{FileOpts, GenOpts, WorldOpts, WorldSimStage, get_horizon_map, sample_pos, sample_wpos},
    util::Sampler,
};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    subcommand: Action,
    /// Whether .bin files should be saved for maps
    #[arg(short, long)]
    save_bin: bool,
    /// Hide progress bars
    #[arg(short, long)]
    no_progress: bool,
    /// Path to where maps are saved
    #[arg(long)]
    maps_path: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Action {
    /// Generate maps in a loop using the provided configuration
    Batch {
        /// Configuration to use for map generation
        config: String,
        /// How many maps will be generated in parallel
        #[arg(short, long)]
        threads: Option<usize>,
    },
    /// Generate a map from the .ron file emitted by the batch command
    Regenerate {
        config: String,
        /// Override erosion quality
        #[arg(long)]
        erosion_quality: Option<f32>,
    },
    /// Generate one world and emit unified preview + chunk audit artifacts
    Audit {
        /// Configuration emitted by batch/regenerate commands
        config: String,
        /// Root path for audit runs
        #[arg(long)]
        output_path: Option<PathBuf>,
        /// Override chunk sample count for chunk-side audit
        #[arg(long)]
        sample_chunks: Option<usize>,
        /// Opt into the deferred heavy world_block_statistics lane
        #[arg(long)]
        heavy_world_block_statistics: bool,
        /// Fixed chunk budget recorded for the opt-in heavy lane contract
        #[arg(long)]
        heavy_world_block_statistics_chunk_budget: Option<usize>,
    },
}

#[derive(Debug, Clone, Deserialize)]
struct BatchGenerateConfig {
    scale: RangeInclusive<f64>,
    size: (u32, u32),
    kind: MapKind,
    erosion_quality: RangeInclusive<f32>,
}

impl BatchGenerateConfig {
    fn gen_rand(&self) -> GenOpts {
        GenOpts {
            x_lg: self.size.0,
            y_lg: self.size.1,
            scale: rng().random_range(self.scale.clone()),
            map_kind: self.kind,
            erosion_quality: rng().random_range(self.erosion_quality.clone()),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct WorldBlockStatisticsAuditConfig {
    #[serde(default)]
    chunk_budget: usize,
}

impl WorldBlockStatisticsAuditConfig {
    fn is_empty(&self) -> bool { self.chunk_budget == 0 }
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct MapAuditConfig {
    #[serde(default)]
    sample_chunks: usize,
    #[serde(
        default,
        skip_serializing_if = "WorldBlockStatisticsAuditConfig::is_empty"
    )]
    world_block_statistics: WorldBlockStatisticsAuditConfig,
}

impl MapAuditConfig {
    fn is_empty(&self) -> bool { self.sample_chunks == 0 && self.world_block_statistics.is_empty() }
}

#[derive(Debug, Clone, Copy)]
struct ResolvedAuditConfig {
    sample_chunks: usize,
    heavy_world_block_statistics_chunk_budget: Option<usize>,
}

#[derive(Serialize, Deserialize, Debug)]
struct MapGenConfig {
    seed: u32,
    gen_opts: GenOpts,
    #[serde(default, skip_serializing_if = "MapAuditConfig::is_empty")]
    audit: MapAuditConfig,
}

impl MapGenConfig {
    fn resolved_audit_config(
        &self,
        sample_chunks_override: Option<usize>,
        heavy_world_block_statistics_chunk_budget_override: Option<usize>,
        heavy_world_block_statistics_requested: bool,
    ) -> ResolvedAuditConfig {
        let configured_heavy_chunk_budget = (self.audit.world_block_statistics.chunk_budget > 0)
            .then_some(self.audit.world_block_statistics.chunk_budget);
        let heavy_world_block_statistics_chunk_budget =
            heavy_world_block_statistics_chunk_budget_override
                .or(configured_heavy_chunk_budget)
                .or(heavy_world_block_statistics_requested.then_some(9))
                .map(|budget| budget.max(1));
        ResolvedAuditConfig {
            sample_chunks: sample_chunks_override
                .or(if self.audit.sample_chunks > 0 {
                    Some(self.audit.sample_chunks)
                } else {
                    None
                })
                .unwrap_or(9)
                .max(1),
            heavy_world_block_statistics_chunk_budget,
        }
    }
}

#[derive(Serialize)]
struct AuditRecipeSummary {
    world_recipe_hash: String,
    chunk_recipe_hash: String,
    topology_id: String,
    preset_id: String,
    world_alg_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    config_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    asset_hash: Option<String>,
    chunk_pass_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    static_feature_profile: Option<String>,
    seed_elements: bool,
}

#[derive(Serialize)]
struct AuditSimSummary {
    chunk_count: usize,
    alt_min: f32,
    alt_max: f32,
    alt_mean: f32,
    water_alt_min: f32,
    water_alt_max: f32,
    water_alt_mean: f32,
    river_chunks: usize,
    lake_chunks: usize,
    ocean_chunks: usize,
    near_water_chunks: usize,
    coastal_chunks: usize,
    dry_land_chunks: usize,
    surface_cave_entrance_chunks: usize,
    land_ratio: f32,
    ocean_ratio: f32,
    coast_ratio: f32,
    surface_cave_entrance_ratio: f32,
    site_chunks: usize,
    poi_chunks: usize,
    spot_kind_counts: BTreeMap<String, usize>,
    site_kind_counts: BTreeMap<String, usize>,
    mean_temp: f32,
    mean_humidity: f32,
    marine_adjacency_compare: AuditMarineAdjacencyCompareSummary,
}

#[derive(Serialize)]
struct AuditMarineAdjacencyCompareSummary {
    runtime_probe: String,
    compare_contract: String,
    compared_chunks: usize,
    skipped_runtime_probe_chunks: usize,
    static_true_runtime_true_chunks: usize,
    static_true_runtime_false_chunks: usize,
    static_false_runtime_true_chunks: usize,
    static_false_runtime_false_chunks: usize,
}

#[derive(Serialize)]
struct AuditPreviewMetrics {
    run_id: String,
    seed: u32,
    gen_opts: GenOpts,
    recipe: AuditRecipeSummary,
    world_generate_time_contract: String,
    world_generate_ms: u64,
    dimensions_lg: [u32; 2],
    chunk_dimensions: [u32; 2],
    max_height: f32,
    site_markers: usize,
    possible_starting_sites: usize,
    starting_site_profile_contract: String,
    starting_site_scoring_contract: String,
    starting_site_candidates: Vec<AuditStartingSiteCandidate>,
    poi_markers: usize,
    sim: AuditSimSummary,
}

#[derive(Serialize)]
struct AuditStartingSiteCandidate {
    rank: usize,
    selected: bool,
    profile: AuditStartingSiteProfile,
    score: AuditStartingSiteScore,
}

#[derive(Serialize)]
struct AuditStartingSiteProfile {
    site_id: u64,
    name: String,
    site_kind: String,
    center_biome: String,
    center_chunk: [i32; 2],
    plot_count: usize,
    biome_factor: f32,
}

#[derive(Serialize)]
struct AuditStartingSiteScore {
    base_kind_score: f32,
    size_score: f32,
    position_score: f32,
    biome_score: f32,
    final_score: f32,
}

#[derive(Serialize)]
struct AuditChunkStatsFile {
    run_id: String,
    seed: u32,
    gen_opts: GenOpts,
    recipe: AuditRecipeSummary,
    sample_chunks: usize,
    chunk_audit_mode: String,
    strict_determinism: bool,
    sampled_chunks: Vec<AuditChunkStats>,
}

#[derive(Serialize)]
struct AuditCompareStatus {
    schema_version: String,
    run_id: String,
    compare_mode: String,
    diff_generated: bool,
    diff_dir: String,
    baseline_ref: Option<String>,
    reason: String,
    artifacts: AuditCompareArtifacts,
    comparability: AuditCompareComparability,
    volatile_fields: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    heavy_opt_in: Option<AuditHeavyOptInStatus>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    deferred_heavy_artifacts: Vec<String>,
    notes: Vec<String>,
}

#[derive(Serialize)]
struct AuditHeavyOptInStatus {
    lane: String,
    execution_mode: String,
    input_parity_contract: String,
    comparability: String,
    world_bin: String,
    recipe_sidecar: String,
    precheck_artifact: String,
    output_artifact: String,
    baseline_artifact: String,
    requested_chunk_budget: usize,
    notes: Vec<String>,
}

#[derive(Serialize)]
struct AuditHeavyPrecheckStatus {
    schema_version: String,
    lane: String,
    execution_mode: String,
    input_parity_contract: String,
    requested_chunk_budget: usize,
    world_bin: String,
    world_bin_exists: bool,
    recipe_sidecar: String,
    recipe_sidecar_exists: bool,
    strict_reload_status: String,
    compat_entry: String,
    compat_decision: String,
    compat_failure: String,
    expected_world_recipe_hash: String,
    reloaded_world_recipe_hash: String,
    expected_chunk_recipe_hash: String,
    reloaded_chunk_recipe_hash: String,
    detail: String,
}

#[derive(Serialize)]
struct AuditCompareArtifacts {
    preview_metrics: String,
    chunk_stats: String,
    runtime_matrix: String,
    wildlife_runtime_matrix: String,
    warnings: String,
}

#[derive(Serialize)]
struct AuditCompareComparability {
    preview_metrics: String,
    chunk_stats: String,
    runtime_matrix: String,
    wildlife_runtime_matrix: String,
}

#[derive(Serialize)]
struct AuditChunkStats {
    chunk_pos: [i32; 2],
    generate_ms: u64,
    min_z: i32,
    max_z: i32,
    sub_chunks: usize,
    name: Option<String>,
    biome: String,
    alt: f32,
    tree_density: f32,
    contains_river: bool,
    near_water: bool,
    temp: f32,
    humidity: f32,
    rockiness: f32,
    cliff_height: f32,
    block_total: u64,
    non_air_blocks: u64,
    sprite_total: u64,
    block_kind_counts: BTreeMap<String, u64>,
    sprite_kind_counts: BTreeMap<String, u64>,
}

struct AuditChunkVolumeSummary {
    block_total: u64,
    non_air_blocks: u64,
    sprite_total: u64,
    block_kind_counts: BTreeMap<String, u64>,
    sprite_kind_counts: BTreeMap<String, u64>,
}

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(Level::WARN)
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let command = Cli::parse();

    let maps_path = command.maps_path.unwrap_or(PathBuf::from("maps"));

    match command.subcommand {
        Action::Batch { config, threads } => do_batch_generate(
            config,
            command.save_bin,
            threads,
            command.no_progress,
            maps_path,
        ),
        Action::Regenerate {
            config,
            erosion_quality,
        } => do_regenerate(
            config,
            maps_path,
            erosion_quality,
            command.no_progress,
            command.save_bin,
        ),
        Action::Audit {
            config,
            output_path,
            sample_chunks,
            heavy_world_block_statistics,
            heavy_world_block_statistics_chunk_budget,
        } => do_audit(
            config,
            output_path.unwrap_or_else(|| PathBuf::from("target/worldgen-audit")),
            sample_chunks,
            heavy_world_block_statistics,
            heavy_world_block_statistics_chunk_budget,
            command.no_progress,
            command.save_bin,
        ),
    }
}

fn generate_one(
    seed: u32,
    base_path: &Path,
    gen_opts: GenOpts,
    (save_bin, save_image, save_metadata): (bool, bool, bool),
    span: &Span,
    threadpool: &ThreadPool,
    progress: Option<ProgressBar>,
) -> (World, IndexOwned, u64) {
    if let Some(progress) = &progress {
        progress.set_message(seed.to_string());
    }

    let world_generate_start = Instant::now();
    let (world, index) = World::generate(
        seed,
        WorldOpts {
            seed_elements: false,
            world_file: if save_bin {
                FileOpts::Save(base_path.with_extension("bin"), gen_opts.clone())
            } else {
                FileOpts::Generate(gen_opts.clone())
            },
            calendar: None,
            compat_mode: Default::default(),
            load_legacy_mode: Default::default(),
            load_or_generate_sidecarless_mode: Default::default(),
        },
        threadpool,
        &|stage| {
            if let WorldGenerateStage::WorldSimGenerate(WorldSimStage::Erosion {
                progress: percentage,
                ..
            }) = stage
            {
                if let Some(progress) = &progress {
                    progress.set_position(percentage as u64);
                }

                span.in_scope(|| {
                    info!("Erosion progress: {percentage:02.0}%");
                })
            }
        },
    )
    .expect("batch world generation should succeed");
    let world_generate_ms = world_generate_start.elapsed().as_millis() as u64;

    if save_image
        && let Err(error) = write_preview_image(
            &world,
            index.as_index_ref(),
            &base_path.with_extension("png"),
        )
    {
        error!(?error, "Could not write preview image");
    }

    if save_metadata {
        if let Err(error) = write_map_config(&base_path.with_extension("ron"), &MapGenConfig {
            seed,
            gen_opts: gen_opts.clone(),
            audit: MapAuditConfig::default(),
        }) {
            error!(?error, "Colud not write map configuration file");
        }
    }

    info!("Finished writing map to: {}", base_path.display());
    if let Some(progress) = progress {
        progress.finish()
    }

    (world, index, world_generate_ms)
}

fn do_regenerate(
    config: String,
    maps_path: PathBuf,
    erosion_quality: Option<f32>,
    no_progress: bool,
    save_bin: bool,
) {
    let mut config: MapGenConfig =
        ron::from_str(&fs::read_to_string(config).expect("Failed to read generation file"))
            .expect("Could not parse generation file");

    let base_path = if let Some(erosion_quality) = erosion_quality {
        config.gen_opts.erosion_quality = erosion_quality;
        maps_path.join(format!("{}_{:03}", config.seed, erosion_quality * 100.0))
    } else {
        maps_path.join(config.seed.to_string())
    };

    let span = info_span!("Generating map", map = ?config);
    let pool = rayon::ThreadPoolBuilder::new().build().unwrap();

    generate_one(
        config.seed,
        &base_path,
        config.gen_opts,
        (save_bin, true, true),
        &span,
        &pool,
        (!no_progress).then(progress_bar),
    );
}

fn do_batch_generate(
    file: String,
    save_bin: bool,
    threads: Option<usize>,
    no_progress: bool,
    maps_path: PathBuf,
) {
    let config: BatchGenerateConfig =
        ron::from_str(&fs::read_to_string(file).expect("Failed to read generator config file"))
            .expect("Could not parse generator config");

    #[cfg(debug_assertions)]
    tracing::warn!("For best performance, run this in release mode");

    let threads = threads.unwrap_or(1);

    let mut handles = vec![];

    let map_i = Arc::new(AtomicUsize::new(0));
    let shutdown_started = Arc::new(std::sync::atomic::AtomicBool::new(false));

    debug!("Registering shutdown signal");
    use signal_hook::consts::signal::*;
    let _ = signal_hook::flag::register_conditional_default(SIGINT, Arc::clone(&shutdown_started));
    let _ = signal_hook::flag::register(SIGINT, Arc::clone(&shutdown_started));

    create_dir_all(&maps_path).unwrap();

    let progress_bars = (!no_progress).then(MultiProgress::new);

    for thread_id in 0..threads {
        info!(?thread_id, "Starting thread");
        let config = config.clone();
        let map_i = Arc::clone(&map_i);
        let shutdown_started = Arc::clone(&shutdown_started);
        let maps_path = maps_path.clone();
        let progress_bars = progress_bars.clone();

        let h = std::thread::spawn::<_, ()>(move || {
            loop {
                let progress = progress_bars.as_ref().map(|bars| {
                    let progress = progress_bar();
                    bars.add(progress.clone());
                    progress
                });

                if shutdown_started.load(Ordering::Relaxed) {
                    info!(?thread_id, "Shutting down thread");
                    break;
                }

                let map_i = map_i.fetch_add(1, Ordering::SeqCst);

                if let Some(progress) = &progress {
                    progress.set_prefix(format!("Map {}", map_i));
                }

                let seed = rand::rng().random::<u32>();
                let span = info_span!("generate", map_i, thread_id);
                let _guard = span.enter();
                let gen_opts = config.gen_rand();
                let base_path = maps_path.join(seed.to_string());

                let threadpool = rayon::ThreadPoolBuilder::new().build().unwrap();

                info!("Starting world generation");
                generate_one(
                    seed,
                    &base_path,
                    gen_opts,
                    (save_bin, true, true),
                    &span,
                    &threadpool,
                    progress,
                );
            }
        });

        handles.push(h);
    }

    for handle in handles {
        let _ = handle.join();
    }
}

fn do_audit(
    config: String,
    output_root: PathBuf,
    sample_chunks_override: Option<usize>,
    heavy_world_block_statistics: bool,
    heavy_world_block_statistics_chunk_budget: Option<usize>,
    no_progress: bool,
    save_bin: bool,
) {
    let mut config: MapGenConfig =
        ron::from_str(&fs::read_to_string(config).expect("Failed to read generation file"))
            .expect("Could not parse generation file");
    let heavy_world_block_statistics_requested = heavy_world_block_statistics
        || heavy_world_block_statistics_chunk_budget.is_some()
        || config.audit.world_block_statistics.chunk_budget > 0;
    let audit_config = config.resolved_audit_config(
        sample_chunks_override,
        heavy_world_block_statistics_chunk_budget,
        heavy_world_block_statistics_requested,
    );
    config.audit = MapAuditConfig {
        sample_chunks: audit_config.sample_chunks,
        world_block_statistics: WorldBlockStatisticsAuditConfig {
            chunk_budget: audit_config
                .heavy_world_block_statistics_chunk_budget
                .unwrap_or(0),
        },
    };
    let run_id = format!(
        "{}-{}",
        config.seed,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_secs()
    );
    let run_dir = output_root.join(&run_id);
    let preview_dir = run_dir.join("preview");
    let chunk_dir = run_dir.join("chunk");
    let runtime_dir = run_dir.join("runtime");
    let diff_dir = run_dir.join("compare").join("diff");
    create_dir_all(&preview_dir).expect("Could not create preview audit directory");
    create_dir_all(&chunk_dir).expect("Could not create chunk audit directory");
    create_dir_all(&runtime_dir).expect("Could not create runtime audit directory");
    create_dir_all(&diff_dir).expect("Could not create diff audit directory");

    let span = info_span!("Auditing map", run_id, map = ?config);
    let pool = rayon::ThreadPoolBuilder::new().build().unwrap();
    let base_path = run_dir.join("world");
    let save_bin_for_audit = save_bin || heavy_world_block_statistics_requested;
    let heavy_opt_in = audit_config
        .heavy_world_block_statistics_chunk_budget
        .map(|chunk_budget| build_heavy_world_block_statistics_status(chunk_budget, !save_bin));

    let (world, index, world_generate_ms) = generate_one(
        config.seed,
        &base_path,
        config.gen_opts.clone(),
        (save_bin_for_audit, false, false),
        &span,
        &pool,
        (!no_progress).then(progress_bar),
    );
    let index_ref = index.as_index_ref();
    let preview_metrics = build_preview_metrics(
        &run_id,
        &world,
        index_ref,
        &config.gen_opts,
        &pool,
        world_generate_ms,
    );
    let chunk_stats = build_chunk_stats(
        &run_id,
        &world,
        index_ref,
        &config.gen_opts,
        audit_config.sample_chunks,
    );
    let runtime_matrix = batch_generate_runtime_audit::build_runtime_chunk_matrix(
        &run_id,
        &world,
        index_ref,
        &config.gen_opts,
        audit_config.sample_chunks,
    );
    let wildlife_runtime_matrix = batch_generate_runtime_audit::build_wildlife_runtime_matrix(
        &run_id,
        &world,
        index_ref,
        &config.gen_opts,
        audit_config.sample_chunks,
    );
    if let Some(heavy_opt_in) = heavy_opt_in.as_ref() {
        write_heavy_world_block_statistics_precheck(&run_dir, &world, &pool, heavy_opt_in)
            .expect("Could not write heavy world_block_statistics precheck");
    }

    write_preview_image(&world, index_ref, &preview_dir.join("preview.png"))
        .expect("Could not write preview audit image");
    write_json(&preview_dir.join("metrics.json"), &preview_metrics)
        .expect("Could not write preview metrics");
    write_json(&chunk_dir.join("chunk_stats.json"), &chunk_stats)
        .expect("Could not write chunk stats");
    write_json(&runtime_dir.join("runtime_matrix.json"), &runtime_matrix)
        .expect("Could not write runtime chunk matrix");
    write_json(
        &runtime_dir.join("wildlife_runtime_matrix.json"),
        &wildlife_runtime_matrix,
    )
    .expect("Could not write wildlife runtime matrix");
    write_warnings_file(&run_dir.join("warnings.txt"), &world)
        .expect("Could not write warnings file");
    write_map_config(&run_dir.join("input.ron"), &config).expect("Could not write audit config");
    write_json(
        &run_dir.join("compare").join("status.json"),
        &build_compare_status(
            &run_id,
            &chunk_stats,
            &runtime_matrix,
            &wildlife_runtime_matrix,
            heavy_opt_in,
        ),
    )
    .expect("Could not write compare status");

    info!(run_dir = %run_dir.display(), "Finished writing audit artifacts");
}

#[expect(clippy::literal_string_with_formatting_args)]
fn progress_bar() -> ProgressBar {
    ProgressBar::new(100).with_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] [{eta:6}] {prefix:8} {msg:15} [{wide_bar:.red/cyan}] {percent:3}%",
        )
        .unwrap()
        .progress_chars("#>~"),
    )
}

fn write_preview_image(
    world: &World,
    index_ref: IndexRef,
    output_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let sampler = world.sim();
    let map_size_lg = sampler.map_size_lg();

    let horizons = get_horizon_map(
        map_size_lg,
        Aabr {
            min: Vec2::zero(),
            max: map_size_lg.chunks().map(|e| e as i32),
        },
        CONFIG.sea_level,
        CONFIG.sea_level + sampler.max_height,
        |posi| {
            let sample = sampler.get(uniform_idx_as_vec2(map_size_lg, posi)).unwrap();

            sample.basement.max(sample.water_alt)
        },
        |a| a,
        |h| h,
    )
    .ok();

    let mut map_config = MapConfig::orthographic(map_size_lg, 0.0..=sampler.max_height);
    map_config.horizons = horizons.as_ref();
    map_config.is_shaded = true;
    map_config.is_stylized_topo = true;
    let map = sampler.get_map(index_ref, None);

    let mut image = DynamicImage::new(
        map_size_lg.chunks().x as u32,
        map_size_lg.chunks().y as u32,
        image::ColorType::Rgba8,
    );

    map_config.generate(
        |pos| {
            let default_sample = sample_pos(&map_config, sampler, index_ref, None, pos);
            let [r, g, b, _a] = map.rgba[pos].to_le_bytes();

            MapSample {
                rgb: Rgb::new(r, g, b),
                ..default_sample
            }
        },
        |wpos| sample_wpos(&map_config, sampler, wpos),
        |pos, (r, g, b, a)| {
            image.put_pixel(
                pos.x as u32,
                map_size_lg.chunks().y as u32 - pos.y as u32 - 1,
                [r, g, b, a].into(),
            )
        },
    );

    if let Some(parent) = output_path.parent() {
        create_dir_all(parent)?;
    }

    let mut image_file = File::create(output_path)?;
    PngEncoder::new(&mut image_file).write_image(
        image.as_bytes(),
        map_size_lg.chunks().x as u32,
        map_size_lg.chunks().y as u32,
        image::ExtendedColorType::Rgba8,
    )?;
    image_file.flush()?;
    Ok(())
}

fn write_map_config(path: &Path, config: &MapGenConfig) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }

    fs::write(
        path,
        ron::ser::to_string_pretty(config, Default::default())?,
    )?;
    Ok(())
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }

    let file = File::create(path)?;
    to_writer_pretty(file, value)?;
    Ok(())
}

fn recipe_sidecar_path_for_map_path(map_path: &Path) -> PathBuf {
    let mut sidecar_path = map_path.as_os_str().to_owned();
    sidecar_path.push(".recipe.ron");
    PathBuf::from(sidecar_path)
}

fn write_heavy_world_block_statistics_precheck(
    run_dir: &Path,
    world: &World,
    threadpool: &ThreadPool,
    heavy_opt_in: &AuditHeavyOptInStatus,
) -> Result<(), Box<dyn std::error::Error>> {
    let world_bin_path = run_dir.join(&heavy_opt_in.world_bin);
    let recipe_sidecar_path = run_dir.join(&heavy_opt_in.recipe_sidecar);
    let expected_manifest = world.sim().recipe_manifest();
    let world_bin_exists = world_bin_path.is_file();
    let recipe_sidecar_exists = recipe_sidecar_path.is_file();
    if !world_bin_exists {
        return Err(format!(
            "heavy world_block_statistics parity requires '{}', but it does not exist",
            world_bin_path.display()
        )
        .into());
    }
    if !recipe_sidecar_exists {
        return Err(format!(
            "heavy world_block_statistics parity requires '{}', but it does not exist",
            recipe_sidecar_path.display()
        )
        .into());
    }

    let (reloaded_world, reloaded_index) = World::generate(
        world.sim().seed,
        WorldOpts {
            seed_elements: expected_manifest.world_recipe.seed_elements,
            world_file: FileOpts::Load(world_bin_path),
            calendar: None,
            compat_mode: Default::default(),
            load_legacy_mode: Default::default(),
            load_or_generate_sidecarless_mode: Default::default(),
        },
        threadpool,
        &|_| {},
    )?;
    let compat_audit = reloaded_world.sim().compat_audit();
    let reloaded_manifest = reloaded_world.sim().recipe_manifest();
    let strict_reload_passed = compat_audit.entry.as_str() == "load"
        && compat_audit.decision.as_str() == "loaded_existing"
        && compat_audit.failure_kind.as_str() == "none"
        && expected_manifest.world_recipe_hash == reloaded_manifest.world_recipe_hash
        && expected_manifest.chunk_recipe_hash == reloaded_manifest.chunk_recipe_hash;
    if !strict_reload_passed {
        return Err(format!(
            "heavy world_block_statistics strict reload parity failed: entry={}, decision={}, \
             failure={}, expected_world_recipe_hash={}, reloaded_world_recipe_hash={}",
            compat_audit.entry.as_str(),
            compat_audit.decision.as_str(),
            compat_audit.failure_kind.as_str(),
            expected_manifest.world_recipe_hash,
            reloaded_manifest.world_recipe_hash
        )
        .into());
    }

    world_block_statistics_example::write_bounded_statistics_artifact(
        &reloaded_world,
        reloaded_index.as_index_ref(),
        heavy_opt_in.requested_chunk_budget,
        &run_dir.join(&heavy_opt_in.output_artifact),
    )?;

    write_json(
        &run_dir.join(&heavy_opt_in.precheck_artifact),
        &AuditHeavyPrecheckStatus {
            schema_version: "world_block_statistics_precheck_v1".to_owned(),
            lane: heavy_opt_in.lane.clone(),
            execution_mode: heavy_opt_in.execution_mode.clone(),
            input_parity_contract: heavy_opt_in.input_parity_contract.clone(),
            requested_chunk_budget: heavy_opt_in.requested_chunk_budget,
            world_bin: heavy_opt_in.world_bin.clone(),
            world_bin_exists,
            recipe_sidecar: heavy_opt_in.recipe_sidecar.clone(),
            recipe_sidecar_exists,
            strict_reload_status: "passed".to_owned(),
            compat_entry: compat_audit.entry.as_str().to_owned(),
            compat_decision: compat_audit.decision.as_str().to_owned(),
            compat_failure: compat_audit.failure_kind.as_str().to_owned(),
            expected_world_recipe_hash: expected_manifest.world_recipe_hash.clone(),
            reloaded_world_recipe_hash: reloaded_manifest.world_recipe_hash.clone(),
            expected_chunk_recipe_hash: expected_manifest.chunk_recipe_hash.clone(),
            reloaded_chunk_recipe_hash: reloaded_manifest.chunk_recipe_hash.clone(),
            detail: "strict Load(path) successfully reloaded this run's saved world.bin plus \
                     adjacent RecipeManifestV1 sidecar and emitted the bounded \
                     heavy/world_block_statistics.normalized.json compare surface"
                .to_owned(),
        },
    )?;
    Ok(())
}

fn write_warnings_file(world_path: &Path, world: &World) -> Result<(), Box<dyn std::error::Error>> {
    let manifest = world.sim().recipe_manifest();
    let map_size_lg = world.sim().map_size_lg();
    let map_size = map_size_lg.vec();
    let mut warnings = vec![
        "recipe provenance and hashes are serialized into normalized audit artifacts, currently \
         participate in gating compare, and are not baseline selection keys"
            .to_string(),
        "compare normalization drops run_id fields while preview/world_generate_ms and \
         chunk/sampled_chunks[*].generate_ms remain volatile and non-gating"
            .to_string(),
        "chunk_stats.json is produced through the explicit static chunk snapshot entry; runtime \
         supplement and rtsim thinning remain outside this contract"
            .to_string(),
        "runtime/runtime_matrix.json records base no-time chunk/supplement summaries, empty/fixed \
         overlay chunk variants, fixed night/calendar full-return variants, and a dedicated \
         baseline-night rtsim thinning sample surface driven by a fixed fixture while preview \
         metrics remain raw"
            .to_string(),
    ];

    if manifest.chunk_recipe.static_feature_profile.is_none() {
        warnings.push(
            "chunk_recipe_hash is still partial until static_feature_profile is wired".to_string(),
        );
    }

    if map_size.x <= 8 || map_size.y <= 8 {
        warnings.push(format!(
            "tiny audit world (map_size_lg={}x{}) may fail civilisation bootstrap placement; this \
             is kept in warnings.txt while the default log surface suppresses it as small-map \
             audit noise",
            map_size.x, map_size.y
        ));
    }

    fs::write(world_path, warnings.join("\n") + "\n")?;
    Ok(())
}

fn build_recipe_summary(world: &World) -> AuditRecipeSummary {
    let manifest = world.sim().recipe_manifest();
    AuditRecipeSummary {
        world_recipe_hash: manifest.world_recipe_hash.clone(),
        chunk_recipe_hash: manifest.chunk_recipe_hash.clone(),
        topology_id: manifest.world_recipe.topology_id.as_str().to_owned(),
        preset_id: manifest.world_recipe.preset_id.as_str().to_owned(),
        world_alg_version: manifest.world_recipe.world_alg_version.clone(),
        config_hash: manifest.world_recipe.config_hash.clone(),
        asset_hash: manifest.world_recipe.asset_hash.clone(),
        chunk_pass_version: manifest.chunk_recipe.chunk_pass_version.clone(),
        static_feature_profile: manifest.chunk_recipe.static_feature_profile.clone(),
        seed_elements: manifest.world_recipe.seed_elements,
    }
}

fn build_preview_metrics(
    run_id: &str,
    world: &World,
    index_ref: IndexRef,
    gen_opts: &GenOpts,
    threadpool: &ThreadPool,
    world_generate_ms: u64,
) -> AuditPreviewMetrics {
    let sampler = world.sim();
    let map = world.get_map_data(index_ref, threadpool);
    let size = sampler.get_size();
    let starting_site_selection = world.starting_site_selection(index_ref);
    let selected_starting_sites = starting_site_selection
        .selected_site_ids()
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut site_kind_counts = BTreeMap::new();
    for site in world.civs().sites.values() {
        *site_kind_counts
            .entry(format!("{:?}", site.kind))
            .or_insert(0) += 1;
    }
    let starting_site_candidates = starting_site_selection
        .candidates
        .into_iter()
        .enumerate()
        .map(|(rank, candidate)| AuditStartingSiteCandidate {
            rank: rank + 1,
            selected: selected_starting_sites.contains(&candidate.profile.site_id),
            profile: AuditStartingSiteProfile {
                site_id: candidate.profile.site_id,
                name: candidate.profile.name,
                site_kind: candidate
                    .profile
                    .site_kind
                    .map(|site_kind| format!("{site_kind:?}"))
                    .unwrap_or_else(|| "None".to_owned()),
                center_biome: candidate
                    .profile
                    .center_biome
                    .map(|biome| format!("{biome:?}"))
                    .unwrap_or_else(|| "None".to_owned()),
                center_chunk: [candidate.profile.center.x, candidate.profile.center.y],
                plot_count: candidate.profile.plot_count,
                biome_factor: candidate.profile.biome_factor,
            },
            score: AuditStartingSiteScore {
                base_kind_score: candidate.score.base_kind_score,
                size_score: candidate.score.size_score,
                position_score: candidate.score.position_score,
                biome_score: candidate.score.biome_score,
                final_score: candidate.score.final_score,
            },
        })
        .collect::<Vec<_>>();

    let mut alt_min = f32::INFINITY;
    let mut alt_max = f32::NEG_INFINITY;
    let mut alt_sum = 0.0f64;
    let mut water_alt_min = f32::INFINITY;
    let mut water_alt_max = f32::NEG_INFINITY;
    let mut water_alt_sum = 0.0f64;
    let mut river_chunks = 0usize;
    let mut lake_chunks = 0usize;
    let mut ocean_chunks = 0usize;
    let mut near_water_chunks = 0usize;
    let mut coastal_chunks = 0usize;
    let mut dry_land_chunks = 0usize;
    let mut surface_cave_entrance_chunks = 0usize;
    let mut site_chunks = 0usize;
    let mut poi_chunks = 0usize;
    let mut spot_kind_counts = BTreeMap::new();
    let mut temp_sum = 0.0f64;
    let mut humidity_sum = 0.0f64;
    let mut static_true_runtime_true_chunks = 0usize;
    let mut static_true_runtime_false_chunks = 0usize;
    let mut static_false_runtime_true_chunks = 0usize;
    let mut static_false_runtime_false_chunks = 0usize;
    let mut skipped_runtime_probe_chunks = 0usize;

    for y in 0..size.y as i32 {
        for x in 0..size.x as i32 {
            let chunk_pos = Vec2::new(x, y);
            let chunk = sampler
                .get(chunk_pos)
                .expect("chunk coordinates within world bounds");
            alt_min = alt_min.min(chunk.alt);
            alt_max = alt_max.max(chunk.alt);
            alt_sum += f64::from(chunk.alt);
            water_alt_min = water_alt_min.min(chunk.water_alt);
            water_alt_max = water_alt_max.max(chunk.water_alt);
            water_alt_sum += f64::from(chunk.water_alt);
            river_chunks += usize::from(chunk.river.is_river());
            lake_chunks += usize::from(chunk.river.is_lake());
            ocean_chunks += usize::from(chunk.river.is_ocean());
            near_water_chunks += usize::from(chunk.river.near_water());
            site_chunks += usize::from(!chunk.sites.is_empty());
            poi_chunks += usize::from(chunk.poi.is_some());
            if let Some(spot) = chunk.spot {
                *spot_kind_counts.entry(format!("{spot:?}")).or_insert(0) += 1;
            }
            temp_sum += f64::from(chunk.temp);
            humidity_sum += f64::from(chunk.humidity);

            let static_marine_adjacent = static_marine_adjacent_at_chunk(sampler, chunk_pos)
                .expect("chunk coordinates within world bounds");
            coastal_chunks += usize::from(static_marine_adjacent && !chunk.river.is_ocean());
            dry_land_chunks += usize::from(!chunk.river.near_water());
            let water_occupied = chunk.river.near_water() || chunk.water_alt > chunk.alt;
            surface_cave_entrance_chunks +=
                usize::from(chunk.cliff_height > 0.0 && !water_occupied);
            let Some(runtime_marine_adjacent) =
                sample_chunk_center_column(world, index_ref, chunk_pos)
                    .map(|column| column.marine_adjacent)
            else {
                skipped_runtime_probe_chunks += 1;
                continue;
            };
            match (static_marine_adjacent, runtime_marine_adjacent) {
                (true, true) => static_true_runtime_true_chunks += 1,
                (true, false) => static_true_runtime_false_chunks += 1,
                (false, true) => static_false_runtime_true_chunks += 1,
                (false, false) => static_false_runtime_false_chunks += 1,
            }
        }
    }

    let chunk_count = size.product() as usize;
    let land_chunks = chunk_count.saturating_sub(ocean_chunks);
    AuditPreviewMetrics {
        run_id: run_id.to_owned(),
        seed: world.sim().seed,
        gen_opts: gen_opts.clone(),
        recipe: build_recipe_summary(world),
        world_generate_time_contract: "record-only wall-clock elapsed milliseconds for this audit \
                                       run's World::generate(...) call; volatile and non-gating"
            .to_owned(),
        world_generate_ms,
        dimensions_lg: [map.dimensions_lg.x, map.dimensions_lg.y],
        chunk_dimensions: [size.x, size.y],
        max_height: map.max_height,
        site_markers: map.sites.len(),
        possible_starting_sites: map.possible_starting_sites.len(),
        starting_site_profile_contract: "starting_site_profile_v2".to_owned(),
        starting_site_scoring_contract: "starting_site_scoring_v1".to_owned(),
        starting_site_candidates,
        poi_markers: map.pois.len(),
        sim: AuditSimSummary {
            chunk_count,
            alt_min,
            alt_max,
            alt_mean: (alt_sum / chunk_count as f64) as f32,
            water_alt_min,
            water_alt_max,
            water_alt_mean: (water_alt_sum / chunk_count as f64) as f32,
            river_chunks,
            lake_chunks,
            ocean_chunks,
            near_water_chunks,
            coastal_chunks,
            dry_land_chunks,
            surface_cave_entrance_chunks,
            land_ratio: land_chunks as f32 / chunk_count as f32,
            ocean_ratio: ocean_chunks as f32 / chunk_count as f32,
            coast_ratio: coastal_chunks as f32 / chunk_count as f32,
            surface_cave_entrance_ratio: surface_cave_entrance_chunks as f32 / chunk_count as f32,
            site_chunks,
            poi_chunks,
            spot_kind_counts,
            site_kind_counts,
            mean_temp: (temp_sum / chunk_count as f64) as f32,
            mean_humidity: (humidity_sum / chunk_count as f64) as f32,
            marine_adjacency_compare: AuditMarineAdjacencyCompareSummary {
                runtime_probe: "chunk_center_column_when_available_v1".to_owned(),
                compare_contract: "record static/runtime marine_adjacent handoff counts; \
                                   static_true_runtime_false_chunks should remain zero"
                    .to_owned(),
                compared_chunks: chunk_count - skipped_runtime_probe_chunks,
                skipped_runtime_probe_chunks,
                static_true_runtime_true_chunks,
                static_true_runtime_false_chunks,
                static_false_runtime_true_chunks,
                static_false_runtime_false_chunks,
            },
        },
    }
}

fn sample_chunk_center_column<'a>(
    world: &'a World,
    index_ref: IndexRef<'a>,
    chunk_pos: Vec2<i32>,
) -> Option<veldr_world::ColumnSample<'a>> {
    world
        .sample_columns()
        .get((chunk_pos.cpos_to_wpos_center(), index_ref, None))
}

fn static_marine_adjacent_at_chunk(
    sim: &veldr_world::sim::WorldSim,
    chunk_pos: Vec2<i32>,
) -> Option<bool> {
    let center_alt = sim.get(chunk_pos)?.alt;
    Some(
        (-1..=1)
            .flat_map(|x| (-1..=1).map(move |y| Vec2::new(x, y)))
            .any(|offset| {
                let check_pos = chunk_pos + offset;
                sim.get(check_pos).is_some_and(|chunk| {
                    (center_alt - chunk.alt).abs() < 200.0 && chunk.river.is_ocean()
                })
            }),
    )
}

fn build_chunk_stats(
    run_id: &str,
    world: &World,
    index_ref: IndexRef,
    gen_opts: &GenOpts,
    sample_chunks: usize,
) -> AuditChunkStatsFile {
    let sampled_chunks = sampled_chunk_positions(world.sim().get_size(), sample_chunks)
        .into_iter()
        .map(|chunk_pos| build_single_chunk_stats(world, index_ref, chunk_pos))
        .collect();

    AuditChunkStatsFile {
        run_id: run_id.to_owned(),
        seed: world.sim().seed,
        gen_opts: gen_opts.clone(),
        recipe: build_recipe_summary(world),
        sample_chunks,
        chunk_audit_mode: "sampled_static_chunk_snapshot_v1".to_owned(),
        strict_determinism: true,
        sampled_chunks,
    }
}

fn build_compare_status(
    run_id: &str,
    chunk_stats: &AuditChunkStatsFile,
    runtime_matrix: &batch_generate_runtime_audit::AuditChunkRuntimeMatrixFile,
    wildlife_runtime_matrix: &batch_generate_runtime_audit::AuditWildlifeRuntimeMatrixFile,
    heavy_opt_in: Option<AuditHeavyOptInStatus>,
) -> AuditCompareStatus {
    let mut notes = vec![
        "compare/diff remains empty during single-run audit and may be populated later by \
         external baseline verification tooling"
            .to_owned(),
        "strict chunk comparability applies to TerrainChunk-only facts after normalization \
         removes run_id and compare filtering excludes declared volatile fields"
            .to_owned(),
        "chunk_stats.json intentionally stops before runtime supplement and rtsim finalize mutate \
         the full returned-value contract"
            .to_owned(),
        "runtime/runtime_matrix.json captures the world runtime chunk path with both no-time and \
         fixed night/calendar contexts, keeps supplement summaries separate from overlay-only \
         chunk variants, and now also includes a dedicated baseline-night fixed fixture surface \
         for rtsim_resource_thinning while leaving preview metrics unchanged"
            .to_owned(),
        "preview/metrics.json now records world_generate_ms as a record-only volatile field \
         attached to this audit run's real World::generate(...) call"
            .to_owned(),
        "preview/metrics.json now records world-side starter-site selection as distinct profile \
         and score stages using the same selection path that feeds possible_starting_sites"
            .to_owned(),
        "runtime/wildlife_runtime_matrix.json captures wildlife-only runtime spawns under fixed \
         night and calendar contexts plus a static aquatic-fauna audit surface using dedicated \
         deterministic samplers"
            .to_owned(),
        "world_block_statistics remains outside the default verifier lane; when explicitly \
         requested it runs as a bounded heavy contract over the audited world's saved input \
         instead of the legacy default-asset full-world scan"
            .to_owned(),
    ];
    if heavy_opt_in.is_some() {
        notes.push(
            "heavy world_block_statistics opt-in now emits a bounded runtime chunk compare \
             surface after run-owned strict reload parity; missing \
             heavy/world_block_statistics_precheck.json or failed strict reload reports an input \
             parity problem, missing heavy/world_block_statistics.normalized.json baseline or \
             mismatches stay non-gating, and neither status changes the default verifier result"
                .to_owned(),
        );
    }

    AuditCompareStatus {
        schema_version: "worldgen_compare_status_v2".to_owned(),
        run_id: run_id.to_owned(),
        compare_mode: "single_run_only_v1".to_owned(),
        diff_generated: false,
        diff_dir: "compare/diff".to_owned(),
        baseline_ref: None,
        reason: "no baseline or previous run is attached to this audit invocation".to_owned(),
        artifacts: AuditCompareArtifacts {
            preview_metrics: "preview/metrics.json".to_owned(),
            chunk_stats: "chunk/chunk_stats.json".to_owned(),
            runtime_matrix: "runtime/runtime_matrix.json".to_owned(),
            wildlife_runtime_matrix: "runtime/wildlife_runtime_matrix.json".to_owned(),
            warnings: "warnings.txt".to_owned(),
        },
        comparability: AuditCompareComparability {
            preview_metrics: "comparable_for_future_diff".to_owned(),
            chunk_stats: if chunk_stats.strict_determinism {
                "static_chunk_strict_comparable".to_owned()
            } else {
                "sample_based_non_strict".to_owned()
            },
            runtime_matrix: if runtime_matrix.strict_determinism {
                "runtime_chunk_strict_comparable".to_owned()
            } else {
                "sample_based_non_strict".to_owned()
            },
            wildlife_runtime_matrix: if wildlife_runtime_matrix.strict_determinism {
                "runtime_chunk_strict_comparable".to_owned()
            } else {
                "sample_based_non_strict".to_owned()
            },
        },
        volatile_fields: vec![
            "preview/run_id".to_owned(),
            "preview/world_generate_ms".to_owned(),
            "chunk/run_id".to_owned(),
            "runtime/run_id".to_owned(),
            "chunk/sampled_chunks[*]/generate_ms".to_owned(),
        ],
        heavy_opt_in,
        deferred_heavy_artifacts: vec!["world_block_statistics".to_owned()],
        notes,
    }
}

fn build_heavy_world_block_statistics_status(
    requested_chunk_budget: usize,
    forced_save_bin: bool,
) -> AuditHeavyOptInStatus {
    let mut notes = vec![
        "the heavy lane is opt-in only; unified audit emits a bounded runtime chunk compare \
         surface and never falls back to the full-world default-asset scan"
            .to_owned(),
        "audit itself now performs a strict Load(path) reload over the run-owned world.bin plus \
         adjacent recipe sidecar, records the result in the precheck artifact, and writes the \
         bounded heavy/world_block_statistics.normalized.json surface from the reloaded world"
            .to_owned(),
        "the verifier compares this lane independently but keeps it non-gating for overall_status \
         because the contract is still explicit heavy opt-in"
            .to_owned(),
    ];
    if forced_save_bin {
        notes.push(
            "audit forced world.bin emission because heavy world_block_statistics parity requires \
             a strict Load(path) input instead of the default asset world"
                .to_owned(),
        );
    }

    AuditHeavyOptInStatus {
        lane: "world_block_statistics".to_owned(),
        execution_mode: "opt_in_bounded_compare_surface_v1".to_owned(),
        input_parity_contract: "strict Load(path) over this run's saved world.bin plus adjacent \
                                recipe sidecar"
            .to_owned(),
        comparability: "bounded_runtime_chunk_block_statistics_non_gating".to_owned(),
        world_bin: "world.bin".to_owned(),
        recipe_sidecar: recipe_sidecar_path_for_map_path(Path::new("world.bin"))
            .display()
            .to_string(),
        precheck_artifact: "heavy/world_block_statistics_precheck.json".to_owned(),
        output_artifact: "heavy/world_block_statistics.normalized.json".to_owned(),
        baseline_artifact: "heavy/world_block_statistics.normalized.json".to_owned(),
        requested_chunk_budget,
        notes,
    }
}

fn build_single_chunk_stats(
    world: &World,
    index_ref: IndexRef,
    chunk_pos: Vec2<i32>,
) -> AuditChunkStats {
    let start = Instant::now();
    let chunk = world
        .generate_chunk_static_snapshot(index_ref, chunk_pos, || false, None)
        .expect("audit chunk generation should succeed");
    let generate_ms = start.elapsed().as_millis() as u64;
    let meta = chunk.meta();
    let volume = summarize_chunk_volume(&chunk);

    AuditChunkStats {
        chunk_pos: [chunk_pos.x, chunk_pos.y],
        generate_ms,
        min_z: chunk.get_min_z(),
        max_z: chunk.get_max_z(),
        sub_chunks: chunk.sub_chunks_len(),
        name: meta.name().map(str::to_owned),
        biome: format!("{:?}", meta.biome()),
        alt: meta.alt(),
        tree_density: meta.tree_density(),
        contains_river: meta.contains_river(),
        near_water: meta.near_water(),
        temp: meta.temp(),
        humidity: meta.humidity(),
        rockiness: meta.rockiness(),
        cliff_height: meta.cliff_height(),
        block_total: volume.block_total,
        non_air_blocks: volume.non_air_blocks,
        sprite_total: volume.sprite_total,
        block_kind_counts: volume.block_kind_counts,
        sprite_kind_counts: volume.sprite_kind_counts,
    }
}

fn summarize_chunk_volume(chunk: &common::terrain::TerrainChunk) -> AuditChunkVolumeSummary {
    let lo = Vec3::new(0, 0, chunk.get_min_z());
    let hi = TerrainChunkSize::RECT_SIZE.as_().with_z(chunk.get_max_z());
    let mut block_total = 0u64;
    let mut non_air_blocks = 0u64;
    let mut sprite_total = 0u64;
    let mut block_kind_counts = BTreeMap::new();
    let mut sprite_kind_counts = BTreeMap::new();

    for (_, block) in chunk.vol_iter(lo, hi) {
        block_total += 1;
        *block_kind_counts
            .entry(format!("{:?}", block.kind()))
            .or_insert(0) += 1;
        if block.kind() != BlockKind::Air {
            non_air_blocks += 1;
        }
        if let Some(sprite) = block.get_sprite() {
            sprite_total += 1;
            *sprite_kind_counts
                .entry(format!("{:?}", sprite))
                .or_insert(0) += 1;
        }
    }

    AuditChunkVolumeSummary {
        block_total,
        non_air_blocks,
        sprite_total,
        block_kind_counts,
        sprite_kind_counts,
    }
}

fn sampled_chunk_positions(size: Vec2<u32>, sample_chunks: usize) -> Vec<Vec2<i32>> {
    let target = sample_chunks.max(1);
    let grid_side = (target as f64).sqrt().ceil() as usize;
    let x_bounds = interior_axis_bounds(size.x);
    let y_bounds = interior_axis_bounds(size.y);
    let mut positions = BTreeSet::new();

    for gy in 0..grid_side {
        for gx in 0..grid_side {
            if positions.len() >= target {
                break;
            }

            positions.insert((
                interpolate_axis(x_bounds, gx, grid_side),
                interpolate_axis(y_bounds, gy, grid_side),
            ));
        }
    }

    let center = (
        interpolate_axis(x_bounds, grid_side / 2, grid_side.max(1)),
        interpolate_axis(y_bounds, grid_side / 2, grid_side.max(1)),
    );
    positions.insert(center);

    positions
        .into_iter()
        .take(target)
        .map(|(x, y)| Vec2::new(x, y))
        .collect()
}

fn interior_axis_bounds(size: u32) -> (i32, i32) {
    let max = size.saturating_sub(1) as i32;
    if size > 2 { (1, max - 1) } else { (0, max) }
}

fn interpolate_axis((min, max): (i32, i32), index: usize, slots: usize) -> i32 {
    if slots <= 1 || min >= max {
        min + (max - min) / 2
    } else {
        min + ((max - min) * index as i32) / (slots as i32 - 1)
    }
}
