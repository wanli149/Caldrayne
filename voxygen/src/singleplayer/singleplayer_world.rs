use std::{
    fs,
    io::{BufReader, Read},
    path::{Path, PathBuf},
};

use common::{assets::ASSETS_PATH, consts::DAY_LENGTH_DEFAULT, uuid::Uuid};
use serde::{Deserialize, Serialize};
use server::{
    CompatAuditV1, CompatDecisionV1, DEFAULT_WORLD_MAP, DEFAULT_WORLD_SEED, FileOpts, GenOpts,
    RecipeManifestV1, TopologyId,
};
use tracing::error;

const SINGLEPLAYER_META_SCHEMA_VERSION: u64 = 2;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SingleplayerWorldSource {
    LegacyUnknown,
    LegacyMigrated,
    Generated,
    DefaultAsset,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SingleplayerLegacyOrigin {
    LoadPath(String),
    LoadLegacyPath(String),
    LoadAsset(String),
    LoadOrGenerate { name: String, overwrite: bool },
}

#[derive(Clone)]
pub struct SingleplayerWorld {
    pub world_id: Uuid,
    pub realm_id: Uuid,
    pub name: String,
    pub gen_opts: Option<GenOpts>,
    pub day_length: f64,
    pub seed: u32,
    pub world_source: SingleplayerWorldSource,
    pub source_ref: Option<String>,
    pub legacy_origin: Option<SingleplayerLegacyOrigin>,
    pub compat_audit: Option<CompatAuditV1>,
    pub managed_recipe_sidecar_missing: bool,
    pub world_recipe_hash: Option<String>,
    pub topology_id: Option<String>,
    pub is_generated: bool,
    pub path: PathBuf,
    pub map_path: PathBuf,
}

impl SingleplayerWorld {
    pub fn copy_default_world(&self) {
        if let Err(e) = fs::copy(asset_path(DEFAULT_WORLD_MAP), &self.map_path) {
            println!("Error when trying to copy default world: {e}");
        }
    }

    /// Updates pending metadata for an unmaterialized world selection. This is
    /// only a local preview and must be replaced by the actual runtime contract
    /// once the server has finished loading the world.
    pub fn refresh_pending_source_contract(&mut self) {
        if self.is_generated {
            return;
        }

        self.compat_audit = None;
        self.managed_recipe_sidecar_missing = false;

        if let Some(gen_opts) = &self.gen_opts {
            let recipe_manifest = RecipeManifestV1::record_only(self.seed, gen_opts, true);
            self.world_source = SingleplayerWorldSource::Generated;
            self.source_ref = None;
            self.legacy_origin = None;
            self.world_recipe_hash = Some(recipe_manifest.world_recipe_hash);
            self.topology_id = Some(recipe_manifest.world_recipe.topology_id.as_str().to_owned());
        } else {
            self.world_source = SingleplayerWorldSource::DefaultAsset;
            self.source_ref = Some(DEFAULT_WORLD_MAP.to_string());
            self.legacy_origin = None;
            self.world_recipe_hash = None;
            self.topology_id = Some(TopologyId::BoundedPlaneV1.as_str().to_owned());
        }
    }

    pub fn sync_runtime_source_contract(
        &mut self,
        compat_audit: CompatAuditV1,
        recipe_manifest: &RecipeManifestV1,
        managed_recipe_sidecar_missing: bool,
    ) {
        self.is_generated = fs::metadata(&self.map_path).is_ok_and(|f| f.is_file());
        self.compat_audit = Some(compat_audit);
        self.managed_recipe_sidecar_missing = managed_recipe_sidecar_missing;

        match compat_audit.decision {
            CompatDecisionV1::LoadedExisting => match self.world_source {
                SingleplayerWorldSource::Generated => {
                    self.source_ref = None;
                    self.legacy_origin = None;
                    self.world_recipe_hash = Some(recipe_manifest.world_recipe_hash.clone());
                    self.topology_id =
                        Some(recipe_manifest.world_recipe.topology_id.as_str().to_owned());
                },
                SingleplayerWorldSource::DefaultAsset => {
                    self.source_ref = Some(
                        self.source_ref
                            .clone()
                            .unwrap_or_else(|| DEFAULT_WORLD_MAP.to_string()),
                    );
                    self.legacy_origin = None;
                    self.world_recipe_hash = None;
                    self.topology_id =
                        Some(recipe_manifest.world_recipe.topology_id.as_str().to_owned());
                },
                SingleplayerWorldSource::LegacyUnknown
                | SingleplayerWorldSource::LegacyMigrated => {
                    // Keep legacy provenance explicit until later migration
                    // work can validate and rewrite those
                    // contracts with a dedicated path.
                },
            },
            CompatDecisionV1::GenerateRequested | CompatDecisionV1::FallbackGenerate => {
                self.world_source = SingleplayerWorldSource::Generated;
                self.source_ref = None;
                self.legacy_origin = None;
                self.gen_opts = Some(recipe_manifest.world_recipe.gen_opts.clone());
                self.world_recipe_hash = Some(recipe_manifest.world_recipe_hash.clone());
                self.topology_id =
                    Some(recipe_manifest.world_recipe.topology_id.as_str().to_owned());
            },
        }
    }

    /// Best-effort metadata writeback for startup failures after the world may
    /// already have been materialized on disk. This keeps persisted provenance
    /// aligned with the runtime path we can still prove, without inventing a
    /// runtime recipe manifest that the failed startup never surfaced.
    pub fn sync_runtime_failure_source_contract(&mut self, compat_audit: Option<CompatAuditV1>) {
        self.is_generated = fs::metadata(&self.map_path).is_ok_and(|f| f.is_file());
        self.compat_audit = compat_audit;
        self.managed_recipe_sidecar_missing = false;

        match compat_audit {
            Some(audit) if matches!(audit.decision, CompatDecisionV1::LoadedExisting) => {
                match self.world_source {
                    SingleplayerWorldSource::Generated => {
                        self.source_ref = None;
                        self.legacy_origin = None;
                    },
                    SingleplayerWorldSource::DefaultAsset => {
                        self.source_ref = Some(
                            self.source_ref
                                .clone()
                                .unwrap_or_else(|| DEFAULT_WORLD_MAP.to_string()),
                        );
                        self.legacy_origin = None;
                    },
                    SingleplayerWorldSource::LegacyUnknown
                    | SingleplayerWorldSource::LegacyMigrated => {},
                }
            },
            Some(audit)
                if matches!(
                    audit.decision,
                    CompatDecisionV1::GenerateRequested | CompatDecisionV1::FallbackGenerate
                ) && !audit.is_rejected()
                    && self.is_generated =>
            {
                self.world_source = SingleplayerWorldSource::Generated;
                self.source_ref = None;
                self.legacy_origin = None;
                if self.gen_opts.is_none() {
                    self.topology_id = None;
                }
                if !matches!(self.compat_audit, Some(audit) if audit.decision == CompatDecisionV1::GenerateRequested)
                {
                    self.world_recipe_hash = None;
                }
            },
            _ => {},
        }
    }

    pub fn persist_meta(&self) { write_world_meta(self); }

    pub fn runtime_file_opts(&self) -> FileOpts {
        if let Some(gen_opts) = &self.gen_opts
            && !self.is_generated
        {
            return FileOpts::Save(self.map_path.clone(), gen_opts.clone());
        }

        match &self.world_source {
            SingleplayerWorldSource::Generated => FileOpts::Load(self.map_path.clone()),
            SingleplayerWorldSource::DefaultAsset => FileOpts::LoadAsset(
                self.source_ref
                    .clone()
                    .unwrap_or_else(|| DEFAULT_WORLD_MAP.to_string()),
            ),
            SingleplayerWorldSource::LegacyUnknown | SingleplayerWorldSource::LegacyMigrated => {
                FileOpts::LoadLegacy(self.map_path.clone())
            },
        }
    }
}

fn new_singleplayer_identity() -> (Uuid, Uuid) {
    let world_id = Uuid::new_v4();
    (world_id, world_id)
}

fn load_map(path: &Path) -> Option<SingleplayerWorld> {
    let meta_path = path.join("meta.ron");

    let Ok(f) = fs::File::open(&meta_path) else {
        error!("Failed to open {}", meta_path.to_string_lossy());
        return None;
    };

    let f = BufReader::new(f);

    let Ok(bytes) = f.bytes().collect::<Result<Vec<u8>, _>>() else {
        error!("Failed to read {}", meta_path.to_string_lossy());
        return None;
    };

    let load_result = version::try_load(std::io::Cursor::new(bytes), path)?;
    if load_result.needs_upgrade {
        write_world_meta(&load_result.world);
    }
    Some(load_result.world)
}

fn write_world_meta(world: &SingleplayerWorld) {
    let path = &world.path;

    if let Err(e) = fs::create_dir_all(path) {
        error!("Failed to create world folder: {e}");
    }

    match fs::File::create(path.join("meta.ron")) {
        Ok(file) => {
            if let Err(e) = ron::options::Options::default().to_io_writer_pretty(
                file,
                &version::Current::from_world(world),
                ron::ser::PrettyConfig::new(),
            ) {
                error!("Failed to create world meta file: {e}")
            }
        },
        Err(e) => error!("Failed to create world meta file: {e}"),
    }
}

fn asset_path(asset: &str) -> PathBuf {
    let mut s = asset.replace('.', "/");
    s.push_str(".bin");
    ASSETS_PATH.join(s)
}

fn legacy_load_or_generate_map_path(name: &str) -> PathBuf {
    // Mirrors the historical `FileOpts::LoadOrGenerate` path resolution used
    // before singleplayer metadata migration.
    Path::new("./maps").join(format!("{name}.bin"))
}

fn migrate_old_singleplayer(from: &Path, to: &Path) {
    if fs::metadata(from).is_ok_and(|meta| meta.is_dir()) {
        if let Err(e) = fs::rename(from, to) {
            error!("Failed to migrate singleplayer: {e}");
            return;
        }

        let mut seed = DEFAULT_WORLD_SEED;
        let mut day_length = DAY_LENGTH_DEFAULT;
        let (map_file, gen_opts, world_source, source_ref, legacy_origin) =
            fs::read_to_string(to.join("server_config/settings.ron"))
                .ok()
                .and_then(|settings| {
                    let settings: server::Settings = ron::from_str(&settings).ok()?;
                    seed = settings.world_seed;
                    day_length = settings.day_length;
                    Some(match settings.map_file? {
                        FileOpts::LoadOrGenerate {
                            name,
                            opts,
                            overwrite,
                        } => (
                            Some(legacy_load_or_generate_map_path(&name)),
                            Some(opts),
                            SingleplayerWorldSource::LegacyMigrated,
                            None,
                            Some(SingleplayerLegacyOrigin::LoadOrGenerate { name, overwrite }),
                        ),
                        FileOpts::Generate(opts) => (
                            None,
                            Some(opts),
                            SingleplayerWorldSource::Generated,
                            None,
                            None,
                        ),
                        FileOpts::LoadLegacy(path) => (
                            Some(path.clone()),
                            None,
                            SingleplayerWorldSource::LegacyMigrated,
                            None,
                            Some(SingleplayerLegacyOrigin::LoadLegacyPath(
                                path.to_string_lossy().into_owned(),
                            )),
                        ),
                        FileOpts::Load(path) => (
                            Some(path.clone()),
                            None,
                            SingleplayerWorldSource::LegacyMigrated,
                            None,
                            Some(SingleplayerLegacyOrigin::LoadPath(
                                path.to_string_lossy().into_owned(),
                            )),
                        ),
                        FileOpts::LoadAsset(asset) if asset == DEFAULT_WORLD_MAP => (
                            Some(asset_path(&asset)),
                            None,
                            SingleplayerWorldSource::DefaultAsset,
                            Some(asset),
                            None,
                        ),
                        FileOpts::LoadAsset(asset) => (
                            Some(asset_path(&asset)),
                            None,
                            SingleplayerWorldSource::LegacyMigrated,
                            None,
                            Some(SingleplayerLegacyOrigin::LoadAsset(asset)),
                        ),
                        FileOpts::Save(_, gen_opts) => (
                            None,
                            Some(gen_opts),
                            SingleplayerWorldSource::Generated,
                            None,
                            None,
                        ),
                    })
                })
                .unwrap_or((
                    Some(asset_path(DEFAULT_WORLD_MAP)),
                    None,
                    SingleplayerWorldSource::DefaultAsset,
                    Some(DEFAULT_WORLD_MAP.to_string()),
                    None,
                ));

        let map_path = to.join("map.bin");
        if let Some(map_file) = map_file
            && let Err(err) = fs::copy(map_file, &map_path)
        {
            error!("Failed to copy map file to singleplayer world: {err}");
        }
        let is_generated = fs::metadata(&map_path).is_ok_and(|f| f.is_file());

        let (world_id, realm_id) = new_singleplayer_identity();
        let mut world = SingleplayerWorld {
            world_id,
            realm_id,
            name: "singleplayer world".to_string(),
            gen_opts,
            seed,
            day_length,
            world_source,
            source_ref,
            legacy_origin,
            compat_audit: None,
            managed_recipe_sidecar_missing: false,
            world_recipe_hash: None,
            topology_id: None,
            path: to.to_path_buf(),
            is_generated,
            map_path,
        };
        if matches!(world.world_source, SingleplayerWorldSource::Generated) {
            world.refresh_pending_source_contract();
            world.is_generated = is_generated;
        } else if matches!(world.world_source, SingleplayerWorldSource::DefaultAsset) {
            world.topology_id = Some(TopologyId::BoundedPlaneV1.as_str().to_owned());
        }
        write_world_meta(&world);
    }
}

fn load_worlds(path: &Path) -> Vec<SingleplayerWorld> {
    let Ok(paths) = fs::read_dir(path) else {
        let _ = fs::create_dir_all(path);
        return Vec::new();
    };

    paths
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if entry.file_type().ok()?.is_dir() {
                let path = entry.path();
                load_map(&path)
            } else {
                None
            }
        })
        .collect()
}

