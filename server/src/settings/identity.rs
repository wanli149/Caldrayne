use common::uuid::Uuid;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tracing::warn;

const IDENTITY_FILENAME: &str = "identity.ron";

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
        let path = Self::get_path(data_dir);
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
        let path = Self::get_path(data_dir);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }

        let ron = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).unwrap();
        fs::write(path, ron.as_bytes())
    }

    fn get_path(data_dir: &Path) -> PathBuf { data_dir.join(IDENTITY_FILENAME) }
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
}
