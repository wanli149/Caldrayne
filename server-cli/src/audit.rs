use crate::settings::AuditRetentionPolicy;
use chrono::Utc;
use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditSource {
    Argv,
    Tui,
    UiApi,
    Signal,
    Runtime,
}

impl AuditSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Argv => "argv",
            Self::Tui => "tui",
            Self::UiApi => "ui-api",
            Self::Signal => "signal",
            Self::Runtime => "runtime",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditAction {
    RuntimeListenerFailure,
    ObservabilityRuntimeFailure,
    WebRuntimeFailure,
    StartupFailure,
    #[cfg(feature = "worldgen")]
    WorldCompatStartupReject,
    #[cfg(feature = "worldgen")]
    WorldCompatFallback,
    AdminAdd,
    AdminRemove,
    DisconnectAllClients,
    SendGlobalMessage,
    SetSqlLogMode,
    ShutdownAbort,
    ShutdownGraceful,
    ShutdownImmediate,
    ShutdownReachedDeadline,
}

impl AuditAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeListenerFailure => "runtime-listener-failure",
            Self::ObservabilityRuntimeFailure => "observability-runtime-failure",
            Self::WebRuntimeFailure => "web-runtime-failure",
            Self::StartupFailure => "startup-failure",
            #[cfg(feature = "worldgen")]
            Self::WorldCompatStartupReject => "world-compat-startup-reject",
            #[cfg(feature = "worldgen")]
            Self::WorldCompatFallback => "world-compat-fallback",
            Self::AdminAdd => "admin-add",
            Self::AdminRemove => "admin-remove",
            Self::DisconnectAllClients => "disconnect-all-clients",
            Self::SendGlobalMessage => "send-global-message",
            Self::SetSqlLogMode => "set-sql-log-mode",
            Self::ShutdownAbort => "shutdown-abort",
            Self::ShutdownGraceful => "shutdown-graceful",
            Self::ShutdownImmediate => "shutdown-immediate",
            Self::ShutdownReachedDeadline => "shutdown-reached-deadline",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditOutcome {
    Accepted,
    Failed,
    Ignored,
}

impl AuditOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Failed => "failed",
            Self::Ignored => "ignored",
        }
    }
}

#[derive(Serialize)]
struct AuditRecord<'a> {
    timestamp_utc: String,
    source: &'static str,
    action: &'static str,
    outcome: &'static str,
    detail: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditMaintenanceReport {
    pub active_file_size_bytes: u64,
    pub rotated_to: Option<PathBuf>,
    pub deleted_archives: Vec<PathBuf>,
    pub retained_archives: usize,
}

pub fn append_event(
    path: &Path,
    source: AuditSource,
    action: AuditAction,
    outcome: AuditOutcome,
    detail: &str,
) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let record = AuditRecord {
        timestamp_utc: Utc::now().to_rfc3339(),
        source: source.as_str(),
        action: action.as_str(),
        outcome: outcome.as_str(),
        detail,
    };
    let line = ron::ser::to_string(&record).map_err(io::Error::other)?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    file.flush()
}

pub fn apply_retention_policy(
    path: &Path,
    policy: AuditRetentionPolicy,
) -> io::Result<AuditMaintenanceReport> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let threshold_bytes = policy.max_active_file_bytes();
    let mut active_file_size_bytes = fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let mut rotated_to = None;

    if active_file_size_bytes > 0 && active_file_size_bytes > threshold_bytes {
        let archive_path = next_archive_path(path);
        fs::rename(path, &archive_path)?;
        rotated_to = Some(archive_path);
        active_file_size_bytes = 0;
    }

    let mut archives = archive_paths(path)?;
    archives.sort_by(|left, right| archive_sort_key(left).cmp(&archive_sort_key(right)));

    let archives_to_delete = archives.len().saturating_sub(policy.max_archive_files);
    let deleted_archives = archives
        .into_iter()
        .take(archives_to_delete)
        .map(|archive| {
            fs::remove_file(&archive)?;
            Ok(archive)
        })
        .collect::<io::Result<Vec<_>>>()?;

    let retained_archives = archive_paths(path)?.len();

    Ok(AuditMaintenanceReport {
        active_file_size_bytes,
        rotated_to,
        deleted_archives,
        retained_archives,
    })
}

