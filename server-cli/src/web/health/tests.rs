use super::*;
use crate::web::bind_listener;
use std::{
    fs,
    net::SocketAddr,
    time::{SystemTime, UNIX_EPOCH},
};

fn unique_temp_dir() -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("caldrayne-health-{unique}"))
}

fn test_runtime_observability_inventory() -> RuntimeObservabilityInventory {
    default_runtime_observability_inventory()
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

fn seed_settings_file(data_dir: &Path) {
    let settings_path = server::settings::settings_file_path(data_dir);
    let settings = ron::ser::to_string_pretty(
        &server::Settings::default(),
        ron::ser::PrettyConfig::default(),
    )
    .expect("should serialize default server settings");
    fs::write(&settings_path, settings).expect("should write settings file");
}

fn seed_live_runtime_state(state: &server::ServerStatePaths) {
    fs::create_dir_all(&state.config_dir).expect("should create config dir");
    fs::create_dir_all(&state.ops_dir).expect("should create ops dir");
    seed_identity_file(&state.identity_file);
    seed_database_file(&state.database_dir);
    seed_settings_file(&state.data_dir);
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

fn test_auth_provider(configured: bool) -> Option<String> {
    configured.then_some("https://auth.example.test".to_owned())
}

fn test_repo_bundled_snapshot_report(
    baseline: Option<common::official_entry::BundledOfficialEntryPosture>,
) -> RepoBundledOfficialEntrySnapshotReport {
    RepoBundledOfficialEntrySnapshotReport {
        status: if baseline.is_some() {
            "repo-bundled-entry-baseline-available"
        } else {
            "repo-bundled-entry-unavailable"
        },
        evidence_scope: "repo/local bundled official_entry baseline only",
        load_source: "voxygen.official_entry asset via common asset loader",
        authoritative_for_release_cutover: false,
        required_external_match_fields: vec![
            "bundled_official_entry_artifact_identity",
            "bundled_target_kind",
        ],
        baseline,
        load_error: None,
        semantics: "test snapshot",
    }
}

fn test_health_state(root: &Path) -> HealthState {
    let server_state = server::ServerStatePaths::new(root.join("live"));
    HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state,
        recovery_staging_state: server::ServerStatePaths::new(root.join("recovery-staging")),
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
}

#[test]
fn readiness_report_is_ready_when_required_state_exists() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    seed_live_runtime_state(&state);
    seed_recovery_staging_restore_state(&recovery_staging_state);

    let health = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state.clone(),
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    };

    let report = health.readiness_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "ready");
    assert!(
        report
            .checks
            .iter()
            .all(|check| !check.required || check.ok)
    );
    assert!(
        report.checks.iter().any(|check| {
            check.name == "server-rtsim-state-file" && !check.required && !check.ok
        })
    );
    assert!(report.checks.iter().any(|check| {
        check.name == "backup-evidence-sink-parent-dir"
            && !check.required
            && check.ok
            && check.detail.contains("advisory")
    }));
}

#[test]
fn readiness_report_is_not_ready_when_database_file_is_missing() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    fs::create_dir_all(&state.config_dir).expect("should create config dir");
    fs::create_dir_all(&state.database_dir).expect("should create database dir");
    fs::create_dir_all(&state.ops_dir).expect("should create ops dir");
    seed_identity_file(&state.identity_file);

    let report = test_health_state(&root).readiness_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "not_ready");
    assert!(
        report
            .checks
            .iter()
            .any(|check| check.name == "server-database-file" && !check.ok)
    );
}

#[test]
fn readiness_report_blocks_when_server_database_file_is_invalid() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    fs::create_dir_all(&state.config_dir).expect("should create config dir");
    fs::create_dir_all(&state.database_dir).expect("should create database dir");
    fs::create_dir_all(&state.ops_dir).expect("should create ops dir");
    seed_identity_file(&state.identity_file);
    fs::write(&state.database_file, b"not a sqlite database")
        .expect("should write invalid db file");
    seed_settings_file(&state.data_dir);
    fs::create_dir_all(&recovery_staging_state.data_dir)
        .expect("should create recovery staging dir");

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .readiness_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "not_ready");
    assert!(report.checks.iter().any(|check| {
        check.name == "server-database-file"
            && check.required
            && !check.ok
            && check.detail.contains("readable SQLite database")
    }));
}

#[test]
fn readiness_report_blocks_when_server_identity_file_is_missing() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    fs::create_dir_all(&state.config_dir).expect("should create config dir");
    fs::create_dir_all(&state.database_dir).expect("should create database dir");
    fs::create_dir_all(&state.ops_dir).expect("should create ops dir");
    seed_database_file(&state.database_dir);
    seed_settings_file(&state.data_dir);
    fs::create_dir_all(&recovery_staging_state.data_dir)
        .expect("should create recovery staging dir");

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .readiness_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "not_ready");
    assert!(report.checks.iter().any(|check| {
        check.name == "server-identity-file"
            && check.required
            && !check.ok
            && check.detail.contains("identity file missing")
    }));
}

#[test]
fn readiness_report_blocks_when_server_identity_file_is_invalid() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    fs::create_dir_all(&state.config_dir).expect("should create config dir");
    fs::create_dir_all(&state.database_dir).expect("should create database dir");
    fs::create_dir_all(&state.ops_dir).expect("should create ops dir");
    fs::write(
        server::settings::identity_file_path(&state.data_dir),
        b"this is not valid ron",
    )
    .expect("should write invalid identity");
    seed_database_file(&state.database_dir);
    seed_settings_file(&state.data_dir);
    fs::create_dir_all(&recovery_staging_state.data_dir)
        .expect("should create recovery staging dir");

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .readiness_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "not_ready");
    assert!(report.checks.iter().any(|check| {
        check.name == "server-identity-file"
            && check.required
            && !check.ok
            && check.detail.contains("not valid RON")
    }));
}

#[test]
fn local_environment_allows_missing_auth_in_readiness_report() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    fs::create_dir_all(&state.config_dir).expect("should create config dir");
    fs::create_dir_all(&state.database_dir).expect("should create database dir");
    fs::create_dir_all(&state.ops_dir).expect("should create ops dir");
    seed_identity_file(&state.identity_file);
    seed_database_file(&state.database_dir);
    seed_settings_file(&state.data_dir);
    fs::create_dir_all(&recovery_staging_state.data_dir)
        .expect("should create recovery staging dir");

    let report = HealthState {
        environment: "local",
        auth_server_configured: false,
        authoritative_auth_provider: test_auth_provider(false),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .readiness_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "ready");
    assert!(
        report
            .checks
            .iter()
            .any(|check| { check.name == "auth-runtime-policy" && check.ok && !check.required })
    );
}

#[test]
fn readiness_report_keeps_optional_operational_state_as_advisory() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    fs::create_dir_all(&state.config_dir).expect("should create config dir");
    fs::create_dir_all(&state.database_dir).expect("should create database dir");
    fs::create_dir_all(&state.ops_dir).expect("should create ops dir");
    seed_identity_file(&state.identity_file);
    seed_database_file(&state.database_dir);
    seed_settings_file(&state.data_dir);
    fs::create_dir_all(&recovery_staging_state.data_dir)
        .expect("should create recovery staging dir");

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .readiness_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "ready");
    assert!(
        report.checks.iter().any(|check| {
            check.name == "server-rtsim-state-file" && !check.required && !check.ok
        })
    );
    assert!(report.checks.iter().any(|check| {
        check.name == "server-terrain-persistence-dir" && !check.required && !check.ok
    }));
    assert!(
        report
            .checks
            .iter()
            .any(|check| { check.name == "audit-log-target-file" && !check.required && check.ok })
    );
    assert!(report.checks.iter().any(|check| {
        check.name == "backup-evidence-sink-parent-dir" && !check.required && check.ok
    }));
    assert!(report.checks.iter().any(|check| {
        check.name == "recovery-drill-evidence-sink-parent-dir" && !check.required && check.ok
    }));
}

#[test]
fn readiness_report_blocks_when_runtime_state_layout_conflicts() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::with_overrides(
        root.join("live"),
        None,
        Some(root.join("live").join("ops")),
    );
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    fs::create_dir_all(&state.config_dir).expect("should create config dir");
    fs::create_dir_all(&state.database_dir).expect("should create database dir");
    fs::create_dir_all(&state.ops_dir).expect("should create ops dir");
    seed_identity_file(&state.identity_file);
    seed_database_file(&state.database_dir);
    fs::create_dir_all(&state.terrain_dir).expect("should create terrain dir");
    fs::create_dir_all(&recovery_staging_state.data_dir)
        .expect("should create recovery staging dir");

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .readiness_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "not_ready");
    assert!(report.checks.iter().any(|check| {
        check.name == "runtime-state-layout"
            && check.required
            && !check.ok
            && check.detail.contains("terrain-persistence")
            && check.detail.contains("operational-audit-trail")
    }));
}

#[test]
fn readiness_report_is_not_ready_when_ops_dir_is_missing() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    fs::create_dir_all(&state.config_dir).expect("should create config dir");
    fs::create_dir_all(&state.database_dir).expect("should create database dir");
    seed_identity_file(&state.identity_file);
    seed_database_file(&state.database_dir);
    fs::create_dir_all(&recovery_staging_state.data_dir)
        .expect("should create recovery staging dir");

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .readiness_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "not_ready");
    assert!(
        report
            .checks
            .iter()
            .any(|check| check.name == "server-ops-dir" && !check.ok)
    );
}

#[test]
fn readiness_report_keeps_evidence_sink_issues_as_advisory() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    fs::create_dir_all(&state.config_dir).expect("should create config dir");
    fs::create_dir_all(&state.database_dir).expect("should create database dir");
    fs::create_dir_all(&state.ops_dir).expect("should create ops dir");
    seed_identity_file(&state.identity_file);
    seed_database_file(&state.database_dir);
    seed_settings_file(&state.data_dir);
    fs::create_dir_all(&state.backup_evidence_log_file)
        .expect("should create invalid evidence sink directory");
    fs::create_dir_all(&recovery_staging_state.data_dir)
        .expect("should create recovery staging dir");

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .readiness_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "ready");
    assert!(report.checks.iter().any(|check| {
        check.name == "backup-evidence-sink-target-file" && !check.required && !check.ok
    }));
}

#[test]
fn readiness_report_blocks_on_fail_fast_transport_security_material_issues() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    fs::create_dir_all(&state.config_dir).expect("should create config dir");
    fs::create_dir_all(&state.database_dir).expect("should create database dir");
    fs::create_dir_all(&state.ops_dir).expect("should create ops dir");
    seed_identity_file(&state.identity_file);
    seed_database_file(&state.database_dir);
    fs::create_dir_all(&recovery_staging_state.data_dir)
        .expect("should create recovery staging dir");

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: vec![crate::settings::TransportSecurityInventoryEntry {
            surface: "game-quic",
            bind_address: "0.0.0.0:14004".parse().unwrap(),
            transport: "quic",
            encryption: "tls-required",
            cert_file_path: Path::new("tls").join("cert.pem"),
            key_file_path: Path::new("tls").join("key.pem"),
            rollout_policy:
                crate::settings::TransportSecurityRolloutPolicy::ExperimentalOptInActive,
            validation_policy:
                crate::settings::TransportSecurityValidationPolicy::FailFastAtStartup,
            material_state: crate::settings::TransportSecurityMaterialState::Invalid,
            detail: "TLS material failed validation".to_owned(),
        }],
        governance_findings: Vec::new(),
    }
    .readiness_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "not_ready");
    assert!(report.checks.iter().any(|check| {
        check.name == "quic-binding-0-tls-material" && check.required && !check.ok
    }));
}

#[test]
fn readiness_report_blocks_when_server_settings_file_is_missing_in_production() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    fs::create_dir_all(&state.config_dir).expect("should create config dir");
    fs::create_dir_all(&state.database_dir).expect("should create database dir");
    fs::create_dir_all(&state.ops_dir).expect("should create ops dir");
    seed_identity_file(&state.identity_file);
    seed_database_file(&state.database_dir);
    fs::create_dir_all(&recovery_staging_state.data_dir)
        .expect("should create recovery staging dir");

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .readiness_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "not_ready");
    assert!(report.checks.iter().any(|check| {
        check.name == "server-settings-file"
            && check.required
            && !check.ok
            && check.detail.contains("settings.ron")
    }));
}

#[test]
fn readiness_report_blocks_when_server_settings_file_is_invalid_in_production() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    fs::create_dir_all(&state.config_dir).expect("should create config dir");
    fs::create_dir_all(&state.database_dir).expect("should create database dir");
    fs::create_dir_all(&state.ops_dir).expect("should create ops dir");
    seed_identity_file(&state.identity_file);
    seed_database_file(&state.database_dir);
    fs::write(
        server::settings::settings_file_path(&state.data_dir),
        b"this is not valid ron",
    )
    .expect("should write invalid settings");
    fs::create_dir_all(&recovery_staging_state.data_dir)
        .expect("should create recovery staging dir");

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .readiness_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "not_ready");
    assert!(report.checks.iter().any(|check| {
        check.name == "server-settings-file"
            && check.required
            && !check.ok
            && check.detail.contains("not valid RON")
    }));
}

#[test]
fn readiness_report_keeps_advisory_transport_security_material_issues_non_blocking() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    fs::create_dir_all(&state.config_dir).expect("should create config dir");
    fs::create_dir_all(&state.database_dir).expect("should create database dir");
    fs::create_dir_all(&state.ops_dir).expect("should create ops dir");
    seed_identity_file(&state.identity_file);
    seed_database_file(&state.database_dir);
    fs::create_dir_all(&recovery_staging_state.data_dir)
        .expect("should create recovery staging dir");

    let report = HealthState {
        environment: "local",
        auth_server_configured: false,
        authoritative_auth_provider: test_auth_provider(false),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: vec![crate::settings::TransportSecurityInventoryEntry {
            surface: "game-quic",
            bind_address: "127.0.0.1:14004".parse().unwrap(),
            transport: "quic",
            encryption: "tls-required",
            cert_file_path: Path::new("tls").join("cert.pem"),
            key_file_path: Path::new("tls").join("key.pem"),
            rollout_policy:
                crate::settings::TransportSecurityRolloutPolicy::ExperimentalOptInActive,
            validation_policy:
                crate::settings::TransportSecurityValidationPolicy::AdvisoryAtStartup,
            material_state: crate::settings::TransportSecurityMaterialState::Invalid,
            detail: "TLS material failed validation".to_owned(),
        }],
        governance_findings: Vec::new(),
    }
    .readiness_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "ready");
    assert!(report.checks.iter().any(|check| {
        check.name == "quic-binding-0-tls-material" && !check.required && !check.ok
    }));
}

#[test]
fn readiness_report_keeps_audit_log_target_issues_as_advisory() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    fs::create_dir_all(&state.config_dir).expect("should create config dir");
    fs::create_dir_all(&state.database_dir).expect("should create database dir");
    fs::create_dir_all(&state.ops_dir).expect("should create ops dir");
    seed_identity_file(&state.identity_file);
    seed_database_file(&state.database_dir);
    seed_settings_file(&state.data_dir);
    fs::create_dir_all(&state.audit_log_file)
        .expect("should create invalid audit log directory target");
    fs::create_dir_all(&recovery_staging_state.data_dir)
        .expect("should create recovery staging dir");

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .readiness_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "ready");
    assert!(
        report
            .checks
            .iter()
            .any(|check| { check.name == "audit-log-target-file" && !check.required && !check.ok })
    );
}

#[test]
fn readiness_report_keeps_audit_log_path_conflicts_as_advisory() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    fs::create_dir_all(&state.config_dir).expect("should create config dir");
    fs::create_dir_all(&state.database_dir).expect("should create database dir");
    fs::create_dir_all(&state.ops_dir).expect("should create ops dir");
    seed_identity_file(&state.identity_file);
    seed_database_file(&state.database_dir);
    seed_settings_file(&state.data_dir);
    fs::create_dir_all(&recovery_staging_state.data_dir)
        .expect("should create recovery staging dir");

    let mut conflict_state = state.clone();
    conflict_state.backup_evidence_log_file = conflict_state.audit_log_file.clone();

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: conflict_state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .readiness_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "ready");
    assert!(
        report.checks.iter().any(|check| {
            check.name == "audit-log-path-conflict" && !check.required && !check.ok
        })
    );
    assert!(report.checks.iter().any(|check| {
        check.name == "backup-evidence-sink-path-conflict" && !check.required && !check.ok
    }));
}

#[test]
fn metrics_contract_declares_scrape_only_semantics() {
    let contract = test_health_state(Path::new("test-root")).metrics_contract();

    assert_eq!(contract.surface, "metrics");
    assert_eq!(contract.consumption, "machine-scrape");
    assert_eq!(contract.scrape_mode, "prometheus-text-export");
    assert!(!contract.readiness_signal);
    assert!(
        contract
            .interpretation_boundary
            .contains("do not replace readiness")
    );
    assert!(contract.signal_families.iter().any(|family| {
        family.family == "server-loop-and-world-state"
            && family
                .example_metrics
                .iter()
                .any(|metric| *metric == "tick_time_hist")
    }));
    assert!(contract.signal_families.iter().any(|family| {
        family.family == "network-and-discovery"
            && family
                .example_metrics
                .iter()
                .any(|metric| *metric == "query_server::received_packets")
    }));
}

