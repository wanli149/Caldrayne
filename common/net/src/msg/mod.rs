pub mod client;
pub mod compression;
pub mod ecs_packet;
pub mod server;
pub mod world_msg;

// Reexports
pub use self::{
    client::{ClientGeneral, ClientMsg, ClientRegister, ClientType},
    compression::{
        CompressedData, GridLtrPacking, PackingFormula, QuadPngEncoding, TriPngEncoding,
        VoxelImageEncoding, WidePacking, WireChonk,
    },
    ecs_packet::EcsCompPacket,
    server::{
        CURRENT_COMPATIBILITY_GENERATION, CharacterInfo, ChatTypeContext, DisconnectReason,
        InviteAnswer, MINIMUM_SUPPORTED_COMPATIBILITY_GENERATION, Notification, PlayerInfo,
        PlayerListUpdate, RegisterError, SerializedTerrainChunk, ServerAuth, ServerAuthMode,
        ServerCompatibility, ServerEnvironment, ServerGeneral, ServerInfo, ServerInit, ServerMsg,
        ServerRegisterAnswer,
    },
    world_msg::WorldMapMsg,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PingMsg {
    Ping,
    Pong,
}
