use common::uuid::Uuid;
use hashbrown::HashSet;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DevMultiplayerTargetKind {
    #[default]
    DirectConnect,
    LocalDedicated,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LocalDedicatedConnectionKind {
    #[default]
    Tcp,
    Quic,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LocalDedicatedSourceKind {
    #[default]
    Manual,
    UserdataDefault,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LocalDedicatedServer {
    /// Stable client-local reference for this configured local instance.
    #[serde(default = "Uuid::new_v4")]
    pub instance_id: Uuid,
    /// How this local dedicated source entered the client inventory.
    pub source_kind: LocalDedicatedSourceKind,
    /// Optional backing data directory for this local dedicated source.
    ///
    /// This is inventory/source metadata, not the logical realm identity.
    pub data_dir: Option<PathBuf>,
    /// Whether this entry currently carries a user-declared manual registration
    /// on top of any discovered source metadata.
    ///
    /// Pure manual entries remain manual even when this flag is false, so this
    /// field mainly matters when a discovered source is upgraded with manual
    /// overrides.
    pub manual_registration: bool,
    /// Last discovered source address for this local dedicated source.
    ///
    /// This lets manual overrides coexist with discovered source refreshes
    /// without losing the underlying source defaults.
    pub source_server_address: Option<String>,
    /// Last discovered source connection kind for this local dedicated source.
    pub source_connection_kind: Option<LocalDedicatedConnectionKind>,
    /// Last discovered source TLS validation semantic for this local dedicated
    /// source.
    pub source_validate_tls: Option<bool>,
    /// Human-facing label for a local dedicated instance.
    ///
    /// This is convenience metadata for developer tooling and UI only. Logical
    /// identity still comes from the server-provided `realm_id`.
    pub display_name: String,
    /// Reachable address of the local dedicated instance.
    pub server_address: String,
    /// Preferred connection kind for this local dedicated instance.
    pub connection_kind: LocalDedicatedConnectionKind,
    /// Whether TLS validation should be enforced when using QUIC.
    pub validate_tls: bool,
    /// Cached realm identity last observed from the server handshake.
    ///
    /// This is a client-side observation cache only, not the authority source.
    pub last_seen_realm_id: Option<Uuid>,
    /// Cached server display name last observed from the server handshake.
    pub last_seen_server_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManualLocalDedicatedServerSpec {
    /// Optional stable reference when editing an existing manual local
    /// instance.
    pub instance_id: Option<Uuid>,
    /// Optional backing data directory when this manual source is anchored to a
    /// concrete local dedicated data dir.
    pub data_dir: Option<PathBuf>,
    /// Human-facing label for a local dedicated instance.
    pub display_name: String,
    /// Reachable address of the local dedicated instance.
    pub server_address: String,
    /// Preferred connection kind for this local dedicated instance.
    pub connection_kind: LocalDedicatedConnectionKind,
    /// Whether TLS validation should be enforced when using QUIC.
    pub validate_tls: bool,
}

impl Default for LocalDedicatedServer {
    fn default() -> Self {
        Self {
            instance_id: Uuid::new_v4(),
            source_kind: LocalDedicatedSourceKind::Manual,
            data_dir: None,
            manual_registration: false,
            source_server_address: None,
            source_connection_kind: None,
            source_validate_tls: None,
            display_name: String::new(),
            server_address: String::new(),
            connection_kind: LocalDedicatedConnectionKind::Tcp,
            validate_tls: false,
            last_seen_realm_id: None,
            last_seen_server_name: None,
        }
    }
}

impl LocalDedicatedServer {
    pub fn has_manual_registration(&self) -> bool {
        self.manual_registration || self.source_kind == LocalDedicatedSourceKind::Manual
    }

    pub fn label(&self) -> String {
        let server_address = self.server_address.trim();
        let display_name = self.display_name.trim();

        if !display_name.is_empty() {
            return format!("{display_name} ({server_address})");
        }

        let observed_name = self
            .last_seen_server_name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty());
        if let Some(observed_name) = observed_name {
            return format!("{observed_name} ({server_address})");
        }

        server_address.to_string()
    }

    pub fn connection_args(&self) -> client::addr::ConnectionArgs {
        match self.connection_kind {
            LocalDedicatedConnectionKind::Tcp => client::addr::ConnectionArgs::Tcp {
                hostname: self.server_address.trim().to_string(),
                prefer_ipv6: false,
            },
            LocalDedicatedConnectionKind::Quic => client::addr::ConnectionArgs::Quic {
                hostname: self.server_address.trim().to_string(),
                prefer_ipv6: false,
                validate_tls: self.validate_tls,
            },
        }
    }
}

impl NetworkingSettings {
    pub fn register_manual_local_dedicated_from_direct_connect(
        &mut self,
        server_address: &str,
    ) -> Option<(Uuid, bool)> {
        let server_address = server_address.trim();
        if server_address.is_empty() {
            return None;
        }

        let existing = self.local_dedicated_server(server_address).cloned();
        let connection_kind = if self.use_quic {
            LocalDedicatedConnectionKind::Quic
        } else {
            LocalDedicatedConnectionKind::Tcp
        };
        let validate_tls =
            matches!(connection_kind, LocalDedicatedConnectionKind::Quic) && self.validate_tls;

        Some(
            self.upsert_manual_local_dedicated_server(ManualLocalDedicatedServerSpec {
                instance_id: existing.as_ref().map(|entry| entry.instance_id),
                data_dir: existing.and_then(|entry| entry.data_dir),
                display_name: String::new(),
                server_address: server_address.to_string(),
                connection_kind,
                validate_tls,
            }),
        )
    }

    pub fn upsert_manual_local_dedicated_server(
        &mut self,
        spec: ManualLocalDedicatedServerSpec,
    ) -> (Uuid, bool) {
        let instance_id = spec
            .instance_id
            .or_else(|| spec.data_dir.as_deref().map(local_instance_id))
            .unwrap_or_else(Uuid::new_v4);

        let incoming = LocalDedicatedServer {
            instance_id,
            source_kind: LocalDedicatedSourceKind::Manual,
            data_dir: spec.data_dir.clone(),
            manual_registration: true,
            display_name: spec.display_name,
            server_address: spec.server_address,
            connection_kind: spec.connection_kind,
            validate_tls: spec.validate_tls,
            ..LocalDedicatedServer::default()
        };

        if let Some(existing_index) = self
            .local_dedicated_servers
            .iter()
            .position(|entry| same_manual_local_dedicated_inventory_entry(entry, &incoming))
        {
            let instance_id = self.local_dedicated_servers[existing_index].instance_id;
            let changed = merge_manual_local_dedicated_server(
                &mut self.local_dedicated_servers[existing_index],
                incoming,
            );
            return (instance_id, changed);
        }

        let changed = self.upsert_local_dedicated_server(incoming);
        (instance_id, changed)
    }

    pub fn update_manual_local_dedicated_registration(
        &mut self,
        instance_id: Uuid,
        mut spec: ManualLocalDedicatedServerSpec,
    ) -> bool {
        let Some(existing) = self
            .local_dedicated_server_by_instance_id(instance_id)
            .cloned()
        else {
            return false;
        };

        spec.instance_id = Some(instance_id);
        if spec.data_dir.is_none() {
            spec.data_dir = existing.data_dir;
        }

        self.upsert_manual_local_dedicated_server(spec).1
    }

    pub fn remove_local_dedicated_manual_registration(&mut self, instance_id: Uuid) -> bool {
        let Some(existing_index) = self
            .local_dedicated_servers
            .iter()
            .position(|entry| entry.instance_id == instance_id)
        else {
            return false;
        };

        if self.local_dedicated_servers[existing_index].source_kind
            == LocalDedicatedSourceKind::Manual
        {
            self.local_dedicated_servers.remove(existing_index);
            let mut changed = true;
            if self.default_local_dedicated_instance_id == Some(instance_id) {
                self.default_local_dedicated_instance_id = None;
                changed = true;
            }
            return changed;
        }

        if !self.local_dedicated_servers[existing_index].manual_registration {
            return false;
        }

        let entry = &mut self.local_dedicated_servers[existing_index];
        let mut changed = false;

        if entry.manual_registration {
            entry.manual_registration = false;
            changed = true;
        }
        if !entry.display_name.is_empty() {
            entry.display_name.clear();
            changed = true;
        }
        if let Some(source_server_address) = entry.source_server_address.as_ref()
            && entry.server_address != *source_server_address
        {
            entry.server_address = source_server_address.clone();
            changed = true;
        }
        if let Some(source_connection_kind) = entry.source_connection_kind
            && entry.connection_kind != source_connection_kind
        {
            entry.connection_kind = source_connection_kind;
            changed = true;
        }
        if let Some(source_validate_tls) = entry.source_validate_tls
            && entry.validate_tls != source_validate_tls
        {
            entry.validate_tls = source_validate_tls;
            changed = true;
        }

        changed
    }

    pub fn upsert_local_dedicated_server(&mut self, incoming: LocalDedicatedServer) -> bool {
        if let Some(existing_index) = self
            .local_dedicated_servers
            .iter()
            .position(|entry| same_local_dedicated_inventory_entry(entry, &incoming))
        {
            let mut changed = merge_local_dedicated_server(
                &mut self.local_dedicated_servers[existing_index],
                incoming,
            );

            let dedup_instance_id = self.local_dedicated_servers[existing_index].instance_id;
            let dedup_data_dir = self.local_dedicated_servers[existing_index]
                .data_dir
                .clone();
            let dedup_server_address = self.local_dedicated_servers[existing_index]
                .server_address
                .trim()
                .to_string();
            let dedup_source_kind = self.local_dedicated_servers[existing_index].source_kind;

            let len_before = self.local_dedicated_servers.len();
            self.local_dedicated_servers = self
                .local_dedicated_servers
                .drain(..)
                .enumerate()
                .filter_map(|(index, entry)| {
                    (index == existing_index
                        || !same_local_dedicated_inventory_key(
                            &entry,
                            dedup_instance_id,
                            dedup_source_kind,
                            dedup_data_dir.as_deref(),
                            &dedup_server_address,
                        ))
                    .then_some(entry)
                })
                .collect();
            changed |= self.local_dedicated_servers.len() != len_before;
            changed
        } else {
            self.local_dedicated_servers.push(incoming);
            true
        }
    }

    pub fn update_local_dedicated_observation(
        &mut self,
        server_address: &str,
        realm_id: Uuid,
        server_name: &str,
    ) -> bool {
        let server_address = server_address.trim();
        let Some(entry) = self
            .local_dedicated_servers
            .iter_mut()
            .find(|entry| entry.server_address.trim() == server_address)
        else {
            return false;
        };

        let mut changed = false;
        if entry.last_seen_realm_id != Some(realm_id) {
            entry.last_seen_realm_id = Some(realm_id);
            changed = true;
        }

        if entry.last_seen_server_name.as_deref() != Some(server_name) {
            entry.last_seen_server_name = Some(server_name.to_string());
            changed = true;
        }

        changed
    }

    pub fn update_local_dedicated_observation_by_instance_id(
        &mut self,
        instance_id: Uuid,
        realm_id: Uuid,
        server_name: &str,
    ) -> bool {
        let Some(entry) = self
            .local_dedicated_servers
            .iter_mut()
            .find(|entry| entry.instance_id == instance_id)
        else {
            return false;
        };

        let mut changed = false;
        if entry.last_seen_realm_id != Some(realm_id) {
            entry.last_seen_realm_id = Some(realm_id);
            changed = true;
        }

        if entry.last_seen_server_name.as_deref() != Some(server_name) {
            entry.last_seen_server_name = Some(server_name.to_string());
            changed = true;
        }

        changed
    }

    pub fn local_dedicated_server(&self, server_address: &str) -> Option<&LocalDedicatedServer> {
        let server_address = server_address.trim();
        self.local_dedicated_servers
            .iter()
            .find(|entry| entry.server_address.trim() == server_address)
    }

    pub fn local_dedicated_server_by_instance_id(
        &self,
        instance_id: Uuid,
    ) -> Option<&LocalDedicatedServer> {
        self.local_dedicated_servers
            .iter()
            .find(|entry| entry.instance_id == instance_id)
    }

    pub fn default_local_dedicated_server(&self) -> Option<&LocalDedicatedServer> {
        self.default_local_dedicated_instance_id
            .and_then(|instance_id| self.local_dedicated_server_by_instance_id(instance_id))
            .or_else(|| {
                let server_address = self.default_server.trim();
                (!server_address.is_empty()).then(|| self.local_dedicated_server(server_address))?
            })
    }

    pub fn sync_default_local_dedicated_source(&mut self, userdata_dir: &Path) -> bool {
        let default_data_dir = userdata_dir.join("server");
        let default_instance_id = local_instance_id(&default_data_dir);

        let Some(detected) = probe_default_local_dedicated_source(userdata_dir) else {
            let stale_before = self.local_dedicated_servers.len();
            self.local_dedicated_servers.retain(|entry| {
                !(entry.source_kind == LocalDedicatedSourceKind::UserdataDefault
                    || entry.instance_id == default_instance_id)
            });
            let mut changed = self.local_dedicated_servers.len() != stale_before;
            if self.default_local_dedicated_instance_id == Some(default_instance_id) {
                self.default_local_dedicated_instance_id = None;
                changed = true;
            }
            return changed;
        };

        self.upsert_local_dedicated_server(detected)
    }
}

#[derive(Deserialize, Serialize)]
struct ProbeDedicatedSettings {
    gameserver_protocols: Vec<ProbeDedicatedProtocol>,
    server_name: String,
}

#[derive(Deserialize, Serialize)]
enum ProbeDedicatedProtocol {
    Quic { address: SocketAddr },
    Tcp { address: SocketAddr },
}

#[derive(Deserialize, Serialize)]
struct ProbeDedicatedIdentity {
    realm_id: Uuid,
}

fn probe_default_local_dedicated_source(userdata_dir: &Path) -> Option<LocalDedicatedServer> {
    let data_dir = userdata_dir.join("server");
    let settings = read_probe_settings(&data_dir)?;
    let (server_address, connection_kind, validate_tls) =
        preferred_local_connection(&settings.gameserver_protocols)?;
    let identity = read_probe_identity(&data_dir);

    Some(LocalDedicatedServer {
        instance_id: local_instance_id(&data_dir),
        source_kind: LocalDedicatedSourceKind::UserdataDefault,
        data_dir: Some(data_dir),
        source_server_address: Some(server_address.clone()),
        source_connection_kind: Some(connection_kind),
        source_validate_tls: Some(validate_tls),
        server_address,
        connection_kind,
        validate_tls,
        last_seen_realm_id: identity.map(|identity| identity.realm_id),
        last_seen_server_name: Some(settings.server_name),
        ..LocalDedicatedServer::default()
    })
}

fn read_probe_settings(data_dir: &Path) -> Option<ProbeDedicatedSettings> {
    read_ron::<ProbeDedicatedSettings>(&data_dir.join("server_config").join("settings.ron"))
}

fn read_probe_identity(data_dir: &Path) -> Option<ProbeDedicatedIdentity> {
    read_ron::<ProbeDedicatedIdentity>(&data_dir.join("identity.ron"))
}

fn read_ron<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let file = fs::File::open(path).ok()?;
    ron::de::from_reader(file).ok()
}

fn preferred_local_connection(
    protocols: &[ProbeDedicatedProtocol],
) -> Option<(String, LocalDedicatedConnectionKind, bool)> {
    for protocol in protocols {
        if let ProbeDedicatedProtocol::Tcp { address } = protocol {
            return Some((
                local_reachable_socket_addr(*address).to_string(),
                LocalDedicatedConnectionKind::Tcp,
                false,
            ));
        }
    }

    for protocol in protocols {
        if let ProbeDedicatedProtocol::Quic { address } = protocol {
            return Some((
                local_reachable_socket_addr(*address).to_string(),
                LocalDedicatedConnectionKind::Quic,
                false,
            ));
        }
    }

    None
}

fn local_reachable_socket_addr(address: SocketAddr) -> SocketAddr {
    match address.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => {
            SocketAddr::from((Ipv4Addr::LOCALHOST, address.port()))
        },
        IpAddr::V6(ip) if ip.is_unspecified() => {
            SocketAddr::from((Ipv6Addr::LOCALHOST, address.port()))
        },
        _ => address,
    }
}

fn local_instance_id(data_dir: &Path) -> Uuid {
    let canonical = data_dir
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(data_dir));
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    let hash = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    Uuid::from_u128(u128::from_be_bytes(bytes))
}