#[test]
fn health_contract_distinguishes_liveness_and_readiness_endpoints() {
    let contract = test_health_state(Path::new("test-root")).health_contract();

    assert_eq!(contract.surface, "health");
    assert_eq!(contract.consumption, "machine-probe");
    assert!(
        contract
            .endpoints
            .iter()
            .any(|endpoint| endpoint.path == "/health/ready"
                && endpoint.signal == "readiness"
                && endpoint.failure_status == Some(StatusCode::SERVICE_UNAVAILABLE.as_u16()))
    );
    assert!(
        contract
            .endpoints
            .iter()
            .any(|endpoint| endpoint.path == "/health/recovery"
                && endpoint.signal == "recovery-contract"
                && endpoint.failure_status.is_none())
    );
    assert!(
        contract
            .endpoints
            .iter()
            .any(|endpoint| endpoint.path == "/health/backup"
                && endpoint.signal == "backup-preflight"
                && endpoint.failure_status == Some(StatusCode::SERVICE_UNAVAILABLE.as_u16()))
    );
    assert!(
        contract
            .endpoints
            .iter()
            .any(|endpoint| endpoint.path == "/health/recovery/drill"
                && endpoint.signal == "recovery-drill"
                && endpoint.failure_status == Some(StatusCode::SERVICE_UNAVAILABLE.as_u16()))
    );
    assert!(
        contract
            .endpoints
            .iter()
            .any(|endpoint| endpoint.path == "/health/operations"
                && endpoint.signal == "operational-baseline"
                && endpoint.failure_status.is_none())
    );
    assert!(
        contract
            .endpoints
            .iter()
            .any(|endpoint| endpoint.path == "/health/compatibility"
                && endpoint.signal == "compatibility-contract"
                && endpoint.failure_status == Some(StatusCode::SERVICE_UNAVAILABLE.as_u16())
                && endpoint.semantics.contains("authoritative handshake")
                && endpoint.semantics.contains("lockstep"))
    );
    assert!(
        contract
            .endpoints
            .iter()
            .any(|endpoint| endpoint.path == "/health/account-auth"
                && endpoint.signal == "account-auth-governance"
                && endpoint.failure_status == Some(StatusCode::SERVICE_UNAVAILABLE.as_u16())
                && endpoint.semantics.contains("identity anchor"))
    );
    assert!(
        contract
            .endpoints
            .iter()
            .any(|endpoint| endpoint.path == "/health/management-auth"
                && endpoint.signal == "management-auth"
                && endpoint.failure_status.is_none())
    );
    assert!(
        contract
            .endpoints
            .iter()
            .any(|endpoint| endpoint.path == "/health/transport-security"
                && endpoint.signal == "transport-security"
                && endpoint.failure_status.is_none())
    );
    assert!(
        contract
            .endpoints
            .iter()
            .any(|endpoint| endpoint.path == "/health/listeners"
                && endpoint.signal == "runtime-listeners"
                && endpoint.failure_status.is_none()
                && endpoint.semantics.contains("post-start failure"))
    );
    assert!(
        contract
            .endpoints
            .iter()
            .any(|endpoint| endpoint.path == "/health/observability"
                && endpoint.signal == "observability-runtime"
                && endpoint.failure_status.is_none()
                && endpoint.semantics.contains("log-only"))
    );
    assert!(
        contract
            .endpoints
            .iter()
            .any(|endpoint| endpoint.path == "/health/preflight"
                && endpoint.signal == "operational-preflight"
                && endpoint.failure_status == Some(StatusCode::SERVICE_UNAVAILABLE.as_u16())
                && endpoint.semantics.contains("runtime listener truth")
                && endpoint.semantics.contains("management auth review"))
    );
    assert!(
        contract
            .endpoints
            .iter()
            .any(|endpoint| endpoint.path == "/health/governance"
                && endpoint.signal == "governance-audit"
                && endpoint.failure_status.is_none())
    );
    assert!(
        contract
            .endpoints
            .iter()
            .any(|endpoint| endpoint.path == "/health/surfaces"
                && endpoint.signal == "runtime-surfaces"
                && endpoint.failure_status.is_none())
    );
}

#[test]
fn governance_report_exposes_machine_readable_runtime_findings() {
    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: server::ServerStatePaths::new(Path::new("test-root").join("live")),
        recovery_staging_state: server::ServerStatePaths::new(
            Path::new("test-root").join("recovery-staging"),
        ),
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: vec![crate::settings::RuntimeGovernanceFinding {
            id: "remote-unaudited-web-opt-in-active",
            severity: crate::settings::RuntimeGovernanceSeverity::Warning,
            subject: "web-stack",
            detail: "remote web opt-in accepted for review".to_owned(),
        }],
    }
    .governance_report();

    assert_eq!(report.status, "operator_review_required");
    assert!(report.requires_operator_review);
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].id, "remote-unaudited-web-opt-in-active");
    assert_eq!(
        report.findings[0].severity,
        crate::settings::RuntimeGovernanceSeverity::Warning
    );
}

#[test]
fn account_auth_governance_report_reuses_core_contract() {
    let report = test_health_state(Path::new("test-root")).account_auth_governance_report();

    assert_eq!(report.status, "account-auth-governance");
    assert_eq!(report.environment, "production");
    assert!(report.startup_policy.startup_permitted);
    assert_eq!(
        report
            .runtime_topology
            .authoritative_auth_provider
            .as_deref(),
        Some("https://auth.example.test")
    );
    assert_eq!(
        report.principal_definition.formal_non_local_account_kind,
        "external-auth-issued-player-uuid"
    );
    assert!(report.governed_scopes.iter().any(|scope| {
        scope.scope == "character-ownership"
            && scope.anchor_kind == "player-uuid"
            && scope.anchor_source == "external-auth-provider-issued-uuid"
    }));
}

#[test]
fn account_auth_governance_report_marks_local_no_auth_as_development_only() {
    let mut health = test_health_state(Path::new("test-root"));
    health.environment = "local";
    health.auth_server_configured = false;
    health.authoritative_auth_provider = None;

    let report = health.account_auth_governance_report();

    assert_eq!(report.status, "account-auth-governance");
    assert!(report.startup_policy.no_auth_allowed_in_current_environment);
    assert!(report.startup_policy.startup_permitted);
    assert_eq!(
        report.principal_definition.current_runtime_principal_kind,
        "deterministic-local-player-uuid"
    );
    assert_eq!(
        report
            .environment_namespace_policy
            .local_development_relationship,
        "local-no-auth-identities-are-development-standins"
    );
}

#[test]
fn operational_baseline_report_exposes_tps_and_recovery_objectives() {
    let report = test_health_state(Path::new("test-root")).operational_baseline_report();

    assert_eq!(report.status, "operational-baseline");
    assert_eq!(report.target_tick_rate, crate::TPS);
    assert_eq!(report.target_tick_interval_millis, 1_000 / crate::TPS);
    assert!(
        report
            .runbook_baseline
            .iter()
            .any(|item| item.contains("/health/preflight"))
    );
    assert!(report.observability_channels.iter().any(|channel| {
        channel.channel == "runtime-log-stream"
            && !channel.authoritative_for_ops_audit
            && channel.retention_owner == "external-process-supervisor"
    }));
    assert!(report.observability_channels.iter().any(|channel| {
        channel.channel == "operational-audit-trail"
            && channel.authoritative_for_ops_audit
            && channel.retention_policy.contains("archive rotation")
    }));
    assert!(report.objectives.iter().any(|objective| {
        objective.category == "rpo"
            && objective.name == "backup-recovery-point"
            && objective
                .supporting_endpoints
                .iter()
                .any(|endpoint| *endpoint == "/health/backup")
    }));
    assert!(report.objectives.iter().any(|objective| {
        objective.category == "rto"
            && objective.name == "staged-restore-cutover"
            && objective
                .supporting_endpoints
                .iter()
                .any(|endpoint| *endpoint == "/health/recovery/drill")
    }));
}

#[test]
fn management_auth_report_exposes_control_and_observability_auth_inventory() {
    let report = HealthState {
            environment: "production",
            auth_server_configured: true,
            authoritative_auth_provider: test_auth_provider(true),
            server_state: server::ServerStatePaths::new(Path::new("test-root").join("live")),
            recovery_staging_state: server::ServerStatePaths::new(
                Path::new("test-root").join("recovery-staging"),
            ),
            audit_retention: crate::settings::AuditRetentionPolicy::default(),
            runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            runtime_observability_inventory: test_runtime_observability_inventory(),
            surface_inventory: Vec::new(),
            management_auth_inventory: vec![
                crate::settings::ManagementAuthInventoryEntry {
                    surface: "ui-api",
                    bind_address: Some("0.0.0.0:14005".parse().unwrap()),
                    reachability: crate::settings::SurfaceReachability::NetworkAccessible,
                    review_status:
                        crate::settings::SurfaceReviewStatus::PrototypeControlPlaneUnaudited,
                    remote_exposure_policy:
                        crate::settings::SurfaceRemoteExposurePolicy::RemoteRequiresExplicitWebOptInAndSecret,
                    capability: crate::settings::ManagementSurfaceCapability::MutatingControl,
                    auth_scheme: crate::settings::SurfaceAuth::ExplicitSecret,
                    credential_bootstrap:
                        crate::settings::SurfaceCredentialBootstrap::OperatorProvidedSecret,
                    credential_transport:
                        crate::settings::ManagementCredentialTransport::CookieSecret,
                    secret_config_id: Some("ui-api-secret"),
                    proxy_forwarding_forbidden: false,
                    detail: "ui api requires cookie secret".to_owned(),
                },
                crate::settings::ManagementAuthInventoryEntry {
                    surface: "metrics",
                    bind_address: Some("0.0.0.0:14005".parse().unwrap()),
                    reachability: crate::settings::SurfaceReachability::NetworkAccessible,
                    review_status: crate::settings::SurfaceReviewStatus::InternalObservabilityOnly,
                    remote_exposure_policy:
                        crate::settings::SurfaceRemoteExposurePolicy::RemoteRequiresExplicitWebOptIn,
                    capability: crate::settings::ManagementSurfaceCapability::ObservabilityScrape,
                    auth_scheme: crate::settings::SurfaceAuth::None,
                    credential_bootstrap: crate::settings::SurfaceCredentialBootstrap::None,
                    credential_transport:
                        crate::settings::ManagementCredentialTransport::None,
                    secret_config_id: None,
                    proxy_forwarding_forbidden: false,
                    detail: "metrics has no in-process auth".to_owned(),
                },
            ],
            transport_security_inventory: Vec::new(),
            governance_findings: Vec::new(),
        }
        .management_auth_report();

    assert_eq!(report.status, "operator_review_required");
    assert!(report.requires_operator_review);
    assert_eq!(report.entries.len(), 2);
    assert!(
        report
            .review_surfaces
            .iter()
            .any(|surface| *surface == "ui-api")
    );
    assert!(
        report
            .review_surfaces
            .iter()
            .any(|surface| *surface == "metrics")
    );
    assert!(report.entries.iter().any(|entry| {
        entry.surface == "ui-api"
            && entry.capability == "mutating-control"
            && entry.auth_scheme == "explicit-secret"
            && entry.credential_transport == "cookie-secret"
            && entry.secret_config_id == Some("ui-api-secret")
    }));
    assert!(report.entries.iter().any(|entry| {
        entry.surface == "metrics"
            && entry.capability == "observability-scrape"
            && entry.auth_scheme == "none"
            && entry.credential_transport == "none"
    }));
}

#[test]
fn surface_inventory_report_exposes_query_discovery_posture_without_elevating_authority() {
    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: server::ServerStatePaths::new(Path::new("test-root").join("live")),
        recovery_staging_state: server::ServerStatePaths::new(
            Path::new("test-root").join("recovery-staging"),
        ),
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: vec![crate::settings::RuntimeSurface {
            name: "query-server",
            bind_address: Some("0.0.0.0:14006".parse().unwrap()),
            reachability: crate::settings::SurfaceReachability::NetworkAccessible,
            auth: crate::settings::SurfaceAuth::None,
            credential_bootstrap: crate::settings::SurfaceCredentialBootstrap::None,
            review_status: crate::settings::SurfaceReviewStatus::DiscoveryOnlyNotAuthority,
            remote_exposure_policy:
                crate::settings::SurfaceRemoteExposurePolicy::RemoteRequiresExplicitQueryOptIn,
            purpose: crate::settings::SurfacePurpose::LightweightDiscovery,
            consumption: crate::settings::SurfaceConsumption::DiscoveryQuery,
        }],
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .surface_inventory_report();

    assert_eq!(report.status, "runtime-surface-inventory");
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].surface, "query-server");
    assert_eq!(
        report.entries[0].bind_address,
        Some("0.0.0.0:14006".to_owned())
    );
    assert_eq!(
        report.entries[0].review_status,
        "discovery-only-not-authority"
    );
    assert_eq!(
        report.entries[0].remote_exposure_policy,
        "remote-requires-explicit-query-opt-in"
    );
    assert_eq!(
        report.entries[0].authority_scope,
        Some("discovery-hint-only")
    );
    assert_eq!(report.entries[0].auth_required, Some(true));
    assert!(
        report.entries[0]
            .published_protocol_fields
            .iter()
            .any(|field| *field == "compatibility")
    );
    assert!(
        report.entries[0]
            .published_protocol_fields
            .iter()
            .any(|field| *field == "realm_id")
    );
    assert!(report.entries[0].detail.contains("compatibility"));
}

