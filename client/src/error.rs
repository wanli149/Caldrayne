use authc::AuthClientError;
use common_net::msg::server::{BanInfo, ServerCompatibility};
pub use network::{InitProtocolError, NetworkConnectError, NetworkError};
use network::{ParticipantError, StreamError};
use rustls::Error as RustlsError;
use specs::error::Error as SpecsError;

pub const OTHER_NO_IP_ADDR: &str = "client.no_ip_addr";
pub const OTHER_BAD_WORLD_MAP_DIMENSIONS: &str = "client.bad_world_map_dimensions";
pub const OTHER_BAD_WORLD_MAP_IMAGE: &str = "client.bad_world_map_image";
pub const OTHER_BAD_ALTITUDE_MAP: &str = "client.bad_altitude_map";
pub const OTHER_ENTITY_FROM_UID_NOT_FOUND: &str = "client.entity_from_uid_not_found";

#[derive(Debug)]
pub enum Error {
    Kicked(String),
    NetworkErr(NetworkError),
    ParticipantErr(ParticipantError),
    StreamErr(StreamError),
    ServerTimeout,
    ServerShutdown,
    TooManyPlayers,
    NotOnWhitelist,
    AuthErr(String),
    AuthClientError(AuthClientError),
    AuthServerUrlInvalid(String),
    AuthServerNotTrusted,
    IncompatibleServerGeneration {
        client: ServerCompatibility,
        server: ServerCompatibility,
    },
    HostnameLookupFailed(std::io::Error),
    Banned(BanInfo),
    /// Persisted character data is invalid or missing
    InvalidCharacter,
    //TODO: InvalidAlias,
    Other(String),
    SpecsErr(SpecsError),
    RustlsErr(RustlsError),
}

impl From<SpecsError> for Error {
    fn from(err: SpecsError) -> Self { Self::SpecsErr(err) }
}

impl From<RustlsError> for Error {
    fn from(err: RustlsError) -> Self { Self::RustlsErr(err) }
}

impl From<NetworkError> for Error {
    fn from(err: NetworkError) -> Self { Self::NetworkErr(err) }
}

impl From<ParticipantError> for Error {
    fn from(err: ParticipantError) -> Self { Self::ParticipantErr(err) }
}

impl From<StreamError> for Error {
    fn from(err: StreamError) -> Self { Self::StreamErr(err) }
}

impl From<AuthClientError> for Error {
    fn from(err: AuthClientError) -> Self { Self::AuthClientError(err) }
}