#[derive(Default)]
pub struct SingleplayerWorlds {
    pub worlds: Vec<SingleplayerWorld>,
    pub current: Option<usize>,
    worlds_folder: PathBuf,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SingleplayerLegacyInventory {
    pub total_worlds: usize,
    pub legacy_worlds: usize,
    pub legacy_unknown_worlds: usize,
    pub legacy_migrated_worlds: usize,
    pub legacy_worlds_with_typed_origin: usize,
    pub legacy_worlds_without_typed_origin: usize,
    pub legacy_worlds_without_compat_audit: usize,
    pub legacy_worlds_with_sidecarless_managed_residual: usize,
}

impl SingleplayerWorlds {
    pub fn load(userdata_folder: &Path) -> SingleplayerWorlds {
        let worlds_folder = userdata_folder.join("singleplayer_worlds");

        if let Err(e) = fs::create_dir_all(&worlds_folder) {
            error!("Failed to create singleplayer worlds folder: {e}");
        }

        migrate_old_singleplayer(
            &userdata_folder.join("singleplayer"),
            &worlds_folder.join("singleplayer"),
        );

        let worlds = load_worlds(&worlds_folder);

        SingleplayerWorlds {
            worlds,
            current: None,
            worlds_folder,
        }
    }

    pub fn delete_map_file(&mut self, map: usize) {
        let w = &mut self.worlds[map];
        if w.is_generated {
            // We don't care about the result here since we aren't sure the file exists.
            let _ = fs::remove_file(&w.map_path);
        }
        w.is_generated = false;
        w.refresh_pending_source_contract();
    }

    pub fn remove(&mut self, idx: usize) {
        if let Some(ref mut i) = self.current {
            match (*i).cmp(&idx) {
                std::cmp::Ordering::Less => {},
                std::cmp::Ordering::Equal => self.current = None,
                std::cmp::Ordering::Greater => *i -= 1,
            }
        }
        let _ = fs::remove_dir_all(&self.worlds[idx].path);
        self.worlds.remove(idx);
    }

    fn world_folder_name(&self) -> String {
        use chrono::{Datelike, Timelike};
        let now = chrono::Local::now().naive_local();
        let name = format!(
            "world-{}-{}-{}-{}_{}_{}_{}",
            now.year(),
            now.month(),
            now.day(),
            now.hour(),
            now.minute(),
            now.second(),
            now.and_utc().timestamp_subsec_millis() /* .and_utc() necessary, as other fn is
                                                     * deprecated */
        );

        let mut test_name = name.clone();
        let mut i = 0;
        'fail: loop {
            for world in self.worlds.iter() {
                if world.path.ends_with(&test_name) {
                    test_name.clone_from(&name);
                    test_name.push('_');
                    test_name.push_str(&i.to_string());
                    i += 1;
                    continue 'fail;
                }
            }
            break;
        }
        test_name
    }

    pub fn current(&self) -> Option<&SingleplayerWorld> {
        self.current.and_then(|i| self.worlds.get(i))
    }

    pub fn legacy_inventory(&self) -> SingleplayerLegacyInventory {
        let mut inventory = SingleplayerLegacyInventory {
            total_worlds: self.worlds.len(),
            ..SingleplayerLegacyInventory::default()
        };

        for world in &self.worlds {
            if !matches!(
                world.world_source,
                SingleplayerWorldSource::LegacyUnknown | SingleplayerWorldSource::LegacyMigrated
            ) {
                continue;
            }

            inventory.legacy_worlds += 1;
            if matches!(world.world_source, SingleplayerWorldSource::LegacyUnknown) {
                inventory.legacy_unknown_worlds += 1;
            } else {
                inventory.legacy_migrated_worlds += 1;
            }

            if world.legacy_origin.is_some() {
                inventory.legacy_worlds_with_typed_origin += 1;
            } else {
                inventory.legacy_worlds_without_typed_origin += 1;
            }

            if world.compat_audit.is_none() {
                inventory.legacy_worlds_without_compat_audit += 1;
            }

            if world.managed_recipe_sidecar_missing {
                inventory.legacy_worlds_with_sidecarless_managed_residual += 1;
            }
        }

        inventory
    }