fn next_archive_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("audit-log");
    let extension = path.extension().and_then(|extension| extension.to_str());
    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ");

    for suffix in 0usize.. {
        let filename = match (extension, suffix) {
            (Some(extension), 0) => format!("{stem}.{timestamp}.{extension}"),
            (Some(extension), _) => format!("{stem}.{timestamp}.{suffix}.{extension}"),
            (None, 0) => format!("{stem}.{timestamp}"),
            (None, _) => format!("{stem}.{timestamp}.{suffix}"),
        };
        let candidate = parent.join(filename);
        if !candidate.exists() {
            return candidate;
        }
    }

    unreachable!("infinite suffix search should always find a free archive name")
}

fn archive_paths(path: &Path) -> io::Result<Vec<PathBuf>> {
    let parent = match path.parent() {
        Some(parent) => parent,
        None => return Ok(Vec::new()),
    };
    if !parent.is_dir() {
        return Ok(Vec::new());
    }

    let active_filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("");
    let extension = path.extension().and_then(|extension| extension.to_str());
    let prefix = format!("{stem}.");

    let archives = fs::read_dir(parent)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<io::Result<Vec<_>>>()?
        .into_iter()
        .filter(|candidate| {
            let name = match candidate.file_name().and_then(|name| name.to_str()) {
                Some(name) => name,
                None => return false,
            };
            if name == active_filename || !name.starts_with(&prefix) {
                return false;
            }

            match extension {
                Some(extension) => name.ends_with(&format!(".{extension}")),
                None => true,
            }
        })
        .collect::<Vec<_>>();

    Ok(archives)
}

