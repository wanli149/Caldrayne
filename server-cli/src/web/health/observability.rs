use std::sync::{Arc, Mutex};

pub type RuntimeObservabilityInventory = Arc<Mutex<Vec<RuntimeObservabilityStatus>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeObservabilitySurface {
    MetricsExport,
    #[cfg(feature = "worldgen")]
    WorldCompat,
}

impl RuntimeObservabilitySurface {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MetricsExport => "metrics-export",
            #[cfg(feature = "worldgen")]
            Self::WorldCompat => "world-compat",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeObservabilityState {
    Healthy,
    Failing,
}

impl RuntimeObservabilityState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Failing => "failing",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum RuntimeObservabilityContext {
    #[default]
    None,
    #[cfg(feature = "worldgen")]
    WorldCompat(WorldCompatObservabilityContext),
}

#[cfg(feature = "worldgen")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldCompatObservabilityContext {
    pub configured_mode: String,
    pub audit: server::CompatAuditV1,
    pub world_recipe_hash: String,
    pub chunk_recipe_hash: String,
    pub topology_id: String,
    pub preset_id: String,
    pub strict_load_contract_gap: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeObservabilityStatus {
    pub surface: RuntimeObservabilitySurface,
    pub state: RuntimeObservabilityState,
    pub detail: String,
    pub context: RuntimeObservabilityContext,
}

pub fn default_runtime_observability_inventory() -> RuntimeObservabilityInventory {
    Arc::new(Mutex::new(vec![RuntimeObservabilityStatus {
        surface: RuntimeObservabilitySurface::MetricsExport,
        state: RuntimeObservabilityState::Healthy,
        detail: "no metrics export failures observed since startup".to_owned(),
        context: RuntimeObservabilityContext::None,
    }]))
}

pub fn snapshot_runtime_observability_inventory(
    runtime_observability_inventory: &RuntimeObservabilityInventory,
) -> Vec<RuntimeObservabilityStatus> {
    match runtime_observability_inventory.lock() {
        Ok(entries) => entries.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

pub(in crate::web) fn set_runtime_observability_status(
    runtime_observability_inventory: &RuntimeObservabilityInventory,
    surface: RuntimeObservabilitySurface,
    state: RuntimeObservabilityState,
    detail: impl Into<String>,
) {
    let detail = detail.into();
    let mut entries = match runtime_observability_inventory.lock() {
        Ok(entries) => entries,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(entry) = entries.iter_mut().find(|entry| entry.surface == surface) {
        entry.state = state;
        entry.detail = detail;
        entry.context = RuntimeObservabilityContext::None;
    } else {
        entries.push(RuntimeObservabilityStatus {
            surface,
            state,
            detail,
            context: RuntimeObservabilityContext::None,
        });
    }
}

#[cfg(feature = "worldgen")]
fn world_compat_requires_operator_review(audit: server::CompatAuditV1) -> bool {
    audit.is_strict_load_contract_gap() || audit.entry == server::CompatEntryKindV1::LoadLegacy
}

#[cfg(feature = "worldgen")]
pub(crate) fn set_world_compat_observability_status(
    runtime_observability_inventory: &RuntimeObservabilityInventory,
    configured_mode: impl Into<String>,
    audit: server::CompatAuditV1,
    recipe_manifest: &server::RecipeManifestV1,
) {
    let configured_mode = configured_mode.into();
    let strict_load_contract_gap = audit.is_strict_load_contract_gap();
    let requires_operator_review = world_compat_requires_operator_review(audit);
    let state = if requires_operator_review {
        RuntimeObservabilityState::Failing
    } else {
        RuntimeObservabilityState::Healthy
    };
    let detail = if strict_load_contract_gap {
        format!(
            "strict world load contract fell back to generation: entry={}, decision={}, failure={}",
            audit.entry.as_str(),
            audit.decision.as_str(),
            audit.failure_kind.as_str()
        )
    } else if audit.entry == server::CompatEntryKindV1::LoadLegacy {
        format!(
            "transitional compat import path remains in use: entry={}, decision={}, failure={}; \
             explicit operator review required until the world is migrated to a strict load \
             contract",
            audit.entry.as_str(),
            audit.decision.as_str(),
            audit.failure_kind.as_str()
        )
    } else {
        format!(
            "world compatibility audit recorded without strict fallback: entry={}, decision={}, \
             failure={}",
            audit.entry.as_str(),
            audit.decision.as_str(),
            audit.failure_kind.as_str()
        )
    };
    let context = RuntimeObservabilityContext::WorldCompat(WorldCompatObservabilityContext {
        configured_mode,
        audit,
        world_recipe_hash: recipe_manifest.world_recipe_hash.clone(),
        chunk_recipe_hash: recipe_manifest.chunk_recipe_hash.clone(),
        topology_id: recipe_manifest.world_recipe.topology_id.as_str().to_owned(),
        preset_id: recipe_manifest.world_recipe.preset_id.as_str().to_owned(),
        strict_load_contract_gap,
    });

    let mut entries = match runtime_observability_inventory.lock() {
        Ok(entries) => entries,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(entry) = entries
        .iter_mut()
        .find(|entry| entry.surface == RuntimeObservabilitySurface::WorldCompat)
    {
        entry.state = state;
        entry.detail = detail;
        entry.context = context;
    } else {
        entries.push(RuntimeObservabilityStatus {
            surface: RuntimeObservabilitySurface::WorldCompat,
            state,
            detail,
            context,
        });
    }
}
