#![deny(unsafe_code)]
#![deny(clippy::clone_on_ref_ptr)]

#[cfg(all(
    target_os = "windows",
    not(feature = "hot-agent"),
    not(feature = "hot-site"),
))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// `server-cli` interface commands not to be confused with the commands sent
/// from the client to the server
mod audit;
mod cli;
mod settings;
mod shutdown_coordinator;
mod tui_runner;
mod tuilog;
mod web;
use crate::{
    audit::{AuditAction, AuditOutcome, AuditSource},
    cli::{
        Admin, ArgvApp, ArgvCommand, BenchParams, Message, MessageReturn, SharedCommand, Shutdown,
    },
    settings::{RuntimeGuardInputs, Settings},
    shutdown_coordinator::ShutdownCoordinator,
    tui_runner::Tui,
    tuilog::TuiLog,
};
use common::{
    clock::Clock,
    comp::{ChatType, Player},
    consts::MIN_RECOMMENDED_TOKIO_THREADS,
};
use common_base::span;
use core::sync::atomic::{AtomicUsize, Ordering};
use rand::distr::SampleString;
use server::{Event, Input, Server, persistence::DatabaseSettings, settings::Protocol};
use std::{
    collections::HashMap,
    io,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};
use tokio::sync::Notify;
use tracing::{info, trace, warn};