#[test]
fn surface_inventory_report_uses_auth_server_configuration_for_query_auth_requirement() {
    let report = HealthState {
        environment: "local",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: server::ServerStatePaths::new(Path::new("test-root").join("live")),
        recovery_staging_state: server::ServerStatePaths::new(
            Path::new("test-root").join("recovery-staging"),
        ),
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: vec![crate::settings::RuntimeSurface {
            name: "query-server",
            bind_address: Some("127.0.0.1:14006".parse().unwrap()),
            reachability: crate::settings::SurfaceReachability::LoopbackOnly,
            auth: crate::settings::SurfaceAuth::None,
            credential_bootstrap: crate::settings::SurfaceCredentialBootstrap::None,
            review_status: crate::settings::SurfaceReviewStatus::DiscoveryOnlyNotAuthority,
            remote_exposure_policy:
                crate::settings::SurfaceRemoteExposurePolicy::RemoteRequiresExplicitQueryOptIn,
            purpose: crate::settings::SurfacePurpose::LightweightDiscovery,
            consumption: crate::settings::SurfaceConsumption::DiscoveryQuery,
        }],
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .surface_inventory_report();

    assert_eq!(report.entries[0].auth_required, Some(true));
}

#[test]
fn surface_inventory_report_omits_query_wire_fields_when_query_surface_is_disabled() {
    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: server::ServerStatePaths::new(Path::new("test-root").join("live")),
        recovery_staging_state: server::ServerStatePaths::new(
            Path::new("test-root").join("recovery-staging"),
        ),
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: vec![crate::settings::RuntimeSurface {
            name: "query-server",
            bind_address: None,
            reachability: crate::settings::SurfaceReachability::Disabled,
            auth: crate::settings::SurfaceAuth::None,
            credential_bootstrap: crate::settings::SurfaceCredentialBootstrap::None,
            review_status: crate::settings::SurfaceReviewStatus::DiscoveryOnlyNotAuthority,
            remote_exposure_policy:
                crate::settings::SurfaceRemoteExposurePolicy::RemoteRequiresExplicitQueryOptIn,
            purpose: crate::settings::SurfacePurpose::LightweightDiscovery,
            consumption: crate::settings::SurfaceConsumption::DiscoveryQuery,
        }],
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .surface_inventory_report();

    assert_eq!(report.entries[0].authority_scope, None);
    assert!(report.entries[0].published_protocol_fields.is_empty());
    assert_eq!(report.entries[0].auth_required, None);
}

#[test]
fn compatibility_contract_report_exposes_query_v2_as_hint_only() {
    let report = test_health_state(Path::new("test-root")).compatibility_contract_report();

    assert_eq!(report.status, "compatibility-contract-aligned");
    assert_eq!(report.authoritative_handshake.surface, "realm-handshake");
    assert_eq!(
        report.authoritative_handshake.authority_scope,
        "authoritative"
    );
    assert_eq!(
        report.authoritative_handshake.auth_mode,
        common_net::msg::ServerAuthMode::ExternalProvider.as_str()
    );
    assert_eq!(
        report.authoritative_handshake.auth_provider.as_deref(),
        Some("https://auth.example.test")
    );
    assert_eq!(report.query_hint.surface, "query-server");
    assert_eq!(report.query_hint.authority_scope, "discovery-hint-only");
    assert_eq!(
        report.query_hint.auth_hint_scope,
        "auth-requirement-only-hint"
    );
    assert_eq!(
        report.query_hint.protocol_version,
        veloren_query_server::proto::CURRENT_PROTOCOL_VERSION
    );
    assert_eq!(
        report.query_hint.version_selection_policy,
        veloren_query_server::proto::VERSION_SELECTION_POLICY
    );
    assert!(!report.query_hint.supports_multi_version_negotiation);
    assert!(report.query_protocol_rollout.requires_lockstep_rollout);
    assert!(
        report
            .query_protocol_rollout
            .current_stage_policy
            .contains("formal policy")
    );
    assert!(
        report
            .query_protocol_rollout
            .policy_change_requirement
            .contains("multi-version negotiation")
    );
    assert_eq!(
        report.query_protocol_rollout.authoritative_client_path,
        "official_entry -> EntryPolicy -> realm handshake"
    );
    assert!(
        report
            .query_protocol_rollout
            .known_in_repo_consumers
            .iter()
            .any(|consumer| consumer.contains("examples"))
    );
    assert!(
        report
            .query_protocol_rollout
            .mixed_version_policy
            .contains("unsupported")
    );
    assert!(
        report
            .query_protocol_rollout
            .safe_transition_options
            .iter()
            .any(|option| option.contains("disabled"))
    );
    assert!(
        report
            .query_protocol_rollout
            .upgrade_order
            .iter()
            .any(|step| step.contains("same protocol version"))
    );
    assert!(
        report
            .query_protocol_rollout
            .rollback_order
            .iter()
            .any(|step| step.contains("version parity"))
    );
    assert!(report.environment_matches);
    assert!(report.compatibility_matches);
    assert!(report.auth_requirement_matches_runtime_config);
    assert!(
        report
            .query_hint
            .published_protocol_fields
            .iter()
            .any(|field| *field == "compatibility")
    );
    assert_eq!(report.public_entry_handoff.signal, "public-entry-handoff");
    assert_eq!(
        report.public_entry_handoff.status,
        "external-review-required"
    );
    assert!(
        report
            .public_entry_handoff
            .applies_to_non_local_public_rollout
    );
    assert!(report.public_entry_handoff.requires_operator_review);
    assert!(!report.public_entry_handoff.release_blocked);
    assert!(
        report
            .public_entry_handoff
            .development_stage_closure_available_without_real_materials
    );
    assert!(
        report
            .public_entry_handoff
            .development_stage_closure_scope
            .contains("typed Public handoff contract")
    );
    assert!(
        report
            .public_entry_handoff
            .real_cutover_still_requires_external_materials
    );
    assert!(
        report
            .public_entry_handoff
            .real_cutover_dependency_boundary
            .contains("real realm/auth authority material")
    );
    assert_eq!(
        report.public_entry_handoff.expected_handshake_auth_mode,
        common_net::msg::ServerAuthMode::ExternalProvider.as_str()
    );
    assert_eq!(
        report
            .public_entry_handoff
            .authoritative_handshake_auth_provider
            .as_deref(),
        Some("https://auth.example.test")
    );
    assert!(
        report
            .public_entry_handoff
            .machine_verification_available_in_this_process
            == report
                .public_entry_handoff
                .repo_bundled_official_entry_snapshot
                .baseline
                .is_some()
    );
    assert_eq!(
        report.public_entry_handoff.machine_verification_scope,
        "repo/local bundled official_entry baseline only"
    );
    assert!(
        report
            .public_entry_handoff
            .machine_verification_limitations
            .contains("shipped Public client artifact")
    );
    assert!(
        report
            .public_entry_handoff
            .repo_bundled_official_entry_snapshot
            .required_external_match_fields
            .iter()
            .any(|field| *field == "bundled_official_entry_artifact_identity")
    );
    assert!(
        report
            .public_entry_handoff
            .repo_bundled_official_entry_snapshot
            .required_external_match_fields
            .iter()
            .any(|field| *field == "bundled_target_kind")
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_fields
            .iter()
            .any(|field| *field == "result_status")
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_fields
            .iter()
            .any(|field| *field == "bundled_public_client_artifact_reference")
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_fields
            .iter()
            .any(|field| *field == "bundled_official_entry_artifact_identity")
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_fields
            .iter()
            .any(|field| *field == "bundled_official_entry_server_address")
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_fields
            .iter()
            .any(|field| *field == "bundled_official_entry_auth_server")
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_fields
            .iter()
            .any(|field| *field == "bundled_official_entry_use_srv")
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_fields
            .iter()
            .any(|field| *field == "bundled_official_entry_validate_tls")
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_fields
            .iter()
            .any(|field| *field == "bundled_target_kind")
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_fields
            .iter()
            .any(|field| *field == "non_local_cutover_gap_reasons")
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_fields
            .iter()
            .any(|field| *field == "target_runtime_environment")
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_fields
            .iter()
            .any(|field| *field == "authoritative_compatibility_generation")
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_fields
            .iter()
            .any(|field| *field == "backup_evidence_reference")
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_fields
            .iter()
            .any(|field| *field == "recovery_drill_reference")
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_fields
            .iter()
            .any(|field| *field == "rollback_reference")
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_fields
            .iter()
            .any(|field| *field == "rollback_public_client_artifact_reference")
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_fields
            .iter()
            .any(|field| *field == "rollback_bundled_official_entry_artifact_identity")
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_fields
            .iter()
            .any(|field| *field == "expected_handshake_auth_mode")
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_fields
            .iter()
            .any(|field| *field == "authoritative_handshake_auth_provider")
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_fields
            .iter()
            .any(|field| *field == "query_auth_required_hint")
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_field_contracts
            .iter()
            .any(
                |field| field.name == "bundled_public_client_artifact_reference"
                    && field.value_kind == "release-artifact-reference"
                    && field.evidence_source.contains("external-release-tracker")
            )
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_field_contracts
            .iter()
            .any(|field| field.name == "bundled_target_kind"
                && field.value_kind == "target-kind-enum"
                && field.evidence_source == "client-exported-entry-contract")
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_field_contracts
            .iter()
            .any(
                |field| field.name == "authoritative_handshake_auth_provider"
                    && field.value_kind == "url-or-null"
                    && field.evidence_source == "/health/compatibility"
            )
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_field_contracts
            .iter()
            .any(|field| field.name == "ready_report_status"
                && field.evidence_source == "/health/ready")
    );
    assert!(
        report
            .public_entry_handoff
            .required_cutover_preconditions
            .iter()
            .any(|item| item.contains("authoritative handshake auth provider"))
    );
    assert!(
        report
            .public_entry_handoff
            .required_cutover_preconditions
            .iter()
            .any(|item| item.contains("target runtime environment"))
    );
    assert!(
        report
            .public_entry_handoff
            .required_cutover_preconditions
            .iter()
            .any(|item| item.contains("shipped Public client artifact reference"))
    );
    assert!(
        report
            .public_entry_handoff
            .required_cutover_preconditions
            .iter()
            .any(|item| item.contains("rollback_bundled_official_entry_artifact_identity"))
    );
    assert!(
        report
            .public_entry_handoff
            .required_cutover_material_checklist
            .iter()
            .any(|item| item.id == "bundled-public-entry-artifact-reviewed"
                && item
                    .completion_criteria
                    .contains("shipped Public client artifact reference")
                && item.current_stage_status == CUTOVER_MATERIAL_STATUS_EXTERNAL_MATERIAL_REQUIRED
                && item
                    .operator_next_step
                    .contains("client-exported bundled entry posture"))
    );
    assert!(
        report
            .public_entry_handoff
            .required_cutover_material_checklist
            .iter()
            .any(|item| item.id == "external-auth-authority-pinned"
                && item.required_for_cutover
                && item.current_repo_baseline.contains("auth_server is None")
                && item.current_stage_status == CUTOVER_MATERIAL_STATUS_REPO_BASELINE_BLOCKED
                && item
                    .current_stage_detail
                    .contains("auth_provider is https://auth.example.test"))
    );
    assert!(
        report
            .public_entry_handoff
            .required_cutover_material_checklist
            .iter()
            .any(|item| item.id == "non-local-target-material-ready"
                && item.completion_criteria.contains("non_local_cutover_ready")
                && item.current_stage_status == CUTOVER_MATERIAL_STATUS_REPO_BASELINE_BLOCKED
                && item
                    .current_stage_detail
                    .contains("private-or-unique-local-ip"))
    );
    assert!(
        report
            .public_entry_handoff
            .required_cutover_material_checklist
            .iter()
            .any(|item| item.id == "authoritative-runtime-target-confirmed"
                && item
                    .completion_criteria
                    .contains("authoritative_handshake_auth_provider")
                && item.current_stage_status == CUTOVER_MATERIAL_STATUS_OPERATOR_CAPTURE_REQUIRED
                && item.current_stage_detail.contains("environment=production"))
    );
    assert!(
        report
            .public_entry_handoff
            .required_cutover_material_checklist
            .iter()
            .any(|item| item.id == "rollback-path-recorded"
                && item
                    .completion_criteria
                    .contains("rollback_public_client_artifact_reference")
                && item
                    .completion_criteria
                    .contains("rollback_bundled_official_entry_artifact_identity")
                && item.current_stage_status == CUTOVER_MATERIAL_STATUS_EXTERNAL_MATERIAL_REQUIRED
                && item
                    .operator_next_step
                    .contains("rollback_public_client_artifact_reference"))
    );
    assert!(
        report
            .public_entry_handoff
            .required_cutover_preconditions
            .iter()
            .any(|item| item.contains("ready_report_status"))
    );
    let transition_contract = report
        .public_entry_handoff
        .public_entry_transition_contract
        .as_ref()
        .expect("non-local public handoff should expose a transition contract");
    assert_eq!(
        transition_contract.transition_scope,
        "non-local Public official_entry cutover transition unit"
    );
    assert_eq!(
        transition_contract.record_scope,
        "same public-entry-handoff section keyed by release_reference"
    );
    assert!(
        transition_contract
            .atomic_bundle_fields
            .iter()
            .any(|field| *field == "bundled_official_entry_server_address")
    );
    assert!(
        transition_contract
            .atomic_bundle_fields
            .iter()
            .any(|field| *field == "bundled_official_entry_auth_server")
    );
    assert!(
        transition_contract
            .atomic_bundle_fields
            .iter()
            .any(|field| *field == "bundled_official_entry_use_quic")
    );
    assert!(
        transition_contract
            .atomic_runtime_gate_fields
            .iter()
            .any(|field| *field == "authoritative_handshake_auth_provider")
    );
    assert!(
        transition_contract
            .atomic_runtime_gate_fields
            .iter()
            .any(|field| *field == "ready_report_status")
    );
    assert!(
        transition_contract
            .atomic_rollback_restore_fields
            .iter()
            .any(|field| *field == "rollback_public_client_artifact_reference")
    );
    assert!(
        transition_contract
            .forbidden_partial_transitions
            .iter()
            .any(|rule| rule.contains("official_entry.server_address"))
    );
    assert!(
        transition_contract
            .forbidden_partial_transitions
            .iter()
            .any(|rule| rule.contains("official_entry.auth_server"))
    );
    assert!(
        transition_contract
            .approval_gate
            .contains("same release_reference")
    );
    let lifecycle_contract = report
        .public_entry_handoff
        .public_entry_lifecycle_transition_contract
        .as_ref()
        .expect("non-local public handoff should expose a lifecycle transition contract");
    assert_eq!(lifecycle_contract.initial_state, "draft");
    assert_eq!(lifecycle_contract.evidence_ready_state, "evidence-linked");
    assert!(
        lifecycle_contract
            .terminal_states_requiring_archive_receipt
            .iter()
            .any(|state| *state == "cutover-approved")
    );
    assert!(
        lifecycle_contract
            .unsupported_paths
            .iter()
            .any(|path| path.contains("exception-accepted"))
    );
    assert!(lifecycle_contract.transitions.iter().any(|transition| {
        transition.from_state == "draft"
            && transition.to_state == "evidence-linked"
            && transition.approval_decision.is_none()
            && !transition.archive_required
            && transition
                .required_fields
                .iter()
                .any(|field| *field == "bundled_public_client_artifact_reference")
            && transition
                .required_fields
                .iter()
                .any(|field| *field == "backup_evidence_reference")
    }));
    assert!(lifecycle_contract.transitions.iter().any(|transition| {
        transition.from_state == "evidence-linked"
            && transition.to_state == "cutover-approved"
            && transition.approval_decision == Some("approved")
            && transition.archive_required
            && transition
                .required_fields
                .iter()
                .any(|field| *field == "rollback_public_client_artifact_reference")
    }));
    assert!(lifecycle_contract.transitions.iter().any(|transition| {
        transition.from_state == "cutover-approved"
            && transition.to_state == "rolled-back"
            && transition.approval_decision == Some("approved")
            && transition
                .notes
                .iter()
                .any(|note| note.contains("previously cutover-approved"))
    }));
    assert!(
        report
            .public_entry_handoff
            .required_authority_pairing_checks
            .iter()
            .any(|check| {
                check.id == "bundled-artifact-vs-release-unit"
                    && check
                        .review_fields
                        .iter()
                        .any(|field| *field == "bundled_public_client_artifact_reference")
            })
    );
    assert!(
        report
            .public_entry_handoff
            .required_authority_pairing_checks
            .iter()
            .any(
                |check| check.id == "bundled-auth-pin-vs-handshake-authority"
                    && check.release_blocking_on_mismatch
                    && check
                        .review_fields
                        .iter()
                        .any(|field| *field == "bundled_official_entry_auth_server")
                    && check
                        .review_fields
                        .iter()
                        .any(|field| *field == "authoritative_handshake_auth_provider")
            )
    );
    assert!(
        report
            .public_entry_handoff
            .required_authority_pairing_checks
            .iter()
            .any(|check| {
                check.id == "rollback-entry-material-vs-rollback-path"
                    && check
                        .review_fields
                        .iter()
                        .any(|field| *field == "rollback_public_client_artifact_reference")
                    && check
                        .review_fields
                        .iter()
                        .any(|field| *field == "rollback_bundled_official_entry_artifact_identity")
            })
    );
    assert!(
        report
            .public_entry_handoff
            .required_authority_pairing_checks
            .iter()
            .any(|check| {
                check.id == "rollback-entry-material-vs-rollback-path"
                    && check
                        .review_fields
                        .iter()
                        .any(|field| *field == "rollback_bundled_official_entry_artifact_identity")
            })
    );
    assert!(
        report
            .public_entry_handoff
            .supporting_health_endpoints
            .iter()
            .any(|endpoint| *endpoint == "/health/ready")
    );
    assert!(
        report
            .public_entry_handoff
            .supporting_health_endpoints
            .iter()
            .any(|endpoint| *endpoint == "/health/recovery/drill")
    );
    assert!(report.operator_consumption.contains("authoritative"));
}

#[test]
fn compatibility_contract_report_marks_local_public_entry_handoff_as_not_applicable() {
    let report = HealthState {
        environment: "local",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: server::ServerStatePaths::new(Path::new("test-root").join("live")),
        recovery_staging_state: server::ServerStatePaths::new(
            Path::new("test-root").join("recovery-staging"),
        ),
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .compatibility_contract_report();

    assert_eq!(report.public_entry_handoff.status, "not-applicable-local");
    assert!(
        !report
            .public_entry_handoff
            .applies_to_non_local_public_rollout
    );
    assert!(!report.public_entry_handoff.requires_operator_review);
    assert!(!report.public_entry_handoff.release_blocked);
    assert!(
        report
            .public_entry_handoff
            .development_stage_closure_available_without_real_materials
    );
    assert_eq!(
        report.public_entry_handoff.development_stage_closure_status,
        "not-applicable-local"
    );
    assert!(
        !report
            .public_entry_handoff
            .real_cutover_still_requires_external_materials
    );
    assert_eq!(
        report.public_entry_handoff.real_cutover_execution_status,
        "not-applicable-local"
    );
    assert!(
        report
            .public_entry_handoff
            .real_cutover_dependency_boundary
            .contains("outside local-mode scope")
    );
    assert!(
        report
            .public_entry_handoff
            .remaining_external_execution_dependencies
            .is_empty()
    );
    assert_eq!(
        report.authoritative_handshake.auth_provider.as_deref(),
        Some("https://auth.example.test")
    );
    assert_eq!(
        report
            .public_entry_handoff
            .authoritative_handshake_auth_provider
            .as_deref(),
        Some("https://auth.example.test")
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_fields
            .is_empty()
    );
    assert!(
        report
            .public_entry_handoff
            .required_external_review_field_contracts
            .is_empty()
    );
    assert!(
        report
            .public_entry_handoff
            .required_cutover_preconditions
            .is_empty()
    );
    assert!(
        report
            .public_entry_handoff
            .required_cutover_material_checklist
            .is_empty()
    );
    assert!(
        report
            .public_entry_handoff
            .required_authority_pairing_checks
            .is_empty()
    );
    assert!(
        report
            .public_entry_handoff
            .supporting_health_endpoints
            .is_empty()
    );
    assert!(
        report
            .public_entry_handoff
            .public_entry_transition_contract
            .is_none()
    );
    assert!(
        report
            .public_entry_handoff
            .public_entry_lifecycle_transition_contract
            .is_none()
    );
    assert!(
        report
            .public_entry_handoff
            .section_instance_validation_contract
            .is_none()
    );
}

#[test]
fn compatibility_contract_report_blocks_non_local_public_entry_handoff_without_external_auth() {
    let report = HealthState {
        environment: "production",
        auth_server_configured: false,
        authoritative_auth_provider: test_auth_provider(false),
        server_state: server::ServerStatePaths::new(Path::new("test-root").join("live")),
        recovery_staging_state: server::ServerStatePaths::new(
            Path::new("test-root").join("recovery-staging"),
        ),
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .compatibility_contract_report();

    assert_eq!(
        report.public_entry_handoff.status,
        "non-local-public-rollout-unsupported"
    );
    assert!(
        report
            .public_entry_handoff
            .applies_to_non_local_public_rollout
    );
    assert!(!report.public_entry_handoff.requires_operator_review);
    assert!(report.public_entry_handoff.release_blocked);
    assert!(
        report
            .public_entry_handoff
            .development_stage_closure_available_without_real_materials
    );
    assert_eq!(
        report.public_entry_handoff.development_stage_closure_status,
        "development-contract-closure-available"
    );
    assert!(
        report
            .public_entry_handoff
            .real_cutover_still_requires_external_materials
    );
    assert_eq!(
        report.public_entry_handoff.real_cutover_execution_status,
        "blocked-by-runtime-auth-posture-and-external-materials"
    );
    assert!(
        report
            .public_entry_handoff
            .real_cutover_dependency_boundary
            .contains("supported external-auth handshake posture")
    );
    assert_eq!(
        report.public_entry_handoff.expected_handshake_auth_mode,
        common_net::msg::ServerAuthMode::NoExternalAuth.as_str()
    );
    assert_eq!(report.authoritative_handshake.auth_provider, None);
    assert_eq!(
        report
            .public_entry_handoff
            .authoritative_handshake_auth_provider,
        None
    );
    assert!(
        report
            .public_entry_handoff
            .public_entry_transition_contract
            .is_some()
    );
    assert!(
        report
            .public_entry_handoff
            .public_entry_lifecycle_transition_contract
            .is_some()
    );
    assert!(
        report
            .public_entry_handoff
            .remaining_external_execution_dependencies
            .iter()
            .any(|dependency| {
                dependency.id == "external-auth-runtime-authority"
                    && dependency.current_stage_status == "runtime-posture-unsupported"
                    && !dependency.blocks_development_stage_closure
                    && dependency.blocks_real_cutover
            })
    );
}

#[test]
fn public_entry_handoff_schema_stays_aligned_between_compatibility_and_preflight() {
    let root = unique_temp_dir();
    let live_state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    seed_live_runtime_state(&live_state);
    seed_recovery_staging_restore_state(&recovery_staging_state);

    let health = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: live_state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    };

    let compatibility = health.compatibility_contract_report();
    let preflight = health.preflight_report();

    let _ = fs::remove_dir_all(root);

    let preflight_contract = preflight
        .review_decision_contracts
        .iter()
        .find(|contract| contract.signal == "public-entry-handoff")
        .expect("public-entry-handoff decision contract should exist");

    assert_eq!(
        compatibility
            .public_entry_handoff
            .required_external_review_fields,
        preflight_contract.required_decision_fields
    );
    assert_eq!(
        preflight.repo_bundled_official_entry_snapshot.status,
        compatibility
            .public_entry_handoff
            .repo_bundled_official_entry_snapshot
            .status
    );
    assert_eq!(
        preflight.repo_bundled_official_entry_snapshot.baseline,
        compatibility
            .public_entry_handoff
            .repo_bundled_official_entry_snapshot
            .baseline
    );
    assert_eq!(
        compatibility
            .public_entry_handoff
            .required_external_review_field_contracts,
        preflight_contract.required_decision_field_contracts
    );
    assert_eq!(
        compatibility
            .public_entry_handoff
            .required_authority_pairing_checks,
        preflight_contract.authority_pairing_checks
    );
    assert_eq!(
        compatibility
            .public_entry_handoff
            .public_entry_transition_contract,
        preflight_contract.public_entry_transition_contract
    );
    assert_eq!(
        compatibility
            .public_entry_handoff
            .public_entry_lifecycle_transition_contract,
        preflight_contract.public_entry_lifecycle_transition_contract
    );
    assert_eq!(
        compatibility
            .public_entry_handoff
            .section_instance_validation_contract,
        preflight_contract.section_instance_validation_contract
    );
    assert_eq!(
        compatibility
            .public_entry_handoff
            .section_instance_validation_contract
            .as_ref()
            .map(|contract| &contract.snapshot_input_contract),
        preflight_contract
            .section_instance_validation_contract
            .as_ref()
            .map(|contract| &contract.snapshot_input_contract)
    );
    assert_eq!(
        compatibility
            .public_entry_handoff
            .section_instance_validation_contract
            .as_ref()
            .map(|contract| &contract.snapshot_template_contract),
        preflight_contract
            .section_instance_validation_contract
            .as_ref()
            .map(|contract| &contract.snapshot_template_contract)
    );
    assert_eq!(
        compatibility
            .public_entry_handoff
            .section_instance_validation_contract
            .as_ref()
            .map(|contract| &contract.minimum_snapshot_example),
        preflight_contract
            .section_instance_validation_contract
            .as_ref()
            .map(|contract| &contract.minimum_snapshot_example)
    );
    assert_eq!(
        compatibility
            .public_entry_handoff
            .section_instance_validation_contract
            .as_ref()
            .map(|contract| &contract.validation_result_contract),
        preflight_contract
            .section_instance_validation_contract
            .as_ref()
            .map(|contract| &contract.validation_result_contract)
    );
    assert_eq!(
        compatibility
            .public_entry_handoff
            .section_instance_validation_contract
            .as_ref()
            .map(|contract| &contract.minimum_validation_result_example),
        preflight_contract
            .section_instance_validation_contract
            .as_ref()
            .map(|contract| &contract.minimum_validation_result_example)
    );
    assert_eq!(
        compatibility
            .public_entry_handoff
            .supporting_health_endpoints,
        preflight_contract
            .supporting_endpoints
            .iter()
            .map(|endpoint| endpoint.endpoint)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        preflight.development_stage_closure_status,
        compatibility
            .public_entry_handoff
            .development_stage_closure_status
    );
    assert_eq!(
        preflight.real_cutover_execution_status,
        compatibility
            .public_entry_handoff
            .real_cutover_execution_status
    );
    assert_eq!(
        preflight.remaining_external_execution_dependencies,
        compatibility
            .public_entry_handoff
            .remaining_external_execution_dependencies
    );
    let lifecycle_contract = preflight_contract
        .public_entry_lifecycle_transition_contract
        .as_ref()
        .expect("public-entry-handoff preflight contract should expose lifecycle transitions");
    let validation_contract = preflight_contract
        .section_instance_validation_contract
        .as_ref()
        .expect("public-entry-handoff preflight contract should expose section validation");
    let readiness_summary = preflight_contract
        .validator_integration_readiness_summary
        .as_ref()
        .expect("public-entry-handoff preflight contract should expose validator readiness");
    assert_eq!(readiness_summary.status, "validator-contract-ready");
    assert_eq!(
        readiness_summary.input_snapshot_kind,
        validation_contract.snapshot_input_contract.snapshot_kind
    );
    assert_eq!(
        readiness_summary.field_values_key,
        validation_contract.snapshot_input_contract.field_values_key
    );
    assert_eq!(
        readiness_summary.lifecycle_state_field,
        validation_contract.lifecycle_state_field
    );
    assert_eq!(
        readiness_summary.output_result_kind,
        validation_contract.validation_result_contract.result_kind
    );
    assert_eq!(
        readiness_summary.output_stage_status_field,
        validation_contract
            .validation_result_contract
            .stage_status_field
    );
    assert_eq!(
        readiness_summary.blocking_interpretation,
        validation_contract.blocking_interpretation
    );
    assert_eq!(
        lifecycle_contract.terminal_states_requiring_archive_receipt,
        preflight_contract
            .archive_handoff_contract
            .terminal_states_requiring_archive_receipt
    );
    assert_eq!(
        validation_contract
            .terminal_requirements
            .accepted_result_statuses,
        lifecycle_contract.terminal_states_requiring_archive_receipt
    );
    assert_eq!(
        validation_contract
            .archive_receipt_requirements
            .accepted_result_statuses,
        preflight_contract
            .archive_handoff_contract
            .terminal_states_requiring_archive_receipt
    );
    assert_eq!(
        validation_contract.required_archive_correlation_dimensions,
        preflight_contract
            .archive_handoff_contract
            .required_archive_correlation_dimensions
    );
    assert_eq!(
        validation_contract.source_record_state_field,
        "source_record_state"
    );
    assert_eq!(
        validation_contract.snapshot_input_contract.snapshot_kind,
        "external-release-review-section-snapshot-v1"
    );
    assert_eq!(
        validation_contract.snapshot_input_contract.field_values_key,
        "field_values"
    );
    assert_eq!(
        validation_contract.snapshot_template_contract.snapshot_kind,
        "external-release-review-section-snapshot-v1"
    );
    assert!(
        validation_contract
            .snapshot_template_contract
            .top_level_fields
            .iter()
            .any(|field| field.name == "field_values")
    );
    assert!(
        validation_contract
            .snapshot_template_contract
            .field_value_entries
            .iter()
            .any(|field| field.name == "archive_reference")
    );
    assert!(
        validation_contract
            .minimum_snapshot_example
            .illustrative_only
    );
    assert!(
        validation_contract
            .minimum_snapshot_example
            .top_level_fields
            .iter()
            .any(|field| field.name == "prior_result_statuses"
                && field.value == "[\"cutover-approved\"]")
    );
    assert!(
        validation_contract
            .minimum_snapshot_example
            .field_value_entries
            .iter()
            .any(|field| field.name == "result_status" && field.value == "rolled-back")
    );
    assert!(
        validation_contract
            .minimum_snapshot_example
            .field_value_entries
            .iter()
            .any(|field| field.name == "archive_reference")
    );
    assert_eq!(
        validation_contract.validation_result_contract.result_kind,
        "external-section-validation-result-v1"
    );
    assert_eq!(
        validation_contract
            .validation_result_contract
            .stage_status_field,
        "stage_status"
    );
    assert!(
        validation_contract
            .validation_result_contract
            .required_fields
            .iter()
            .any(|field| field.name == "highest_satisfied_stage")
    );
    assert!(
        validation_contract
            .validation_result_contract
            .optional_fields
            .iter()
            .any(|field| field.name == "missing_required_fields")
    );
    assert!(
        validation_contract
            .minimum_validation_result_example
            .illustrative_only
    );
    assert!(
        validation_contract
            .minimum_validation_result_example
            .fields
            .iter()
            .any(|field| field.name == "stage_status" && field.value == "valid")
    );
    assert!(
        validation_contract
            .minimum_validation_result_example
            .fields
            .iter()
            .any(|field| {
                field.name == "highest_satisfied_stage" && field.value == "post-archive-verified"
            })
    );
    assert_eq!(
        validation_contract.validation_result_contract.result_kind,
        "external-section-validation-result-v1"
    );
    assert_eq!(
        validation_contract
            .validation_result_contract
            .stage_status_field,
        "stage_status"
    );
    assert!(
        validation_contract
            .validation_result_contract
            .required_fields
            .iter()
            .any(|field| field.name == "highest_satisfied_stage")
    );
    assert!(
        validation_contract
            .validation_result_contract
            .optional_fields
            .iter()
            .any(|field| field.name == "missing_required_fields")
    );
    assert!(
        validation_contract
            .minimum_validation_result_example
            .illustrative_only
    );
    assert!(
        validation_contract
            .minimum_validation_result_example
            .fields
            .iter()
            .any(|field| field.name == "stage_status" && field.value == "valid")
    );
    assert!(
        validation_contract
            .minimum_validation_result_example
            .fields
            .iter()
            .any(|field| field.name == "highest_satisfied_stage"
                && field.value == "post-archive-verified")
    );
    assert!(
        validation_contract
            .snapshot_input_contract
            .required_top_level_fields
            .iter()
            .any(|field| field.name == "record_kind")
    );
    assert!(
        validation_contract
            .snapshot_input_contract
            .always_present_field_values
            .iter()
            .any(|field| field.name == "result_status")
    );
    assert!(
        validation_contract
            .snapshot_input_contract
            .stage_scoped_field_values
            .iter()
            .any(|field| field.name == "archive_reference")
    );
    assert_eq!(
        validation_contract
            .snapshot_input_contract
            .prior_result_statuses_required_for_states,
        vec!["rolled-back"]
    );
    assert_eq!(
        validation_contract
            .draft_requirements
            .accepted_result_statuses,
        vec!["draft"]
    );
    assert_eq!(
        validation_contract
            .draft_requirements
            .required_workflow_steps,
        vec![1]
    );
    assert!(
        validation_contract
            .draft_requirements
            .conditional_required_fields
            .is_empty()
    );
    assert!(
        validation_contract
            .draft_requirements
            .required_fields
            .iter()
            .any(|field| *field == "reviewed_by")
    );
    assert!(
        validation_contract
            .draft_requirements
            .required_fields
            .iter()
            .any(|field| *field == "decision_recorded_at_utc")
    );
    assert_eq!(
        validation_contract
            .evidence_linked_requirements
            .accepted_result_statuses,
        vec!["evidence-linked"]
    );
    assert_eq!(
        validation_contract
            .evidence_linked_requirements
            .required_workflow_steps,
        vec![1, 2, 3]
    );
    assert!(
        validation_contract
            .evidence_linked_requirements
            .conditional_required_fields
            .is_empty()
    );
    assert!(
        validation_contract
            .evidence_linked_requirements
            .required_fields
            .iter()
            .any(|field| *field == "bundled_public_client_artifact_reference")
    );
    assert!(
        validation_contract
            .evidence_linked_requirements
            .required_fields
            .iter()
            .any(|field| *field == "authoritative_handshake_auth_provider")
    );
    assert!(
        validation_contract
            .evidence_linked_requirements
            .required_fields
            .iter()
            .any(|field| *field == "backup_evidence_reference")
    );
    assert!(
        validation_contract
            .evidence_linked_requirements
            .required_fields
            .iter()
            .any(|field| *field == "recovery_drill_reference")
    );
    assert!(
        validation_contract
            .terminal_requirements
            .required_workflow_steps
            .iter()
            .copied()
            .eq([1, 2, 3, 4].into_iter())
    );
    assert!(
        validation_contract
            .terminal_requirements
            .conditional_required_fields
            .is_empty()
    );
    assert!(
        validation_contract
            .terminal_requirements
            .required_fields
            .iter()
            .any(|field| *field == "approval_decision")
    );
    assert!(
        validation_contract
            .terminal_requirements
            .required_fields
            .iter()
            .any(|field| *field == "rollback_reference")
    );
    assert!(
        validation_contract
            .terminal_requirements
            .required_additional_checks
            .iter()
            .any(|check| check.contains("rolled-back"))
    );
    assert!(
        validation_contract
            .archive_receipt_requirements
            .required_workflow_steps
            .iter()
            .copied()
            .eq([1, 2, 3, 4, 5].into_iter())
    );
    assert!(
        validation_contract
            .archive_receipt_requirements
            .conditional_required_fields
            .is_empty()
    );
    assert!(
        validation_contract
            .archive_receipt_requirements
            .required_fields
            .iter()
            .any(|field| *field == "archive_reference")
    );
    assert!(
        validation_contract
            .archive_receipt_requirements
            .required_fields
            .iter()
            .any(|field| *field == "source_record_state")
    );
    assert!(
        validation_contract
            .archive_receipt_requirements
            .required_additional_checks
            .iter()
            .any(|check| check.contains("source_record_state"))
    );
    assert!(
        validation_contract
            .post_archive_verification_requirements
            .required_workflow_steps
            .iter()
            .copied()
            .eq([1, 2, 3, 4, 5, 6].into_iter())
    );
    assert!(
        validation_contract
            .post_archive_verification_requirements
            .conditional_required_fields
            .is_empty()
    );
    assert!(
        validation_contract
            .post_archive_verification_requirements
            .required_fields
            .iter()
            .any(|field| *field == "post_archive_verified_by")
    );
    assert!(
        validation_contract
            .post_archive_verification_requirements
            .required_fields
            .iter()
            .any(|field| *field == "post_archive_verification_reference")
    );
    assert!(
        !validation_contract
            .post_archive_verification_requirements
            .blocking_until_satisfied
    );
    assert!(
        validation_contract
            .post_archive_verification_requirements
            .required_additional_checks
            .iter()
            .any(|check| check.contains("archive receipt fields"))
    );
    assert!(
        validation_contract
            .required_authority_pairing_check_ids
            .iter()
            .any(|id| *id == "bundled-auth-pin-vs-handshake-authority")
    );
    assert!(
        validation_contract
            .required_authority_pairing_check_ids
            .iter()
            .any(|id| *id == "rollback-entry-material-vs-rollback-path")
    );
    assert!(
        validation_contract
            .forbidden_shortcuts
            .iter()
            .any(|rule| rule.contains("rollback Public client artifact reference"))
    );
    assert!(
        validation_contract
            .forbidden_post_terminal_mutations
            .iter()
            .any(|rule| rule.contains("bundled_public_client_artifact_reference"))
    );
    assert!(
        validation_contract
            .blocking_interpretation
            .contains("post-archive verification")
    );
    assert!(lifecycle_contract.transitions.iter().any(|transition| {
        transition.from_state == "evidence-linked"
            && transition.to_state == "cutover-rejected"
            && transition.approval_decision == Some("rejected")
    }));
    let transition_contract = preflight_contract
        .public_entry_transition_contract
        .as_ref()
        .expect("public-entry-handoff preflight contract should expose transition rules");
    assert!(
        transition_contract
            .atomic_bundle_fields
            .iter()
            .any(|field| *field == "bundled_official_entry_validate_tls")
    );
    assert!(
        transition_contract
            .atomic_runtime_gate_fields
            .iter()
            .any(|field| *field == "backup_evidence_reference")
    );
    assert!(
        transition_contract
            .atomic_rollback_restore_fields
            .iter()
            .any(|field| *field == "rollback_bundled_official_entry_artifact_identity")
    );
    assert!(
        transition_contract
            .forbidden_partial_transitions
            .iter()
            .any(|rule| rule.contains("rollback_reference"))
    );
    assert_eq!(
        preflight_contract
            .record_lifecycle_contract
            .minimum_complete_record_fields,
        preflight_contract.required_decision_fields
    );
    assert!(
        preflight_contract
            .required_decision_fields
            .iter()
            .any(|field| *field == "result_status")
    );
    assert!(
        preflight_contract
            .record_lifecycle_contract
            .same_record_must_link_rollout_and_rollback
    );
    assert!(preflight_contract.result_status_model.iter().any(|state| {
        state.state == "cutover-approved"
            && state
                .semantics
                .contains("reopening non-local Public traffic")
    }));
    assert!(
        preflight_contract
            .result_status_model
            .iter()
            .any(|state| state.state == "rolled-back" && state.semantics.contains("reverted"))
    );
    assert!(
        preflight_contract
            .archive_handoff_contract
            .terminal_states_requiring_archive_receipt
            .iter()
            .any(|state| *state == "cutover-approved")
    );
    assert!(
        preflight_contract
            .archive_handoff_contract
            .required_archive_receipt_fields
            .iter()
            .any(|field| *field == "archive_reference")
    );
    assert!(
        preflight_contract
            .archive_handoff_contract
            .required_archive_correlation_dimensions
            .iter()
            .any(|dimension| *dimension == "section_signal")
    );
    assert!(
        preflight_contract
            .archive_handoff_contract
            .required_archive_correlation_dimensions
            .iter()
            .any(|dimension| *dimension == "bundled_public_client_artifact_reference")
    );
    assert!(
        preflight_contract
            .archive_handoff_contract
            .required_archive_correlation_dimensions
            .iter()
            .any(|dimension| *dimension == "bundled_official_entry_artifact_identity")
    );
    assert!(
        preflight_contract
            .archive_handoff_contract
            .required_archive_correlation_dimensions
            .iter()
            .any(|dimension| *dimension == "rollback_reference")
    );
    assert!(
        preflight_contract
            .archive_handoff_contract
            .required_archive_correlation_dimensions
            .iter()
            .any(|dimension| *dimension == "rollback_public_client_artifact_reference")
    );
    assert!(
        preflight_contract
            .archive_handoff_contract
            .required_archive_correlation_dimensions
            .iter()
            .any(|dimension| *dimension == "rollback_bundled_official_entry_artifact_identity")
    );
    assert!(
        preflight_contract
            .archive_handoff_contract
            .source_record_state_must_match_section_result_status
    );
    assert!(
        preflight_contract
            .archive_handoff_contract
            .terminal_section_not_complete_without_archive_receipt
    );
    assert_eq!(
        preflight_contract
            .retention_contract
            .authoritative_retention_owner,
        "external-release-tracker"
    );
    assert!(
        preflight_contract
            .retention_contract
            .post_archive_verification_required
    );
    assert!(
        preflight_contract
            .retention_contract
            .required_post_archive_checks
            .iter()
            .any(|check| check.contains("archive receipt fields"))
    );
    assert!(
        preflight_contract
            .retention_contract
            .required_post_archive_checks
            .iter()
            .any(|check| check.contains("post_archive_verified_by"))
    );
    assert!(
        preflight_contract
            .retention_contract
            .required_post_archive_checks
            .iter()
            .any(|check| check.contains("section_signal"))
    );
    assert!(
        preflight_contract
            .retention_contract
            .required_post_archive_checks
            .iter()
            .any(|check| check.contains("rollback_public_client_artifact_reference"))
    );
    assert!(
        preflight_contract
            .retention_contract
            .required_post_archive_checks
            .iter()
            .any(|check| check.contains("silently rewritten"))
    );
    assert!(
        preflight_contract
            .retention_contract
            .required_post_archive_writeback_fields
            .iter()
            .any(|field| *field == "post_archive_verification_result")
    );
    assert!(
        preflight_contract
            .retention_contract
            .post_archive_writeback_target
            .contains("same section")
    );
    assert!(
        preflight_contract
            .terminal_mutation_contract
            .immutable_after_states
            .iter()
            .any(|state| *state == "cutover-approved")
    );
    assert!(
        preflight_contract
            .terminal_mutation_contract
            .allowed_append_only_updates
            .iter()
            .any(|field| *field == "archive_reference")
    );
    assert!(
        preflight_contract
            .terminal_mutation_contract
            .allowed_append_only_updates
            .iter()
            .any(|field| *field == "post_archive_verification_reference")
    );
    assert!(
        preflight_contract
            .terminal_mutation_contract
            .forbidden_mutations
            .iter()
            .any(|rule| rule.contains("bundled_public_client_artifact_reference"))
    );
    assert!(
        preflight_contract
            .terminal_mutation_contract
            .forbidden_mutations
            .iter()
            .any(|rule| rule.contains("bundled_official_entry_artifact_identity"))
    );
    assert!(
        preflight_contract
            .terminal_mutation_contract
            .forbidden_mutations
            .iter()
            .any(|rule| rule.contains("post-archive verification evidence"))
    );
    assert_eq!(
        preflight_contract
            .execution_boundary_contract
            .authoritative_live_record_system,
        "external-release-tracker"
    );
    assert!(
        preflight_contract
            .execution_boundary_contract
            .minimum_terminal_snapshot_record_fields
            .iter()
            .any(|field| *field == "bundled_public_client_artifact_reference")
    );
    assert!(
        preflight_contract
            .execution_boundary_contract
            .minimum_terminal_snapshot_record_fields
            .iter()
            .any(|field| *field == "rollback_public_client_artifact_reference")
    );
    assert!(
        preflight_contract
            .execution_boundary_contract
            .minimum_terminal_snapshot_record_fields
            .iter()
            .any(|field| *field == "rollback_bundled_official_entry_artifact_identity")
    );
    assert!(
        preflight_contract
            .execution_boundary_contract
            .required_system_separation
            .iter()
            .any(|rule| rule.contains("independent"))
    );
    assert!(
        preflight_contract
            .execution_boundary_contract
            .forbidden_shortcuts
            .iter()
            .any(|rule| rule.contains("rollback Public client artifact reference"))
    );
    assert!(
        preflight_contract
            .execution_boundary_contract
            .forbidden_shortcuts
            .iter()
            .any(|rule| rule.contains("rollback bundled official_entry artifact identity"))
    );
    assert_eq!(
        preflight_contract.section_record_template.section_signal,
        "public-entry-handoff"
    );
    assert_eq!(
        preflight_contract
            .section_record_template
            .lifecycle_state_field,
        "result_status"
    );
    assert!(
        preflight_contract
            .section_record_template
            .required_fields
            .iter()
            .any(
                |field| field.name == "bundled_public_client_artifact_reference"
                    && field.placeholder == "<public-client-release-artifact-ref>"
            )
    );
    assert!(
        preflight_contract
            .section_record_template
            .required_fields
            .iter()
            .any(
                |field| field.name == "bundled_official_entry_server_address"
                    && field.placeholder == "<public-realm-host-or-socket>"
            )
    );
    assert!(
        preflight_contract
            .section_record_template
            .required_fields
            .iter()
            .any(
                |field| field.name == "rollback_public_client_artifact_reference"
                    && field.placeholder == "<rollback-public-client-release-artifact-ref>"
            )
    );
    assert!(
        preflight_contract
            .section_record_template
            .required_fields
            .iter()
            .any(
                |field| field.name == "rollback_bundled_official_entry_artifact_identity"
                    && field.placeholder == "<rollback-official-entry-content-sha256-v1:...>"
            )
    );
    assert!(
        preflight_contract
            .section_record_template
            .post_archive_follow_up_fields
            .iter()
            .any(|field| {
                field.name == "post_archive_verification_result"
                    && field.placeholder == "<verified|needs-follow-up>"
            })
    );
    assert!(
        preflight_contract
            .section_record_template
            .post_archive_follow_up_fields
            .iter()
            .any(|field| {
                field.name == "post_archive_verification_reference"
                    && field.completion_rule.contains("post-archive verification")
            })
    );
    assert!(preflight_contract.minimum_section_example.illustrative_only);
    assert!(
        preflight_contract
            .minimum_section_example
            .example_fields
            .iter()
            .any(
                |field| field.name == "bundled_public_client_artifact_reference"
                    && field.value == "artifact://public-client/release-2026-05-01-build-01"
            )
    );
    assert!(
        preflight_contract
            .minimum_section_example
            .example_fields
            .iter()
            .any(|field| field.name == "bundled_official_entry_auth_server"
                && field.value == "https://auth.realm.example")
    );
    assert!(
        preflight_contract
            .minimum_section_example
            .example_fields
            .iter()
            .any(
                |field| field.name == "rollback_public_client_artifact_reference"
                    && field.value == "artifact://public-client/release-2026-04-18-build-03"
            )
    );
    assert!(
        preflight_contract
            .minimum_section_example
            .example_fields
            .iter()
            .any(
                |field| field.name == "rollback_bundled_official_entry_artifact_identity"
                    && field.value == "official-entry-content-sha256-v1:previousbundlecafebabe"
            )
    );
    assert!(
        preflight_contract
            .minimum_section_example
            .notes
            .iter()
            .any(|note| note.contains("illustrative only"))
    );
    assert_eq!(preflight_contract.section_execution_workflow.len(), 6);
    assert!(
        preflight_contract
            .section_execution_workflow
            .iter()
            .any(|step| {
                step.sequence == 1
                    && step.action.contains("open or locate")
                    && step.record_effect.contains("result_status = draft")
                    && step
                        .completion_record_fields
                        .iter()
                        .any(|field| *field == "release_reference")
                    && step
                        .completion_record_fields
                        .iter()
                        .any(|field| *field == "decision_recorded_at_utc")
                    && step.blocking_until_complete
            })
    );
    assert!(
        preflight_contract
            .section_execution_workflow
            .iter()
            .any(|step| {
                step.sequence == 2
                    && step.action.contains("client artifact reference")
                    && step
                        .record_effect
                        .contains("shipped Public client artifact")
                    && step
                        .completion_record_fields
                        .iter()
                        .any(|field| *field == "bundled_public_client_artifact_reference")
                    && step
                        .completion_record_fields
                        .iter()
                        .any(|field| *field == "non_local_cutover_gap_reasons")
            })
    );
    assert!(
        preflight_contract
            .section_execution_workflow
            .iter()
            .any(|step| {
                step.sequence == 3
                    && step.evidence_source.contains("/health/compatibility")
                    && step.record_effect.contains("evidence-linked")
                    && step
                        .completion_record_fields
                        .iter()
                        .any(|field| *field == "target_runtime_environment")
                    && step
                        .completion_record_fields
                        .iter()
                        .any(|field| *field == "authoritative_handshake_auth_provider")
                    && step
                        .completion_record_fields
                        .iter()
                        .any(|field| *field == "recovery_drill_reference")
            })
    );
    assert!(
        preflight_contract
            .section_execution_workflow
            .iter()
            .any(|step| {
                step.sequence == 4
                    && step
                        .record_effect
                        .contains("rollback Public client artifact")
                    && step
                        .record_effect
                        .contains("artifact and entry material fixed")
                    && step
                        .completion_record_fields
                        .iter()
                        .any(|field| *field == "approval_decision")
                    && step
                        .completion_record_fields
                        .iter()
                        .any(|field| *field == "rollback_bundled_official_entry_artifact_identity")
            })
    );
    assert!(
        preflight_contract
            .section_execution_workflow
            .iter()
            .any(|step| {
                step.sequence == 5
                    && step.action.contains("archive")
                    && step.record_effect.contains("archive_reference")
                    && step
                        .record_effect
                        .contains("bundled_public_client_artifact_reference")
                    && step.record_effect.contains("section_signal")
                    && step
                        .record_effect
                        .contains("rollback_public_client_artifact_reference")
                    && step
                        .completion_record_fields
                        .iter()
                        .any(|field| *field == "archive_reference")
                    && step
                        .completion_record_fields
                        .iter()
                        .any(|field| *field == "source_record_state")
            })
    );
    assert!(
        preflight_contract
            .section_execution_workflow
            .iter()
            .any(|step| {
                step.sequence == 6
                    && step.action.contains("post-archive verification")
                    && !step.blocking_until_complete
                    && step
                        .record_effect
                        .contains("post_archive_verification_result")
                    && step
                        .completion_record_fields
                        .iter()
                        .any(|field| *field == "post_archive_verified_by")
                    && step
                        .completion_record_fields
                        .iter()
                        .any(|field| *field == "post_archive_verification_reference")
            })
    );
}

#[test]
fn section_instance_validation_contracts_expose_snapshot_input_contracts() {
    let public_entry = public_entry_section_instance_validation_contract(
        &public_entry_handoff_required_review_field_contracts(),
    );
    let governance = governance_section_instance_validation_contract(
        &governance_required_decision_field_contracts(),
    );
    let management_auth = management_auth_section_instance_validation_contract(
        &management_auth_required_decision_field_contracts(),
    );

    assert_eq!(
        public_entry
            .snapshot_input_contract
            .prior_result_statuses_key,
        "prior_result_statuses"
    );
    assert!(
        public_entry
            .snapshot_input_contract
            .stage_scoped_field_values
            .iter()
            .any(|field| {
                field.name == "rollback_public_client_artifact_reference"
                    && field.semantics.contains("rollback")
            })
    );
    assert!(
        public_entry
            .snapshot_input_contract
            .notes
            .iter()
            .any(|note| note.contains("field_values.result_status"))
    );
    assert!(
        public_entry
            .snapshot_template_contract
            .top_level_fields
            .iter()
            .any(|field| field.name == "prior_result_statuses")
    );
    assert!(
        public_entry
            .minimum_snapshot_example
            .top_level_fields
            .iter()
            .any(|field| field.name == "prior_result_statuses")
    );
    assert!(
        public_entry
            .validation_result_contract
            .optional_fields
            .iter()
            .any(|field| field.name == "failed_authority_pairing_check_ids")
    );
    assert!(
        public_entry
            .minimum_validation_result_example
            .fields
            .iter()
            .any(|field| field.name == "evaluated_result_status" && field.value == "rolled-back")
    );
    assert!(
        governance
            .snapshot_input_contract
            .stage_scoped_field_values
            .iter()
            .any(|field| field.name == "exception_reason")
    );
    assert!(
        governance
            .snapshot_template_contract
            .field_value_entries
            .iter()
            .any(|field| field.name == "exception_reason")
    );
    assert!(
        governance
            .snapshot_input_contract
            .notes
            .iter()
            .any(|note| note.contains("exception-accepted"))
    );
    assert!(
        governance
            .minimum_snapshot_example
            .field_value_entries
            .iter()
            .any(|field| field.name == "result_status" && field.value == "approved")
    );
    assert!(
        governance
            .validation_result_contract
            .required_fields
            .iter()
            .any(|field| field.name == "summary")
    );
    assert!(
        management_auth
            .snapshot_input_contract
            .stage_scoped_field_values
            .iter()
            .any(|field| field.name == "compensating_controls")
    );
    assert!(
        management_auth
            .snapshot_input_contract
            .optional_top_level_fields
            .iter()
            .any(|field| field.name == "prior_result_statuses")
    );
    assert!(
        management_auth
            .snapshot_template_contract
            .field_value_entries
            .iter()
            .any(|field| field.name == "compensating_controls")
    );
    assert!(
        management_auth
            .minimum_snapshot_example
            .field_value_entries
            .iter()
            .any(|field| field.name == "result_status" && field.value == "approved")
    );
    assert!(
        management_auth
            .minimum_validation_result_example
            .fields
            .iter()
            .any(|field| field.name == "stage_status" && field.value == "valid")
    );

    let root = unique_temp_dir();
    let preflight = test_health_state(&root).preflight_report();
    let _ = fs::remove_dir_all(root);

    for contract in &preflight.review_decision_contracts {
        let validation_contract = contract
            .section_instance_validation_contract
            .as_ref()
            .expect(
                "exported preflight review decision contracts should expose section validation",
            );
        let readiness_summary = contract
            .validator_integration_readiness_summary
            .as_ref()
            .expect(
                "exported preflight review decision contracts should expose validator readiness",
            );

        assert_eq!(readiness_summary.status, "validator-contract-ready");
        assert_eq!(
            readiness_summary.input_snapshot_kind,
            validation_contract.snapshot_input_contract.snapshot_kind
        );
        assert_eq!(
            readiness_summary.field_values_key,
            validation_contract.snapshot_input_contract.field_values_key
        );
        assert_eq!(
            readiness_summary.lifecycle_state_field,
            validation_contract.lifecycle_state_field
        );
        assert_eq!(
            readiness_summary.output_result_kind,
            validation_contract.validation_result_contract.result_kind
        );
        assert_eq!(
            readiness_summary.output_stage_status_field,
            validation_contract
                .validation_result_contract
                .stage_status_field
        );
        assert_eq!(
            readiness_summary.blocking_interpretation,
            validation_contract.blocking_interpretation
        );
    }
}

#[test]
fn public_entry_lifecycle_transition_contract_disallows_orphan_exception_path() {
    let contract = public_entry_lifecycle_transition_contract();

    assert!(
        contract
            .unsupported_paths
            .iter()
            .any(|path| path.contains("exception-accepted"))
    );
    assert!(
        !contract
            .transitions
            .iter()
            .any(|transition| transition.from_state == "exception-accepted"
                || transition.to_state == "exception-accepted")
    );
    assert!(contract.transitions.iter().any(|transition| {
        transition.from_state == "cutover-approved"
            && transition.to_state == "rolled-back"
            && transition.approval_decision == Some("approved")
    }));
}

#[test]
fn repo_bundled_snapshot_review_items_warn_when_local_baseline_is_still_transitional() {
    let items = repo_bundled_official_entry_snapshot_review_items(
        &RepoBundledOfficialEntrySnapshotReport {
            status: "repo-bundled-entry-transitional",
            evidence_scope: "repo/local bundled official_entry baseline only",
            load_source: "voxygen.official_entry asset via common asset loader",
            authoritative_for_release_cutover: false,
            required_external_match_fields: vec![
                "bundled_official_entry_artifact_identity",
                "bundled_target_kind",
            ],
            baseline: Some(common::official_entry::BundledOfficialEntryPosture {
                source_kind: common::official_entry::OfficialEntrySourceKind::Bundled,
                display_name: "Official Realm".to_owned(),
                server_address_configured: true,
                server_address: "192.168.1.8:14004".to_owned(),
                artifact_identity: "official-entry-content-sha256-v1:test".to_owned(),
                target_kind:
                    common::official_entry::BundledOfficialTargetKind::PrivateOrUniqueLocalIp,
                target_is_non_local_candidate: false,
                transport_kind: common::official_entry::BundledOfficialTransportKind::DirectTcp,
                use_srv: false,
                use_quic: false,
                validate_tls: true,
                auth_mode: common::official_entry::BundledOfficialAuthMode::NoExternalAuth,
                auth_server: None,
                non_local_cutover_ready: false,
                non_local_cutover_gap_reasons: vec![
                    "bundled_public_target_is_private_or_unique_local_ip",
                    "bundled_public_auth_pin_missing",
                ],
                rollout_readiness_scope: "test-scope",
            }),
            load_error: None,
            semantics: "test snapshot",
        },
    );

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, "repo-bundled-entry-transitional-baseline");
    assert!(!items[0].blocking);
    assert!(
        items[0]
            .detail
            .contains("target_kind=private-or-unique-local-ip")
    );
    assert!(items[0].detail.contains("bundled_public_auth_pin_missing"));
    assert!(items[0].detail.contains("comparison baseline"));
}

#[test]
fn repo_bundled_snapshot_review_items_warn_when_local_baseline_is_unavailable() {
    let items = repo_bundled_official_entry_snapshot_review_items(
        &RepoBundledOfficialEntrySnapshotReport {
            status: "repo-bundled-entry-unavailable",
            evidence_scope: "repo/local bundled official_entry baseline only",
            load_source: "voxygen.official_entry asset via common asset loader",
            authoritative_for_release_cutover: false,
            required_external_match_fields: vec![
                "bundled_official_entry_artifact_identity",
                "bundled_target_kind",
            ],
            baseline: None,
            load_error: Some("asset not found".to_owned()),
            semantics: "test snapshot",
        },
    );

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, "repo-bundled-entry-snapshot-unavailable");
    assert!(!items[0].blocking);
    assert!(items[0].detail.contains("asset not found"));
    assert!(
        items[0]
            .detail
            .contains("full shipped Public client artifact comparison")
    );
}

