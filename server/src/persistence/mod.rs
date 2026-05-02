//! DB operations and schema migrations

// Touch this comment if changes only include .sql files and no .rs so that
// migration happens.
// nya~

pub(in crate::persistence) mod character;
pub mod character_loader;
pub mod character_updater;
mod diesel_to_rusqlite;
pub mod error;
mod json_models;
mod models;

use crate::persistence::character_updater::PetPersistenceData;
use common::comp;
use refinery::Report;
use rusqlite::{
    Connection, OpenFlags, OptionalExtension,
    trace::{TraceEvent, TraceEventCodes},
};
use std::{
    fs,
    ops::Deref,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};
use tracing::info;

// re-export waypoint parser for use to look up location names in character list
pub(crate) use character::parse_waypoint;

/// A struct of the components that are persisted to the DB for each character
#[derive(Debug)]
pub struct PersistedComponents {
    pub body: comp::Body,
    pub hardcore: Option<comp::Hardcore>,
    pub stats: comp::Stats,
    pub skill_set: comp::SkillSet,
    pub inventory: comp::Inventory,
    pub waypoint: Option<comp::Waypoint>,
    pub pets: Vec<PetPersistenceData>,
    pub active_abilities: comp::ActiveAbilities,
    pub map_marker: Option<comp::MapMarker>,
}

pub type EditableComponents = (comp::Body,);

// See: https://docs.rs/refinery/0.5.0/refinery/macro.embed_migrations.html
// This macro is called at build-time, and produces the necessary migration info
// for the `run_migrations` call below.
mod embedded {
    use refinery::embed_migrations;
    embed_migrations!("./src/migrations");
}

/// A database connection blessed by the project runtime.
pub(crate) struct VeldrConnection {
    connection: Connection,
    sql_log_mode: SqlLogMode,
}

impl VeldrConnection {
    fn new(connection: Connection) -> Self {
        Self {
            connection,
            sql_log_mode: SqlLogMode::Disabled,
        }
    }

    /// Updates the SQLite log mode if DatabaseSetting.sql_log_mode has changed
    pub fn update_log_mode(&mut self, database_settings: &Arc<RwLock<DatabaseSettings>>) {
        let settings = database_settings
            .read()
            .expect("DatabaseSettings RwLock was poisoned");
        if self.sql_log_mode == settings.sql_log_mode {
            return;
        }

        set_log_mode(&mut self.connection, settings.sql_log_mode);
        self.sql_log_mode = settings.sql_log_mode;

        info!(
            "SQL log mode for connection changed to {:?}",
            settings.sql_log_mode
        );
    }
}

impl Deref for VeldrConnection {
    type Target = Connection;

    fn deref(&self) -> &Connection { &self.connection }
}

fn set_log_mode(connection: &mut Connection, sql_log_mode: SqlLogMode) {
    match sql_log_mode {
        SqlLogMode::Trace => {
            connection.trace_v2(
                TraceEventCodes::SQLITE_TRACE_STMT,
                Some(rusqlite_trace_callback),
            );
        },
        SqlLogMode::Profile => {
            connection.trace_v2(
                TraceEventCodes::SQLITE_TRACE_PROFILE,
                Some(rusqlite_trace_callback),
            );
        },
        SqlLogMode::Disabled => {
            connection.trace_v2(TraceEventCodes::empty(), None);
        },
    };
}

#[derive(Clone)]
pub struct DatabaseSettings {
    pub db_dir: PathBuf,
    pub sql_log_mode: SqlLogMode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DatabaseFileProbe {
    Ready {
        path: PathBuf,
        representative_table: String,
    },
    Missing {
        path: PathBuf,
    },
    Unreadable {
        path: PathBuf,
        message: String,
    },
    Invalid {
        path: PathBuf,
        message: String,
    },
    Uninitialized {
        path: PathBuf,
    },
}

impl DatabaseFileProbe {
    pub fn is_ready(&self) -> bool { matches!(self, Self::Ready { .. }) }