lazy_static::lazy_static! {
    pub static ref LOG: TuiLog<'static> = TuiLog::default();
}
const TPS: u64 = 30;
type RuntimeListenerAuditState =
    HashMap<(&'static str, SocketAddr), (server::RuntimeListenerState, String)>;
type RuntimeObservabilityAuditState =
    HashMap<&'static str, (web::RuntimeObservabilityState, String)>;
type WebUiRequest = (Message, tokio::sync::oneshot::Sender<MessageReturn>);
type WebUiRequestReceiver = tokio::sync::mpsc::Receiver<WebUiRequest>;

fn append_audit_event_warn(
    audit_log_path: &std::path::Path,
    source: AuditSource,
    action: AuditAction,
    outcome: AuditOutcome,
    detail: &str,
) {
    if let Err(error) = audit::append_event(audit_log_path, source, action, outcome, detail) {
        tracing::warn!(?error, path = %audit_log_path.display(), "Failed to append audit event");
    }
}

fn startup_failure_error(
    audit_log_path: &std::path::Path,
    kind: io::ErrorKind,
    detail: impl Into<String>,
) -> io::Error {
    let detail = detail.into();
    append_audit_event_warn(
        audit_log_path,
        AuditSource::Runtime,
        AuditAction::StartupFailure,
        AuditOutcome::Failed,
        &detail,
    );
    io::Error::new(kind, detail)
}

#[cfg(feature = "worldgen")]
fn compat_audit_summary(audit: server::CompatAuditV1) -> String {
    format!(
        "world compat audit: entry={}, decision={}, failure={}",
        audit.entry.as_str(),
        audit.decision.as_str(),
        audit.failure_kind.as_str()
    )
}

#[cfg(feature = "worldgen")]
fn startup_world_compat_reject_error(
    audit_log_path: &std::path::Path,
    error_display: impl std::fmt::Display,
    audit: server::CompatAuditV1,
) -> io::Error {
    let detail = format!(
        "failed to create server instance: {error_display}; {}",
        compat_audit_summary(audit)
    );
    tracing::error!(
        compat_entry = %audit.entry.as_str(),
        compat_decision = %audit.decision.as_str(),
        compat_failure = %audit.failure_kind.as_str(),
        "dedicated startup failed with world compatibility rejection"
    );
    append_audit_event_warn(
        audit_log_path,
        AuditSource::Runtime,
        AuditAction::WorldCompatStartupReject,
        AuditOutcome::Failed,
        &detail,
    );
    io::Error::new(io::ErrorKind::InvalidData, detail)
}

fn startup_server_error(audit_log_path: &std::path::Path, error: server::Error) -> io::Error {
    #[cfg(feature = "worldgen")]
    if let Some(audit) = error.compat_audit() {
        return startup_world_compat_reject_error(audit_log_path, &error, audit);
    }

    startup_failure_error(
        audit_log_path,
        io::ErrorKind::Other,
        format!("failed to create server instance: {error}"),
    )
}

#[cfg(feature = "worldgen")]
fn observe_startup_compat_fallback(audit_log_path: &std::path::Path, audit: server::CompatAuditV1) {
    if !audit.is_strict_load_contract_gap() {
        return;
    }

    let detail = format!(
        "dedicated startup continued after strict world load contract fallback: {}",
        compat_audit_summary(audit)
    );
    warn!(
        compat_entry = %audit.entry.as_str(),
        compat_decision = %audit.decision.as_str(),
        compat_failure = %audit.failure_kind.as_str(),
        "dedicated startup continued after a strict world load contract fallback; keep this observable before enforce"
    );
    append_audit_event_warn(
        audit_log_path,
        AuditSource::Runtime,
        AuditAction::WorldCompatFallback,
        AuditOutcome::Accepted,
        &detail,
    );
}

#[cfg(feature = "worldgen")]
fn startup_server_pre_web<T, E, M, F>(
    audit_log_path: &std::path::Path,
    server_result: Result<T, E>,
    map_startup_error: M,
    compat_audit: F,
) -> io::Result<T>
where
    M: FnOnce(E) -> io::Error,
    F: FnOnce(&T) -> server::CompatAuditV1,
{
    let server = server_result.map_err(map_startup_error)?;
    observe_startup_compat_fallback(audit_log_path, compat_audit(&server));
    Ok(server)
}

#[cfg(feature = "worldgen")]
fn startup_server_pre_web_then<T, E, M, F, C, R>(
    audit_log_path: &std::path::Path,
    server_result: Result<T, E>,
    map_startup_error: M,
    compat_audit: F,
    continue_startup: C,
) -> io::Result<R>
where
    M: FnOnce(E) -> io::Error,
    F: FnOnce(&T) -> server::CompatAuditV1,
    C: FnOnce(T) -> io::Result<R>,
{
    let server = startup_server_pre_web(
        audit_log_path,
        server_result,
        map_startup_error,
        compat_audit,
    )?;
    continue_startup(server)
}

fn snapshot_runtime_listener_inventory(
    runtime_listener_inventory: &server::RuntimeListenerInventory,
) -> Vec<server::RuntimeListenerStatus> {
    match runtime_listener_inventory.lock() {
        Ok(entries) => entries.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

fn runtime_listener_audit_state(
    entries: &[server::RuntimeListenerStatus],
) -> RuntimeListenerAuditState {
    entries
        .iter()
        .map(|entry| {
            (
                (entry.surface.as_str(), entry.bind_address),
                (entry.state, entry.detail.clone()),
            )
        })
        .collect()
}

fn append_runtime_listener_startup_audit_events(
    audit_log_path: &std::path::Path,
    runtime_listener_inventory: &server::RuntimeListenerInventory,
) -> RuntimeListenerAuditState {
    let entries = snapshot_runtime_listener_inventory(runtime_listener_inventory);

    for entry in &entries {
        if entry.state == server::RuntimeListenerState::Listening {
            continue;
        }

        append_audit_event_warn(
            audit_log_path,
            AuditSource::Runtime,
            AuditAction::StartupFailure,
            AuditOutcome::Failed,
            &format!(
                "runtime listener {} at {} entered {} during startup: {}",
                entry.surface.as_str(),
                entry.bind_address,
                entry.state.as_str(),
                entry.detail
            ),
        );
    }

    runtime_listener_audit_state(&entries)
}

fn append_runtime_listener_transition_audit_events(
    audit_log_path: &std::path::Path,
    runtime_listener_inventory: &server::RuntimeListenerInventory,
    previous_states: &mut RuntimeListenerAuditState,
) {
    let entries = snapshot_runtime_listener_inventory(runtime_listener_inventory);
    let current_states = runtime_listener_audit_state(&entries);

    for entry in &entries {
        if entry.state == server::RuntimeListenerState::Listening {
            continue;
        }

        let key = (entry.surface.as_str(), entry.bind_address);
        let changed = previous_states
            .get(&key)
            .is_none_or(|(state, detail)| *state != entry.state || *detail != entry.detail);
        if !changed {
            continue;
        }

        append_audit_event_warn(
            audit_log_path,
            AuditSource::Runtime,
            AuditAction::RuntimeListenerFailure,
            AuditOutcome::Failed,
            &format!(
                "runtime listener {} at {} entered {} after startup: {}",
                entry.surface.as_str(),
                entry.bind_address,
                entry.state.as_str(),
                entry.detail
            ),
        );
    }

    *previous_states = current_states;
}

fn append_web_runtime_failure_audit_event(
    audit_log_path: &std::path::Path,
    bind_address: Option<SocketAddr>,
    error: &io::Error,
) {
    let bind_address = bind_address
        .map(|address| address.to_string())
        .unwrap_or_else(|| "unknown".to_owned());
    append_audit_event_warn(
        audit_log_path,
        AuditSource::Runtime,
        AuditAction::WebRuntimeFailure,
        AuditOutcome::Failed,
        &format!(
            "web listener {} stopped unexpectedly after startup: {}",
            bind_address, error
        ),
    );
}

fn runtime_observability_audit_state(
    entries: &[web::RuntimeObservabilityStatus],
) -> RuntimeObservabilityAuditState {
    entries
        .iter()
        .map(|entry| (entry.surface.as_str(), (entry.state, entry.detail.clone())))
        .collect()
}

fn append_runtime_observability_transition_audit_events(
    audit_log_path: &std::path::Path,
    runtime_observability_inventory: &web::RuntimeObservabilityInventory,
    previous_states: &mut RuntimeObservabilityAuditState,
) {
    let entries = web::snapshot_runtime_observability_inventory(runtime_observability_inventory);
    let current_states = runtime_observability_audit_state(&entries);

    for entry in &entries {
        if entry.state == web::RuntimeObservabilityState::Healthy {
            continue;
        }

        let key = entry.surface.as_str();
        let changed = previous_states
            .get(key)
            .is_none_or(|(state, detail)| *state != entry.state || *detail != entry.detail);
        if !changed {
            continue;
        }

        append_audit_event_warn(
            audit_log_path,
            AuditSource::Runtime,
            AuditAction::ObservabilityRuntimeFailure,
            AuditOutcome::Failed,
            &format!(
                "observability surface {} entered {}: {}",
                entry.surface.as_str(),
                entry.state.as_str(),
                entry.detail
            ),
        );
    }

    *previous_states = current_states;
}

pub(crate) fn startup_runtime_observability_inventory(
    chunk_lifecycle_summary: Option<server::ChunkLifecycleAbnormalSummary>,
) -> web::RuntimeObservabilityInventory {
    let runtime_observability_inventory = web::default_runtime_observability_inventory();
    web::set_chunk_lifecycle_observability_status(
        &runtime_observability_inventory,
        chunk_lifecycle_summary,
    );
    runtime_observability_inventory
}

#[cfg(feature = "worldgen")]
#[derive(Clone, Copy)]
pub(crate) struct StartupWorldCompatObservability<'a> {
    pub configured_mode: &'a str,
    pub load_legacy_mode: &'a str,
    pub load_or_generate_sidecarless_mode: &'a str,
    pub compat_audit: server::CompatAuditV1,
    pub recipe_manifest: &'a server::RecipeManifestV1,
    pub managed_recipe_sidecar_missing: bool,
}

#[cfg(feature = "worldgen")]
pub(crate) fn apply_startup_world_compat_observability(
    runtime_observability_inventory: &web::RuntimeObservabilityInventory,
    world_compat: StartupWorldCompatObservability<'_>,
) {
    web::set_world_compat_observability_status(
        runtime_observability_inventory,
        world_compat.configured_mode,
        world_compat.load_legacy_mode,
        world_compat.load_or_generate_sidecarless_mode,
        world_compat.compat_audit,
        world_compat.recipe_manifest,
        world_compat.managed_recipe_sidecar_missing,
    );
}

#[cfg(feature = "worldgen")]
struct PreparedWorldgenStartup {
    server: Server,
    registry: Arc<prometheus::Registry>,
    chat: server::chat::ChatCache,
    runtime_listener_inventory: server::RuntimeListenerInventory,
    runtime_listener_audit_state: RuntimeListenerAuditState,
    runtime_observability_inventory: web::RuntimeObservabilityInventory,
    runtime_observability_audit_state: RuntimeObservabilityAuditState,
    health_state: web::HealthState,
}

struct PreparedRuntimeInputs {
    runtime_layout: crate::settings::RuntimeLayout,
    audit_log_path: PathBuf,
    server_data_dir: PathBuf,
    runtime: Arc<tokio::runtime::Runtime>,
    server_identity: server::ServerIdentity,
    server_settings: server::Settings,
    editable_settings: server::EditableSettings,
    database_settings: server::persistence::DatabaseSettings,
}

struct PreparedServerLoopHandoff {
    server: Server,
    metrics_shutdown: Arc<Notify>,
    web_server_task: tokio::task::JoinHandle<()>,
    web_bind_address: Option<SocketAddr>,
    web_ui_request_r: WebUiRequestReceiver,
    runtime_listener_inventory: server::RuntimeListenerInventory,
    runtime_listener_audit_state: RuntimeListenerAuditState,
    runtime_observability_inventory: web::RuntimeObservabilityInventory,
    runtime_observability_audit_state: RuntimeObservabilityAuditState,
}

pub(crate) fn startup_health_state(
    settings: &Settings,
    runtime_layout: &crate::settings::RuntimeLayout,
    runtime_listener_inventory: server::RuntimeListenerInventory,
    runtime_observability_inventory: web::RuntimeObservabilityInventory,
    auth_server_configured: bool,
    authoritative_auth_provider: Option<String>,
    surface_inventory: Vec<crate::settings::RuntimeSurface>,
    management_auth_inventory: Vec<crate::settings::ManagementAuthInventoryEntry>,
    transport_security_inventory: Vec<crate::settings::TransportSecurityInventoryEntry>,
    governance_findings: Vec<crate::settings::RuntimeGovernanceFinding>,
) -> web::HealthState {
    web::HealthState {
        environment: settings.environment.as_str(),
        auth_server_configured,
        authoritative_auth_provider,
        server_state: runtime_layout.server_state.clone(),
        recovery_staging_state: runtime_layout.recovery_staging_state.clone(),
        audit_retention: settings.audit_retention,
        runtime_listener_inventory,
        runtime_observability_inventory,
        surface_inventory,
        management_auth_inventory,
        transport_security_inventory,
        governance_findings,
    }
}

fn resolved_ui_api_secret(settings: &Settings) -> String {
    settings.ui_api_secret.clone().unwrap_or_else(|| {
        // When no secret is provided we generate one that we distribute via the
        // loopback-only /ui bootstrap endpoint.
        use rand::distr::Alphanumeric;
        Alphanumeric.sample_string(&mut rand::rng(), 32)
    })
}

fn bind_startup_web_listener(
    settings: &Settings,
    runtime: &tokio::runtime::Runtime,
    audit_log_path: &std::path::Path,
) -> io::Result<tokio::net::TcpListener> {
    runtime
        .block_on(web::bind_listener(settings.web_address))
        .map_err(|error| {
            startup_failure_error(
                audit_log_path,
                error.kind(),
                format!(
                    "failed to bind web listener on {}: {error}",
                    settings.web_address
                ),
            )
        })
}

#[cfg(feature = "worldgen")]
fn prepare_worldgen_startup(
    settings: &Settings,
    runtime_layout: &crate::settings::RuntimeLayout,
    audit_log_path: &std::path::Path,
    server_settings: server::Settings,
    editable_settings: server::EditableSettings,
    server_identity: server::ServerIdentity,
    database_settings: server::persistence::DatabaseSettings,
    server_data_dir: &std::path::Path,
    runtime: Arc<tokio::runtime::Runtime>,
) -> io::Result<PreparedWorldgenStartup> {
    startup_server_pre_web_then(
        audit_log_path,
        Server::new(
            server_settings,
            editable_settings,
            server_identity,
            database_settings,
            server_data_dir,
            &|_| {},
            Arc::clone(&runtime),
        ),
        |error| startup_server_error(audit_log_path, error),
        |server| server.world().sim().compat_audit(),
        |server| {
            let registry = Arc::clone(server.metrics_registry());
            let chat = server.chat_cache().clone();
            let runtime_listener_inventory = server.runtime_listener_inventory();
            let runtime_listener_audit_state = append_runtime_listener_startup_audit_events(
                audit_log_path,
                &runtime_listener_inventory,
            );
            let runtime_observability_inventory =
                startup_runtime_observability_inventory(server.chunk_lifecycle_abnormal_summary());
            apply_startup_world_compat_observability(
                &runtime_observability_inventory,
                StartupWorldCompatObservability {
                    configured_mode: server.world().sim().compat_mode().as_str(),
                    load_legacy_mode: server.world().sim().load_legacy_mode().as_str(),
                    load_or_generate_sidecarless_mode: server
                        .world()
                        .sim()
                        .load_or_generate_sidecarless_mode()
                        .as_str(),
                    compat_audit: server.world().sim().compat_audit(),
                    recipe_manifest: server.world().sim().recipe_manifest(),
                    managed_recipe_sidecar_missing: server
                        .world()
                        .sim()
                        .managed_recipe_sidecar_missing(),
                },
            );
            let runtime_observability_audit_state = runtime_observability_audit_state(
                &web::snapshot_runtime_observability_inventory(&runtime_observability_inventory),
            );

            let effective_server_settings = server.settings();
            let auth_server_configured = effective_server_settings.auth_server_address.is_some();
            let authoritative_auth_provider = effective_server_settings.auth_server_address.clone();
            let surface_inventory = settings.surface_inventory(&effective_server_settings);
            let management_auth_inventory =
                settings.management_auth_inventory(&effective_server_settings);
            let transport_security_inventory =
                settings.transport_security_inventory(&effective_server_settings);
            let governance_findings = settings.governance_findings(&effective_server_settings);
            drop(effective_server_settings);

            let health_state = startup_health_state(
                settings,
                runtime_layout,
                Arc::clone(&runtime_listener_inventory),
                runtime_observability_inventory.clone(),
                auth_server_configured,
                authoritative_auth_provider,
                surface_inventory,
                management_auth_inventory,
                transport_security_inventory,
                governance_findings,
            );

            Ok(PreparedWorldgenStartup {
                server,
                registry,
                chat,
                runtime_listener_inventory,
                runtime_listener_audit_state,
                runtime_observability_inventory,
                runtime_observability_audit_state,
                health_state,
            })
        },
    )
}

fn prepare_runtime_inputs(
    settings: &Settings,
    runtime_layout: crate::settings::RuntimeLayout,
    no_auth: bool,
    sql_log_mode: server::persistence::SqlLogMode,
) -> io::Result<PreparedRuntimeInputs> {
    info!(
        "Using userdata folder at {}",
        runtime_layout.userdata_dir.display()
    );
    let audit_log_path = runtime_layout.server_state.audit_log_file.clone();
    info!(
        server_data_dir = %runtime_layout.server_state.data_dir.display(),
        server_config_dir = %runtime_layout.server_state.config_dir.display(),
        server_identity_file = %runtime_layout.server_state.identity_file.display(),
        server_database_file = %runtime_layout.server_state.database_file.display(),
        rtsim_data_file = %runtime_layout.server_state.rtsim_data_file.display(),
        terrain_persistence_dir = %runtime_layout.server_state.terrain_dir.display(),
        recovery_staging_dir = %runtime_layout.recovery_staging_state.data_dir.display(),
        "Resolved server runtime layout"
    );
    for entry in runtime_layout.server_state.inventory() {
        info!(?entry.kind, ?entry.recovery, path = %entry.path.display(), "Server state inventory entry");
    }
    std::fs::create_dir_all(&runtime_layout.recovery_staging_state.data_dir).map_err(|error| {
        io::Error::other(format!(
            "failed to create recovery staging directory {}: {error}",
            runtime_layout.recovery_staging_state.data_dir.display()
        ))
    })?;
    let recovery_overlap_details = settings::recovery_drill_overlap_details(
        &runtime_layout.server_state,
        &runtime_layout.recovery_staging_state,
    );
    if !recovery_overlap_details.is_empty() {
        for detail in &recovery_overlap_details {
            tracing::error!("{detail}");
        }
        return Err(startup_failure_error(
            &audit_log_path,
            io::ErrorKind::InvalidInput,
            "refusing to start with overlapping recovery staging layout",
        ));
    }
    let runtime_state_layout_conflicts =
        settings::runtime_state_layout_conflict_details(&runtime_layout.server_state);
    if !runtime_state_layout_conflicts.is_empty() {
        for detail in &runtime_state_layout_conflicts {
            tracing::error!("{detail}");
        }
        return Err(startup_failure_error(
            &audit_log_path,
            io::ErrorKind::InvalidInput,
            "refusing to start with overlapping runtime state layout",
        ));
    }
    let server_data_dir = runtime_layout.server_state.data_dir.clone();
    let server_settings_file_probe = server::settings::inspect_settings_file(&server_data_dir);
    if !settings.environment.allows_optional_auth() && !server_settings_file_probe.is_ready() {
        tracing::error!("{}", server_settings_file_probe.detail());
        return Err(startup_failure_error(
            &audit_log_path,
            io::ErrorKind::InvalidInput,
            format!(
                "Refusing to start in {} environment without a valid server settings file",
                settings.environment.as_str()
            ),
        ));
    }
    let server_identity_file_probe = server::settings::inspect_identity_file(&server_data_dir);
    if !settings.environment.allows_optional_auth() && !server_identity_file_probe.is_ready() {
        tracing::error!("{}", server_identity_file_probe.detail());
        return Err(startup_failure_error(
            &audit_log_path,
            io::ErrorKind::InvalidInput,
            format!(
                "Refusing to start in {} environment without a valid server identity file",
                settings.environment.as_str()
            ),
        ));
    }
    let server_database_file_probe =
        server::persistence::inspect_database_file(&runtime_layout.server_state.database_file);
    if !settings.environment.allows_optional_auth() && !server_database_file_probe.is_ready() {
        tracing::error!("{}", server_database_file_probe.detail());
        return Err(startup_failure_error(
            &audit_log_path,
            io::ErrorKind::InvalidInput,
            format!(
                "Refusing to start in {} environment without a valid server database file",
                settings.environment.as_str()
            ),
        ));
    }
    let audit_maintenance =
        audit::apply_retention_policy(&audit_log_path, settings.audit_retention).map_err(
            |error| io::Error::other(format!("failed to apply audit retention policy: {error}")),
        )?;
    info!(
        audit_log_path = %audit_log_path.display(),
        max_active_file_mebibytes = settings.audit_retention.max_active_file_mebibytes,
        max_archive_files = settings.audit_retention.max_archive_files,
        retained_archives = audit_maintenance.retained_archives,
        "Applied audit retention policy"
    );
    if let Some(rotated_to) = audit_maintenance.rotated_to.as_ref() {
        info!(
            rotated_to = %rotated_to.display(),
            "Rotated oversized active audit log before startup"
        );
    }
    for deleted_archive in &audit_maintenance.deleted_archives {
        info!(
            deleted_archive = %deleted_archive.display(),
            "Pruned old audit archive due to retention policy"
        );
    }

    // We don't need that many threads in the async pool, at least 2 but generally
    // 25% of all available will do
    // TODO: evaluate std::thread::available_concurrency as a num_cpus replacement
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads((num_cpus::get() / 4).max(MIN_RECOMMENDED_TOKIO_THREADS))
            .thread_name_fn(|| {
                static ATOMIC_ID: AtomicUsize = AtomicUsize::new(0);
                let id = ATOMIC_ID.fetch_add(1, Ordering::SeqCst);
                format!("tokio-server-{}", id)
            })
            .build()
            .unwrap(),
    );

    #[cfg(feature = "hot-agent")]
    {
        agent::init();
    }
    #[cfg(feature = "hot-site")]
    {
        world::init();
    }

    let server_identity = server::ServerIdentity::load(&server_data_dir);
    let mut server_settings = server::Settings::load(&server_data_dir);
    let editable_settings = server::EditableSettings::load(&server_data_dir);
    server_settings.runtime_environment = match settings.environment {
        settings::Environment::Local => server::settings::RuntimeEnvironment::Local,
        settings::Environment::Test => server::settings::RuntimeEnvironment::Test,
        settings::Environment::Production => server::settings::RuntimeEnvironment::Production,
    };

    if no_auth {
        server_settings.auth_server_address = None;
    }

    if let Err(errors) = settings.validate_runtime(RuntimeGuardInputs {
        no_auth_cli: no_auth,
        auth_server_configured: server_settings.auth_server_address.is_some(),
        web_address: settings.web_address,
        query_address: server_settings.query_address,
        quic_bindings: server::settings::QuicBinding::from_protocols(
            &server_settings.gameserver_protocols,
        ),
    }) {
        for error in errors {
            tracing::error!("{error}");
        }

        return Err(startup_failure_error(
            &audit_log_path,
            io::ErrorKind::InvalidInput,
            format!(
                "Refusing to start with invalid {} environment runtime configuration",
                settings.environment.as_str()
            ),
        ));
    }

    let database_settings = DatabaseSettings {
        db_dir: runtime_layout.server_state.database_dir.clone(),
        sql_log_mode,
    };

    Ok(PreparedRuntimeInputs {
        runtime_layout,
        audit_log_path,
        server_data_dir,
        runtime,
        server_identity,
        server_settings,
        editable_settings,
        database_settings,
    })
}

fn prepare_server_loop_handoff(
    settings: &Settings,
    runtime_layout: &crate::settings::RuntimeLayout,
    audit_log_path: &std::path::Path,
    server_settings: server::Settings,
    editable_settings: server::EditableSettings,
    server_identity: server::ServerIdentity,
    database_settings: server::persistence::DatabaseSettings,
    server_data_dir: &std::path::Path,
    runtime: Arc<tokio::runtime::Runtime>,
) -> io::Result<PreparedServerLoopHandoff> {
    #[cfg(feature = "worldgen")]
    let (
        server,
        registry,
        chat,
        runtime_listener_inventory,
        runtime_listener_audit_state,
        runtime_observability_inventory,
        runtime_observability_audit_state,
    ) = {
        let PreparedWorldgenStartup {
            server,
            registry,
            chat,
            runtime_listener_inventory,
            runtime_listener_audit_state,
            runtime_observability_inventory,
            runtime_observability_audit_state,
            health_state: _,
        } = prepare_worldgen_startup(
            settings,
            runtime_layout,
            audit_log_path,
            server_settings,
            editable_settings,
            server_identity,
            database_settings,
            server_data_dir,
            Arc::clone(&runtime),
        )?;

        (
            server,
            registry,
            chat,
            runtime_listener_inventory,
            runtime_listener_audit_state,
            runtime_observability_inventory,
            runtime_observability_audit_state,
        )
    };

    #[cfg(not(feature = "worldgen"))]
    let (
        server,
        registry,
        chat,
        runtime_listener_inventory,
        runtime_listener_audit_state,
        runtime_observability_inventory,
        runtime_observability_audit_state,
    ) = {
        #[cfg_attr(not(feature = "worldgen"), expect(unused_mut))]
        let mut server = Server::new(
            server_settings,
            editable_settings,
            server_identity,
            database_settings,
            server_data_dir,
            &|_| {},
            Arc::clone(&runtime),
        )
        .map_err(|error| startup_server_error(audit_log_path, error))?;

        let registry = Arc::clone(server.metrics_registry());
        let chat = server.chat_cache().clone();
        let runtime_listener_inventory = server.runtime_listener_inventory();
        let runtime_listener_audit_state = append_runtime_listener_startup_audit_events(
            audit_log_path,
            &runtime_listener_inventory,
        );
        let runtime_observability_inventory =
            startup_runtime_observability_inventory(server.chunk_lifecycle_abnormal_summary());
        let runtime_observability_audit_state = runtime_observability_audit_state(
            &web::snapshot_runtime_observability_inventory(&runtime_observability_inventory),
        );

        (
            server,
            registry,
            chat,
            runtime_listener_inventory,
            runtime_listener_audit_state,
            runtime_observability_inventory,
            runtime_observability_audit_state,
        )
    };

    let metrics_shutdown = Arc::new(Notify::new());
    let metrics_shutdown_clone = Arc::clone(&metrics_shutdown);
    let web_chat_secret = settings.web_chat_secret.clone();
    let ui_api_secret = resolved_ui_api_secret(settings);

    let (
        auth_server_configured,
        authoritative_auth_provider,
        surface_inventory,
        management_auth_inventory,
        transport_security_inventory,
        governance_findings,
    ) = {
        let effective_server_settings = server.settings();
        let surface_inventory = settings.surface_inventory(&effective_server_settings);
        for surface in &surface_inventory {
            let bind_address = surface
                .bind_address
                .map(|address| address.to_string())
                .unwrap_or_else(|| "disabled".to_owned());
            info!(
                environment = settings.environment.as_str(),
                surface = surface.name,
                bind_address,
                reachability = surface.reachability.as_str(),
                auth = surface.auth.as_str(),
                credential_bootstrap = surface.credential_bootstrap.as_str(),
                review_status = surface.review_status.as_str(),
                remote_exposure_policy = surface.remote_exposure_policy.as_str(),
                purpose = surface.purpose.as_str(),
                consumption = surface.consumption.as_str(),
                "Runtime surface inventory entry"
            );

            if !matches!(settings.environment, settings::Environment::Local)
                && matches!(
                    surface.reachability,
                    settings::SurfaceReachability::NetworkAccessible
                )
            {
                if let Some(message) = surface.review_status.network_access_warning() {
                    warn!(
                        environment = settings.environment.as_str(),
                        surface = surface.name,
                        review_status = surface.review_status.as_str(),
                        "{message}"
                    );
                }
            }
        }

        for config in settings.sensitive_config_inventory(&effective_server_settings) {
            let bind_address = config
                .bind_address
                .map(|address| address.to_string())
                .unwrap_or_else(|| "n/a".to_owned());
            let file_path = config
                .file_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "inline".to_owned());
            info!(
                environment = settings.environment.as_str(),
                config_id = config.id,
                consumer_surface = config.consumer_surface,
                bind_address,
                file_path,
                configured = config.configured,
                sensitivity = config.sensitivity.as_str(),
                source = config.source.as_str(),
                value_state = config.value_state.as_str(),
                operator_responsibility = config.operator_responsibility.as_str(),
                exposure_dependency = config.exposure_dependency.as_str(),
                "Sensitive config inventory entry"
            );
        }

        let management_auth_inventory =
            settings.management_auth_inventory(&effective_server_settings);
        for entry in &management_auth_inventory {
            let bind_address = entry
                .bind_address
                .map(|address| address.to_string())
                .unwrap_or_else(|| "disabled".to_owned());
            let secret_config_id = entry.secret_config_id.unwrap_or("n/a");
            info!(
                environment = settings.environment.as_str(),
                surface = entry.surface,
                bind_address,
                reachability = entry.reachability.as_str(),
                review_status = entry.review_status.as_str(),
                remote_exposure_policy = entry.remote_exposure_policy.as_str(),
                capability = entry.capability.as_str(),
                auth_scheme = entry.auth_scheme.as_str(),
                credential_bootstrap = entry.credential_bootstrap.as_str(),
                credential_transport = entry.credential_transport.as_str(),
                secret_config_id,
                proxy_forwarding_forbidden = entry.proxy_forwarding_forbidden,
                detail = entry.detail.as_str(),
                "Management auth inventory entry"
            );
        }

        let transport_security_inventory =
            settings.transport_security_inventory(&effective_server_settings);
        for entry in &transport_security_inventory {
            info!(
                environment = settings.environment.as_str(),
                surface = entry.surface,
                bind_address = %entry.bind_address,
                transport = entry.transport,
                encryption = entry.encryption,
                cert_file_path = %entry.cert_file_path.display(),
                key_file_path = %entry.key_file_path.display(),
                rollout_policy = entry.rollout_policy.as_str(),
                validation_policy = entry.validation_policy.as_str(),
                material_state = entry.material_state.as_str(),
                detail = entry.detail.as_str(),
                "Transport security inventory entry"
            );
        }

        let governance_findings = settings.governance_findings(&effective_server_settings);
        for finding in &governance_findings {
            match finding.severity {
                settings::RuntimeGovernanceSeverity::Notice => info!(
                    environment = settings.environment.as_str(),
                    finding_id = finding.id,
                    severity = finding.severity.as_str(),
                    subject = finding.subject,
                    detail = finding.detail.as_str(),
                    "Runtime governance finding"
                ),
                settings::RuntimeGovernanceSeverity::Warning => warn!(
                    environment = settings.environment.as_str(),
                    finding_id = finding.id,
                    severity = finding.severity.as_str(),
                    subject = finding.subject,
                    detail = finding.detail.as_str(),
                    "Runtime governance finding"
                ),
            }
        }

        let quic_bindings = server::settings::QuicBinding::from_protocols(
            &effective_server_settings.gameserver_protocols,
        );
        for binding in &quic_bindings {
            info!(
                environment = settings.environment.as_str(),
                quic_address = %binding.address,
                quic_cert_file = %binding.cert_file_path.display(),
                quic_key_file = %binding.key_file_path.display(),
                "QUIC transport configured"
            );
            if !matches!(settings.environment, settings::Environment::Local) {
                warn!(
                    environment = settings.environment.as_str(),
                    quic_address = %binding.address,
                    "QUIC is experimental in the current codebase. Keep it under explicit rollout and \
                     certificate governance."
                );
            }
        }

        (
            effective_server_settings.auth_server_address.is_some(),
            effective_server_settings.auth_server_address.clone(),
            surface_inventory,
            management_auth_inventory,
            transport_security_inventory,
            governance_findings,
        )
    };

    let (web_ui_request_s, web_ui_request_r) = tokio::sync::mpsc::channel(1000);
    let web_listener = bind_startup_web_listener(settings, &runtime, audit_log_path)?;
    let health_state = startup_health_state(
        settings,
        runtime_layout,
        Arc::clone(&runtime_listener_inventory),
        runtime_observability_inventory.clone(),
        auth_server_configured,
        authoritative_auth_provider,
        surface_inventory,
        management_auth_inventory,
        transport_security_inventory,
        governance_findings,
    );
    let web_bind_address = web_listener.local_addr().ok();
    let audit_log_path_for_web = audit_log_path.to_path_buf();

    let web_server_task = runtime.spawn(async move {
        let result = web::run_with_listener(
            registry,
            chat,
            web_chat_secret,
            ui_api_secret,
            web_ui_request_s,
            health_state,
            web_listener,
            metrics_shutdown_clone.notified(),
        )
        .await;
        match result {
            Ok(()) => tracing::debug!("webserver shutdown successful"),
            Err(error) => {
                tracing::error!(
                    ?error,
                    bind_address = ?web_bind_address,
                    "webserver shutdown error"
                );
                append_web_runtime_failure_audit_event(
                    &audit_log_path_for_web,
                    web_bind_address,
                    &error,
                );
            },
        }
    });

    Ok(PreparedServerLoopHandoff {
        server,
        metrics_shutdown,
        web_server_task,
        web_bind_address,
        web_ui_request_r,
        runtime_listener_inventory,
        runtime_listener_audit_state,
        runtime_observability_inventory,
        runtime_observability_audit_state,
    })
}