#[test]
fn public_entry_cutover_material_checklist_marks_ready_baseline_as_capture_required() {
    let checklist = public_entry_cutover_material_checklist(
        "production",
        common_net::msg::ServerAuthMode::ExternalProvider,
        Some("https://auth.example.test"),
        &test_repo_bundled_snapshot_report(Some(
            common::official_entry::BundledOfficialEntryPosture {
                source_kind: common::official_entry::OfficialEntrySourceKind::Bundled,
                display_name: "Caldrayne Online".to_owned(),
                server_address_configured: true,
                server_address: "prod.realm.example:14004".to_owned(),
                artifact_identity: "official-entry-content-sha256-v1:readybundle".to_owned(),
                target_kind: common::official_entry::BundledOfficialTargetKind::NamedHostCandidate,
                target_is_non_local_candidate: true,
                transport_kind: common::official_entry::BundledOfficialTransportKind::DirectQuic,
                use_srv: false,
                use_quic: true,
                validate_tls: true,
                auth_mode: common::official_entry::BundledOfficialAuthMode::ExternalProvider,
                auth_server: Some("https://auth.example.test".to_owned()),
                non_local_cutover_ready: true,
                non_local_cutover_gap_reasons: Vec::new(),
                rollout_readiness_scope: "test-ready-scope",
            },
        )),
    );

    assert!(checklist.iter().any(|item| {
        item.id == "external-auth-authority-pinned"
            && item.current_stage_status == CUTOVER_MATERIAL_STATUS_OPERATOR_CAPTURE_REQUIRED
            && item
                .current_stage_detail
                .contains("already matches the authoritative handshake auth_provider")
    }));
    assert!(checklist.iter().any(|item| {
        item.id == "non-local-target-material-ready"
            && item.current_stage_status == CUTOVER_MATERIAL_STATUS_OPERATOR_CAPTURE_REQUIRED
            && item
                .current_stage_detail
                .contains("no remaining non-local cutover gaps")
    }));
}