    pub fn new_world(&mut self) {
        let folder_name = self.world_folder_name();
        let path = self.worlds_folder.join(folder_name);

        let (world_id, realm_id) = new_singleplayer_identity();
        let new_world = SingleplayerWorld {
            world_id,
            realm_id,
            name: "New World".to_string(),
            gen_opts: None,
            day_length: DAY_LENGTH_DEFAULT,
            seed: DEFAULT_WORLD_SEED,
            world_source: SingleplayerWorldSource::DefaultAsset,
            source_ref: Some(DEFAULT_WORLD_MAP.to_string()),
            legacy_origin: None,
            compat_audit: None,
            managed_recipe_sidecar_missing: false,
            world_recipe_hash: None,
            topology_id: Some(TopologyId::BoundedPlaneV1.as_str().to_owned()),
            is_generated: false,
            map_path: path.join("map.bin"),
            path,
        };

        write_world_meta(&new_world);

        self.worlds.push(new_world)
    }

    pub fn save_current_meta(&self) {
        if let Some(world) = self.current() {
            write_world_meta(world);
        }
    }

    pub fn save_world_meta(&self, idx: usize) { write_world_meta(&self.worlds[idx]); }
}

mod version {
    use std::any::{Any, type_name};

    use serde::de::DeserializeOwned;

    use super::*;

    pub type Current = V5;

    pub struct LoadResult {
        pub world: SingleplayerWorld,
        pub needs_upgrade: bool,
    }

