use clap::{Arg, Command};
use common::{
    terrain::{BlockKind, TerrainChunkSize},
    vol::{IntoVolIterator, RectVolSize},
};
use fixed::{
    FixedU8,
    types::{U8F0, U32F0, extra::U0},
};
use kiddo::{
    fixed::{distance::SquaredEuclidean, kdtree::KdTree},
    nearest_neighbour::NearestNeighbour,
};
use num_traits::identities::{One, Zero};
use rayon::{
    ThreadPoolBuilder,
    iter::{IntoParallelIterator, ParallelIterator},
};
use rusqlite::{
    Connection, ToSql, Transaction, TransactionBehavior, fallible_iterator::FallibleIterator,
};
use serde::Serialize;
use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap, HashSet},
    error::Error,
    fs::File,
    io::Write,
    ops::{Add, Mul, SubAssign},
    path::{Path, PathBuf},
    str::FromStr,
    sync::mpsc,
    time::{SystemTime, UNIX_EPOCH},
};
use vek::*;
use veloren_world::{
    IndexRef, World,
    sim::{DEFAULT_WORLD_MAP, DEFAULT_WORLD_SEED, FileOpts, WorldOpts},
};

#[derive(Serialize)]
pub struct WorldBlockStatisticsBoundedFile {
    schema_version: String,
    contract: String,
    comparability: String,
    input_contract: String,
    runtime_chunk_entry: String,
    selection_contract: String,
    strict_determinism: bool,
    requested_chunk_budget: usize,
    sampled_chunk_count: usize,
    map_size_chunks: [u32; 2],
    world_recipe_hash: String,
    chunk_recipe_hash: String,
    topology_id: String,
    chunk_pass_version: String,
    aggregate: WorldBlockStatisticsAggregate,
    sampled_chunks: Vec<WorldBlockStatisticsBoundedChunk>,
}

#[derive(Serialize)]
struct WorldBlockStatisticsAggregate {
    block_total: u64,
    non_air_blocks: u64,
    sprite_total: u64,
    block_kind_counts: BTreeMap<String, u64>,
    sprite_kind_counts: BTreeMap<String, u64>,
}

#[derive(Serialize)]
struct WorldBlockStatisticsBoundedChunk {
    selection_rank: usize,
    chunk_pos: [i32; 2],
    min_z: i32,
    max_z: i32,
    height: i32,
    block_total: u64,
    non_air_blocks: u64,
    sprite_total: u64,
    block_kind_counts: BTreeMap<String, u64>,
    sprite_kind_counts: BTreeMap<String, u64>,
}

struct ChunkVolumeSummary {
    min_z: i32,
    max_z: i32,
    height: i32,
    block_total: u64,
    non_air_blocks: u64,
    sprite_total: u64,
    block_kind_counts: BTreeMap<String, u64>,
    sprite_kind_counts: BTreeMap<String, u64>,
}

#[derive(Debug, Default, Clone, Copy, Hash, Eq, PartialEq /* , Serialize, Deserialize */)]
struct KiddoRgb(Rgb<U8F0>);

impl PartialOrd for KiddoRgb {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

impl Ord for KiddoRgb {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.0.r, self.0.g, self.0.b).cmp(&(other.0.r, other.0.g, other.0.b))
    }
}

impl Zero for KiddoRgb {
    fn zero() -> Self { KiddoRgb(Rgb::zero()) }

    fn is_zero(&self) -> bool { self == &Self::zero() }
}

impl One for KiddoRgb {
    fn one() -> Self { KiddoRgb(Rgb::one()) }

    fn is_one(&self) -> bool { self == &Self::one() }
}

impl SubAssign for KiddoRgb {
    fn sub_assign(&mut self, other: Self) {
        *self = Self(Rgb {
            r: self.0.r - other.0.r,
            g: self.0.g - other.0.g,
            b: self.0.b - other.0.b,
        });
    }
}

