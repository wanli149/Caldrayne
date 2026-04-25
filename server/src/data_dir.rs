use std::path::{Path, PathBuf};

/// Used so that different server frontends can share the same server saves,
/// etc.
pub const DEFAULT_DATA_DIR_NAME: &str = "server";
pub const CONFIG_DIR_NAME: &str = "server_config";
pub const IDENTITY_FILENAME: &str = "identity.ron";
pub const SAVES_DIR_NAME: &str = "saves";
pub const DATABASE_FILENAME: &str = "db.sqlite";
pub const RTSIM_DIR_NAME: &str = "rtsim";
pub const RTSIM_FILENAME: &str = "data.dat";
pub const TERRAIN_DIR_NAME: &str = "terrain";
pub const OPS_DIR_NAME: &str = "ops";
pub const AUDIT_LOG_FILENAME: &str = "audit-log.ronl";
pub const BACKUP_EVIDENCE_LOG_FILENAME: &str = "backup-evidence.ronl";
pub const RECOVERY_DRILL_EVIDENCE_LOG_FILENAME: &str = "recovery-drill-evidence.ronl";

/// Indicates where maps, saves, and server_config folders are to be stored
pub struct DataDir {
    pub path: PathBuf,
}
impl AsRef<Path> for DataDir {
    fn as_ref(&self) -> &Path { &self.path }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerStateKind {
    ConfigDir,
    InstanceIdentity,
    CharacterDatabase,
    RtSimState,
    TerrainPersistence,
    OperationalAuditTrail,
    BackupEvidenceTrail,
    RecoveryDrillEvidenceTrail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryClass {
    MustKeep,
    ManualRepair,
    Rebuildable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerStateDomain {
    EnvironmentConfig,
    InstanceMetadata,
    CharacterPersistence,
    WorldRuntime,
    OperationalEvidence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerStateWriteOwner {
    ServerCoreSettings,
    ServerCoreIdentity,
    ServerCorePersistence,
    ServerCoreRtSim,
    ServerCoreTerrainPersistence,
    ServerCliOperations,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerStateConsistency {
    StableAuthoritative,
    AuthoritativeWithOperatorReview,
    DerivedRebuildable,
    AppendOnlyEvidence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerStateMigration {
    ManualFileReview,
    PreserveOrRepair,
    SchemaManagedInProcess,
    RebuildOrDiscard,
    RotateAndArchive,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerStateEntry {
    pub kind: ServerStateKind,
    pub path: PathBuf,
    pub recovery: RecoveryClass,
    pub domain: ServerStateDomain,
    pub write_owner: ServerStateWriteOwner,
    pub consistency: ServerStateConsistency,
    pub migration: ServerStateMigration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerStatePaths {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub identity_file: PathBuf,
    pub database_dir: PathBuf,
    pub database_file: PathBuf,
    pub rtsim_dir: PathBuf,
    pub rtsim_data_file: PathBuf,
    pub terrain_dir: PathBuf,
    pub ops_dir: PathBuf,
    pub audit_log_file: PathBuf,
    pub backup_evidence_log_file: PathBuf,
    pub recovery_drill_evidence_log_file: PathBuf,
}

impl ServerStatePaths {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self::with_overrides(
            data_dir,
            std::env::var_os("VELOREN_RTSIM").map(PathBuf::from),
            std::env::var_os("VELOREN_TERRAIN").map(PathBuf::from),
        )
    }

    pub fn with_overrides(
        data_dir: impl Into<PathBuf>,
        rtsim_override: Option<PathBuf>,
        terrain_override: Option<PathBuf>,
    ) -> Self {
        let data_dir = data_dir.into();
        let config_dir = data_dir.join(CONFIG_DIR_NAME);
        let database_dir = data_dir.join(SAVES_DIR_NAME);
        let rtsim_dir = rtsim_override.unwrap_or_else(|| data_dir.join(RTSIM_DIR_NAME));
        let terrain_dir = terrain_override.unwrap_or_else(|| data_dir.join(TERRAIN_DIR_NAME));
        let ops_dir = data_dir.join(OPS_DIR_NAME);

        Self {
            identity_file: data_dir.join(IDENTITY_FILENAME),
            database_file: database_dir.join(DATABASE_FILENAME),
            rtsim_data_file: rtsim_dir.join(RTSIM_FILENAME),
            audit_log_file: ops_dir.join(AUDIT_LOG_FILENAME),
            backup_evidence_log_file: ops_dir.join(BACKUP_EVIDENCE_LOG_FILENAME),
            recovery_drill_evidence_log_file: ops_dir.join(RECOVERY_DRILL_EVIDENCE_LOG_FILENAME),
            data_dir,
            config_dir,
            database_dir,
            rtsim_dir,
            terrain_dir,
            ops_dir,
        }
    }

    pub fn inventory(&self) -> [ServerStateEntry; 8] {
        [
            ServerStateEntry {
                kind: ServerStateKind::ConfigDir,
                path: self.config_dir.clone(),
                recovery: RecoveryClass::ManualRepair,
                domain: ServerStateDomain::EnvironmentConfig,
                write_owner: ServerStateWriteOwner::ServerCoreSettings,
                consistency: ServerStateConsistency::AuthoritativeWithOperatorReview,
                migration: ServerStateMigration::ManualFileReview,
            },
            ServerStateEntry {
                kind: ServerStateKind::InstanceIdentity,
                path: self.identity_file.clone(),
                recovery: RecoveryClass::MustKeep,
                domain: ServerStateDomain::InstanceMetadata,
                write_owner: ServerStateWriteOwner::ServerCoreIdentity,
                consistency: ServerStateConsistency::StableAuthoritative,
                migration: ServerStateMigration::PreserveOrRepair,
            },
            ServerStateEntry {
                kind: ServerStateKind::CharacterDatabase,
                path: self.database_file.clone(),
                recovery: RecoveryClass::MustKeep,
                domain: ServerStateDomain::CharacterPersistence,
                write_owner: ServerStateWriteOwner::ServerCorePersistence,
                consistency: ServerStateConsistency::StableAuthoritative,
                migration: ServerStateMigration::SchemaManagedInProcess,
            },
            ServerStateEntry {
                kind: ServerStateKind::RtSimState,
                path: self.rtsim_data_file.clone(),
                recovery: RecoveryClass::ManualRepair,
                domain: ServerStateDomain::WorldRuntime,
                write_owner: ServerStateWriteOwner::ServerCoreRtSim,
                consistency: ServerStateConsistency::AuthoritativeWithOperatorReview,
                migration: ServerStateMigration::PreserveOrRepair,
            },
            ServerStateEntry {
                kind: ServerStateKind::TerrainPersistence,
                path: self.terrain_dir.clone(),
                recovery: RecoveryClass::Rebuildable,
                domain: ServerStateDomain::WorldRuntime,
                write_owner: ServerStateWriteOwner::ServerCoreTerrainPersistence,
                consistency: ServerStateConsistency::DerivedRebuildable,
                migration: ServerStateMigration::RebuildOrDiscard,
            },
            ServerStateEntry {
                kind: ServerStateKind::OperationalAuditTrail,
                path: self.audit_log_file.clone(),
                recovery: RecoveryClass::ManualRepair,
                domain: ServerStateDomain::OperationalEvidence,
                write_owner: ServerStateWriteOwner::ServerCliOperations,
                consistency: ServerStateConsistency::AppendOnlyEvidence,
                migration: ServerStateMigration::RotateAndArchive,
            },
            ServerStateEntry {
                kind: ServerStateKind::BackupEvidenceTrail,
                path: self.backup_evidence_log_file.clone(),
                recovery: RecoveryClass::ManualRepair,
                domain: ServerStateDomain::OperationalEvidence,
                write_owner: ServerStateWriteOwner::ServerCliOperations,
                consistency: ServerStateConsistency::AppendOnlyEvidence,
                migration: ServerStateMigration::RotateAndArchive,
            },
            ServerStateEntry {
                kind: ServerStateKind::RecoveryDrillEvidenceTrail,
                path: self.recovery_drill_evidence_log_file.clone(),
                recovery: RecoveryClass::ManualRepair,
                domain: ServerStateDomain::OperationalEvidence,
                write_owner: ServerStateWriteOwner::ServerCliOperations,
                consistency: ServerStateConsistency::AppendOnlyEvidence,
                migration: ServerStateMigration::RotateAndArchive,
            },
        ]
    }
}

pub fn with_config_dir(path: &Path) -> PathBuf { path.join(CONFIG_DIR_NAME) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_paths_use_standard_layout() {
        let root = PathBuf::from("userdata").join("server");
        let paths = ServerStatePaths::with_overrides(&root, None, None);

        assert_eq!(paths.config_dir, root.join("server_config"));
        assert_eq!(paths.identity_file, root.join("identity.ron"));
        assert_eq!(paths.database_file, root.join("saves").join("db.sqlite"));
        assert_eq!(paths.rtsim_data_file, root.join("rtsim").join("data.dat"));
        assert_eq!(paths.terrain_dir, root.join("terrain"));
        assert_eq!(paths.ops_dir, root.join("ops"));
        assert_eq!(
            paths.audit_log_file,
            root.join("ops").join("audit-log.ronl")
        );
        assert_eq!(
            paths.backup_evidence_log_file,
            root.join("ops").join("backup-evidence.ronl")
        );
        assert_eq!(
            paths.recovery_drill_evidence_log_file,
            root.join("ops").join("recovery-drill-evidence.ronl")
        );
    }

    #[test]
    fn overrides_replace_rtsim_and_terrain_paths_only() {
        let root = PathBuf::from("userdata").join("server");
        let paths = ServerStatePaths::with_overrides(
            &root,
            Some(PathBuf::from("D:/rtsim-state")),
            Some(PathBuf::from("E:/terrain-state")),
        );

        assert_eq!(paths.data_dir, root);
        assert_eq!(paths.rtsim_dir, PathBuf::from("D:/rtsim-state"));
        assert_eq!(
            paths.rtsim_data_file,
            PathBuf::from("D:/rtsim-state").join("data.dat")
        );
        assert_eq!(paths.terrain_dir, PathBuf::from("E:/terrain-state"));
        assert_eq!(
            paths.audit_log_file,
            root.join("ops").join("audit-log.ronl")
        );
        assert_eq!(
            paths.backup_evidence_log_file,
            root.join("ops").join("backup-evidence.ronl")
        );
        assert_eq!(
            paths.recovery_drill_evidence_log_file,
            root.join("ops").join("recovery-drill-evidence.ronl")
        );
    }

    #[test]
    fn inventory_reports_expected_recovery_classes() {
        let paths = ServerStatePaths::with_overrides(PathBuf::from("userdata/server"), None, None);
        let inventory = paths.inventory();

        assert_eq!(inventory[0].recovery, RecoveryClass::ManualRepair);
        assert_eq!(inventory[1].recovery, RecoveryClass::MustKeep);
        assert_eq!(inventory[2].recovery, RecoveryClass::MustKeep);
        assert_eq!(inventory[3].recovery, RecoveryClass::ManualRepair);
        assert_eq!(inventory[4].recovery, RecoveryClass::Rebuildable);
        assert_eq!(inventory[5].recovery, RecoveryClass::ManualRepair);
        assert_eq!(inventory[6].recovery, RecoveryClass::ManualRepair);
        assert_eq!(inventory[7].recovery, RecoveryClass::ManualRepair);
    }

    #[test]
    fn inventory_reports_expected_governance_dimensions() {
        let paths = ServerStatePaths::with_overrides(PathBuf::from("userdata/server"), None, None);
        let inventory = paths.inventory();

        assert_eq!(inventory[0].domain, ServerStateDomain::EnvironmentConfig);
        assert_eq!(
            inventory[0].write_owner,
            ServerStateWriteOwner::ServerCoreSettings
        );
        assert_eq!(
            inventory[0].consistency,
            ServerStateConsistency::AuthoritativeWithOperatorReview
        );
        assert_eq!(
            inventory[0].migration,
            ServerStateMigration::ManualFileReview
        );

        assert_eq!(inventory[1].domain, ServerStateDomain::InstanceMetadata);
        assert_eq!(
            inventory[1].write_owner,
            ServerStateWriteOwner::ServerCoreIdentity
        );
        assert_eq!(
            inventory[1].consistency,
            ServerStateConsistency::StableAuthoritative
        );
        assert_eq!(
            inventory[1].migration,
            ServerStateMigration::PreserveOrRepair
        );

        assert_eq!(inventory[2].domain, ServerStateDomain::CharacterPersistence);
        assert_eq!(
            inventory[2].write_owner,
            ServerStateWriteOwner::ServerCorePersistence
        );
        assert_eq!(
            inventory[2].consistency,
            ServerStateConsistency::StableAuthoritative
        );
        assert_eq!(
            inventory[2].migration,
            ServerStateMigration::SchemaManagedInProcess
        );

        assert_eq!(inventory[4].domain, ServerStateDomain::WorldRuntime);
        assert_eq!(
            inventory[4].write_owner,
            ServerStateWriteOwner::ServerCoreTerrainPersistence
        );
        assert_eq!(
            inventory[4].consistency,
            ServerStateConsistency::DerivedRebuildable
        );
        assert_eq!(
            inventory[4].migration,
            ServerStateMigration::RebuildOrDiscard
        );

        assert_eq!(inventory[5].domain, ServerStateDomain::OperationalEvidence);
        assert_eq!(
            inventory[5].write_owner,
            ServerStateWriteOwner::ServerCliOperations
        );
        assert_eq!(
            inventory[5].consistency,
            ServerStateConsistency::AppendOnlyEvidence
        );
        assert_eq!(
            inventory[5].migration,
            ServerStateMigration::RotateAndArchive
        );
    }
}
