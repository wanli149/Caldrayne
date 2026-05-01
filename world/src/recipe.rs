use crate::{
    config::{CONFIG, Config, Features},
    sim::GenOpts,
};
use bincode::{config::standard, serde::encode_to_vec};
use common::assets::AssetExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub const RECIPE_SCHEMA_VERSION: u32 = 1;
pub const WORLD_ALG_VERSION: &str = "world-sim-v1-record-only";
pub const CHUNK_PASS_VERSION: &str = "chunk-static-v1-record-only";
const WORLD_FEATURES_MANIFEST: &str = "world.features";

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompatEntryKindV1 {
    #[default]
    Generate,
    Save,
    LoadOrGenerate,
    LoadLegacy,
    Load,
    LoadAsset,
}

impl CompatEntryKindV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Generate => "generate",
            Self::Save => "save",
            Self::LoadOrGenerate => "load_or_generate",
            Self::LoadLegacy => "load_legacy",
            Self::Load => "load",
            Self::LoadAsset => "load_asset",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompatDecisionV1 {
    #[default]
    GenerateRequested,
    LoadedExisting,
    FallbackGenerate,
}

impl CompatDecisionV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GenerateRequested => "generate_requested",
            Self::LoadedExisting => "loaded_existing",
            Self::FallbackGenerate => "fallback_generate",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompatFailureKindV1 {
    #[default]
    None,
    MissingInput,
    ParseError,
    InvalidWorld,
    OptionMismatch,
    PolicyDenied,
}

impl CompatFailureKindV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::MissingInput => "missing_input",
            Self::ParseError => "parse_error",
            Self::InvalidWorld => "invalid_world",
            Self::OptionMismatch => "option_mismatch",
            Self::PolicyDenied => "policy_denied",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompatResolutionV1 {
    #[default]
    Continue,
    Reject,
}

impl CompatResolutionV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Continue => "continue",
            Self::Reject => "reject",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompatFailureSubjectV1 {
    #[default]
    None,
    World,
    Recipe,
    Topology,
    Options,
}

impl CompatFailureSubjectV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::World => "world",
            Self::Recipe => "recipe",
            Self::Topology => "topology",
            Self::Options => "options",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct CompatFailureDetailV1 {
    pub legacy_world_version: bool,
    pub world_size_mismatch: bool,
    pub world_scale_mismatch: bool,
}

impl CompatFailureDetailV1 {
    pub const fn legacy_world_version() -> Self {
        Self {
            legacy_world_version: true,
            world_size_mismatch: false,
            world_scale_mismatch: false,
        }
    }