fn archive_sort_key(path: &Path) -> (std::time::SystemTime, String) {
    let modified = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    (modified, path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    fn unique_temp_path() -> std::path::PathBuf {
        static UNIQUE_COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let counter = UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir()
            .join(format!("caldrayne-audit-{unique}-{counter}"))
            .join("audit-log.ronl")
    }

    #[test]
    fn append_event_writes_ron_line() {
        let path = unique_temp_path();

        append_event(
            &path,
            AuditSource::Argv,
            AuditAction::ShutdownImmediate,
            AuditOutcome::Accepted,
            "operator requested immediate shutdown",
        )
        .expect("audit append should succeed");

        let line = fs::read_to_string(&path).expect("audit log should be readable");

        let _ = fs::remove_dir_all(path.parent().expect("audit path should have parent"));

        assert!(line.contains("source:\"argv\""));
        assert!(line.contains("action:\"shutdown-immediate\""));
        assert!(line.contains("outcome:\"accepted\""));
    }

    #[test]
    fn append_event_writes_failed_outcome() {
        let path = unique_temp_path();

        append_event(
            &path,
            AuditSource::Runtime,
            AuditAction::StartupFailure,
            AuditOutcome::Failed,
            "failed to bind web listener on 127.0.0.1:14005: address in use",
        )
        .expect("audit append should succeed");

        let line = fs::read_to_string(&path).expect("audit log should be readable");

        let _ = fs::remove_dir_all(path.parent().expect("audit path should have parent"));

        assert!(line.contains("source:\"runtime\""));
        assert!(line.contains("action:\"startup-failure\""));
        assert!(line.contains("outcome:\"failed\""));
    }

    #[cfg(feature = "worldgen")]
    #[test]
    fn append_event_writes_world_compat_fallback_action() {
        let path = unique_temp_path();

        append_event(
            &path,
            AuditSource::Runtime,
            AuditAction::WorldCompatFallback,
            AuditOutcome::Accepted,
            "dedicated startup continued after strict world load contract fallback: world compat \
             audit: entry=load, decision=fallback_generate, failure=missing_input",
        )
        .expect("audit append should succeed");

        let line = fs::read_to_string(&path).expect("audit log should be readable");

        let _ = fs::remove_dir_all(path.parent().expect("audit path should have parent"));

        assert!(line.contains("source:\"runtime\""));
        assert!(line.contains("action:\"world-compat-fallback\""));
        assert!(line.contains("outcome:\"accepted\""));
    }

    #[cfg(feature = "worldgen")]
    #[test]
    fn append_event_writes_world_compat_startup_reject_action() {
        let path = unique_temp_path();

        append_event(
            &path,
            AuditSource::Runtime,
            AuditAction::WorldCompatStartupReject,
            AuditOutcome::Failed,
            "failed to create server instance: World Error: compat enforce rejected load: \
             entry=load, decision=fallback_generate, failure=parse_error; world compat audit: \
             entry=load, decision=fallback_generate, failure=parse_error",
        )
        .expect("audit append should succeed");

        let line = fs::read_to_string(&path).expect("audit log should be readable");

        let _ = fs::remove_dir_all(path.parent().expect("audit path should have parent"));

        assert!(line.contains("source:\"runtime\""));
        assert!(line.contains("action:\"world-compat-startup-reject\""));
        assert!(line.contains("outcome:\"failed\""));
    }

    #[test]
    fn append_event_writes_runtime_listener_failure_action() {
        let path = unique_temp_path();

        append_event(
            &path,
            AuditSource::Runtime,
            AuditAction::RuntimeListenerFailure,
            AuditOutcome::Failed,
            "runtime listener query-server at 0.0.0.0:14006 entered stopped-unexpectedly after \
             startup: query server stopped unexpectedly after startup",
        )
        .expect("audit append should succeed");

        let line = fs::read_to_string(&path).expect("audit log should be readable");

        let _ = fs::remove_dir_all(path.parent().expect("audit path should have parent"));

        assert!(line.contains("source:\"runtime\""));
        assert!(line.contains("action:\"runtime-listener-failure\""));
        assert!(line.contains("outcome:\"failed\""));
    }

    #[test]
    fn append_event_writes_web_runtime_failure_action() {
        let path = unique_temp_path();

        append_event(
            &path,
            AuditSource::Runtime,
            AuditAction::WebRuntimeFailure,
            AuditOutcome::Failed,
            "web listener 127.0.0.1:14005 stopped unexpectedly after startup: broken pipe",
        )
        .expect("audit append should succeed");

        let line = fs::read_to_string(&path).expect("audit log should be readable");

        let _ = fs::remove_dir_all(path.parent().expect("audit path should have parent"));

        assert!(line.contains("source:\"runtime\""));
        assert!(line.contains("action:\"web-runtime-failure\""));
        assert!(line.contains("outcome:\"failed\""));
    }

    #[test]
    fn retention_policy_rotates_oversized_active_file() {
        let path = unique_temp_path();
        fs::create_dir_all(path.parent().expect("audit path should have parent"))
            .expect("should create audit parent dir");
        fs::write(&path, vec![b'x'; 1024 * 1024 + 1]).expect("should seed active audit file");

        let report = apply_retention_policy(&path, AuditRetentionPolicy {
            max_active_file_mebibytes: 1,
            max_archive_files: 7,
        })
        .expect("retention policy should succeed");

        let archives = archive_paths(&path).expect("archive listing should succeed");

        let _ = fs::remove_dir_all(path.parent().expect("audit path should have parent"));

        assert_eq!(report.active_file_size_bytes, 0);
        assert!(report.rotated_to.is_some());
        assert_eq!(archives.len(), 1);
        assert!(!path.exists());
    }

    #[test]
    fn retention_policy_prunes_old_archives_beyond_limit() {
        let path = unique_temp_path();
        let parent = path.parent().expect("audit path should have parent");
        fs::create_dir_all(parent).expect("should create audit parent dir");
        fs::write(parent.join("audit-log.20260101T000000Z.ronl"), b"1")
            .expect("should write archive 1");
        fs::write(parent.join("audit-log.20260102T000000Z.ronl"), b"2")
            .expect("should write archive 2");
        fs::write(parent.join("audit-log.20260103T000000Z.ronl"), b"3")
            .expect("should write archive 3");

        let report = apply_retention_policy(&path, AuditRetentionPolicy {
            max_active_file_mebibytes: 32,
            max_archive_files: 2,
        })
        .expect("retention policy should succeed");

        let archives = archive_paths(&path).expect("archive listing should succeed");

        let _ = fs::remove_dir_all(parent);

        assert_eq!(report.deleted_archives.len(), 1);
        assert_eq!(report.retained_archives, 2);
        assert_eq!(archives.len(), 2);
        assert!(report.deleted_archives[0].ends_with("audit-log.20260101T000000Z.ronl"));
    }
}