impl Add for KiddoRgb {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(Rgb {
            r: self.0.r + other.0.r,
            g: self.0.g + other.0.g,
            b: self.0.b + other.0.b,
        })
    }
}

impl Mul for KiddoRgb {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self {
        Self(Rgb {
            r: self.0.r * rhs.0.r,
            g: self.0.g * rhs.0.g,
            b: self.0.b * rhs.0.b,
        })
    }
}

impl From<Rgb<u8>> for KiddoRgb {
    fn from(value: Rgb<u8>) -> Self {
        Self(Rgb {
            r: FixedU8::<U0>::from_num(value.r),
            g: FixedU8::<U0>::from_num(value.g),
            b: FixedU8::<U0>::from_num(value.b),
        })
    }
}

fn block_statistics_db(db_path: &str) -> Result<Connection, Box<dyn Error>> {
    let conn = Connection::open(db_path)?;
    #[rustfmt::skip]
    conn.execute_batch("
    CREATE TABLE IF NOT EXISTS chunk (
        xcoord INTEGER NOT NULL,
        ycoord INTEGER NOT NULL,
        height INTEGER NOT NULL,
        start_time REAL NOT NULL,
        end_time REAL NOT NULL
    );
    CREATE UNIQUE INDEX IF NOT EXISTS chunk_position ON chunk(xcoord, ycoord);
    CREATE TABLE IF NOT EXISTS block (
        xcoord INTEGER NOT NULL,
        ycoord INTEGER NOT NULL,
        kind TEXT NOT NULL,
        r INTEGER NOT NULL,
        g INTEGER NOT NULL,
        b INTEGER NOT NULL,
        quantity INTEGER NOT NULL
    );
    CREATE UNIQUE INDEX IF NOT EXISTS block_position ON block(xcoord, ycoord, kind, r, g, b);
    CREATE TABLE IF NOT EXISTS sprite (
        xcoord INTEGER NOT NULL,
        ycoord INTEGER NOT NULL,
        kind TEXT NOT NULL,
        quantity INTEGER NOT NULL
    );
    CREATE UNIQUE INDEX IF NOT EXISTS sprite_position ON sprite(xcoord, ycoord, kind);
    ")?;
    Ok(conn)
}

fn write_pretty_json<T: Serialize>(output_path: &Path, value: &T) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = File::create(output_path)?;
    serde_json::to_writer_pretty(file, value)?;
    Ok(())
}

fn lattice_anchor_coordinate(min: i32, max: i32, index: usize, count: usize) -> f64 {
    if count <= 1 || min >= max {
        return f64::from(min + max) / 2.0;
    }

    let span = f64::from(max - min);
    f64::from(min) + (index as f64 * span) / ((count - 1) as f64)
}

fn bounded_chunk_positions(size: Vec2<u32>, requested_chunk_budget: usize) -> Vec<Vec2<i32>> {
    let x_min = if size.x > 1 { 1 } else { 0 };
    let y_min = if size.y > 1 { 1 } else { 0 };
    let x_max = size.x.saturating_sub(1) as i32;
    let y_max = size.y.saturating_sub(1) as i32;
    if x_min > x_max || y_min > y_max {
        return vec![];
    }

    let candidate_count = ((x_max - x_min + 1) as usize) * ((y_max - y_min + 1) as usize);
    let bounded_budget = requested_chunk_budget.max(1).min(candidate_count);
    let column_count = (bounded_budget as f64).sqrt().ceil() as usize;
    let row_count = bounded_budget.div_ceil(column_count);
    let mut selected = vec![];
    let mut selected_keys = HashSet::new();

    for slot in 0..bounded_budget {
        let row = slot / column_count;
        let column = slot % column_count;
        let anchor_x = lattice_anchor_coordinate(x_min, x_max, column, column_count);
        let anchor_y = lattice_anchor_coordinate(y_min, y_max, row, row_count);
        let mut best_chunk = None;
        let mut best_distance = i64::MAX;

        for y in y_min..=y_max {
            for x in x_min..=x_max {
                if selected_keys.contains(&(x, y)) {
                    continue;
                }

                let dx = f64::from(x) - anchor_x;
                let dy = f64::from(y) - anchor_y;
                let distance = (dx * dx + dy * dy) * 1_000_000.0;
                let distance_key = distance.round() as i64;
                let candidate = (distance_key, y, x);
                let current_best =
                    best_chunk.map(|chunk: Vec2<i32>| (best_distance, chunk.y, chunk.x));
                if current_best.is_none_or(|best| candidate < best) {
                    best_distance = distance_key;
                    best_chunk = Some(Vec2::new(x, y));
                }
            }
        }

        if let Some(chunk_pos) = best_chunk {
            selected_keys.insert((chunk_pos.x, chunk_pos.y));
            selected.push(chunk_pos);
        }
    }

    if selected.len() < bounded_budget {
        for y in y_min..=y_max {
            for x in x_min..=x_max {
                if selected_keys.insert((x, y)) {
                    selected.push(Vec2::new(x, y));
                    if selected.len() == bounded_budget {
                        return selected;
                    }
                }
            }
        }
    }

    selected
}

