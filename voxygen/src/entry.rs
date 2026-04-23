use crate::{
    cli::ProductModeArg,
    settings::{DevMultiplayerTargetKind, LocalDedicatedServer, NetworkingSettings, Settings},
};
use client::addr::ConnectionArgs;
use common::{
    assets::{AssetExt, Ron},
    uuid::Uuid,
};
use hashbrown::HashSet;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostKind {
    PublicOfficial,
    DevDirectConnect,
    #[cfg(feature = "singleplayer")]
    DevSingleplayer,
    DevLocalDedicated,
}

#[derive(Clone, Debug)]
pub struct ResolvedConnectHost {
    pub kind: HostKind,
    pub connection_args: ConnectionArgs,
    pub target_address: Option<String>,
    pub local_dedicated_instance_id: Option<Uuid>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevMultiplayerEntry {
    pub label: String,
    pub kind_label: String,
    pub detail: String,
    pub server_address: String,
    pub local_dedicated_instance_id: Option<Uuid>,
    pub host_kind: HostKind,
    pub can_register_local_dedicated: bool,
    pub can_delete: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OfficialEntry {
    pub display_name: String,
    pub server_address: String,
    pub auth_server: Option<String>,
    pub use_srv: bool,
    pub use_quic: bool,
    pub validate_tls: bool,
}

impl OfficialEntry {
    pub fn load() -> Self { Ron::<Self>::load_expect_cloned("voxygen.official_entry").into_inner() }

    pub fn is_configured(&self) -> bool { !self.server_address.trim().is_empty() }

    pub fn login_label(&self) -> String {
        let display_name = self.display_name.trim();
        if !display_name.is_empty() {
            display_name.to_string()
        } else {
            let server_address = self.server_address.trim();
            if server_address.is_empty() {
                "Official Realm".to_string()
            } else {
                server_address.to_string()
            }
        }
    }

    pub fn connection_args(&self) -> Result<ConnectionArgs, String> {
        let hostname = self.server_address.trim();
        if hostname.is_empty() {
            return Err(
                "Public mode is enabled, but the bundled official entry configuration does not \
                 contain a server address."
                    .to_string(),
            );
        }

        let hostname = hostname.to_string();
        Ok(if self.use_srv {
            ConnectionArgs::Srv {
                hostname,
                prefer_ipv6: false,
                validate_tls: self.validate_tls,
                use_quic: self.use_quic,
            }
        } else if self.use_quic {
            ConnectionArgs::Quic {
                hostname,
                prefer_ipv6: false,
                validate_tls: self.validate_tls,
            }
        } else {
            ConnectionArgs::Tcp {
                hostname,
                prefer_ipv6: false,
            }
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProductMode {
    Public,
    Dev,
}

impl From<ProductModeArg> for ProductMode {
    fn from(value: ProductModeArg) -> Self {
        match value {
            ProductModeArg::Public => Self::Public,
            ProductModeArg::Dev => Self::Dev,
        }
    }
}

#[derive(Clone, Debug)]
pub struct EntryPolicy {
    product_mode: ProductMode,
    official_entry: OfficialEntry,
}

impl EntryPolicy {
    pub fn load(args: &crate::cli::Args) -> Self {
        let product_mode = args.product_mode.map(Into::into).unwrap_or_else(|| {
            if cfg!(debug_assertions) {
                ProductMode::Dev
            } else {
                ProductMode::Public
            }
        });

        Self {
            product_mode,
            official_entry: OfficialEntry::load(),
        }
    }

    pub fn product_mode(&self) -> ProductMode { self.product_mode }

    pub fn is_public(&self) -> bool { matches!(self.product_mode, ProductMode::Public) }

    pub fn is_dev(&self) -> bool { matches!(self.product_mode, ProductMode::Dev) }

    pub fn public_mode_blocker_message(&self) -> Option<String> {
        if self.is_public() && !self.official_entry.is_configured() {
            Some(
                "Public mode is enabled, but the bundled official entry is not configured yet. \
                 This build is currently intended for development mode only."
                    .to_string(),
            )
        } else {
            None
        }
    }

    pub fn can_attempt_multiplayer(&self) -> bool { self.public_mode_blocker_message().is_none() }

    pub fn can_use_singleplayer(&self) -> bool { self.is_dev() }

    pub fn should_lock_server_field(&self, cli_server: Option<&str>) -> bool {
        self.is_public() || cli_server.is_some()
    }

    pub fn can_unlock_server_field(&self, cli_server: Option<&str>) -> bool {
        self.is_dev() && cli_server.is_some()
    }

    pub fn can_show_server_list(&self) -> bool { self.is_dev() }

    pub fn can_manage_server_history(&self) -> bool { self.is_dev() }

    pub fn multiplayer_host_kind(&self) -> HostKind {
        match self.product_mode {
            ProductMode::Public => HostKind::PublicOfficial,
            ProductMode::Dev => HostKind::DevDirectConnect,
        }
    }

    pub fn initial_multiplayer_host_kind(
        &self,
        settings: &Settings,
        cli_server: Option<&str>,
    ) -> HostKind {
        match self.product_mode {
            ProductMode::Public => HostKind::PublicOfficial,
            ProductMode::Dev => {
                if cli_server.is_some() {
                    return HostKind::DevDirectConnect;
                }

                if matches!(
                    settings.networking.default_multiplayer_target_kind,
                    DevMultiplayerTargetKind::LocalDedicated
                ) && self
                    .preferred_dev_local_dedicated_server(settings)
                    .is_some()
                {
                    HostKind::DevLocalDedicated
                } else if settings.networking.default_server.trim().is_empty()
                    && self
                        .preferred_dev_local_dedicated_server(settings)
                        .is_some()
                {
                    HostKind::DevLocalDedicated
                } else {
                    HostKind::DevDirectConnect
                }
            },
        }
    }

    pub fn dev_multiplayer_entries(
        &self,
        settings: &Settings,
        cli_server: Option<&str>,
    ) -> Vec<DevMultiplayerEntry> {
        if !self.is_dev() || cli_server.is_some() {
            return Vec::new();
        }

        let mut entries = settings
            .networking
            .servers
            .iter()
            .filter_map(|server_address| {
                let server_address = server_address.trim();
                (!server_address.is_empty()).then(|| DevMultiplayerEntry {
                    label: server_address.to_string(),
                    kind_label: "Direct Connect".to_string(),
                    detail: "Dev history entry | Register Local available".to_string(),
                    server_address: server_address.to_string(),
                    local_dedicated_instance_id: None,
                    host_kind: HostKind::DevDirectConnect,
                    can_register_local_dedicated: true,
                    can_delete: true,
                })
            })
            .collect::<Vec<_>>();

        entries.extend(
            settings
                .networking
                .local_dedicated_servers
                .iter()
                .filter_map(|entry| {
                    let server_address = entry.server_address.trim();
                    if server_address.is_empty() {
                        return None;
                    }
                    Some(DevMultiplayerEntry {
                        label: entry.label(),
                        kind_label: "Local Dedicated".to_string(),
                        detail: local_dedicated_inventory_detail(entry),
                        server_address: server_address.to_string(),
                        local_dedicated_instance_id: Some(entry.instance_id),
                        host_kind: HostKind::DevLocalDedicated,
                        can_register_local_dedicated: false,
                        can_delete: entry.has_manual_registration(),
                    })
                }),
        );

        entries
    }

    pub fn initial_server_field_value(
        &self,
        settings: &Settings,
        cli_server: Option<&str>,
    ) -> String {
        match self.product_mode {
            ProductMode::Public => {
                if self.public_mode_blocker_message().is_some() {
                    "Official Entry Unavailable".to_string()
                } else {
                    self.official_entry.login_label()
                }
            },
            ProductMode::Dev => {
                if let Some(cli_server) = cli_server {
                    cli_server.to_string()
                } else if matches!(
                    settings.networking.default_multiplayer_target_kind,
                    DevMultiplayerTargetKind::LocalDedicated
                ) {
                    self.preferred_dev_local_dedicated_server(settings)
                        .map(|entry| entry.server_address.clone())
                        .unwrap_or_else(|| settings.networking.default_server.clone())
                } else if settings.networking.default_server.trim().is_empty() {
                    self.preferred_dev_local_dedicated_server(settings)
                        .map(|entry| entry.server_address.clone())
                        .unwrap_or_default()
                } else {
                    settings.networking.default_server.clone()
                }
            },
        }
    }

    pub fn apply_login_settings(
        &self,
        networking: &mut NetworkingSettings,
        host_kind: HostKind,
        username: &str,
        requested_server: &str,
        local_dedicated_instance_id: Option<Uuid>,
    ) {
        networking.username = username.to_string();

        if self.is_dev() {
            networking.default_server = requested_server.to_string();
            networking.default_multiplayer_target_kind = match host_kind {
                HostKind::DevLocalDedicated => DevMultiplayerTargetKind::LocalDedicated,
                HostKind::DevDirectConnect | HostKind::PublicOfficial => {
                    DevMultiplayerTargetKind::DirectConnect
                },
                #[cfg(feature = "singleplayer")]
                HostKind::DevSingleplayer => DevMultiplayerTargetKind::DirectConnect,
            };
            networking.default_local_dedicated_instance_id =
                matches!(host_kind, HostKind::DevLocalDedicated)
                    .then_some(local_dedicated_instance_id)
                    .flatten();
            if networking.default_local_dedicated_instance_id.is_none()
                && matches!(host_kind, HostKind::DevLocalDedicated)
            {
                networking.default_local_dedicated_instance_id = networking
                    .local_dedicated_server(requested_server)
                    .map(|entry| entry.instance_id);
            }

            if matches!(host_kind, HostKind::DevDirectConnect)
                && !requested_server.is_empty()
                && !networking
                    .servers
                    .iter()
                    .any(|server| server == requested_server)
            {
                networking.servers.push(requested_server.to_string());
            }
        }
    }

    pub fn resolve_multiplayer_host(
        &self,
        host_kind: HostKind,
        requested_server: &str,
        local_dedicated_instance_id: Option<Uuid>,
        networking: &NetworkingSettings,
    ) -> Result<ResolvedConnectHost, String> {
        if let Some(message) = self.public_mode_blocker_message() {
            return Err(message);
        }

        let connection_args = match self.product_mode {
            ProductMode::Public => self.official_entry.connection_args(),
            ProductMode::Dev => {
                match host_kind {
                    HostKind::DevDirectConnect => {},
                    HostKind::DevLocalDedicated => {
                        let Some(entry) = self.configured_local_dedicated_entry(
                            networking,
                            requested_server,
                            local_dedicated_instance_id,
                        ) else {
                            return Err("Selected local dedicated instance is no longer \
                                        available in the configured local dedicated source list."
                                .to_string());
                        };
                        return Ok(ResolvedConnectHost {
                            kind: HostKind::DevLocalDedicated,
                            connection_args: entry.connection_args(),
                            target_address: Some(entry.server_address.trim().to_string()),
                            local_dedicated_instance_id: Some(entry.instance_id),
                        });
                    },
                    HostKind::PublicOfficial => {
                        return Err("Public official host kind cannot be used for \
                                    development-mode multiplayer."
                            .to_string());
                    },
                    #[cfg(feature = "singleplayer")]
                    HostKind::DevSingleplayer => {
                        return Err("Singleplayer host kind cannot be used for multiplayer \
                                    connection."
                            .to_string());
                    },
                }

                let hostname = requested_server.trim();
                if hostname.is_empty() {
                    return Err("Server address cannot be empty.".to_string());
                }

                let hostname = hostname.to_string();
                Ok(if networking.use_srv {
                    ConnectionArgs::Srv {
                        hostname,
                        prefer_ipv6: false,
                        validate_tls: networking.validate_tls,
                        use_quic: networking.use_quic,
                    }
                } else if networking.use_quic {
                    ConnectionArgs::Quic {
                        hostname,
                        prefer_ipv6: false,
                        validate_tls: networking.validate_tls,
                    }
                } else {
                    ConnectionArgs::Tcp {
                        hostname,
                        prefer_ipv6: false,
                    }
                })
            },
        }?;

        let kind = match self.product_mode {
            ProductMode::Public => HostKind::PublicOfficial,
            ProductMode::Dev => host_kind,
        };

        Ok(ResolvedConnectHost {
            kind,
            connection_args,
            target_address: match self.product_mode {
                ProductMode::Public => Some(self.official_entry.server_address.trim().to_string()),
                ProductMode::Dev => Some(requested_server.trim().to_string()),
            },
            local_dedicated_instance_id: None,
        })
    }

    pub fn connection_args(
        &self,
        host_kind: HostKind,
        requested_server: &str,
        local_dedicated_instance_id: Option<Uuid>,
        networking: &NetworkingSettings,
    ) -> Result<ConnectionArgs, String> {
        self.resolve_multiplayer_host(
            host_kind,
            requested_server,
            local_dedicated_instance_id,
            networking,
        )
        .map(|host| host.connection_args)
    }

    fn configured_local_dedicated_entry<'a>(
        &self,
        networking: &'a NetworkingSettings,
        server_address: &str,
        local_dedicated_instance_id: Option<Uuid>,
    ) -> Option<&'a LocalDedicatedServer> {
        local_dedicated_instance_id
            .and_then(|instance_id| networking.local_dedicated_server_by_instance_id(instance_id))
            .or_else(|| networking.local_dedicated_server(server_address))
    }

    fn preferred_dev_local_dedicated_server<'a>(
        &self,
        settings: &'a Settings,
    ) -> Option<&'a LocalDedicatedServer> {
        settings
            .networking
            .default_local_dedicated_server()
            .or_else(|| {
                settings
                    .networking
                    .local_dedicated_servers
                    .iter()
                    .find(|entry| {
                        !entry.server_address.trim().is_empty()
                            && matches!(
                                entry.source_kind,
                                crate::settings::LocalDedicatedSourceKind::UserdataDefault
                            )
                    })
            })
            .or_else(|| {
                settings
                    .networking
                    .local_dedicated_servers
                    .iter()
                    .find(|entry| !entry.server_address.trim().is_empty())
            })
    }

    pub fn is_auth_server_trusted(
        &self,
        host_kind: HostKind,
        auth_server: &str,
        trusted_auth_servers: &HashSet<String>,
    ) -> bool {
        match host_kind {
            HostKind::PublicOfficial => self
                .official_entry
                .auth_server
                .as_deref()
                .is_some_and(|expected| expected == auth_server),
            HostKind::DevDirectConnect | HostKind::DevLocalDedicated => {
                trusted_auth_servers.contains(auth_server)
            },
            #[cfg(feature = "singleplayer")]
            HostKind::DevSingleplayer => false,
        }
    }

    pub fn should_prompt_for_auth_trust(&self, host_kind: HostKind) -> bool {
        matches!(
            host_kind,
            HostKind::DevDirectConnect | HostKind::DevLocalDedicated
        )
    }

    pub fn should_persist_auth_trust(&self, host_kind: HostKind) -> bool {
        matches!(
            host_kind,
            HostKind::DevDirectConnect | HostKind::DevLocalDedicated
        )
    }
}

fn local_dedicated_inventory_detail(entry: &LocalDedicatedServer) -> String {
    let mut parts = vec![match entry.source_kind {
        crate::settings::LocalDedicatedSourceKind::Manual => "Manual source".to_string(),
        crate::settings::LocalDedicatedSourceKind::UserdataDefault => {
            "userdata/server source".to_string()
        },
    }];

    if entry.manual_registration
        && !matches!(
            entry.source_kind,
            crate::settings::LocalDedicatedSourceKind::Manual
        )
    {
        parts.push("manual override".to_string());
    }

    if let Some(data_dir) = entry.data_dir.as_ref() {
        let data_dir = data_dir.to_string_lossy().trim().to_string();
        if !data_dir.is_empty() {
            parts.push(data_dir);
        }
    }

    parts.join(" | ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn official_entry(server_address: &str, auth_server: Option<&str>) -> OfficialEntry {
        OfficialEntry {
            display_name: "Official Realm".to_string(),
            server_address: server_address.to_string(),
            auth_server: auth_server.map(ToOwned::to_owned),
            use_srv: false,
            use_quic: false,
            validate_tls: true,
        }
    }

    fn entry_policy(product_mode: ProductMode, official_entry: OfficialEntry) -> EntryPolicy {
        EntryPolicy {
            product_mode,
            official_entry,
        }
    }

    #[test]
    fn public_mode_blocks_when_official_entry_missing() {
        let policy = entry_policy(ProductMode::Public, official_entry("", None));

        assert!(policy.public_mode_blocker_message().is_some());
    }

    #[test]
    fn public_mode_uses_official_connection_args() {
        let policy = entry_policy(
            ProductMode::Public,
            official_entry("192.168.1.8:14004", Some("auth.example.test")),
        );
        let settings = NetworkingSettings::default();

        let host = policy
            .resolve_multiplayer_host(
                HostKind::PublicOfficial,
                "ignored.example.test",
                None,
                &settings,
            )
            .expect("public mode should resolve to official entry");

        assert_eq!(host.kind, HostKind::PublicOfficial);

        match host.connection_args {
            ConnectionArgs::Tcp { hostname, .. } => {
                assert_eq!(hostname, "192.168.1.8:14004");
            },
            other => panic!("unexpected connection args: {other:?}"),
        }
    }

    #[test]
    fn public_mode_ignores_cli_server_for_initial_value() {
        let policy = entry_policy(
            ProductMode::Public,
            official_entry("192.168.1.8:14004", Some("auth.example.test")),
        );
        let settings = Settings::default();

        let server = policy.initial_server_field_value(&settings, Some("203.0.113.20:14004"));

        assert_eq!(server, "Official Realm");
    }

    #[test]
    fn dev_mode_uses_cli_server_for_initial_value() {
        let policy = entry_policy(ProductMode::Dev, official_entry("192.168.1.8:14004", None));
        let settings = Settings::default();

        let server = policy.initial_server_field_value(&settings, Some("203.0.113.20:14004"));

        assert_eq!(server, "203.0.113.20:14004");
    }

    #[test]
    fn public_mode_uses_unavailable_label_when_official_entry_missing() {
        let policy = entry_policy(ProductMode::Public, official_entry("", None));
        let settings = Settings::default();

        let server = policy.initial_server_field_value(&settings, None);

        assert_eq!(server, "Official Entry Unavailable");
    }

    #[test]
    fn server_field_lock_rules_match_public_and_dev_semantics() {
        let public_policy = entry_policy(
            ProductMode::Public,
            official_entry("192.168.1.8:14004", None),
        );
        let dev_policy = entry_policy(ProductMode::Dev, official_entry("192.168.1.8:14004", None));

        assert!(public_policy.should_lock_server_field(None));
        assert!(!public_policy.can_unlock_server_field(Some("203.0.113.20:14004")));

        assert!(!dev_policy.should_lock_server_field(None));
        assert!(dev_policy.should_lock_server_field(Some("203.0.113.20:14004")));
        assert!(dev_policy.can_unlock_server_field(Some("203.0.113.20:14004")));
        assert!(!dev_policy.can_unlock_server_field(None));
    }

    #[test]
    fn public_mode_disables_multiplayer_when_official_entry_missing() {
        let policy = entry_policy(ProductMode::Public, official_entry("", None));

        assert!(!policy.can_attempt_multiplayer());
    }

    #[test]
    fn dev_mode_uses_requested_server_and_persists_history() {
        let policy = entry_policy(ProductMode::Dev, official_entry("192.168.1.8:14004", None));
        let mut networking = NetworkingSettings::default();

        policy.apply_login_settings(
            &mut networking,
            HostKind::DevDirectConnect,
            "tester",
            "10.0.0.2:14004",
            None,
        );

        assert_eq!(networking.username, "tester");
        assert_eq!(networking.default_server, "10.0.0.2:14004");
        assert_eq!(networking.default_local_dedicated_instance_id, None);
        assert_eq!(
            networking.default_multiplayer_target_kind,
            DevMultiplayerTargetKind::DirectConnect
        );
        assert_eq!(networking.servers, vec!["10.0.0.2:14004".to_string()]);
    }

    #[test]
    fn public_mode_does_not_persist_server_history() {
        let policy = entry_policy(
            ProductMode::Public,
            official_entry("192.168.1.8:14004", None),
        );
        let mut networking = NetworkingSettings {
            username: "old-user".to_string(),
            servers: vec!["10.0.0.2:14004".to_string()],
            default_server: "10.0.0.2:14004".to_string(),
            ..NetworkingSettings::default()
        };

        policy.apply_login_settings(
            &mut networking,
            HostKind::PublicOfficial,
            "tester",
            "203.0.113.5:14004",
            None,
        );

        assert_eq!(networking.username, "tester");
        assert_eq!(networking.default_server, "10.0.0.2:14004");
        assert_eq!(networking.default_local_dedicated_instance_id, None);
        assert_eq!(networking.servers, vec!["10.0.0.2:14004".to_string()]);
    }

    #[test]
    fn public_mode_only_trusts_official_auth_server() {
        let policy = entry_policy(
            ProductMode::Public,
            official_entry("192.168.1.8:14004", Some("auth.official.test")),
        );
        let mut trusted_auth_servers = HashSet::new();
        trusted_auth_servers.insert("auth.official.test".to_string());
        trusted_auth_servers.insert("auth.other.test".to_string());

        assert!(policy.is_auth_server_trusted(
            HostKind::PublicOfficial,
            "auth.official.test",
            &trusted_auth_servers
        ));
        assert!(!policy.is_auth_server_trusted(
            HostKind::PublicOfficial,
            "auth.other.test",
            &trusted_auth_servers
        ));
    }

    #[test]
    fn dev_mode_respects_saved_auth_trust_list() {
        let policy = entry_policy(ProductMode::Dev, official_entry("192.168.1.8:14004", None));
        let mut trusted_auth_servers = HashSet::new();
        trusted_auth_servers.insert("auth.dev.test".to_string());

        assert!(policy.is_auth_server_trusted(
            HostKind::DevDirectConnect,
            "auth.dev.test",
            &trusted_auth_servers
        ));
        assert!(!policy.is_auth_server_trusted(
            HostKind::DevDirectConnect,
            "auth.unknown.test",
            &trusted_auth_servers
        ));
    }

    #[test]
    fn multiplayer_host_kind_matches_product_mode() {
        let public_policy = entry_policy(
            ProductMode::Public,
            official_entry("192.168.1.8:14004", None),
        );
        let dev_policy = entry_policy(ProductMode::Dev, official_entry("192.168.1.8:14004", None));

        assert_eq!(
            public_policy.multiplayer_host_kind(),
            HostKind::PublicOfficial
        );
        assert_eq!(
            dev_policy.multiplayer_host_kind(),
            HostKind::DevDirectConnect
        );
    }

    #[test]
    fn dev_local_dedicated_does_not_pollute_direct_history() {
        let policy = entry_policy(ProductMode::Dev, official_entry("192.168.1.8:14004", None));
        let instance_id = common::uuid::Uuid::new_v4();
        let mut networking = NetworkingSettings {
            local_dedicated_servers: vec![LocalDedicatedServer {
                instance_id,
                display_name: "Local Dev Realm".to_string(),
                server_address: "127.0.0.1:14004".to_string(),
                ..LocalDedicatedServer::default()
            }],
            ..NetworkingSettings::default()
        };

        policy.apply_login_settings(
            &mut networking,
            HostKind::DevLocalDedicated,
            "tester",
            "127.0.0.1:14004",
            Some(instance_id),
        );

        assert_eq!(networking.default_server, "127.0.0.1:14004");
        assert_eq!(
            networking.default_local_dedicated_instance_id,
            Some(instance_id)
        );
        assert_eq!(
            networking.default_multiplayer_target_kind,
            DevMultiplayerTargetKind::LocalDedicated
        );
        assert!(networking.servers.is_empty());
    }

    #[test]
    fn initial_dev_multiplayer_host_kind_uses_configured_local_dedicated_target() {
        let policy = entry_policy(ProductMode::Dev, official_entry("192.168.1.8:14004", None));
        let mut settings = Settings::default();
        settings.networking.default_server = "127.0.0.1:14004".to_string();
        settings.networking.default_multiplayer_target_kind =
            DevMultiplayerTargetKind::LocalDedicated;
        settings.networking.local_dedicated_servers = vec![LocalDedicatedServer {
            display_name: "Local Dev Realm".to_string(),
            server_address: "127.0.0.1:14004".to_string(),
            ..LocalDedicatedServer::default()
        }];

        assert_eq!(
            policy.initial_multiplayer_host_kind(&settings, None),
            HostKind::DevLocalDedicated
        );
    }

    #[test]
    fn initial_dev_local_dedicated_target_survives_address_drift_via_instance_id() {
        let policy = entry_policy(ProductMode::Dev, official_entry("192.168.1.8:14004", None));
        let mut settings = Settings::default();
        let instance_id = common::uuid::Uuid::new_v4();
        settings.networking.default_server = "127.0.0.1:9999".to_string();
        settings.networking.default_local_dedicated_instance_id = Some(instance_id);
        settings.networking.default_multiplayer_target_kind =
            DevMultiplayerTargetKind::LocalDedicated;
        settings.networking.local_dedicated_servers = vec![LocalDedicatedServer {
            instance_id,
            display_name: "Local Dev Realm".to_string(),
            server_address: "127.0.0.1:14004".to_string(),
            ..LocalDedicatedServer::default()
        }];

        assert_eq!(
            policy.initial_multiplayer_host_kind(&settings, None),
            HostKind::DevLocalDedicated
        );
        assert_eq!(
            policy.initial_server_field_value(&settings, None),
            "127.0.0.1:14004"
        );
    }

    #[test]
    fn dev_mode_prefers_discovered_local_dedicated_when_no_direct_default_exists() {
        let policy = entry_policy(ProductMode::Dev, official_entry("192.168.1.8:14004", None));
        let mut settings = Settings::default();
        settings.networking.default_server = String::new();
        settings.networking.default_multiplayer_target_kind =
            DevMultiplayerTargetKind::DirectConnect;
        settings.networking.local_dedicated_servers = vec![LocalDedicatedServer {
            source_kind: crate::settings::LocalDedicatedSourceKind::UserdataDefault,
            display_name: "Local Dev Realm".to_string(),
            server_address: "127.0.0.1:14004".to_string(),
            ..LocalDedicatedServer::default()
        }];

        assert_eq!(
            policy.initial_multiplayer_host_kind(&settings, None),
            HostKind::DevLocalDedicated
        );
        assert_eq!(
            policy.initial_server_field_value(&settings, None),
            "127.0.0.1:14004"
        );
    }

    #[test]
    fn dev_multiplayer_entries_keep_direct_and_local_dedicated_semantics_separate() {
        let policy = entry_policy(ProductMode::Dev, official_entry("192.168.1.8:14004", None));
        let mut settings = Settings::default();
        settings.networking.servers = vec!["10.0.0.2:14004".to_string()];
        settings.networking.local_dedicated_servers = vec![LocalDedicatedServer {
            source_kind: crate::settings::LocalDedicatedSourceKind::UserdataDefault,
            display_name: "Local Dev Realm".to_string(),
            server_address: "127.0.0.1:14004".to_string(),
            ..LocalDedicatedServer::default()
        }];

        let entries = policy.dev_multiplayer_entries(&settings, None);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].host_kind, HostKind::DevDirectConnect);
        assert_eq!(entries[0].kind_label, "Direct Connect");
        assert!(entries[0].detail.contains("Register Local available"));
        assert!(entries[0].can_register_local_dedicated);
        assert!(entries[0].can_delete);
        assert_eq!(entries[1].host_kind, HostKind::DevLocalDedicated);
        assert_eq!(entries[1].kind_label, "Local Dedicated");
        assert!(entries[1].detail.contains("userdata/server source"));
        assert!(!entries[1].can_register_local_dedicated);
        assert!(!entries[1].can_delete);
        assert_eq!(entries[1].server_address, "127.0.0.1:14004");
    }