    pub const fn option_mismatch(world_size_mismatch: bool, world_scale_mismatch: bool) -> Self {
        Self {
            legacy_world_version: false,
            world_size_mismatch,
            world_scale_mismatch,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct CompatAuditV1 {
    pub entry: CompatEntryKindV1,
    pub decision: CompatDecisionV1,
    pub failure_kind: CompatFailureKindV1,
    #[serde(default)]
    pub resolution: CompatResolutionV1,
    #[serde(default)]
    pub failure_subject: CompatFailureSubjectV1,
    #[serde(default)]
    pub failure_detail: CompatFailureDetailV1,
}

impl CompatAuditV1 {
    pub const fn generate_requested(entry: CompatEntryKindV1) -> Self {
        Self {
            entry,
            decision: CompatDecisionV1::GenerateRequested,
            failure_kind: CompatFailureKindV1::None,
            resolution: CompatResolutionV1::Continue,
            failure_subject: CompatFailureSubjectV1::None,
            failure_detail: CompatFailureDetailV1 {
                legacy_world_version: false,
                world_size_mismatch: false,
                world_scale_mismatch: false,
            },
        }
    }

    pub const fn loaded_existing(entry: CompatEntryKindV1) -> Self {
        Self {
            entry,
            decision: CompatDecisionV1::LoadedExisting,
            failure_kind: CompatFailureKindV1::None,
            resolution: CompatResolutionV1::Continue,
            failure_subject: CompatFailureSubjectV1::None,
            failure_detail: CompatFailureDetailV1 {
                legacy_world_version: false,
                world_size_mismatch: false,
                world_scale_mismatch: false,
            },
        }
    }

    pub const fn fallback_generate(
        entry: CompatEntryKindV1,
        failure_kind: CompatFailureKindV1,
    ) -> Self {
        Self::fallback_generate_with_detail(
            entry,
            failure_kind,
            CompatFailureSubjectV1::None,
            CompatFailureDetailV1 {
                legacy_world_version: false,
                world_size_mismatch: false,
                world_scale_mismatch: false,
            },
        )
    }

    pub const fn fallback_generate_with_detail(
        entry: CompatEntryKindV1,
        failure_kind: CompatFailureKindV1,
        failure_subject: CompatFailureSubjectV1,
        failure_detail: CompatFailureDetailV1,
    ) -> Self {
        Self {
            entry,
            decision: CompatDecisionV1::FallbackGenerate,
            failure_kind,
            resolution: CompatResolutionV1::Continue,
            failure_subject,
            failure_detail,
        }
    }

    pub const fn reject(
        entry: CompatEntryKindV1,
        failure_kind: CompatFailureKindV1,
        failure_subject: CompatFailureSubjectV1,
        failure_detail: CompatFailureDetailV1,
    ) -> Self {
        Self {
            entry,
            decision: CompatDecisionV1::FallbackGenerate,
            failure_kind,
            resolution: CompatResolutionV1::Reject,
            failure_subject,
            failure_detail,
        }
    }

    pub const fn is_rejected(self) -> bool { matches!(self.resolution, CompatResolutionV1::Reject) }

    pub const fn is_strict_load_contract_gap(self) -> bool {
        matches!(
            self.entry,
            CompatEntryKindV1::Load | CompatEntryKindV1::LoadAsset
        ) && matches!(self.decision, CompatDecisionV1::FallbackGenerate)
            && matches!(self.resolution, CompatResolutionV1::Continue)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TopologyId {
    #[default]
    BoundedPlaneV1,
    WrapToroidalExpV1,
    WrapCylindricalExpV1,
}

impl TopologyId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BoundedPlaneV1 => "bounded_plane_v1",
            Self::WrapToroidalExpV1 => "wrap_toroidal_exp_v1",
            Self::WrapCylindricalExpV1 => "wrap_cylindrical_exp_v1",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PresetId {
    #[default]
    CustomGenOptsV1,
}

impl PresetId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CustomGenOptsV1 => "custom_gen_opts_v1",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorldRecipeV1 {
    pub schema_version: u32,
    pub world_seed: u32,
    pub gen_opts: GenOpts,
    pub topology_id: TopologyId,
    pub seed_elements: bool,
    pub preset_id: PresetId,
    pub world_alg_version: String,
    pub config_hash: Option<String>,
    pub asset_hash: Option<String>,
}

// Keep the new provenance hashes observable in the record-only recipe without
// silently promoting them into the stable world recipe hash contract yet.
#[derive(Clone, Copy, Debug, Serialize)]
struct WorldRecipeHashContractV1<'a> {
    schema_version: u32,
    world_seed: u32,
    gen_opts: &'a GenOpts,
    topology_id: TopologyId,
    seed_elements: bool,
    preset_id: PresetId,
    world_alg_version: &'a str,
    config_hash: Option<&'a str>,
    asset_hash: Option<&'a str>,
}

impl<'a> From<&'a WorldRecipeV1> for WorldRecipeHashContractV1<'a> {
    fn from(recipe: &'a WorldRecipeV1) -> Self {
        Self {
            schema_version: recipe.schema_version,
            world_seed: recipe.world_seed,
            gen_opts: &recipe.gen_opts,
            topology_id: recipe.topology_id,
            seed_elements: recipe.seed_elements,
            preset_id: recipe.preset_id,
            world_alg_version: recipe.world_alg_version.as_str(),
            config_hash: None,
            asset_hash: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
struct WorldConfigProvenanceV1 {
    sea_level: f32,
    mountain_scale: f32,
    snow_temp: f32,
    temperate_temp: f32,
    tropical_temp: f32,
    desert_temp: f32,
    desert_hum: f32,
    forest_hum: f32,
    jungle_hum: f32,
    rainfall_chunk_rate: f32,
    river_roughness: f32,
    river_max_width: f32,
    river_min_height: f32,
    river_width_to_depth: f32,
    ice_color: [u8; 3],
}

impl WorldConfigProvenanceV1 {
    fn from_config(config: &Config) -> Self {
        Self {
            sea_level: config.sea_level,
            mountain_scale: config.mountain_scale,
            snow_temp: config.snow_temp,
            temperate_temp: config.temperate_temp,
            tropical_temp: config.tropical_temp,
            desert_temp: config.desert_temp,
            desert_hum: config.desert_hum,
            forest_hum: config.forest_hum,
            jungle_hum: config.jungle_hum,
            rainfall_chunk_rate: config.rainfall_chunk_rate,
            river_roughness: config.river_roughness,
            river_max_width: config.river_max_width,
            river_min_height: config.river_min_height,
            river_width_to_depth: config.river_width_to_depth,
            ice_color: [config.ice_color.r, config.ice_color.g, config.ice_color.b],
        }
    }

    fn recorded_hash() -> String { stable_hash(&Self::from_config(&CONFIG)) }
}

#[derive(Clone, Copy, Debug, Serialize)]
struct WorldAssetProvenanceV1 {
    caverns: bool,
    caves: bool,
    rocks: bool,
    shrubs: bool,
    trees: bool,
    scatter: bool,
    paths: bool,
    spots: bool,
    wildlife_density: f32,
    peak_naming: bool,
    biome_naming: bool,
    train_tracks: bool,
}

impl WorldAssetProvenanceV1 {
    fn record_only() -> Self {
        let features = Arc::<Features>::load_expect(WORLD_FEATURES_MANIFEST);
        let features = features.read();
        Self::from_features(features.as_ref())
    }

    fn from_features(features: &Features) -> Self {
        Self {
            caverns: features.caverns,
            caves: features.caves,
            rocks: features.rocks,
            shrubs: features.shrubs,
            trees: features.trees,
            scatter: features.scatter,
            paths: features.paths,
            spots: features.spots,
            wildlife_density: features.wildlife_density,
            peak_naming: features.peak_naming,
            biome_naming: features.biome_naming,
            train_tracks: features.train_tracks,
        }
    }

    fn recorded_hash() -> String { stable_hash(&Self::record_only()) }
}

impl WorldRecipeV1 {
    pub fn record_only(world_seed: u32, gen_opts: &GenOpts, seed_elements: bool) -> Self {
        let config_hash = WorldConfigProvenanceV1::recorded_hash();
        let asset_hash = WorldAssetProvenanceV1::recorded_hash();
        Self {
            schema_version: RECIPE_SCHEMA_VERSION,
            world_seed,
            gen_opts: gen_opts.clone(),
            topology_id: TopologyId::BoundedPlaneV1,
            seed_elements,
            preset_id: PresetId::CustomGenOptsV1,
            world_alg_version: WORLD_ALG_VERSION.to_string(),
            config_hash: Some(config_hash),
            asset_hash: Some(asset_hash),
        }
    }

    fn stable_hash(&self) -> String { stable_hash(&WorldRecipeHashContractV1::from(self)) }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct StaticFeatureProfileV1 {
    caverns: bool,
    caves: bool,
    rocks: bool,
    shrubs: bool,
    trees: bool,
    scatter: bool,
    paths: bool,
    spots: bool,
    peak_naming: bool,
    biome_naming: bool,
    train_tracks: bool,
}

impl StaticFeatureProfileV1 {
    fn record_only() -> Self {
        let features = Arc::<Features>::load_expect(WORLD_FEATURES_MANIFEST);
        let features = features.read();
        Self::from_features(features.as_ref())
    }

    const fn from_features(features: &Features) -> Self {
        Self {
            caverns: features.caverns,
            caves: features.caves,
            rocks: features.rocks,
            shrubs: features.shrubs,
            trees: features.trees,
            scatter: features.scatter,
            paths: features.paths,
            spots: features.spots,
            peak_naming: features.peak_naming,
            biome_naming: features.biome_naming,
            train_tracks: features.train_tracks,
        }
    }

    fn stable_id(&self) -> String {
        fn flag(enabled: bool) -> u8 { u8::from(enabled) }

        format!(
            "static_feature_profile_v1(caverns={},caves={},rocks={},shrubs={},trees={},scatter={},\
             paths={},spots={},peak_naming={},biome_naming={},train_tracks={})",
            flag(self.caverns),
            flag(self.caves),
            flag(self.rocks),
            flag(self.shrubs),
            flag(self.trees),
            flag(self.scatter),
            flag(self.paths),
            flag(self.spots),
            flag(self.peak_naming),
            flag(self.biome_naming),
            flag(self.train_tracks),
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChunkRecipeV1 {
    pub schema_version: u32,
    pub world_recipe_hash: String,
    pub topology_id: TopologyId,
    pub preset_id: PresetId,
    pub chunk_pass_version: String,
    pub static_feature_profile: Option<String>,
}

impl ChunkRecipeV1 {
    pub fn record_only(world_recipe: &WorldRecipeV1) -> Self {
        let static_feature_profile = StaticFeatureProfileV1::record_only();
        Self {
            schema_version: RECIPE_SCHEMA_VERSION,
            world_recipe_hash: world_recipe.stable_hash(),
            topology_id: world_recipe.topology_id,
            preset_id: world_recipe.preset_id,
            chunk_pass_version: CHUNK_PASS_VERSION.to_string(),
            static_feature_profile: Some(static_feature_profile.stable_id()),
        }
    }

    fn stable_hash(&self) -> String { stable_hash(self) }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct RecipeManifestV1 {
    pub world_recipe: WorldRecipeV1,
    pub world_recipe_hash: String,
    pub chunk_recipe: ChunkRecipeV1,
    pub chunk_recipe_hash: String,
}

impl RecipeManifestV1 {
    pub fn record_only(world_seed: u32, gen_opts: &GenOpts, seed_elements: bool) -> Self {
        let world_recipe = WorldRecipeV1::record_only(world_seed, gen_opts, seed_elements);
        let world_recipe_hash = world_recipe.stable_hash();
        let chunk_recipe = ChunkRecipeV1::record_only(&world_recipe);
        let chunk_recipe_hash = chunk_recipe.stable_hash();

        Self {
            world_recipe,
            world_recipe_hash,
            chunk_recipe,
            chunk_recipe_hash,
        }
    }

    pub fn validates_record_only_contract(&self) -> bool {
        self.world_recipe_hash == self.world_recipe.stable_hash()
            && self.chunk_recipe.world_recipe_hash == self.world_recipe_hash
            && self.chunk_recipe.topology_id == self.world_recipe.topology_id
            && self.chunk_recipe.preset_id == self.world_recipe.preset_id
            && self.chunk_recipe_hash == self.chunk_recipe.stable_hash()
    }
}

impl Default for WorldRecipeV1 {
    fn default() -> Self { Self::record_only(0, &GenOpts::default(), true) }
}

impl Default for ChunkRecipeV1 {
    fn default() -> Self { Self::record_only(&WorldRecipeV1::default()) }
}

fn stable_hash<T: Serialize>(value: &T) -> String {
    let bytes = encode_to_vec(value, standard()).expect("recipe encoding should not fail");
    let digest = Sha256::digest(&bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::{
        ChunkRecipeV1, CompatAuditV1, CompatDecisionV1, CompatEntryKindV1, CompatFailureDetailV1,
        CompatFailureKindV1, CompatFailureSubjectV1, CompatResolutionV1, RecipeManifestV1,
        StaticFeatureProfileV1, TopologyId, WorldRecipeV1,
    };
    use crate::sim::GenOpts;

    #[test]
    fn record_only_manifest_is_stable_for_same_inputs() {
        let lhs = RecipeManifestV1::record_only(42, &GenOpts::default(), true);
        let rhs = RecipeManifestV1::record_only(42, &GenOpts::default(), true);

        assert_eq!(lhs.world_recipe_hash, rhs.world_recipe_hash);
        assert_eq!(lhs.chunk_recipe_hash, rhs.chunk_recipe_hash);
    }

    #[test]
    fn world_recipe_hash_changes_with_seed() {
        let lhs = RecipeManifestV1::record_only(42, &GenOpts::default(), true);
        let rhs = RecipeManifestV1::record_only(7, &GenOpts::default(), true);

        assert_ne!(lhs.world_recipe_hash, rhs.world_recipe_hash);
        assert_ne!(lhs.chunk_recipe_hash, rhs.chunk_recipe_hash);
    }

    #[test]
    fn record_only_manifest_detects_internal_contract_drift() {
        let mut manifest = RecipeManifestV1::record_only(42, &GenOpts::default(), true);

        assert!(manifest.validates_record_only_contract());

        manifest.world_recipe_hash = "tampered-world-recipe-hash".to_owned();
        assert!(!manifest.validates_record_only_contract());
    }

    #[test]
    fn topology_is_part_of_recipe_contract() {
        let mut world_recipe = WorldRecipeV1::record_only(42, &GenOpts::default(), true);
        let base_hash = world_recipe.stable_hash();
        world_recipe.topology_id = TopologyId::WrapToroidalExpV1;

        assert_ne!(base_hash, world_recipe.stable_hash());
        let chunk_recipe = ChunkRecipeV1::record_only(&world_recipe);
        assert_eq!(chunk_recipe.topology_id, TopologyId::WrapToroidalExpV1);
    }

    #[test]
    fn world_recipe_records_config_and_asset_provenance_hashes() {
        let recipe = WorldRecipeV1::record_only(42, &GenOpts::default(), true);

        assert_eq!(recipe.config_hash.as_deref().map(str::len), Some(64));
        assert_eq!(recipe.asset_hash.as_deref().map(str::len), Some(64));
    }

    #[test]
    fn world_recipe_hash_ignores_record_only_provenance_fields() {
        let mut recipe = WorldRecipeV1::record_only(42, &GenOpts::default(), true);
        let base_hash = recipe.stable_hash();

        recipe.config_hash = Some("manually-overridden-config-hash".to_owned());
        recipe.asset_hash = Some("manually-overridden-asset-hash".to_owned());

        assert_eq!(base_hash, recipe.stable_hash());
    }

    #[test]
    fn chunk_recipe_records_static_feature_profile() {
        let chunk_recipe = ChunkRecipeV1::record_only(&WorldRecipeV1::default());

        assert!(
            chunk_recipe
                .static_feature_profile
                .as_deref()
                .is_some_and(|profile| profile.starts_with("static_feature_profile_v1("))
        );
    }

    #[test]
    fn chunk_recipe_hash_changes_with_static_feature_profile() {
        let world_recipe = WorldRecipeV1::record_only(42, &GenOpts::default(), true);
        let base_profile = StaticFeatureProfileV1::record_only();
        let mut changed_profile = base_profile.clone();
        changed_profile.peak_naming = !changed_profile.peak_naming;

        let base_recipe = ChunkRecipeV1::record_only(&world_recipe);
        let changed_recipe = ChunkRecipeV1 {
            static_feature_profile: Some(changed_profile.stable_id()),
            ..base_recipe.clone()
        };

        assert_ne!(base_profile.stable_id(), changed_profile.stable_id());
        assert_ne!(base_recipe.stable_hash(), changed_recipe.stable_hash());
    }

    #[test]
    fn strict_load_fallback_is_reported_as_contract_gap() {
        let audit = CompatAuditV1::fallback_generate(
            CompatEntryKindV1::Load,
            CompatFailureKindV1::ParseError,
        );

        assert_eq!(audit.decision, CompatDecisionV1::FallbackGenerate);
        assert_eq!(audit.resolution, CompatResolutionV1::Continue);
        assert!(audit.is_strict_load_contract_gap());
    }

    #[test]
    fn load_or_generate_fallback_is_not_strict_load_gap() {
        let audit = CompatAuditV1::fallback_generate(
            CompatEntryKindV1::LoadOrGenerate,
            CompatFailureKindV1::MissingInput,
        );

        assert!(!audit.is_strict_load_contract_gap());
    }

    #[test]
    fn load_legacy_fallback_is_not_strict_load_gap() {
        let audit = CompatAuditV1::fallback_generate(
            CompatEntryKindV1::LoadLegacy,
            CompatFailureKindV1::MissingInput,
        );

        assert!(!audit.is_strict_load_contract_gap());
    }

    #[test]
    fn reject_preserves_structured_failure_contract() {
        let audit = CompatAuditV1::reject(
            CompatEntryKindV1::LoadOrGenerate,
            CompatFailureKindV1::OptionMismatch,
            CompatFailureSubjectV1::Options,
            CompatFailureDetailV1::option_mismatch(true, false),
        );

        assert_eq!(audit.decision, CompatDecisionV1::FallbackGenerate);
        assert_eq!(audit.resolution, CompatResolutionV1::Reject);
        assert!(audit.is_rejected());
        assert_eq!(audit.failure_subject, CompatFailureSubjectV1::Options);
        assert!(audit.failure_detail.world_size_mismatch);
        assert!(!audit.failure_detail.world_scale_mismatch);
    }
}