fn main() -> io::Result<()> {
    #[cfg(feature = "tracy")]
    common_base::tracy_client::Client::start();

    use clap::Parser;
    let app = ArgvApp::parse();

    let basic = !app.tui || app.command.is_some();
    let noninteractive = app.non_interactive;
    let no_auth = app.no_auth;
    let sql_log_mode = app.sql_log_mode;

    // noninteractive implies basic
    let basic = basic || noninteractive;

    let shutdown_signal = Arc::new(AtomicBool::new(false));

    let (_guards, _guards2) = if basic {
        (Vec::new(), common_frontend::init_stdout(None))
    } else {
        (common_frontend::init(None, &|| LOG.clone()), Vec::new())
    };

    // Load settings
    let settings = settings::Settings::load().ok_or(io::ErrorKind::Other)?;

    #[cfg(not(target_os = "windows"))]
    {
        for signal in &settings.shutdown_signals {
            let _ = signal_hook::flag::register(signal.to_signal(), Arc::clone(&shutdown_signal));
        }
    }

    #[cfg(target_os = "windows")]
    if !settings.shutdown_signals.is_empty() {
        tracing::warn!(
            "Server configuration contains shutdown signals, but your platform does not support \
             them"
        );
    }

    let PreparedRuntimeInputs {
        runtime_layout,
        audit_log_path,
        server_data_dir,
        runtime,
        server_identity,
        mut server_settings,
        mut editable_settings,
        database_settings,
    } = prepare_runtime_inputs(
        &settings,
        settings.resolve_runtime_layout(),
        no_auth,
        sql_log_mode,
    )?;

    let mut bench = None;
    if let Some(command) = app.command {
        match command {
            ArgvCommand::Shared(SharedCommand::Admin { command }) => {
                let login_provider = server::login_provider::LoginProvider::new(
                    server_settings.auth_server_address,
                    runtime,
                );

                return match command {
                    Admin::Add { username, role } => {
                        // FIXME: Currently the UUID can get returned even if the file didn't
                        // change, so this can't be relied on as an error
                        // code; moreover, we do nothing with the UUID
                        // returned in the success case.  Fix the underlying function to return
                        // enough information that we can reliably return an error code.
                        let _ = server::add_admin(
                            &username,
                            role,
                            &login_provider,
                            &mut editable_settings,
                            &server_data_dir,
                        );
                        append_audit_event_warn(
                            &audit_log_path,
                            AuditSource::Argv,
                            AuditAction::AdminAdd,
                            AuditOutcome::Accepted,
                            &format!("username={username} role={role:?}"),
                        );
                        Ok(())
                    },
                    Admin::Remove { username } => {
                        // FIXME: Currently the UUID can get returned even if the file didn't
                        // change, so this can't be relied on as an error
                        // code; moreover, we do nothing with the UUID
                        // returned in the success case.  Fix the underlying function to return
                        // enough information that we can reliably return an error code.
                        let _ = server::remove_admin(
                            &username,
                            &login_provider,
                            &mut editable_settings,
                            &server_data_dir,
                        );
                        append_audit_event_warn(
                            &audit_log_path,
                            AuditSource::Argv,
                            AuditAction::AdminRemove,
                            AuditOutcome::Accepted,
                            &format!("username={username}"),
                        );
                        Ok(())
                    },
                };
            },
            ArgvCommand::Bench(params) => {
                bench = Some(params);
                // If we are trying to benchmark, don't limit the server view distance.
                server_settings.max_view_distance = None;
                // TODO: add setting to adjust wildlife spawn density, note I
                // tried but Index setup makes it a bit
                // annoying, might require a more involved refactor to get
                // working nicely
            },
        };
    }

    // Panic hook to ensure that console mode is set back correctly if in non-basic
    // mode
    if !basic {
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            Tui::shutdown(basic);
            hook(info);
        }));
    }

    let tui = (!noninteractive).then(|| Tui::run(basic));

    info!("Starting server...");
    info!(
        environment = settings.environment.as_str(),
        "Server CLI environment selected"
    );

    let protocols_and_addresses = server_settings.gameserver_protocols.clone();
    let web_port = &settings.web_address.port();
    let PreparedServerLoopHandoff {
        mut server,
        metrics_shutdown,
        web_server_task: _web_server_task,
        web_bind_address: _web_bind_address,
        web_ui_request_r,
        runtime_listener_inventory,
        runtime_listener_audit_state,
        runtime_observability_inventory,
        runtime_observability_audit_state,
    } = prepare_server_loop_handoff(
        &settings,
        &runtime_layout,
        &audit_log_path,
        server_settings,
        editable_settings,
        server_identity,
        database_settings,
        &server_data_dir,
        runtime,
    )?;

    // Collect addresses that the server is listening to log.
    let gameserver_addresses = protocols_and_addresses
        .into_iter()
        .map(|protocol| match protocol {
            Protocol::Tcp { address } => ("TCP", address),
            Protocol::Quic {
                address,
                cert_file_path: _,
                key_file_path: _,
            } => ("QUIC", address),
        });

    info!(
        ?web_port,
        ?gameserver_addresses,
        "Server startup completed; verify /health/listeners and /health/preflight before treating \
         the instance as rollout-ready."
    );

    #[cfg(feature = "worldgen")]
    if let Some(bench) = bench {
        server.create_centered_persister(bench.view_distance);
    }

    server_loop(
        server,
        bench,
        settings,
        tui,
        web_ui_request_r,
        audit_log_path,
        shutdown_signal,
        runtime_listener_inventory,
        runtime_listener_audit_state,
        runtime_observability_inventory,
        runtime_observability_audit_state,
    )?;

    metrics_shutdown.notify_one();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "worldgen")]
    use server::{CompatAuditV1, CompatEntryKindV1, CompatFailureKindV1};
    use std::{
        cell::{Cell, RefCell},
        fs,
        net::SocketAddr,
        path::Path,
        sync::{Arc, Mutex},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
    #[cfg(feature = "worldgen")]
    #[derive(Clone, Copy)]
    struct StartupCompatTestState {
        audit: CompatAuditV1,
    }

    #[cfg(feature = "worldgen")]
    #[derive(Clone, Copy)]
    struct StartupCompatTestError {
        audit: CompatAuditV1,
    }

    fn unique_temp_path() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("caldrayne-startup-audit-{unique}"))
            .join("audit-log.ronl")
    }

    fn unique_temp_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("caldrayne-startup-web-{unique}"))
    }

    fn seed_identity_file(identity_file: &Path) {
        let identity = ron::ser::to_string_pretty(
            &server::ServerIdentity::default(),
            ron::ser::PrettyConfig::default(),
        )
        .expect("should serialize server identity");
        fs::write(identity_file, identity).expect("should write identity file");
    }

    fn seed_database_file(database_dir: &Path) {
        server::persistence::run_migrations(&server::persistence::DatabaseSettings {
            db_dir: database_dir.to_path_buf(),
            sql_log_mode: server::persistence::SqlLogMode::Disabled,
        });
    }

    fn write_settings_file(data_dir: &Path, settings: &server::Settings) {
        let settings_path = server::settings::settings_file_path(data_dir);
        let settings = ron::ser::to_string_pretty(settings, ron::ser::PrettyConfig::default())
            .expect("should serialize server settings");
        fs::write(&settings_path, settings).expect("should write settings file");
    }

    fn seed_settings_file(data_dir: &Path) {
        write_settings_file(data_dir, &server::Settings::default());
    }

    fn seed_live_runtime_state(state: &server::ServerStatePaths) {
        seed_live_runtime_state_with_settings(state, &server::Settings::default());
    }

    fn seed_live_runtime_state_with_settings(
        state: &server::ServerStatePaths,
        settings: &server::Settings,
    ) {
        fs::create_dir_all(&state.config_dir).expect("should create config dir");
        fs::create_dir_all(&state.ops_dir).expect("should create ops dir");
        seed_identity_file(&state.identity_file);
        seed_database_file(&state.database_dir);
        write_settings_file(&state.data_dir, settings);
    }

    fn seed_recovery_staging_restore_state(recovery_staging_state: &server::ServerStatePaths) {
        fs::create_dir_all(&recovery_staging_state.data_dir)
            .expect("should create recovery staging dir");
        fs::create_dir_all(&recovery_staging_state.config_dir)
            .expect("should create recovery staging config dir");
        seed_identity_file(&recovery_staging_state.identity_file);
        seed_database_file(&recovery_staging_state.database_dir);
        seed_settings_file(&recovery_staging_state.data_dir);
    }

    fn http_response_body(response: &str) -> &str {
        response
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .unwrap_or("")
    }

    async fn fetch_http_response(addr: SocketAddr, path: &str) -> io::Result<String> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut last_error = None;
        for _attempt in 0..20 {
            match tokio::net::TcpStream::connect(addr).await {
                Ok(mut stream) => {
                    let request =
                        format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
                    stream.write_all(request.as_bytes()).await?;
                    stream.flush().await?;

                    let mut response = Vec::new();
                    stream.read_to_end(&mut response).await?;
                    return Ok(String::from_utf8_lossy(&response).into_owned());
                },
                Err(error) => {
                    last_error = Some(error);
                    tokio::time::sleep(Duration::from_millis(25)).await;
                },
            }
        }

        Err(last_error.unwrap_or_else(|| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                format!("timed out waiting for HTTP listener at {addr}"),
            )
        }))
    }

    #[cfg(feature = "worldgen")]
    #[test]
    fn startup_server_error_records_world_compat_reject_as_invalid_data_audit() {
        let path = unique_temp_path();
        let audit = CompatAuditV1::fallback_generate(
            CompatEntryKindV1::LoadLegacy,
            CompatFailureKindV1::PolicyDenied,
        );
        let startup_error =
            startup_world_compat_reject_error(&path, "World Error: compat reject", audit);
        let contents = fs::read_to_string(&path).expect("audit log should be readable");
        let _ = fs::remove_dir_all(path.parent().expect("audit path should have parent"));

        assert_eq!(startup_error.kind(), io::ErrorKind::InvalidData);
        let detail = startup_error.to_string();
        assert!(detail.contains("failed to create server instance"));
        assert!(detail.contains("entry=load_legacy"));
        assert!(detail.contains("decision=fallback_generate"));
        assert!(detail.contains("failure=policy_denied"));
        assert!(contents.contains("source:\"runtime\""));
        assert!(contents.contains("action:\"world-compat-startup-reject\""));
        assert!(contents.contains("outcome:\"failed\""));
        assert!(contents.contains("entry=load_legacy"));
        assert!(!contents.contains("action:\"startup-failure\""));
    }

    #[cfg(feature = "worldgen")]
    #[test]
    fn observe_startup_compat_fallback_only_records_strict_load_contract_gaps() {
        let path = unique_temp_path();
        let strict_gap_audit = CompatAuditV1::fallback_generate(
            CompatEntryKindV1::Load,
            CompatFailureKindV1::MissingInput,
        );
        let non_gap_audit = CompatAuditV1::loaded_existing(CompatEntryKindV1::LoadOrGenerate);

        observe_startup_compat_fallback(&path, strict_gap_audit);
        observe_startup_compat_fallback(&path, non_gap_audit);

        let contents = fs::read_to_string(&path).expect("audit log should be readable");
        let _ = fs::remove_dir_all(path.parent().expect("audit path should have parent"));

        assert_eq!(contents.lines().count(), 1);
        assert!(contents.contains("action:\"world-compat-fallback\""));
        assert!(contents.contains("outcome:\"accepted\""));
        assert!(contents.contains("entry=load"));
        assert!(contents.contains("failure=missing_input"));
        assert!(!contents.contains("load_or_generate"));
    }

    #[cfg(feature = "worldgen")]
    #[test]
    fn startup_server_pre_web_compat_reject_short_circuits_before_continue_stage() {
        let path = unique_temp_path();
        let audit = CompatAuditV1::fallback_generate(
            CompatEntryKindV1::LoadLegacy,
            CompatFailureKindV1::PolicyDenied,
        );
        let continued = Cell::new(false);

        let startup_error = startup_server_pre_web(
            &path,
            Err::<StartupCompatTestState, _>(StartupCompatTestError { audit }),
            |error| {
                startup_world_compat_reject_error(&path, "World Error: compat reject", error.audit)
            },
            |state| state.audit,
        )
        .map(|_| continued.set(true))
        .expect_err("compat reject should short-circuit pre-web startup continuation");

        let contents = fs::read_to_string(&path).expect("audit log should be readable");
        let _ = fs::remove_dir_all(path.parent().expect("audit path should have parent"));

        assert_eq!(startup_error.kind(), io::ErrorKind::InvalidData);
        assert!(!continued.get());
        assert!(contents.contains("action:\"world-compat-startup-reject\""));
        assert!(!contents.contains("action:\"world-compat-fallback\""));
        assert!(!contents.contains("action:\"startup-failure\""));
    }

    #[cfg(feature = "worldgen")]
    #[test]
    fn startup_server_pre_web_sidecarless_reject_short_circuits_before_continue_stage() {
        let path = unique_temp_path();
        let audit = CompatAuditV1::reject(
            CompatEntryKindV1::LoadOrGenerate,
            CompatFailureKindV1::PolicyDenied,
            server::CompatFailureSubjectV1::Options,
            server::CompatFailureDetailV1::default(),
        );
        let continued = Cell::new(false);

        let startup_error = startup_server_pre_web(
            &path,
            Err::<StartupCompatTestState, _>(StartupCompatTestError { audit }),
            |error| {
                startup_world_compat_reject_error(&path, "World Error: compat reject", error.audit)
            },
            |state| state.audit,
        )
        .map(|_| continued.set(true))
        .expect_err("sidecarless managed reject should short-circuit pre-web startup");

        let contents = fs::read_to_string(&path).expect("audit log should be readable");
        let _ = fs::remove_dir_all(path.parent().expect("audit path should have parent"));

        assert_eq!(startup_error.kind(), io::ErrorKind::InvalidData);
        assert!(!continued.get());
        assert!(contents.contains("action:\"world-compat-startup-reject\""));
        assert!(contents.contains("entry=load_or_generate"));
        assert!(contents.contains("failure=policy_denied"));
        assert!(!contents.contains("action:\"world-compat-fallback\""));
    }

    #[cfg(feature = "worldgen")]
    #[test]
    fn startup_server_pre_web_records_strict_fallback_before_continue_stage() {
        let path = unique_temp_path();
        let audit = CompatAuditV1::fallback_generate(
            CompatEntryKindV1::Load,
            CompatFailureKindV1::MissingInput,
        );
        let continued = Cell::new(false);

        startup_server_pre_web(
            &path,
            Ok::<StartupCompatTestState, StartupCompatTestError>(StartupCompatTestState { audit }),
            |_| unreachable!("error mapper should not run on success"),
            |state| state.audit,
        )
        .map(|_| continued.set(true))
        .expect("strict fallback should remain in the pre-web success path");

        let contents = fs::read_to_string(&path).expect("audit log should be readable");
        let _ = fs::remove_dir_all(path.parent().expect("audit path should have parent"));

        assert!(continued.get());
        assert_eq!(contents.lines().count(), 1);
        assert!(contents.contains("action:\"world-compat-fallback\""));
        assert!(contents.contains("entry=load"));
        assert!(contents.contains("failure=missing_input"));
        assert!(!contents.contains("action:\"world-compat-startup-reject\""));
    }

    #[cfg(feature = "worldgen")]
    #[test]
    fn startup_server_pre_web_success_path_orders_fallback_audit_before_post_creation_side_effects()
    {
        let path = unique_temp_path();
        let audit = CompatAuditV1::fallback_generate(
            CompatEntryKindV1::Load,
            CompatFailureKindV1::MissingInput,
        );
        let events = RefCell::new(Vec::new());

        let result = startup_server_pre_web_then(
            &path,
            Ok::<StartupCompatTestState, StartupCompatTestError>(StartupCompatTestState { audit }),
            |_| unreachable!("error mapper should not run on success"),
            |state| {
                events.borrow_mut().push("compat_audit");
                state.audit
            },
            |state| {
                let contents = fs::read_to_string(&path)
                    .expect("fallback audit should exist before the continuation stage starts");
                assert_eq!(events.borrow().as_slice(), ["compat_audit"]);
                assert!(contents.contains("action:\"world-compat-fallback\""));
                assert!(!contents.contains("action:\"world-compat-startup-reject\""));

                events.borrow_mut().push("continuation");
                assert_eq!(events.borrow().as_slice(), ["compat_audit", "continuation"]);

                let post_creation_effect = || {
                    events.borrow_mut().push("post_creation_side_effect");
                };
                post_creation_effect();

                assert_eq!(events.borrow().as_slice(), [
                    "compat_audit",
                    "continuation",
                    "post_creation_side_effect"
                ]);
                Ok(state)
            },
        )
        .expect("strict fallback should stay on the dedicated startup success path");

        let contents = fs::read_to_string(&path).expect("audit log should be readable");
        let _ = fs::remove_dir_all(path.parent().expect("audit path should have parent"));

        assert_eq!(result.audit, audit);
        assert_eq!(events.into_inner(), vec![
            "compat_audit",
            "continuation",
            "post_creation_side_effect"
        ]);
        assert_eq!(contents.lines().count(), 1);
        assert!(contents.contains("action:\"world-compat-fallback\""));
    }

    #[cfg(feature = "worldgen")]
    #[test]
    fn startup_server_pre_web_success_path_without_fallback_keeps_post_creation_side_effects_audit_free()
     {
        let path = unique_temp_path();
        let audit = CompatAuditV1::loaded_existing(CompatEntryKindV1::LoadAsset);
        let events = RefCell::new(Vec::new());

        let result = startup_server_pre_web_then(
            &path,
            Ok::<StartupCompatTestState, StartupCompatTestError>(StartupCompatTestState { audit }),
            |_| unreachable!("error mapper should not run on success"),
            |state| {
                events.borrow_mut().push("compat_audit");
                state.audit
            },
            |state| {
                let read_error = fs::read_to_string(&path)
                    .expect_err("clean pre-web success should not append a compat audit event");
                assert_eq!(read_error.kind(), io::ErrorKind::NotFound);
                assert_eq!(events.borrow().as_slice(), ["compat_audit"]);

                events.borrow_mut().push("continuation");
                let post_creation_effect = || {
                    events.borrow_mut().push("post_creation_side_effect");
                };
                post_creation_effect();

                assert_eq!(events.borrow().as_slice(), [
                    "compat_audit",
                    "continuation",
                    "post_creation_side_effect"
                ]);
                Ok(state)
            },
        )
        .expect("clean pre-web success should continue without a compat fallback audit");

        let read_error =
            fs::read_to_string(&path).expect_err("clean success should keep the audit log absent");
        let _ = fs::remove_dir_all(path.parent().expect("audit path should have parent"));

        assert_eq!(result.audit, audit);
        assert_eq!(read_error.kind(), io::ErrorKind::NotFound);
        assert_eq!(events.into_inner(), vec![
            "compat_audit",
            "continuation",
            "post_creation_side_effect"
        ]);
    }

    #[test]
    #[cfg(feature = "worldgen")]
    fn startup_health_state_happy_path_smoke_serves_world_compat_and_preflight() {
        let root = unique_temp_dir();
        let runtime_layout = crate::settings::RuntimeLayout {
            userdata_dir: root.clone(),
            server_cli_settings_dir: root.join("server-cli"),
            server_state: server::ServerStatePaths::new(root.join("live")),
            recovery_staging_state: server::ServerStatePaths::new(root.join("recovery-staging")),
        };
        seed_live_runtime_state(&runtime_layout.server_state);
        seed_recovery_staging_restore_state(&runtime_layout.recovery_staging_state);

        let settings = Settings::default();
        let runtime = Arc::new(tokio::runtime::Runtime::new().expect("tokio runtime should build"));
        let (chat, _chat_exporter) =
            server::chat::ChatCache::new(Duration::from_secs(30), &runtime);
        let registry = Arc::new(prometheus::Registry::new());
        let runtime_listener_inventory = Arc::new(Mutex::new(Vec::new()));
        let runtime_observability_inventory = startup_runtime_observability_inventory(None);
        let manifest = server::RecipeManifestV1::record_only(
            server::DEFAULT_WORLD_SEED,
            &server::GenOpts::default(),
            true,
        );
        apply_startup_world_compat_observability(
            &runtime_observability_inventory,
            StartupWorldCompatObservability {
                configured_mode: "record",
                load_legacy_mode: "deny",
                load_or_generate_sidecarless_mode: "deny",
                compat_audit: CompatAuditV1::loaded_existing(CompatEntryKindV1::Load),
                recipe_manifest: &manifest,
                managed_recipe_sidecar_missing: false,
            },
        );
        let health_state = startup_health_state(
            &settings,
            &runtime_layout,
            Arc::clone(&runtime_listener_inventory),
            runtime_observability_inventory,
            true,
            Some("https://auth.example.test".to_owned()),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );

        let listener = runtime
            .block_on(web::bind_listener(
                "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
            ))
            .expect("web listener should bind");
        let addr = listener
            .local_addr()
            .expect("listener should expose a local address");
        let (web_ui_request_s, _web_ui_request_r) = tokio::sync::mpsc::channel(4);
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let web_task = runtime.spawn(async move {
            web::run_with_listener(
                registry,
                chat,
                None,
                "ui-secret".to_owned(),
                web_ui_request_s,
                health_state,
                listener,
                async move {
                    let _ = shutdown_rx.await;
                },
            )
            .await
        });

        let world_compat_response = runtime
            .block_on(fetch_http_response(addr, "/health/world-compat"))
            .expect("world-compat route should respond");
        let preflight_response = runtime
            .block_on(fetch_http_response(addr, "/health/preflight"))
            .expect("preflight route should respond");
        shutdown_tx.send(()).expect("shutdown signal should send");
        let web_result = runtime
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(5), web_task)
                    .await
                    .expect("web task should finish after shutdown")
            })
            .expect("web task should not panic");
        let _ = fs::remove_dir_all(root);

        assert!(web_result.is_ok());
        assert!(
            world_compat_response.contains(" 200 OK\r\n"),
            "world_compat_response={world_compat_response}"
        );
        assert!(world_compat_response.contains("\"status\":\"world-compat-clear\""));
        assert!(world_compat_response.contains("\"load_legacy_mode\":\"deny\""));
        assert!(world_compat_response.contains("\"load_or_generate_sidecarless_mode\":\"deny\""));
        assert!(world_compat_response.contains("\"review_result_status_hint\":\"approved\""));
        assert!(
            world_compat_response
                .contains("\"required_terminal_record_fields\":[\"rollback_reference\"]")
        );
        assert!(
            preflight_response.contains(" 200 OK\r\n"),
            "preflight_response={preflight_response}"
        );
        let preflight_body = http_response_body(&preflight_response);
        assert!(preflight_body.contains("\"status\":\"preflight_clear\""));
        assert!(preflight_body.contains("\"signal\":\"world-compat\""));
        assert!(preflight_body.contains("\"status\":\"world-compat-clear\""));
    }

    #[test]
    #[cfg(feature = "worldgen")]
    fn startup_health_state_allow_window_smoke_exports_world_compat_exception_path() {
        let root = unique_temp_dir();
        let runtime_layout = crate::settings::RuntimeLayout {
            userdata_dir: root.clone(),
            server_cli_settings_dir: root.join("server-cli"),
            server_state: server::ServerStatePaths::new(root.join("live")),
            recovery_staging_state: server::ServerStatePaths::new(root.join("recovery-staging")),
        };
        seed_live_runtime_state(&runtime_layout.server_state);
        seed_recovery_staging_restore_state(&runtime_layout.recovery_staging_state);

        let settings = Settings::default();
        let runtime = Arc::new(tokio::runtime::Runtime::new().expect("tokio runtime should build"));
        let (chat, _chat_exporter) =
            server::chat::ChatCache::new(Duration::from_secs(30), &runtime);
        let registry = Arc::new(prometheus::Registry::new());
        let runtime_listener_inventory = Arc::new(Mutex::new(Vec::new()));
        let runtime_observability_inventory = startup_runtime_observability_inventory(None);
        let manifest = server::RecipeManifestV1::record_only(
            server::DEFAULT_WORLD_SEED,
            &server::GenOpts::default(),
            true,
        );
        apply_startup_world_compat_observability(
            &runtime_observability_inventory,
            StartupWorldCompatObservability {
                configured_mode: "record",
                load_legacy_mode: "allow",
                load_or_generate_sidecarless_mode: "allow",
                compat_audit: CompatAuditV1::loaded_existing(CompatEntryKindV1::LoadLegacy),
                recipe_manifest: &manifest,
                managed_recipe_sidecar_missing: false,
            },
        );
        let health_state = startup_health_state(
            &settings,
            &runtime_layout,
            Arc::clone(&runtime_listener_inventory),
            runtime_observability_inventory,
            true,
            Some("https://auth.example.test".to_owned()),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );

        let listener = runtime
            .block_on(web::bind_listener(
                "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
            ))
            .expect("web listener should bind");
        let addr = listener
            .local_addr()
            .expect("listener should expose a local address");
        let (web_ui_request_s, _web_ui_request_r) = tokio::sync::mpsc::channel(4);
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let web_task = runtime.spawn(async move {
            web::run_with_listener(
                registry,
                chat,
                None,
                "ui-secret".to_owned(),
                web_ui_request_s,
                health_state,
                listener,
                async move {
                    let _ = shutdown_rx.await;
                },
            )
            .await
        });

        let world_compat_response = runtime
            .block_on(fetch_http_response(addr, "/health/world-compat"))
            .expect("world-compat route should respond");
        let preflight_response = runtime
            .block_on(fetch_http_response(addr, "/health/preflight"))
            .expect("preflight route should respond");
        shutdown_tx.send(()).expect("shutdown signal should send");
        let web_result = runtime
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(5), web_task)
                    .await
                    .expect("web task should finish after shutdown")
            })
            .expect("web task should not panic");
        let _ = fs::remove_dir_all(root);

        assert!(web_result.is_ok());
        assert!(world_compat_response.contains(" 200 OK\r\n"));
        assert!(world_compat_response.contains("\"status\":\"world-compat-review-required\""));
        assert!(world_compat_response.contains("\"transition_window_open\":true"));
        assert!(
            world_compat_response.contains("\"review_result_status_hint\":\"exception-accepted\"")
        );
        assert!(world_compat_response.contains(
            "\"required_terminal_record_fields\":[\"exception_reason\",\"rollback_reference\"]"
        ));
        assert!(preflight_response.contains(" 200 OK\r\n"));
        let preflight_body = http_response_body(&preflight_response);
        assert!(preflight_body.contains("\"status\":\"operator_review_required\""));
        assert!(preflight_body.contains("\"signal\":\"world-compat\""));
        assert!(
            preflight_body.contains("\"current_result_status_hint\":\"exception-accepted\""),
            "preflight_body={preflight_body}"
        );
        assert!(preflight_body.contains(
            "\"current_terminal_record_fields\":[\"exception_reason\",\"rollback_reference\"]"
        ));
    }

    #[test]
    #[cfg(feature = "worldgen")]
    fn startup_server_new_live_health_happy_path_serves_world_compat_preflight_and_listeners() {
        let root = unique_temp_dir();
        let runtime_layout = crate::settings::RuntimeLayout {
            userdata_dir: root.clone(),
            server_cli_settings_dir: root.join("server-cli"),
            server_state: server::ServerStatePaths::new(root.join("live")),
            recovery_staging_state: server::ServerStatePaths::new(root.join("recovery-staging")),
        };
        let mut seeded_server_settings = server::Settings::default();
        seeded_server_settings.gameserver_protocols = vec![Protocol::Tcp {
            address: "127.0.0.1:0"
                .parse()
                .expect("loopback tcp socket should parse"),
        }];
        seeded_server_settings.query_address = None;
        seed_live_runtime_state_with_settings(
            &runtime_layout.server_state,
            &seeded_server_settings,
        );
        seed_recovery_staging_restore_state(&runtime_layout.recovery_staging_state);

        let audit_log_path = runtime_layout.server_state.ops_dir.join("audit-log.ronl");
        let mut cli_settings = Settings::default();
        cli_settings.ui_api_secret = Some("ui-secret".to_owned());
        cli_settings.web_address = "127.0.0.1:0".parse().unwrap();
        let server_data_dir = runtime_layout.server_state.data_dir.clone();
        let server_identity = server::ServerIdentity::load(&server_data_dir);
        let server_settings = server::Settings::load(&server_data_dir);
        let editable_settings = server::EditableSettings::load(&server_data_dir);
        let database_settings = server::persistence::DatabaseSettings {
            db_dir: runtime_layout.server_state.database_dir.clone(),
            sql_log_mode: server::persistence::SqlLogMode::Disabled,
        };
        let runtime = Arc::new(tokio::runtime::Runtime::new().expect("tokio runtime should build"));
        let PreparedWorldgenStartup {
            server: _server,
            registry,
            chat,
            runtime_listener_inventory: _runtime_listener_inventory,
            runtime_listener_audit_state: _runtime_listener_audit_state,
            runtime_observability_inventory: _runtime_observability_inventory,
            runtime_observability_audit_state: _runtime_observability_audit_state,
            health_state,
        } = prepare_worldgen_startup(
            &cli_settings,
            &runtime_layout,
            &audit_log_path,
            server_settings,
            editable_settings,
            server_identity,
            database_settings,
            &server_data_dir,
            Arc::clone(&runtime),
        )
        .expect("real Server::new startup should prepare dedicated health state");
        let listener = bind_startup_web_listener(&cli_settings, &runtime, &audit_log_path)
            .expect("web listener should bind");
        let addr = listener
            .local_addr()
            .expect("listener should expose a local address");
        let ui_api_secret = resolved_ui_api_secret(&cli_settings);
        let (web_ui_request_s, _web_ui_request_r) = tokio::sync::mpsc::channel(4);
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let web_task = runtime.spawn(async move {
            web::run_with_listener(
                registry,
                chat,
                None,
                ui_api_secret,
                web_ui_request_s,
                health_state,
                listener,
                async move {
                    let _ = shutdown_rx.await;
                },
            )
            .await
        });

        let world_compat_response = runtime
            .block_on(fetch_http_response(addr, "/health/world-compat"))
            .expect("world-compat route should respond");
        let preflight_response = runtime
            .block_on(fetch_http_response(addr, "/health/preflight"))
            .expect("preflight route should respond");
        let listeners_response = runtime
            .block_on(fetch_http_response(addr, "/health/listeners"))
            .expect("listeners route should respond");
        shutdown_tx.send(()).expect("shutdown signal should send");
        let web_result = runtime
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(5), web_task)
                    .await
                    .expect("web task should finish after shutdown")
            })
            .expect("web task should not panic");

        assert!(web_result.is_ok());
        assert!(
            world_compat_response.contains(" 200 OK\r\n"),
            "world_compat_response={world_compat_response}"
        );
        assert!(world_compat_response.contains("\"status\":\"world-compat-clear\""));
        assert!(world_compat_response.contains("\"configured_mode\":\"record\""));
        assert!(world_compat_response.contains("\"load_legacy_mode\":\"deny\""));
        assert!(world_compat_response.contains("\"load_or_generate_sidecarless_mode\":\"deny\""));
        assert!(world_compat_response.contains("\"compat_entry\":\"load_asset\""));
        assert!(world_compat_response.contains("\"review_result_status_hint\":\"approved\""));
        assert!(
            world_compat_response
                .contains("\"required_terminal_record_fields\":[\"rollback_reference\"]")
        );
        assert!(
            preflight_response.contains(" 200 OK\r\n"),
            "preflight_response={preflight_response}"
        );
        let preflight_body = http_response_body(&preflight_response);
        assert!(
            preflight_body.contains("\"status\":\"preflight_clear\""),
            "preflight_body={preflight_body}"
        );
        assert!(
            preflight_body.contains("\"signal\":\"world-compat\""),
            "preflight_body={preflight_body}"
        );
        assert!(
            preflight_body.contains("\"status\":\"world-compat-clear\""),
            "preflight_body={preflight_body}"
        );
        assert!(
            preflight_body.contains("\"signal\":\"runtime-listeners\""),
            "preflight_body={preflight_body}"
        );
        assert!(
            preflight_body.contains("\"status\":\"runtime-listeners-ready\""),
            "preflight_body={preflight_body}"
        );
        assert!(
            listeners_response.contains(" 200 OK\r\n"),
            "listeners_response={listeners_response}"
        );
        assert!(listeners_response.contains("\"status\":\"runtime-listener-inventory\""));
        assert!(listeners_response.contains("\"surface\":\"game-tcp\""));
        assert!(listeners_response.contains("\"state\":\"listening\""));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    #[cfg(feature = "worldgen")]
    fn startup_runtime_guard_and_handoff_smoke_serves_ready_preflight_and_listeners() {
        let root = unique_temp_dir();
        let runtime_layout = crate::settings::RuntimeLayout {
            userdata_dir: root.clone(),
            server_cli_settings_dir: root.join("server-cli"),
            server_state: server::ServerStatePaths::new(root.join("live")),
            recovery_staging_state: server::ServerStatePaths::new(root.join("recovery-staging")),
        };
        let mut seeded_server_settings = server::Settings::default();
        seeded_server_settings.gameserver_protocols = vec![Protocol::Tcp {
            address: "127.0.0.1:0"
                .parse()
                .expect("loopback tcp socket should parse"),
        }];
        seeded_server_settings.query_address = None;
        seed_live_runtime_state_with_settings(
            &runtime_layout.server_state,
            &seeded_server_settings,
        );
        seed_recovery_staging_restore_state(&runtime_layout.recovery_staging_state);

        let mut cli_settings = Settings::default();
        cli_settings.ui_api_secret = Some("ui-secret".to_owned());
        cli_settings.web_address = "127.0.0.1:0".parse().unwrap();

        let PreparedRuntimeInputs {
            runtime_layout,
            audit_log_path,
            server_data_dir,
            runtime,
            server_identity,
            server_settings,
            editable_settings,
            database_settings,
        } = prepare_runtime_inputs(
            &cli_settings,
            runtime_layout,
            false,
            server::persistence::SqlLogMode::Disabled,
        )
        .expect("runtime guard and probe stage should prepare startup inputs");

        let PreparedServerLoopHandoff {
            server: _server,
            metrics_shutdown,
            web_server_task,
            web_bind_address,
            web_ui_request_r: _web_ui_request_r,
            runtime_listener_inventory: _runtime_listener_inventory,
            runtime_listener_audit_state: _runtime_listener_audit_state,
            runtime_observability_inventory: _runtime_observability_inventory,
            runtime_observability_audit_state: _runtime_observability_audit_state,
        } = prepare_server_loop_handoff(
            &cli_settings,
            &runtime_layout,
            &audit_log_path,
            server_settings,
            editable_settings,
            server_identity,
            database_settings,
            &server_data_dir,
            Arc::clone(&runtime),
        )
        .expect("startup handoff should assemble live web/health state");
        let addr = web_bind_address.expect("startup handoff should expose the web listener");

        let ready_response = runtime
            .block_on(fetch_http_response(addr, "/health/ready"))
            .expect("ready route should respond");
        let preflight_response = runtime
            .block_on(fetch_http_response(addr, "/health/preflight"))
            .expect("preflight route should respond");
        let listeners_response = runtime
            .block_on(fetch_http_response(addr, "/health/listeners"))
            .expect("listeners route should respond");

        metrics_shutdown.notify_one();
        runtime
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(5), web_server_task)
                    .await
                    .expect("web task should finish after shutdown")
            })
            .expect("web task should not panic");
        let _ = fs::remove_dir_all(root);

        assert!(
            ready_response.contains(" 200 OK\r\n"),
            "ready_response={ready_response}"
        );
        let ready_body = http_response_body(&ready_response);
        assert!(
            ready_body.contains("\"status\":\"ready\""),
            "ready_body={ready_body}"
        );
        assert!(
            ready_body.contains("\"name\":\"recovery-staging-layout\""),
            "ready_body={ready_body}"
        );
        assert!(
            ready_body.contains("\"name\":\"runtime-state-layout\""),
            "ready_body={ready_body}"
        );
        assert!(
            preflight_response.contains(" 200 OK\r\n"),
            "preflight_response={preflight_response}"
        );
        let preflight_body = http_response_body(&preflight_response);
        assert!(
            preflight_body.contains("\"status\":\"preflight_clear\""),
            "preflight_body={preflight_body}"
        );
        assert!(
            preflight_body.contains("\"signal\":\"world-compat\""),
            "preflight_body={preflight_body}"
        );
        assert!(
            preflight_body.contains("\"status\":\"world-compat-clear\""),
            "preflight_body={preflight_body}"
        );
        assert!(
            preflight_body.contains("\"signal\":\"runtime-listeners\""),
            "preflight_body={preflight_body}"
        );
        assert!(
            preflight_body.contains("\"status\":\"runtime-listeners-ready\""),
            "preflight_body={preflight_body}"
        );
        assert!(
            listeners_response.contains(" 200 OK\r\n"),
            "listeners_response={listeners_response}"
        );
        assert!(listeners_response.contains("\"status\":\"runtime-listener-inventory\""));
        assert!(listeners_response.contains("\"surface\":\"game-tcp\""));
        assert!(listeners_response.contains("\"state\":\"listening\""));
    }

    #[test]
    fn runtime_listener_startup_audit_records_non_listening_entries() {
        let path = unique_temp_path();
        let inventory = Arc::new(Mutex::new(vec![
            server::RuntimeListenerStatus {
                surface: server::RuntimeListenerSurface::GameTcp,
                bind_address: "0.0.0.0:14004".parse().unwrap(),
                state: server::RuntimeListenerState::Listening,
                detail: "listener accepted the declared TCP gameplay bind address".to_owned(),
            },
            server::RuntimeListenerStatus {
                surface: server::RuntimeListenerSurface::QueryServer,
                bind_address: "0.0.0.0:14006".parse().unwrap(),
                state: server::RuntimeListenerState::StartupFailed,
                detail: "failed to bind query server listener on 0.0.0.0:14006: address in use"
                    .to_owned(),
            },
        ]));

        let _ = append_runtime_listener_startup_audit_events(&path, &inventory);

        let contents = fs::read_to_string(&path).expect("audit log should be readable");
        let _ = fs::remove_dir_all(path.parent().expect("audit path should have parent"));

        assert!(contents.contains("action:\"startup-failure\""));
        assert!(contents.contains("query-server"));
        assert!(contents.contains("startup-failed"));
        assert!(!contents.contains("game-tcp"));
    }

    #[test]
    fn runtime_listener_transition_audit_records_state_changes_once() {
        let path = unique_temp_path();
        let inventory = Arc::new(Mutex::new(vec![server::RuntimeListenerStatus {
            surface: server::RuntimeListenerSurface::QueryServer,
            bind_address: "0.0.0.0:14006".parse().unwrap(),
            state: server::RuntimeListenerState::Listening,
            detail: "listener accepted the declared query server bind address".to_owned(),
        }]));
        let mut previous_states =
            runtime_listener_audit_state(&snapshot_runtime_listener_inventory(&inventory));

        {
            let mut entries = inventory.lock().expect("inventory lock should succeed");
            entries[0].state = server::RuntimeListenerState::StoppedUnexpectedly;
            entries[0].detail =
                "query server stopped unexpectedly after startup: task returned".to_owned();
        }

        append_runtime_listener_transition_audit_events(&path, &inventory, &mut previous_states);
        append_runtime_listener_transition_audit_events(&path, &inventory, &mut previous_states);

        let contents = fs::read_to_string(&path).expect("audit log should be readable");
        let _ = fs::remove_dir_all(path.parent().expect("audit path should have parent"));

        assert_eq!(contents.lines().count(), 1);
        assert!(contents.contains("action:\"runtime-listener-failure\""));
        assert!(contents.contains("query-server"));
        assert!(contents.contains("stopped-unexpectedly"));
    }

    #[test]
    fn web_runtime_failure_audit_records_bind_address() {
        let path = unique_temp_path();
        let error = io::Error::other("broken pipe");

        append_web_runtime_failure_audit_event(
            &path,
            Some("127.0.0.1:14005".parse().unwrap()),
            &error,
        );

        let contents = fs::read_to_string(&path).expect("audit log should be readable");
        let _ = fs::remove_dir_all(path.parent().expect("audit path should have parent"));

        assert!(contents.contains("action:\"web-runtime-failure\""));
        assert!(contents.contains("127.0.0.1:14005"));
        assert!(contents.contains("broken pipe"));
    }

    #[test]
    fn observability_runtime_transition_audit_records_metrics_failures_once() {
        let path = unique_temp_path();
        let inventory = web::default_runtime_observability_inventory();
        let mut previous_states = runtime_observability_audit_state(
            &web::snapshot_runtime_observability_inventory(&inventory),
        );

        {
            let mut entries = inventory.lock().expect("inventory lock should succeed");
            entries[0].state = web::RuntimeObservabilityState::Failing;
            entries[0].detail = "failed to encode metrics HTTP response: broken pipe".to_owned();
        }

        append_runtime_observability_transition_audit_events(
            &path,
            &inventory,
            &mut previous_states,
        );
        append_runtime_observability_transition_audit_events(
            &path,
            &inventory,
            &mut previous_states,
        );

        let contents = fs::read_to_string(&path).expect("audit log should be readable");
        let _ = fs::remove_dir_all(path.parent().expect("audit path should have parent"));

        assert_eq!(contents.lines().count(), 1);
        assert!(contents.contains("action:\"observability-runtime-failure\""));
        assert!(contents.contains("metrics-export"));
        assert!(contents.contains("broken pipe"));
    }
}

