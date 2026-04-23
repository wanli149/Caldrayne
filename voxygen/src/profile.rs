use crate::hud;
use client::ServerInfo;
use common::{character::CharacterId, uuid::Uuid};
use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tracing::warn;

/// Represents a character in the profile.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CharacterProfile {
    /// Array representing a character's hotbar.
    pub hotbar_slots: [Option<hud::HotbarSlotContents>; 10],
}

const fn default_slots() -> [Option<hud::HotbarSlotContents>; 10] {
    [None, None, None, None, None, None, None, None, None, None]
}

impl Default for CharacterProfile {
    fn default() -> Self {
        CharacterProfile {
            hotbar_slots: default_slots(),
        }
    }
}

/// Represents a realm in the profile.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct RealmProfile {
    /// A map of character's by id to their CharacterProfile.
    pub characters: HashMap<CharacterId, CharacterProfile>,
    /// Selected character in the chararacter selection screen
    pub selected_character: Option<CharacterId>,
    /// Last spectate position
    pub spectate_position: Option<vek::Vec3<f32>>,
    /// Hash of left-accepted server rules
    pub accepted_rules: Option<u64>,
}

impl Default for RealmProfile {
    fn default() -> Self {
        RealmProfile {
            characters: HashMap::new(),
            selected_character: None,
            spectate_position: None,
            accepted_rules: None,
        }
    }
}

/// `Profile` contains everything that can be configured in the profile.ron
///
/// Initially it is just for persisting things that don't belong in
/// settings.ron - like the state of hotbar and any other character level
/// configuration.
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub realms: HashMap<Uuid, RealmProfile>,
    #[serde(rename = "servers", skip_serializing_if = "HashMap::is_empty")]
    legacy_servers: HashMap<String, RealmProfile>,
    pub mutelist: HashMap<Uuid, String>,
    /// Temporary character profile, used when it should
    /// not be persisted to the disk.
    #[serde(skip)]
    pub transient_character: Option<CharacterProfile>,
    pub tutorial: hud::tutorial::TutorialState,
}

impl Profile {
    /// Load the profile.ron file from the standard path or create it.
    pub fn load(config_dir: &Path) -> Self {
        let path = Profile::get_path(config_dir);

        let profile = common::util::ron_from_path_recoverable::<Self>(&path);
        // Save profile to add new fields or create the file if it is not already there
        profile.save_to_file_warn(config_dir);
        profile
    }

    /// Migrate a legacy name-keyed server bucket into the stable realm bucket
    /// used by current builds.
    ///
    /// Returns `true` when a legacy bucket was consumed and the profile should
    /// be saved.
    pub fn prepare_realm(&mut self, server_info: &ServerInfo) -> bool {
        if self.realms.contains_key(&server_info.realm_id) {
            return false;
        }

        let Some(legacy_profile) = self.legacy_servers.remove(&server_info.name) else {
            return false;
        };

        self.realms.insert(server_info.realm_id, legacy_profile);
        true
    }

    /// Save the current profile to disk, warn on failure.
    pub fn save_to_file_warn(&self, config_dir: &Path) {
        if let Err(e) = self.save_to_file(config_dir) {
            warn!(?e, "Failed to save profile");
        }
    }

    fn realm_profile(&self, realm_id: Uuid) -> Option<&RealmProfile> { self.realms.get(&realm_id) }

    fn realm_profile_mut(&mut self, realm_id: Uuid) -> &mut RealmProfile {
        self.realms.entry(realm_id).or_default()
    }

    /// Get the hotbar_slots for the requested character_id.
    ///
    /// If the realm or character does not exist then the default hotbar_slots
    /// (empty) is returned.
    ///
    /// # Arguments
    ///
    /// * realm_id - current realm the character is on.
    /// * character_id - id of the character, passing `None` indicates the
    ///   transient character profile should be used.
    pub fn get_hotbar_slots(
        &self,
        realm_id: Uuid,
        character_id: Option<CharacterId>,
    ) -> [Option<hud::HotbarSlotContents>; 10] {
        match character_id {
            Some(character_id) => self
                .realm_profile(realm_id)
                .and_then(|s| s.characters.get(&character_id)),
            None => self.transient_character.as_ref(),
        }
        .map(|c| c.hotbar_slots.clone())
        .unwrap_or_else(default_slots)
    }

    /// Set the hotbar_slots for the requested character_id.
    ///
    /// If the realm or character does not exist then the appropriate fields
    /// will be initialised and the slots added.
    ///
    /// # Arguments
    ///
    /// * realm_id - current realm the character is on.
    /// * character_id - id of the character, passing `None` indicates the
    ///   transient character profile should be used.
    /// * slots - array of hotbar_slots to save.
    pub fn set_hotbar_slots(
        &mut self,
        realm_id: Uuid,
        character_id: Option<CharacterId>,
        slots: [Option<hud::HotbarSlotContents>; 10],
    ) {
        match character_id {
            Some(character_id) => self
              .realm_profile_mut(realm_id)
              // Get or update the CharacterProfile.
              .characters
              .entry(character_id)
              .or_default(),
            None => self.transient_character.get_or_insert_default(),
        }
        .hotbar_slots = slots;
    }

