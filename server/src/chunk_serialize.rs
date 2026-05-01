use crate::client::PreparedMsg;
use common::terrain::TerrainChunk;
use network::StreamParams;
use specs::Entity;
use std::sync::Arc;
use vek::Vec2;

/// Sending a chunk to the user works the following way:
/// A system like `msg::terrain` `terrain` or `terrain_sync` either decide to
/// trigger chunk generation, or if the chunk already exists
/// push a `ChunkSendQueue` to the eventbus.
/// The `chunk_serialize` system will coordinate serializing via a SlowJob
/// outside of the tick. On the next tick, the `chunk_send` system will pick up
/// finished chunks.
///
/// Deferring allows us to remove code duplication and maybe serialize ONCE,
/// send to MULTIPLE clients
/// TODO: store a urgent flag and seperate even more, 5 ticks vs 5 seconds
#[derive(Debug, PartialEq, Eq)]
pub struct ChunkSendEntry {
    pub(crate) entity: Entity,
    pub(crate) chunk_key: Vec2<i32>,
}

#[derive(Debug, Clone)]
pub struct AdmittedSerializableChunk {
    pub(crate) chunk: Arc<TerrainChunk>,
    pub(crate) chunk_key: Vec2<i32>,
    pub(crate) lossy_compression: bool,
    pub(crate) recipients: Vec<Entity>,
    pub(crate) params: StreamParams,
}

#[derive(Default)]
pub struct ChunkSerializeQueue {
    pub(crate) entries: Vec<ChunkSendEntry>,
    pub(crate) ready_chunks: Vec<AdmittedSerializableChunk>,
}

pub struct SerializedChunk {
    pub(crate) chunk_key: Vec2<i32>,
    pub(crate) lossy_compression: bool,
    pub(crate) msg: PreparedMsg,
    pub(crate) recipients: Vec<Entity>,
}