fn same_local_dedicated_inventory_entry(
    existing: &LocalDedicatedServer,
    incoming: &LocalDedicatedServer,
) -> bool {
    existing.instance_id == incoming.instance_id
        || (existing.data_dir.is_some()
            && incoming.data_dir.is_some()
            && existing.data_dir == incoming.data_dir)
        || (existing.source_kind == incoming.source_kind
            && !existing.server_address.trim().is_empty()
            && existing.server_address.trim() == incoming.server_address.trim())
}

fn same_local_dedicated_inventory_key(
    entry: &LocalDedicatedServer,
    instance_id: Uuid,
    source_kind: LocalDedicatedSourceKind,
    data_dir: Option<&Path>,
    server_address: &str,
) -> bool {
    entry.instance_id == instance_id
        || (data_dir.is_some() && entry.data_dir.as_deref() == data_dir)
        || (entry.source_kind == source_kind
            && !server_address.is_empty()
            && entry.server_address.trim() == server_address)
}

fn same_manual_local_dedicated_inventory_entry(
    existing: &LocalDedicatedServer,
    incoming: &LocalDedicatedServer,
) -> bool {
    existing.instance_id == incoming.instance_id
        || (existing.data_dir.is_some()
            && incoming.data_dir.is_some()
            && existing.data_dir == incoming.data_dir)
}