    /// Get the selected_character for the provided realm.
    ///
    /// if the realm does not exist then the default selected_character (None)
    /// is returned.
    ///
    /// # Arguments
    ///
    /// * realm_id - current realm the character is on.
    pub fn get_selected_character(&self, realm_id: Uuid) -> Option<CharacterId> {
        self.realm_profile(realm_id)
            .map(|s| s.selected_character)
            .unwrap_or_default()
    }

    /// Set the selected_character for the provided realm.
    ///
    /// If the realm does not exist then the appropriate fields
    /// will be initialised and the selected_character added.
    ///
    /// # Arguments
    ///
    /// * realm_id - current realm the character is on.
    /// * selected_character - option containing selected character ID
    pub fn set_selected_character(
        &mut self,
        realm_id: Uuid,
        selected_character: Option<CharacterId>,
    ) {
        self.realm_profile_mut(realm_id).selected_character = selected_character;
    }

    /// Get the spectate_position for the provided realm.
    ///
    /// if the realm does not exist then the default spectate_position (None)
    /// is returned.
    ///
    /// # Arguments
    ///
    /// * realm_id - current realm the player is on.
    pub fn get_spectate_position(&self, realm_id: Uuid) -> Option<vek::Vec3<f32>> {
        self.realm_profile(realm_id)
            .map(|s| s.spectate_position)
            .unwrap_or_default()
    }

    /// Set the spectate_position for the provided realm.
    ///
    /// If the realm does not exist then the appropriate fields
    /// will be initialised and the selected_character added.
    ///
    /// # Arguments
    ///
    /// * realm_id - current realm the player is on.
    /// * spectate_position - option containing the position we're spectating
    pub fn set_spectate_position(
        &mut self,
        realm_id: Uuid,
        spectate_position: Option<vek::Vec3<f32>>,
    ) {
        self.realm_profile_mut(realm_id).spectate_position = spectate_position;
    }

    pub fn get_accepted_rules(&self, realm_id: Uuid) -> Option<u64> {
        self.realm_profile(realm_id)
            .and_then(|realm| realm.accepted_rules)
    }

    pub fn set_accepted_rules(&mut self, realm_id: Uuid, accepted_rules: Option<u64>) {
        self.realm_profile_mut(realm_id).accepted_rules = accepted_rules;
    }

    /// Save the current profile to disk.
    fn save_to_file(&self, config_dir: &Path) -> std::io::Result<()> {
        let path = Self::get_path(config_dir);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }

        let ron = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).unwrap();
        fs::write(path, ron.as_bytes())
    }

    fn get_path(config_dir: &Path) -> PathBuf { config_dir.join("profile.ron") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_slots_with_empty_profile() {
        let profile = Profile::default();
        let slots = profile.get_hotbar_slots(Uuid::new_v4(), Some(CharacterId(12345)));
        assert_eq!(slots, [(); 10].map(|()| None))
    }

    #[test]
    fn test_set_slots_with_empty_profile() {
        let mut profile = Profile::default();
        let slots = [(); 10].map(|()| None);
        profile.set_hotbar_slots(Uuid::new_v4(), Some(CharacterId(12345)), slots);
    }

    #[test]
    fn prepare_realm_moves_legacy_bucket_once() {
        let realm_id = Uuid::new_v4();
        let mut profile = Profile::default();
        profile
            .legacy_servers
            .insert("Official Realm".to_string(), RealmProfile {
                selected_character: Some(CharacterId(42)),
                ..Default::default()
            });

        assert!(profile.prepare_realm(&server_info("Official Realm", realm_id)));
        assert_eq!(
            profile.get_selected_character(realm_id),
            Some(CharacterId(42))
        );
        assert!(!profile.prepare_realm(&server_info("Official Realm", realm_id)));
        assert!(!profile.legacy_servers.contains_key("Official Realm"));
    }

    #[test]
    fn prepare_realm_consumes_legacy_singleplayer_bucket_only_once() {
        let first_realm = Uuid::new_v4();
        let second_realm = Uuid::new_v4();
        let mut profile = Profile::default();
        profile
            .legacy_servers
            .insert("Singleplayer".to_string(), RealmProfile {
                spectate_position: Some(vek::Vec3::new(1.0, 2.0, 3.0)),
                ..Default::default()
            });

        assert!(profile.prepare_realm(&server_info("Singleplayer", first_realm)));
        assert_eq!(
            profile.get_spectate_position(first_realm),
            Some(vek::Vec3::new(1.0, 2.0, 3.0))
        );

        assert!(!profile.prepare_realm(&server_info("Singleplayer", second_realm)));
        assert_eq!(profile.get_spectate_position(second_realm), None);
    }

    fn server_info(name: &str, realm_id: Uuid) -> ServerInfo {
        ServerInfo {
            realm_id,
            name: name.to_string(),
            git_hash: 0,
            git_timestamp: 0,
            auth_provider: None,
        }
    }
}
