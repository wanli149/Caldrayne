use std::sync::{Arc, Mutex};

pub type RuntimeObservabilityInventory = Arc<Mutex<Vec<RuntimeObservabilityStatus>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeObservabilitySurface {
    MetricsExport,
}

impl RuntimeObservabilitySurface {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MetricsExport => "metrics-export",
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeObservabilityStatus {
    pub surface: RuntimeObservabilitySurface,
    pub state: RuntimeObservabilityState,
    pub detail: String,
}

pub fn default_runtime_observability_inventory() -> RuntimeObservabilityInventory {
    Arc::new(Mutex::new(vec![RuntimeObservabilityStatus {
        surface: RuntimeObservabilitySurface::MetricsExport,
        state: RuntimeObservabilityState::Healthy,
        detail: "no metrics export failures observed since startup".to_owned(),
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
    } else {
        entries.push(RuntimeObservabilityStatus {
            surface,
            state,
            detail,
        });
    }
}