#[test]
fn public_entry_cutover_material_checklist_falls_back_to_external_material_when_snapshot_is_unavailable()
 {
    let mut unavailable_snapshot = test_repo_bundled_snapshot_report(None);
    unavailable_snapshot.load_error = Some("asset missing".to_owned());

    let checklist = public_entry_cutover_material_checklist(
        "production",
        common_net::msg::ServerAuthMode::ExternalProvider,
        Some("https://auth.example.test"),
        &unavailable_snapshot,
    );

    assert!(checklist.iter().any(|item| {
        item.id == "external-auth-authority-pinned"
            && item.current_stage_status == CUTOVER_MATERIAL_STATUS_EXTERNAL_MATERIAL_REQUIRED
            && item
                .current_stage_detail
                .contains("status=repo-bundled-entry-unavailable")
    }));
    assert!(checklist.iter().any(|item| {
        item.id == "non-local-target-material-ready"
            && item.current_stage_status == CUTOVER_MATERIAL_STATUS_EXTERNAL_MATERIAL_REQUIRED
            && item
                .operator_next_step
                .contains("target posture and gap reasons")
    }));
}

#[test]
fn compatibility_contract_report_detects_query_hint_drift() {
    let health = test_health_state(Path::new("test-root"));
    let report = health.compatibility_contract_report_with_query_hint(
        veloren_query_server::proto::ServerInfo {
            realm_id: veloren_query_server::proto::ServerRealmId::from_u128(0),
            environment: veloren_query_server::proto::ServerEnvironment::Local,
            compatibility: veloren_query_server::proto::ServerCompatibility {
                generation: 99,
                minimum_supported_generation: 98,
            },
            auth_required: false,
            git_hash: 0,
            git_timestamp: 0,
            players_count: 0,
            player_cap: 0,
            battlemode: veloren_query_server::proto::ServerBattleMode::GlobalPvP,
        },
    );

    assert_eq!(report.status, "compatibility-contract-drift");
    assert!(!report.environment_matches);
    assert!(!report.compatibility_matches);
    assert!(!report.auth_requirement_matches_runtime_config);
    assert!(report.mismatch_effect.contains("rollout-blocking"));
}