fn summarize_runtime_chunk(
    world: &World,
    index_ref: IndexRef,
    chunk_pos: Vec2<i32>,
) -> Result<ChunkVolumeSummary, Box<dyn Error>> {
    let (chunk, _supplement) = world
        .generate_chunk(index_ref, chunk_pos, None, || false, None)
        .map_err(|()| format!("runtime chunk generation failed at {:?}", chunk_pos))?;
    let mut block_kind_counts = BTreeMap::new();
    let mut sprite_kind_counts = BTreeMap::new();
    let mut block_total = 0;
    let mut non_air_blocks = 0;
    let mut sprite_total = 0;
    let lo = Vec3::new(0, 0, chunk.get_min_z());
    let hi = TerrainChunkSize::RECT_SIZE.as_().with_z(chunk.get_max_z());

    for (_, block) in chunk.vol_iter(lo, hi) {
        block_total += 1;
        if block.kind() != BlockKind::Air {
            non_air_blocks += 1;
        }
        *block_kind_counts
            .entry(format!("{:?}", block.kind()))
            .or_insert(0) += 1;
        if let Some(sprite) = block.get_sprite() {
            sprite_total += 1;
            *sprite_kind_counts
                .entry(format!("{:?}", sprite))
                .or_insert(0) += 1;
        }
    }

    Ok(ChunkVolumeSummary {
        min_z: chunk.get_min_z(),
        max_z: chunk.get_max_z(),
        height: chunk.get_max_z() - chunk.get_min_z(),
        block_total,
        non_air_blocks,
        sprite_total,
        block_kind_counts,
        sprite_kind_counts,
    })
}