    type LoadWorldFn<R> = fn(R, &Path) -> Result<LoadResult, (&'static str, ron::de::SpannedError)>;
    fn loaders<'a, R: std::io::Read + Clone>() -> &'a [LoadWorldFn<R>] {
        // Step [5]
        &[
            load_raw::<V5, _>,
            load_raw::<V4, _>,
            load_raw::<V3, _>,
            load_raw::<V2, _>,
            load_raw::<V1, _>,
        ]
    }

    #[derive(Deserialize, Serialize)]
    pub struct V1 {
        #[serde(deserialize_with = "version::<_, 1>")]
        version: u64,
        name: String,
        gen_opts: Option<GenOpts>,
        seed: u32,
    }

    impl ToWorld for V1 {
        fn to_world(self, path: PathBuf) -> LoadResult {
            let map_path = path.join("map.bin");
            let is_generated = fs::metadata(&map_path).is_ok_and(|f| f.is_file());
            let (world_id, realm_id) = new_singleplayer_identity();

            LoadResult {
                world: SingleplayerWorld {
                    world_id,
                    realm_id,
                    name: self.name,
                    gen_opts: self.gen_opts,
                    seed: self.seed,
                    day_length: DAY_LENGTH_DEFAULT,
                    world_source: SingleplayerWorldSource::LegacyUnknown,
                    source_ref: None,
                    legacy_origin: None,
                    compat_audit: None,
                    managed_recipe_sidecar_missing: false,
                    world_recipe_hash: None,
                    topology_id: None,
                    is_generated,
                    path,
                    map_path,
                },
                needs_upgrade: true,
            }
        }
    }

    #[derive(Deserialize, Serialize)]
    pub struct V2 {
        #[serde(deserialize_with = "version::<_, 2>")]
        version: u64,
        name: String,
        gen_opts: Option<GenOpts>,
        seed: u32,
        day_length: f64,
    }

    impl ToWorld for V2 {
        fn to_world(self, path: PathBuf) -> LoadResult {
            let map_path = path.join("map.bin");
            let is_generated = fs::metadata(&map_path).is_ok_and(|f| f.is_file());
            let (world_id, realm_id) = new_singleplayer_identity();

            LoadResult {
                world: SingleplayerWorld {
                    world_id,
                    realm_id,
                    name: self.name,
                    gen_opts: self.gen_opts,
                    seed: self.seed,
                    day_length: self.day_length,
                    world_source: SingleplayerWorldSource::LegacyUnknown,
                    source_ref: None,
                    legacy_origin: None,
                    compat_audit: None,
                    managed_recipe_sidecar_missing: false,
                    world_recipe_hash: None,
                    topology_id: None,
                    is_generated,
                    path,
                    map_path,
                },
                needs_upgrade: true,
            }
        }
    }

    #[derive(Deserialize, Serialize)]
    pub struct V3 {
        #[serde(deserialize_with = "version::<_, 3>")]
        version: u64,
        world_id: Uuid,
        realm_id: Uuid,
        name: String,
        gen_opts: Option<GenOpts>,
        seed: u32,
        day_length: f64,
    }

    #[derive(Deserialize, Serialize)]
    pub struct V4 {
        #[serde(deserialize_with = "version::<_, 4>")]
        version: u64,
        schema_version: u64,
        world_id: Uuid,
        realm_id: Uuid,
        name: String,
        gen_opts: Option<GenOpts>,
        seed: u32,
        day_length: f64,
        world_source: SingleplayerWorldSource,
        source_ref: Option<String>,
        world_recipe_hash: Option<String>,
        topology_id: Option<String>,
    }

    #[derive(Deserialize, Serialize)]
    pub struct V5 {
        #[serde(deserialize_with = "version::<_, 5>")]
        version: u64,
        #[serde(deserialize_with = "schema_version")]
        schema_version: u64,
        world_id: Uuid,
        realm_id: Uuid,
        name: String,
        gen_opts: Option<GenOpts>,
        seed: u32,
        day_length: f64,
        world_source: SingleplayerWorldSource,
        source_ref: Option<String>,
        #[serde(default)]
        legacy_origin: Option<SingleplayerLegacyOrigin>,
        #[serde(default)]
        compat_audit: Option<CompatAuditV1>,
        #[serde(default)]
        managed_recipe_sidecar_missing: bool,
        world_recipe_hash: Option<String>,
        topology_id: Option<String>,
    }

    impl V5 {
        pub fn from_world(world: &SingleplayerWorld) -> Self {
            V5 {
                version: 5,
                schema_version: SINGLEPLAYER_META_SCHEMA_VERSION,
                world_id: world.world_id,
                realm_id: world.realm_id,
                name: world.name.clone(),
                gen_opts: world.gen_opts.clone(),
                seed: world.seed,
                day_length: world.day_length,
                world_source: world.world_source.clone(),
                source_ref: world.source_ref.clone(),
                legacy_origin: world.legacy_origin.clone(),
                compat_audit: world.compat_audit,
                managed_recipe_sidecar_missing: world.managed_recipe_sidecar_missing,
                world_recipe_hash: world.world_recipe_hash.clone(),
                topology_id: world.topology_id.clone(),
            }
        }
    }

    impl ToWorld for V5 {
        fn to_world(self, path: PathBuf) -> LoadResult {
            let map_path = path.join("map.bin");
            let is_generated = fs::metadata(&map_path).is_ok_and(|f| f.is_file());

            LoadResult {
                world: SingleplayerWorld {
                    world_id: self.world_id,
                    realm_id: self.realm_id,
                    name: self.name,
                    gen_opts: self.gen_opts,
                    seed: self.seed,
                    day_length: self.day_length,
                    world_source: self.world_source,
                    source_ref: self.source_ref,
                    legacy_origin: self.legacy_origin,
                    compat_audit: self.compat_audit,
                    managed_recipe_sidecar_missing: self.managed_recipe_sidecar_missing,
                    world_recipe_hash: self.world_recipe_hash,
                    topology_id: self.topology_id,
                    is_generated,
                    path,
                    map_path,
                },
                needs_upgrade: false,
            }
        }
    }

    impl ToWorld for V4 {
        fn to_world(self, path: PathBuf) -> LoadResult {
            let map_path = path.join("map.bin");
            let is_generated = fs::metadata(&map_path).is_ok_and(|f| f.is_file());

            LoadResult {
                world: SingleplayerWorld {
                    world_id: self.world_id,
                    realm_id: self.realm_id,
                    name: self.name,
                    gen_opts: self.gen_opts,
                    seed: self.seed,
                    day_length: self.day_length,
                    world_source: self.world_source,
                    source_ref: self.source_ref,
                    legacy_origin: None,
                    compat_audit: None,
                    managed_recipe_sidecar_missing: false,
                    world_recipe_hash: self.world_recipe_hash,
                    topology_id: self.topology_id,
                    is_generated,
                    path,
                    map_path,
                },
                needs_upgrade: true,
            }
        }
    }

    impl ToWorld for V3 {
        fn to_world(self, path: PathBuf) -> LoadResult {
            let map_path = path.join("map.bin");
            let is_generated = fs::metadata(&map_path).is_ok_and(|f| f.is_file());

            LoadResult {
                world: SingleplayerWorld {
                    world_id: self.world_id,
                    realm_id: self.realm_id,
                    name: self.name,
                    gen_opts: self.gen_opts,
                    seed: self.seed,
                    day_length: self.day_length,
                    world_source: SingleplayerWorldSource::LegacyUnknown,
                    source_ref: None,
                    legacy_origin: None,
                    compat_audit: None,
                    managed_recipe_sidecar_missing: false,
                    world_recipe_hash: None,
                    topology_id: None,
                    is_generated,
                    path,
                    map_path,
                },
                needs_upgrade: true,
            }
        }
    }

    // Utilities
    fn version<'de, D: serde::Deserializer<'de>, const V: u64>(de: D) -> Result<u64, D::Error> {
        u64::deserialize(de).and_then(|x| {
            if x == V {
                Ok(x)
            } else {
                Err(serde::de::Error::invalid_value(
                    serde::de::Unexpected::Unsigned(x),
                    &"incorrect magic/version bytes",
                ))
            }
        })
    }

    fn schema_version<'de, D: serde::Deserializer<'de>>(de: D) -> Result<u64, D::Error> {
        u64::deserialize(de).and_then(|x| {
            if x == SINGLEPLAYER_META_SCHEMA_VERSION {
                Ok(x)
            } else {
                Err(serde::de::Error::invalid_value(
                    serde::de::Unexpected::Unsigned(x),
                    &"incorrect singleplayer meta schema version",
                ))
            }
        })
    }

    trait ToWorld {
        fn to_world(self, path: PathBuf) -> LoadResult;
    }

    fn load_raw<RawWorld: Any + ToWorld + DeserializeOwned, R: std::io::Read + Clone>(
        reader: R,
        path: &Path,
    ) -> Result<LoadResult, (&'static str, ron::de::SpannedError)> {
        ron::de::from_reader::<_, RawWorld>(reader)
            .map(|s| s.to_world(path.to_path_buf()))
            .map_err(|e| (type_name::<RawWorld>(), e))
    }

    pub fn try_load<R: std::io::Read + Clone>(reader: R, path: &Path) -> Option<LoadResult> {
        loaders()
            .iter()
            .find_map(|load_raw| match load_raw(reader.clone(), path) {
                Ok(chunk) => Some(chunk),
                Err((raw_name, e)) => {
                    error!(
                        "Attempt to load chunk with raw format `{}` failed: {:?}",
                        raw_name, e
                    );
                    None
                },
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use world::recipe::{CompatFailureDetailV1, CompatFailureSubjectV1};

    fn legacy_unknown_runtime_world(world_dir: &Path) -> SingleplayerWorld {
        SingleplayerWorld {
            world_id: Uuid::new_v4(),
            realm_id: Uuid::new_v4(),
            name: "Legacy Unknown".to_string(),
            gen_opts: None,
            day_length: DAY_LENGTH_DEFAULT,
            seed: DEFAULT_WORLD_SEED,
            world_source: SingleplayerWorldSource::LegacyUnknown,
            source_ref: None,
            legacy_origin: None,
            compat_audit: None,
            managed_recipe_sidecar_missing: false,
            world_recipe_hash: None,
            topology_id: None,
            is_generated: true,
            path: world_dir.to_path_buf(),
            map_path: world_dir.join("map.bin"),
        }
    }

    fn inventory_world(
        world_dir: &Path,
        world_source: SingleplayerWorldSource,
        legacy_origin: Option<SingleplayerLegacyOrigin>,
        compat_audit: Option<CompatAuditV1>,
    ) -> SingleplayerWorld {
        SingleplayerWorld {
            world_id: Uuid::new_v4(),
            realm_id: Uuid::new_v4(),
            name: "Inventory World".to_string(),
            gen_opts: None,
            day_length: DAY_LENGTH_DEFAULT,
            seed: DEFAULT_WORLD_SEED,
            world_source,
            source_ref: None,
            legacy_origin,
            compat_audit,
            managed_recipe_sidecar_missing: false,
            world_recipe_hash: None,
            topology_id: None,
            is_generated: true,
            path: world_dir.to_path_buf(),
            map_path: world_dir.join("map.bin"),
        }
    }

    #[test]
    fn loading_v1_world_upgrades_to_v5_and_persists_legacy_unknown() {
        let world_dir =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-world-{}", Uuid::new_v4()));
        fs::create_dir_all(&world_dir).unwrap();

        let legacy_meta = format!(
            "(\n    version: 1,\n    name: \"Legacy World\",\n    gen_opts: None,\n    seed: \
             {},\n)\n",
            DEFAULT_WORLD_SEED
        );
        fs::write(world_dir.join("meta.ron"), legacy_meta).unwrap();

        let first = load_map(&world_dir).expect("legacy world should load");
        let second = load_map(&world_dir).expect("upgraded world should load");

        assert_eq!(first.world_id, second.world_id);
        assert_eq!(first.realm_id, second.realm_id);
        assert!(!first.world_id.is_nil());
        assert!(!first.realm_id.is_nil());
        assert!(matches!(
            first.world_source,
            SingleplayerWorldSource::LegacyUnknown
        ));
        assert_eq!(first.source_ref, None);
        assert_eq!(first.legacy_origin, None);

        let meta = fs::read_to_string(world_dir.join("meta.ron")).unwrap();
        assert!(meta.contains("version: 5"));
        assert!(meta.contains("schema_version: 2"));
        assert!(meta.contains("world_source: legacy_unknown"));
        assert!(meta.contains("compat_audit: None"));

        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn legacy_inventory_counts_residual_legacy_stock_truthfully() {
        let root =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-world-{}", Uuid::new_v4()));
        let worlds = SingleplayerWorlds {
            worlds: vec![
                inventory_world(
                    &root.join("legacy-unknown"),
                    SingleplayerWorldSource::LegacyUnknown,
                    None,
                    None,
                ),
                inventory_world(
                    &root.join("legacy-migrated-typed"),
                    SingleplayerWorldSource::LegacyMigrated,
                    Some(SingleplayerLegacyOrigin::LoadPath(
                        root.join("typed-source.bin").to_string_lossy().into_owned(),
                    )),
                    Some(CompatAuditV1::loaded_existing(
                        server::CompatEntryKindV1::LoadLegacy,
                    )),
                ),
                inventory_world(
                    &root.join("legacy-migrated-untyped"),
                    SingleplayerWorldSource::LegacyMigrated,
                    None,
                    None,
                ),
                {
                    let mut world = inventory_world(
                        &root.join("legacy-migrated-sidecarless"),
                        SingleplayerWorldSource::LegacyMigrated,
                        Some(SingleplayerLegacyOrigin::LoadOrGenerate {
                            name: "managed-sidecarless".to_owned(),
                            overwrite: false,
                        }),
                        Some(CompatAuditV1::loaded_existing(
                            server::CompatEntryKindV1::LoadLegacy,
                        )),
                    );
                    world.managed_recipe_sidecar_missing = true;
                    world
                },
                inventory_world(
                    &root.join("generated"),
                    SingleplayerWorldSource::Generated,
                    None,
                    Some(CompatAuditV1::loaded_existing(
                        server::CompatEntryKindV1::Load,
                    )),
                ),
                inventory_world(
                    &root.join("default-asset"),
                    SingleplayerWorldSource::DefaultAsset,
                    None,
                    None,
                ),
            ],
            current: None,
            worlds_folder: root.clone(),
        };

        let inventory = worlds.legacy_inventory();

        assert_eq!(inventory, SingleplayerLegacyInventory {
            total_worlds: 6,
            legacy_worlds: 4,
            legacy_unknown_worlds: 1,
            legacy_migrated_worlds: 3,
            legacy_worlds_with_typed_origin: 2,
            legacy_worlds_without_typed_origin: 2,
            legacy_worlds_without_compat_audit: 2,
            legacy_worlds_with_sidecarless_managed_residual: 1,
        });

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn loading_v2_world_upgrades_to_v5_and_persists_ids() {
        let world_dir =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-world-{}", Uuid::new_v4()));
        fs::create_dir_all(&world_dir).unwrap();

        let legacy_meta = format!(
            "(\n    version: 2,\n    name: \"Legacy World\",\n    gen_opts: None,\n    seed: \
             {},\n    day_length: {},\n)\n",
            DEFAULT_WORLD_SEED, DAY_LENGTH_DEFAULT
        );
        fs::write(world_dir.join("meta.ron"), legacy_meta).unwrap();

        let first = load_map(&world_dir).expect("legacy world should load");
        let second = load_map(&world_dir).expect("upgraded world should load");

        assert_eq!(first.world_id, second.world_id);
        assert_eq!(first.realm_id, second.realm_id);
        assert!(!first.world_id.is_nil());
        assert!(!first.realm_id.is_nil());

        let meta = fs::read_to_string(world_dir.join("meta.ron")).unwrap();
        assert!(meta.contains("version: 5"));
        assert!(meta.contains("schema_version: 2"));
        assert!(meta.contains("world_id"));
        assert!(meta.contains("realm_id"));
        assert!(meta.contains("world_source: legacy_unknown"));
        assert!(meta.contains("compat_audit: None"));

        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn loading_v3_world_upgrades_to_v5_and_persists_legacy_unknown() {
        let world_dir =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-world-{}", Uuid::new_v4()));
        fs::create_dir_all(&world_dir).unwrap();

        let world_id = Uuid::new_v4();
        let realm_id = Uuid::new_v4();
        let legacy_meta = format!(
            "(\n    version: 3,\n    world_id: \"{}\",\n    realm_id: \"{}\",\n    name: \"Legacy \
             V3\",\n    gen_opts: None,\n    seed: {},\n    day_length: {},\n)\n",
            world_id, realm_id, DEFAULT_WORLD_SEED, DAY_LENGTH_DEFAULT
        );
        fs::write(world_dir.join("meta.ron"), legacy_meta).unwrap();

        let loaded = load_map(&world_dir).expect("legacy world should load");
        let meta = fs::read_to_string(world_dir.join("meta.ron")).unwrap();

        assert_eq!(loaded.world_id, world_id);
        assert_eq!(loaded.realm_id, realm_id);
        assert!(matches!(
            loaded.world_source,
            SingleplayerWorldSource::LegacyUnknown
        ));
        assert_eq!(loaded.source_ref, None);
        assert_eq!(loaded.legacy_origin, None);
        assert!(meta.contains("version: 5"));
        assert!(meta.contains("schema_version: 2"));
        assert!(meta.contains("world_source: legacy_unknown"));
        assert!(meta.contains("compat_audit: None"));

        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn loading_v4_world_upgrades_to_v5_and_backfills_empty_compat_audit() {
        let world_dir =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-world-{}", Uuid::new_v4()));
        fs::create_dir_all(&world_dir).unwrap();

        let world_id = Uuid::new_v4();
        let realm_id = Uuid::new_v4();
        let legacy_meta = format!(
            "(\n    version: 4,\n    schema_version: 1,\n    world_id: \"{}\",\n    realm_id: \
             \"{}\",\n    name: \"Legacy V4\",\n    gen_opts: None,\n    seed: {},\n    \
             day_length: {},\n    world_source: default_asset,\n    source_ref: Some(\"{}\"),\n    \
             world_recipe_hash: None,\n    topology_id: Some(\"{}\"),\n)\n",
            world_id,
            realm_id,
            DEFAULT_WORLD_SEED,
            DAY_LENGTH_DEFAULT,
            DEFAULT_WORLD_MAP,
            TopologyId::BoundedPlaneV1.as_str(),
        );
        fs::write(world_dir.join("meta.ron"), legacy_meta).unwrap();

        let loaded = load_map(&world_dir).expect("legacy v4 world should load");
        let meta = fs::read_to_string(world_dir.join("meta.ron")).unwrap();

        assert_eq!(loaded.world_id, world_id);
        assert_eq!(loaded.realm_id, realm_id);
        assert!(matches!(
            loaded.world_source,
            SingleplayerWorldSource::DefaultAsset
        ));
        assert_eq!(loaded.compat_audit, None);
        assert!(meta.contains("version: 5"));
        assert!(meta.contains("schema_version: 2"));
        assert!(meta.contains("compat_audit: None"));

        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn migrating_old_singleplayer_preserves_load_legacy_worlds() {
        let root =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-migrate-{}", Uuid::new_v4()));
        let old_world = root.join("singleplayer");
        let migrated_world = root.join("singleplayer_worlds").join("singleplayer");
        fs::create_dir_all(old_world.join("server_config")).unwrap();
        fs::create_dir_all(root.join("singleplayer_worlds")).unwrap();

        let legacy_map_path = root.join("legacy-source-world.bin");
        let legacy_map_bytes = b"legacy-world-map";
        fs::write(&legacy_map_path, legacy_map_bytes).unwrap();

        let mut settings = server::Settings::default();
        settings.world_seed = 424242;
        settings.day_length = 37.5;
        settings.map_file = Some(FileOpts::LoadLegacy(legacy_map_path.clone()));
        let settings_ron = ron::ser::to_string_pretty(&settings, ron::ser::PrettyConfig::default())
            .expect("settings should serialize");
        fs::write(old_world.join("server_config/settings.ron"), settings_ron).unwrap();

        migrate_old_singleplayer(&old_world, &migrated_world);

        let migrated = load_map(&migrated_world).expect("migrated world should load");
        assert!(!old_world.exists());
        assert!(migrated_world.exists());
        assert_eq!(migrated.seed, 424242);
        assert_eq!(migrated.day_length, 37.5);
        assert!(matches!(
            migrated.world_source,
            SingleplayerWorldSource::LegacyMigrated
        ));
        assert_eq!(migrated.source_ref, None);
        assert_eq!(
            migrated.legacy_origin,
            Some(SingleplayerLegacyOrigin::LoadLegacyPath(
                legacy_map_path.to_string_lossy().into_owned()
            ))
        );
        assert!(migrated.is_generated);
        assert_eq!(
            fs::read(migrated_world.join("map.bin")).unwrap(),
            legacy_map_bytes
        );

        let meta = fs::read_to_string(migrated_world.join("meta.ron")).unwrap();
        assert!(meta.contains("world_source: legacy_migrated"));
        assert!(meta.contains("legacy_origin: Some(load_legacy_path("));
        assert!(meta.contains("compat_audit: None"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn migrating_old_singleplayer_preserves_strict_load_origin_metadata() {
        let root =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-migrate-{}", Uuid::new_v4()));
        let old_world = root.join("singleplayer");
        let migrated_world = root.join("singleplayer_worlds").join("singleplayer");
        fs::create_dir_all(old_world.join("server_config")).unwrap();
        fs::create_dir_all(root.join("singleplayer_worlds")).unwrap();

        let strict_map_path = root.join("strict-source-world.bin");
        let strict_map_bytes = b"strict-world-map";
        fs::write(&strict_map_path, strict_map_bytes).unwrap();

        let mut settings = server::Settings::default();
        settings.map_file = Some(FileOpts::Load(strict_map_path.clone()));
        let settings_ron = ron::ser::to_string_pretty(&settings, ron::ser::PrettyConfig::default())
            .expect("settings should serialize");
        fs::write(old_world.join("server_config/settings.ron"), settings_ron).unwrap();

        migrate_old_singleplayer(&old_world, &migrated_world);

        let migrated = load_map(&migrated_world).expect("migrated world should load");
        assert!(matches!(
            migrated.world_source,
            SingleplayerWorldSource::LegacyMigrated
        ));
        assert_eq!(migrated.source_ref, None);
        assert_eq!(
            migrated.legacy_origin,
            Some(SingleplayerLegacyOrigin::LoadPath(
                strict_map_path.to_string_lossy().into_owned()
            ))
        );
        assert_eq!(
            fs::read(migrated_world.join("map.bin")).unwrap(),
            strict_map_bytes
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn migrating_old_singleplayer_preserves_non_default_asset_origin_metadata() {
        let root =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-migrate-{}", Uuid::new_v4()));
        let old_world = root.join("singleplayer");
        let migrated_world = root.join("singleplayer_worlds").join("singleplayer");
        fs::create_dir_all(old_world.join("server_config")).unwrap();
        fs::create_dir_all(root.join("singleplayer_worlds")).unwrap();

        let asset_name = format!("world.test.singleplayer-asset-{}", Uuid::new_v4());
        let mut settings = server::Settings::default();
        settings.map_file = Some(FileOpts::LoadAsset(asset_name.clone()));
        let settings_ron = ron::ser::to_string_pretty(&settings, ron::ser::PrettyConfig::default())
            .expect("settings should serialize");
        fs::write(old_world.join("server_config/settings.ron"), settings_ron).unwrap();

        migrate_old_singleplayer(&old_world, &migrated_world);

        let migrated = load_map(&migrated_world).expect("migrated world should load");
        assert!(matches!(
            migrated.world_source,
            SingleplayerWorldSource::LegacyMigrated
        ));
        assert_eq!(migrated.source_ref, None);
        assert_eq!(
            migrated.legacy_origin,
            Some(SingleplayerLegacyOrigin::LoadAsset(asset_name))
        );
        assert!(!migrated.is_generated);

        let meta = fs::read_to_string(migrated_world.join("meta.ron")).unwrap();
        assert!(meta.contains("legacy_origin: Some(load_asset("));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn migrating_old_singleplayer_preserves_load_or_generate_origin_metadata() {
        let root =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-migrate-{}", Uuid::new_v4()));
        let old_world = root.join("singleplayer");
        let migrated_world = root.join("singleplayer_worlds").join("singleplayer");
        fs::create_dir_all(old_world.join("server_config")).unwrap();
        fs::create_dir_all(root.join("singleplayer_worlds")).unwrap();

        let managed_name = format!("singleplayer-managed-world-{}", Uuid::new_v4());
        let managed_map_path = legacy_load_or_generate_map_path(&managed_name);
        let managed_map_bytes = b"managed-world-map";
        if let Some(parent) = managed_map_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&managed_map_path, managed_map_bytes).unwrap();

        let gen_opts = GenOpts {
            x_lg: 8,
            y_lg: 9,
            scale: 3.25,
            ..GenOpts::default()
        };

        let mut settings = server::Settings::default();
        settings.map_file = Some(FileOpts::LoadOrGenerate {
            name: managed_name.clone(),
            opts: gen_opts.clone(),
            overwrite: true,
        });
        let settings_ron = ron::ser::to_string_pretty(&settings, ron::ser::PrettyConfig::default())
            .expect("settings should serialize");
        fs::write(old_world.join("server_config/settings.ron"), settings_ron).unwrap();

        migrate_old_singleplayer(&old_world, &migrated_world);

        let migrated = load_map(&migrated_world).expect("migrated world should load");
        assert!(matches!(
            migrated.world_source,
            SingleplayerWorldSource::LegacyMigrated
        ));
        assert_eq!(migrated.source_ref, None);
        assert_eq!(
            migrated.legacy_origin,
            Some(SingleplayerLegacyOrigin::LoadOrGenerate {
                name: managed_name,
                overwrite: true,
            })
        );
        let migrated_gen_opts = migrated
            .gen_opts
            .as_ref()
            .expect("load_or_generate migration should retain gen opts");
        assert_eq!(migrated_gen_opts.x_lg, gen_opts.x_lg);
        assert_eq!(migrated_gen_opts.y_lg, gen_opts.y_lg);
        assert_eq!(migrated_gen_opts.scale, gen_opts.scale);
        assert_eq!(migrated_gen_opts.map_kind, gen_opts.map_kind);
        assert_eq!(migrated_gen_opts.erosion_quality, gen_opts.erosion_quality);
        assert!(migrated.is_generated);
        assert_eq!(
            fs::read(migrated_world.join("map.bin")).unwrap(),
            managed_map_bytes
        );

        let meta = fs::read_to_string(migrated_world.join("meta.ron")).unwrap();
        assert!(meta.contains("legacy_origin: Some(load_or_generate("));
        assert!(meta.contains("overwrite: true"));

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_file(managed_map_path);
    }

    #[test]
    fn missing_migrated_load_or_generate_keeps_save_runtime_contract() {
        let world_dir =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-world-{}", Uuid::new_v4()));
        let map_path = world_dir.join("map.bin");
        let gen_opts = GenOpts {
            x_lg: 7,
            y_lg: 8,
            scale: 2.5,
            ..GenOpts::default()
        };
        let managed_name = format!("singleplayer-missing-world-{}", Uuid::new_v4());
        let world = SingleplayerWorld {
            world_id: Uuid::new_v4(),
            realm_id: Uuid::new_v4(),
            name: "Managed Legacy".to_string(),
            gen_opts: Some(gen_opts.clone()),
            day_length: DAY_LENGTH_DEFAULT,
            seed: DEFAULT_WORLD_SEED,
            world_source: SingleplayerWorldSource::LegacyMigrated,
            source_ref: None,
            legacy_origin: Some(SingleplayerLegacyOrigin::LoadOrGenerate {
                name: managed_name,
                overwrite: false,
            }),
            compat_audit: None,
            managed_recipe_sidecar_missing: false,
            world_recipe_hash: None,
            topology_id: None,
            is_generated: false,
            path: world_dir.clone(),
            map_path: map_path.clone(),
        };

        match world.runtime_file_opts() {
            FileOpts::Save(path, opts) => {
                assert_eq!(path, map_path);
                assert_eq!(opts.x_lg, gen_opts.x_lg);
                assert_eq!(opts.y_lg, gen_opts.y_lg);
                assert_eq!(opts.scale, gen_opts.scale);
                assert_eq!(opts.map_kind, gen_opts.map_kind);
                assert_eq!(opts.erosion_quality, gen_opts.erosion_quality);
            },
            other => {
                panic!("pending migrated load_or_generate world should use Save(..), got {other:?}")
            },
        }
    }

    #[test]
    fn v5_round_trip_preserves_contract_metadata_and_compat_audit() {
        let world_dir =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-world-{}", Uuid::new_v4()));
        fs::create_dir_all(&world_dir).unwrap();

        let gen_opts = GenOpts::default();
        let manifest = RecipeManifestV1::record_only(DEFAULT_WORLD_SEED, &gen_opts, true);
        let compat_audit = CompatAuditV1::fallback_generate(
            server::CompatEntryKindV1::Load,
            server::CompatFailureKindV1::MissingInput,
        );
        let world = SingleplayerWorld {
            world_id: Uuid::new_v4(),
            realm_id: Uuid::new_v4(),
            name: "Custom World".to_string(),
            gen_opts: Some(gen_opts),
            day_length: DAY_LENGTH_DEFAULT,
            seed: DEFAULT_WORLD_SEED,
            world_source: SingleplayerWorldSource::Generated,
            source_ref: None,
            legacy_origin: None,
            compat_audit: Some(compat_audit),
            managed_recipe_sidecar_missing: false,
            world_recipe_hash: Some(manifest.world_recipe_hash.clone()),
            topology_id: Some(manifest.world_recipe.topology_id.as_str().to_owned()),
            is_generated: false,
            path: world_dir.clone(),
            map_path: world_dir.join("map.bin"),
        };

        write_world_meta(&world);
        let loaded = load_map(&world_dir).expect("v4 world should load");

        assert!(matches!(
            loaded.world_source,
            SingleplayerWorldSource::Generated
        ));
        assert_eq!(loaded.source_ref, None);
        assert_eq!(loaded.compat_audit, Some(compat_audit));
        assert_eq!(loaded.world_recipe_hash, Some(manifest.world_recipe_hash));
        assert_eq!(
            loaded.topology_id,
            Some(manifest.world_recipe.topology_id.as_str().to_owned())
        );

        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn loading_v5_world_with_mismatched_schema_version_is_rejected() {
        let world_dir =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-world-{}", Uuid::new_v4()));
        fs::create_dir_all(&world_dir).unwrap();

        let raw_meta = format!(
            "(\n    version: 5,\n    schema_version: 1,\n    world_id: \"{}\",\n    realm_id: \
             \"{}\",\n    name: \"Broken V5\",\n    gen_opts: None,\n    seed: {},\n    \
             day_length: {},\n    world_source: default_asset,\n    source_ref: Some(\"{}\"),\n    \
             compat_audit: None,\n    world_recipe_hash: None,\n    topology_id: \
             Some(\"{}\"),\n)\n",
            Uuid::new_v4(),
            Uuid::new_v4(),
            DEFAULT_WORLD_SEED,
            DAY_LENGTH_DEFAULT,
            DEFAULT_WORLD_MAP,
            TopologyId::BoundedPlaneV1.as_str(),
        );
        fs::write(world_dir.join("meta.ron"), raw_meta).unwrap();

        assert!(load_map(&world_dir).is_none());

        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn runtime_sync_promotes_generation_result_to_generated_contract() {
        let world_dir =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-world-{}", Uuid::new_v4()));
        fs::create_dir_all(&world_dir).unwrap();

        let manifest = RecipeManifestV1::record_only(DEFAULT_WORLD_SEED, &GenOpts::default(), true);
        let mut world = SingleplayerWorld {
            world_id: Uuid::new_v4(),
            realm_id: Uuid::new_v4(),
            name: "Pending World".to_string(),
            gen_opts: None,
            day_length: DAY_LENGTH_DEFAULT,
            seed: DEFAULT_WORLD_SEED,
            world_source: SingleplayerWorldSource::DefaultAsset,
            source_ref: Some(DEFAULT_WORLD_MAP.to_string()),
            legacy_origin: None,
            compat_audit: None,
            managed_recipe_sidecar_missing: false,
            world_recipe_hash: None,
            topology_id: Some(TopologyId::BoundedPlaneV1.as_str().to_owned()),
            is_generated: false,
            path: world_dir.clone(),
            map_path: world_dir.join("map.bin"),
        };

        world.sync_runtime_source_contract(
            CompatAuditV1::fallback_generate(
                server::CompatEntryKindV1::Load,
                server::CompatFailureKindV1::MissingInput,
            ),
            &manifest,
            false,
        );

        assert!(matches!(
            world.world_source,
            SingleplayerWorldSource::Generated
        ));
        assert_eq!(world.source_ref, None);
        assert_eq!(world.legacy_origin, None);
        assert_eq!(
            world.compat_audit,
            Some(CompatAuditV1::fallback_generate(
                server::CompatEntryKindV1::Load,
                server::CompatFailureKindV1::MissingInput,
            ))
        );
        let synced_gen_opts = world
            .gen_opts
            .as_ref()
            .expect("runtime generation should persist resolved gen opts");
        assert_eq!(synced_gen_opts.x_lg, manifest.world_recipe.gen_opts.x_lg);
        assert_eq!(synced_gen_opts.y_lg, manifest.world_recipe.gen_opts.y_lg);
        assert_eq!(synced_gen_opts.scale, manifest.world_recipe.gen_opts.scale);
        assert_eq!(
            synced_gen_opts.map_kind,
            manifest.world_recipe.gen_opts.map_kind
        );
        assert_eq!(
            synced_gen_opts.erosion_quality,
            manifest.world_recipe.gen_opts.erosion_quality
        );
        assert_eq!(world.world_recipe_hash, Some(manifest.world_recipe_hash));
        assert_eq!(
            world.topology_id,
            Some(manifest.world_recipe.topology_id.as_str().to_owned())
        );

        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn runtime_sync_preserves_default_asset_provenance_on_loaded_existing() {
        let world_dir =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-world-{}", Uuid::new_v4()));
        fs::create_dir_all(&world_dir).unwrap();
        fs::write(world_dir.join("map.bin"), b"map").unwrap();

        let manifest = RecipeManifestV1::record_only(DEFAULT_WORLD_SEED, &GenOpts::default(), true);
        let mut world = SingleplayerWorld {
            world_id: Uuid::new_v4(),
            realm_id: Uuid::new_v4(),
            name: "Default Asset".to_string(),
            gen_opts: None,
            day_length: DAY_LENGTH_DEFAULT,
            seed: DEFAULT_WORLD_SEED,
            world_source: SingleplayerWorldSource::DefaultAsset,
            source_ref: Some(DEFAULT_WORLD_MAP.to_string()),
            legacy_origin: None,
            compat_audit: None,
            managed_recipe_sidecar_missing: false,
            world_recipe_hash: None,
            topology_id: None,
            is_generated: false,
            path: world_dir.clone(),
            map_path: world_dir.join("map.bin"),
        };

        world.sync_runtime_source_contract(
            CompatAuditV1::loaded_existing(server::CompatEntryKindV1::Load),
            &manifest,
            false,
        );

        assert!(matches!(
            world.world_source,
            SingleplayerWorldSource::DefaultAsset
        ));
        assert_eq!(world.source_ref, Some(DEFAULT_WORLD_MAP.to_string()));
        assert_eq!(
            world.compat_audit,
            Some(CompatAuditV1::loaded_existing(
                server::CompatEntryKindV1::Load
            ))
        );
        assert_eq!(world.world_recipe_hash, None);
        assert_eq!(
            world.topology_id,
            Some(manifest.world_recipe.topology_id.as_str().to_owned())
        );
        assert!(world.is_generated);

        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn runtime_failure_sync_promotes_materialized_fallback_contract_without_recipe_history() {
        let world_dir =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-world-{}", Uuid::new_v4()));
        fs::create_dir_all(&world_dir).unwrap();
        fs::write(world_dir.join("map.bin"), b"map").unwrap();

        let mut world = SingleplayerWorld {
            world_id: Uuid::new_v4(),
            realm_id: Uuid::new_v4(),
            name: "Failure Case".to_string(),
            gen_opts: None,
            day_length: DAY_LENGTH_DEFAULT,
            seed: DEFAULT_WORLD_SEED,
            world_source: SingleplayerWorldSource::DefaultAsset,
            source_ref: Some(DEFAULT_WORLD_MAP.to_string()),
            legacy_origin: None,
            compat_audit: None,
            managed_recipe_sidecar_missing: false,
            world_recipe_hash: None,
            topology_id: Some(TopologyId::BoundedPlaneV1.as_str().to_owned()),
            is_generated: false,
            path: world_dir.clone(),
            map_path: world_dir.join("map.bin"),
        };

        let compat_audit = CompatAuditV1::fallback_generate(
            server::CompatEntryKindV1::Load,
            server::CompatFailureKindV1::OptionMismatch,
        );
        world.sync_runtime_failure_source_contract(Some(compat_audit));

        assert_eq!(world.compat_audit, Some(compat_audit));
        assert!(matches!(
            world.world_source,
            SingleplayerWorldSource::Generated
        ));
        assert_eq!(world.source_ref, None);
        assert_eq!(world.legacy_origin, None);
        assert_eq!(world.world_recipe_hash, None);
        assert_eq!(world.topology_id, None);
        assert!(world.is_generated);

        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn runtime_failure_sync_keeps_rejected_legacy_world_provenance() {
        let world_dir =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-world-{}", Uuid::new_v4()));
        fs::create_dir_all(&world_dir).unwrap();
        fs::write(world_dir.join("map.bin"), b"map").unwrap();

        let legacy_origin = SingleplayerLegacyOrigin::LoadLegacyPath(
            world_dir
                .join("legacy-source-world.bin")
                .to_string_lossy()
                .into_owned(),
        );
        let mut world = SingleplayerWorld {
            world_id: Uuid::new_v4(),
            realm_id: Uuid::new_v4(),
            name: "Rejected Legacy".to_string(),
            gen_opts: None,
            day_length: DAY_LENGTH_DEFAULT,
            seed: DEFAULT_WORLD_SEED,
            world_source: SingleplayerWorldSource::LegacyMigrated,
            source_ref: None,
            legacy_origin: Some(legacy_origin.clone()),
            compat_audit: None,
            managed_recipe_sidecar_missing: false,
            world_recipe_hash: None,
            topology_id: None,
            is_generated: false,
            path: world_dir.clone(),
            map_path: world_dir.join("map.bin"),
        };

        let compat_audit = CompatAuditV1::reject(
            server::CompatEntryKindV1::LoadLegacy,
            server::CompatFailureKindV1::PolicyDenied,
            CompatFailureSubjectV1::None,
            CompatFailureDetailV1::default(),
        );
        world.sync_runtime_failure_source_contract(Some(compat_audit));

        assert_eq!(world.compat_audit, Some(compat_audit));
        assert!(matches!(
            world.world_source,
            SingleplayerWorldSource::LegacyMigrated
        ));
        assert_eq!(world.source_ref, None);
        assert_eq!(world.legacy_origin, Some(legacy_origin));
        assert!(world.is_generated);

        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn runtime_loaded_existing_sync_preserves_legacy_unknown_provenance() {
        let world_dir =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-world-{}", Uuid::new_v4()));
        fs::create_dir_all(&world_dir).unwrap();
        fs::write(world_dir.join("map.bin"), b"map").unwrap();

        let manifest = RecipeManifestV1::record_only(DEFAULT_WORLD_SEED, &GenOpts::default(), true);
        let mut world = legacy_unknown_runtime_world(&world_dir);

        world.sync_runtime_source_contract(
            CompatAuditV1::loaded_existing(server::CompatEntryKindV1::LoadLegacy),
            &manifest,
            false,
        );

        assert!(matches!(
            world.world_source,
            SingleplayerWorldSource::LegacyUnknown
        ));
        assert_eq!(world.source_ref, None);
        assert_eq!(world.legacy_origin, None);
        assert_eq!(
            world.compat_audit,
            Some(CompatAuditV1::loaded_existing(
                server::CompatEntryKindV1::LoadLegacy
            ))
        );
        assert_eq!(world.world_recipe_hash, None);
        assert_eq!(world.topology_id, None);
        assert!(world.is_generated);

        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn runtime_sync_persists_sidecarless_managed_residual_truth() {
        let world_dir =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-world-{}", Uuid::new_v4()));
        fs::create_dir_all(&world_dir).unwrap();
        fs::write(world_dir.join("map.bin"), b"map").unwrap();

        let manifest = RecipeManifestV1::record_only(DEFAULT_WORLD_SEED, &GenOpts::default(), true);
        let mut world = SingleplayerWorld {
            world_id: Uuid::new_v4(),
            realm_id: Uuid::new_v4(),
            name: "Managed Residual".to_string(),
            gen_opts: None,
            day_length: DAY_LENGTH_DEFAULT,
            seed: DEFAULT_WORLD_SEED,
            world_source: SingleplayerWorldSource::LegacyMigrated,
            source_ref: None,
            legacy_origin: Some(SingleplayerLegacyOrigin::LoadOrGenerate {
                name: "managed-sidecarless".to_owned(),
                overwrite: false,
            }),
            compat_audit: None,
            managed_recipe_sidecar_missing: false,
            world_recipe_hash: None,
            topology_id: None,
            is_generated: true,
            path: world_dir.clone(),
            map_path: world_dir.join("map.bin"),
        };

        world.sync_runtime_source_contract(
            CompatAuditV1::loaded_existing(server::CompatEntryKindV1::LoadLegacy),
            &manifest,
            true,
        );
        write_world_meta(&world);

        let persisted = load_map(&world_dir).expect("persisted world should reload");
        assert!(persisted.managed_recipe_sidecar_missing);
        assert_eq!(
            persisted.legacy_origin,
            Some(SingleplayerLegacyOrigin::LoadOrGenerate {
                name: "managed-sidecarless".to_owned(),
                overwrite: false,
            })
        );

        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn runtime_failure_sync_keeps_legacy_unknown_provenance() {
        let world_dir =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-world-{}", Uuid::new_v4()));
        fs::create_dir_all(&world_dir).unwrap();
        fs::write(world_dir.join("map.bin"), b"map").unwrap();

        let mut world = legacy_unknown_runtime_world(&world_dir);
        let compat_audit = CompatAuditV1::reject(
            server::CompatEntryKindV1::LoadLegacy,
            server::CompatFailureKindV1::PolicyDenied,
            CompatFailureSubjectV1::Options,
            CompatFailureDetailV1::default(),
        );

        world.sync_runtime_failure_source_contract(Some(compat_audit));

        assert!(matches!(
            world.world_source,
            SingleplayerWorldSource::LegacyUnknown
        ));
        assert_eq!(world.source_ref, None);
        assert_eq!(world.legacy_origin, None);
        assert_eq!(world.compat_audit, Some(compat_audit));
        assert_eq!(world.world_recipe_hash, None);
        assert_eq!(world.topology_id, None);
        assert!(world.is_generated);

        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn runtime_failure_sync_without_compat_audit_keeps_materialized_generate_contract() {
        let world_dir =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-world-{}", Uuid::new_v4()));
        fs::create_dir_all(&world_dir).unwrap();
        fs::write(world_dir.join("map.bin"), b"map").unwrap();

        let gen_opts = GenOpts::default();
        let manifest = RecipeManifestV1::record_only(DEFAULT_WORLD_SEED, &gen_opts, true);
        let mut world = SingleplayerWorld {
            world_id: Uuid::new_v4(),
            realm_id: Uuid::new_v4(),
            name: "Generated Failure".to_string(),
            gen_opts: Some(gen_opts),
            day_length: DAY_LENGTH_DEFAULT,
            seed: DEFAULT_WORLD_SEED,
            world_source: SingleplayerWorldSource::Generated,
            source_ref: None,
            legacy_origin: Some(SingleplayerLegacyOrigin::LoadPath(
                world_dir
                    .join("stale-world.bin")
                    .to_string_lossy()
                    .into_owned(),
            )),
            compat_audit: Some(CompatAuditV1::loaded_existing(
                server::CompatEntryKindV1::Load,
            )),
            managed_recipe_sidecar_missing: false,
            world_recipe_hash: Some(manifest.world_recipe_hash.clone()),
            topology_id: Some(manifest.world_recipe.topology_id.as_str().to_owned()),
            is_generated: false,
            path: world_dir.clone(),
            map_path: world_dir.join("map.bin"),
        };

        world.sync_runtime_failure_source_contract(None);

        assert_eq!(world.compat_audit, None);
        assert!(matches!(
            world.world_source,
            SingleplayerWorldSource::Generated
        ));
        assert_eq!(world.source_ref, None);
        assert_eq!(world.world_recipe_hash, Some(manifest.world_recipe_hash));
        assert_eq!(
            world.topology_id,
            Some(manifest.world_recipe.topology_id.as_str().to_owned())
        );
        assert!(world.is_generated);

        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn refresh_pending_contract_clears_stale_runtime_compat_audit() {
        let world_dir =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-world-{}", Uuid::new_v4()));
        fs::create_dir_all(&world_dir).unwrap();

        let mut world = SingleplayerWorld {
            world_id: Uuid::new_v4(),
            realm_id: Uuid::new_v4(),
            name: "Pending World".to_string(),
            gen_opts: Some(GenOpts::default()),
            day_length: DAY_LENGTH_DEFAULT,
            seed: DEFAULT_WORLD_SEED,
            world_source: SingleplayerWorldSource::Generated,
            source_ref: None,
            legacy_origin: None,
            compat_audit: Some(CompatAuditV1::loaded_existing(
                server::CompatEntryKindV1::Load,
            )),
            managed_recipe_sidecar_missing: false,
            world_recipe_hash: None,
            topology_id: None,
            is_generated: false,
            path: world_dir.clone(),
            map_path: world_dir.join("map.bin"),
        };

        world.refresh_pending_source_contract();

        assert_eq!(world.compat_audit, None);
        assert_eq!(world.legacy_origin, None);

        let _ = fs::remove_dir_all(world_dir);
    }

    #[test]
    fn deleting_map_then_saving_world_meta_persists_pending_contract_immediately() {
        let root =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-world-{}", Uuid::new_v4()));
        let world_dir = root.join("singleplayer");
        fs::create_dir_all(&world_dir).unwrap();

        let gen_opts = GenOpts {
            x_lg: 8,
            y_lg: 9,
            scale: 3.25,
            ..GenOpts::default()
        };
        let recipe_manifest = RecipeManifestV1::record_only(DEFAULT_WORLD_SEED, &gen_opts, true);
        let world_recipe_hash = recipe_manifest.world_recipe_hash.clone();
        let topology_id = recipe_manifest.world_recipe.topology_id.as_str().to_owned();
        let map_path = world_dir.join("map.bin");
        fs::write(&map_path, b"generated-world-map").unwrap();

        let world = SingleplayerWorld {
            world_id: Uuid::new_v4(),
            realm_id: Uuid::new_v4(),
            name: "Generated World".to_string(),
            gen_opts: Some(gen_opts),
            day_length: DAY_LENGTH_DEFAULT,
            seed: DEFAULT_WORLD_SEED,
            world_source: SingleplayerWorldSource::Generated,
            source_ref: None,
            legacy_origin: None,
            compat_audit: Some(CompatAuditV1::loaded_existing(
                server::CompatEntryKindV1::Load,
            )),
            managed_recipe_sidecar_missing: false,
            world_recipe_hash: Some(world_recipe_hash),
            topology_id: Some(topology_id),
            is_generated: true,
            path: world_dir.clone(),
            map_path: map_path.clone(),
        };
        write_world_meta(&world);

        let mut worlds = SingleplayerWorlds {
            worlds: vec![world],
            current: None,
            worlds_folder: root.clone(),
        };

        worlds.delete_map_file(0);
        worlds.save_world_meta(0);

        let persisted = load_map(&world_dir).expect("pending world should reload");
        assert!(!map_path.exists());
        assert!(!persisted.is_generated);
        assert!(matches!(
            persisted.world_source,
            SingleplayerWorldSource::Generated
        ));
        assert_eq!(persisted.compat_audit, None);
        assert_eq!(persisted.source_ref, None);
        assert_eq!(persisted.legacy_origin, None);
        let persisted_gen_opts = persisted
            .gen_opts
            .clone()
            .expect("pending generated world should retain gen opts");
        assert_eq!(
            persisted.world_recipe_hash,
            Some(
                RecipeManifestV1::record_only(DEFAULT_WORLD_SEED, &persisted_gen_opts, true)
                    .world_recipe_hash
            )
        );
        assert_eq!(
            persisted.topology_id,
            Some(TopologyId::BoundedPlaneV1.as_str().to_owned())
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn runtime_file_opts_uses_load_asset_for_default_asset_worlds() {
        let world_dir =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-world-{}", Uuid::new_v4()));
        let world = SingleplayerWorld {
            world_id: Uuid::new_v4(),
            realm_id: Uuid::new_v4(),
            name: "Default Asset".to_string(),
            gen_opts: None,
            day_length: DAY_LENGTH_DEFAULT,
            seed: DEFAULT_WORLD_SEED,
            world_source: SingleplayerWorldSource::DefaultAsset,
            source_ref: Some(DEFAULT_WORLD_MAP.to_string()),
            legacy_origin: None,
            compat_audit: None,
            managed_recipe_sidecar_missing: false,
            world_recipe_hash: None,
            topology_id: Some(TopologyId::BoundedPlaneV1.as_str().to_owned()),
            is_generated: false,
            path: world_dir.clone(),
            map_path: world_dir.join("map.bin"),
        };

        match world.runtime_file_opts() {
            FileOpts::LoadAsset(asset) => assert_eq!(asset, DEFAULT_WORLD_MAP),
            other => panic!("default asset world should use LoadAsset(..), got {other:?}"),
        }
    }

    #[test]
    fn runtime_file_opts_uses_load_legacy_for_materialized_legacy_worlds() {
        let world_dir =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-world-{}", Uuid::new_v4()));
        let map_path = world_dir.join("map.bin");

        for world_source in [
            SingleplayerWorldSource::LegacyUnknown,
            SingleplayerWorldSource::LegacyMigrated,
        ] {
            let world = SingleplayerWorld {
                world_id: Uuid::new_v4(),
                realm_id: Uuid::new_v4(),
                name: "Legacy".to_string(),
                gen_opts: None,
                day_length: DAY_LENGTH_DEFAULT,
                seed: DEFAULT_WORLD_SEED,
                world_source,
                source_ref: None,
                legacy_origin: None,
                compat_audit: None,
                managed_recipe_sidecar_missing: false,
                world_recipe_hash: None,
                topology_id: None,
                is_generated: true,
                path: world_dir.clone(),
                map_path: map_path.clone(),
            };

            match world.runtime_file_opts() {
                FileOpts::LoadLegacy(path) => assert_eq!(path, map_path),
                other => panic!("legacy world should use LoadLegacy(..), got {other:?}"),
            }
        }
    }

    #[test]
    fn runtime_file_opts_keeps_generated_worlds_on_managed_contracts() {
        let world_dir =
            std::env::temp_dir().join(format!("caldrayne-singleplayer-world-{}", Uuid::new_v4()));
        let map_path = world_dir.join("map.bin");
        let gen_opts = GenOpts::default();
        let pending_world = SingleplayerWorld {
            world_id: Uuid::new_v4(),
            realm_id: Uuid::new_v4(),
            name: "Pending Generated".to_string(),
            gen_opts: Some(gen_opts.clone()),
            day_length: DAY_LENGTH_DEFAULT,
            seed: DEFAULT_WORLD_SEED,
            world_source: SingleplayerWorldSource::Generated,
            source_ref: None,
            legacy_origin: None,
            compat_audit: None,
            managed_recipe_sidecar_missing: false,
            world_recipe_hash: None,
            topology_id: None,
            is_generated: false,
            path: world_dir.clone(),
            map_path: map_path.clone(),
        };

        match pending_world.runtime_file_opts() {
            FileOpts::Save(path, opts) => {
                assert_eq!(path, map_path);
                assert_eq!(opts.x_lg, gen_opts.x_lg);
                assert_eq!(opts.y_lg, gen_opts.y_lg);
                assert_eq!(opts.scale, gen_opts.scale);
            },
            other => panic!("pending generated world should use Save(..), got {other:?}"),
        }

        let materialized_world = SingleplayerWorld {
            is_generated: true,
            ..pending_world
        };

        match materialized_world.runtime_file_opts() {
            FileOpts::Load(path) => assert_eq!(path, map_path),
            other => panic!("materialized generated world should use Load(..), got {other:?}"),
        }
    }
}