    #[test]
    fn manual_local_dedicated_entries_are_deletable_but_discovered_only_entries_are_not() {
        let policy = entry_policy(ProductMode::Dev, official_entry("192.168.1.8:14004", None));
        let mut settings = Settings::default();
        settings.networking.local_dedicated_servers = vec![
            LocalDedicatedServer {
                source_kind: crate::settings::LocalDedicatedSourceKind::UserdataDefault,
                manual_registration: true,
                server_address: "127.0.0.1:14004".to_string(),
                ..LocalDedicatedServer::default()
            },
            LocalDedicatedServer {
                source_kind: crate::settings::LocalDedicatedSourceKind::UserdataDefault,
                server_address: "127.0.0.1:15004".to_string(),
                ..LocalDedicatedServer::default()
            },
        ];

        let entries = policy.dev_multiplayer_entries(&settings, None);

        assert_eq!(entries.len(), 2);
        assert!(entries[0].detail.contains("manual override"));
        assert!(entries[1].detail.contains("userdata/server source"));
        assert!(!entries[0].can_register_local_dedicated);
        assert!(entries[0].can_delete);
        assert!(!entries[1].can_register_local_dedicated);
        assert!(!entries[1].can_delete);
    }

    #[test]
    fn local_dedicated_entries_can_fall_back_to_observed_server_name() {
        let policy = entry_policy(ProductMode::Dev, official_entry("192.168.1.8:14004", None));
        let mut settings = Settings::default();
        settings.networking.local_dedicated_servers = vec![LocalDedicatedServer {
            server_address: "127.0.0.1:14004".to_string(),
            last_seen_server_name: Some("Observed Realm".to_string()),
            ..LocalDedicatedServer::default()
        }];

        let entries = policy.dev_multiplayer_entries(&settings, None);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].label, "Observed Realm (127.0.0.1:14004)");
    }

    #[test]
    fn dev_mode_resolves_local_dedicated_host_kind_explicitly() {
        let policy = entry_policy(ProductMode::Dev, official_entry("192.168.1.8:14004", None));
        let settings = NetworkingSettings {
            local_dedicated_servers: vec![LocalDedicatedServer {
                server_address: "127.0.0.1:14004".to_string(),
                ..LocalDedicatedServer::default()
            }],
            ..NetworkingSettings::default()
        };

        let host = policy
            .resolve_multiplayer_host(
                HostKind::DevLocalDedicated,
                "127.0.0.1:14004",
                None,
                &settings,
            )
            .expect("local dedicated dev host should resolve");

        assert_eq!(host.kind, HostKind::DevLocalDedicated);
        assert_eq!(
            host.local_dedicated_instance_id,
            Some(settings.local_dedicated_servers[0].instance_id)
        );
    }

    #[test]
    fn local_dedicated_resolution_uses_entry_transport_instead_of_dev_direct_preferences() {
        let policy = entry_policy(ProductMode::Dev, official_entry("192.168.1.8:14004", None));
        let settings = NetworkingSettings {
            use_srv: true,
            local_dedicated_servers: vec![LocalDedicatedServer {
                server_address: "127.0.0.1:14004".to_string(),
                connection_kind: crate::settings::LocalDedicatedConnectionKind::Tcp,
                ..LocalDedicatedServer::default()
            }],
            ..NetworkingSettings::default()
        };

        let host = policy
            .resolve_multiplayer_host(
                HostKind::DevLocalDedicated,
                "127.0.0.1:14004",
                None,
                &settings,
            )
            .expect("local dedicated dev host should resolve");

        match host.connection_args {
            ConnectionArgs::Tcp { hostname, .. } => assert_eq!(hostname, "127.0.0.1:14004"),
            other => panic!("unexpected connection args: {other:?}"),
        }
    }

    #[test]
    fn auth_trust_prompt_semantics_follow_host_kind() {
        let policy = entry_policy(ProductMode::Dev, official_entry("192.168.1.8:14004", None));

        assert!(policy.should_prompt_for_auth_trust(HostKind::DevDirectConnect));
        assert!(policy.should_persist_auth_trust(HostKind::DevLocalDedicated));
        assert!(!policy.should_prompt_for_auth_trust(HostKind::PublicOfficial));
    }
}