pub fn build_bounded_statistics_file(
    world: &World,
    index_ref: IndexRef,
    requested_chunk_budget: usize,
) -> Result<WorldBlockStatisticsBoundedFile, Box<dyn Error>> {
    let size = world.sim().get_size();
    let sampled_positions = bounded_chunk_positions(size, requested_chunk_budget);
    let mut aggregate_block_total = 0;
    let mut aggregate_non_air_blocks = 0;
    let mut aggregate_sprite_total = 0;
    let mut aggregate_block_kind_counts = BTreeMap::new();
    let mut aggregate_sprite_kind_counts = BTreeMap::new();
    let mut sampled_chunks = Vec::with_capacity(sampled_positions.len());

    for (selection_rank, chunk_pos) in sampled_positions.into_iter().enumerate() {
        let volume = summarize_runtime_chunk(world, index_ref, chunk_pos)?;
        aggregate_block_total += volume.block_total;
        aggregate_non_air_blocks += volume.non_air_blocks;
        aggregate_sprite_total += volume.sprite_total;
        for (kind, count) in &volume.block_kind_counts {
            *aggregate_block_kind_counts.entry(kind.clone()).or_insert(0) += *count;
        }
        for (kind, count) in &volume.sprite_kind_counts {
            *aggregate_sprite_kind_counts
                .entry(kind.clone())
                .or_insert(0) += *count;
        }

        sampled_chunks.push(WorldBlockStatisticsBoundedChunk {
            selection_rank: selection_rank + 1,
            chunk_pos: [chunk_pos.x, chunk_pos.y],
            min_z: volume.min_z,
            max_z: volume.max_z,
            height: volume.height,
            block_total: volume.block_total,
            non_air_blocks: volume.non_air_blocks,
            sprite_total: volume.sprite_total,
            block_kind_counts: volume.block_kind_counts,
            sprite_kind_counts: volume.sprite_kind_counts,
        });
    }

    let manifest = world.sim().recipe_manifest();
    Ok(WorldBlockStatisticsBoundedFile {
        schema_version: "world_block_statistics_bounded_v1".to_owned(),
        contract: "bounded_runtime_chunk_block_statistics_v1".to_owned(),
        comparability: "bounded_runtime_chunk_block_statistics_non_gating".to_owned(),
        input_contract: "strict Load(path) over saved world.bin plus adjacent RecipeManifestV1 \
                         sidecar"
            .to_owned(),
        runtime_chunk_entry: "world.generate_chunk(rtsim_resource_fractions=None,time=None,\
                              calendar=None)"
            .to_owned(),
        selection_contract: "deterministic best-effort interior lattice over runtime chunk \
                             coordinates bounded by requested_chunk_budget"
            .to_owned(),
        strict_determinism: true,
        requested_chunk_budget,
        sampled_chunk_count: sampled_chunks.len(),
        map_size_chunks: [size.x, size.y],
        world_recipe_hash: manifest.world_recipe_hash.clone(),
        chunk_recipe_hash: manifest.chunk_recipe_hash.clone(),
        topology_id: manifest.world_recipe.topology_id.as_str().to_owned(),
        chunk_pass_version: manifest.chunk_recipe.chunk_pass_version.clone(),
        aggregate: WorldBlockStatisticsAggregate {
            block_total: aggregate_block_total,
            non_air_blocks: aggregate_non_air_blocks,
            sprite_total: aggregate_sprite_total,
            block_kind_counts: aggregate_block_kind_counts,
            sprite_kind_counts: aggregate_sprite_kind_counts,
        },
        sampled_chunks,
    })
}

pub fn write_bounded_statistics_artifact(
    world: &World,
    index_ref: IndexRef,
    requested_chunk_budget: usize,
    output_path: &Path,
) -> Result<(), Box<dyn Error>> {
    let file = build_bounded_statistics_file(world, index_ref, requested_chunk_budget)?;
    write_pretty_json(output_path, &file)
}

fn write_bounded_statistics_from_world_file(
    world_file: FileOpts,
    requested_chunk_budget: usize,
    output_path: &Path,
) -> Result<(), Box<dyn Error>> {
    let pool = ThreadPoolBuilder::new().build().unwrap();
    let (world, index) = World::generate(
        DEFAULT_WORLD_SEED,
        WorldOpts {
            seed_elements: true,
            world_file,
            calendar: None,
            compat_mode: Default::default(),
            load_legacy_mode: Default::default(),
            load_or_generate_sidecarless_mode: Default::default(),
        },
        &pool,
        &|_| {},
    )?;
    write_bounded_statistics_artifact(
        &world,
        index.as_index_ref(),
        requested_chunk_budget,
        output_path,
    )
}

