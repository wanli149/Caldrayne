use super::{
    FileLoadContent, FileOpts, GenOpts, LoadOrGenerateSidecarlessMode, LoadedMapContent, ModernMap,
    WorldLoadBootstrap, compat,
};
use crate::recipe::{
    CompatAuditV1, CompatFailureDetailV1, CompatFailureKindV1, CompatFailureSubjectV1,
    RecipeManifestV1,
};
use common::terrain::MapSizeLg;
use tracing::warn;

pub(super) fn build_file_load_content(
    file_opts: &FileOpts,
    requested_gen_opts: Option<GenOpts>,
    compat_resolution: compat::CompatResolved<LoadedMapContent>,
    load_or_generate_sidecarless_mode: LoadOrGenerateSidecarlessMode,
) -> Result<FileLoadContent, compat::CompatResolveError> {
    let loaded_recipe_manifest = compat_resolution
        .parsed_world_file
        .as_ref()
        .and_then(|loaded| loaded.recipe_manifest.clone());
    let loaded_inferred_gen_opts = compat_resolution
        .parsed_world_file
        .as_ref()
        .and_then(|loaded| loaded.inferred_gen_opts.clone());
    let parsed_world_file = compat_resolution.parsed_world_file.map(|loaded| loaded.map);
    let compat_audit = compat_resolution.compat_audit;
    let managed_recipe_sidecar_missing = matches!(file_opts, FileOpts::LoadOrGenerate { .. })
        && parsed_world_file.is_some()
        && loaded_recipe_manifest.is_none();
    if managed_recipe_sidecar_missing
        && matches!(
            load_or_generate_sidecarless_mode,
            LoadOrGenerateSidecarlessMode::Deny
        )
    {
        warn!(
            load_or_generate_sidecarless_mode = %load_or_generate_sidecarless_mode.as_str(),
            map_path = ?file_opts.map_path(),
            "LoadOrGenerate(path) sidecarless managed reuse is disabled by configured gate"
        );
        return Err(compat::CompatResolveError {
            audit: CompatAuditV1::reject(
                compat::entry_kind(file_opts),
                CompatFailureKindV1::PolicyDenied,
                CompatFailureSubjectV1::Options,
                CompatFailureDetailV1::default(),
            ),
        });
    }

    let mut gen_opts = loaded_recipe_manifest
        .as_ref()
        .map(|recipe_manifest| recipe_manifest.world_recipe.gen_opts.clone())
        .or(loaded_inferred_gen_opts)
        .or(requested_gen_opts)
        .unwrap_or_default();

    let map_size_lg = if let Some(map) = &parsed_world_file {
        map_size_lg_from_loaded_map(map)
    } else {
        file_opts.map_size()
    };

    if let Some(map) = &parsed_world_file {
        gen_opts.scale = map.continent_scale_hack;
    };

    Ok(FileLoadContent {
        parsed_world_file,
        loaded_recipe_manifest,
        map_size_lg,
        gen_opts,
        compat_audit,
        managed_recipe_sidecar_missing,
    })
}

pub(super) fn build_world_load_bootstrap(
    seed: u32,
    seed_elements: bool,
    file_load_content: FileLoadContent,
) -> WorldLoadBootstrap {
    let FileLoadContent {
        parsed_world_file,
        loaded_recipe_manifest,
        map_size_lg,
        gen_opts,
        compat_audit,
        managed_recipe_sidecar_missing,
    } = file_load_content;

    // Currently only used with LoadOrGenerate to know if we need to
    // overwrite world file.
    let fresh = parsed_world_file.is_none();
    let recipe_manifest = loaded_recipe_manifest
        .unwrap_or_else(|| RecipeManifestV1::record_only(seed, &gen_opts, seed_elements));
    let effective_seed = recipe_manifest.world_recipe.world_seed;
    let effective_seed_elements = recipe_manifest.world_recipe.seed_elements;

    WorldLoadBootstrap {
        parsed_world_file,
        map_size_lg,
        gen_opts,
        compat_audit,
        managed_recipe_sidecar_missing,
        recipe_manifest,
        fresh,
        effective_seed,
        effective_seed_elements,
    }
}

fn map_size_lg_from_loaded_map(map: &ModernMap) -> MapSizeLg {
    MapSizeLg::new(map.map_size_lg).expect("World size of loaded map does not satisfy invariants.")
}

#[cfg(test)]
mod tests {
    use super::build_world_load_bootstrap;
    use crate::{
        recipe::RecipeManifestV1,
        sim::{CompatAuditV1, FileLoadContent, GenOpts, ModernMap},
    };
    use common::terrain::MapSizeLg;
    use vek::Vec2;

    fn test_map() -> ModernMap {
        ModernMap {
            map_size_lg: Vec2::new(10, 9),
            continent_scale_hack: 3.5,
            alt: vec![0.0; 1].into_boxed_slice(),
            basement: vec![0.0; 1].into_boxed_slice(),
        }
    }

    #[test]
    fn build_world_load_bootstrap_backfills_runtime_recipe_manifest() {
        let gen_opts = GenOpts::default();
        let bootstrap = build_world_load_bootstrap(424242, true, FileLoadContent {
            parsed_world_file: Some(test_map()),
            loaded_recipe_manifest: None,
            map_size_lg: MapSizeLg::new(Vec2::new(10, 9)).unwrap(),
            gen_opts: gen_opts.clone(),
            compat_audit: CompatAuditV1::default(),
            managed_recipe_sidecar_missing: true,
        });

        assert!(!bootstrap.fresh);
        assert!(bootstrap.parsed_world_file.is_some());
        assert!(bootstrap.managed_recipe_sidecar_missing);
        let expected = RecipeManifestV1::record_only(424242, &gen_opts, true);
        assert_eq!(
            bootstrap.recipe_manifest.world_recipe_hash,
            expected.world_recipe_hash
        );
        assert_eq!(
            bootstrap.recipe_manifest.world_recipe.world_seed,
            expected.world_recipe.world_seed
        );
        assert_eq!(bootstrap.effective_seed, 424242);
        assert!(bootstrap.effective_seed_elements);
    }

    #[test]
    fn build_world_load_bootstrap_preserves_loaded_recipe_manifest_contract() {
        let gen_opts = GenOpts::default();
        let recipe_manifest = RecipeManifestV1::record_only(777, &gen_opts, false);
        let bootstrap = build_world_load_bootstrap(424242, true, FileLoadContent {
            parsed_world_file: None,
            loaded_recipe_manifest: Some(recipe_manifest.clone()),
            map_size_lg: MapSizeLg::new(Vec2::new(10, 10)).unwrap(),
            gen_opts,
            compat_audit: CompatAuditV1::default(),
            managed_recipe_sidecar_missing: false,
        });

        assert!(bootstrap.fresh);
        assert!(bootstrap.parsed_world_file.is_none());
        assert_eq!(
            bootstrap.recipe_manifest.world_recipe_hash,
            recipe_manifest.world_recipe_hash
        );
        assert_eq!(
            bootstrap.recipe_manifest.world_recipe.world_seed,
            recipe_manifest.world_recipe.world_seed
        );
        assert_eq!(bootstrap.effective_seed, 777);
        assert!(!bootstrap.effective_seed_elements);
    }
}
