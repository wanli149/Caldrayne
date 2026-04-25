use super::{RuntimeEnvironment, Settings};
use common_net::msg::ServerAuthMode;
use serde::Serialize;
use std::{error::Error as StdError, fmt};

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct AccountAuthGovernanceReport {
    pub status: &'static str,
    pub environment: &'static str,
    pub startup_policy: AccountAuthStartupPolicyReport,
    pub runtime_topology: AccountAuthRuntimeTopologyReport,
    pub principal_definition: AccountPrincipalDefinitionReport,
    pub environment_namespace_policy: AccountNamespacePolicyReport,
    pub governed_scopes: Vec<AccountIdentityScopeReport>,
    pub unsupported_topologies: Vec<UnsupportedAccountAuthTopologyReport>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct AccountAuthStartupPolicyReport {
    pub no_auth_allowed_in_current_environment: bool,
    pub non_local_requires_external_auth: bool,
    pub startup_permitted: bool,
    pub gate_owner: &'static str,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct AccountAuthRuntimeTopologyReport {
    pub auth_mode: &'static str,
    pub authoritative_auth_source: &'static str,
    pub authoritative_auth_provider: Option<String>,
    pub login_input_contract: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct AccountPrincipalDefinitionReport {
    pub formal_non_local_account_kind: &'static str,
    pub formal_non_local_account_established_by: &'static str,
    pub formal_registration_authority: &'static str,
    pub current_runtime_principal_kind: &'static str,
    pub current_runtime_principal_source: &'static str,
    pub local_no_auth_identity_rule: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct AccountNamespacePolicyReport {
    pub test_production_relationship: &'static str,
    pub test_production_detail: &'static str,
    pub local_development_relationship: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct AccountIdentityScopeReport {
    pub scope: &'static str,
    pub anchor_kind: &'static str,
    pub anchor_source: &'static str,
    pub detail: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct UnsupportedAccountAuthTopologyReport {
    pub id: &'static str,
    pub applies: bool,
    pub reason: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountAuthTopologyError {
    MissingNonLocalExternalAuth { environment: RuntimeEnvironment },
}

impl fmt::Display for AccountAuthTopologyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingNonLocalExternalAuth { environment } => write!(
                f,
                "Refusing to start in {} environment: non-local runtime requires \
                 auth_server_address because deterministic local username fallback is \
                 development-only.",
                environment.as_str()
            ),
        }
    }
}

impl StdError for AccountAuthTopologyError {}

impl RuntimeEnvironment {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Test => "test",
            Self::Production => "production",
        }
    }

    pub const fn allows_optional_auth(self) -> bool { matches!(self, Self::Local) }
}

impl Settings {
    pub fn validate_account_auth_topology(&self) -> Result<(), AccountAuthTopologyError> {
        if !self.runtime_environment.allows_optional_auth() && self.auth_server_address.is_none() {
            Err(AccountAuthTopologyError::MissingNonLocalExternalAuth {
                environment: self.runtime_environment,
            })
        } else {
            Ok(())
        }
    }

    pub fn account_auth_governance_report(&self) -> AccountAuthGovernanceReport {
        let auth_mode = self.server_auth_mode();
        let startup_permitted = self.validate_account_auth_topology().is_ok();
        let anchor_source = current_principal_source(auth_mode);

        AccountAuthGovernanceReport {
            status: if startup_permitted {
                "account-auth-governance"
            } else {
                "account-auth-governance-invalid"
            },
            environment: self.runtime_environment.as_str(),
            startup_policy: AccountAuthStartupPolicyReport {
                no_auth_allowed_in_current_environment: self
                    .runtime_environment
                    .allows_optional_auth(),
                non_local_requires_external_auth: true,
                startup_permitted,
                gate_owner: "server-core-settings",
                detail: startup_policy_detail(self),
            },
            runtime_topology: AccountAuthRuntimeTopologyReport {
                auth_mode: auth_mode.as_str(),
                authoritative_auth_source: current_authoritative_auth_source(auth_mode),
                authoritative_auth_provider: self.auth_server_address.clone(),
                login_input_contract: login_input_contract(auth_mode),
            },
            principal_definition: AccountPrincipalDefinitionReport {
                formal_non_local_account_kind: "external-auth-issued-player-uuid",
                formal_non_local_account_established_by: "configured-external-auth-provider",
                formal_registration_authority: "external-auth-system",
                current_runtime_principal_kind: current_principal_kind(auth_mode),
                current_runtime_principal_source: anchor_source,
                local_no_auth_identity_rule: "derive a deterministic UUID from the submitted \
                                              username; local development only and not a formal \
                                              account registry",
            },
            environment_namespace_policy: AccountNamespacePolicyReport {
                test_production_relationship: "external-auth-authority-defined",
                test_production_detail: "the game server does not mint or map a second account \
                                         namespace; test and production only share accounts when \
                                         operators pin both environments to the same external \
                                         auth authority",
                local_development_relationship: "local-no-auth-identities-are-development-standins",
            },
            governed_scopes: vec![
                AccountIdentityScopeReport {
                    scope: "session-principal",
                    anchor_kind: "player-uuid",
                    anchor_source,
                    detail: "the live player session is keyed by the UUID resolved during login",
                },
                AccountIdentityScopeReport {
                    scope: "admin-membership",
                    anchor_kind: "player-uuid",
                    anchor_source,
                    detail: "admins.ron stores role grants by UUID rather than by username",
                },
                AccountIdentityScopeReport {
                    scope: "whitelist-membership",
                    anchor_kind: "player-uuid",
                    anchor_source,
                    detail: "whitelist.ron grants access by UUID rather than by username",
                },
                AccountIdentityScopeReport {
                    scope: "uuid-ban-membership",
                    anchor_kind: "player-uuid",
                    anchor_source,
                    detail: "banlist UUID entries and ban actor metadata both stay anchored to \
                             UUID identity",
                },
                AccountIdentityScopeReport {
                    scope: "ip-ban-upgrade-link",
                    anchor_kind: "player-uuid-plus-ip-address",
                    anchor_source: "player-uuid plus normalized remote IP evidence",
                    detail: "upgradeable IP bans remain linked back to the triggering player UUID",
                },
                AccountIdentityScopeReport {
                    scope: "character-ownership",
                    anchor_kind: "player-uuid",
                    anchor_source,
                    detail: "character persistence stores player_uuid as the owner key for \
                             create/load/edit/delete flows",
                },
            ],
            unsupported_topologies: vec![UnsupportedAccountAuthTopologyReport {
                id: "non-local-no-external-auth",
                applies: !self.runtime_environment.allows_optional_auth()
                    && !auth_mode.requires_external_auth(),
                reason: "test and production must not fall back to deterministic local usernames; \
                         non-local account truth must come from an external auth provider",
            }],
        }
    }
}

fn startup_policy_detail(settings: &Settings) -> String {
    match (
        settings.runtime_environment,
        settings.auth_server_address.as_deref(),
    ) {
        (RuntimeEnvironment::Local, None) => {
            "local runtime is using deterministic username-derived UUIDs; this path is \
             development-only and does not define a formal non-local account"
                .to_owned()
        },
        (RuntimeEnvironment::Local, Some(provider)) => format!(
            "local runtime is pinned to external auth provider {provider}; local environment \
             still retains the option to run without external auth"
        ),
        (environment, Some(provider)) => format!(
            "{} runtime is pinned to external auth provider {}; no-auth fallback is disabled",
            environment.as_str(),
            provider
        ),
        (environment, None) => format!(
            "{} runtime cannot start without auth_server_address because deterministic local \
             username fallback is development-only",
            environment.as_str()
        ),
    }
}

fn current_principal_kind(auth_mode: ServerAuthMode) -> &'static str {
    match auth_mode {
        ServerAuthMode::ExternalProvider => "external-auth-issued-player-uuid",
        ServerAuthMode::NoExternalAuth => "deterministic-local-player-uuid",
    }
}