#[test]
fn preflight_report_can_require_management_auth_review_without_governance_findings() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    seed_live_runtime_state(&state);
    seed_recovery_staging_restore_state(&recovery_staging_state);

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: vec![crate::settings::ManagementAuthInventoryEntry {
            surface: "metrics",
            bind_address: Some("0.0.0.0:14005".parse().unwrap()),
            reachability: crate::settings::SurfaceReachability::NetworkAccessible,
            review_status: crate::settings::SurfaceReviewStatus::InternalObservabilityOnly,
            remote_exposure_policy:
                crate::settings::SurfaceRemoteExposurePolicy::RemoteRequiresExplicitWebOptIn,
            capability: crate::settings::ManagementSurfaceCapability::ObservabilityScrape,
            auth_scheme: crate::settings::SurfaceAuth::None,
            credential_bootstrap: crate::settings::SurfaceCredentialBootstrap::None,
            credential_transport: crate::settings::ManagementCredentialTransport::None,
            secret_config_id: None,
            proxy_forwarding_forbidden: false,
            detail: "metrics has no in-process auth".to_owned(),
        }],
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .preflight_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "operator_review_required");
    assert!(!report.release_blocked);
    assert!(report.requires_operator_review);
    assert!(report.blocking_signals.is_empty());
    assert_eq!(report.review_signals, vec![
        "public-entry-handoff",
        "management-auth"
    ]);
    assert!(report.follow_up_endpoints.iter().any(|follow_up| {
        follow_up.signal == "management-auth"
            && follow_up.endpoint == "/health/management-auth"
            && !follow_up.blocking
            && follow_up.owner == "release-operator"
    }));
    assert!(report.follow_up_endpoints.iter().any(|follow_up| {
        follow_up.signal == "public-entry-handoff"
            && follow_up.endpoint == "/health/compatibility"
            && !follow_up.blocking
            && follow_up.owner == "release-operator"
            && follow_up.reason.contains("cutover / rollback")
    }));
    assert!(report.review_decision_contracts.iter().any(|contract| {
        contract.signal == "management-auth"
            && contract.review_owner == "release-operator"
            && contract.external_record_owner == "release-operator"
            && contract.external_record_authority == "external-release-tracker"
            && contract.decision_reference_kind == "release-review-record"
            && contract.exception_reference_kind == "management-auth-exception-record"
            && contract.local_contract_role == "minimum-schema-only"
            && contract.supporting_endpoints.is_empty()
            && contract
                .required_decision_fields
                .iter()
                .any(|field| *field == "result_status")
            && contract
                .required_decision_fields
                .iter()
                .any(|field| *field == "approval_decision")
            && contract
                .required_decision_fields
                .iter()
                .any(|field| *field == "affected_surfaces")
            && contract
                .required_decision_field_contracts
                .iter()
                .any(|field| {
                    field.name == "release_reference"
                        && field.evidence_source == "external-release-tracker"
                })
            && contract
                .required_decision_field_contracts
                .iter()
                .any(|field| {
                    field.name == "affected_surfaces"
                        && field.evidence_source == "/health/management-auth"
                })
            && contract
                .record_lifecycle_contract
                .canonical_record_location
                .contains("release_reference")
            && contract
                .record_lifecycle_contract
                .same_record_must_link_rollout_and_rollback
            && contract.result_status_model.iter().any(|state| {
                state.state == "exception-accepted"
                    && state.semantics.contains("compensating controls")
            })
            && contract
                .exception_record_fields
                .iter()
                .any(|field| *field == "compensating_controls")
            && !contract
                .exception_record_fields
                .iter()
                .any(|field| *field == "affected_surfaces")
            && contract
                .exception_record_field_contracts
                .iter()
                .any(|field| {
                    field.name == "compensating_controls"
                        && field.evidence_source == "external-release-tracker"
                })
            && contract
                .archive_handoff_contract
                .terminal_states_requiring_archive_receipt
                .iter()
                .any(|state| *state == "approved")
            && contract
                .archive_handoff_contract
                .required_archive_receipt_fields
                .iter()
                .any(|field| *field == "archive_reference")
            && contract
                .archive_handoff_contract
                .required_archive_correlation_dimensions
                .iter()
                .any(|dimension| *dimension == "section_signal")
            && contract
                .archive_handoff_contract
                .source_record_state_must_match_section_result_status
            && contract
                .retention_contract
                .post_archive_verification_required
            && contract
                .retention_contract
                .required_post_archive_checks
                .iter()
                .any(|check| check.contains("release_reference"))
            && contract
                .retention_contract
                .required_post_archive_writeback_fields
                .iter()
                .any(|field| *field == "post_archive_verified_at_utc")
            && contract
                .retention_contract
                .post_archive_writeback_target
                .contains("same section")
            && contract
                .terminal_mutation_contract
                .allowed_append_only_updates
                .iter()
                .any(|field| *field == "archive_reference")
            && contract
                .terminal_mutation_contract
                .allowed_append_only_updates
                .iter()
                .any(|field| *field == "post_archive_verification_result")
            && contract
                .terminal_mutation_contract
                .forbidden_mutations
                .iter()
                .any(|rule| rule.contains("silent"))
            && contract
                .section_record_template
                .post_archive_follow_up_fields
                .iter()
                .any(|field| field.name == "post_archive_verification_reference")
            && contract.section_execution_workflow.iter().any(|step| {
                step.sequence == 2
                    && step.action.contains("reviewed surfaces")
                    && step
                        .completion_record_fields
                        .iter()
                        .any(|field| *field == "affected_surfaces")
                    && !step
                        .completion_record_fields
                        .iter()
                        .any(|field| *field == "compensating_controls")
            })
            && contract.section_execution_workflow.iter().any(|step| {
                step.sequence == 3
                    && step.action.contains("record approval_decision")
                    && step.record_effect.contains("terminal result_status")
                    && step.record_effect.contains("compensating_controls")
                    && step
                        .completion_record_fields
                        .iter()
                        .any(|field| *field == "approval_decision")
            })
            && contract.section_execution_workflow.iter().any(|step| {
                step.sequence == 5
                    && step.action.contains("post-archive verification")
                    && !step.blocking_until_complete
                    && step
                        .completion_record_fields
                        .iter()
                        .any(|field| *field == "post_archive_verification_reference")
            })
            && contract
                .section_record_template
                .required_fields
                .iter()
                .any(|field| field.name == "result_status")
            && contract
                .section_record_template
                .required_fields
                .iter()
                .any(|field| field.name == "affected_surfaces")
            && contract
                .section_instance_validation_contract
                .as_ref()
                .is_some_and(|validation| {
                    validation
                        .evidence_linked_requirements
                        .required_fields
                        .iter()
                        .any(|field| *field == "affected_surfaces")
                        && validation
                            .terminal_requirements
                            .conditional_required_fields
                            .iter()
                            .any(|requirement| {
                                requirement.when_result_statuses == vec!["exception-accepted"]
                                    && requirement
                                        .required_fields
                                        .iter()
                                        .any(|field| *field == "compensating_controls")
                                    && requirement
                                        .required_fields
                                        .iter()
                                        .any(|field| *field == "rollback_reference")
                            })
                })
            && contract.minimum_section_example.illustrative_only
    }));
    assert!(
        report.operator_review_items.iter().any(|item| {
            item.kind == "management-auth-review" && item.detail.contains("metrics")
        })
    );
    assert!(report.review_decision_contracts.iter().any(|contract| {
        contract.signal == "public-entry-handoff"
            && contract.review_owner == "release-operator"
            && contract.external_record_authority == "external-release-tracker"
            && contract
                .required_decision_fields
                .iter()
                .any(|field| *field == "result_status")
            && contract
                .required_decision_fields
                .iter()
                .any(|field| *field == "bundled_public_client_artifact_reference")
            && contract
                .required_decision_fields
                .iter()
                .any(|field| *field == "bundled_official_entry_artifact_identity")
            && contract
                .required_decision_fields
                .iter()
                .any(|field| *field == "bundled_official_entry_use_quic")
            && contract
                .required_decision_fields
                .iter()
                .any(|field| *field == "bundled_target_kind")
            && contract
                .required_decision_fields
                .iter()
                .any(|field| *field == "non_local_cutover_ready")
            && contract
                .required_decision_fields
                .iter()
                .any(|field| *field == "rollback_public_client_artifact_reference")
            && contract
                .required_decision_fields
                .iter()
                .any(|field| *field == "recovery_drill_reference")
            && contract
                .required_decision_fields
                .iter()
                .any(|field| *field == "rollback_reference")
            && contract
                .required_decision_fields
                .iter()
                .any(|field| *field == "expected_handshake_auth_mode")
            && contract
                .required_decision_fields
                .iter()
                .any(|field| *field == "authoritative_handshake_auth_provider")
            && contract
                .required_decision_fields
                .iter()
                .any(|field| *field == "query_auth_required_hint")
            && contract
                .required_decision_field_contracts
                .iter()
                .any(|field| {
                    field.name == "bundled_public_client_artifact_reference"
                        && field.value_kind == "release-artifact-reference"
                        && field.evidence_source.contains("external-release-tracker")
                })
            && contract
                .required_decision_field_contracts
                .iter()
                .any(|field| {
                    field.name == "bundled_official_entry_validate_tls"
                        && field.value_kind == "boolean"
                        && field.evidence_source == "bundled-client-artifact-review"
                })
            && contract
                .required_decision_field_contracts
                .iter()
                .any(|field| {
                    field.name == "authoritative_handshake_auth_provider"
                        && field.value_kind == "url-or-null"
                        && field.evidence_source == "/health/compatibility"
                })
            && contract
                .required_decision_field_contracts
                .iter()
                .any(|field| {
                    field.name == "query_auth_required_hint"
                        && field.evidence_source == "/health/compatibility"
                })
            && contract
                .record_lifecycle_contract
                .same_record_must_link_rollout_and_rollback
            && contract.result_status_model.iter().any(|state| {
                state.state == "cutover-rejected"
                    && state.semantics.contains("rejected or deferred")
            })
            && contract
                .public_entry_lifecycle_transition_contract
                .as_ref()
                .is_some_and(|lifecycle| {
                    lifecycle.initial_state == "draft"
                        && lifecycle.evidence_ready_state == "evidence-linked"
                        && lifecycle
                            .terminal_states_requiring_archive_receipt
                            .iter()
                            .any(|state| *state == "rolled-back")
                        && lifecycle
                            .unsupported_paths
                            .iter()
                            .any(|path| path.contains("exception-accepted"))
                        && lifecycle.transitions.iter().any(|transition| {
                            transition.from_state == "evidence-linked"
                                && transition.to_state == "cutover-approved"
                                && transition.approval_decision == Some("approved")
                                && transition.archive_required
                        })
                        && lifecycle.transitions.iter().any(|transition| {
                            transition.from_state == "cutover-approved"
                                && transition.to_state == "rolled-back"
                                && transition.approval_decision == Some("approved")
                        })
                })
            && contract
                .exception_record_fields
                .iter()
                .any(|field| *field == "bundled_public_client_artifact_reference")
            && contract
                .exception_record_fields
                .iter()
                .any(|field| *field == "bundled_official_entry_artifact_identity")
            && contract
                .exception_record_fields
                .iter()
                .any(|field| *field == "rollback_public_client_artifact_reference")
            && contract
                .exception_record_fields
                .iter()
                .any(|field| *field == "recovery_drill_reference")
            && contract
                .accepted_exception_follow_up
                .iter()
                .any(|item| item.contains("does not support exception-accepted"))
            && contract
                .exception_record_field_contracts
                .iter()
                .any(|field| {
                    field.name == "bundled_auth_pin_review_reference"
                        && field.evidence_source == "external-release-tracker"
                })
            && contract
                .archive_handoff_contract
                .terminal_states_requiring_archive_receipt
                .iter()
                .any(|state| *state == "rolled-back")
            && contract
                .archive_handoff_contract
                .required_archive_correlation_dimensions
                .iter()
                .any(|dimension| *dimension == "bundled_public_client_artifact_reference")
            && contract
                .archive_handoff_contract
                .required_archive_correlation_dimensions
                .iter()
                .any(|dimension| *dimension == "bundled_official_entry_artifact_identity")
            && contract
                .archive_handoff_contract
                .required_archive_correlation_dimensions
                .iter()
                .any(|dimension| *dimension == "rollback_public_client_artifact_reference")
            && contract
                .archive_handoff_contract
                .required_archive_correlation_dimensions
                .iter()
                .any(|dimension| *dimension == "rollback_bundled_official_entry_artifact_identity")
            && contract
                .retention_contract
                .retention_policy
                .contains("Public cutover review")
            && contract
                .retention_contract
                .required_post_archive_writeback_fields
                .iter()
                .any(|field| *field == "post_archive_verification_reference")
            && contract
                .terminal_mutation_contract
                .forbidden_mutations
                .iter()
                .any(|rule| rule.contains("bundled target posture"))
            && contract
                .terminal_mutation_contract
                .allowed_append_only_updates
                .iter()
                .any(|field| *field == "post_archive_verified_by")
            && contract
                .section_record_template
                .required_fields
                .iter()
                .any(|field| {
                    field.name == "approval_decision" && field.placeholder == "<approved|rejected>"
                })
            && contract
                .section_record_template
                .required_fields
                .iter()
                .any(|field| {
                    field.name == "bundled_public_client_artifact_reference"
                        && field.placeholder == "<public-client-release-artifact-ref>"
                })
            && contract
                .section_record_template
                .required_fields
                .iter()
                .any(|field| {
                    field.name == "rollback_public_client_artifact_reference"
                        && field.placeholder == "<rollback-public-client-release-artifact-ref>"
                })
            && contract
                .section_record_template
                .required_fields
                .iter()
                .any(|field| {
                    field.name == "bundled_official_entry_auth_server"
                        && field.placeholder == "<https://auth.realm.example-or-null>"
                })
            && contract
                .section_record_template
                .post_archive_follow_up_fields
                .iter()
                .any(|field| {
                    field.name == "post_archive_verification_result"
                        && field.placeholder == "<verified|needs-follow-up>"
                })
            && contract
                .minimum_section_example
                .example_fields
                .iter()
                .any(|field| {
                    field.name == "bundled_public_client_artifact_reference"
                        && field.value == "artifact://public-client/release-2026-05-01-build-01"
                })
            && contract
                .minimum_section_example
                .example_fields
                .iter()
                .any(|field| {
                    field.name == "bundled_official_entry_server_address"
                        && field.value == "prod.realm.example:14004"
                })
            && contract
                .minimum_section_example
                .example_fields
                .iter()
                .any(|field| {
                    field.name == "rollback_public_client_artifact_reference"
                        && field.value == "artifact://public-client/release-2026-04-18-build-03"
                })
            && contract.section_execution_workflow.iter().any(|step| {
                step.sequence == 2
                    && step.action.contains("client artifact reference")
                    && step.evidence_source.contains("external-release-tracker")
            })
            && contract.section_execution_workflow.iter().any(|step| {
                step.sequence == 3
                    && step
                        .completion_record_fields
                        .iter()
                        .any(|field| *field == "expected_handshake_auth_mode")
                    && step
                        .completion_record_fields
                        .iter()
                        .any(|field| *field == "authoritative_handshake_auth_provider")
                    && step
                        .completion_record_fields
                        .iter()
                        .any(|field| *field == "query_auth_required_hint")
            })
            && contract.section_execution_workflow.iter().any(|step| {
                step.sequence == 5
                    && step.action.contains("archive")
                    && step.record_effect.contains("archive_reference")
                    && step
                        .record_effect
                        .contains("bundled_public_client_artifact_reference")
                    && step
                        .record_effect
                        .contains("bundled_official_entry_artifact_identity")
                    && step
                        .record_effect
                        .contains("rollback_public_client_artifact_reference")
            })
            && contract.section_execution_workflow.iter().any(|step| {
                step.sequence == 6
                    && step.action.contains("post-archive verification")
                    && step
                        .record_effect
                        .contains("post_archive_verification_reference")
            })
            && contract.supporting_endpoints.iter().any(|endpoint| {
                endpoint.signal == "compatibility-contract"
                    && endpoint.endpoint == "/health/compatibility"
            })
            && contract.supporting_endpoints.iter().any(|endpoint| {
                endpoint.signal == "readiness" && endpoint.endpoint == "/health/ready"
            })
            && contract.supporting_endpoints.iter().any(|endpoint| {
                endpoint.signal == "backup-preflight" && endpoint.endpoint == "/health/backup"
            })
            && contract.supporting_endpoints.iter().any(|endpoint| {
                endpoint.signal == "recovery-drill" && endpoint.endpoint == "/health/recovery/drill"
            })
    }));
    assert!(report.operator_review_items.iter().any(|item| {
        item.kind == "public-entry-handoff-review"
            && item.detail.contains("client artifact reference")
            && item.detail.contains("artifact identity")
            && item.detail.contains("transport flags")
            && item.detail.contains("target posture/gap reasons")
            && item.detail.contains("rollback reference")
            && item.detail.contains("post_archive verification fields")
    }));
}

#[test]
fn transport_security_report_exposes_quic_tls_inventory() {
    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: server::ServerStatePaths::new(Path::new("test-root").join("live")),
        recovery_staging_state: server::ServerStatePaths::new(
            Path::new("test-root").join("recovery-staging"),
        ),
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: vec![crate::settings::ManagementAuthInventoryEntry {
            surface: "ui-api",
            bind_address: Some("127.0.0.1:14005".parse().unwrap()),
            reachability: crate::settings::SurfaceReachability::LoopbackOnly,
            review_status: crate::settings::SurfaceReviewStatus::PrototypeControlPlaneUnaudited,
            remote_exposure_policy:
                crate::settings::SurfaceRemoteExposurePolicy::LoopbackRuntimeEnforced,
            capability: crate::settings::ManagementSurfaceCapability::MutatingControl,
            auth_scheme: crate::settings::SurfaceAuth::LoopbackUiSession,
            credential_bootstrap: crate::settings::SurfaceCredentialBootstrap::LoopbackUiBootstrap,
            credential_transport: crate::settings::ManagementCredentialTransport::CookieSecret,
            secret_config_id: Some("ui-api-secret"),
            proxy_forwarding_forbidden: false,
            detail: "loopback ui bootstrap hands out the ui-api cookie".to_owned(),
        }],
        transport_security_inventory: vec![crate::settings::TransportSecurityInventoryEntry {
            surface: "game-quic",
            bind_address: "0.0.0.0:14004".parse().unwrap(),
            transport: "quic",
            encryption: "tls-required",
            cert_file_path: Path::new("tls").join("cert.pem"),
            key_file_path: Path::new("tls").join("key.pem"),
            rollout_policy:
                crate::settings::TransportSecurityRolloutPolicy::ExperimentalOptInActive,
            validation_policy:
                crate::settings::TransportSecurityValidationPolicy::FailFastAtStartup,
            material_state: crate::settings::TransportSecurityMaterialState::Invalid,
            detail: "No valid TLS certificate chain found in tls/cert.pem".to_owned(),
        }],
        governance_findings: Vec::new(),
    }
    .transport_security_report();

    assert_eq!(report.status, "transport-security-inventory");
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].surface, "game-quic");
    assert_eq!(report.entries[0].bind_address, "0.0.0.0:14004");
    assert_eq!(
        report.entries[0].rollout_policy,
        "experimental-opt-in-active"
    );
    assert_eq!(report.entries[0].validation_policy, "fail-fast-at-startup");
    assert_eq!(report.entries[0].material_state, "invalid");
    assert!(report.entries[0].detail.contains("certificate chain"));
}

