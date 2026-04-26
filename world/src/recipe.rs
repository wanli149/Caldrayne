use crate::sim::GenOpts;
use bincode::{config::standard, serde::encode_to_vec};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const RECIPE_SCHEMA_VERSION: u32 = 1;
pub const WORLD_ALG_VERSION: &str = "world-sim-v1-record-only";
pub const CHUNK_PASS_VERSION: &str = "chunk-static-v1-record-only";

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
}

impl CompatFailureKindV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::MissingInput => "missing_input",
            Self::ParseError => "parse_error",
            Self::InvalidWorld => "invalid_world",
            Self::OptionMismatch => "option_mismatch",
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
            CompatEntryKindV1::LoadLegacy | CompatEntryKindV1::Load | CompatEntryKindV1::LoadAsset
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

impl WorldRecipeV1 {
    pub fn record_only(world_seed: u32, gen_opts: &GenOpts, seed_elements: bool) -> Self {
        Self {
            schema_version: RECIPE_SCHEMA_VERSION,
            world_seed,
            gen_opts: gen_opts.clone(),
            topology_id: TopologyId::BoundedPlaneV1,
            seed_elements,
            preset_id: PresetId::CustomGenOptsV1,
            world_alg_version: WORLD_ALG_VERSION.to_string(),
            config_hash: None,
            asset_hash: None,
        }
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
        Self {
            schema_version: RECIPE_SCHEMA_VERSION,
            world_recipe_hash: stable_hash(world_recipe),
            topology_id: world_recipe.topology_id,
            preset_id: world_recipe.preset_id,
            chunk_pass_version: CHUNK_PASS_VERSION.to_string(),
            static_feature_profile: None,
        }
    }
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
        let world_recipe_hash = stable_hash(&world_recipe);
        let chunk_recipe = ChunkRecipeV1::record_only(&world_recipe);
        let chunk_recipe_hash = stable_hash(&chunk_recipe);

        Self {
            world_recipe,
            world_recipe_hash,
            chunk_recipe,
            chunk_recipe_hash,
        }
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
        TopologyId, WorldRecipeV1, stable_hash,
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
    fn topology_is_part_of_recipe_contract() {
        let mut world_recipe = WorldRecipeV1::record_only(42, &GenOpts::default(), true);
        let base_hash = stable_hash(&world_recipe);
        world_recipe.topology_id = TopologyId::WrapToroidalExpV1;

        assert_ne!(base_hash, stable_hash(&world_recipe));
        let chunk_recipe = ChunkRecipeV1::record_only(&world_recipe);
        assert_eq!(chunk_recipe.topology_id, TopologyId::WrapToroidalExpV1);
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