fn merge_local_dedicated_server(
    existing: &mut LocalDedicatedServer,
    incoming: LocalDedicatedServer,
) -> bool {
    let mut changed = false;
    let preserve_manual_overrides = existing.has_manual_registration()
        && incoming.source_kind != LocalDedicatedSourceKind::Manual;
    let incoming_source_server_address = incoming.source_server_address.clone().or_else(|| {
        (incoming.source_kind != LocalDedicatedSourceKind::Manual
            && !incoming.server_address.is_empty())
        .then(|| incoming.server_address.clone())
    });
    let incoming_source_connection_kind = incoming.source_connection_kind.or_else(|| {
        (incoming.source_kind != LocalDedicatedSourceKind::Manual)
            .then_some(incoming.connection_kind)
    });
    let incoming_source_validate_tls = incoming.source_validate_tls.or_else(|| {
        (incoming.source_kind != LocalDedicatedSourceKind::Manual).then_some(incoming.validate_tls)
    });

    if existing.instance_id != incoming.instance_id {
        existing.instance_id = incoming.instance_id;
        changed = true;
    }
    if existing.source_kind != incoming.source_kind {
        existing.source_kind = incoming.source_kind;
        changed = true;
    }
    if existing.data_dir != incoming.data_dir {
        existing.data_dir = incoming.data_dir.clone();
        changed = true;
    }
    if existing.manual_registration
        != (existing.manual_registration
            || preserve_manual_overrides
            || incoming.manual_registration)
    {
        existing.manual_registration = existing.manual_registration
            || preserve_manual_overrides
            || incoming.manual_registration;
        changed = true;
    }
    if existing.source_server_address != incoming_source_server_address
        && incoming_source_server_address.is_some()
    {
        existing.source_server_address = incoming_source_server_address;
        changed = true;
    }
    if existing.source_connection_kind != incoming_source_connection_kind
        && incoming_source_connection_kind.is_some()
    {
        existing.source_connection_kind = incoming_source_connection_kind;
        changed = true;
    }
    if existing.source_validate_tls != incoming_source_validate_tls
        && incoming_source_validate_tls.is_some()
    {
        existing.source_validate_tls = incoming_source_validate_tls;
        changed = true;
    }
    if !incoming.display_name.trim().is_empty() && existing.display_name != incoming.display_name {
        existing.display_name = incoming.display_name;
        changed = true;
    }
    if !preserve_manual_overrides && existing.server_address != incoming.server_address {
        existing.server_address = incoming.server_address;
        changed = true;
    }
    if !preserve_manual_overrides && existing.connection_kind != incoming.connection_kind {
        existing.connection_kind = incoming.connection_kind;
        changed = true;
    }
    if !preserve_manual_overrides && existing.validate_tls != incoming.validate_tls {
        existing.validate_tls = incoming.validate_tls;
        changed = true;
    }
    if existing.last_seen_realm_id != incoming.last_seen_realm_id {
        existing.last_seen_realm_id = incoming.last_seen_realm_id;
        changed = true;
    }
    if existing.last_seen_server_name != incoming.last_seen_server_name {
        existing.last_seen_server_name = incoming.last_seen_server_name;
        changed = true;
    }

    changed
}

