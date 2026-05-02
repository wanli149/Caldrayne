#![expect(non_local_definitions)] // necessary because of the Protocol derive macro
use protocol::Protocol;

pub const CURRENT_PROTOCOL_VERSION: u16 = 2;
pub const VERSION_SELECTION_POLICY: &str = "exact-match";
pub const SUPPORTS_MULTI_VERSION_NEGOTIATION: bool = false;
pub const PUBLISHED_SERVER_INFO_FIELDS: &[&str] = &[
    "realm_id",
    "environment",
    "compatibility",
    "auth_required",
    "git_hash",
    "git_timestamp",
    "players_count",
    "player_cap",
    "battlemode",
];
pub(crate) const CALDRAYNE_HEADER: [u8; 5] = [b'c', b'a', b'l', b'd', b'r'];
pub(crate) const MAX_REQUEST_CONTENT_SIZE: usize = 300;
// NOTE: The actual maximum size must never exceed 1200 or we risk getting near
// MTU limits for some networks.
pub(crate) const MAX_REQUEST_SIZE: usize = MAX_REQUEST_CONTENT_SIZE + CALDRAYNE_HEADER.len() + 2;
pub(crate) const MAX_RESPONSE_SIZE: usize = 256;

#[derive(Protocol, Debug, Clone, Copy)]
pub(crate) struct RawQueryServerRequest {
    /// See comment on [`Init::p`]
    pub p: u64,
    pub request: QueryServerRequest,
}

#[derive(Protocol, Debug, Clone, Copy)]
#[protocol(discriminant = "integer")]
#[protocol(discriminator(u8))]
pub enum QueryServerRequest {
    /// This requests exists mostly for backwards-compatibilty reasons. As the
    /// first message sent to the server should always be in the currently
    /// supported version of the protocol, if future versions of the protocol
    /// have more requests than server info it may be confusing to request
    /// `P` and the max version with a `QueryServerRequest::ServerInfo`
    /// request (the request will still be dropped as the supplied `P` value
    /// is invalid).
    Init,
    ServerInfo,
    // New requests should be added at the end to prevent breakage.
    // NOTE: Any new (sub-)variants must be added to the `check_request_sizes` test at the end of
    // this file
}

#[derive(Protocol, Debug, Clone, Copy)]
pub(crate) struct Init {
    /// This is used as a challenge to prevent IP address spoofing by verifying
    /// that the client can receive from the source address.
    ///
    /// Any request to the server must include this value to be processed,
    /// otherwise this response will be returned (giving clients a value to pass
    /// for later requests).
    pub p: u64,
    /// The maximum supported protocol version by the server. The first request
    /// to a server must always be done in the current protocol to query this
    /// value. Following requests (when the version is known), can be done
    /// in the maximum version or below, responses will be sent in the same
    /// version as the requests.
    pub max_supported_version: u16,
}

#[derive(Protocol, Debug, Clone, Copy)]
#[protocol(discriminant = "integer")]
#[protocol(discriminator(u8))]
pub(crate) enum RawQueryServerResponse {
    Response(QueryServerResponse),
    Init(Init),
}

#[derive(Protocol, Debug, Clone, Copy)]
#[protocol(discriminant = "integer")]
#[protocol(discriminator(u8))]
pub enum QueryServerResponse {
    ServerInfo(ServerInfo),
    // New responses should be added at the end to prevent breakage
}

#[derive(Protocol, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerInfo {
    pub realm_id: ServerRealmId,
    pub environment: ServerEnvironment,
    pub compatibility: ServerCompatibility,
    pub auth_required: bool,
    pub git_hash: u32,
    pub git_timestamp: i64,
    pub players_count: u16,
    pub player_cap: u16,
    pub battlemode: ServerBattleMode,
}

#[derive(Protocol, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerRealmId {
    pub msb: u64,
    pub lsb: u64,
}

impl ServerRealmId {
    pub fn from_u128(value: u128) -> Self {
        Self {
            msb: (value >> 64) as u64,
            lsb: value as u64,
        }
    }
}

#[derive(Protocol, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerCompatibility {
    pub generation: u16,
    pub minimum_supported_generation: u16,
}

#[derive(Protocol, Debug, Clone, Copy, PartialEq, Eq)]
#[protocol(discriminant = "integer")]
#[protocol(discriminator(u8))]
#[repr(u8)]
pub enum ServerEnvironment {
    Local,
    Test,
    Production,
}

#[derive(Protocol, Debug, Clone, Copy, PartialEq, Eq)]
#[protocol(discriminant = "integer")]
#[protocol(discriminator(u8))]
#[repr(u8)]
pub enum ServerBattleMode {
    GlobalPvP,
    GlobalPvE,
    PerPlayer,
}

impl RawQueryServerRequest {
    #[cfg(any(feature = "client", test))]
    pub fn serialize(&self) -> Result<Vec<u8>, protocol::Error> {
        use protocol::Parcel;

        let mut buf = Vec::with_capacity(MAX_REQUEST_SIZE);

        // 2 extra bytes for protocol version information. This is currently
        // only used for exact-version gating, not multi-version negotiation.
        buf.extend(CURRENT_PROTOCOL_VERSION.to_le_bytes());
        buf.extend({
            let request_data =
                <RawQueryServerRequest as Parcel>::raw_bytes(self, &Default::default())?;
            if request_data.len() > MAX_REQUEST_CONTENT_SIZE {
                panic!(
                    "Attempted to send request larger than the max size (size: {}, max size: \
                     {MAX_REQUEST_CONTENT_SIZE}, request: {self:?})",
                    request_data.len()
                );
            }
            request_data
        });
        const _: () = assert!(MAX_RESPONSE_SIZE + CALDRAYNE_HEADER.len() <= MAX_REQUEST_SIZE);
        buf.resize(MAX_RESPONSE_SIZE.max(buf.len()), 0);
        buf.extend(CALDRAYNE_HEADER);
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        QueryServerRequest, QueryServerResponse, RawQueryServerRequest, RawQueryServerResponse,
        ServerBattleMode, ServerCompatibility, ServerEnvironment, ServerInfo, ServerRealmId,
    };
    use protocol::Parcel;

    #[test]
    fn check_request_sizes() {
        const ALL_REQUESTS: &[QueryServerRequest] =
            &[QueryServerRequest::ServerInfo, QueryServerRequest::Init];
        for request in ALL_REQUESTS {
            let request = RawQueryServerRequest {
                p: 0,
                request: *request,
            };
            request.serialize().unwrap(); // This will panic if the size is above MAX_REQUEST_SIZE
        }
    }

    #[test]
    fn check_response_sizes() {
        let response =
            RawQueryServerResponse::Response(QueryServerResponse::ServerInfo(ServerInfo {
                realm_id: ServerRealmId {
                    msb: u64::MAX,
                    lsb: u64::MAX,
                },
                environment: ServerEnvironment::Production,
                compatibility: ServerCompatibility {
                    generation: u16::MAX,
                    minimum_supported_generation: u16::MAX,
                },
                auth_required: true,
                git_hash: u32::MAX,
                git_timestamp: i64::MAX,
                players_count: u16::MAX,
                player_cap: u16::MAX,
                battlemode: ServerBattleMode::PerPlayer,
            }));

        let bytes =
            <RawQueryServerResponse as Parcel>::raw_bytes(&response, &Default::default()).unwrap();
        assert!(
            bytes.len() <= super::MAX_RESPONSE_SIZE,
            "response size {} exceeds {}",
            bytes.len(),
            super::MAX_RESPONSE_SIZE
        );
    }
}