fn server_loop(
    mut server: Server,
    bench: Option<BenchParams>,
    settings: Settings,
    tui: Option<Tui>,
    mut web_ui_request_r: WebUiRequestReceiver,
    audit_log_path: PathBuf,
    shutdown_signal: Arc<AtomicBool>,
    runtime_listener_inventory: server::RuntimeListenerInventory,
    mut runtime_listener_audit_state: RuntimeListenerAuditState,
    runtime_observability_inventory: web::RuntimeObservabilityInventory,
    mut runtime_observability_audit_state: RuntimeObservabilityAuditState,
) -> io::Result<()> {
    // Set up an fps clock
    let mut clock = Clock::new(Duration::from_secs_f64(1.0 / TPS as f64));
    let mut shutdown_coordinator = ShutdownCoordinator::new(Arc::clone(&shutdown_signal));
    let mut bench_exit_time = None;

    let mut tick_no = 0u64;
    'outer: loop {
        span!(guard, "work");
        if let Some(bench) = bench {
            if let Some(t) = bench_exit_time {
                if Instant::now() > t {
                    break;
                }
            } else if tick_no != 0 && !server.chunks_pending() {
                println!("Chunk loading complete");
                bench_exit_time = Some(Instant::now() + Duration::from_secs(bench.duration.into()));
            }
        };

        tick_no += 1;
        // Terminate the server if instructed to do so by the shutdown coordinator
        if shutdown_coordinator.check(&mut server, &settings, &audit_log_path) {
            break;
        }
        append_runtime_listener_transition_audit_events(
            &audit_log_path,
            &runtime_listener_inventory,
            &mut runtime_listener_audit_state,
        );
        append_runtime_observability_transition_audit_events(
            &audit_log_path,
            &runtime_observability_inventory,
            &mut runtime_observability_audit_state,
        );

        let events = server
            .tick(Input::default(), clock.dt())
            .expect("Failed to tick server");

        for event in events {
            match event {
                Event::ClientConnected { entity: _ } => info!("Client connected!"),
                Event::ClientDisconnected { entity: _ } => info!("Client disconnected!"),
                Event::Chat { entity: _, msg } => info!("[Client] {}", msg),
            }
        }

        // Clean up the server after a tick.
        server.cleanup();
        web::set_chunk_lifecycle_observability_status(
            &runtime_observability_inventory,
            server.chunk_lifecycle_abnormal_summary(),
        );

        if tick_no.rem_euclid(1000) == 0 {
            trace!(?tick_no, "keepalive")
        }

        let mut handle_msg =
            |source: AuditSource, msg, response: tokio::sync::oneshot::Sender<MessageReturn>| {
                use specs::{Join, WorldExt};
                match msg {
                    Message::Shutdown {
                        command: Shutdown::Cancel,
                    } => shutdown_coordinator.abort_shutdown(&mut server, &audit_log_path, source),
                    Message::Shutdown {
                        command: Shutdown::Graceful { seconds, reason },
                    } => {
                        shutdown_coordinator.initiate_shutdown(
                            &mut server,
                            Duration::from_secs(seconds),
                            reason,
                            &audit_log_path,
                            source,
                        );
                    },
                    Message::Shutdown {
                        command: Shutdown::Immediate,
                    } => {
                        append_audit_event_warn(
                            &audit_log_path,
                            source,
                            AuditAction::ShutdownImmediate,
                            AuditOutcome::Accepted,
                            "operator requested immediate shutdown",
                        );
                        return true;
                    },
                    Message::Shared(SharedCommand::Admin {
                        command: Admin::Add { username, role },
                    }) => {
                        server.add_admin(&username, role);
                        append_audit_event_warn(
                            &audit_log_path,
                            source,
                            AuditAction::AdminAdd,
                            AuditOutcome::Accepted,
                            &format!("username={username} role={role:?}"),
                        );
                    },
                    Message::Shared(SharedCommand::Admin {
                        command: Admin::Remove { username },
                    }) => {
                        server.remove_admin(&username);
                        append_audit_event_warn(
                            &audit_log_path,
                            source,
                            AuditAction::AdminRemove,
                            AuditOutcome::Accepted,
                            &format!("username={username}"),
                        );
                    },
                    #[cfg(feature = "worldgen")]
                    Message::LoadArea { view_distance } => {
                        server.create_centered_persister(view_distance);
                    },
                    Message::SqlLogMode { mode } => {
                        server.set_sql_log_mode(mode);
                        append_audit_event_warn(
                            &audit_log_path,
                            source,
                            AuditAction::SetSqlLogMode,
                            AuditOutcome::Accepted,
                            &format!("mode={mode:?}"),
                        );
                    },
                    Message::DisconnectAllClients => {
                        server.disconnect_all_clients();
                        append_audit_event_warn(
                            &audit_log_path,
                            source,
                            AuditAction::DisconnectAllClients,
                            AuditOutcome::Accepted,
                            "disconnect all clients requested",
                        );
                    },
                    Message::ListPlayers => {
                        let players: Vec<String> = server
                            .state()
                            .ecs()
                            .read_storage::<Player>()
                            .join()
                            .map(|p| p.alias.clone())
                            .collect();
                        let _ = response.send(MessageReturn::Players(players));
                    },
                    Message::ListLogs => {
                        let log = LOG.inner.lock().unwrap();
                        let lines: Vec<_> = log
                            .lines
                            .iter()
                            .rev()
                            .take(30)
                            .map(|l| l.to_string())
                            .collect();
                        let _ = response.send(MessageReturn::Logs(lines));
                    },
                    Message::SendGlobalMsg { msg } => {
                        use server::state_ext::StateExt;
                        let detail = format!("bytes={}", msg.len());
                        let msg = ChatType::Meta.into_plain_msg(msg);
                        server.state().send_chat(msg, false);
                        append_audit_event_warn(
                            &audit_log_path,
                            source,
                            AuditAction::SendGlobalMessage,
                            AuditOutcome::Accepted,
                            &detail,
                        );
                    },
                }
                false
            };

        if let Some(tui) = tui.as_ref() {
            while let Ok(msg) = tui.msg_r.try_recv() {
                let (sender, mut recv) = tokio::sync::oneshot::channel();
                if handle_msg(AuditSource::Tui, msg, sender) {
                    info!("Closing the server");
                    break 'outer;
                }
                if let Ok(msg_answ) = recv.try_recv() {
                    match msg_answ {
                        MessageReturn::Players(players) => info!("Players: {:?}", players),
                        MessageReturn::Logs(_) => info!("skipp sending logs to tui"),
                    };
                }
            }
        }

        while let Ok((msg, sender)) = web_ui_request_r.try_recv() {
            if handle_msg(AuditSource::UiApi, msg, sender) {
                info!("Closing the server");
                break 'outer;
            }
        }

        drop(guard);
        // Wait for the next tick.
        clock.tick();
        #[cfg(feature = "tracy")]
        common_base::tracy_client::frame_mark();
    }
    Ok(())
}
