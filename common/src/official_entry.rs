use crate::assets::{AssetExt, Error as AssetError, Ron};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::net::{IpAddr, Ipv6Addr, SocketAddr};

pub const OFFICIAL_ENTRY_ARTIFACT_IDENTITY_SCHEME: &str = "official-entry-content-sha256-v1";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum OfficialEntrySourceKind {
    #[default]
    Bundled,
    LauncherManaged,
    BootstrapManaged,
}

impl OfficialEntrySourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bundled => "bundled",
            Self::LauncherManaged => "launcher_managed",
            Self::BootstrapManaged => "bootstrap_managed",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BundledOfficialTargetKind {
    Missing,
    LocalhostOrLoopback,
    PrivateOrUniqueLocalIp,
    ReservedNonPublicIp,
    NamedHostCandidate,
    PublicIpCandidate,
}

impl BundledOfficialTargetKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::LocalhostOrLoopback => "localhost-or-loopback",
            Self::PrivateOrUniqueLocalIp => "private-or-unique-local-ip",
            Self::ReservedNonPublicIp => "reserved-non-public-ip",
            Self::NamedHostCandidate => "named-host-candidate",
            Self::PublicIpCandidate => "public-ip-candidate",
        }
    }

    pub const fn is_non_local_candidate(self) -> bool {
        matches!(self, Self::NamedHostCandidate | Self::PublicIpCandidate)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BundledOfficialTransportKind {
    DirectTcp,
    DirectQuic,
    SrvLookup,
}

impl BundledOfficialTransportKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DirectTcp => "direct-tcp",
            Self::DirectQuic => "direct-quic",
            Self::SrvLookup => "srv-lookup",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BundledOfficialAuthMode {
    NoExternalAuth,
    ExternalProvider,
}

impl BundledOfficialAuthMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoExternalAuth => "no-external-auth",
            Self::ExternalProvider => "external-provider",
        }
    }

    pub const fn requires_external_auth(self) -> bool { matches!(self, Self::ExternalProvider) }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct OfficialEntry {
    pub display_name: String,
    pub server_address: String,
    pub auth_server: Option<String>,
    pub use_srv: bool,
    pub use_quic: bool,
    pub validate_tls: bool,
    #[serde(default)]
    pub source_kind: OfficialEntrySourceKind,
}

impl OfficialEntry {
    pub fn load() -> Self { Ron::<Self>::load_expect_cloned("voxygen.official_entry").into_inner() }

    pub fn try_load() -> Result<Self, AssetError> {
        Ron::<Self>::load_cloned("voxygen.official_entry").map(|entry| entry.into_inner())
    }

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

    pub fn artifact_identity(&self) -> String {
        fn update_field(hasher: &mut Sha256, key: &str, value: &str) {
            hasher.update(key.as_bytes());
            hasher.update([0]);
            hasher.update(value.as_bytes());
            hasher.update([0]);
        }

        let mut hasher = Sha256::new();
        update_field(
            &mut hasher,
            "scheme",
            OFFICIAL_ENTRY_ARTIFACT_IDENTITY_SCHEME,
        );
        update_field(&mut hasher, "display_name", &self.display_name);
        update_field(&mut hasher, "server_address", &self.server_address);
        update_field(
            &mut hasher,
            "auth_server_present",
            if self.auth_server.is_some() {
                "true"
            } else {
                "false"
            },
        );
        update_field(
            &mut hasher,
            "auth_server",
            self.auth_server.as_deref().unwrap_or(""),
        );
        update_field(
            &mut hasher,
            "use_srv",
            if self.use_srv { "true" } else { "false" },
        );
        update_field(
            &mut hasher,
            "use_quic",
            if self.use_quic { "true" } else { "false" },
        );
        update_field(
            &mut hasher,
            "validate_tls",
            if self.validate_tls { "true" } else { "false" },
        );
        update_field(&mut hasher, "source_kind", self.source_kind.as_str());

        format!(
            "{}:{:x}",
            OFFICIAL_ENTRY_ARTIFACT_IDENTITY_SCHEME,
            hasher.finalize()
        )
    }

    pub fn target_kind(&self) -> BundledOfficialTargetKind {
        let server_address = self.server_address.trim();
        if server_address.is_empty() {
            return BundledOfficialTargetKind::Missing;
        }

        if let Some(ip) = parse_server_address_ip_literal(server_address) {
            return bundled_target_kind_for_ip(ip);
        }

        let host = extract_server_host(server_address);
        if host.eq_ignore_ascii_case("localhost") {
            BundledOfficialTargetKind::LocalhostOrLoopback
        } else {
            BundledOfficialTargetKind::NamedHostCandidate
        }
    }

    pub fn transport_kind(&self) -> BundledOfficialTransportKind {
        if self.use_srv {
            BundledOfficialTransportKind::SrvLookup
        } else if self.use_quic {
            BundledOfficialTransportKind::DirectQuic
        } else {
            BundledOfficialTransportKind::DirectTcp
        }
    }