#[test]
fn runtime_listener_report_exposes_startup_truth_for_configured_surfaces() {
    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: server::ServerStatePaths::new(Path::new("test-root").join("live")),
        recovery_staging_state: server::ServerStatePaths::new(
            Path::new("test-root").join("recovery-staging"),
        ),
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(vec![
            server::RuntimeListenerStatus {
                surface: server::RuntimeListenerSurface::GameQuic,
                bind_address: "0.0.0.0:14004".parse().unwrap(),
                state: server::RuntimeListenerState::Listening,
                detail: "listener accepted the declared QUIC gameplay bind address".to_owned(),
            },
            server::RuntimeListenerStatus {
                surface: server::RuntimeListenerSurface::QueryServer,
                bind_address: "0.0.0.0:14006".parse().unwrap(),
                state: server::RuntimeListenerState::StartupFailed,
                detail: "failed to bind query server listener on 0.0.0.0:14006: address in use"
                    .to_owned(),
            },
        ])),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .runtime_listener_report();

    assert_eq!(report.status, "runtime-listener-inventory");
    assert_eq!(report.entries.len(), 2);
    assert!(report.entries.iter().any(|entry| {
        entry.surface == "game-quic"
            && entry.bind_address == "0.0.0.0:14004"
            && entry.state == "listening"
    }));
    assert!(report.entries.iter().any(|entry| {
        entry.surface == "query-server"
            && entry.bind_address == "0.0.0.0:14006"
            && entry.state == "startup-failed"
            && entry.detail.contains("address in use")
    }));
}

#[test]
fn runtime_observability_report_exposes_metrics_export_failures() {
    let inventory = test_runtime_observability_inventory();
    {
        let mut entries = inventory.lock().expect("inventory lock should succeed");
        entries[0].state = RuntimeObservabilityState::Failing;
        entries[0].detail = "failed to encode metrics HTTP response: broken pipe".to_owned();
    }

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: server::ServerStatePaths::new(Path::new("test-root").join("live")),
        recovery_staging_state: server::ServerStatePaths::new(
            Path::new("test-root").join("recovery-staging"),
        ),
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: inventory,
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .runtime_observability_report();

    assert_eq!(report.status, "operator_review_required");
    assert!(report.requires_operator_review);
    assert!(report.entries.iter().any(|entry| {
        entry.surface == "metrics-export"
            && entry.state == "failing"
            && entry.detail.contains("broken pipe")
    }));
}

#[test]
fn preflight_report_blocks_when_required_runtime_checks_fail() {
    let report = test_health_state(Path::new("test-root")).preflight_report();

    assert_eq!(report.status, "preflight_blocked");
    assert!(report.release_blocked);
    assert!(report.requires_operator_review);
    assert_eq!(report.blocking_signals, vec![
        "readiness",
        "backup-preflight",
        "recovery-drill"
    ]);
    assert_eq!(report.review_signals, vec!["public-entry-handoff"]);
    assert!(
        report
            .operator_review_items
            .iter()
            .any(|item| { item.kind == "resolve-blocking-runtime-checks" && item.blocking })
    );
    assert!(report.components.iter().any(|component| {
        component.signal == "readiness" && component.blocking && component.status == "not_ready"
    }));
    assert!(report.components.iter().any(|component| {
        component.signal == "backup-preflight"
            && component.blocking
            && component.status == "backup_blocked"
    }));
    assert!(report.components.iter().any(|component| {
        component.signal == "recovery-drill"
            && component.blocking
            && component.status == "drill_blocked"
    }));
    assert!(report.components.iter().any(|component| {
        component.signal == "runtime-listeners"
            && !component.blocking
            && component.status == "runtime-listeners-ready"
    }));
    assert!(report.components.iter().any(|component| {
        component.signal == "compatibility-contract"
            && !component.blocking
            && component.status == "compatibility-contract-aligned"
    }));
    assert!(report.components.iter().any(|component| {
        component.signal == "public-entry-handoff"
            && !component.blocking
            && component.requires_operator_review
            && component.status == "external-review-required"
    }));
    assert!(report.components.iter().any(|component| {
        component.signal == "governance-audit"
            && !component.blocking
            && !component.requires_operator_review
    }));
    assert!(report.follow_up_endpoints.iter().any(|follow_up| {
        follow_up.signal == "readiness"
            && follow_up.endpoint == "/health/ready"
            && follow_up.blocking
            && follow_up.owner == "service-operator"
    }));
    assert!(report.follow_up_endpoints.iter().any(|follow_up| {
        follow_up.signal == "backup-preflight"
            && follow_up.endpoint == "/health/backup"
            && follow_up.blocking
    }));
    assert!(report.follow_up_endpoints.iter().any(|follow_up| {
        follow_up.signal == "recovery-drill"
            && follow_up.endpoint == "/health/recovery/drill"
            && follow_up.blocking
    }));
    assert!(report.follow_up_endpoints.iter().any(|follow_up| {
        follow_up.signal == "public-entry-handoff"
            && follow_up.endpoint == "/health/compatibility"
            && !follow_up.blocking
    }));
    assert!(
        report
            .required_signoff_fields
            .iter()
            .any(|field| *field == "approval_decision")
    );
}

#[test]
fn preflight_report_blocks_on_compatibility_contract_drift() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    seed_live_runtime_state(&state);
    seed_recovery_staging_restore_state(&recovery_staging_state);

    let health = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    };
    let compatibility_contract = health.compatibility_contract_report_with_query_hint(
        veloren_query_server::proto::ServerInfo {
            realm_id: veloren_query_server::proto::ServerRealmId::from_u128(0),
            environment: veloren_query_server::proto::ServerEnvironment::Local,
            compatibility: veloren_query_server::proto::ServerCompatibility {
                generation: 0,
                minimum_supported_generation: 0,
            },
            auth_required: false,
            git_hash: 0,
            git_timestamp: 0,
            players_count: 0,
            player_cap: 0,
            battlemode: veloren_query_server::proto::ServerBattleMode::GlobalPvP,
        },
    );
    let report = health.preflight_report_with_compatibility_contract(compatibility_contract);

    let _ = fs::remove_dir_all(root);

    assert!(report.release_blocked);
    assert!(report.blocking_signals.contains(&"compatibility-contract"));
    assert!(report.components.iter().any(|component| {
        component.signal == "compatibility-contract"
            && component.blocking
            && component.status == "compatibility-contract-drift"
    }));
    assert!(report.follow_up_endpoints.iter().any(|follow_up| {
        follow_up.signal == "compatibility-contract"
            && follow_up.endpoint == "/health/compatibility"
            && follow_up.blocking
    }));
    assert!(report.operator_review_items.iter().any(|item| {
        item.kind == "compatibility-contract-drift"
            && item.blocking
            && item.detail.contains("exact-match")
    }));
}

#[test]
fn preflight_report_can_require_operator_review_without_blocking_runtime_checks() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    seed_live_runtime_state(&state);
    seed_recovery_staging_restore_state(&recovery_staging_state);

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: vec![crate::settings::RuntimeGovernanceFinding {
            id: "remote-unaudited-web-opt-in-active",
            severity: crate::settings::RuntimeGovernanceSeverity::Warning,
            subject: "web-stack",
            detail: "remote web opt-in accepted for review".to_owned(),
        }],
    }
    .preflight_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "operator_review_required");
    assert!(!report.release_blocked);
    assert!(report.requires_operator_review);
    assert!(report.blocking_signals.is_empty());
    assert_eq!(
        report.development_stage_closure_status,
        "development-contract-closure-available"
    );
    assert_eq!(
        report.real_cutover_execution_status,
        "awaiting-external-materials-and-execution"
    );
    assert!(
        report
            .remaining_external_execution_dependencies
            .iter()
            .any(|dependency| {
                dependency.id == "external-release-review-and-archive-execution"
                    && dependency.current_stage_status == "external-execution-required"
            })
    );
    assert_eq!(report.review_signals, vec![
        "public-entry-handoff",
        "governance-audit"
    ]);
    assert!(report.review_decision_contracts.iter().any(|contract| {
        contract.signal == "governance-audit"
            && contract.external_record_authority == "external-release-tracker"
            && contract.decision_reference_kind == "release-review-record"
            && contract.exception_reference_kind == "governance-exception-record"
            && contract
                .retention_contract
                .retention_policy
                .contains("governance review decisions")
            && contract.section_execution_workflow.iter().any(|step| {
                step.sequence == 2
                    && step.action.contains("link governance findings")
                    && step.evidence_source.contains("/health/governance")
                    && step
                        .completion_record_fields
                        .iter()
                        .any(|field| *field == "governance_note_reference")
                    && !step
                        .completion_record_fields
                        .iter()
                        .any(|field| *field == "exception_reason")
            })
            && contract.supporting_endpoints.iter().any(|endpoint| {
                endpoint.signal == "management-auth"
                    && endpoint.endpoint == "/health/management-auth"
                    && endpoint
                        .related_findings
                        .iter()
                        .any(|id| *id == "remote-unaudited-web-opt-in-active")
            })
            && contract
                .required_decision_fields
                .iter()
                .any(|field| *field == "governance_note_reference")
            && contract
                .required_decision_field_contracts
                .iter()
                .any(|field| {
                    field.name == "governance_note_reference"
                        && field.evidence_source == "external-release-tracker"
                })
            && contract
                .exception_record_fields
                .iter()
                .any(|field| *field == "exception_reason")
            && !contract
                .exception_record_fields
                .iter()
                .any(|field| *field == "governance_note_reference")
            && contract.section_execution_workflow.iter().any(|step| {
                step.sequence == 3
                    && step.record_effect.contains("exception_reason")
                    && step.record_effect.contains("rollback_reference")
            })
            && contract
                .section_instance_validation_contract
                .as_ref()
                .is_some_and(|validation| {
                    validation
                        .evidence_linked_requirements
                        .required_fields
                        .iter()
                        .any(|field| *field == "governance_note_reference")
                        && validation
                            .terminal_requirements
                            .conditional_required_fields
                            .iter()
                            .any(|requirement| {
                                requirement.when_result_statuses == vec!["exception-accepted"]
                                    && requirement
                                        .required_fields
                                        .iter()
                                        .any(|field| *field == "exception_reason")
                                    && requirement
                                        .required_fields
                                        .iter()
                                        .any(|field| *field == "rollback_reference")
                            })
                })
    }));
    assert!(report.operator_review_items.iter().any(|item| {
        item.kind == "governance-finding-review"
            && item.detail.contains("remote-unaudited-web-opt-in-active")
    }));
    assert!(report.follow_up_endpoints.iter().any(|follow_up| {
        follow_up.signal == "governance-audit"
            && follow_up.endpoint == "/health/governance"
            && !follow_up.blocking
            && follow_up.owner == "release-operator"
    }));
    assert!(report.review_decision_contracts.iter().any(|contract| {
        contract.signal == "public-entry-handoff"
            && contract.exception_reference_kind == "public-entry-handoff-exception-record"
    }));
    assert!(
        report
            .post_review_actions
            .iter()
            .any(|action| { action.contains("external release tracker") })
    );
    assert!(
        report
            .post_review_actions
            .iter()
            .any(|action| action.contains("post_archive verification fields"))
    );
}

#[test]
fn preflight_report_routes_experimental_quic_governance_findings_to_transport_security() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    seed_live_runtime_state(&state);
    seed_recovery_staging_restore_state(&recovery_staging_state);

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: vec![crate::settings::TransportSecurityInventoryEntry {
            surface: "game-quic",
            bind_address: "0.0.0.0:14004".parse().unwrap(),
            transport: "quic",
            encryption: "tls-required",
            cert_file_path: Path::new("tls").join("cert.pem"),
            key_file_path: Path::new("tls").join("key.pem"),
            rollout_policy:
                crate::settings::TransportSecurityRolloutPolicy::ExperimentalOptInActive,
            validation_policy:
                crate::settings::TransportSecurityValidationPolicy::FailFastAtStartup,
            material_state: crate::settings::TransportSecurityMaterialState::Invalid,
            detail: "No valid TLS certificate chain found in tls/cert.pem".to_owned(),
        }],
        governance_findings: vec![crate::settings::RuntimeGovernanceFinding {
            id: "experimental-quic-opt-in-active",
            severity: crate::settings::RuntimeGovernanceSeverity::Warning,
            subject: "game-quic",
            detail: "QUIC rollout remains operator-governed experimental infrastructure".to_owned(),
        }],
    }
    .preflight_report();

    let _ = fs::remove_dir_all(root);

    assert!(report.review_decision_contracts.iter().any(|contract| {
        contract.signal == "governance-audit"
            && contract.supporting_endpoints.iter().any(|endpoint| {
                endpoint.signal == "transport-security"
                    && endpoint.endpoint == "/health/transport-security"
                    && endpoint.owner == "release-operator"
                    && endpoint.related_findings == vec!["experimental-quic-opt-in-active"]
            })
    }));
}

#[test]
fn preflight_report_routes_remote_query_governance_findings_to_runtime_surfaces() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    seed_live_runtime_state(&state);
    seed_recovery_staging_restore_state(&recovery_staging_state);

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: vec![crate::settings::RuntimeSurface {
            name: "query-server",
            bind_address: Some("0.0.0.0:14006".parse().unwrap()),
            reachability: crate::settings::SurfaceReachability::NetworkAccessible,
            auth: crate::settings::SurfaceAuth::None,
            credential_bootstrap: crate::settings::SurfaceCredentialBootstrap::None,
            review_status: crate::settings::SurfaceReviewStatus::DiscoveryOnlyNotAuthority,
            remote_exposure_policy:
                crate::settings::SurfaceRemoteExposurePolicy::RemoteRequiresExplicitQueryOptIn,
            purpose: crate::settings::SurfacePurpose::LightweightDiscovery,
            consumption: crate::settings::SurfaceConsumption::DiscoveryQuery,
        }],
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: vec![crate::settings::RuntimeGovernanceFinding {
            id: "remote-query-opt-in-active",
            severity: crate::settings::RuntimeGovernanceSeverity::Warning,
            subject: "query-server",
            detail: "query server remote exposure remains discovery-only and operator-reviewed"
                .to_owned(),
        }],
    }
    .preflight_report();

    let _ = fs::remove_dir_all(root);

    assert!(report.review_decision_contracts.iter().any(|contract| {
        contract.signal == "governance-audit"
            && contract.supporting_endpoints.iter().any(|endpoint| {
                endpoint.signal == "runtime-surfaces"
                    && endpoint.endpoint == "/health/surfaces"
                    && endpoint.owner == "release-operator"
                    && endpoint.related_findings == vec!["remote-query-opt-in-active"]
            })
    }));
    assert!(report.review_decision_contracts.iter().any(|contract| {
        contract.signal == "governance-audit"
            && contract.supporting_endpoints.iter().any(|endpoint| {
                endpoint.signal == "compatibility-contract"
                    && endpoint.endpoint == "/health/compatibility"
                    && endpoint.owner == "release-operator"
                    && endpoint.related_findings == vec!["remote-query-opt-in-active"]
            })
    }));
}

#[test]
fn preflight_report_can_be_clear_when_runtime_and_governance_are_clean() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    seed_live_runtime_state(&state);
    seed_recovery_staging_restore_state(&recovery_staging_state);

    let report = HealthState {
        environment: "local",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .preflight_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "preflight_clear");
    assert!(!report.release_blocked);
    assert!(!report.requires_operator_review);
    assert!(report.blocking_signals.is_empty());
    assert!(report.review_signals.is_empty());
    assert!(report.follow_up_endpoints.is_empty());
    assert!(report.review_decision_contracts.is_empty());
    assert!(report.operator_review_items.is_empty());
    assert!(
        report
            .components
            .iter()
            .all(|component| !component.blocking)
    );
    assert!(report.components.iter().any(|component| {
        component.signal == "runtime-listeners" && component.status == "runtime-listeners-ready"
    }));
    assert!(report.components.iter().any(|component| {
        component.signal == "governance-audit" && component.status == "governance_clear"
    }));
    assert!(report.components.iter().any(|component| {
        component.signal == "public-entry-handoff"
            && component.status == "not-applicable-local"
            && !component.blocking
            && !component.requires_operator_review
    }));
}

#[test]
fn preflight_report_blocks_when_required_runtime_listener_is_not_listening() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    fs::create_dir_all(&state.config_dir).expect("should create config dir");
    fs::create_dir_all(&state.database_dir).expect("should create database dir");
    fs::create_dir_all(&state.ops_dir).expect("should create ops dir");
    seed_identity_file(&state.identity_file);
    seed_database_file(&state.database_dir);
    fs::create_dir_all(&recovery_staging_state.data_dir)
        .expect("should create recovery staging dir");

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(vec![
            server::RuntimeListenerStatus {
                surface: server::RuntimeListenerSurface::QueryServer,
                bind_address: "0.0.0.0:14006".parse().unwrap(),
                state: server::RuntimeListenerState::StartupFailed,
                detail: "failed to bind query server listener on 0.0.0.0:14006: address in use"
                    .to_owned(),
            },
        ])),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: vec![crate::settings::RuntimeSurface {
            name: "query-server",
            bind_address: Some("0.0.0.0:14006".parse().unwrap()),
            reachability: crate::settings::SurfaceReachability::NetworkAccessible,
            auth: crate::settings::SurfaceAuth::None,
            credential_bootstrap: crate::settings::SurfaceCredentialBootstrap::None,
            review_status: crate::settings::SurfaceReviewStatus::DiscoveryOnlyNotAuthority,
            remote_exposure_policy:
                crate::settings::SurfaceRemoteExposurePolicy::RemoteRequiresExplicitQueryOptIn,
            purpose: crate::settings::SurfacePurpose::LightweightDiscovery,
            consumption: crate::settings::SurfaceConsumption::DiscoveryQuery,
        }],
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .preflight_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "preflight_blocked");
    assert!(report.release_blocked);
    assert!(
        report
            .blocking_signals
            .iter()
            .any(|signal| *signal == "runtime-listeners")
    );
    assert!(report.components.iter().any(|component| {
        component.signal == "runtime-listeners"
            && component.blocking
            && component.status == "runtime-listeners-blocked"
    }));
    assert!(report.follow_up_endpoints.iter().any(|follow_up| {
        follow_up.signal == "runtime-listeners"
            && follow_up.endpoint == "/health/listeners"
            && follow_up.blocking
            && follow_up.owner == "service-operator"
    }));
    assert!(report.operator_review_items.iter().any(|item| {
        item.kind == "runtime-listener-failure"
            && item.blocking
            && item.detail.contains("query-server 0.0.0.0:14006")
            && item.detail.contains("startup-failed")
    }));
}

