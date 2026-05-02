use crate::{
    cli::ProductModeArg,
    settings::{DevMultiplayerTargetKind, LocalDedicatedServer, NetworkingSettings, Settings},
};
use client::addr::ConnectionArgs;
use common::{
    public_realm::{
        BundledPublicRealmAuthMode, BundledPublicRealmTargetKind, BundledPublicRealmTransportKind,
        PublicRealm, PublicRealmSourceKind,
    },
    uuid::Uuid,
};
use common_net::msg::ServerAuthMode;
use hashbrown::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostKind {
    PublicRealm,
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

trait PublicRealmExt {
    fn connection_args(&self) -> Result<ConnectionArgs, String>;
}

impl PublicRealmExt for PublicRealm {
    fn connection_args(&self) -> Result<ConnectionArgs, String> {
        let hostname = self.server_address.trim();
        if hostname.is_empty() {
            return Err(
                "Public mode is enabled, but the bundled Caldrayne Realm configuration does not \
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
pub enum PublicRealmClientMetadataSurface {
    RealmLabelAndAddress,
    RealmStatus,
    Announcement,
    News,
    ProductHallMetadata,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicRealmClientAuthorityBoundary {
    AccountRouting,
    AuthTrustOverrides,
    FreeServerSelection,
    ArbitraryRealmDirectory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicRealmClientMetadataBoundaryContract {
    pub may_render: Vec<PublicRealmClientMetadataSurface>,
    pub must_not_become_authority_for: Vec<PublicRealmClientAuthorityBoundary>,
    pub requires_explicit_integration: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightweightDiscoverySurface {
    RealmIdentityHint,
    EnvironmentHint,
    AuthRequirementHint,
    CompatibilityHint,
    PopulationSnapshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightweightDiscoveryAuthorityBoundary {
    PublicRealmTargeting,
    MainHandshakeRealmIdentity,
    MainHandshakeEnvironmentTruth,
    MainHandshakeAuthRequirement,
    DirectoryOrReleaseControlPlane,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LightweightDiscoveryBoundaryContract {
    pub may_expose: Vec<LightweightDiscoverySurface>,
    pub must_not_become_authority_for: Vec<LightweightDiscoveryAuthorityBoundary>,
    pub remains_optional_observability_plane: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryUpgradeBoundaryContract {
    pub public_realm_source_kind: PublicRealmSourceKind,
    pub public_realm_source_role: &'static str,
    pub launcher_role: &'static str,
    pub launcher_may_replace_public_realm_source: bool,
    pub launcher_must_not_override_runtime_policy: bool,
    pub client_metadata_boundary: PublicRealmClientMetadataBoundaryContract,
    pub lightweight_discovery_boundary: LightweightDiscoveryBoundaryContract,
    pub forbidden_runtime_overrides: Vec<&'static str>,
    pub must_remain_external: Vec<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicRealmAuthTrustOutcome {
    TrustedExactBundledProvider,
    RejectedMissingBundledProvider,
    RejectedProviderMismatch,
}

impl PublicRealmAuthTrustOutcome {
    pub fn is_trusted(self) -> bool { matches!(self, Self::TrustedExactBundledProvider) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicRealmAuthHandoffContract {
    pub bundled_server_address_configured: bool,
    pub bundled_public_realm_artifact_identity: String,
    pub bundled_target_kind: BundledPublicRealmTargetKind,
    pub bundled_target_is_non_local_candidate: bool,
    pub bundled_transport_kind: BundledPublicRealmTransportKind,
    pub bundled_use_srv: bool,
    pub bundled_use_quic: bool,
    pub bundled_validate_tls: bool,
    pub bundled_auth_mode: ServerAuthMode,
    pub bundled_auth_provider_url: Option<String>,
    pub public_target_authority: &'static str,
    pub public_auth_authority: &'static str,
    pub exact_provider_match_required: bool,
    pub prompt_for_manual_trust: bool,
    pub persist_manual_trust: bool,
    pub external_auth_rollout_ready: bool,
    pub non_local_cutover_ready: bool,
    pub non_local_cutover_gap_reasons: Vec<&'static str>,
    pub rollout_readiness_scope: &'static str,
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
    public_realm: PublicRealm,
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
            public_realm: PublicRealm::load(),
        }
    }

    pub fn product_mode(&self) -> ProductMode { self.product_mode }

    pub fn is_public(&self) -> bool { matches!(self.product_mode, ProductMode::Public) }

    pub fn is_dev(&self) -> bool { matches!(self.product_mode, ProductMode::Dev) }

    pub fn public_mode_blocker_message(&self) -> Option<String> {
        if self.is_public() && !self.public_realm.is_configured() {
            Some(
                "Public mode is enabled, but the bundled Caldrayne Realm is not configured yet. \
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

    pub fn upgrade_boundary_contract(&self) -> EntryUpgradeBoundaryContract {
        EntryUpgradeBoundaryContract {
            public_realm_source_kind: self.public_realm.source_kind,
            public_realm_source_role:
                "single bundled Caldrayne Realm source for public-mode realm targeting",
            launcher_role: "may deliver or replace the bundled Caldrayne Realm source, but must \
                            not bypass the entry policy layer",
            launcher_may_replace_public_realm_source: true,
            launcher_must_not_override_runtime_policy: true,
            client_metadata_boundary: PublicRealmClientMetadataBoundaryContract {
                may_render: vec![
                    PublicRealmClientMetadataSurface::RealmLabelAndAddress,
                    PublicRealmClientMetadataSurface::RealmStatus,
                    PublicRealmClientMetadataSurface::Announcement,
                    PublicRealmClientMetadataSurface::News,
                    PublicRealmClientMetadataSurface::ProductHallMetadata,
                ],
                must_not_become_authority_for: vec![
                    PublicRealmClientAuthorityBoundary::AccountRouting,
                    PublicRealmClientAuthorityBoundary::AuthTrustOverrides,
                    PublicRealmClientAuthorityBoundary::FreeServerSelection,
                    PublicRealmClientAuthorityBoundary::ArbitraryRealmDirectory,
                ],
                requires_explicit_integration: true,
            },
            lightweight_discovery_boundary: LightweightDiscoveryBoundaryContract {
                may_expose: vec![
                    LightweightDiscoverySurface::RealmIdentityHint,
                    LightweightDiscoverySurface::EnvironmentHint,
                    LightweightDiscoverySurface::AuthRequirementHint,
                    LightweightDiscoverySurface::CompatibilityHint,
                    LightweightDiscoverySurface::PopulationSnapshot,
                ],
                must_not_become_authority_for: vec![
                    LightweightDiscoveryAuthorityBoundary::PublicRealmTargeting,
                    LightweightDiscoveryAuthorityBoundary::MainHandshakeRealmIdentity,
                    LightweightDiscoveryAuthorityBoundary::MainHandshakeEnvironmentTruth,
                    LightweightDiscoveryAuthorityBoundary::MainHandshakeAuthRequirement,
                    LightweightDiscoveryAuthorityBoundary::DirectoryOrReleaseControlPlane,
                ],
                remains_optional_observability_plane: true,
            },
            forbidden_runtime_overrides: vec![
                "public-mode server target via --server",
                "public-mode target derived from settings.networking.default_server",
                "public-mode target derived from settings.networking.servers history",
                "public-mode target derived from lightweight discovery or query-server responses",
                "public-mode auth trust sourced from arbitrary local trust cache entries",
            ],
            must_remain_external: vec![
                "patching and binary repair orchestration",
                "anti-cheat bootstrap or preflight enforcement",
                "release-channel selection and staged rollout control",
            ],
        }
    }

    pub fn public_realm_auth_handoff_contract(&self) -> PublicRealmAuthHandoffContract {
        let posture = self.public_realm.posture();
        let bundled_server_address_configured = posture.server_address_configured;
        let bundled_public_realm_artifact_identity = posture.artifact_identity.clone();
        let bundled_target_kind = posture.target_kind;
        let bundled_target_is_non_local_candidate = posture.target_is_non_local_candidate;
        let bundled_transport_kind = posture.transport_kind;
        let bundled_use_srv = posture.use_srv;
        let bundled_use_quic = posture.use_quic;
        let bundled_validate_tls = posture.validate_tls;
        let bundled_auth_mode = match posture.auth_mode {
            BundledPublicRealmAuthMode::ExternalProvider => ServerAuthMode::ExternalProvider,
            BundledPublicRealmAuthMode::NoExternalAuth => ServerAuthMode::NoExternalAuth,
        };
        let bundled_auth_provider_url = posture.auth_server.clone();
        let external_auth_rollout_ready =
            bundled_server_address_configured && posture.auth_mode.requires_external_auth();
        let non_local_cutover_ready = posture.non_local_cutover_ready;
        let non_local_cutover_gap_reasons = posture.non_local_cutover_gap_reasons.clone();
        let rollout_readiness_scope = posture.rollout_readiness_scope;

        PublicRealmAuthHandoffContract {
            bundled_server_address_configured,
            bundled_public_realm_artifact_identity,
            bundled_target_kind,
            bundled_target_is_non_local_candidate,
            bundled_transport_kind,
            bundled_use_srv,
            bundled_use_quic,
            bundled_validate_tls,
            bundled_auth_mode,
            bundled_auth_provider_url,
            public_target_authority: "bundled Caldrayne Realm server address -> EntryPolicy -> \
                                      realm handshake",
            public_auth_authority: "bundled Caldrayne Realm auth pin exact-match -> PublicRealm \
                                    auth trust",
            exact_provider_match_required: true,
            prompt_for_manual_trust: false,
            persist_manual_trust: false,
            external_auth_rollout_ready,
            non_local_cutover_ready,
            non_local_cutover_gap_reasons,
            rollout_readiness_scope,
        }
    }

    pub fn multiplayer_host_kind(&self) -> HostKind {
        match self.product_mode {
            ProductMode::Public => HostKind::PublicRealm,
            ProductMode::Dev => HostKind::DevDirectConnect,
        }
    }

    pub fn initial_multiplayer_host_kind(
        &self,
        settings: &Settings,
        cli_server: Option<&str>,
    ) -> HostKind {
        match self.product_mode {
            ProductMode::Public => HostKind::PublicRealm,
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
                    "Caldrayne Realm Unavailable".to_string()
                } else {
                    self.public_realm.login_label()
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
                HostKind::DevDirectConnect | HostKind::PublicRealm => {
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
            ProductMode::Public => self.public_realm.connection_args(),
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
                    HostKind::PublicRealm => {
                        return Err("Public realm host kind cannot be used for \
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
            ProductMode::Public => HostKind::PublicRealm,
            ProductMode::Dev => host_kind,
        };

        Ok(ResolvedConnectHost {
            kind,
            connection_args,
            target_address: match self.product_mode {
                ProductMode::Public => Some(self.public_realm.server_address.trim().to_string()),
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
            HostKind::PublicRealm => self
                .public_realm_auth_trust_outcome(auth_server)
                .is_trusted(),
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

    pub fn public_realm_auth_trust_outcome(
        &self,
        auth_server: &str,
    ) -> PublicRealmAuthTrustOutcome {
        match self.public_realm.auth_server.as_deref() {
            Some(expected) if expected == auth_server => {
                PublicRealmAuthTrustOutcome::TrustedExactBundledProvider
            },
            Some(_) => PublicRealmAuthTrustOutcome::RejectedProviderMismatch,
            None => PublicRealmAuthTrustOutcome::RejectedMissingBundledProvider,
        }
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

    fn public_realm(server_address: &str, auth_server: Option<&str>) -> PublicRealm {
        public_realm_with_transport(server_address, auth_server, false, false, true)
    }

    fn public_realm_with_transport(
        server_address: &str,
        auth_server: Option<&str>,
        use_srv: bool,
        use_quic: bool,
        validate_tls: bool,
    ) -> PublicRealm {
        PublicRealm {
            display_name: "Caldrayne Realm".to_string(),
            server_address: server_address.to_string(),
            auth_server: auth_server.map(ToOwned::to_owned),
            use_srv,
            use_quic,
            validate_tls,
            source_kind: PublicRealmSourceKind::Bundled,
        }
    }

    fn entry_policy(product_mode: ProductMode, public_realm: PublicRealm) -> EntryPolicy {
        EntryPolicy {
            product_mode,
            public_realm,
        }
    }

    #[test]
    fn public_mode_blocks_when_public_realm_missing() {
        let policy = entry_policy(ProductMode::Public, public_realm("", None));

        assert!(policy.public_mode_blocker_message().is_some());
    }

    #[test]
    fn public_mode_uses_public_realm_connection_args() {
        let policy = entry_policy(
            ProductMode::Public,
            public_realm("192.168.1.8:14004", Some("auth.example.test")),
        );
        let settings = NetworkingSettings::default();

        let host = policy
            .resolve_multiplayer_host(
                HostKind::PublicRealm,
                "ignored.example.test",
                None,
                &settings,
            )
            .expect("public mode should resolve to public realm");

        assert_eq!(host.kind, HostKind::PublicRealm);

        match host.connection_args {
            ConnectionArgs::Tcp { hostname, .. } => {
                assert_eq!(hostname, "192.168.1.8:14004");
            },
            other => panic!("unexpected connection args: {other:?}"),
        }
        assert_eq!(host.target_address.as_deref(), Some("192.168.1.8:14004"));
    }

    #[test]
    fn public_mode_reclaims_drifted_ui_host_selection_back_to_public_realm() {
        let policy = entry_policy(
            ProductMode::Public,
            public_realm("192.168.1.8:14004", Some("auth.example.test")),
        );
        let settings = NetworkingSettings {
            use_srv: true,
            use_quic: true,
            validate_tls: false,
            ..NetworkingSettings::default()
        };

        let host = policy
            .resolve_multiplayer_host(
                HostKind::DevDirectConnect,
                "203.0.113.20:14004",
                None,
                &settings,
            )
            .expect("public mode should reclaim drifted host selection to public realm");

        assert_eq!(host.kind, HostKind::PublicRealm);
        assert_eq!(host.target_address.as_deref(), Some("192.168.1.8:14004"));

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
            public_realm("192.168.1.8:14004", Some("auth.example.test")),
        );
        let settings = Settings::default();

        let server = policy.initial_server_field_value(&settings, Some("203.0.113.20:14004"));

        assert_eq!(server, "Caldrayne Realm");
    }

    #[test]
    fn dev_mode_uses_cli_server_for_initial_value() {
        let policy = entry_policy(ProductMode::Dev, public_realm("192.168.1.8:14004", None));
        let settings = Settings::default();

        let server = policy.initial_server_field_value(&settings, Some("203.0.113.20:14004"));

        assert_eq!(server, "203.0.113.20:14004");
    }

    #[test]
    fn public_mode_uses_unavailable_label_when_public_realm_missing() {
        let policy = entry_policy(ProductMode::Public, public_realm("", None));
        let settings = Settings::default();

        let server = policy.initial_server_field_value(&settings, None);

        assert_eq!(server, "Caldrayne Realm Unavailable");
    }

    #[test]
    fn server_field_lock_rules_match_public_and_dev_semantics() {
        let public_policy = entry_policy(
            ProductMode::Public,
            public_realm("192.168.1.8:14004", None),
        );
        let dev_policy = entry_policy(ProductMode::Dev, public_realm("192.168.1.8:14004", None));

        assert!(public_policy.should_lock_server_field(None));
        assert!(!public_policy.can_unlock_server_field(Some("203.0.113.20:14004")));

        assert!(!dev_policy.should_lock_server_field(None));
        assert!(dev_policy.should_lock_server_field(Some("203.0.113.20:14004")));
        assert!(dev_policy.can_unlock_server_field(Some("203.0.113.20:14004")));
        assert!(!dev_policy.can_unlock_server_field(None));
    }

    #[test]
    fn public_mode_disables_multiplayer_when_public_realm_missing() {
        let policy = entry_policy(ProductMode::Public, public_realm("", None));

        assert!(!policy.can_attempt_multiplayer());
    }

    #[test]
    fn module_f_boundary_keeps_public_realm_as_single_source() {
        let policy = entry_policy(
            ProductMode::Public,
            public_realm("192.168.1.8:14004", Some("auth.example.test")),
        );

        let contract = policy.upgrade_boundary_contract();

        assert_eq!(
            contract.public_realm_source_kind,
            PublicRealmSourceKind::Bundled
        );
        assert!(contract.launcher_may_replace_public_realm_source);
        assert!(contract.launcher_must_not_override_runtime_policy);
        assert!(
            contract
                .client_metadata_boundary
                .may_render
                .contains(&PublicRealmClientMetadataSurface::Announcement)
        );
        assert!(
            contract
                .client_metadata_boundary
                .may_render
                .contains(&PublicRealmClientMetadataSurface::ProductHallMetadata)
        );
        assert!(
            contract
                .client_metadata_boundary
                .must_not_become_authority_for
                .contains(&PublicRealmClientAuthorityBoundary::AccountRouting)
        );
        assert!(
            contract
                .client_metadata_boundary
                .must_not_become_authority_for
                .contains(&PublicRealmClientAuthorityBoundary::FreeServerSelection)
        );
        assert!(
            contract
                .client_metadata_boundary
                .requires_explicit_integration
        );
        assert!(
            contract
                .lightweight_discovery_boundary
                .may_expose
                .contains(&LightweightDiscoverySurface::EnvironmentHint)
        );
        assert!(
            contract
                .lightweight_discovery_boundary
                .must_not_become_authority_for
                .contains(&LightweightDiscoveryAuthorityBoundary::PublicRealmTargeting)
        );
        assert!(
            contract
                .lightweight_discovery_boundary
                .must_not_become_authority_for
                .contains(&LightweightDiscoveryAuthorityBoundary::DirectoryOrReleaseControlPlane)
        );
        assert!(
            contract
                .lightweight_discovery_boundary
                .remains_optional_observability_plane
        );
        assert!(
            contract
                .forbidden_runtime_overrides
                .iter()
                .any(|item| item.contains("--server"))
        );
        assert!(
            contract
                .forbidden_runtime_overrides
                .iter()
                .any(|item| item.contains("query-server"))
        );
        assert!(
            contract
                .must_remain_external
                .iter()
                .any(|item| item.contains("anti-cheat"))
        );
    }

    #[test]
    fn public_auth_handoff_contract_marks_missing_bundled_auth_pin_as_not_external_ready() {
        let policy = entry_policy(
            ProductMode::Public,
            public_realm("192.168.1.8:14004", None),
        );

        let contract = policy.public_realm_auth_handoff_contract();

        assert!(contract.bundled_server_address_configured);
        assert!(
            contract
                .bundled_public_realm_artifact_identity
                .starts_with(common::public_realm::PUBLIC_REALM_ARTIFACT_IDENTITY_SCHEME)
        );
        assert_eq!(
            contract.bundled_target_kind,
            BundledPublicRealmTargetKind::PrivateOrUniqueLocalIp
        );
        assert!(!contract.bundled_target_is_non_local_candidate);
        assert_eq!(
            contract.bundled_transport_kind,
            BundledPublicRealmTransportKind::DirectTcp
        );
        assert!(!contract.bundled_use_srv);
        assert!(!contract.bundled_use_quic);
        assert!(contract.bundled_validate_tls);
        assert_eq!(contract.bundled_auth_mode, ServerAuthMode::NoExternalAuth);
        assert_eq!(contract.bundled_auth_provider_url, None);
        assert!(contract.exact_provider_match_required);
        assert!(!contract.prompt_for_manual_trust);
        assert!(!contract.persist_manual_trust);
        assert!(!contract.external_auth_rollout_ready);
        assert!(!contract.non_local_cutover_ready);
        assert!(
            contract
                .non_local_cutover_gap_reasons
                .contains(&"bundled_public_target_is_private_or_unique_local_ip")
        );
        assert!(
            contract
                .non_local_cutover_gap_reasons
                .contains(&"bundled_public_auth_pin_missing")
        );
        assert!(
            contract
                .rollout_readiness_scope
                .contains("transitional mode")
        );
    }

    #[test]
    fn public_auth_handoff_contract_marks_bundled_external_auth_pin_as_rollout_ready() {
        let policy = entry_policy(
            ProductMode::Public,
            public_realm("192.168.1.8:14004", Some("https://auth.official.test")),
        );

        let contract = policy.public_realm_auth_handoff_contract();

        assert!(contract.bundled_server_address_configured);
        assert!(
            contract
                .bundled_public_realm_artifact_identity
                .starts_with(common::public_realm::PUBLIC_REALM_ARTIFACT_IDENTITY_SCHEME)
        );
        assert_eq!(
            contract.bundled_target_kind,
            BundledPublicRealmTargetKind::PrivateOrUniqueLocalIp
        );
        assert!(!contract.bundled_target_is_non_local_candidate);
        assert_eq!(contract.bundled_auth_mode, ServerAuthMode::ExternalProvider);
        assert_eq!(
            contract.bundled_auth_provider_url.as_deref(),
            Some("https://auth.official.test")
        );
        assert!(contract.external_auth_rollout_ready);
        assert!(!contract.non_local_cutover_ready);
        assert_eq!(contract.non_local_cutover_gap_reasons, vec![
            "bundled_public_target_is_private_or_unique_local_ip"
        ]);
        assert!(contract.public_auth_authority.contains("exact-match"));
    }

    #[test]
    fn public_auth_handoff_contract_exports_bundled_transport_policy() {
        let policy = entry_policy(
            ProductMode::Public,
            public_realm_with_transport(
                "play.caldrayne.example",
                Some("https://auth.official.test"),
                true,
                true,
                false,
            ),
        );

        let contract = policy.public_realm_auth_handoff_contract();

        assert_eq!(
            contract.bundled_transport_kind,
            BundledPublicRealmTransportKind::SrvLookup
        );
        assert!(contract.bundled_use_srv);
        assert!(contract.bundled_use_quic);
        assert!(!contract.bundled_validate_tls);
    }

    #[test]
    fn public_auth_handoff_contract_marks_named_host_with_external_auth_as_non_local_cutover_ready()
    {
        let policy = entry_policy(
            ProductMode::Public,
            public_realm(
                "play.caldrayne.example:14004",
                Some("https://auth.official.test"),
            ),
        );

        let contract = policy.public_realm_auth_handoff_contract();

        assert_eq!(
            contract.bundled_target_kind,
            BundledPublicRealmTargetKind::NamedHostCandidate
        );
        assert!(contract.bundled_target_is_non_local_candidate);
        assert!(contract.external_auth_rollout_ready);
        assert!(contract.non_local_cutover_ready);
        assert!(contract.non_local_cutover_gap_reasons.is_empty());
    }

    #[test]
    fn public_auth_handoff_contract_marks_reserved_public_doc_ip_as_not_cutover_ready() {
        let policy = entry_policy(
            ProductMode::Public,
            public_realm("203.0.113.10:14004", Some("https://auth.official.test")),
        );

        let contract = policy.public_realm_auth_handoff_contract();

        assert_eq!(
            contract.bundled_target_kind,
            BundledPublicRealmTargetKind::ReservedNonPublicIp
        );
        assert!(!contract.bundled_target_is_non_local_candidate);
        assert!(contract.external_auth_rollout_ready);
        assert!(!contract.non_local_cutover_ready);
        assert_eq!(contract.non_local_cutover_gap_reasons, vec![
            "bundled_public_target_is_reserved_non_public_ip"
        ]);
    }

    #[test]
    fn public_auth_handoff_contract_artifact_identity_is_stable_for_same_entry_content() {
        let first = entry_policy(
            ProductMode::Public,
            public_realm("192.168.1.8:14004", Some("https://auth.official.test")),
        )
        .public_realm_auth_handoff_contract()
        .bundled_public_realm_artifact_identity;
        let second = entry_policy(
            ProductMode::Public,
            public_realm("192.168.1.8:14004", Some("https://auth.official.test")),
        )
        .public_realm_auth_handoff_contract()
        .bundled_public_realm_artifact_identity;

        assert_eq!(first, second);
    }

    #[test]
    fn public_auth_handoff_contract_artifact_identity_changes_when_server_address_changes() {
        let baseline = entry_policy(
            ProductMode::Public,
            public_realm("192.168.1.8:14004", Some("https://auth.official.test")),
        )
        .public_realm_auth_handoff_contract()
        .bundled_public_realm_artifact_identity;
        let changed = entry_policy(
            ProductMode::Public,
            public_realm("203.0.113.10:14004", Some("https://auth.official.test")),
        )
        .public_realm_auth_handoff_contract()
        .bundled_public_realm_artifact_identity;

        assert_ne!(baseline, changed);
    }

    #[test]
    fn public_auth_handoff_contract_artifact_identity_changes_when_auth_server_changes() {
        let baseline = entry_policy(
            ProductMode::Public,
            public_realm("192.168.1.8:14004", Some("https://auth.official.test")),
        )
        .public_realm_auth_handoff_contract()
        .bundled_public_realm_artifact_identity;
        let changed = entry_policy(
            ProductMode::Public,
            public_realm("192.168.1.8:14004", Some("https://auth.backup.test")),
        )
        .public_realm_auth_handoff_contract()
        .bundled_public_realm_artifact_identity;

        assert_ne!(baseline, changed);
    }

    #[test]
    fn public_realm_source_kind_defaults_to_bundled_when_omitted() {
        let entry: PublicRealm = ron::from_str(
            r#"(
                display_name: "Caldrayne Realm",
                server_address: "192.168.1.8:14004",
                auth_server: None,
                use_srv: false,
                use_quic: false,
                validate_tls: true,
            )"#,
        )
        .expect("legacy public realm should deserialize");

        assert_eq!(entry.source_kind, PublicRealmSourceKind::Bundled);
    }

    #[test]
    fn dev_mode_uses_requested_server_and_persists_history() {
        let policy = entry_policy(ProductMode::Dev, public_realm("192.168.1.8:14004", None));
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
            public_realm("192.168.1.8:14004", None),
        );
        let mut networking = NetworkingSettings {
            username: "old-user".to_string(),
            servers: vec!["10.0.0.2:14004".to_string()],
            default_server: "10.0.0.2:14004".to_string(),
            ..NetworkingSettings::default()
        };

        policy.apply_login_settings(
            &mut networking,
            HostKind::PublicRealm,
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
    fn public_mode_only_trusts_public_realm_auth_server() {
        let policy = entry_policy(
            ProductMode::Public,
            public_realm("192.168.1.8:14004", Some("auth.official.test")),
        );
        let mut trusted_auth_servers = HashSet::new();
        trusted_auth_servers.insert("auth.official.test".to_string());
        trusted_auth_servers.insert("auth.other.test".to_string());

        assert!(policy.is_auth_server_trusted(
            HostKind::PublicRealm,
            "auth.official.test",
            &trusted_auth_servers
        ));
        assert!(!policy.is_auth_server_trusted(
            HostKind::PublicRealm,
            "auth.other.test",
            &trusted_auth_servers
        ));
        assert_eq!(
            policy.public_realm_auth_trust_outcome("auth.other.test"),
            PublicRealmAuthTrustOutcome::RejectedProviderMismatch
        );
    }

    #[test]
    fn public_mode_does_not_fall_back_to_saved_auth_cache_without_bundled_auth_pin() {
        let policy = entry_policy(
            ProductMode::Public,
            public_realm("192.168.1.8:14004", None),
        );
        let mut trusted_auth_servers = HashSet::new();
        trusted_auth_servers.insert("auth.official.test".to_string());

        assert!(!policy.is_auth_server_trusted(
            HostKind::PublicRealm,
            "auth.official.test",
            &trusted_auth_servers
        ));
        assert_eq!(
            policy.public_realm_auth_trust_outcome("auth.official.test"),
            PublicRealmAuthTrustOutcome::RejectedMissingBundledProvider
        );
    }

    #[test]
    fn dev_mode_respects_saved_auth_trust_list() {
        let policy = entry_policy(ProductMode::Dev, public_realm("192.168.1.8:14004", None));
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
            public_realm("192.168.1.8:14004", None),
        );
        let dev_policy = entry_policy(ProductMode::Dev, public_realm("192.168.1.8:14004", None));

        assert_eq!(
            public_policy.multiplayer_host_kind(),
            HostKind::PublicRealm
        );
        assert_eq!(
            dev_policy.multiplayer_host_kind(),
            HostKind::DevDirectConnect
        );
    }

    #[test]
    fn dev_local_dedicated_does_not_pollute_direct_history() {
        let policy = entry_policy(ProductMode::Dev, public_realm("192.168.1.8:14004", None));
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
        let policy = entry_policy(ProductMode::Dev, public_realm("192.168.1.8:14004", None));
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
        let policy = entry_policy(ProductMode::Dev, public_realm("192.168.1.8:14004", None));
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
        let policy = entry_policy(ProductMode::Dev, public_realm("192.168.1.8:14004", None));
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
        let policy = entry_policy(ProductMode::Dev, public_realm("192.168.1.8:14004", None));
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
        let policy = entry_policy(ProductMode::Dev, public_realm("192.168.1.8:14004", None));
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
        let policy = entry_policy(ProductMode::Dev, public_realm("192.168.1.8:14004", None));
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
        let policy = entry_policy(ProductMode::Dev, public_realm("192.168.1.8:14004", None));
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
        let policy = entry_policy(ProductMode::Dev, public_realm("192.168.1.8:14004", None));
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
        let policy = entry_policy(ProductMode::Dev, public_realm("192.168.1.8:14004", None));

        assert!(policy.should_prompt_for_auth_trust(HostKind::DevDirectConnect));
        assert!(policy.should_persist_auth_trust(HostKind::DevLocalDedicated));
        assert!(!policy.should_prompt_for_auth_trust(HostKind::PublicRealm));
    }
}
