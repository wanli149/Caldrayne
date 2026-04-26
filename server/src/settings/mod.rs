mod account_auth;
pub mod admin;
pub mod banlist;
mod editable;
mod identity;
pub mod server_description;
pub mod server_physics;
pub mod whitelist;

pub use account_auth::{
    AccountAuthGovernanceReport, AccountAuthRuntimeTopologyReport, AccountAuthStartupPolicyReport,
    AccountAuthTopologyError, AccountIdentityScopeReport, AccountNamespacePolicyReport,
    AccountPrincipalDefinitionReport, UnsupportedAccountAuthTopologyReport,
};
pub use editable::{EditableSetting, Error as SettingError};
pub use identity::{IdentityFileProbe, ServerIdentity, identity_file_path, inspect_identity_file};

pub use admin::{AdminRecord, Admins};
pub use banlist::{
    Ban, BanEntry, BanError, BanErrorKind, BanInfo, BanKind, BanOperation, BanOperationError,
    BanRecord, Banlist,
};
pub use server_description::ServerDescriptions;
pub use whitelist::{Whitelist, WhitelistInfo, WhitelistRecord};

use chrono::Utc;
use common::{
    calendar::{Calendar, CalendarEvent},
    consts::DAY_LENGTH_DEFAULT,
    resources::BattleMode,
    rtsim::WorldSettings,
};
use core::time::Duration;
use portpicker::pick_unused_port;
use serde::{Deserialize, Serialize};
use std::{
    fmt::Display,
    fs, io,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    path::{Path, PathBuf},
};
use tracing::{error, warn};
use world::sim::{CompatMode, DEFAULT_WORLD_SEED, FileOpts};

use self::server_physics::ServerPhysicsForceList;
use crate::data_dir;

const SETTINGS_FILENAME: &str = "settings.ron";
const WHITELIST_FILENAME: &str = "whitelist.ron";
const BANLIST_FILENAME: &str = "banlist.ron";
const SERVER_DESCRIPTION_FILENAME: &str = "description.ron";
const ADMINS_FILENAME: &str = "admins.ron";
const SERVER_PHYSICS_FORCE_FILENAME: &str = "server_physics_force.ron";

pub const SINGLEPLAYER_SERVER_NAME: &str = "Singleplayer";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsFileProbe {
    Ready { path: PathBuf },
    Missing { path: PathBuf },
    Unreadable { path: PathBuf, message: String },
    Invalid { path: PathBuf, message: String },
}

impl SettingsFileProbe {
    pub fn is_ready(&self) -> bool { matches!(self, Self::Ready { .. }) }

    pub fn detail(&self) -> String {
        match self {
            Self::Ready { path } => {
                format!(
                    "server settings file is present and parseable at {}",
                    path.display()
                )
            },
            Self::Missing { path } => {
                format!("server settings file missing at {}", path.display())
            },
            Self::Unreadable { path, message } => {
                format!(
                    "server settings file could not be read at {}: {}",
                    path.display(),
                    message
                )
            },
            Self::Invalid { path, message } => {
                format!(
                    "server settings file is not valid RON at {}: {}",
                    path.display(),
                    message
                )
            },
        }
    }
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize)]
pub enum ServerBattleMode {
    Global(BattleMode),
    PerPlayer { default: BattleMode },
}

impl Default for ServerBattleMode {
    fn default() -> Self { Self::Global(BattleMode::PvP) }
}

impl ServerBattleMode {
    pub fn allow_choosing(&self) -> bool {
        match self {
            ServerBattleMode::Global { .. } => false,
            ServerBattleMode::PerPlayer { .. } => true,
        }
    }

    pub fn default_mode(&self) -> BattleMode {
        match self {
            ServerBattleMode::Global(mode) => *mode,
            ServerBattleMode::PerPlayer { default: mode } => *mode,
        }
    }
}