fn generate(
    db_path: &str,
    world_file: FileOpts,
    ymin: Option<i32>,
    ymax: Option<i32>,
) -> Result<(), Box<dyn Error>> {
    common_frontend::init_stdout(None);
    println!("Loading world");
    let pool = ThreadPoolBuilder::new().build().unwrap();
    let (world, index) = World::generate(
        DEFAULT_WORLD_SEED,
        WorldOpts {
            seed_elements: true,
            world_file,
            calendar: None,
            compat_mode: Default::default(),
            load_legacy_mode: Default::default(),
            load_or_generate_sidecarless_mode: Default::default(),
        },
        &pool,
        &|_| {},
    )
    .expect("world block statistics world should load");
    println!("Loaded world");

    let conn = block_statistics_db(db_path)?;

    let existing_chunks: HashSet<(i32, i32)> = conn
        .prepare("SELECT xcoord, ycoord FROM chunk")?
        .query([])?
        .map(|row| Ok((row.get(0)?, row.get(1)?)))
        .collect()?;

    let sz = world.sim().get_size();
    let (tx, rx) = mpsc::channel();
    rayon::spawn(move || {
        let coords: Vec<_> = (ymin.unwrap_or(1)..ymax.unwrap_or(sz.y as i32))
            .flat_map(move |y| {
                let tx = tx.clone();
                (1..sz.x as i32).map(move |x| (tx.clone(), x, y))
            })
            .collect();
        coords.into_par_iter().for_each(|(tx, x, y)| {
            if existing_chunks.contains(&(x, y)) {
                return;
            }
            let start_time = SystemTime::now();
            if let Ok((chunk, _supplement)) =
                world.generate_chunk(index.as_index_ref(), Vec2::new(x, y), None, || false, None)
            {
                let end_time = SystemTime::now();
                // TODO: The KiddoRgb wrapper type is necessary to satisfy trait bounds.
                // We store the colors twice currently, once as coordinates and another time
                // as Content. Kiddo version 6.x is supposed to add the ability to have
                // Content be (), which would be useful here. Once that's added, do that.
                // TODO: dist_sq is the same type as the coordinates, and since squared
                // euclidean distances between colors go way higher than 255,
                // we're using a U32F0 here instead of the optimal U8F0 (A U16F0
                // works too, but it could theoretically still overflow so U32F0
                // is used to be safe). Kiddo version 6.x will change this — once that
                // releases, replace U32F0 with U8F0.
                let mut block_colors: KdTree<U32F0, KiddoRgb, 3, 32, u32> = KdTree::new();
                let mut block_counts = HashMap::new();
                let mut sprite_counts = HashMap::new();
                let lo = Vec3::new(0, 0, chunk.get_min_z());
                let hi = TerrainChunkSize::RECT_SIZE.as_().with_z(chunk.get_max_z());
                let height = chunk.get_max_z() - chunk.get_min_z();
                for (_, block) in chunk.vol_iter(lo, hi) {
                    let mut rgb =
                        KiddoRgb::from(block.get_color().unwrap_or_else(|| Rgb::new(0, 0, 0)));
                    let color: [U32F0; 3] = [rgb.0.r.into(), rgb.0.g.into(), rgb.0.b.into()];
                    let NearestNeighbour {
                        distance: dist_sq,
                        item: nearest,
                    } = block_colors.nearest_one::<SquaredEuclidean>(&color);
                    if dist_sq < 5_u32.pow(2) {
                        rgb = nearest;
                    } else {
                        block_colors.add(&color, rgb);
                    }
                    *block_counts.entry((block.kind(), rgb)).or_insert(0) += 1;
                    if let Some(sprite) = block.get_sprite() {
                        *sprite_counts.entry(sprite).or_insert(0) += 1;
                    }
                }
                let _ = tx.send((
                    x,
                    y,
                    height,
                    start_time,
                    end_time,
                    block_counts,
                    sprite_counts,
                ));
            }
        });
    });
    let mut tx = Transaction::new_unchecked(&conn, TransactionBehavior::Deferred)?;
    let mut i = 0;
    let mut j = 0;
    while let Ok((x, y, height, start_time, end_time, block_counts, sprite_counts)) = rx.recv() {
        #[rustfmt::skip]
        let mut insert_block = tx.prepare_cached("
            REPLACE INTO block (xcoord, ycoord, kind, r, g, b, quantity)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ")?;
        #[rustfmt::skip]
        let mut insert_sprite = tx.prepare_cached("
            REPLACE INTO sprite (xcoord, ycoord, kind, quantity)
            VALUES (?1, ?2, ?3, ?4)
        ")?;
        #[rustfmt::skip]
        let mut insert_chunk = tx.prepare_cached("
            REPLACE INTO chunk (xcoord, ycoord, height, start_time, end_time)
            VALUES (?1, ?2, ?3, ?4, ?5)
        ")?;
        for ((kind, color), count) in block_counts.iter() {
            insert_block.execute([
                &x as &dyn ToSql,
                &y,
                &format!("{:?}", kind),
                &color.0.r.to_num::<u8>(),
                &color.0.g.to_num::<u8>(),
                &color.0.b.to_num::<u8>(),
                &count,
            ])?;
        }
        for (kind, count) in sprite_counts.iter() {
            insert_sprite.execute([&x as &dyn ToSql, &y, &format!("{:?}", kind), &count])?;
        }
        let start_time = start_time.duration_since(UNIX_EPOCH)?.as_secs_f64();
        let end_time = end_time.duration_since(UNIX_EPOCH)?.as_secs_f64();
        insert_chunk.execute([&x as &dyn ToSql, &y, &height, &start_time, &end_time])?;
        if i % 32 == 0 {
            println!("Committing hunk of 32 chunks: {}", j);
            drop(insert_block);
            drop(insert_sprite);
            drop(insert_chunk);
            tx.commit()?;
            tx = Transaction::new_unchecked(&conn, TransactionBehavior::Deferred)?;
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

fn palette(conn: Connection) -> Result<(), Box<dyn Error>> {
    let mut stmt =
        conn.prepare("SELECT kind, r, g, b, SUM(quantity) FROM block GROUP BY kind, r, g, b")?;
    let mut block_colors: HashMap<BlockKind, Vec<(KiddoRgb, i64)>> = HashMap::new();

    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let kind = BlockKind::from_str(&row.get::<_, String>(0)?)?;
        let rgb: KiddoRgb = KiddoRgb::from(Rgb::new(row.get(1)?, row.get(2)?, row.get(3)?));
        let count: i64 = row.get(4)?;
        block_colors.entry(kind).or_default().push((rgb, count));
    }
    for (_, v) in block_colors.iter_mut() {
        v.sort_by(|a, b| b.1.cmp(&a.1));
    }

    let mut palettes: HashMap<BlockKind, Vec<KiddoRgb>> = HashMap::new();
    for (kind, colors) in block_colors.iter() {
        let palette = palettes.entry(*kind).or_default();
        if colors.len() <= 256 {
            for (color, _) in colors {
                palette.push(*color);
            }
            println!("{:?}: {:?}", kind, palette);
            continue;
        }
        let mut radius = 1024.0;
        let mut tree: KdTree<U32F0, KiddoRgb, 3, 256, u32> = KdTree::new();
        while palette.len() < 256 {
            if let Some((color, _)) = colors.iter().find(|(color, _)| {
                tree.nearest_one::<SquaredEuclidean>(&[
                    color.0.r.into(),
                    color.0.g.into(),
                    color.0.b.into(),
                ])
                .distance
                    > radius
            }) {
                palette.push(*color);
                tree.add(
                    &[color.0.r.into(), color.0.g.into(), color.0.b.into()],
                    *color,
                );
                println!("{:?}, {:?}: {:?}", kind, radius, *color);
            } else {
                radius -= 1.0;
            }
        }
    }
    let palettes: HashMap<BlockKind, Vec<Rgb<u8>>> = palettes
        .iter()
        .map(|(k, v)| {
            (
                *k,
                v.iter()
                    .map(|c| Rgb {
                        r: c.0.r.to_num::<u8>(),
                        g: c.0.g.to_num::<u8>(),
                        b: c.0.b.to_num::<u8>(),
                    })
                    .collect(),
            )
        })
        .collect();
    let mut f = File::create("palettes.ron")?;
    let pretty = ron::ser::PrettyConfig::default().depth_limit(2);
    write!(f, "{}", ron::ser::to_string_pretty(&palettes, pretty)?)?;
    Ok(())
}

fn resolve_world_file(matches: &clap::ArgMatches) -> FileOpts {
    if let Some(world_path) = matches.get_one::<String>("world_path") {
        FileOpts::Load(PathBuf::from(world_path))
    } else {
        FileOpts::LoadAsset(
            matches
                .get_one::<String>("world_asset")
                .cloned()
                .unwrap_or_else(|| DEFAULT_WORLD_MAP.to_owned()),
        )
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut app = Command::new("world_block_statistics")
        .version(common::util::DISPLAY_VERSION.as_str())
        .author("The Caldrayne contributors <https://github.com/wanli149/Caldrayne>")
        .about("Compute and process block statistics on generated chunks")
        .subcommand(
            Command::new("generate")
                .about("Generate block statistics")
                .args(&[
                    Arg::new("database")
                        .required(true)
                        .help("File to generate/resume generation"),
                    Arg::new("world_path")
                        .long("world-path")
                        .help(
                            "Strict world.bin path to load; requires an adjacent recipe sidecar \
                             and is intended for unified audit parity",
                        )
                        .conflicts_with("world_asset"),
                    Arg::new("world_asset").long("world-asset").help(
                        "Asset specifier to load when no strict world path is provided; defaults \
                         to the built-in default world asset",
                    ),
                    Arg::new("ymin")
                        .long("ymin")
                        .value_parser(clap::value_parser!(i32)),
                    Arg::new("ymax")
                        .long("ymax")
                        .value_parser(clap::value_parser!(i32)),
                ]),
        )
        .subcommand(
            Command::new("bounded")
                .about("Emit bounded normalized world_block_statistics summary")
                .args(&[
                    Arg::new("output")
                        .required(true)
                        .help("Path to write heavy/world_block_statistics.normalized.json"),
                    Arg::new("chunk_budget")
                        .long("chunk-budget")
                        .required(true)
                        .value_parser(clap::value_parser!(usize))
                        .help("Bounded runtime chunk budget for the summary surface"),
                    Arg::new("world_path")
                        .long("world-path")
                        .help(
                            "Strict world.bin path to load; requires an adjacent recipe sidecar \
                             and is intended for unified audit parity",
                        )
                        .conflicts_with("world_asset"),
                    Arg::new("world_asset").long("world-asset").help(
                        "Asset specifier to load when no strict world path is provided; defaults \
                         to the built-in default world asset",
                    ),
                ]),
        )
        .subcommand(
            Command::new("palette")
                .about("Compute a palette from previously gathered statistics")
                .args(&[Arg::new("database").required(true)]),
        );

    let matches = app.clone().get_matches();
    match matches.subcommand() {
        Some(("generate", matches)) => {
            let db_path = matches
                .get_one::<String>("database")
                .expect("database is required");
            let world_file = resolve_world_file(matches);
            let ymin = matches.get_one::<i32>("ymin").cloned();
            let ymax = matches.get_one::<i32>("ymax").cloned();
            generate(db_path, world_file, ymin, ymax)?;
        },
        Some(("bounded", matches)) => {
            let world_file = resolve_world_file(matches);
            let output_path = PathBuf::from(
                matches
                    .get_one::<String>("output")
                    .expect("output is required"),
            );
            let chunk_budget = *matches
                .get_one::<usize>("chunk_budget")
                .expect("chunk_budget is required");
            write_bounded_statistics_from_world_file(world_file, chunk_budget, &output_path)?;
        },
        Some(("palette", matches)) => {
            let conn = Connection::open(
                matches
                    .get_one::<String>("database")
                    .expect("database is required"),
            )?;
            palette(conn)?;
        },
        _ => {
            app.print_help()?;
        },
    }
    Ok(())
}