fn merge_manual_local_dedicated_server(
    existing: &mut LocalDedicatedServer,
    incoming: LocalDedicatedServer,
) -> bool {
    let mut changed = false;

    if !existing.manual_registration {
        existing.manual_registration = true;
        changed = true;
    }
    if existing.source_kind != LocalDedicatedSourceKind::Manual
        && existing.source_server_address.is_none()
        && !existing.server_address.is_empty()
    {
        existing.source_server_address = Some(existing.server_address.clone());
        changed = true;
    }
    if existing.source_kind != LocalDedicatedSourceKind::Manual
        && existing.source_connection_kind.is_none()
    {
        existing.source_connection_kind = Some(existing.connection_kind);
        changed = true;
    }
    if existing.source_kind != LocalDedicatedSourceKind::Manual
        && existing.source_validate_tls.is_none()
    {
        existing.source_validate_tls = Some(existing.validate_tls);
        changed = true;
    }
    if existing.source_kind == LocalDedicatedSourceKind::Manual
        && existing.instance_id != incoming.instance_id
    {
        existing.instance_id = incoming.instance_id;
        changed = true;
    }
    if existing.data_dir.is_none() && incoming.data_dir.is_some() {
        existing.data_dir = incoming.data_dir.clone();
        changed = true;
    }
    if !incoming.display_name.trim().is_empty() && existing.display_name != incoming.display_name {
        existing.display_name = incoming.display_name;
        changed = true;
    }
    if existing.server_address != incoming.server_address {
        existing.server_address = incoming.server_address;
        changed = true;
    }
    if existing.connection_kind != incoming.connection_kind {
        existing.connection_kind = incoming.connection_kind;
        changed = true;
    }
    if existing.validate_tls != incoming.validate_tls {
        existing.validate_tls = incoming.validate_tls;
        changed = true;
    }

    changed
}

/// `NetworkingSettings` stores local networking preferences plus legacy/dev
/// topology state.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkingSettings {
    pub username: String,
    /// Saved direct-connect history for development workflows.
    ///
    /// Public mode must not treat this list as a product entry source.
    pub servers: Vec<String>,
    /// Last direct-connect target used for development workflows.
    ///
    /// Public mode must ignore this as an official Realm selector.
    pub default_server: String,
    /// Stable reference to the last selected local dedicated instance.
    ///
    /// This lets DevLocalDedicated defaults survive address drift without
    /// falling back to raw address matching.
    pub default_local_dedicated_instance_id: Option<Uuid>,
    /// Last selected developer multiplayer target semantic.
    ///
    /// This disambiguates whether `default_server` should be treated as a
    /// generic direct-connect address or as a configured local dedicated
    /// instance reference.
    pub default_multiplayer_target_kind: DevMultiplayerTargetKind,
    /// Saved trust decisions for non-official authentication servers in
    /// development mode.
    ///
    /// Public mode only trusts the bundled official auth server configuration.
    pub trusted_auth_servers: HashSet<String>,
    /// Configured local dedicated instances available from development mode.
    ///
    /// These are UI/dev entry references only, not logical Realm identity.
    pub local_dedicated_servers: Vec<LocalDedicatedServer>,
    /// Dev direct-connect transport preference. Public mode uses the bundled
    /// official entry.
    pub use_srv: bool,
    /// Dev direct-connect transport preference. Public mode uses the bundled
    /// official entry.
    pub use_quic: bool,
    /// Dev direct-connect transport preference. Public mode uses the bundled
    /// official entry.
    pub validate_tls: bool,
    pub player_physics_behavior: bool,
    pub lossy_terrain_compression: bool,
    pub enable_discord_integration: bool,
}

