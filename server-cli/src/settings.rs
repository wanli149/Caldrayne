use serde::{Deserialize, Serialize};
use std::{
    fmt, fs,
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
};
use tracing::{error, warn};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[expect(clippy::upper_case_acronyms)]
pub enum ShutdownSignal {
    SIGUSR1,
    SIGUSR2,
    SIGTERM,
}

impl ShutdownSignal {
    #[cfg(not(target_os = "windows"))]
    pub fn to_signal(self) -> core::ffi::c_int {
        match self {
            Self::SIGUSR1 => signal_hook::consts::SIGUSR1,
            Self::SIGUSR2 => signal_hook::consts::SIGUSR2,
            Self::SIGTERM => signal_hook::consts::SIGTERM,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Environment {
    #[default]
    Local,
    Test,
    Production,
}

impl Environment {
    pub fn allows_optional_auth(self) -> bool { matches!(self, Self::Local) }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Test => "test",
            Self::Production => "production",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRetentionPolicy {
    pub max_active_file_mebibytes: u64,
    pub max_archive_files: usize,
}

impl AuditRetentionPolicy {
    pub fn max_active_file_bytes(self) -> u64 {
        self.max_active_file_mebibytes
            .saturating_mul(1024)
            .saturating_mul(1024)
    }
}

impl Default for AuditRetentionPolicy {
    fn default() -> Self {
        Self {
            max_active_file_mebibytes: 32,
            max_archive_files: 7,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceReachability {
    Disabled,
    LoopbackOnly,
    NetworkAccessible,
}

impl SurfaceReachability {
    fn from_socket_addr(addr: SocketAddr) -> Self {
        if addr.ip().is_loopback() {
            Self::LoopbackOnly
        } else {
            Self::NetworkAccessible
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::LoopbackOnly => "loopback-only",
            Self::NetworkAccessible => "network-accessible",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceAuth {
    None,
    ExplicitSecret,
    LoopbackUiSession,
    RealmHandshake,
}

impl SurfaceAuth {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ExplicitSecret => "explicit-secret",
            Self::LoopbackUiSession => "loopback-ui-session",
            Self::RealmHandshake => "realm-handshake",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceCredentialBootstrap {
    None,
    OperatorProvidedSecret,
    LoopbackUiBootstrap,
    RealmHandshakeFlow,
}

impl SurfaceCredentialBootstrap {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::OperatorProvidedSecret => "operator-provided-secret",
            Self::LoopbackUiBootstrap => "loopback-ui-bootstrap",
            Self::RealmHandshakeFlow => "realm-handshake-flow",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfacePurpose {
    PrimaryGameTraffic,
    PrototypeControlPlane,
    InternalObservability,
    LightweightDiscovery,
}

impl SurfacePurpose {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PrimaryGameTraffic => "primary-game-traffic",
            Self::PrototypeControlPlane => "prototype-control-plane",
            Self::InternalObservability => "internal-observability",
            Self::LightweightDiscovery => "lightweight-discovery",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceConsumption {
    RealmHandshake,
    InteractiveSession,
    ControlApi,
    MachineScrape,
    MachineProbe,
    DiscoveryQuery,
}

impl SurfaceConsumption {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RealmHandshake => "realm-handshake",
            Self::InteractiveSession => "interactive-session",
            Self::ControlApi => "control-api",
            Self::MachineScrape => "machine-scrape",
            Self::MachineProbe => "machine-probe",
            Self::DiscoveryQuery => "discovery-query",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceReviewStatus {
    GameplayTransport,
    PrototypeControlPlaneUnaudited,
    InternalObservabilityOnly,
    DiscoveryOnlyNotAuthority,
}

impl SurfaceReviewStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GameplayTransport => "gameplay-transport",
            Self::PrototypeControlPlaneUnaudited => "prototype-control-plane-unaudited",
            Self::InternalObservabilityOnly => "internal-observability-only",
            Self::DiscoveryOnlyNotAuthority => "discovery-only-not-authority",
        }
    }

    pub fn network_access_warning(self) -> Option<&'static str> {
        match self {
            Self::GameplayTransport => None,
            Self::PrototypeControlPlaneUnaudited => Some(
                "This surface is still treated as prototype/internal tooling. Do not treat it as \
                 a production-grade control plane without a separate security review.",
            ),
            Self::InternalObservabilityOnly => Some(
                "This observability surface is network-accessible. Third-stage policy treats it \
                 as internal operations infrastructure, not a public endpoint.",
            ),
            Self::DiscoveryOnlyNotAuthority => Some(
                "This discovery surface is network-accessible. It must not become a second, \
                 independent realm/version authority beside the primary handshake path.",
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceRemoteExposurePolicy {
    RemoteAllowedByDesign,
    LoopbackRuntimeEnforced,
    RemoteRequiresExplicitWebOptIn,
    RemoteRequiresExplicitWebOptInAndSecret,
    RemoteRequiresExplicitQueryOptIn,
}

impl SurfaceRemoteExposurePolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RemoteAllowedByDesign => "remote-allowed-by-design",
            Self::LoopbackRuntimeEnforced => "loopback-runtime-enforced",
            Self::RemoteRequiresExplicitWebOptIn => "remote-requires-explicit-web-opt-in",
            Self::RemoteRequiresExplicitWebOptInAndSecret => {
                "remote-requires-explicit-web-opt-in-and-secret"
            },
            Self::RemoteRequiresExplicitQueryOptIn => "remote-requires-explicit-query-opt-in",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensitiveConfigSensitivity {
    SharedSecret,
    PublicCertificateChain,
    PrivateKey,
}

impl SensitiveConfigSensitivity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SharedSecret => "shared-secret",
            Self::PublicCertificateChain => "public-certificate-chain",
            Self::PrivateKey => "private-key",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensitiveConfigSource {
    InlineOperatorProvided,
    InlineOperatorProvidedOrLoopbackBootstrap,
    FileBackedOperatorManaged,
}

impl SensitiveConfigSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InlineOperatorProvided => "inline-operator-provided",
            Self::InlineOperatorProvidedOrLoopbackBootstrap => {
                "inline-operator-provided-or-loopback-bootstrap"
            },
            Self::FileBackedOperatorManaged => "file-backed-operator-managed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensitiveConfigValueState {
    Unset,
    Blank,
    NonEmptyInline,
    GeneratedAtStartup,
    FilePathConfigured,
}

impl SensitiveConfigValueState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unset => "unset",
            Self::Blank => "blank",
            Self::NonEmptyInline => "non-empty-inline",
            Self::GeneratedAtStartup => "generated-at-startup",
            Self::FilePathConfigured => "file-path-configured",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensitiveConfigOperatorResponsibility {
    OptionalLocalBootstrapWhenUnset,
    SurfaceDisabledWhenUnset,
    RequiredForQuicBinding,
}

impl SensitiveConfigOperatorResponsibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OptionalLocalBootstrapWhenUnset => "optional-local-bootstrap-when-unset",
            Self::SurfaceDisabledWhenUnset => "surface-disabled-when-unset",
            Self::RequiredForQuicBinding => "required-for-quic-binding",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensitiveConfigExposureDependency {
    WebStackOptIn,
    ExperimentalQuicOptIn,
}

impl SensitiveConfigExposureDependency {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WebStackOptIn => "web-stack-opt-in",
            Self::ExperimentalQuicOptIn => "experimental-quic-opt-in",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportSecurityRolloutPolicy {
    DisabledUntilExplicitOptIn,
    ExperimentalOptInActive,
}

impl TransportSecurityRolloutPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DisabledUntilExplicitOptIn => "disabled-until-explicit-opt-in",
            Self::ExperimentalOptInActive => "experimental-opt-in-active",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportSecurityValidationPolicy {
    AdvisoryAtStartup,
    FailFastAtStartup,
}

impl TransportSecurityValidationPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AdvisoryAtStartup => "advisory-at-startup",
            Self::FailFastAtStartup => "fail-fast-at-startup",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportSecurityMaterialState {
    Valid,
    Invalid,
}

impl TransportSecurityMaterialState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Invalid => "invalid",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagementSurfaceCapability {
    InteractiveBootstrap,
    ReadOnlyOpsData,
    MutatingControl,
    ObservabilityScrape,
    ObservabilityProbe,
}

impl ManagementSurfaceCapability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InteractiveBootstrap => "interactive-bootstrap",
            Self::ReadOnlyOpsData => "read-only-ops-data",
            Self::MutatingControl => "mutating-control",
            Self::ObservabilityScrape => "observability-scrape",
            Self::ObservabilityProbe => "observability-probe",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagementCredentialTransport {
    LoopbackRuntimeGuard,
    CookieSecret,
    HeaderSecret,
    None,
}

impl ManagementCredentialTransport {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LoopbackRuntimeGuard => "loopback-runtime-guard",
            Self::CookieSecret => "cookie-secret",
            Self::HeaderSecret => "header-secret",
            Self::None => "none",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSurface {
    pub name: &'static str,
    pub bind_address: Option<SocketAddr>,
    pub reachability: SurfaceReachability,
    pub auth: SurfaceAuth,
    pub credential_bootstrap: SurfaceCredentialBootstrap,
    pub review_status: SurfaceReviewStatus,
    pub remote_exposure_policy: SurfaceRemoteExposurePolicy,
    pub purpose: SurfacePurpose,
    pub consumption: SurfaceConsumption,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SensitiveConfigInventoryEntry {
    pub id: &'static str,
    pub consumer_surface: &'static str,
    pub bind_address: Option<SocketAddr>,
    pub file_path: Option<PathBuf>,
    pub configured: bool,
    pub sensitivity: SensitiveConfigSensitivity,
    pub source: SensitiveConfigSource,
    pub value_state: SensitiveConfigValueState,
    pub operator_responsibility: SensitiveConfigOperatorResponsibility,
    pub exposure_dependency: SensitiveConfigExposureDependency,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagementAuthInventoryEntry {
    pub surface: &'static str,
    pub bind_address: Option<SocketAddr>,
    pub reachability: SurfaceReachability,
    pub review_status: SurfaceReviewStatus,
    pub remote_exposure_policy: SurfaceRemoteExposurePolicy,
    pub capability: ManagementSurfaceCapability,
    pub auth_scheme: SurfaceAuth,
    pub credential_bootstrap: SurfaceCredentialBootstrap,
    pub credential_transport: ManagementCredentialTransport,
    pub secret_config_id: Option<&'static str>,
    pub proxy_forwarding_forbidden: bool,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportSecurityInventoryEntry {
    pub surface: &'static str,
    pub bind_address: SocketAddr,
    pub transport: &'static str,
    pub encryption: &'static str,
    pub cert_file_path: PathBuf,
    pub key_file_path: PathBuf,
    pub rollout_policy: TransportSecurityRolloutPolicy,
    pub validation_policy: TransportSecurityValidationPolicy,
    pub material_state: TransportSecurityMaterialState,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum RuntimeGovernanceSeverity {
    Notice,
    Warning,
}

impl RuntimeGovernanceSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Notice => "notice",
            Self::Warning => "warning",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RuntimeGovernanceFinding {
    pub id: &'static str,
    pub severity: RuntimeGovernanceSeverity,
    pub subject: &'static str,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GovernanceSupportingContract {
    pub signal: &'static str,
    pub endpoint: &'static str,
    pub purpose: &'static str,
}

const GOVERNANCE_SUPPORTING_MANAGEMENT_AUTH: GovernanceSupportingContract =
    GovernanceSupportingContract {
        signal: "management-auth",
        endpoint: "/health/management-auth",
        purpose: "inspect remote management auth posture and bootstrap credential exposure for \
                  affected management surfaces",
    };

const GOVERNANCE_SUPPORTING_TRANSPORT_SECURITY: GovernanceSupportingContract =
    GovernanceSupportingContract {
        signal: "transport-security",
        endpoint: "/health/transport-security",
        purpose: "inspect QUIC rollout posture, certificate material state, and startup \
                  validation policy before accepting transport exceptions",
    };

const GOVERNANCE_SUPPORTING_QUERY_DISCOVERY: GovernanceSupportingContract =
    GovernanceSupportingContract {
        signal: "runtime-surfaces",
        endpoint: "/health/surfaces",
        purpose: "inspect the discovery-only query surface posture within the shared runtime \
                  surface inventory so it does not become a second realm targeting or environment \
                  authority",
    };

const GOVERNANCE_SUPPORTING_QUERY_COMPATIBILITY: GovernanceSupportingContract =
    GovernanceSupportingContract {
        signal: "compatibility-contract",
        endpoint: "/health/compatibility",
        purpose: "inspect the authoritative-handshake versus query-hint compatibility contract so \
                  discovery hints stay aligned with rollout truth, remain non-authoritative, and \
                  keep exact-match query protocol rollouts under explicit lockstep control",
    };

const GOVERNANCE_SUPPORTING_QUERY_DISCOVERY_CONTRACTS: [GovernanceSupportingContract; 2] = [
    GOVERNANCE_SUPPORTING_QUERY_DISCOVERY,
    GOVERNANCE_SUPPORTING_QUERY_COMPATIBILITY,
];

impl RuntimeGovernanceFinding {
    pub fn supporting_contracts(&self) -> &'static [GovernanceSupportingContract] {
        match self.id {
            "generated-ui-api-bootstrap-active"
            | "remote-unaudited-web-opt-in-active"
            | "prototype-control-plane-remote-exposure" => &[GOVERNANCE_SUPPORTING_MANAGEMENT_AUTH],
            "experimental-quic-opt-in-active" => &[GOVERNANCE_SUPPORTING_TRANSPORT_SECURITY],
            "remote-query-opt-in-active" => &GOVERNANCE_SUPPORTING_QUERY_DISCOVERY_CONTRACTS,
            _ => &[],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeLayout {
    pub userdata_dir: PathBuf,
    pub server_cli_settings_dir: PathBuf,
    pub server_state: server::ServerStatePaths,
    pub recovery_staging_state: server::ServerStatePaths,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeGuardInputs {
    pub no_auth_cli: bool,
    pub auth_server_configured: bool,
    pub web_address: SocketAddr,
    pub query_address: Option<SocketAddr>,
    pub quic_bindings: Vec<server::settings::QuicBinding>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeGuardError {
    NoAuthDisallowed {
        environment: Environment,
    },
    MissingAuthProvider {
        environment: Environment,
    },
    RemoteWebDisallowed {
        environment: Environment,
        address: SocketAddr,
    },
    RemoteWebRequiresExplicitUiApiSecret {
        environment: Environment,
        address: SocketAddr,
    },
    BlankSensitiveConfig {
        environment: Environment,
        config_id: &'static str,
        consumer_surface: &'static str,
    },
    RemoteQueryDisallowed {
        environment: Environment,
        address: SocketAddr,
    },
    ExperimentalQuicDisallowed {
        environment: Environment,
        address: SocketAddr,
    },
    InvalidQuicTlsMaterial {
        environment: Environment,
        address: SocketAddr,
        details: String,
    },
}

impl fmt::Display for RuntimeGuardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAuthDisallowed { environment } => write!(
                f,
                "Refusing to start in {} environment: --no-auth is only allowed in local \
                 environment.",
                environment.as_str()
            ),
            Self::MissingAuthProvider { environment } => write!(
                f,
                "Refusing to start in {} environment: auth_server_address must be configured in \
                 the selected server data directory's server_config/settings.ron.",
                environment.as_str()
            ),
            Self::RemoteWebDisallowed {
                environment,
                address,
            } => write!(
                f,
                "Refusing to start in {} environment: web_address {} would expose the current \
                 Web/UI/metrics/health stack beyond loopback. These surfaces are still treated as \
                 unaudited internal tooling in phase 3. Keep web_address on 127.0.0.1/::1 or set \
                 allow_unaudited_remote_web = true after reviewing the risk.",
                environment.as_str(),
                address
            ),
            Self::RemoteWebRequiresExplicitUiApiSecret {
                environment,
                address,
            } => write!(
                f,
                "Refusing to start in {} environment: web_address {} is network-accessible, so \
                 ui_api_secret must be explicitly configured to a non-empty operator-provided \
                 value. The autogenerated loopback UI session token is only acceptable for \
                 local-only operation.",
                environment.as_str(),
                address
            ),
            Self::BlankSensitiveConfig {
                environment,
                config_id,
                consumer_surface,
            } => write!(
                f,
                "Refusing to start in {} environment: sensitive config {} for {} is present but \
                 blank/whitespace. Shared secrets must be non-empty operator-provided values.",
                environment.as_str(),
                config_id,
                consumer_surface
            ),
            Self::RemoteQueryDisallowed {
                environment,
                address,
            } => write!(
                f,
                "Refusing to start in {} environment: query_address {} would expose the current \
                 lightweight discovery surface beyond loopback. This query path is not yet a \
                 formal realm/environment authority in phase 3. Keep it on 127.0.0.1/::1, disable \
                 it, or set allow_unaudited_remote_query = true after explicit review.",
                environment.as_str(),
                address
            ),
            Self::ExperimentalQuicDisallowed {
                environment,
                address,
            } => write!(
                f,
                "Refusing to start in {} environment: QUIC transport {} is still experimental in \
                 this codebase. Enable it only with explicit review by setting \
                 allow_experimental_quic = true.",
                environment.as_str(),
                address
            ),
            Self::InvalidQuicTlsMaterial {
                environment,
                address,
                details,
            } => write!(
                f,
                "Refusing to start in {} environment: QUIC transport {} does not have a valid TLS \
                 certificate/key configuration ({details}). Non-local environments must fail fast \
                 instead of silently dropping QUIC.",
                environment.as_str(),
                address
            ),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub environment: Environment,
    pub server_data_dir: Option<PathBuf>,
    pub recovery_staging_dir: Option<PathBuf>,
    pub allow_unaudited_remote_web: bool,
    pub allow_unaudited_remote_query: bool,
    pub allow_experimental_quic: bool,
    pub audit_retention: AuditRetentionPolicy,
    pub update_shutdown_grace_period_secs: u32,
    pub update_shutdown_message: String,
    pub web_address: SocketAddr,
    /// SECRET API HEADER used to access the chat api, if disabled the API is
    /// unreachable
    pub web_chat_secret: Option<String>,
    /// Operator-provided secret token for the /ui_api surface. When omitted, a
    /// random secret is generated at startup and only handed out via the
    /// loopback-only /ui bootstrap page.
    pub ui_api_secret: Option<String>,
    pub shutdown_signals: Vec<ShutdownSignal>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            environment: Environment::Local,
            server_data_dir: None,
            recovery_staging_dir: None,
            allow_unaudited_remote_web: false,
            allow_unaudited_remote_query: false,
            allow_experimental_quic: false,
            audit_retention: AuditRetentionPolicy::default(),
            update_shutdown_grace_period_secs: 120,
            update_shutdown_message: "The server is restarting for an update".to_owned(),
            web_address: SocketAddr::from((Ipv4Addr::LOCALHOST, 14005)),
            web_chat_secret: None,
            ui_api_secret: None,
            shutdown_signals: if cfg!(not(target_os = "windows")) {
                vec![ShutdownSignal::SIGUSR1]
            } else {
                Vec::new()
            },
        }
    }
}

impl Settings {
    const FILENAME: &str = "settings.ron";

    pub fn load() -> Option<Self> {
        let path = Self::get_settings_path();
        let template_path = path.with_extension("template.ron");

        let settings = if let Ok(file) = fs::File::open(&path) {
            match ron::de::from_reader(file) {
                Ok(s) => return Some(s),
                Err(e) => {
                    error!(
                        ?e,
                        "FATAL: Failed to parse setting file! Creating a template file for you to \
                         migrate your current settings file: {}",
                        template_path.display()
                    );
                    None
                },
            }
        } else {
            warn!(
                "Settings file not found! Creating a template file: {} — If you wish to change \
                 any settings, copy/move the template to {} and edit the fields as you wish.",
                template_path.display(),
                Self::FILENAME
            );
            Some(Self::default())
        };

        // This is reached if either:
        // - The file can't be opened (presumably it doesn't exist)
        // - Or there was an error parsing the file
        if let Err(e) = Self::save_template(&template_path) {
            error!(?e, "Failed to create template settings file!");
        }

        settings
    }

    pub fn validate_runtime(
        &self,
        inputs: RuntimeGuardInputs,
    ) -> Result<(), Vec<RuntimeGuardError>> {
        let mut errors = Vec::new();
        let sensitive_configs =
            self.sensitive_config_inventory_for_quic_bindings(&inputs.quic_bindings);
        let ui_api_secret = sensitive_configs
            .iter()
            .find(|config| config.id == "ui-api-secret")
            .expect("ui-api secret inventory entry should exist");

        for config in &sensitive_configs {
            if matches!(config.value_state, SensitiveConfigValueState::Blank) {
                errors.push(RuntimeGuardError::BlankSensitiveConfig {
                    environment: self.environment,
                    config_id: config.id,
                    consumer_surface: config.consumer_surface,
                });
            }
        }

        if !self.environment.allows_optional_auth() {
            if inputs.no_auth_cli {
                errors.push(RuntimeGuardError::NoAuthDisallowed {
                    environment: self.environment,
                });
            }

            if !inputs.auth_server_configured {
                errors.push(RuntimeGuardError::MissingAuthProvider {
                    environment: self.environment,
                });
            }

            let web_reachability = SurfaceReachability::from_socket_addr(inputs.web_address);
            if matches!(web_reachability, SurfaceReachability::NetworkAccessible) {
                if !self.allow_unaudited_remote_web {
                    errors.push(RuntimeGuardError::RemoteWebDisallowed {
                        environment: self.environment,
                        address: inputs.web_address,
                    });
                }

                if !matches!(
                    ui_api_secret.value_state,
                    SensitiveConfigValueState::NonEmptyInline
                ) && !matches!(ui_api_secret.value_state, SensitiveConfigValueState::Blank)
                {
                    errors.push(RuntimeGuardError::RemoteWebRequiresExplicitUiApiSecret {
                        environment: self.environment,
                        address: inputs.web_address,
                    });
                }
            }

            if let Some(query_address) = inputs.query_address {
                if matches!(
                    SurfaceReachability::from_socket_addr(query_address),
                    SurfaceReachability::NetworkAccessible
                ) && !self.allow_unaudited_remote_query
                {
                    errors.push(RuntimeGuardError::RemoteQueryDisallowed {
                        environment: self.environment,
                        address: query_address,
                    });
                }
            }

            for binding in &inputs.quic_bindings {
                if !self.allow_experimental_quic {
                    errors.push(RuntimeGuardError::ExperimentalQuicDisallowed {
                        environment: self.environment,
                        address: binding.address,
                    });
                }

                if let Err(error) = binding.validate_tls_material() {
                    errors.push(RuntimeGuardError::InvalidQuicTlsMaterial {
                        environment: self.environment,
                        address: binding.address,
                        details: error.to_string(),
                    });
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    pub fn resolve_runtime_layout(&self) -> RuntimeLayout {
        let userdata_dir = common_base::userdata_dir();
        self.resolve_runtime_layout_for(&userdata_dir)
    }

    pub fn surface_inventory(&self, server_settings: &server::Settings) -> Vec<RuntimeSurface> {
        let web_reachability = SurfaceReachability::from_socket_addr(self.web_address);

        let mut surfaces = server_settings
            .gameserver_protocols
            .iter()
            .map(|protocol| RuntimeSurface {
                name: match protocol {
                    server::settings::Protocol::Quic { .. } => "game-quic",
                    server::settings::Protocol::Tcp { .. } => "game-tcp",
                },
                bind_address: Some(protocol.address()),
                reachability: SurfaceReachability::from_socket_addr(protocol.address()),
                auth: SurfaceAuth::RealmHandshake,
                credential_bootstrap: SurfaceCredentialBootstrap::RealmHandshakeFlow,
                review_status: SurfaceReviewStatus::GameplayTransport,
                remote_exposure_policy: SurfaceRemoteExposurePolicy::RemoteAllowedByDesign,
                purpose: SurfacePurpose::PrimaryGameTraffic,
                consumption: SurfaceConsumption::RealmHandshake,
            })
            .collect::<Vec<_>>();

        surfaces.extend([
            RuntimeSurface {
                name: "web-ui",
                bind_address: Some(self.web_address),
                reachability: SurfaceReachability::LoopbackOnly,
                auth: SurfaceAuth::LoopbackUiSession,
                credential_bootstrap: SurfaceCredentialBootstrap::None,
                review_status: SurfaceReviewStatus::PrototypeControlPlaneUnaudited,
                remote_exposure_policy: SurfaceRemoteExposurePolicy::LoopbackRuntimeEnforced,
                purpose: SurfacePurpose::PrototypeControlPlane,
                consumption: SurfaceConsumption::InteractiveSession,
            },
            RuntimeSurface {
                name: "ui-api",
                bind_address: Some(self.web_address),
                reachability: web_reachability,
                auth: if self.ui_api_secret.is_some() {
                    SurfaceAuth::ExplicitSecret
                } else {
                    SurfaceAuth::LoopbackUiSession
                },
                credential_bootstrap: if self.ui_api_secret.is_some() {
                    SurfaceCredentialBootstrap::OperatorProvidedSecret
                } else {
                    SurfaceCredentialBootstrap::LoopbackUiBootstrap
                },
                review_status: SurfaceReviewStatus::PrototypeControlPlaneUnaudited,
                remote_exposure_policy:
                    SurfaceRemoteExposurePolicy::RemoteRequiresExplicitWebOptInAndSecret,
                purpose: SurfacePurpose::PrototypeControlPlane,
                consumption: SurfaceConsumption::ControlApi,
            },
            RuntimeSurface {
                name: "chat-api",
                bind_address: self.web_chat_secret.as_ref().map(|_| self.web_address),
                reachability: if self.web_chat_secret.is_some() {
                    web_reachability
                } else {
                    SurfaceReachability::Disabled
                },
                auth: if self.web_chat_secret.is_some() {
                    SurfaceAuth::ExplicitSecret
                } else {
                    SurfaceAuth::None
                },
                credential_bootstrap: if self.web_chat_secret.is_some() {
                    SurfaceCredentialBootstrap::OperatorProvidedSecret
                } else {
                    SurfaceCredentialBootstrap::None
                },
                review_status: SurfaceReviewStatus::PrototypeControlPlaneUnaudited,
                remote_exposure_policy:
                    SurfaceRemoteExposurePolicy::RemoteRequiresExplicitWebOptInAndSecret,
                purpose: SurfacePurpose::PrototypeControlPlane,
                consumption: SurfaceConsumption::ControlApi,
            },
            RuntimeSurface {
                name: "metrics",
                bind_address: Some(self.web_address),
                reachability: web_reachability,
                auth: SurfaceAuth::None,
                credential_bootstrap: SurfaceCredentialBootstrap::None,
                review_status: SurfaceReviewStatus::InternalObservabilityOnly,
                remote_exposure_policy: SurfaceRemoteExposurePolicy::RemoteRequiresExplicitWebOptIn,
                purpose: SurfacePurpose::InternalObservability,
                consumption: SurfaceConsumption::MachineScrape,
            },
            RuntimeSurface {
                name: "health",
                bind_address: Some(self.web_address),
                reachability: web_reachability,
                auth: SurfaceAuth::None,
                credential_bootstrap: SurfaceCredentialBootstrap::None,
                review_status: SurfaceReviewStatus::InternalObservabilityOnly,
                remote_exposure_policy: SurfaceRemoteExposurePolicy::RemoteRequiresExplicitWebOptIn,
                purpose: SurfacePurpose::InternalObservability,
                consumption: SurfaceConsumption::MachineProbe,
            },
            RuntimeSurface {
                name: "query-server",
                bind_address: server_settings.query_address,
                reachability: server_settings
                    .query_address
                    .map(SurfaceReachability::from_socket_addr)
                    .unwrap_or(SurfaceReachability::Disabled),
                auth: SurfaceAuth::None,
                credential_bootstrap: SurfaceCredentialBootstrap::None,
                review_status: SurfaceReviewStatus::DiscoveryOnlyNotAuthority,
                remote_exposure_policy:
                    SurfaceRemoteExposurePolicy::RemoteRequiresExplicitQueryOptIn,
                purpose: SurfacePurpose::LightweightDiscovery,
                consumption: SurfaceConsumption::DiscoveryQuery,
            },
        ]);

        surfaces
    }

    pub fn sensitive_config_inventory(
        &self,
        server_settings: &server::Settings,
    ) -> Vec<SensitiveConfigInventoryEntry> {
        self.sensitive_config_inventory_for_quic_bindings(
            &server::settings::QuicBinding::from_protocols(&server_settings.gameserver_protocols),
        )
    }

    pub fn management_auth_inventory(
        &self,
        server_settings: &server::Settings,
    ) -> Vec<ManagementAuthInventoryEntry> {
        self.surface_inventory(server_settings)
            .into_iter()
            .filter_map(|surface| match surface.name {
                "web-ui" => Some(ManagementAuthInventoryEntry {
                    surface: surface.name,
                    bind_address: surface.bind_address,
                    reachability: surface.reachability,
                    review_status: surface.review_status,
                    remote_exposure_policy: surface.remote_exposure_policy,
                    capability: ManagementSurfaceCapability::InteractiveBootstrap,
                    auth_scheme: surface.auth,
                    credential_bootstrap: surface.credential_bootstrap,
                    credential_transport: ManagementCredentialTransport::LoopbackRuntimeGuard,
                    secret_config_id: None,
                    proxy_forwarding_forbidden: true,
                    detail: "the /ui bootstrap page is only served to loopback clients, rejects \
                             forwarded/proxied access, and hands out the ui-api session token for \
                             local interactive use"
                        .to_owned(),
                }),
                "ui-api" => Some(ManagementAuthInventoryEntry {
                    surface: surface.name,
                    bind_address: surface.bind_address,
                    reachability: surface.reachability,
                    review_status: surface.review_status,
                    remote_exposure_policy: surface.remote_exposure_policy,
                    capability: ManagementSurfaceCapability::MutatingControl,
                    auth_scheme: surface.auth,
                    credential_bootstrap: surface.credential_bootstrap,
                    credential_transport: ManagementCredentialTransport::CookieSecret,
                    secret_config_id: Some("ui-api-secret"),
                    proxy_forwarding_forbidden: false,
                    detail: if self.ui_api_secret.is_some() {
                        "the /ui_api surface requires the X-Secret-Token cookie carrying an \
                         operator-provided secret; remote use additionally depends on explicit web \
                         opt-in"
                            .to_owned()
                    } else {
                        "the /ui_api surface requires the X-Secret-Token cookie; when \
                         ui_api_secret is unset startup generates a transient token that is only \
                         bootstrapped through the loopback-only /ui page"
                            .to_owned()
                    },
                }),
                "chat-api" => Some(ManagementAuthInventoryEntry {
                    surface: surface.name,
                    bind_address: surface.bind_address,
                    reachability: surface.reachability,
                    review_status: surface.review_status,
                    remote_exposure_policy: surface.remote_exposure_policy,
                    capability: ManagementSurfaceCapability::ReadOnlyOpsData,
                    auth_scheme: surface.auth,
                    credential_bootstrap: surface.credential_bootstrap,
                    credential_transport: if self.web_chat_secret.is_some() {
                        ManagementCredentialTransport::HeaderSecret
                    } else {
                        ManagementCredentialTransport::None
                    },
                    secret_config_id: Some("chat-api-secret"),
                    proxy_forwarding_forbidden: false,
                    detail: if self.web_chat_secret.is_some() {
                        "the /chat endpoint requires the X-Secret-Token request header and remains \
                         part of the unaudited prototype control plane"
                            .to_owned()
                    } else {
                        "the /chat endpoint stays disabled until chat-api-secret is explicitly \
                         configured"
                            .to_owned()
                    },
                }),
                "metrics" => Some(ManagementAuthInventoryEntry {
                    surface: surface.name,
                    bind_address: surface.bind_address,
                    reachability: surface.reachability,
                    review_status: surface.review_status,
                    remote_exposure_policy: surface.remote_exposure_policy,
                    capability: ManagementSurfaceCapability::ObservabilityScrape,
                    auth_scheme: surface.auth,
                    credential_bootstrap: surface.credential_bootstrap,
                    credential_transport: ManagementCredentialTransport::None,
                    secret_config_id: None,
                    proxy_forwarding_forbidden: false,
                    detail: "the /metrics surface has no in-process auth and is only classified \
                             as internal observability infrastructure, not a public control plane"
                        .to_owned(),
                }),
                "health" => Some(ManagementAuthInventoryEntry {
                    surface: surface.name,
                    bind_address: surface.bind_address,
                    reachability: surface.reachability,
                    review_status: surface.review_status,
                    remote_exposure_policy: surface.remote_exposure_policy,
                    capability: ManagementSurfaceCapability::ObservabilityProbe,
                    auth_scheme: surface.auth,
                    credential_bootstrap: surface.credential_bootstrap,
                    credential_transport: ManagementCredentialTransport::None,
                    secret_config_id: None,
                    proxy_forwarding_forbidden: false,
                    detail: "the /health surface has no in-process auth and is treated as \
                             machine-probe observability, not a production control plane"
                        .to_owned(),
                }),
                _ => None,
            })
            .collect()
    }

    pub fn management_auth_review_surfaces_for_environment(
        environment: Environment,
        inventory: &[ManagementAuthInventoryEntry],
    ) -> Vec<&'static str> {
        inventory
            .iter()
            .filter(|entry| {
                environment != Environment::Local
                    && matches!(entry.reachability, SurfaceReachability::NetworkAccessible)
                    && (matches!(
                        entry.review_status,
                        SurfaceReviewStatus::PrototypeControlPlaneUnaudited
                    ) || (matches!(
                        entry.review_status,
                        SurfaceReviewStatus::InternalObservabilityOnly
                    ) && matches!(entry.auth_scheme, SurfaceAuth::None)))
            })
            .map(|entry| entry.surface)
            .collect()
    }

    pub fn transport_security_inventory(
        &self,
        server_settings: &server::Settings,
    ) -> Vec<TransportSecurityInventoryEntry> {
        self.transport_security_inventory_for_quic_bindings(
            &server::settings::QuicBinding::from_protocols(&server_settings.gameserver_protocols),
        )
    }

    pub fn governance_findings(
        &self,
        server_settings: &server::Settings,
    ) -> Vec<RuntimeGovernanceFinding> {
        let surfaces = self.surface_inventory(server_settings);
        let sensitive_configs = self.sensitive_config_inventory(server_settings);
        let mut findings = Vec::new();

        if sensitive_configs.iter().any(|config| {
            config.id == "ui-api-secret"
                && matches!(
                    config.value_state,
                    SensitiveConfigValueState::GeneratedAtStartup
                )
        }) {
            findings.push(RuntimeGovernanceFinding {
                id: "generated-ui-api-bootstrap-active",
                severity: RuntimeGovernanceSeverity::Notice,
                subject: "ui-api",
                detail: "ui_api_secret is not explicitly configured. Startup will generate a \
                         transient token and only hand it out through the loopback-only /ui \
                         bootstrap path."
                    .to_owned(),
            });
        }

        let web_surfaces_remote = surfaces.iter().any(|surface| {
            matches!(surface.reachability, SurfaceReachability::NetworkAccessible)
                && matches!(
                    surface.remote_exposure_policy,
                    SurfaceRemoteExposurePolicy::RemoteRequiresExplicitWebOptIn
                        | SurfaceRemoteExposurePolicy::RemoteRequiresExplicitWebOptInAndSecret
                )
        });
        if web_surfaces_remote && self.allow_unaudited_remote_web {
            findings.push(RuntimeGovernanceFinding {
                id: "remote-unaudited-web-opt-in-active",
                severity: RuntimeGovernanceSeverity::Warning,
                subject: "web-stack",
                detail: format!(
                    "web_address {} is network-accessible and allow_unaudited_remote_web=true is \
                     active. The current Web/UI/metrics/health stack is still treated as \
                     unaudited internal/prototype infrastructure in phase 3.",
                    self.web_address
                ),
            });
        }

        for surface in surfaces.iter().filter(|surface| {
            matches!(surface.reachability, SurfaceReachability::NetworkAccessible)
                && matches!(
                    surface.review_status,
                    SurfaceReviewStatus::PrototypeControlPlaneUnaudited
                )
        }) {
            findings.push(RuntimeGovernanceFinding {
                id: "prototype-control-plane-remote-exposure",
                severity: RuntimeGovernanceSeverity::Warning,
                subject: surface.name,
                detail: format!(
                    "{} is network-accessible but still classified as prototype control plane \
                     without a dedicated security review.",
                    surface.name
                ),
            });
        }

        if let Some(query_surface) = surfaces.iter().find(|surface| {
            surface.name == "query-server"
                && matches!(surface.reachability, SurfaceReachability::NetworkAccessible)
        }) {
            if self.allow_unaudited_remote_query {
                findings.push(RuntimeGovernanceFinding {
                    id: "remote-query-opt-in-active",
                    severity: RuntimeGovernanceSeverity::Warning,
                    subject: query_surface.name,
                    detail: format!(
                        "query_address {} is remotely exposed under \
                         allow_unaudited_remote_query=true. This path is still treated as \
                         lightweight discovery, not a formal realm authority.",
                        query_surface
                            .bind_address
                            .expect("network-accessible query surface should have bind address")
                    ),
                });
            }
        }

        if self.allow_experimental_quic {
            for binding in
                server::settings::QuicBinding::from_protocols(&server_settings.gameserver_protocols)
            {
                findings.push(RuntimeGovernanceFinding {
                    id: "experimental-quic-opt-in-active",
                    severity: RuntimeGovernanceSeverity::Warning,
                    subject: "game-quic",
                    detail: format!(
                        "QUIC transport {} is enabled under allow_experimental_quic=true. Treat \
                         rollout, certificate provenance, and rollback as operator-governed \
                         experimental infrastructure.",
                        binding.address
                    ),
                });
            }
        }

        findings
    }

    fn sensitive_config_inventory_for_quic_bindings(
        &self,
        quic_bindings: &[server::settings::QuicBinding],
    ) -> Vec<SensitiveConfigInventoryEntry> {
        let mut inventory = vec![
            SensitiveConfigInventoryEntry {
                id: "ui-api-secret",
                consumer_surface: "ui-api",
                bind_address: Some(self.web_address),
                file_path: None,
                configured: self.ui_api_secret.is_some(),
                sensitivity: SensitiveConfigSensitivity::SharedSecret,
                source: if self.ui_api_secret.is_some() {
                    SensitiveConfigSource::InlineOperatorProvided
                } else {
                    SensitiveConfigSource::InlineOperatorProvidedOrLoopbackBootstrap
                },
                value_state: secret_value_state(
                    self.ui_api_secret.as_deref(),
                    SensitiveConfigValueState::GeneratedAtStartup,
                ),
                operator_responsibility:
                    SensitiveConfigOperatorResponsibility::OptionalLocalBootstrapWhenUnset,
                exposure_dependency: SensitiveConfigExposureDependency::WebStackOptIn,
            },
            SensitiveConfigInventoryEntry {
                id: "chat-api-secret",
                consumer_surface: "chat-api",
                bind_address: Some(self.web_address),
                file_path: None,
                configured: self.web_chat_secret.is_some(),
                sensitivity: SensitiveConfigSensitivity::SharedSecret,
                source: SensitiveConfigSource::InlineOperatorProvided,
                value_state: secret_value_state(
                    self.web_chat_secret.as_deref(),
                    SensitiveConfigValueState::Unset,
                ),
                operator_responsibility:
                    SensitiveConfigOperatorResponsibility::SurfaceDisabledWhenUnset,
                exposure_dependency: SensitiveConfigExposureDependency::WebStackOptIn,
            },
        ];

        inventory.extend(quic_bindings.iter().cloned().flat_map(|binding| {
            [
                SensitiveConfigInventoryEntry {
                    id: "quic-cert-file",
                    consumer_surface: "game-quic",
                    bind_address: Some(binding.address),
                    file_path: Some(binding.cert_file_path.clone()),
                    configured: true,
                    sensitivity: SensitiveConfigSensitivity::PublicCertificateChain,
                    source: SensitiveConfigSource::FileBackedOperatorManaged,
                    value_state: SensitiveConfigValueState::FilePathConfigured,
                    operator_responsibility:
                        SensitiveConfigOperatorResponsibility::RequiredForQuicBinding,
                    exposure_dependency: SensitiveConfigExposureDependency::ExperimentalQuicOptIn,
                },
                SensitiveConfigInventoryEntry {
                    id: "quic-key-file",
                    consumer_surface: "game-quic",
                    bind_address: Some(binding.address),
                    file_path: Some(binding.key_file_path.clone()),
                    configured: true,
                    sensitivity: SensitiveConfigSensitivity::PrivateKey,
                    source: SensitiveConfigSource::FileBackedOperatorManaged,
                    value_state: SensitiveConfigValueState::FilePathConfigured,
                    operator_responsibility:
                        SensitiveConfigOperatorResponsibility::RequiredForQuicBinding,
                    exposure_dependency: SensitiveConfigExposureDependency::ExperimentalQuicOptIn,
                },
            ]
        }));

        inventory
    }

    fn transport_security_inventory_for_quic_bindings(
        &self,
        quic_bindings: &[server::settings::QuicBinding],
    ) -> Vec<TransportSecurityInventoryEntry> {
        quic_bindings
            .iter()
            .map(|binding| {
                let (material_state, detail) = match binding.validate_tls_material() {
                    Ok(()) => (
                        TransportSecurityMaterialState::Valid,
                        "TLS material parsed successfully and QUIC server config can be built"
                            .to_owned(),
                    ),
                    Err(error) => (TransportSecurityMaterialState::Invalid, error.to_string()),
                };

                TransportSecurityInventoryEntry {
                    surface: "game-quic",
                    bind_address: binding.address,
                    transport: "quic",
                    encryption: "tls-required",
                    cert_file_path: binding.cert_file_path.clone(),
                    key_file_path: binding.key_file_path.clone(),
                    rollout_policy: if self.allow_experimental_quic {
                        TransportSecurityRolloutPolicy::ExperimentalOptInActive
                    } else {
                        TransportSecurityRolloutPolicy::DisabledUntilExplicitOptIn
                    },
                    validation_policy: if self.environment.allows_optional_auth() {
                        TransportSecurityValidationPolicy::AdvisoryAtStartup
                    } else {
                        TransportSecurityValidationPolicy::FailFastAtStartup
                    },
                    material_state,
                    detail,
                }
            })
            .collect()
    }

    fn resolve_runtime_layout_for(&self, userdata_dir: &Path) -> RuntimeLayout {
        self.resolve_runtime_layout_for_overrides(
            userdata_dir,
            std::env::var_os("VELOREN_RTSIM").map(PathBuf::from),
            std::env::var_os("VELOREN_TERRAIN").map(PathBuf::from),
        )
    }

    fn resolve_runtime_layout_for_overrides(
        &self,
        userdata_dir: &Path,
        rtsim_override: Option<PathBuf>,
        terrain_override: Option<PathBuf>,
    ) -> RuntimeLayout {
        let server_cli_settings_dir = data_dir_for(userdata_dir);
        let server_data_dir = self.resolve_server_data_dir_for(userdata_dir);
        let recovery_staging_dir =
            self.resolve_recovery_staging_dir_for(userdata_dir, &server_cli_settings_dir);

        RuntimeLayout {
            userdata_dir: userdata_dir.to_owned(),
            server_cli_settings_dir,
            server_state: server::ServerStatePaths::with_overrides(
                server_data_dir,
                rtsim_override.clone(),
                terrain_override.clone(),
            ),
            recovery_staging_state: server::ServerStatePaths::with_overrides(
                recovery_staging_dir,
                rtsim_override,
                terrain_override,
            ),
        }
    }

    fn resolve_server_data_dir_for(&self, userdata_dir: &Path) -> PathBuf {
        if let Some(path) = self.server_data_dir.as_ref() {
            if path.is_absolute() {
                path.clone()
            } else {
                userdata_dir.join(path)
            }
        } else {
            userdata_dir.join(match self.environment {
                Environment::Local => server::DEFAULT_DATA_DIR_NAME,
                Environment::Test => "server-test",
                Environment::Production => "server-production",
            })
        }
    }

    fn resolve_recovery_staging_dir_for(
        &self,
        userdata_dir: &Path,
        server_cli_settings_dir: &Path,
    ) -> PathBuf {
        if let Some(path) = self.recovery_staging_dir.as_ref() {
            if path.is_absolute() {
                path.clone()
            } else {
                userdata_dir.join(path)
            }
        } else {
            server_cli_settings_dir
                .join("recovery-staging")
                .join(self.environment.as_str())
        }
    }

    fn save_template(path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }

        let ron = ron::ser::to_string_pretty(&Self::default(), ron::ser::PrettyConfig::default())
            .unwrap();
        fs::write(path, ron.as_bytes())
    }

    pub fn get_settings_path() -> PathBuf {
        let mut path = data_dir();
        path.push(Self::FILENAME);
        path
    }
}

pub fn data_dir() -> PathBuf { data_dir_for(&common_base::userdata_dir()) }

fn data_dir_for(userdata_dir: &Path) -> PathBuf {
    let mut path = PathBuf::from(userdata_dir);
    path.push("server-cli");
    path
}

pub fn recovery_drill_overlap_details(
    live: &server::ServerStatePaths,
    staging: &server::ServerStatePaths,
) -> Vec<String> {
    let mut overlaps = Vec::new();
    let live_data_dir = normalize_path(&live.data_dir);
    let staging_data_dir = normalize_path(&staging.data_dir);
    if paths_overlap(&live_data_dir, &staging_data_dir) {
        overlaps.push(format!(
            "live data root {} overlaps recovery staging root {}",
            live.data_dir.display(),
            staging.data_dir.display()
        ));
    }

    overlaps.extend(live.inventory().into_iter().flat_map(|live_entry| {
        staging
            .inventory()
            .into_iter()
            .filter_map(move |staging_entry| {
                let live_path = normalize_path(&live_entry.path);
                let staging_path = normalize_path(&staging_entry.path);
                if paths_overlap(&live_path, &staging_path) {
                    Some(format!(
                        "live {} at {} overlaps recovery staging {} at {}",
                        state_kind_name(live_entry.kind),
                        live_entry.path.display(),
                        state_kind_name(staging_entry.kind),
                        staging_entry.path.display()
                    ))
                } else {
                    None
                }
            })
    }));

    overlaps
}

pub fn runtime_state_layout_conflict_details(paths: &server::ServerStatePaths) -> Vec<String> {
    let entries = paths.inventory();
    let mut conflicts = Vec::new();

    for (index, left_entry) in entries.iter().enumerate() {
        for right_entry in entries.iter().skip(index + 1) {
            if !(runtime_state_layout_blocking_kind(left_entry.kind)
                || runtime_state_layout_blocking_kind(right_entry.kind))
            {
                continue;
            }

            let left_path = normalize_path(&left_entry.path);
            let right_path = normalize_path(&right_entry.path);
            if paths_overlap(&left_path, &right_path) {
                conflicts.push(format!(
                    "live {} at {} overlaps live {} at {}",
                    state_kind_name(left_entry.kind),
                    left_entry.path.display(),
                    state_kind_name(right_entry.kind),
                    right_entry.path.display()
                ));
            }
        }
    }

    conflicts
}

fn state_kind_name(kind: server::ServerStateKind) -> &'static str {
    match kind {
        server::ServerStateKind::ConfigDir => "config-dir",
        server::ServerStateKind::InstanceIdentity => "instance-identity",
        server::ServerStateKind::CharacterDatabase => "character-database",
        server::ServerStateKind::RtSimState => "rtsim-state",
        server::ServerStateKind::TerrainPersistence => "terrain-persistence",
        server::ServerStateKind::OperationalAuditTrail => "operational-audit-trail",
        server::ServerStateKind::BackupEvidenceTrail => "backup-evidence-trail",
        server::ServerStateKind::RecoveryDrillEvidenceTrail => "recovery-drill-evidence-trail",
    }
}

fn runtime_state_layout_blocking_kind(kind: server::ServerStateKind) -> bool {
    matches!(
        kind,
        server::ServerStateKind::ConfigDir
            | server::ServerStateKind::InstanceIdentity
            | server::ServerStateKind::CharacterDatabase
            | server::ServerStateKind::RtSimState
            | server::ServerStateKind::TerrainPersistence
    )
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {},
            Component::ParentDir => {
                let _ = normalized.pop();
            },
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn secret_value_state(
    secret: Option<&str>,
    unset_state: SensitiveConfigValueState,
) -> SensitiveConfigValueState {
    match secret {
        Some(secret) if secret.trim().is_empty() => SensitiveConfigValueState::Blank,
        Some(_) => SensitiveConfigValueState::NonEmptyInline,
        None => unset_state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn runtime_inputs(settings: &Settings) -> RuntimeGuardInputs {
        RuntimeGuardInputs {
            no_auth_cli: false,
            auth_server_configured: true,
            web_address: settings.web_address,
            query_address: None,
            quic_bindings: Vec::new(),
        }
    }

    fn unique_temp_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("caldrayne-{name}-{unique}.tmp"))
    }

    #[test]
    fn legacy_settings_without_environment_field_default_to_local() {
        let ron = r#"(
            update_shutdown_grace_period_secs: 120,
            update_shutdown_message: "The server is restarting for an update",
            web_address: "127.0.0.1:14005",
            web_chat_secret: None,
            ui_api_secret: None,
            shutdown_signals: [],
        )"#;

        let settings: Settings = ron::from_str(ron).expect("legacy settings should deserialize");
        assert_eq!(settings.environment, Environment::Local);
        assert_eq!(settings.audit_retention, AuditRetentionPolicy::default());
    }

    #[test]
    fn local_environment_allows_optional_auth() {
        let settings = Settings::default();
        let inputs = RuntimeGuardInputs {
            no_auth_cli: true,
            auth_server_configured: false,
            ..runtime_inputs(&settings)
        };

        assert_eq!(settings.validate_runtime(inputs), Ok(()));
    }

    #[test]
    fn test_environment_rejects_no_auth_and_missing_auth_provider() {
        let settings = Settings {
            environment: Environment::Test,
            ..Settings::default()
        };
        let inputs = RuntimeGuardInputs {
            no_auth_cli: true,
            auth_server_configured: false,
            ..runtime_inputs(&settings)
        };

        assert_eq!(
            settings.validate_runtime(inputs),
            Err(vec![
                RuntimeGuardError::NoAuthDisallowed {
                    environment: Environment::Test,
                },
                RuntimeGuardError::MissingAuthProvider {
                    environment: Environment::Test,
                },
            ])
        );
    }

    #[test]
    fn production_environment_accepts_explicit_auth_configuration() {
        let settings = Settings {
            environment: Environment::Production,
            ..Settings::default()
        };
        let inputs = runtime_inputs(&settings);

        assert_eq!(settings.validate_runtime(inputs), Ok(()));
    }

    #[test]
    fn test_environment_rejects_remote_web_without_opt_in() {
        let settings = Settings {
            environment: Environment::Test,
            web_address: "0.0.0.0:14005".parse().unwrap(),
            ui_api_secret: Some("ui-secret".into()),
            ..Settings::default()
        };
        let inputs = runtime_inputs(&settings);

        assert_eq!(
            settings.validate_runtime(inputs),
            Err(vec![RuntimeGuardError::RemoteWebDisallowed {
                environment: Environment::Test,
                address: "0.0.0.0:14005".parse().unwrap(),
            },])
        );
    }

    #[test]
    fn production_environment_remote_web_requires_explicit_ui_api_secret() {
        let settings = Settings {
            environment: Environment::Production,
            allow_unaudited_remote_web: true,
            web_address: "0.0.0.0:14005".parse().unwrap(),
            ..Settings::default()
        };
        let inputs = runtime_inputs(&settings);

        assert_eq!(
            settings.validate_runtime(inputs),
            Err(vec![
                RuntimeGuardError::RemoteWebRequiresExplicitUiApiSecret {
                    environment: Environment::Production,
                    address: "0.0.0.0:14005".parse().unwrap(),
                },
            ])
        );
    }

    #[test]
    fn production_environment_accepts_remote_web_with_opt_in_and_explicit_secret() {
        let settings = Settings {
            environment: Environment::Production,
            allow_unaudited_remote_web: true,
            web_address: "0.0.0.0:14005".parse().unwrap(),
            ui_api_secret: Some("ui-secret".into()),
            ..Settings::default()
        };
        let inputs = runtime_inputs(&settings);

        assert_eq!(settings.validate_runtime(inputs), Ok(()));
    }

    #[test]
    fn local_environment_rejects_blank_ui_api_secret() {
        let settings = Settings {
            ui_api_secret: Some("   ".into()),
            ..Settings::default()
        };

        assert_eq!(
            settings.validate_runtime(runtime_inputs(&settings)),
            Err(vec![RuntimeGuardError::BlankSensitiveConfig {
                environment: Environment::Local,
                config_id: "ui-api-secret",
                consumer_surface: "ui-api",
            }])
        );
    }

    #[test]
    fn local_environment_rejects_blank_web_chat_secret() {
        let settings = Settings {
            web_chat_secret: Some("   ".into()),
            ..Settings::default()
        };

        assert_eq!(
            settings.validate_runtime(runtime_inputs(&settings)),
            Err(vec![RuntimeGuardError::BlankSensitiveConfig {
                environment: Environment::Local,
                config_id: "chat-api-secret",
                consumer_surface: "chat-api",
            }])
        );
    }

    #[test]
    fn production_environment_remote_web_with_blank_ui_api_secret_fails_fast() {
        let settings = Settings {
            environment: Environment::Production,
            allow_unaudited_remote_web: true,
            web_address: "0.0.0.0:14005".parse().unwrap(),
            ui_api_secret: Some("   ".into()),
            ..Settings::default()
        };

        assert_eq!(
            settings.validate_runtime(runtime_inputs(&settings)),
            Err(vec![RuntimeGuardError::BlankSensitiveConfig {
                environment: Environment::Production,
                config_id: "ui-api-secret",
                consumer_surface: "ui-api",
            }])
        );
    }

    #[test]
    fn governance_findings_include_generated_ui_bootstrap_notice_by_default() {
        let settings = Settings::default();

        let findings = settings.governance_findings(&server::Settings::default());

        assert!(findings.iter().any(|finding| {
            finding.id == "generated-ui-api-bootstrap-active"
                && finding.severity == RuntimeGovernanceSeverity::Notice
                && finding.subject == "ui-api"
        }));
    }

    #[test]
    fn governance_findings_capture_explicit_remote_risk_acceptance() {
        let settings = Settings {
            environment: Environment::Production,
            allow_unaudited_remote_web: true,
            allow_unaudited_remote_query: true,
            allow_experimental_quic: true,
            web_address: "0.0.0.0:14005".parse().unwrap(),
            ui_api_secret: Some("ui-secret".into()),
            web_chat_secret: Some("chat-secret".into()),
            ..Settings::default()
        };
        let server_settings = server::Settings {
            query_address: Some("0.0.0.0:14006".parse().unwrap()),
            gameserver_protocols: vec![server::settings::Protocol::Quic {
                address: "0.0.0.0:14004".parse().unwrap(),
                cert_file_path: PathBuf::from("tls/cert.pem"),
                key_file_path: PathBuf::from("tls/key.pem"),
            }],
            ..server::Settings::default()
        };

        let findings = settings.governance_findings(&server_settings);

        assert!(findings.iter().any(|finding| {
            finding.id == "remote-unaudited-web-opt-in-active"
                && finding.severity == RuntimeGovernanceSeverity::Warning
                && finding.subject == "web-stack"
        }));
        assert!(findings.iter().any(|finding| {
            finding.id == "prototype-control-plane-remote-exposure"
                && finding.severity == RuntimeGovernanceSeverity::Warning
                && finding.subject == "ui-api"
        }));
        assert!(findings.iter().any(|finding| {
            finding.id == "prototype-control-plane-remote-exposure"
                && finding.severity == RuntimeGovernanceSeverity::Warning
                && finding.subject == "chat-api"
        }));
        assert!(findings.iter().any(|finding| {
            finding.id == "remote-query-opt-in-active"
                && finding.severity == RuntimeGovernanceSeverity::Warning
                && finding.subject == "query-server"
        }));
        assert!(findings.iter().any(|finding| {
            finding.id == "experimental-quic-opt-in-active"
                && finding.severity == RuntimeGovernanceSeverity::Warning
                && finding.subject == "game-quic"
        }));
    }

    #[test]
    fn test_environment_rejects_remote_query_without_opt_in() {
        let settings = Settings {
            environment: Environment::Test,
            ..Settings::default()
        };
        let inputs = RuntimeGuardInputs {
            query_address: Some("0.0.0.0:14006".parse().unwrap()),
            ..runtime_inputs(&settings)
        };

        assert_eq!(
            settings.validate_runtime(inputs),
            Err(vec![RuntimeGuardError::RemoteQueryDisallowed {
                environment: Environment::Test,
                address: "0.0.0.0:14006".parse().unwrap(),
            },])
        );
    }

    #[test]
    fn production_environment_accepts_remote_query_with_opt_in() {
        let settings = Settings {
            environment: Environment::Production,
            allow_unaudited_remote_query: true,
            ..Settings::default()
        };
        let inputs = RuntimeGuardInputs {
            query_address: Some("0.0.0.0:14006".parse().unwrap()),
            ..runtime_inputs(&settings)
        };

        assert_eq!(settings.validate_runtime(inputs), Ok(()));
    }

    #[test]
    fn test_environment_rejects_quic_without_explicit_opt_in() {
        let settings = Settings {
            environment: Environment::Test,
            ..Settings::default()
        };
        let inputs = RuntimeGuardInputs {
            quic_bindings: vec![server::settings::QuicBinding {
                address: "127.0.0.1:14004".parse().unwrap(),
                cert_file_path: PathBuf::from("cert.pem"),
                key_file_path: PathBuf::from("key.pem"),
            }],
            ..runtime_inputs(&settings)
        };

        let result = settings
            .validate_runtime(inputs)
            .expect_err("test env should reject quic");

        assert!(matches!(
            result.first(),
            Some(RuntimeGuardError::ExperimentalQuicDisallowed {
                environment: Environment::Test,
                address,
            }) if *address == "127.0.0.1:14004".parse().unwrap()
        ));
        assert!(matches!(
            result.get(1),
            Some(RuntimeGuardError::InvalidQuicTlsMaterial {
                environment: Environment::Test,
                address,
                details,
            }) if *address == "127.0.0.1:14004".parse().unwrap() && !details.is_empty()
        ));
    }

    #[test]
    fn production_environment_rejects_invalid_quic_tls_material() {
        let cert_path = unique_temp_path("invalid-cert");
        let key_path = unique_temp_path("invalid-key");
        fs::write(&cert_path, b"not a certificate").expect("should write temp cert file");
        fs::write(&key_path, b"not a key").expect("should write temp key file");

        let settings = Settings {
            environment: Environment::Production,
            allow_experimental_quic: true,
            ..Settings::default()
        };
        let inputs = RuntimeGuardInputs {
            quic_bindings: vec![server::settings::QuicBinding {
                address: "0.0.0.0:14004".parse().unwrap(),
                cert_file_path: cert_path.clone(),
                key_file_path: key_path.clone(),
            }],
            ..runtime_inputs(&settings)
        };

        let result = settings.validate_runtime(inputs);

        let _ = fs::remove_file(cert_path);
        let _ = fs::remove_file(key_path);

        let errors = result.expect_err("production env should reject invalid quic tls");
        assert!(matches!(
            errors.as_slice(),
            [RuntimeGuardError::InvalidQuicTlsMaterial {
                environment: Environment::Production,
                address,
                details,
            }] if *address == "0.0.0.0:14004".parse().unwrap() && !details.is_empty()
        ));
    }

    #[test]
    fn local_environment_uses_legacy_server_data_dir_by_default() {
        let userdata_dir = PathBuf::from("userdata-root");
        let settings = Settings::default();

        let layout = settings.resolve_runtime_layout_for_overrides(&userdata_dir, None, None);

        assert_eq!(layout.server_state.data_dir, userdata_dir.join("server"));
        assert_eq!(
            layout.server_state.database_file,
            userdata_dir.join("server").join("saves").join("db.sqlite")
        );
        assert_eq!(
            layout.server_state.identity_file,
            userdata_dir.join("server").join("identity.ron")
        );
        assert_eq!(
            layout.recovery_staging_state.data_dir,
            userdata_dir
                .join("server-cli")
                .join("recovery-staging")
                .join("local")
        );
    }

    #[test]
    fn test_environment_uses_dedicated_server_data_dir_by_default() {
        let userdata_dir = PathBuf::from("userdata-root");
        let settings = Settings {
            environment: Environment::Test,
            ..Settings::default()
        };

        let layout = settings.resolve_runtime_layout_for_overrides(&userdata_dir, None, None);

        assert_eq!(
            layout.server_state.data_dir,
            userdata_dir.join("server-test")
        );
        assert_eq!(
            layout.server_state.config_dir,
            userdata_dir.join("server-test").join("server_config")
        );
        assert_eq!(
            layout.recovery_staging_state.data_dir,
            userdata_dir
                .join("server-cli")
                .join("recovery-staging")
                .join("test")
        );
    }

    #[test]
    fn production_environment_uses_dedicated_server_data_dir_by_default() {
        let userdata_dir = PathBuf::from("userdata-root");
        let settings = Settings {
            environment: Environment::Production,
            ..Settings::default()
        };

        let layout = settings.resolve_runtime_layout_for_overrides(&userdata_dir, None, None);

        assert_eq!(
            layout.server_state.data_dir,
            userdata_dir.join("server-production")
        );
        assert_eq!(
            layout.server_state.rtsim_data_file,
            userdata_dir
                .join("server-production")
                .join("rtsim")
                .join("data.dat")
        );
        assert_eq!(
            layout.recovery_staging_state.data_dir,
            userdata_dir
                .join("server-cli")
                .join("recovery-staging")
                .join("production")
        );
    }

    #[test]
    fn relative_server_data_dir_override_is_resolved_from_userdata_root() {
        let userdata_dir = PathBuf::from("userdata-root");
        let settings = Settings {
            server_data_dir: Some(PathBuf::from("environments").join("staging-a")),
            ..Settings::default()
        };

        let layout = settings.resolve_runtime_layout_for_overrides(&userdata_dir, None, None);

        assert_eq!(
            layout.server_state.data_dir,
            userdata_dir.join("environments").join("staging-a")
        );
    }

    #[test]
    fn relative_recovery_staging_dir_override_is_resolved_from_userdata_root() {
        let userdata_dir = PathBuf::from("userdata-root");
        let settings = Settings {
            recovery_staging_dir: Some(PathBuf::from("ops").join("restore-sandbox")),
            ..Settings::default()
        };

        let layout = settings.resolve_runtime_layout_for_overrides(&userdata_dir, None, None);

        assert_eq!(
            layout.recovery_staging_state.data_dir,
            userdata_dir.join("ops").join("restore-sandbox")
        );
    }

    #[test]
    fn explicit_runtime_overrides_are_reflected_in_layout() {
        let userdata_dir = PathBuf::from("userdata-root");
        let settings = Settings {
            environment: Environment::Production,
            ..Settings::default()
        };

        let layout = settings.resolve_runtime_layout_for_overrides(
            &userdata_dir,
            Some(PathBuf::from("D:/state/rtsim")),
            Some(PathBuf::from("E:/state/terrain")),
        );

        assert_eq!(
            layout.server_state.rtsim_data_file,
            PathBuf::from("D:/state/rtsim").join("data.dat")
        );
        assert_eq!(
            layout.server_state.terrain_dir,
            PathBuf::from("E:/state/terrain")
        );
        assert_eq!(
            layout.recovery_staging_state.rtsim_data_file,
            PathBuf::from("D:/state/rtsim").join("data.dat")
        );
    }

    #[test]
    fn recovery_drill_overlap_details_reports_shared_paths() {
        let live = server::ServerStatePaths::with_overrides(
            PathBuf::from("userdata/server"),
            Some(PathBuf::from("D:/shared-rtsim")),
            None,
        );
        let staging = server::ServerStatePaths::with_overrides(
            PathBuf::from("userdata/server-cli/recovery-staging/test"),
            Some(PathBuf::from("D:/shared-rtsim")),
            None,
        );

        let overlaps = recovery_drill_overlap_details(&live, &staging);

        assert!(overlaps.iter().any(|detail| detail.contains("rtsim-state")));
    }

    #[test]
    fn recovery_drill_overlap_details_reports_staging_under_live_root() {
        let live =
            server::ServerStatePaths::with_overrides(PathBuf::from("userdata/server"), None, None);
        let staging = server::ServerStatePaths::with_overrides(
            PathBuf::from("userdata/server").join("recovery-staging"),
            None,
            None,
        );

        let overlaps = recovery_drill_overlap_details(&live, &staging);

        assert!(
            overlaps
                .iter()
                .any(|detail| detail.contains("live data root"))
        );
    }

    #[test]
    fn recovery_drill_overlap_details_is_empty_for_separate_layouts() {
        let live =
            server::ServerStatePaths::with_overrides(PathBuf::from("userdata/server"), None, None);
        let staging = server::ServerStatePaths::with_overrides(
            PathBuf::from("userdata/server-cli/recovery-staging/test"),
            None,
            None,
        );

        assert!(recovery_drill_overlap_details(&live, &staging).is_empty());
    }

    #[test]
    fn runtime_state_layout_conflict_details_reports_terrain_overlap_with_ops_trail() {
        let paths = server::ServerStatePaths::with_overrides(
            PathBuf::from("userdata/server"),
            None,
            Some(PathBuf::from("userdata/server/ops")),
        );

        let conflicts = runtime_state_layout_conflict_details(&paths);

        assert!(
            conflicts
                .iter()
                .any(|detail| detail.contains("terrain-persistence")
                    && detail.contains("operational-audit-trail"))
        );
    }

    #[test]
    fn runtime_state_layout_conflict_details_is_empty_for_standard_layout() {
        let paths =
            server::ServerStatePaths::with_overrides(PathBuf::from("userdata/server"), None, None);

        assert!(runtime_state_layout_conflict_details(&paths).is_empty());
    }

    #[test]
    fn surface_inventory_marks_metrics_as_network_accessible_when_web_binds_remotely() {
        let settings = Settings {
            web_address: "0.0.0.0:14005".parse().unwrap(),
            ui_api_secret: Some("ui-secret".into()),
            web_chat_secret: Some("chat-secret".into()),
            ..Settings::default()
        };
        let server_settings = server::Settings {
            query_address: Some("0.0.0.0:14006".parse().unwrap()),
            gameserver_protocols: vec![server::settings::Protocol::Quic {
                address: "0.0.0.0:14004".parse().unwrap(),
                cert_file_path: PathBuf::from("cert.der"),
                key_file_path: PathBuf::from("key.der"),
            }],
            ..server::Settings::default()
        };

        let inventory = settings.surface_inventory(&server_settings);

        assert_eq!(inventory[0].name, "game-quic");
        assert_eq!(inventory[0].auth, SurfaceAuth::RealmHandshake);
        assert_eq!(
            inventory[0].credential_bootstrap,
            SurfaceCredentialBootstrap::RealmHandshakeFlow
        );
        assert_eq!(
            inventory[0].review_status,
            SurfaceReviewStatus::GameplayTransport
        );
        assert_eq!(
            inventory[0].remote_exposure_policy,
            SurfaceRemoteExposurePolicy::RemoteAllowedByDesign
        );
        assert_eq!(inventory[0].purpose, SurfacePurpose::PrimaryGameTraffic);
        assert_eq!(inventory[0].consumption, SurfaceConsumption::RealmHandshake);
        assert_eq!(
            inventory
                .iter()
                .find(|surface| surface.name == "metrics")
                .expect("metrics surface should exist")
                .review_status,
            SurfaceReviewStatus::InternalObservabilityOnly
        );
        assert_eq!(
            inventory
                .iter()
                .find(|surface| surface.name == "metrics")
                .expect("metrics surface should exist")
                .remote_exposure_policy,
            SurfaceRemoteExposurePolicy::RemoteRequiresExplicitWebOptIn
        );
        assert_eq!(
            inventory
                .iter()
                .find(|surface| surface.name == "health")
                .expect("health surface should exist")
                .review_status,
            SurfaceReviewStatus::InternalObservabilityOnly
        );
        assert_eq!(
            inventory
                .iter()
                .find(|surface| surface.name == "health")
                .expect("health surface should exist")
                .remote_exposure_policy,
            SurfaceRemoteExposurePolicy::RemoteRequiresExplicitWebOptIn
        );
        assert_eq!(
            inventory
                .iter()
                .find(|surface| surface.name == "query-server")
                .expect("query-server surface should exist")
                .review_status,
            SurfaceReviewStatus::DiscoveryOnlyNotAuthority
        );
        assert_eq!(
            inventory
                .iter()
                .find(|surface| surface.name == "query-server")
                .expect("query-server surface should exist")
                .remote_exposure_policy,
            SurfaceRemoteExposurePolicy::RemoteRequiresExplicitQueryOptIn
        );
        assert_eq!(
            inventory
                .iter()
                .find(|surface| surface.name == "metrics")
                .expect("metrics surface should exist")
                .consumption,
            SurfaceConsumption::MachineScrape
        );
        assert_eq!(
            inventory
                .iter()
                .find(|surface| surface.name == "health")
                .expect("health surface should exist")
                .consumption,
            SurfaceConsumption::MachineProbe
        );
        assert_eq!(
            inventory[2].reachability,
            SurfaceReachability::NetworkAccessible
        );
        assert_eq!(
            inventory[3].reachability,
            SurfaceReachability::NetworkAccessible
        );
        assert_eq!(
            inventory[4].reachability,
            SurfaceReachability::NetworkAccessible
        );
        assert_eq!(
            inventory[5].reachability,
            SurfaceReachability::NetworkAccessible
        );
        assert_eq!(
            inventory[6].reachability,
            SurfaceReachability::NetworkAccessible
        );
    }

    #[test]
    fn ui_api_without_explicit_secret_keeps_loopback_bootstrap_but_not_fake_loopback_reachability()
    {
        let settings = Settings {
            web_address: "0.0.0.0:14005".parse().unwrap(),
            ..Settings::default()
        };

        let inventory = settings.surface_inventory(&server::Settings::default());

        let ui_api = inventory
            .iter()
            .find(|surface| surface.name == "ui-api")
            .expect("ui-api surface should exist");
        assert_eq!(ui_api.reachability, SurfaceReachability::NetworkAccessible);
        assert_eq!(ui_api.auth, SurfaceAuth::LoopbackUiSession);
        assert_eq!(
            ui_api.credential_bootstrap,
            SurfaceCredentialBootstrap::LoopbackUiBootstrap
        );
        assert_eq!(
            ui_api.remote_exposure_policy,
            SurfaceRemoteExposurePolicy::RemoteRequiresExplicitWebOptInAndSecret
        );
        let web_ui = inventory
            .iter()
            .find(|surface| surface.name == "web-ui")
            .expect("web-ui surface should exist");
        assert_eq!(web_ui.reachability, SurfaceReachability::LoopbackOnly);
        assert_eq!(
            web_ui.remote_exposure_policy,
            SurfaceRemoteExposurePolicy::LoopbackRuntimeEnforced
        );
        assert_eq!(
            inventory
                .iter()
                .find(|surface| surface.name == "metrics")
                .expect("metrics surface should exist")
                .consumption,
            SurfaceConsumption::MachineScrape
        );
        assert_eq!(
            inventory
                .iter()
                .find(|surface| surface.name == "health")
                .expect("health surface should exist")
                .consumption,
            SurfaceConsumption::MachineProbe
        );
    }

    #[test]
    fn management_auth_inventory_distinguishes_cookie_header_and_unauthenticated_surfaces() {
        let settings = Settings {
            web_address: "0.0.0.0:14005".parse().unwrap(),
            ui_api_secret: Some("ui-secret".into()),
            web_chat_secret: Some("chat-secret".into()),
            ..Settings::default()
        };

        let inventory = settings.management_auth_inventory(&server::Settings::default());

        let ui_api = inventory
            .iter()
            .find(|entry| entry.surface == "ui-api")
            .expect("ui-api entry should exist");
        assert_eq!(
            ui_api.capability,
            ManagementSurfaceCapability::MutatingControl
        );
        assert_eq!(
            ui_api.credential_transport,
            ManagementCredentialTransport::CookieSecret
        );
        assert_eq!(ui_api.secret_config_id, Some("ui-api-secret"));
        assert_eq!(ui_api.auth_scheme, SurfaceAuth::ExplicitSecret);

        let chat_api = inventory
            .iter()
            .find(|entry| entry.surface == "chat-api")
            .expect("chat-api entry should exist");
        assert_eq!(
            chat_api.capability,
            ManagementSurfaceCapability::ReadOnlyOpsData
        );
        assert_eq!(
            chat_api.credential_transport,
            ManagementCredentialTransport::HeaderSecret
        );
        assert_eq!(chat_api.secret_config_id, Some("chat-api-secret"));
        assert_eq!(chat_api.auth_scheme, SurfaceAuth::ExplicitSecret);

        let metrics = inventory
            .iter()
            .find(|entry| entry.surface == "metrics")
            .expect("metrics entry should exist");
        assert_eq!(
            metrics.capability,
            ManagementSurfaceCapability::ObservabilityScrape
        );
        assert_eq!(
            metrics.credential_transport,
            ManagementCredentialTransport::None
        );
        assert_eq!(metrics.auth_scheme, SurfaceAuth::None);

        let health = inventory
            .iter()
            .find(|entry| entry.surface == "health")
            .expect("health entry should exist");
        assert_eq!(
            health.capability,
            ManagementSurfaceCapability::ObservabilityProbe
        );
        assert_eq!(
            health.credential_transport,
            ManagementCredentialTransport::None
        );
        assert_eq!(health.auth_scheme, SurfaceAuth::None);
    }

    #[test]
    fn management_auth_inventory_marks_loopback_ui_bootstrap_and_disabled_chat_surface() {
        let settings = Settings::default();

        let inventory = settings.management_auth_inventory(&server::Settings::default());

        let web_ui = inventory
            .iter()
            .find(|entry| entry.surface == "web-ui")
            .expect("web-ui entry should exist");
        assert_eq!(
            web_ui.capability,
            ManagementSurfaceCapability::InteractiveBootstrap
        );
        assert_eq!(
            web_ui.credential_transport,
            ManagementCredentialTransport::LoopbackRuntimeGuard
        );
        assert!(web_ui.proxy_forwarding_forbidden);
        assert_eq!(web_ui.auth_scheme, SurfaceAuth::LoopbackUiSession);

        let ui_api = inventory
            .iter()
            .find(|entry| entry.surface == "ui-api")
            .expect("ui-api entry should exist");
        assert_eq!(
            ui_api.credential_bootstrap,
            SurfaceCredentialBootstrap::LoopbackUiBootstrap
        );
        assert_eq!(ui_api.secret_config_id, Some("ui-api-secret"));

        let chat_api = inventory
            .iter()
            .find(|entry| entry.surface == "chat-api")
            .expect("chat-api entry should exist");
        assert_eq!(chat_api.reachability, SurfaceReachability::Disabled);
        assert_eq!(chat_api.bind_address, None);
        assert_eq!(
            chat_api.credential_transport,
            ManagementCredentialTransport::None
        );
        assert!(chat_api.detail.contains("disabled"));
    }

    #[test]
    fn management_auth_review_surfaces_match_non_local_review_policy() {
        let inventory = vec![
            ManagementAuthInventoryEntry {
                surface: "ui-api",
                bind_address: Some("0.0.0.0:14005".parse().unwrap()),
                reachability: SurfaceReachability::NetworkAccessible,
                review_status: SurfaceReviewStatus::PrototypeControlPlaneUnaudited,
                remote_exposure_policy:
                    SurfaceRemoteExposurePolicy::RemoteRequiresExplicitWebOptInAndSecret,
                capability: ManagementSurfaceCapability::MutatingControl,
                auth_scheme: SurfaceAuth::ExplicitSecret,
                credential_bootstrap: SurfaceCredentialBootstrap::OperatorProvidedSecret,
                credential_transport: ManagementCredentialTransport::CookieSecret,
                secret_config_id: Some("ui-api-secret"),
                proxy_forwarding_forbidden: false,
                detail: "ui api requires cookie secret".to_owned(),
            },
            ManagementAuthInventoryEntry {
                surface: "metrics",
                bind_address: Some("0.0.0.0:14005".parse().unwrap()),
                reachability: SurfaceReachability::NetworkAccessible,
                review_status: SurfaceReviewStatus::InternalObservabilityOnly,
                remote_exposure_policy: SurfaceRemoteExposurePolicy::RemoteRequiresExplicitWebOptIn,
                capability: ManagementSurfaceCapability::ObservabilityScrape,
                auth_scheme: SurfaceAuth::None,
                credential_bootstrap: SurfaceCredentialBootstrap::None,
                credential_transport: ManagementCredentialTransport::None,
                secret_config_id: None,
                proxy_forwarding_forbidden: false,
                detail: "metrics has no in-process auth".to_owned(),
            },
            ManagementAuthInventoryEntry {
                surface: "health",
                bind_address: Some("127.0.0.1:14005".parse().unwrap()),
                reachability: SurfaceReachability::LoopbackOnly,
                review_status: SurfaceReviewStatus::InternalObservabilityOnly,
                remote_exposure_policy: SurfaceRemoteExposurePolicy::LoopbackRuntimeEnforced,
                capability: ManagementSurfaceCapability::ObservabilityProbe,
                auth_scheme: SurfaceAuth::None,
                credential_bootstrap: SurfaceCredentialBootstrap::None,
                credential_transport: ManagementCredentialTransport::None,
                secret_config_id: None,
                proxy_forwarding_forbidden: false,
                detail: "health stays loopback".to_owned(),
            },
        ];

        let review_surfaces = Settings::management_auth_review_surfaces_for_environment(
            Environment::Production,
            &inventory,
        );

        assert_eq!(review_surfaces, vec!["ui-api", "metrics"]);
    }

    #[test]
    fn management_auth_review_surfaces_are_empty_in_local_environment() {
        let inventory = vec![ManagementAuthInventoryEntry {
            surface: "metrics",
            bind_address: Some("0.0.0.0:14005".parse().unwrap()),
            reachability: SurfaceReachability::NetworkAccessible,
            review_status: SurfaceReviewStatus::InternalObservabilityOnly,
            remote_exposure_policy: SurfaceRemoteExposurePolicy::RemoteRequiresExplicitWebOptIn,
            capability: ManagementSurfaceCapability::ObservabilityScrape,
            auth_scheme: SurfaceAuth::None,
            credential_bootstrap: SurfaceCredentialBootstrap::None,
            credential_transport: ManagementCredentialTransport::None,
            secret_config_id: None,
            proxy_forwarding_forbidden: false,
            detail: "metrics has no in-process auth".to_owned(),
        }];

        assert!(
            Settings::management_auth_review_surfaces_for_environment(
                Environment::Local,
                &inventory
            )
            .is_empty()
        );
    }

    #[test]
    fn sensitive_config_inventory_tracks_secret_sources_and_quic_file_material() {
        let settings = Settings {
            web_address: "0.0.0.0:14005".parse().unwrap(),
            ..Settings::default()
        };
        let server_settings = server::Settings {
            gameserver_protocols: vec![server::settings::Protocol::Quic {
                address: "0.0.0.0:14004".parse().unwrap(),
                cert_file_path: PathBuf::from("tls/cert.pem"),
                key_file_path: PathBuf::from("tls/key.pem"),
            }],
            ..server::Settings::default()
        };

        let inventory = settings.sensitive_config_inventory(&server_settings);

        let ui_api_secret = inventory
            .iter()
            .find(|entry| entry.id == "ui-api-secret")
            .expect("ui-api secret entry should exist");
        assert_eq!(ui_api_secret.consumer_surface, "ui-api");
        assert!(!ui_api_secret.configured);
        assert_eq!(
            ui_api_secret.source,
            SensitiveConfigSource::InlineOperatorProvidedOrLoopbackBootstrap
        );
        assert_eq!(
            ui_api_secret.value_state,
            SensitiveConfigValueState::GeneratedAtStartup
        );
        assert_eq!(
            ui_api_secret.operator_responsibility,
            SensitiveConfigOperatorResponsibility::OptionalLocalBootstrapWhenUnset
        );
        assert_eq!(
            ui_api_secret.exposure_dependency,
            SensitiveConfigExposureDependency::WebStackOptIn
        );

        let chat_api_secret = inventory
            .iter()
            .find(|entry| entry.id == "chat-api-secret")
            .expect("chat-api secret entry should exist");
        assert_eq!(chat_api_secret.consumer_surface, "chat-api");
        assert!(!chat_api_secret.configured);
        assert_eq!(
            chat_api_secret.value_state,
            SensitiveConfigValueState::Unset
        );
        assert_eq!(
            chat_api_secret.operator_responsibility,
            SensitiveConfigOperatorResponsibility::SurfaceDisabledWhenUnset
        );
        assert_eq!(
            chat_api_secret.exposure_dependency,
            SensitiveConfigExposureDependency::WebStackOptIn
        );

        let quic_cert = inventory
            .iter()
            .find(|entry| entry.id == "quic-cert-file")
            .expect("quic cert entry should exist");
        assert_eq!(quic_cert.consumer_surface, "game-quic");
        assert_eq!(
            quic_cert.file_path.as_deref(),
            Some(Path::new("tls/cert.pem"))
        );
        assert_eq!(
            quic_cert.sensitivity,
            SensitiveConfigSensitivity::PublicCertificateChain
        );
        assert_eq!(
            quic_cert.source,
            SensitiveConfigSource::FileBackedOperatorManaged
        );
        assert_eq!(
            quic_cert.value_state,
            SensitiveConfigValueState::FilePathConfigured
        );
        assert_eq!(
            quic_cert.operator_responsibility,
            SensitiveConfigOperatorResponsibility::RequiredForQuicBinding
        );
        assert_eq!(
            quic_cert.exposure_dependency,
            SensitiveConfigExposureDependency::ExperimentalQuicOptIn
        );

        let quic_key = inventory
            .iter()
            .find(|entry| entry.id == "quic-key-file")
            .expect("quic key entry should exist");
        assert_eq!(
            quic_key.file_path.as_deref(),
            Some(Path::new("tls/key.pem"))
        );
        assert_eq!(quic_key.sensitivity, SensitiveConfigSensitivity::PrivateKey);
        assert_eq!(
            quic_key.value_state,
            SensitiveConfigValueState::FilePathConfigured
        );
        assert_eq!(
            quic_key.source,
            SensitiveConfigSource::FileBackedOperatorManaged
        );
    }

    #[test]
    fn transport_security_inventory_marks_local_quic_as_advisory_until_opt_in() {
        let cert_path = unique_temp_path("local-quic-cert");
        let key_path = unique_temp_path("local-quic-key");
        fs::write(&cert_path, b"not a certificate").expect("should write temp cert file");
        fs::write(&key_path, b"not a key").expect("should write temp key file");

        let settings = Settings::default();
        let server_settings = server::Settings {
            gameserver_protocols: vec![server::settings::Protocol::Quic {
                address: "127.0.0.1:14004".parse().unwrap(),
                cert_file_path: cert_path.clone(),
                key_file_path: key_path.clone(),
            }],
            ..server::Settings::default()
        };

        let inventory = settings.transport_security_inventory(&server_settings);

        let _ = fs::remove_file(cert_path);
        let _ = fs::remove_file(key_path);

        assert_eq!(inventory.len(), 1);
        assert_eq!(inventory[0].surface, "game-quic");
        assert_eq!(
            inventory[0].rollout_policy,
            TransportSecurityRolloutPolicy::DisabledUntilExplicitOptIn
        );
        assert_eq!(
            inventory[0].validation_policy,
            TransportSecurityValidationPolicy::AdvisoryAtStartup
        );
        assert_eq!(
            inventory[0].material_state,
            TransportSecurityMaterialState::Invalid
        );
        assert!(!inventory[0].detail.is_empty());
    }

    #[test]
    fn transport_security_inventory_marks_non_local_quic_as_fail_fast_after_opt_in() {
        let cert_path = unique_temp_path("production-quic-cert");
        let key_path = unique_temp_path("production-quic-key");
        fs::write(&cert_path, b"not a certificate").expect("should write temp cert file");
        fs::write(&key_path, b"not a key").expect("should write temp key file");

        let settings = Settings {
            environment: Environment::Production,
            allow_experimental_quic: true,
            ..Settings::default()
        };
        let server_settings = server::Settings {
            gameserver_protocols: vec![server::settings::Protocol::Quic {
                address: "0.0.0.0:14004".parse().unwrap(),
                cert_file_path: cert_path.clone(),
                key_file_path: key_path.clone(),
            }],
            ..server::Settings::default()
        };

        let inventory = settings.transport_security_inventory(&server_settings);

        let _ = fs::remove_file(cert_path);
        let _ = fs::remove_file(key_path);

        assert_eq!(inventory.len(), 1);
        assert_eq!(
            inventory[0].rollout_policy,
            TransportSecurityRolloutPolicy::ExperimentalOptInActive
        );
        assert_eq!(
            inventory[0].validation_policy,
            TransportSecurityValidationPolicy::FailFastAtStartup
        );
        assert_eq!(
            inventory[0].material_state,
            TransportSecurityMaterialState::Invalid
        );
        assert!(inventory[0].detail.contains("valid TLS"));
    }

    #[test]
    fn sensitive_config_inventory_marks_operator_provided_control_plane_secrets() {
        let settings = Settings {
            web_address: "127.0.0.1:14005".parse().unwrap(),
            ui_api_secret: Some("ui-secret".into()),
            web_chat_secret: Some("chat-secret".into()),
            ..Settings::default()
        };

        let inventory = settings.sensitive_config_inventory(&server::Settings::default());

        let ui_api_secret = inventory
            .iter()
            .find(|entry| entry.id == "ui-api-secret")
            .expect("ui-api secret entry should exist");
        assert!(ui_api_secret.configured);
        assert_eq!(
            ui_api_secret.source,
            SensitiveConfigSource::InlineOperatorProvided
        );
        assert_eq!(
            ui_api_secret.value_state,
            SensitiveConfigValueState::NonEmptyInline
        );

        let chat_api_secret = inventory
            .iter()
            .find(|entry| entry.id == "chat-api-secret")
            .expect("chat-api secret entry should exist");
        assert!(chat_api_secret.configured);
        assert_eq!(
            chat_api_secret.source,
            SensitiveConfigSource::InlineOperatorProvided
        );
        assert_eq!(
            chat_api_secret.value_state,
            SensitiveConfigValueState::NonEmptyInline
        );
    }

    #[test]
    fn sensitive_config_inventory_marks_blank_inline_secret_values() {
        let settings = Settings {
            ui_api_secret: Some("   ".into()),
            web_chat_secret: Some("   ".into()),
            ..Settings::default()
        };

        let inventory = settings.sensitive_config_inventory(&server::Settings::default());

        assert_eq!(
            inventory
                .iter()
                .find(|entry| entry.id == "ui-api-secret")
                .expect("ui-api secret entry should exist")
                .value_state,
            SensitiveConfigValueState::Blank
        );
        assert_eq!(
            inventory
                .iter()
                .find(|entry| entry.id == "chat-api-secret")
                .expect("chat-api secret entry should exist")
                .value_state,
            SensitiveConfigValueState::Blank
        );
    }
}
