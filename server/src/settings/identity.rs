use crate::ServerStatePaths;
use common::uuid::Uuid;
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use tracing::warn;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentityFileProbe {
    Ready { path: PathBuf, realm_id: Uuid },
    Missing { path: PathBuf },
    Unreadable { path: PathBuf, message: String },
    Invalid { path: PathBuf, message: String },
}

impl IdentityFileProbe {
    pub fn is_ready(&self) -> bool { matches!(self, Self::Ready { .. }) }

    pub fn detail(&self) -> String {
        match self {
            Self::Ready { path, realm_id } => format!(
                "server identity file is present and parseable at {} with realm_id {}",
                path.display(),
                realm_id
            ),
            Self::Missing { path } => {
                format!("server identity file missing at {}", path.display())
            },
            Self::Unreadable { path, message } => format!(
                "server identity file could not be read at {}: {}",
                path.display(),
                message
            ),
            Self::Invalid { path, message } => format!(
                "server identity file is not valid RON at {}: {}",
                path.display(),
                message
            ),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ServerIdentity {
    pub realm_id: Uuid,
}

impl Default for ServerIdentity {
    fn default() -> Self { Self::generate() }
}

impl ServerIdentity {
    pub fn generate() -> Self {
        Self {
            realm_id: Uuid::new_v4(),
        }
    }

    pub fn from_realm_id(realm_id: Uuid) -> Self { Self { realm_id } }

    pub fn load(data_dir: &Path) -> Self {
        let path = identity_file_path(data_dir);
        let identity = if let Ok(file) = fs::File::open(&path) {
            match ron::de::from_reader(file) {
                Ok(identity) => identity,
                Err(error) => {
                    warn!(
                        ?error,
                        path = ?path,
                        "Failed to parse server identity, regenerating a new realm id"
                    );
                    Self::generate()
                },
            }
        } else {
            Self::generate()
        };

        identity.save_to_data_dir_warn(data_dir);
        identity
    }

    pub fn save_to_data_dir_warn(&self, data_dir: &Path) {
        if let Err(error) = self.save_to_file(data_dir) {
            warn!(?error, "Failed to save server identity");
        }
    }

    fn save_to_file(&self, data_dir: &Path) -> std::io::Result<()> {
        let path = identity_file_path(data_dir);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }

        let ron = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).unwrap();
        fs::write(path, ron.as_bytes())
    }
}

pub fn identity_file_path(data_dir: &Path) -> PathBuf {
    ServerStatePaths::new(data_dir).identity_file
}

pub fn inspect_identity_file(data_dir: &Path) -> IdentityFileProbe {
    let path = identity_file_path(data_dir);

    match fs::File::open(&path) {
        Ok(file) => match ron::de::from_reader::<_, ServerIdentity>(file) {
            Ok(identity) => IdentityFileProbe::Ready {
                path,
                realm_id: identity.realm_id,
            },
            Err(error) => IdentityFileProbe::Invalid {
                path,
                message: error.to_string(),
            },
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            IdentityFileProbe::Missing { path }
        },
        Err(error) => IdentityFileProbe::Unreadable {
            path,
            message: error.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_creates_and_reuses_realm_identity() {
        let dir =
            std::env::temp_dir().join(format!("caldrayne-server-identity-{}", Uuid::new_v4()));

        let first = ServerIdentity::load(&dir);
        let second = ServerIdentity::load(&dir);

        assert_eq!(first.realm_id, second.realm_id);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn inspect_identity_file_reports_missing_invalid_and_ready_states() {
        let dir = std::env::temp_dir().join(format!(
            "caldrayne-server-identity-probe-{}",
            Uuid::new_v4()
        ));

        let missing = inspect_identity_file(&dir);
        assert!(matches!(missing, IdentityFileProbe::Missing { .. }));

        fs::create_dir_all(&dir).expect("should create temp dir");
        fs::write(identity_file_path(&dir), b"not valid ron")
            .expect("should write invalid identity");
        let invalid = inspect_identity_file(&dir);
        assert!(matches!(invalid, IdentityFileProbe::Invalid { .. }));

        let identity = ServerIdentity::from_realm_id(Uuid::new_v4());
        identity.save_to_data_dir_warn(&dir);
        let ready = inspect_identity_file(&dir);
        assert!(matches!(ready, IdentityFileProbe::Ready { .. }));

        let _ = fs::remove_dir_all(dir);
    }
}