    pub fn detail(&self) -> String {
        match self {
            Self::Ready {
                path,
                representative_table,
            } => format!(
                "server database file is present and openable at {} with application table {}",
                path.display(),
                representative_table
            ),
            Self::Missing { path } => {
                format!("server database file missing at {}", path.display())
            },
            Self::Unreadable { path, message } => format!(
                "server database file could not be read at {}: {}",
                path.display(),
                message
            ),
            Self::Invalid { path, message } => format!(
                "server database file is not a readable SQLite database at {}: {}",
                path.display(),
                message
            ),
            Self::Uninitialized { path } => format!(
                "server database file is openable at {} but exposes no application tables",
                path.display()
            ),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ConnectionMode {
    ReadOnly,
    ReadWrite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SqlLogMode {
    /// Logging is disabled
    #[default]
    Disabled,
    /// Records timings for each SQL statement
    Profile,
    /// Prints all executed SQL statements
    Trace,
}

impl SqlLogMode {
    pub fn variants() -> [&'static str; 3] { ["disabled", "profile", "trace"] }
}

pub fn inspect_database_file(path: &Path) -> DatabaseFileProbe {
    let path = path.to_path_buf();

    match fs::File::open(&path) {
        Ok(_) => {},
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return DatabaseFileProbe::Missing { path };
        },
        Err(error) => {
            return DatabaseFileProbe::Unreadable {
                path,
                message: error.to_string(),
            };
        },
    }

    let open_flags = OpenFlags::SQLITE_OPEN_PRIVATE_CACHE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_READ_ONLY;
    let connection = match Connection::open_with_flags(&path, open_flags) {
        Ok(connection) => connection,
        Err(error) => {
            return DatabaseFileProbe::Invalid {
                path,
                message: error.to_string(),
            };
        },
    };

    match connection
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
             LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
    {
        Ok(Some(table)) => DatabaseFileProbe::Ready {
            path,
            representative_table: table,
        },
        Ok(None) => DatabaseFileProbe::Uninitialized { path },
        Err(error) => DatabaseFileProbe::Invalid {
            path,
            message: error.to_string(),
        },
    }
}

impl core::str::FromStr for SqlLogMode {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "disabled" => Ok(Self::Disabled),
            "profile" => Ok(Self::Profile),
            "trace" => Ok(Self::Trace),
            _ => Err("Could not parse SqlLogMode"),
        }
    }
}

#[expect(clippy::to_string_trait_impl)]
impl ToString for SqlLogMode {
    fn to_string(&self) -> String {
        match self {
            SqlLogMode::Disabled => "disabled",
            SqlLogMode::Profile => "profile",
            SqlLogMode::Trace => "trace",
        }
        .into()
    }
}

/// Runs any pending database migrations. This is executed during server startup
pub fn run_migrations(settings: &DatabaseSettings) {
    let mut conn = establish_connection(settings, ConnectionMode::ReadWrite);

    diesel_to_rusqlite::migrate_from_diesel(&mut conn)
        .expect("One-time migration from Diesel to Refinery failed");

    // If migrations fail to run, the server cannot start since the database will
    // not be in the required state.
    let report: Report = embedded::migrations::runner()
        .set_abort_divergent(false)
        .run(&mut conn.connection)
        .expect("Database migrations failed, server startup aborted");

    let applied_migrations = report.applied_migrations().len();
    info!("Applied {} database migrations", applied_migrations);
}

/// Runs after the migrations. In some cases, it can reclaim a significant
/// amount of space (reported 30%)
pub fn vacuum_database(settings: &DatabaseSettings) {
    let conn = establish_connection(settings, ConnectionMode::ReadWrite);

    conn.execute("VACUUM main", [])
        .expect("Database vacuuming failed, server startup aborted");

    info!("Database vacuumed");
}

// This callback uses info logging because it is never enabled by default,
// only when explicitly turned on via CLI arguments or interactive CLI commands.
// Setting it to anything other than info would remove the ability to get SQL
// logging from a running server that wasn't started at higher than info.
fn rusqlite_trace_callback(event: TraceEvent<'_>) {
    match event {
        TraceEvent::Stmt(_, msg) => info!("{}", msg),
        TraceEvent::Profile(stmt, dur) => info!("{} Duration: {:?}", stmt.sql(), dur),
        _ => (),
    }
}

pub(crate) fn establish_connection(
    settings: &DatabaseSettings,
    connection_mode: ConnectionMode,
) -> VeldrConnection {
    fs::create_dir_all(&settings.db_dir)
        .unwrap_or_else(|_| panic!("Failed to create saves directory: {:?}", &settings.db_dir));

    let open_flags = OpenFlags::SQLITE_OPEN_PRIVATE_CACHE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | match connection_mode {
            ConnectionMode::ReadWrite => {
                OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_READ_WRITE
            },
            ConnectionMode::ReadOnly => OpenFlags::SQLITE_OPEN_READ_ONLY,
        };

    let connection = Connection::open_with_flags(settings.db_dir.join("db.sqlite"), open_flags)
        .unwrap_or_else(|err| {
            panic!(
                "Error connecting to {}, Error: {:?}",
                settings.db_dir.join("db.sqlite").display(),
                err
            )
        });

    let mut veldr_connection = VeldrConnection::new(connection);

    let connection = &mut veldr_connection.connection;

    set_log_mode(connection, settings.sql_log_mode);
    veldr_connection.sql_log_mode = settings.sql_log_mode;

    rusqlite::vtab::array::load_module(connection).expect("Failed to load sqlite array module");

    connection.set_prepared_statement_cache_capacity(100);

    // Use Write-Ahead-Logging for improved concurrency: https://sqlite.org/wal.html
    // Set a busy timeout (in ms): https://sqlite.org/c3ref/busy_timeout.html
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .expect("Failed to set foreign_keys PRAGMA");
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .expect("Failed to set journal_mode PRAGMA");
    connection
        .pragma_update(None, "busy_timeout", "250")
        .expect("Failed to set busy_timeout PRAGMA");

    veldr_connection
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("caldrayne-{label}-{unique}"))
    }

    #[test]
    fn inspect_database_file_reports_missing_invalid_and_ready_states() {
        let dir = unique_temp_dir("database-probe");
        let database_file = dir.join("db.sqlite");

        let missing = inspect_database_file(&database_file);
        assert!(matches!(missing, DatabaseFileProbe::Missing { .. }));

        fs::create_dir_all(&dir).expect("should create temp db dir");
        fs::write(&database_file, b"not a sqlite database").expect("should write invalid db file");
        let invalid = inspect_database_file(&database_file);
        assert!(matches!(invalid, DatabaseFileProbe::Invalid { .. }));

        fs::remove_file(&database_file).expect("should remove invalid db file");
        Connection::open(&database_file).expect("should create empty sqlite database");
        let uninitialized = inspect_database_file(&database_file);
        assert!(matches!(
            uninitialized,
            DatabaseFileProbe::Uninitialized { .. }
        ));

        run_migrations(&DatabaseSettings {
            db_dir: dir.clone(),
            sql_log_mode: SqlLogMode::Disabled,
        });
        let ready = inspect_database_file(&database_file);
        assert!(matches!(ready, DatabaseFileProbe::Ready { .. }));

        let _ = fs::remove_dir_all(dir);
    }
}