#[test]
fn recovery_contract_includes_audit_and_database_state() {
    let contract = test_health_state(Path::new("test-root")).recovery_contract();

    assert!(contract.state_inventory.iter().any(|entry| {
        entry.kind == "character-database"
            && entry.recovery_class == "must-keep"
            && entry.backup_expectation == "required-in-backup"
    }));
    assert!(contract.state_inventory.iter().any(|entry| {
        entry.kind == "operational-audit-trail"
            && entry.recovery_class == "manual-repair"
            && entry.backup_expectation == "recommended-in-backup"
    }));
    assert!(contract.state_inventory.iter().any(|entry| {
        entry.kind == "backup-evidence-trail"
            && entry.recovery_class == "manual-repair"
            && entry.backup_expectation == "recommended-in-backup"
    }));
    assert!(contract.state_inventory.iter().any(|entry| {
        entry.kind == "recovery-drill-evidence-trail"
            && entry.recovery_class == "manual-repair"
            && entry.backup_expectation == "recommended-in-backup"
    }));
    assert_eq!(contract.audit_retention.format, "ron-line");
    assert_eq!(contract.audit_retention.max_archive_files, 7);
    assert!(contract.recovery_staging.isolated_from_live_state);
    assert!(
        contract
            .minimum_recovery_drill
            .iter()
            .any(|step| step.contains("isolated directory"))
    );
}

#[test]
fn recovery_contract_includes_state_governance_metadata() {
    let contract = test_health_state(Path::new("test-root")).recovery_contract();

    assert!(contract.state_inventory.iter().any(|entry| {
        entry.kind == "character-database"
            && entry.data_domain == "character-persistence"
            && entry.write_owner == "server-core-persistence"
            && entry.consistency_requirement == "stable-authoritative"
            && entry.migration_strategy == "schema-managed-in-process"
    }));
    assert!(contract.state_inventory.iter().any(|entry| {
        entry.kind == "config-dir"
            && entry.data_domain == "environment-config"
            && entry.write_owner == "server-core-settings"
            && entry.consistency_requirement == "authoritative-with-operator-review"
            && entry.migration_strategy == "manual-file-review"
    }));
    assert!(contract.state_inventory.iter().any(|entry| {
        entry.kind == "terrain-persistence"
            && entry.data_domain == "world-runtime"
            && entry.write_owner == "server-core-terrain-persistence"
            && entry.consistency_requirement == "derived-rebuildable"
            && entry.migration_strategy == "rebuild-or-discard"
    }));
    assert!(contract.state_inventory.iter().any(|entry| {
        entry.kind == "operational-audit-trail"
            && entry.data_domain == "operational-evidence"
            && entry.write_owner == "server-cli-ops"
            && entry.consistency_requirement == "append-only-evidence"
            && entry.migration_strategy == "rotate-and-archive"
    }));
}

#[test]
fn backup_report_blocks_when_required_backup_state_is_missing() {
    let report = test_health_state(Path::new("test-root")).backup_report();

    assert_eq!(report.status, "backup_blocked");
    assert!(
        report
            .checks
            .iter()
            .any(|check| { check.kind == "instance-identity" && check.required && !check.ok })
    );
    assert!(
        report
            .responsibility_boundary
            .external_orchestrator
            .iter()
            .any(|item| item.contains("off-host"))
    );
    assert!(
        report
            .evidence_requirements
            .iter()
            .any(|field| field.name == "backup_artifact_id"
                && field.owner == "external-orchestrator")
    );
    assert!(report.evidence_sink.path.ends_with("backup-evidence.ronl"));
    assert!(report.evidence_sink_checks.iter().any(|check| !check.ok));
    assert!(
        report
            .result_status_model
            .iter()
            .any(|status| status.state == "pending-capture")
    );
}

#[test]
fn backup_report_allows_missing_rebuildable_state_when_required_state_exists() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    fs::create_dir_all(&state.config_dir).expect("should create config dir");
    fs::create_dir_all(&state.database_dir).expect("should create database dir");
    fs::create_dir_all(&state.ops_dir).expect("should create ops dir");
    seed_identity_file(&state.identity_file);
    seed_database_file(&state.database_dir);
    fs::create_dir_all(&recovery_staging_state.data_dir)
        .expect("should create recovery staging dir");

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .backup_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "backup_ready");
    assert!(report.checks.iter().any(|check| {
        check.kind == "terrain-persistence"
            && !check.required
            && !check.ok
            && check.backup_expectation == "may-be-rebuilt"
    }));
    assert!(
        report
            .responsibility_boundary
            .this_process
            .iter()
            .any(|item| item.contains("required backup scope"))
    );
    assert!(
        report
            .evidence_requirements
            .iter()
            .any(|field| field.name == "storage_location")
    );
    assert!(
        report
            .evidence_sink_checks
            .iter()
            .all(|check| !check.required || check.ok)
    );
    assert_eq!(report.evidence_write_contract.write_mode, "append-only");
    assert_eq!(
        report.evidence_write_contract.record_granularity,
        "one-record-per-backup-status-transition"
    );
    assert!(report.evidence_write_contract.external_archive_required);
    assert_eq!(
        report.archive_handoff_contract.authoritative_archive_owner,
        "external-orchestrator"
    );
    assert!(
        report
            .archive_handoff_contract
            .handoff_ready_states
            .iter()
            .any(|state| *state == "captured")
    );
    assert!(
        report
            .archive_handoff_contract
            .terminal_states_requiring_archive_receipt
            .iter()
            .any(|state| *state == "restore-verified")
    );
    assert!(
        report
            .archive_handoff_contract
            .required_archive_receipt_fields
            .iter()
            .any(|field| *field == "archive_reference")
    );
    assert!(
        report
            .archive_handoff_contract
            .local_record_not_sufficient_without_archive
    );
    assert!(
        report
            .result_status_model
            .iter()
            .any(|status| status.state == "restore-verified")
    );
}

#[test]
fn backup_report_blocks_when_database_file_is_invalid() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    fs::create_dir_all(&state.config_dir).expect("should create config dir");
    fs::create_dir_all(&state.database_dir).expect("should create database dir");
    fs::create_dir_all(&state.ops_dir).expect("should create ops dir");
    seed_identity_file(&state.identity_file);
    fs::write(&state.database_file, b"not a sqlite database")
        .expect("should create invalid database file");
    fs::create_dir_all(&recovery_staging_state.data_dir)
        .expect("should create recovery staging dir");

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .backup_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "backup_blocked");
    assert!(report.checks.iter().any(|check| {
        check.kind == "character-database"
            && check.required
            && !check.ok
            && check.detail.contains("not a readable SQLite database")
    }));
}

#[test]
fn backup_report_blocks_when_evidence_sink_target_is_a_directory() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    fs::create_dir_all(&state.config_dir).expect("should create config dir");
    fs::create_dir_all(&state.database_dir).expect("should create database dir");
    fs::create_dir_all(&state.ops_dir).expect("should create ops dir");
    seed_identity_file(&state.identity_file);
    seed_database_file(&state.database_dir);
    fs::create_dir_all(&state.backup_evidence_log_file)
        .expect("should create invalid evidence sink directory");
    fs::create_dir_all(&recovery_staging_state.data_dir)
        .expect("should create recovery staging dir");

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .backup_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "backup_blocked");
    assert!(
        report
            .evidence_sink_checks
            .iter()
            .any(|check| !check.ok && check.detail.contains("absent or a regular file"))
    );
}

#[test]
fn recovery_drill_report_is_ready_for_isolated_layout() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    seed_live_runtime_state(&state);
    seed_recovery_staging_restore_state(&recovery_staging_state);

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .recovery_drill_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "drill_ready");
    assert!(report.checks.iter().all(|check| check.ok));
    assert!(
        report
            .responsibility_boundary
            .external_orchestrator
            .iter()
            .any(|item| item.contains("periodic restore drills"))
    );
    assert!(
        report
            .evidence_requirements
            .iter()
            .any(|field| field.name == "cutover_decision")
    );
    assert!(
        report
            .evidence_sink
            .path
            .ends_with("recovery-drill-evidence.ronl")
    );
    assert!(
        report
            .evidence_sink_checks
            .iter()
            .all(|check| !check.required || check.ok)
    );
    assert_eq!(report.evidence_write_contract.write_mode, "append-only");
    assert_eq!(
        report.evidence_write_contract.record_granularity,
        "one-record-per-drill-status-transition"
    );
    assert!(report.evidence_write_contract.external_archive_required);
    assert_eq!(
        report.archive_handoff_contract.authoritative_archive_owner,
        "external-orchestrator"
    );
    assert!(
        report
            .archive_handoff_contract
            .handoff_ready_states
            .iter()
            .any(|state| *state == "ready-validated")
    );
    assert!(
        report
            .archive_handoff_contract
            .terminal_states_requiring_archive_receipt
            .iter()
            .any(|state| *state == "cutover-approved")
    );
    assert!(
        report
            .archive_handoff_contract
            .required_archive_receipt_fields
            .iter()
            .any(|field| *field == "archive_reference")
    );
    assert!(
        report
            .archive_handoff_contract
            .local_record_not_sufficient_without_archive
    );
    assert_eq!(
        report.execution_contract.execution_owner,
        "external-orchestrator"
    );
    assert!(
        report
            .execution_contract
            .required_signoff_fields
            .iter()
            .any(|field| *field == "approval_decision")
    );
    assert!(
        report
            .execution_contract
            .required_post_drill_actions
            .iter()
            .any(|action| action.contains("authoritative external archive"))
    );
    assert!(
        report
            .result_status_model
            .iter()
            .any(|status| status.state == "ready-validated")
    );
}

#[test]
fn recovery_drill_report_blocks_when_staging_overlaps_live_state() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    fs::create_dir_all(&state.config_dir).expect("should create config dir");
    fs::create_dir_all(&state.database_dir).expect("should create database dir");
    fs::create_dir_all(&state.ops_dir).expect("should create ops dir");
    seed_identity_file(&state.identity_file);
    seed_database_file(&state.database_dir);

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state.clone(),
        recovery_staging_state: server::ServerStatePaths::new(root.join("live").join("restore")),
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .recovery_drill_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "drill_blocked");
    assert!(
        report
            .checks
            .iter()
            .any(|check| { check.name == "recovery-staging-layout-isolated" && !check.ok })
    );
    assert!(
        report
            .responsibility_boundary
            .this_process
            .iter()
            .any(|item| item.contains("cutover preconditions"))
    );
    assert!(
        report
            .evidence_requirements
            .iter()
            .any(|field| field.name == "rollback_reference")
    );
    assert!(
        report
            .evidence_sink_checks
            .iter()
            .all(|check| !check.required || check.ok)
    );
    assert!(
        report
            .result_status_model
            .iter()
            .any(|status| status.state == "rolled-back")
    );
}

#[test]
fn recovery_drill_report_blocks_when_restored_state_is_missing_from_staging_layout() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    fs::create_dir_all(&state.config_dir).expect("should create config dir");
    fs::create_dir_all(&state.database_dir).expect("should create database dir");
    fs::create_dir_all(&state.ops_dir).expect("should create ops dir");
    seed_identity_file(&state.identity_file);
    seed_database_file(&state.database_dir);
    fs::create_dir_all(&recovery_staging_state.data_dir)
        .expect("should create recovery staging dir");

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .recovery_drill_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "drill_blocked");
    assert!(report.checks.iter().any(|check| {
        check.name == "staged-identity-file-ready" && check.required && !check.ok
    }));
    assert!(report.checks.iter().any(|check| {
        check.name == "staged-database-file-ready" && check.required && !check.ok
    }));
    assert!(
        report
            .checks
            .iter()
            .any(|check| { check.name == "staged-config-present" && check.required && !check.ok })
    );
}

#[test]
fn recovery_drill_report_blocks_when_live_database_file_is_invalid() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    seed_live_runtime_state(&state);
    seed_recovery_staging_restore_state(&recovery_staging_state);
    fs::write(&state.database_file, b"not a sqlite database")
        .expect("should write invalid live db file");

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .recovery_drill_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "drill_blocked");
    assert!(report.checks.iter().any(|check| {
        check.name == "live-database-file-ready"
            && check.required
            && !check.ok
            && check.detail.contains("readable SQLite database")
    }));
}

#[test]
fn recovery_drill_report_blocks_when_staged_database_file_is_invalid() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    seed_live_runtime_state(&state);
    seed_recovery_staging_restore_state(&recovery_staging_state);
    fs::write(
        &recovery_staging_state.database_file,
        b"not a sqlite database",
    )
    .expect("should write invalid staged db file");

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .recovery_drill_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "drill_blocked");
    assert!(report.checks.iter().any(|check| {
        check.name == "staged-database-file-ready"
            && check.required
            && !check.ok
            && check.detail.contains("readable SQLite database")
    }));
}

#[test]
fn recovery_drill_report_blocks_when_live_identity_file_is_invalid() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    seed_live_runtime_state(&state);
    seed_recovery_staging_restore_state(&recovery_staging_state);
    fs::write(
        server::settings::identity_file_path(&state.data_dir),
        b"this is not valid ron",
    )
    .expect("should write invalid live identity");

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .recovery_drill_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "drill_blocked");
    assert!(report.checks.iter().any(|check| {
        check.name == "live-identity-file-ready"
            && check.required
            && !check.ok
            && check.detail.contains("not valid RON")
    }));
}

#[test]
fn recovery_drill_report_blocks_when_staged_identity_file_is_invalid() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    seed_live_runtime_state(&state);
    seed_recovery_staging_restore_state(&recovery_staging_state);
    fs::write(
        server::settings::identity_file_path(&recovery_staging_state.data_dir),
        b"this is not valid ron",
    )
    .expect("should write invalid staged identity");

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .recovery_drill_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "drill_blocked");
    assert!(report.checks.iter().any(|check| {
        check.name == "staged-identity-file-ready"
            && check.required
            && !check.ok
            && check.detail.contains("not valid RON")
    }));
}

#[test]
fn recovery_drill_report_blocks_when_live_settings_file_is_invalid() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    seed_live_runtime_state(&state);
    seed_recovery_staging_restore_state(&recovery_staging_state);
    fs::write(
        server::settings::settings_file_path(&state.data_dir),
        b"this is not valid ron",
    )
    .expect("should write invalid live settings");

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .recovery_drill_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "drill_blocked");
    assert!(report.checks.iter().any(|check| {
        check.name == "live-settings-file-ready"
            && check.required
            && !check.ok
            && check.detail.contains("not valid RON")
    }));
}

#[test]
fn recovery_drill_report_blocks_when_staged_settings_file_is_missing() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    seed_live_runtime_state(&state);
    fs::create_dir_all(&recovery_staging_state.data_dir)
        .expect("should create recovery staging dir");
    fs::create_dir_all(&recovery_staging_state.config_dir)
        .expect("should create recovery staging config dir");
    fs::create_dir_all(&recovery_staging_state.database_dir)
        .expect("should create recovery staging database dir");
    seed_identity_file(&recovery_staging_state.identity_file);
    seed_database_file(&recovery_staging_state.database_dir);

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .recovery_drill_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "drill_blocked");
    assert!(report.checks.iter().any(|check| {
        check.name == "staged-settings-file-ready"
            && check.required
            && !check.ok
            && check.detail.contains("settings.ron")
    }));
}

#[test]
fn recovery_drill_report_blocks_when_staged_settings_file_is_invalid() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    seed_live_runtime_state(&state);
    seed_recovery_staging_restore_state(&recovery_staging_state);
    fs::write(
        server::settings::settings_file_path(&recovery_staging_state.data_dir),
        b"this is not valid ron",
    )
    .expect("should write invalid staged settings");

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .recovery_drill_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "drill_blocked");
    assert!(report.checks.iter().any(|check| {
        check.name == "staged-settings-file-ready"
            && check.required
            && !check.ok
            && check.detail.contains("not valid RON")
    }));
}

#[test]
fn recovery_drill_report_blocks_when_staging_contains_restored_local_ops_trails() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    seed_live_runtime_state(&state);
    seed_recovery_staging_restore_state(&recovery_staging_state);
    fs::create_dir_all(&recovery_staging_state.ops_dir).expect("should create staged ops dir");
    fs::write(&recovery_staging_state.audit_log_file, b"old audit trail")
        .expect("should write staged audit trail");
    fs::write(
        &recovery_staging_state.backup_evidence_log_file,
        b"old backup evidence",
    )
    .expect("should write staged backup evidence");
    fs::write(
        &recovery_staging_state.recovery_drill_evidence_log_file,
        b"old drill evidence",
    )
    .expect("should write staged recovery drill evidence");

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .recovery_drill_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "drill_blocked");
    assert!(
        report.checks.iter().any(|check| {
            check.name == "staged-audit-trail-clear" && check.required && !check.ok
        })
    );
    assert!(report.checks.iter().any(|check| {
        check.name == "staged-backup-evidence-clear" && check.required && !check.ok
    }));
    assert!(report.checks.iter().any(|check| {
        check.name == "staged-recovery-drill-evidence-clear" && check.required && !check.ok
    }));
    assert!(
        report
            .cutover_preconditions
            .iter()
            .any(|item| { item.contains("clear restored local audit and evidence trails") })
    );
}

#[test]
fn recovery_drill_report_blocks_when_evidence_sink_conflicts_with_audit_log() {
    let root = unique_temp_dir();
    let state = server::ServerStatePaths::new(root.join("live"));
    let recovery_staging_state = server::ServerStatePaths::new(root.join("recovery-staging"));
    fs::create_dir_all(&state.config_dir).expect("should create config dir");
    fs::create_dir_all(&state.database_dir).expect("should create database dir");
    fs::create_dir_all(&state.ops_dir).expect("should create ops dir");
    seed_identity_file(&state.identity_file);
    seed_database_file(&state.database_dir);
    fs::create_dir_all(&recovery_staging_state.data_dir)
        .expect("should create recovery staging dir");
    fs::create_dir_all(&recovery_staging_state.config_dir)
        .expect("should create recovery staging config dir");
    fs::create_dir_all(&recovery_staging_state.database_dir)
        .expect("should create recovery staging database dir");
    seed_identity_file(&recovery_staging_state.identity_file);
    seed_database_file(&recovery_staging_state.database_dir);

    let mut conflict_state = state.clone();
    conflict_state.recovery_drill_evidence_log_file = conflict_state.audit_log_file.clone();

    let report = HealthState {
        environment: "production",
        auth_server_configured: true,
        authoritative_auth_provider: test_auth_provider(true),
        server_state: conflict_state,
        recovery_staging_state,
        audit_retention: crate::settings::AuditRetentionPolicy::default(),
        runtime_listener_inventory: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        runtime_observability_inventory: test_runtime_observability_inventory(),
        surface_inventory: Vec::new(),
        management_auth_inventory: Vec::new(),
        transport_security_inventory: Vec::new(),
        governance_findings: Vec::new(),
    }
    .recovery_drill_report();

    let _ = fs::remove_dir_all(root);

    assert_eq!(report.status, "drill_blocked");
    assert!(
        report
            .evidence_sink_checks
            .iter()
            .any(|check| !check.ok && check.detail.contains("conflicts with"))
    );
}

#[test]
fn bind_listener_fails_when_port_is_already_in_use() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("should create test runtime");

    runtime.block_on(async {
        let first = bind_listener("127.0.0.1:0".parse::<SocketAddr>().unwrap())
            .await
            .expect("should bind first listener");
        let addr = first
            .local_addr()
            .expect("listener should expose local addr");

        let error = bind_listener(addr)
            .await
            .expect_err("second bind on same address should fail");

        assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);
    });
}