fn current_principal_source(auth_mode: ServerAuthMode) -> &'static str {
    match auth_mode {
        ServerAuthMode::ExternalProvider => "external-auth-provider-issued-uuid",
        ServerAuthMode::NoExternalAuth => "deterministic-local-username-derivation",
    }
}

fn current_authoritative_auth_source(auth_mode: ServerAuthMode) -> &'static str {
    match auth_mode {
        ServerAuthMode::ExternalProvider => "configured-external-auth-provider",
        ServerAuthMode::NoExternalAuth => "deterministic-local-username-derivation",
    }
}

fn login_input_contract(auth_mode: ServerAuthMode) -> &'static str {
    match auth_mode {
        ServerAuthMode::ExternalProvider => {
            "client register input is interpreted as an auth token and resolved by the external \
             auth provider"
        },
        ServerAuthMode::NoExternalAuth => {
            "client register input is interpreted as a local username and deterministically mapped \
             to a UUID"
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_no_auth_governance_marks_identity_as_development_only() {
        let settings = Settings::default();
        let report = settings.account_auth_governance_report();

        assert_eq!(report.environment, "local");
        assert_eq!(report.status, "account-auth-governance");
        assert!(report.startup_policy.no_auth_allowed_in_current_environment);
        assert!(report.startup_policy.startup_permitted);
        assert_eq!(
            report.principal_definition.current_runtime_principal_kind,
            "deterministic-local-player-uuid"
        );
        assert_eq!(
            report
                .environment_namespace_policy
                .local_development_relationship,
            "local-no-auth-identities-are-development-standins"
        );
        assert!(
            !report
                .unsupported_topologies
                .iter()
                .any(|topology| topology.applies)
        );
    }

    #[test]
    fn non_local_runtime_requires_external_auth() {
        let settings = Settings {
            runtime_environment: RuntimeEnvironment::Production,
            ..Settings::default()
        };
        let report = settings.account_auth_governance_report();

        assert_eq!(
            settings.validate_account_auth_topology(),
            Err(AccountAuthTopologyError::MissingNonLocalExternalAuth {
                environment: RuntimeEnvironment::Production,
            })
        );
        assert_eq!(report.status, "account-auth-governance-invalid");
        assert!(!report.startup_policy.startup_permitted);
        assert!(
            report
                .unsupported_topologies
                .iter()
                .any(|topology| topology.id == "non-local-no-external-auth" && topology.applies)
        );
    }

    #[test]
    fn external_auth_runtime_reports_provider_issued_identity_anchor() {
        let settings = Settings {
            runtime_environment: RuntimeEnvironment::Test,
            auth_server_address: Some("https://auth.example.test".to_owned()),
            ..Settings::default()
        };
        let report = settings.account_auth_governance_report();

        assert_eq!(settings.validate_account_auth_topology(), Ok(()));
        assert_eq!(
            report
                .runtime_topology
                .authoritative_auth_provider
                .as_deref(),
            Some("https://auth.example.test")
        );
        assert_eq!(
            report.principal_definition.current_runtime_principal_source,
            "external-auth-provider-issued-uuid"
        );
        assert!(report.governed_scopes.iter().any(|scope| {
            scope.scope == "character-ownership"
                && scope.anchor_kind == "player-uuid"
                && scope.anchor_source == "external-auth-provider-issued-uuid"
        }));
    }
}
