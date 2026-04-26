use crate::persistence::error::PersistenceError;
use network::{NetworkError, ParticipantError, StreamError};
use std::fmt::{self, Display};

#[derive(Debug)]
pub enum Error {
    NetworkErr(NetworkError),
    ParticipantErr(ParticipantError),
    StreamErr(StreamError),
    DatabaseErr(rusqlite::Error),
    PersistenceErr(PersistenceError),
    RtsimError(ron::Error),
    WorldErr(world::Error),
    Other(String),
}

impl Error {
    pub const fn compat_audit(&self) -> Option<world::recipe::CompatAuditV1> {
        match self {
            Self::WorldErr(err) => err.compat_audit(),
            Self::NetworkErr(_)
            | Self::ParticipantErr(_)
            | Self::StreamErr(_)
            | Self::DatabaseErr(_)
            | Self::PersistenceErr(_)
            | Self::RtsimError(_)
            | Self::Other(_) => None,
        }
    }
}

impl From<NetworkError> for Error {
    fn from(err: NetworkError) -> Self { Error::NetworkErr(err) }
}

impl From<ParticipantError> for Error {
    fn from(err: ParticipantError) -> Self { Error::ParticipantErr(err) }
}

impl From<StreamError> for Error {
    fn from(err: StreamError) -> Self { Error::StreamErr(err) }
}

// TODO: Don't expose rusqlite::Error from persistence module
impl From<rusqlite::Error> for Error {
    fn from(err: rusqlite::Error) -> Self { Error::DatabaseErr(err) }
}

impl From<PersistenceError> for Error {
    fn from(err: PersistenceError) -> Self { Error::PersistenceErr(err) }
}

impl From<world::Error> for Error {
    fn from(err: world::Error) -> Self { Error::WorldErr(err) }
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::NetworkErr(err) => write!(f, "Network Error: {}", err),
            Self::ParticipantErr(err) => write!(f, "Participant Error: {}", err),
            Self::StreamErr(err) => write!(f, "Stream Error: {}", err),
            Self::DatabaseErr(err) => write!(f, "Database Error: {}", err),
            Self::PersistenceErr(err) => write!(f, "Persistence Error: {}", err),
            Self::RtsimError(err) => write!(f, "Rtsim Error: {}", err),
            Self::WorldErr(err) => write!(f, "World Error: {}", err),
            Self::Other(err) => write!(f, "Error: {}", err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Error;
    use world::recipe::{CompatAuditV1, CompatEntryKindV1, CompatFailureKindV1};

    #[test]
    fn compat_audit_forwards_world_compat_enforce_errors() {
        let audit = CompatAuditV1::fallback_generate(
            CompatEntryKindV1::Load,
            CompatFailureKindV1::ParseError,
        );
        let error = Error::WorldErr(world::Error::CompatEnforce { audit });

        assert_eq!(error.compat_audit(), Some(audit));
    }

    #[test]
    fn compat_audit_is_none_for_non_world_errors() {
        let error = Error::Other("opaque".to_owned());

        assert_eq!(error.compat_audit(), None);
    }
}