impl From<ServerBattleMode> for veloren_query_server::proto::ServerBattleMode {
    fn from(value: ServerBattleMode) -> Self {
        use veloren_query_server::proto::ServerBattleMode as QueryBattleMode;

        match value {
            ServerBattleMode::Global(mode) => match mode {
                BattleMode::PvP => QueryBattleMode::GlobalPvP,
                BattleMode::PvE => QueryBattleMode::GlobalPvE,
            },
            ServerBattleMode::PerPlayer { .. } => QueryBattleMode::PerPlayer,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeEnvironment {
    #[default]
    Local,
    Test,
    Production,
}

impl From<RuntimeEnvironment> for veloren_query_server::proto::ServerEnvironment {
    fn from(value: RuntimeEnvironment) -> Self {
        use veloren_query_server::proto::ServerEnvironment;

        match value {
            RuntimeEnvironment::Local => ServerEnvironment::Local,
            RuntimeEnvironment::Test => ServerEnvironment::Test,
            RuntimeEnvironment::Production => ServerEnvironment::Production,
        }
    }
}

impl From<RuntimeEnvironment> for common_net::msg::ServerEnvironment {
    fn from(value: RuntimeEnvironment) -> Self {
        use common_net::msg::ServerEnvironment;

        match value {
            RuntimeEnvironment::Local => ServerEnvironment::Local,
            RuntimeEnvironment::Test => ServerEnvironment::Test,
            RuntimeEnvironment::Production => ServerEnvironment::Production,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Protocol {
    Quic {
        address: SocketAddr,
        cert_file_path: PathBuf,
        key_file_path: PathBuf,
    },
    Tcp {
        address: SocketAddr,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuicBinding {
    pub address: SocketAddr,
    pub cert_file_path: PathBuf,
    pub key_file_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuicTlsConfigError {
    Io {
        path: PathBuf,
        message: String,
    },
    InvalidPrivateKey {
        path: PathBuf,
    },
    InvalidCertificateChain {
        path: PathBuf,
    },
    InvalidTlsConfig {
        cert_file_path: PathBuf,
        key_file_path: PathBuf,
        message: String,
    },
}

impl Display for QuicTlsConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, message } => {
                write!(f, "I/O error while reading {}: {message}", path.display())
            },
            Self::InvalidPrivateKey { path } => {
                write!(f, "No valid TLS private key found in {}", path.display())
            },
            Self::InvalidCertificateChain { path } => {
                write!(
                    f,
                    "No valid TLS certificate chain found in {}",
                    path.display()
                )
            },
            Self::InvalidTlsConfig {
                cert_file_path,
                key_file_path,
                message,
            } => write!(
                f,
                "Failed to build QUIC/TLS config from cert {} and key {}: {message}",
                cert_file_path.display(),
                key_file_path.display()
            ),
        }
    }
}

impl std::error::Error for QuicTlsConfigError {}

impl Protocol {
    pub fn address(&self) -> SocketAddr {
        match self {
            Self::Quic { address, .. } | Self::Tcp { address } => *address,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Quic { .. } => "quic",
            Self::Tcp { .. } => "tcp",
        }
    }

    pub fn quic_binding(&self) -> Option<QuicBinding> {
        match self {
            Self::Quic {
                address,
                cert_file_path,
                key_file_path,
            } => Some(QuicBinding {
                address: *address,
                cert_file_path: cert_file_path.clone(),
                key_file_path: key_file_path.clone(),
            }),
            Self::Tcp { .. } => None,
        }
    }
}

impl QuicBinding {
    pub fn from_protocols(protocols: &[Protocol]) -> Vec<Self> {
        protocols
            .iter()
            .filter_map(Protocol::quic_binding)
            .collect()
    }

    pub fn validate_tls_material(&self) -> Result<(), QuicTlsConfigError> {
        self.load_server_config().map(|_| ())
    }

    pub fn load_server_config(&self) -> Result<quinn::ServerConfig, QuicTlsConfigError> {
        use quinn::rustls::pki_types::{CertificateDer, PrivateKeyDer};
        use rustls_pemfile::Item;

        let key = fs::read(&self.key_file_path).map_err(|error| QuicTlsConfigError::Io {
            path: self.key_file_path.clone(),
            message: error.to_string(),
        })?;
        let key = if self
            .key_file_path
            .extension()
            .is_some_and(|extension| extension == "der")
        {
            PrivateKeyDer::try_from(key).map_err(|_| QuicTlsConfigError::InvalidPrivateKey {
                path: self.key_file_path.clone(),
            })?
        } else {
            rustls_pemfile::read_all(&mut key.as_slice())
                .find_map(|item| match item {
                    Ok(Item::Pkcs1Key(v)) => Some(Ok(PrivateKeyDer::Pkcs1(v))),
                    Ok(Item::Pkcs8Key(v)) => Some(Ok(PrivateKeyDer::Pkcs8(v))),
                    Ok(Item::Sec1Key(v)) => Some(Ok(PrivateKeyDer::Sec1(v))),
                    Ok(Item::Crl(_)) | Ok(Item::Csr(_)) | Ok(Item::X509Certificate(_)) | Ok(_) => {
                        None
                    },
                    Err(error) => Some(Err(QuicTlsConfigError::Io {
                        path: self.key_file_path.clone(),
                        message: error.to_string(),
                    })),
                })
                .transpose()?
                .ok_or_else(|| QuicTlsConfigError::InvalidPrivateKey {
                    path: self.key_file_path.clone(),
                })?
        };

        let cert_chain =
            fs::read(&self.cert_file_path).map_err(|error| QuicTlsConfigError::Io {
                path: self.cert_file_path.clone(),
                message: error.to_string(),
            })?;
        let cert_chain = if self
            .cert_file_path
            .extension()
            .is_some_and(|extension| extension == "der")
        {
            vec![CertificateDer::from(cert_chain)]
        } else {
            let certs = rustls_pemfile::certs(&mut cert_chain.as_slice())
                .collect::<Result<Vec<_>, io::Error>>()
                .map_err(|error| QuicTlsConfigError::Io {
                    path: self.cert_file_path.clone(),
                    message: error.to_string(),
                })?;
            if certs.is_empty() {
                return Err(QuicTlsConfigError::InvalidCertificateChain {
                    path: self.cert_file_path.clone(),
                });
            }
            certs
        };

        quinn::ServerConfig::with_single_cert(cert_chain, key).map_err(|error| {
            QuicTlsConfigError::InvalidTlsConfig {
                cert_file_path: self.cert_file_path.clone(),
                key_file_path: self.key_file_path.clone(),
                message: error.to_string(),
            }
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameplaySettings {
    #[serde(default)]
    pub battle_mode: ServerBattleMode,
    #[serde(default)]
    // explosion_burn_marks by players
    pub explosion_burn_marks: bool,
}

impl Default for GameplaySettings {
    fn default() -> Self {
        Self {
            battle_mode: ServerBattleMode::default(),
            explosion_burn_marks: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModerationSettings {
    #[serde(default)]
    pub banned_words_files: Vec<PathBuf>,
    #[serde(default)]
    pub automod: bool,
    #[serde(default)]
    pub admins_exempt: bool,
}

impl ModerationSettings {
    pub fn load_banned_words(&self, data_dir: &Path) -> Vec<String> {
        let mut banned_words = Vec::new();
        for fname in self.banned_words_files.iter() {
            let mut path = with_config_dir(data_dir);
            path.push(fname);
            match std::fs::File::open(&path) {
                Ok(file) => match ron::de::from_reader(&file) {
                    Ok(mut words) => banned_words.append(&mut words),
                    Err(error) => error!(?error, ?file, "Couldn't read banned words file"),
                },
                Err(error) => error!(?error, ?path, "Couldn't open banned words file"),
            }
        }
        banned_words
    }
}

impl Default for ModerationSettings {
    fn default() -> Self {
        Self {
            banned_words_files: Vec::new(),
            automod: false,
            admins_exempt: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub enum CalendarMode {
    None,
    #[default]
    Auto,
    Timezone(chrono_tz::Tz),
    Events(Vec<CalendarEvent>),
}

impl CalendarMode {
    pub fn calendar_now(&self) -> Calendar {
        match self {
            CalendarMode::None => Calendar::default(),
            CalendarMode::Auto => Calendar::from_tz(None),
            CalendarMode::Timezone(tz) => Calendar::from_tz(Some(*tz)),
            CalendarMode::Events(events) => Calendar::from_events(events.clone()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub gameserver_protocols: Vec<Protocol>,
    pub auth_server_address: Option<String>,
    pub query_address: Option<SocketAddr>,
    #[serde(default, skip_serializing)]
    pub runtime_environment: RuntimeEnvironment,
    pub max_players: u16,
    pub world_seed: u32,
    pub server_name: String,
    /// Length of a day in minutes.
    pub day_length: f64,
    /// When set to None, loads the default map file (if available); otherwise,
    /// uses the value of the file options to decide how to proceed.
    pub map_file: Option<FileOpts>,
    /// Controls whether strict world load fallback remains advisory (`record`)
    /// or becomes a fail-fast startup contract (`enforce`).
    pub world_compat_mode: CompatMode,
    pub max_view_distance: Option<u32>,
    pub max_player_group_size: u32,
    pub client_timeout: Duration,
    pub max_player_for_kill_broadcast: Option<usize>,
    pub calendar_mode: CalendarMode,

    /// Experimental feature. No guaranteed forwards-compatibility, may be
    /// removed at *any time* with no migration.
    #[serde(default, skip_serializing)]
    pub experimental_terrain_persistence: bool,

    #[serde(default)]
    pub gameplay: GameplaySettings,
    #[serde(default)]
    pub moderation: ModerationSettings,

    #[serde(default)]
    pub world: WorldSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            gameserver_protocols: vec![
                Protocol::Tcp {
                    address: SocketAddr::from((Ipv6Addr::UNSPECIFIED, 14004)),
                },
                Protocol::Tcp {
                    address: SocketAddr::from((Ipv4Addr::UNSPECIFIED, 14004)),
                },
            ],
            auth_server_address: None,
            query_address: Some(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 14006))),
            runtime_environment: RuntimeEnvironment::Local,
            world_seed: DEFAULT_WORLD_SEED,
            server_name: "Caldrayne Online Server".into(),
            max_players: 100,
            day_length: DAY_LENGTH_DEFAULT,
            map_file: None,
            world_compat_mode: CompatMode::Record,
            max_view_distance: Some(65),
            max_player_group_size: 6,
            calendar_mode: CalendarMode::Auto,
            client_timeout: Duration::from_secs(40),
            max_player_for_kill_broadcast: None,
            experimental_terrain_persistence: false,
            gameplay: GameplaySettings::default(),
            moderation: ModerationSettings::default(),
            world: WorldSettings::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Settings;
    use world::sim::CompatMode;

    #[test]
    fn legacy_settings_without_world_compat_mode_default_to_record() {
        let ron = "(world_seed: 1337)";

        let settings: Settings = ron::from_str(ron).expect("legacy settings should deserialize");

        assert_eq!(settings.world_compat_mode, CompatMode::Record);
    }
}

impl Settings {
    pub fn server_auth_mode(&self) -> common_net::msg::ServerAuthMode {
        if self.auth_server_address.is_some() {
            common_net::msg::ServerAuthMode::ExternalProvider
        } else {
            common_net::msg::ServerAuthMode::NoExternalAuth
        }
    }

    pub fn server_auth(&self) -> common_net::msg::ServerAuth {
        match &self.auth_server_address {
            Some(provider_url) => common_net::msg::ServerAuth::External {
                provider_url: provider_url.clone(),
            },
            None => common_net::msg::ServerAuth::None,
        }
    }

    /// path: Directory that contains the server config directory
    pub fn load(path: &Path) -> Self {
        let path = settings_file_path(path);

        let mut settings = if let Ok(file) = fs::File::open(&path) {
            match ron::de::from_reader(file) {
                Ok(x) => x,
                Err(e) => {
                    let default_settings = Self::default();
                    let template_path = path.with_extension("template.ron");
                    warn!(
                        ?e,
                        "Failed to parse setting file! Falling back to default settings and \
                         creating a template file for you to migrate your current settings file: \
                         {}",
                        template_path.display()
                    );
                    if let Err(e) = default_settings.save_to_file(&template_path) {
                        error!(?e, "Failed to create template settings file")
                    }
                    default_settings
                },
            }
        } else {
            let default_settings = Self::default();

            if let Err(e) = default_settings.save_to_file(&path) {
                error!(?e, "Failed to create default settings file!");
            }
            default_settings
        };

        settings.validate();
        settings
    }

    fn save_to_file(&self, path: &Path) -> std::io::Result<()> {
        // Create dir if it doesn't exist
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let ron = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .expect("Failed serialize settings.");

        fs::write(path, ron.as_bytes())
    }

    /// path: Directory that contains the server config directory
    pub fn singleplayer(path: &Path) -> Self {
        let load = Self::load(path);
        Self {
            // BUG: theoretically another process can grab the port between here and server
            // creation, however the time window is quite small.
            gameserver_protocols: vec![Protocol::Tcp {
                address: SocketAddr::from((
                    Ipv4Addr::LOCALHOST,
                    pick_unused_port().expect("Failed to find unused port!"),
                )),
            }],
            auth_server_address: None,
            // If loading the default map file, make sure the seed is also default.
            world_seed: if load.map_file.is_some() {
                load.world_seed
            } else {
                DEFAULT_WORLD_SEED
            },
            server_name: SINGLEPLAYER_SERVER_NAME.to_owned(),
            max_players: 100,
            max_view_distance: None,
            client_timeout: Duration::from_secs(180),
            ..load // Fill in remaining fields from server_settings.ron.
        }
    }

    fn validate(&mut self) {
        const INVALID_SETTING_MSG: &str =
            "Invalid value for setting in userdata/server/server_config/settings.ron.";

        let default_values = Settings::default();

        if self.day_length <= 0.0 {
            warn!(
                "{} Setting: day_length, Value: {}. Set day_length to it's default value of {}. \
                 Help: day_length must be a positive floating point value above 0.",
                INVALID_SETTING_MSG, self.day_length, default_values.day_length
            );
            self.day_length = default_values.day_length;
        }
    }

    /// Derive a coefficient that is the relatively speed of the in-game
    /// day/night cycle compared to reality.
    pub fn day_cycle_coefficient(&self) -> f64 { 1440.0 / self.day_length }
}

pub enum InvalidSettingsError {
    InvalidDayDuration,
}
impl Display for InvalidSettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InvalidSettingsError::InvalidDayDuration => {
                f.write_str("Invalid settings error: Day length was invalid (zero or negative).")
            },
        }
    }
}

pub fn settings_file_path(path: &Path) -> PathBuf {
    let mut path = with_config_dir(path);
    path.push(SETTINGS_FILENAME);
    path
}

pub fn inspect_settings_file(path: &Path) -> SettingsFileProbe {
    let settings_path = settings_file_path(path);

    match fs::File::open(&settings_path) {
        Ok(file) => match ron::de::from_reader::<_, Settings>(file) {
            Ok(_) => SettingsFileProbe::Ready {
                path: settings_path,
            },
            Err(error) => SettingsFileProbe::Invalid {
                path: settings_path,
                message: error.to_string(),
            },
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => SettingsFileProbe::Missing {
            path: settings_path,
        },
        Err(error) => SettingsFileProbe::Unreadable {
            path: settings_path,
            message: error.to_string(),
        },
    }
}

pub fn with_config_dir(path: &Path) -> PathBuf { data_dir::with_config_dir(path) }

/// Our upgrade guarantee is that if validation succeeds
/// for an old version, then migration to the next version must always succeed
/// and produce a valid settings file for that version (if we need to change
/// this in the future, it should require careful discussion).  Therefore, we
/// would normally panic if the upgrade produced an invalid settings file, which
/// we would perform by doing the following post-validation (example
/// is given for a hypothetical upgrade from Whitelist_V1 to Whitelist_V2):
///
/// Ok(Whitelist_V2::try_into().expect())
const MIGRATION_UPGRADE_GUARANTEE: &str = "Any valid file of an old verison should be able to \
                                           successfully migrate to the latest version.";

/// Combines all the editable settings into one struct that is stored in the ecs
#[derive(Clone)]
pub struct EditableSettings {
    pub whitelist: Whitelist,
    pub banlist: Banlist,
    pub server_description: ServerDescriptions,
    pub admins: Admins,
    pub server_physics_force_list: ServerPhysicsForceList,
}

impl EditableSettings {
    pub fn load(data_dir: &Path) -> Self {
        Self {
            whitelist: Whitelist::load(data_dir),
            banlist: Banlist::load(data_dir),
            server_description: ServerDescriptions::load(data_dir),
            admins: Admins::load(data_dir),
            server_physics_force_list: ServerPhysicsForceList::load(data_dir),
        }
    }

    pub fn singleplayer(data_dir: &Path) -> Self {
        let load = Self::load(data_dir);

        let server_description = ServerDescriptions::default();

        let mut admins = Admins::default();
        // TODO: Let the player choose if they want to use admin commands or not
        admins.insert(
            crate::login_provider::derive_singleplayer_uuid(),
            AdminRecord {
                username_when_admined: Some("singleplayer".into()),
                date: Utc::now(),
                role: admin::Role::Admin,
            },
        );

        Self {
            server_description,
            admins,
            ..load
        }
    }
}