    pub fn auth_mode(&self) -> BundledOfficialAuthMode {
        if self.auth_server.is_some() {
            BundledOfficialAuthMode::ExternalProvider
        } else {
            BundledOfficialAuthMode::NoExternalAuth
        }
    }

    pub fn non_local_cutover_gap_reasons(&self) -> Vec<&'static str> {
        let mut reasons = Vec::new();

        match self.target_kind() {
            BundledOfficialTargetKind::Missing => {
                reasons.push("bundled_public_target_missing");
            },
            BundledOfficialTargetKind::LocalhostOrLoopback => {
                reasons.push("bundled_public_target_is_localhost_or_loopback");
            },
            BundledOfficialTargetKind::PrivateOrUniqueLocalIp => {
                reasons.push("bundled_public_target_is_private_or_unique_local_ip");
            },
            BundledOfficialTargetKind::ReservedNonPublicIp => {
                reasons.push("bundled_public_target_is_reserved_non_public_ip");
            },
            BundledOfficialTargetKind::NamedHostCandidate
            | BundledOfficialTargetKind::PublicIpCandidate => {},
        }

        if self.auth_server.is_none() {
            reasons.push("bundled_public_auth_pin_missing");
        }

        reasons
    }

    pub fn posture(&self) -> BundledOfficialEntryPosture {
        let target_kind = self.target_kind();
        let auth_mode = self.auth_mode();
        let non_local_cutover_gap_reasons = self.non_local_cutover_gap_reasons();

        BundledOfficialEntryPosture {
            source_kind: self.source_kind,
            display_name: self.display_name.clone(),
            server_address_configured: self.is_configured(),
            server_address: self.server_address.clone(),
            artifact_identity: self.artifact_identity(),
            target_kind,
            target_is_non_local_candidate: target_kind.is_non_local_candidate(),
            transport_kind: self.transport_kind(),
            use_srv: self.use_srv,
            use_quic: self.use_quic,
            validate_tls: self.validate_tls,
            auth_mode,
            auth_server: self.auth_server.clone(),
            non_local_cutover_ready: non_local_cutover_gap_reasons.is_empty(),
            non_local_cutover_gap_reasons,
            rollout_readiness_scope: "non-local Public cutover still requires both a non-local \
                                      target candidate in official_entry.server_address and an \
                                      exact external auth pin in official_entry.auth_server; \
                                      local/private literals or auth_server = None keep the \
                                      bundled Public entry in transitional mode",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct BundledOfficialEntryPosture {
    pub source_kind: OfficialEntrySourceKind,
    pub display_name: String,
    pub server_address_configured: bool,
    pub server_address: String,
    pub artifact_identity: String,
    pub target_kind: BundledOfficialTargetKind,
    pub target_is_non_local_candidate: bool,
    pub transport_kind: BundledOfficialTransportKind,
    pub use_srv: bool,
    pub use_quic: bool,
    pub validate_tls: bool,
    pub auth_mode: BundledOfficialAuthMode,
    pub auth_server: Option<String>,
    pub non_local_cutover_ready: bool,
    pub non_local_cutover_gap_reasons: Vec<&'static str>,
    pub rollout_readiness_scope: &'static str,
}

fn parse_server_address_ip_literal(server_address: &str) -> Option<IpAddr> {
    let server_address = server_address.trim();
    if server_address.is_empty() {
        return None;
    }

    if let Ok(socket_addr) = server_address.parse::<SocketAddr>() {
        return Some(socket_addr.ip());
    }

    if let Ok(ip) = server_address.parse::<IpAddr>() {
        return Some(ip);
    }

    if let Some(rest) = server_address.strip_prefix('[') {
        if let Some((host, _)) = rest.split_once(']') {
            if let Ok(ip) = host.parse::<IpAddr>() {
                return Some(ip);
            }
        }
    }

    if let Some((host, port)) = server_address.rsplit_once(':') {
        if port.chars().all(|ch| ch.is_ascii_digit()) {
            if let Ok(ip) = host.parse::<IpAddr>() {
                return Some(ip);
            }
        }
    }

    None
}

fn extract_server_host(server_address: &str) -> &str {
    let server_address = server_address.trim();
    if let Some(rest) = server_address.strip_prefix('[') {
        if let Some((host, _)) = rest.split_once(']') {
            return host;
        }
    }

    if let Some((host, port)) = server_address.rsplit_once(':') {
        if port.chars().all(|ch| ch.is_ascii_digit()) && !host.contains(':') {
            return host;
        }
    }

    server_address
}

fn bundled_target_kind_for_ip(ip: IpAddr) -> BundledOfficialTargetKind {
    match ip {
        IpAddr::V4(ip) if ip.is_loopback() => BundledOfficialTargetKind::LocalhostOrLoopback,
        IpAddr::V4(ip) if ip.is_private() || ip.is_link_local() => {
            BundledOfficialTargetKind::PrivateOrUniqueLocalIp
        },
        IpAddr::V4(ip)
            if ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_broadcast()
                || ip.is_documentation() =>
        {
            BundledOfficialTargetKind::ReservedNonPublicIp
        },
        IpAddr::V6(ip) if ip.is_loopback() => BundledOfficialTargetKind::LocalhostOrLoopback,
        IpAddr::V6(ip) if ip.is_unique_local() || ip.is_unicast_link_local() => {
            BundledOfficialTargetKind::PrivateOrUniqueLocalIp
        },
        IpAddr::V6(ip) if ip.is_unspecified() || ip.is_multicast() || is_ipv6_documentation(ip) => {
            BundledOfficialTargetKind::ReservedNonPublicIp
        },
        IpAddr::V4(_) | IpAddr::V6(_) => BundledOfficialTargetKind::PublicIpCandidate,
    }
}

fn is_ipv6_documentation(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    segments[0] == 0x2001 && segments[1] == 0x0db8
}

#[cfg(test)]
mod tests {
    use super::{
        BundledOfficialAuthMode, BundledOfficialTargetKind, BundledOfficialTransportKind,
        OfficialEntry, OfficialEntrySourceKind,
    };

    fn official_entry(server_address: &str, auth_server: Option<&str>) -> OfficialEntry {
        official_entry_with_transport(server_address, auth_server, false, false, true)
    }

    fn official_entry_with_transport(
        server_address: &str,
        auth_server: Option<&str>,
        use_srv: bool,
        use_quic: bool,
        validate_tls: bool,
    ) -> OfficialEntry {
        OfficialEntry {
            display_name: "Official Realm".to_owned(),
            server_address: server_address.to_owned(),
            auth_server: auth_server.map(str::to_owned),
            use_srv,
            use_quic,
            validate_tls,
            source_kind: OfficialEntrySourceKind::Bundled,
        }
    }

    #[test]
    fn official_entry_source_kind_defaults_to_bundled_when_omitted() {
        let entry: OfficialEntry = ron::from_str(
            r#"
                (
                    display_name: "Official Realm",
                    server_address: "example.test:14004",
                    auth_server: Some("https://auth.example.test"),
                    use_srv: false,
                    use_quic: false,
                    validate_tls: true,
                )
            "#,
        )
        .expect("official entry should deserialize");

        assert_eq!(entry.source_kind, OfficialEntrySourceKind::Bundled);
    }

    #[test]
    fn posture_flags_private_ip_without_auth_as_transitional() {
        let posture = official_entry("192.168.1.8:14004", None).posture();

        assert!(posture.server_address_configured);
        assert_eq!(
            posture.target_kind,
            BundledOfficialTargetKind::PrivateOrUniqueLocalIp
        );
        assert_eq!(
            posture.transport_kind,
            BundledOfficialTransportKind::DirectTcp
        );
        assert_eq!(posture.auth_mode, BundledOfficialAuthMode::NoExternalAuth);
        assert!(!posture.non_local_cutover_ready);
        assert!(
            posture
                .non_local_cutover_gap_reasons
                .contains(&"bundled_public_target_is_private_or_unique_local_ip")
        );
        assert!(
            posture
                .non_local_cutover_gap_reasons
                .contains(&"bundled_public_auth_pin_missing")
        );
    }

    #[test]
    fn posture_identifies_named_host_with_external_auth_as_non_local_candidate() {
        let posture = official_entry(
            "prod.realm.example:14004",
            Some("https://auth.realm.example"),
        )
        .posture();

        assert!(posture.server_address_configured);
        assert_eq!(
            posture.target_kind,
            BundledOfficialTargetKind::NamedHostCandidate
        );
        assert!(posture.target_is_non_local_candidate);
        assert_eq!(posture.auth_mode, BundledOfficialAuthMode::ExternalProvider);
        assert!(posture.non_local_cutover_ready);
        assert!(posture.non_local_cutover_gap_reasons.is_empty());
    }

    #[test]
    fn posture_tracks_transport_flags() {
        let posture = official_entry_with_transport(
            "prod.realm.example",
            Some("https://auth.realm.example"),
            true,
            true,
            true,
        )
        .posture();

        assert_eq!(
            posture.transport_kind,
            BundledOfficialTransportKind::SrvLookup
        );
        assert!(posture.use_srv);
        assert!(posture.use_quic);
        assert!(posture.validate_tls);
    }

    #[test]
    fn artifact_identity_changes_when_auth_pin_changes() {
        let baseline = official_entry("prod.realm.example:14004", Some("https://auth-a.example"))
            .artifact_identity();
        let changed = official_entry("prod.realm.example:14004", Some("https://auth-b.example"))
            .artifact_identity();

        assert_ne!(baseline, changed);
    }
}