impl Default for NetworkingSettings {
    fn default() -> Self {
        Self {
            username: "".to_string(),
            servers: Vec::new(),
            default_server: String::new(),
            default_local_dedicated_instance_id: None,
            default_multiplayer_target_kind: DevMultiplayerTargetKind::DirectConnect,
            trusted_auth_servers: HashSet::new(),
            local_dedicated_servers: Vec::new(),
            use_srv: true,
            use_quic: false,
            validate_tls: true,
            player_physics_behavior: false,
            lossy_terrain_compression: false,
            enable_discord_integration: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn local_dedicated_server_default_generates_instance_id() {
        let server = LocalDedicatedServer::default();

        assert!(!server.instance_id.is_nil());
    }

    #[test]
    fn update_local_dedicated_observation_updates_cached_metadata() {
        let mut settings = NetworkingSettings {
            local_dedicated_servers: vec![LocalDedicatedServer {
                server_address: "127.0.0.1:14004".to_string(),
                ..LocalDedicatedServer::default()
            }],
            ..NetworkingSettings::default()
        };
        let realm_id = Uuid::new_v4();

        assert!(settings.update_local_dedicated_observation(
            "127.0.0.1:14004",
            realm_id,
            "Local Dev Realm"
        ));
        assert_eq!(
            settings.local_dedicated_servers[0].last_seen_realm_id,
            Some(realm_id)
        );
        assert_eq!(
            settings.local_dedicated_servers[0]
                .last_seen_server_name
                .as_deref(),
            Some("Local Dev Realm")
        );
    }

    #[test]
    fn update_local_dedicated_observation_by_instance_id_updates_cached_metadata() {
        let instance_id = Uuid::new_v4();
        let mut settings = NetworkingSettings {
            local_dedicated_servers: vec![LocalDedicatedServer {
                instance_id,
                server_address: "127.0.0.1:14004".to_string(),
                ..LocalDedicatedServer::default()
            }],
            ..NetworkingSettings::default()
        };
        let realm_id = Uuid::new_v4();

        assert!(settings.update_local_dedicated_observation_by_instance_id(
            instance_id,
            realm_id,
            "Local Dev Realm"
        ));
        assert_eq!(
            settings.local_dedicated_servers[0].last_seen_realm_id,
            Some(realm_id)
        );
        assert_eq!(
            settings.local_dedicated_servers[0]
                .last_seen_server_name
                .as_deref(),
            Some("Local Dev Realm")
        );
    }

    #[test]
    fn sync_default_local_dedicated_source_registers_userdata_server_source() {
        let userdata_dir =
            std::env::temp_dir().join(format!("caldrayne-voxygen-userdata-{}", Uuid::new_v4()));
        let data_dir = userdata_dir.join("server");
        fs::create_dir_all(data_dir.join("server_config")).expect("create config dir");

        let settings_file = data_dir.join("server_config").join("settings.ron");
        let settings_ron = ron::ser::to_string_pretty(
            &ProbeDedicatedSettings {
                gameserver_protocols: vec![ProbeDedicatedProtocol::Tcp {
                    address: SocketAddr::from((Ipv4Addr::UNSPECIFIED, 14004)),
                }],
                server_name: "Local Source".to_string(),
            },
            ron::ser::PrettyConfig::default(),
        )
        .expect("serialize settings");
        fs::write(&settings_file, settings_ron).expect("write settings");

        let identity_file = data_dir.join("identity.ron");
        let realm_id = Uuid::new_v4();
        let identity_ron = ron::ser::to_string_pretty(
            &ProbeDedicatedIdentity { realm_id },
            ron::ser::PrettyConfig::default(),
        )
        .expect("serialize identity");
        fs::write(&identity_file, identity_ron).expect("write identity");

        let mut networking = NetworkingSettings::default();

        assert!(networking.sync_default_local_dedicated_source(&userdata_dir));
        assert_eq!(networking.local_dedicated_servers.len(), 1);

        let detected = &networking.local_dedicated_servers[0];
        assert_eq!(
            detected.source_kind,
            LocalDedicatedSourceKind::UserdataDefault
        );
        assert_eq!(
            detected.source_server_address.as_deref(),
            Some("127.0.0.1:14004")
        );
        assert_eq!(
            detected.source_connection_kind,
            Some(LocalDedicatedConnectionKind::Tcp)
        );
        assert_eq!(detected.source_validate_tls, Some(false));
        assert_eq!(detected.data_dir.as_deref(), Some(data_dir.as_path()));
        assert_eq!(detected.server_address, "127.0.0.1:14004");
        assert_eq!(detected.connection_kind, LocalDedicatedConnectionKind::Tcp);
        assert_eq!(detected.last_seen_realm_id, Some(realm_id));
        assert_eq!(
            detected.last_seen_server_name.as_deref(),
            Some("Local Source")
        );

        let _ = fs::remove_dir_all(userdata_dir);
    }

    #[test]
    fn sync_default_local_dedicated_source_upgrades_existing_default_entry_metadata() {
        let userdata_dir =
            std::env::temp_dir().join(format!("caldrayne-voxygen-userdata-{}", Uuid::new_v4()));
        let data_dir = userdata_dir.join("server");
        fs::create_dir_all(data_dir.join("server_config")).expect("create config dir");

        let settings_file = data_dir.join("server_config").join("settings.ron");
        let settings_ron = ron::ser::to_string_pretty(
            &ProbeDedicatedSettings {
                gameserver_protocols: vec![ProbeDedicatedProtocol::Tcp {
                    address: SocketAddr::from((Ipv4Addr::UNSPECIFIED, 14004)),
                }],
                server_name: "Local Source".to_string(),
            },
            ron::ser::PrettyConfig::default(),
        )
        .expect("serialize settings");
        fs::write(&settings_file, settings_ron).expect("write settings");

        let mut networking = NetworkingSettings {
            local_dedicated_servers: vec![LocalDedicatedServer {
                instance_id: local_instance_id(&data_dir),
                display_name: "Pinned Local".to_string(),
                server_address: "127.0.0.1:14004".to_string(),
                ..LocalDedicatedServer::default()
            }],
            ..NetworkingSettings::default()
        };

        assert!(networking.sync_default_local_dedicated_source(&userdata_dir));
        assert_eq!(networking.local_dedicated_servers.len(), 1);

        let detected = &networking.local_dedicated_servers[0];
        assert_eq!(detected.display_name, "Pinned Local");
        assert_eq!(
            detected.source_kind,
            LocalDedicatedSourceKind::UserdataDefault
        );
        assert_eq!(detected.data_dir.as_deref(), Some(data_dir.as_path()));
        assert_eq!(
            detected.last_seen_server_name.as_deref(),
            Some("Local Source")
        );

        let _ = fs::remove_dir_all(userdata_dir);
    }

    #[test]
    fn sync_default_local_dedicated_source_removes_stale_default_entry() {
        let userdata_dir =
            std::env::temp_dir().join(format!("caldrayne-voxygen-userdata-{}", Uuid::new_v4()));
        let data_dir = userdata_dir.join("server");
        let default_instance_id = local_instance_id(&data_dir);

        let mut networking = NetworkingSettings {
            default_local_dedicated_instance_id: Some(default_instance_id),
            local_dedicated_servers: vec![
                LocalDedicatedServer {
                    instance_id: default_instance_id,
                    source_kind: LocalDedicatedSourceKind::UserdataDefault,
                    data_dir: Some(data_dir.clone()),
                    server_address: "127.0.0.1:14004".to_string(),
                    ..LocalDedicatedServer::default()
                },
                LocalDedicatedServer {
                    server_address: "127.0.0.1:24004".to_string(),
                    ..LocalDedicatedServer::default()
                },
            ],
            ..NetworkingSettings::default()
        };

        assert!(networking.sync_default_local_dedicated_source(&userdata_dir));
        assert_eq!(networking.local_dedicated_servers.len(), 1);
        assert_eq!(networking.default_local_dedicated_instance_id, None);
        assert_eq!(
            networking.local_dedicated_servers[0].server_address,
            "127.0.0.1:24004"
        );
    }

    #[test]
    fn default_local_dedicated_server_prefers_instance_id_over_stale_default_address() {
        let instance_id = Uuid::new_v4();
        let networking = NetworkingSettings {
            default_server: "127.0.0.1:9999".to_string(),
            default_local_dedicated_instance_id: Some(instance_id),
            local_dedicated_servers: vec![LocalDedicatedServer {
                instance_id,
                server_address: "127.0.0.1:14004".to_string(),
                ..LocalDedicatedServer::default()
            }],
            ..NetworkingSettings::default()
        };

        let entry = networking
            .default_local_dedicated_server()
            .expect("default local dedicated entry should resolve by instance id");
        assert_eq!(entry.server_address, "127.0.0.1:14004");
    }

    #[test]
    fn upsert_local_dedicated_server_collapses_duplicate_instance_entries_only() {
        let shared_instance_id = Uuid::new_v4();
        let unrelated_instance_id = Uuid::new_v4();
        let mut networking = NetworkingSettings {
            local_dedicated_servers: vec![
                LocalDedicatedServer {
                    instance_id: shared_instance_id,
                    source_kind: LocalDedicatedSourceKind::Manual,
                    display_name: "Pinned Local".to_string(),
                    server_address: "127.0.0.1:14004".to_string(),
                    ..LocalDedicatedServer::default()
                },
                LocalDedicatedServer {
                    instance_id: shared_instance_id,
                    source_kind: LocalDedicatedSourceKind::UserdataDefault,
                    data_dir: Some(PathBuf::from("userdata/server")),
                    server_address: "127.0.0.1:14005".to_string(),
                    ..LocalDedicatedServer::default()
                },
                LocalDedicatedServer {
                    instance_id: unrelated_instance_id,
                    source_kind: LocalDedicatedSourceKind::Manual,
                    server_address: "127.0.0.1:24004".to_string(),
                    ..LocalDedicatedServer::default()
                },
            ],
            ..NetworkingSettings::default()
        };

        assert!(
            networking.upsert_local_dedicated_server(LocalDedicatedServer {
                instance_id: shared_instance_id,
                source_kind: LocalDedicatedSourceKind::UserdataDefault,
                data_dir: Some(PathBuf::from("userdata/server")),
                server_address: "127.0.0.1:14006".to_string(),
                ..LocalDedicatedServer::default()
            })
        );

        assert_eq!(networking.local_dedicated_servers.len(), 2);
        assert_eq!(
            networking.local_dedicated_servers[0].display_name,
            "Pinned Local"
        );
        assert_eq!(
            networking.local_dedicated_servers[0].source_kind,
            LocalDedicatedSourceKind::UserdataDefault
        );
        assert_eq!(
            networking.local_dedicated_servers[0].server_address,
            "127.0.0.1:14004"
        );
        assert_eq!(
            networking.local_dedicated_servers[0]
                .source_server_address
                .as_deref(),
            Some("127.0.0.1:14006")
        );
        assert_eq!(
            networking.local_dedicated_servers[1].instance_id,
            unrelated_instance_id
        );
    }

    #[test]
    fn upsert_manual_local_dedicated_server_preserves_discovered_source_metadata() {
        let data_dir = PathBuf::from("userdata/server");
        let instance_id = local_instance_id(&data_dir);
        let realm_id = Uuid::new_v4();
        let mut networking = NetworkingSettings {
            local_dedicated_servers: vec![LocalDedicatedServer {
                instance_id,
                source_kind: LocalDedicatedSourceKind::UserdataDefault,
                data_dir: Some(data_dir.clone()),
                server_address: "127.0.0.1:14004".to_string(),
                connection_kind: LocalDedicatedConnectionKind::Tcp,
                last_seen_realm_id: Some(realm_id),
                last_seen_server_name: Some("Observed Local".to_string()),
                ..LocalDedicatedServer::default()
            }],
            ..NetworkingSettings::default()
        };

        let (returned_instance_id, changed) =
            networking.upsert_manual_local_dedicated_server(ManualLocalDedicatedServerSpec {
                instance_id: None,
                data_dir: Some(data_dir.clone()),
                display_name: "Pinned Local".to_string(),
                server_address: "127.0.0.1:14005".to_string(),
                connection_kind: LocalDedicatedConnectionKind::Quic,
                validate_tls: true,
            });

        assert!(changed);
        assert_eq!(returned_instance_id, instance_id);
        assert_eq!(networking.local_dedicated_servers.len(), 1);

        let entry = &networking.local_dedicated_servers[0];
        assert_eq!(entry.instance_id, instance_id);
        assert_eq!(entry.source_kind, LocalDedicatedSourceKind::UserdataDefault);
        assert_eq!(entry.data_dir.as_deref(), Some(data_dir.as_path()));
        assert_eq!(entry.display_name, "Pinned Local");
        assert_eq!(entry.server_address, "127.0.0.1:14005");
        assert_eq!(entry.connection_kind, LocalDedicatedConnectionKind::Quic);
        assert!(entry.validate_tls);
        assert_eq!(entry.last_seen_realm_id, Some(realm_id));
        assert_eq!(
            entry.source_server_address.as_deref(),
            Some("127.0.0.1:14004")
        );
        assert_eq!(
            entry.source_connection_kind,
            Some(LocalDedicatedConnectionKind::Tcp)
        );
        assert_eq!(entry.source_validate_tls, Some(false));
        assert!(entry.manual_registration);
        assert_eq!(
            entry.last_seen_server_name.as_deref(),
            Some("Observed Local")
        );
    }

    #[test]
    fn upsert_manual_local_dedicated_server_uses_data_dir_as_stable_anchor() {
        let data_dir = PathBuf::from("userdata/local-alpha");
        let expected_instance_id = local_instance_id(&data_dir);
        let mut networking = NetworkingSettings::default();

        let (first_instance_id, first_changed) =
            networking.upsert_manual_local_dedicated_server(ManualLocalDedicatedServerSpec {
                instance_id: None,
                data_dir: Some(data_dir.clone()),
                display_name: "Local Alpha".to_string(),
                server_address: "127.0.0.1:24004".to_string(),
                connection_kind: LocalDedicatedConnectionKind::Tcp,
                validate_tls: false,
            });
        let (second_instance_id, second_changed) =
            networking.upsert_manual_local_dedicated_server(ManualLocalDedicatedServerSpec {
                instance_id: None,
                data_dir: Some(data_dir.clone()),
                display_name: "Local Alpha Renamed".to_string(),
                server_address: "127.0.0.1:25004".to_string(),
                connection_kind: LocalDedicatedConnectionKind::Quic,
                validate_tls: true,
            });

        assert!(first_changed);
        assert!(second_changed);
        assert_eq!(first_instance_id, expected_instance_id);
        assert_eq!(second_instance_id, expected_instance_id);
        assert_eq!(networking.local_dedicated_servers.len(), 1);
        assert_eq!(
            networking.local_dedicated_servers[0].instance_id,
            expected_instance_id
        );
        assert_eq!(
            networking.local_dedicated_servers[0].display_name,
            "Local Alpha Renamed"
        );
        assert_eq!(
            networking.local_dedicated_servers[0].server_address,
            "127.0.0.1:25004"
        );
        assert_eq!(
            networking.local_dedicated_servers[0].connection_kind,
            LocalDedicatedConnectionKind::Quic
        );
        assert!(networking.local_dedicated_servers[0].validate_tls);
        assert!(networking.local_dedicated_servers[0].manual_registration);
    }

    #[test]
    fn discovered_source_refresh_preserves_manual_overrides_and_updates_source_snapshot() {
        let instance_id = Uuid::new_v4();
        let mut networking = NetworkingSettings {
            local_dedicated_servers: vec![LocalDedicatedServer {
                instance_id,
                source_kind: LocalDedicatedSourceKind::UserdataDefault,
                data_dir: Some(PathBuf::from("userdata/server")),
                manual_registration: true,
                source_server_address: Some("127.0.0.1:14004".to_string()),
                source_connection_kind: Some(LocalDedicatedConnectionKind::Tcp),
                source_validate_tls: Some(false),
                display_name: "Pinned Local".to_string(),
                server_address: "127.0.0.1:24004".to_string(),
                connection_kind: LocalDedicatedConnectionKind::Quic,
                validate_tls: true,
                last_seen_server_name: Some("Observed Local".to_string()),
                ..LocalDedicatedServer::default()
            }],
            ..NetworkingSettings::default()
        };

        assert!(
            networking.upsert_local_dedicated_server(LocalDedicatedServer {
                instance_id,
                source_kind: LocalDedicatedSourceKind::UserdataDefault,
                data_dir: Some(PathBuf::from("userdata/server")),
                source_server_address: Some("127.0.0.1:15004".to_string()),
                source_connection_kind: Some(LocalDedicatedConnectionKind::Tcp),
                source_validate_tls: Some(false),
                server_address: "127.0.0.1:15004".to_string(),
                connection_kind: LocalDedicatedConnectionKind::Tcp,
                validate_tls: false,
                last_seen_server_name: Some("Observed Local Refreshed".to_string()),
                ..LocalDedicatedServer::default()
            })
        );

        let entry = &networking.local_dedicated_servers[0];
        assert!(entry.manual_registration);
        assert_eq!(entry.display_name, "Pinned Local");
        assert_eq!(entry.server_address, "127.0.0.1:24004");
        assert_eq!(entry.connection_kind, LocalDedicatedConnectionKind::Quic);
        assert!(entry.validate_tls);
        assert_eq!(
            entry.source_server_address.as_deref(),
            Some("127.0.0.1:15004")
        );
        assert_eq!(
            entry.source_connection_kind,
            Some(LocalDedicatedConnectionKind::Tcp)
        );
        assert_eq!(entry.source_validate_tls, Some(false));
        assert_eq!(
            entry.last_seen_server_name.as_deref(),
            Some("Observed Local Refreshed")
        );
    }

    #[test]
    fn remove_local_dedicated_manual_registration_restores_discovered_source_defaults() {
        let instance_id = Uuid::new_v4();
        let mut networking = NetworkingSettings {
            local_dedicated_servers: vec![LocalDedicatedServer {
                instance_id,
                source_kind: LocalDedicatedSourceKind::UserdataDefault,
                manual_registration: true,
                source_server_address: Some("127.0.0.1:14004".to_string()),
                source_connection_kind: Some(LocalDedicatedConnectionKind::Tcp),
                source_validate_tls: Some(false),
                display_name: "Pinned Local".to_string(),
                server_address: "127.0.0.1:24004".to_string(),
                connection_kind: LocalDedicatedConnectionKind::Quic,
                validate_tls: true,
                last_seen_server_name: Some("Observed Local".to_string()),
                ..LocalDedicatedServer::default()
            }],
            ..NetworkingSettings::default()
        };

        assert!(networking.remove_local_dedicated_manual_registration(instance_id));

        let entry = &networking.local_dedicated_servers[0];
        assert!(!entry.manual_registration);
        assert!(entry.display_name.is_empty());
        assert_eq!(entry.server_address, "127.0.0.1:14004");
        assert_eq!(entry.connection_kind, LocalDedicatedConnectionKind::Tcp);
        assert!(!entry.validate_tls);
        assert_eq!(
            entry.last_seen_server_name.as_deref(),
            Some("Observed Local")
        );
    }

    #[test]
    fn remove_local_dedicated_manual_registration_removes_manual_only_entry_and_clears_default() {
        let instance_id = Uuid::new_v4();
        let mut networking = NetworkingSettings {
            default_local_dedicated_instance_id: Some(instance_id),
            local_dedicated_servers: vec![LocalDedicatedServer {
                instance_id,
                source_kind: LocalDedicatedSourceKind::Manual,
                manual_registration: true,
                display_name: "Pinned Local".to_string(),
                server_address: "127.0.0.1:24004".to_string(),
                connection_kind: LocalDedicatedConnectionKind::Quic,
                validate_tls: true,
                ..LocalDedicatedServer::default()
            }],
            ..NetworkingSettings::default()
        };

        assert!(networking.remove_local_dedicated_manual_registration(instance_id));
        assert!(networking.local_dedicated_servers.is_empty());
        assert_eq!(networking.default_local_dedicated_instance_id, None);
    }

    #[test]
    fn register_manual_local_dedicated_from_direct_connect_captures_transport_snapshot() {
        let mut networking = NetworkingSettings {
            use_quic: true,
            validate_tls: true,
            ..NetworkingSettings::default()
        };

        let (instance_id, changed) = networking
            .register_manual_local_dedicated_from_direct_connect("127.0.0.1:14004")
            .expect("non-empty direct connect address should register");

        assert!(changed);
        assert_eq!(networking.local_dedicated_servers.len(), 1);
        let entry = &networking.local_dedicated_servers[0];
        assert_eq!(entry.instance_id, instance_id);
        assert_eq!(entry.source_kind, LocalDedicatedSourceKind::Manual);
        assert!(entry.manual_registration);
        assert_eq!(entry.server_address, "127.0.0.1:14004");
        assert_eq!(entry.connection_kind, LocalDedicatedConnectionKind::Quic);
        assert!(entry.validate_tls);
    }

    #[test]
    fn register_manual_local_dedicated_from_direct_connect_reuses_existing_local_source() {
        let instance_id = Uuid::new_v4();
        let mut networking = NetworkingSettings {
            local_dedicated_servers: vec![LocalDedicatedServer {
                instance_id,
                source_kind: LocalDedicatedSourceKind::UserdataDefault,
                data_dir: Some(PathBuf::from("userdata/server")),
                source_server_address: Some("127.0.0.1:14004".to_string()),
                source_connection_kind: Some(LocalDedicatedConnectionKind::Tcp),
                source_validate_tls: Some(false),
                server_address: "127.0.0.1:14004".to_string(),
                connection_kind: LocalDedicatedConnectionKind::Tcp,
                validate_tls: false,
                ..LocalDedicatedServer::default()
            }],
            ..NetworkingSettings::default()
        };

        let (returned_instance_id, changed) = networking
            .register_manual_local_dedicated_from_direct_connect("127.0.0.1:14004")
            .expect("existing local source should be promotable");

        assert!(changed);
        assert_eq!(returned_instance_id, instance_id);
        assert_eq!(networking.local_dedicated_servers.len(), 1);
        assert!(networking.local_dedicated_servers[0].manual_registration);
        assert_eq!(
            networking.local_dedicated_servers[0].source_kind,
            LocalDedicatedSourceKind::UserdataDefault
        );
    }

    #[test]
    fn update_manual_local_dedicated_registration_marks_discovered_entry_as_manual_override() {
        let instance_id = Uuid::new_v4();
        let mut networking = NetworkingSettings {
            local_dedicated_servers: vec![LocalDedicatedServer {
                instance_id,
                source_kind: LocalDedicatedSourceKind::UserdataDefault,
                data_dir: Some(PathBuf::from("userdata/server")),
                source_server_address: Some("127.0.0.1:14004".to_string()),
                source_connection_kind: Some(LocalDedicatedConnectionKind::Tcp),
                source_validate_tls: Some(false),
                server_address: "127.0.0.1:14004".to_string(),
                connection_kind: LocalDedicatedConnectionKind::Tcp,
                validate_tls: false,
                ..LocalDedicatedServer::default()
            }],
            ..NetworkingSettings::default()
        };

        assert!(networking.update_manual_local_dedicated_registration(
            instance_id,
            ManualLocalDedicatedServerSpec {
                instance_id: None,
                data_dir: None,
                display_name: "Pinned Local".to_string(),
                server_address: "127.0.0.1:24004".to_string(),
                connection_kind: LocalDedicatedConnectionKind::Quic,
                validate_tls: true,
            }
        ));

        let entry = &networking.local_dedicated_servers[0];
        assert!(entry.manual_registration);
        assert_eq!(entry.display_name, "Pinned Local");
        assert_eq!(entry.server_address, "127.0.0.1:24004");
        assert_eq!(entry.connection_kind, LocalDedicatedConnectionKind::Quic);
        assert!(entry.validate_tls);
        assert_eq!(
            entry.source_server_address.as_deref(),
            Some("127.0.0.1:14004")
        );
    }

    #[test]
    fn update_manual_local_dedicated_registration_updates_manual_entry_in_place() {
        let instance_id = Uuid::new_v4();
        let mut networking = NetworkingSettings {
            local_dedicated_servers: vec![LocalDedicatedServer {
                instance_id,
                source_kind: LocalDedicatedSourceKind::Manual,
                manual_registration: true,
                display_name: "Old Local".to_string(),
                server_address: "127.0.0.1:14004".to_string(),
                connection_kind: LocalDedicatedConnectionKind::Tcp,
                validate_tls: false,
                ..LocalDedicatedServer::default()
            }],
            ..NetworkingSettings::default()
        };

        assert!(networking.update_manual_local_dedicated_registration(
            instance_id,
            ManualLocalDedicatedServerSpec {
                instance_id: None,
                data_dir: None,
                display_name: "New Local".to_string(),
                server_address: "127.0.0.1:15004".to_string(),
                connection_kind: LocalDedicatedConnectionKind::Quic,
                validate_tls: true,
            }
        ));

        let entry = &networking.local_dedicated_servers[0];
        assert_eq!(entry.display_name, "New Local");
        assert_eq!(entry.server_address, "127.0.0.1:15004");
        assert_eq!(entry.connection_kind, LocalDedicatedConnectionKind::Quic);
        assert!(entry.validate_tls);
        assert_eq!(entry.instance_id, instance_id);
    }
}
