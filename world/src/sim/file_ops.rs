use super::{
    DEFAULT_WORLD_MAP, DEFAULT_WORLD_SEED, GenOpts, ModernMap, compat, default_world_asset_gen_opts,
};
use crate::recipe::{
    CompatFailureDetailV1, CompatFailureKindV1, CompatFailureSubjectV1, RecipeManifestV1,
};
use std::{
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};
use tracing::warn;
use vek::Vec2;

fn structured_load_failure(
    kind: CompatFailureKindV1,
    subject: CompatFailureSubjectV1,
    detail: CompatFailureDetailV1,
) -> compat::RawCompatFailure {
    compat::RawCompatFailure::structured(kind, subject, detail)
}

pub(super) fn recipe_sidecar_path_for_map_path(map_path: &Path) -> PathBuf {
    let mut sidecar_path = map_path.as_os_str().to_owned();
    sidecar_path.push(".recipe.ron");
    PathBuf::from(sidecar_path)
}

pub(super) fn load_recipe_sidecar(
    map_path: &Path,
) -> Result<Option<RecipeManifestV1>, compat::RawCompatFailure> {
    let sidecar_path = recipe_sidecar_path_for_map_path(map_path);
    let file = match File::open(&sidecar_path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            warn!(
                ?e,
                ?sidecar_path,
                "Couldn't read adjacent world recipe sidecar"
            );
            return Err(structured_load_failure(
                CompatFailureKindV1::MissingInput,
                CompatFailureSubjectV1::Recipe,
                CompatFailureDetailV1::default(),
            ));
        },
    };

    let reader = BufReader::new(file);
    match ron::de::from_reader::<_, RecipeManifestV1>(reader) {
        Ok(recipe_manifest) => {
            if !recipe_manifest.validates_record_only_contract() {
                warn!(
                    ?sidecar_path,
                    "Parsed adjacent world recipe sidecar failed internal contract validation"
                );
                return Err(structured_load_failure(
                    CompatFailureKindV1::InvalidWorld,
                    CompatFailureSubjectV1::Recipe,
                    CompatFailureDetailV1::default(),
                ));
            }

            Ok(Some(recipe_manifest))
        },
        Err(e) => {
            warn!(
                ?e,
                ?sidecar_path,
                "Couldn't parse adjacent world recipe sidecar"
            );
            Err(structured_load_failure(
                CompatFailureKindV1::ParseError,
                CompatFailureSubjectV1::Recipe,
                CompatFailureDetailV1::default(),
            ))
        },
    }
}

// Built-in asset manifests describe the runtime contract we enforce for
// read-only asset loads; they are not retroactive provenance for arbitrary
// third-party or historical asset worlds. In particular, the default asset
// keeps its longstanding runtime biome seed while using the asset's
// recorded world-shape gen opts.
pub(super) fn fixed_asset_recipe_manifest(specifier: &str) -> Option<RecipeManifestV1> {
    match specifier {
        DEFAULT_WORLD_MAP => Some(RecipeManifestV1::record_only(
            DEFAULT_WORLD_SEED,
            &default_world_asset_gen_opts(),
            true,
        )),
        _ => None,
    }
}

pub(super) fn load_asset_recipe_manifest(
    specifier: &str,
    map: &ModernMap,
) -> Result<RecipeManifestV1, compat::RawCompatFailure> {
    let recipe_manifest = match fixed_asset_recipe_manifest(specifier) {
        Some(recipe_manifest) => recipe_manifest,
        None => {
            warn!(
                ?specifier,
                compat_failure = %CompatFailureKindV1::MissingInput.as_str(),
                compat_subject = %CompatFailureSubjectV1::Recipe.as_str(),
                "LoadAsset(asset) requires a fixed asset recipe manifest; refusing strict asset load"
            );
            return Err(structured_load_failure(
                CompatFailureKindV1::MissingInput,
                CompatFailureSubjectV1::Recipe,
                CompatFailureDetailV1::default(),
            ));
        },
    };

    if !recipe_manifest.validates_record_only_contract() {
        warn!(
            ?specifier,
            compat_failure = %CompatFailureKindV1::InvalidWorld.as_str(),
            compat_subject = %CompatFailureSubjectV1::Recipe.as_str(),
            "Fixed asset recipe manifest failed internal contract validation"
        );
        return Err(structured_load_failure(
            CompatFailureKindV1::InvalidWorld,
            CompatFailureSubjectV1::Recipe,
            CompatFailureDetailV1::default(),
        ));
    }

    let failure = recipe_manifest_world_contract_failure(map, &recipe_manifest);
    if failure.detail.world_size_mismatch || failure.detail.world_scale_mismatch {
        let stored_gen_opts = &recipe_manifest.world_recipe.gen_opts;
        warn!(
            ?specifier,
            stored_world_recipe_hash = %recipe_manifest.world_recipe_hash,
            stored_topology_id = %recipe_manifest.world_recipe.topology_id.as_str(),
            world_file_size = ?map.map_size_lg,
            recipe_size = ?Vec2::new(stored_gen_opts.x_lg, stored_gen_opts.y_lg),
            world_file_scale = map.continent_scale_hack,
            recipe_scale = stored_gen_opts.scale,
            "LoadAsset(asset) found fixed recipe manifest that does not match the asset world file"
        );
        return Err(failure);
    }

    Ok(recipe_manifest)
}

pub(super) fn recipe_manifest_world_contract_failure(
    map: &ModernMap,
    recipe_manifest: &RecipeManifestV1,
) -> compat::RawCompatFailure {
    let stored_gen_opts = &recipe_manifest.world_recipe.gen_opts;
    let world_size_mismatch =
        map.map_size_lg != Vec2::new(stored_gen_opts.x_lg, stored_gen_opts.y_lg);
    let world_scale_mismatch = map.continent_scale_hack != stored_gen_opts.scale;
    structured_load_failure(
        CompatFailureKindV1::OptionMismatch,
        CompatFailureSubjectV1::Recipe,
        CompatFailureDetailV1::option_mismatch(world_size_mismatch, world_scale_mismatch),
    )
}

pub(super) fn inferred_gen_opts_from_map(map: &ModernMap) -> GenOpts {
    GenOpts {
        x_lg: map.map_size_lg.x,
        y_lg: map.map_size_lg.y,
        scale: map.continent_scale_hack,
        ..GenOpts::default()
    }
}
