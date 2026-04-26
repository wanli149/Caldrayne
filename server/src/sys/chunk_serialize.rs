use crate::{
    Tick,
    chunk_lifecycle::{ChunkLifecycleHandle, ChunkLifecycleTerminal},
    chunk_serialize::{ChunkSendEntry, SerializedChunk},
    client::Client,
    metrics::NetworkRequestMetrics,
};
use common::{comp::Presence, event::EventBus, slowjob::SlowJobPool, terrain::TerrainGrid};
use common_ecs::{Job, Origin, Phase, System};
use common_net::msg::{SerializedTerrainChunk, ServerGeneral};
use hashbrown::{HashMap, HashSet, hash_map::Entry};
use network::StreamParams;
use specs::{Entity, Read, ReadExpect, ReadStorage};
use std::sync::Arc;

/// This system will handle sending terrain to clients by
/// collecting chunks that need to be send for a single generation run and then
/// trigger a SlowJob for serialisation.
#[derive(Default)]
pub struct Sys;
impl<'a> System<'a> for Sys {
    type SystemData = (
        Read<'a, Tick>,
        ReadStorage<'a, Client>,
        ReadStorage<'a, Presence>,
        ReadExpect<'a, EventBus<ChunkSendEntry>>,
        ReadExpect<'a, NetworkRequestMetrics>,
        ReadExpect<'a, SlowJobPool>,
        ReadExpect<'a, TerrainGrid>,
        ReadExpect<'a, crossbeam_channel::Sender<SerializedChunk>>,
        ReadExpect<'a, ChunkLifecycleHandle>,
    );

    const NAME: &'static str = "chunk_serialize";
    const ORIGIN: Origin = Origin::Server;
    const PHASE: Phase = Phase::Create;

    fn run(
        _job: &mut Job<Self>,
        (
            tick,
            clients,
            presences,
            chunk_send_queues_bus,
            network_metrics,
            slow_jobs,
            terrain,
            chunk_sender,
            chunk_lifecycle,
        ): Self::SystemData,
    ) {
        // Only operate twice per second
        //TODO: move out of this system and now even spawn this.
        if tick.0.rem_euclid(15) != 0 {
            return;
        }

        struct Metadata {
            recipients: Vec<Entity>,
            lossy_compression: bool,
            params: StreamParams,
        }

        // collect all deduped entities that request a chunk
        let mut chunks = HashMap::<_, Metadata>::new();
        let mut dropped_without_live_recipients = HashSet::new();
        let mut requests = 0u64;
        let mut distinct_requests = 0u64;

        for queue_entry in chunk_send_queues_bus.recv_all() {
            let entry = chunks.entry(queue_entry.chunk_key);
            let meta = match entry {
                Entry::Vacant(ve) => {
                    match clients.get(queue_entry.entity).map(|c| c.terrain_params()) {
                        Some(params) => {
                            dropped_without_live_recipients.remove(&queue_entry.chunk_key);
                            distinct_requests += 1;
                            ve.insert(Metadata {
                                recipients: Vec::new(),
                                lossy_compression: true,
                                params,
                            })
                        },
                        None => {
                            dropped_without_live_recipients.insert(queue_entry.chunk_key);
                            continue;
                        },
                    }
                },
                Entry::Occupied(oe) => oe.into_mut(),
            };

            // We decide here, to ONLY send lossy compressed data If all clients want those.
            // If at least 1 client here does not want lossy we don't compress it twice.
            // It would just be too expensive for the server
            meta.lossy_compression = meta.lossy_compression
                && presences
                    .get(queue_entry.entity)
                    .map(|p| p.lossy_terrain_compression)
                    .unwrap_or(true);
            meta.recipients.push(queue_entry.entity);
            requests += 1;
        }

        network_metrics
            .chunks_serialisation_requests
            .inc_by(requests);
        network_metrics
            .chunks_distinct_serialisation_requests
            .inc_by(distinct_requests);

        if !dropped_without_live_recipients.is_empty() {
            let mut lifecycle = chunk_lifecycle.lock().expect("Poisoned");
            for chunk_key in dropped_without_live_recipients {
                let _ = lifecycle.complete(
                    chunk_key,
                    Some(tick.0),
                    ChunkLifecycleTerminal::Dropped,
                    Some(0),
                );
            }
        }

        // Trigger serialization in a SlowJob
        const CHUNK_SIZE: usize = 10; // trigger one job per 10 chunks to reduce SlowJob overhead. as we use a channel, there is no disadvantage to this
        let mut serializable_chunks = Vec::with_capacity(chunks.len());
        let mut terrain_missing_chunks = Vec::new();
        for (chunk_key, meta) in chunks {
            if let Some(chunk) = terrain.get_key_arc_real(chunk_key) {
                serializable_chunks.push((Arc::clone(chunk), chunk_key, meta));
            } else {
                terrain_missing_chunks.push((chunk_key, meta.recipients.len()));
            }
        }
        if !terrain_missing_chunks.is_empty() {
            let mut lifecycle = chunk_lifecycle.lock().expect("Poisoned");
            for (chunk_key, recipient_count) in terrain_missing_chunks {
                let _ = lifecycle.complete(
                    chunk_key,
                    Some(tick.0),
                    ChunkLifecycleTerminal::Dropped,
                    Some(recipient_count),
                );
            }
        }
        let mut chunks_iter = serializable_chunks.into_iter().peekable();

        while chunks_iter.peek().is_some() {
            let mut chunks: Vec<_> = chunks_iter.by_ref().take(CHUNK_SIZE).collect();
            for (_, _, meta) in &mut chunks {
                meta.recipients.sort_unstable();
                meta.recipients.dedup();
            }
            {
                let mut lifecycle = chunk_lifecycle.lock().expect("Poisoned");
                for (_, chunk_key, meta) in &chunks {
                    lifecycle.record_serialize_queued(*chunk_key, tick.0, meta.recipients.len());
                }
            }
            let chunk_sender = chunk_sender.clone();
            let chunk_lifecycle = chunk_lifecycle.clone();
            let handoff_dropped_counter = network_metrics.chunks_serialize_handoff_dropped.clone();
            let serialize_done_tick = tick.0;
            slow_jobs.spawn("CHUNK_SERIALIZER", move || {
                let mut chunks = chunks.into_iter();
                while let Some((chunk, chunk_key, meta)) = chunks.next() {
                    let msg = Client::prepare_chunk_update_msg(
                        ServerGeneral::TerrainChunkUpdate {
                            key: chunk_key,
                            chunk: Ok(SerializedTerrainChunk::via_heuristic(
                                &chunk,
                                meta.lossy_compression,
                            )),
                        },
                        &meta.params,
                    );
                    chunk_lifecycle
                        .lock()
                        .expect("Poisoned")
                        .record_serialize_done(chunk_key, serialize_done_tick);
                    if let Err(e) = chunk_sender.send(SerializedChunk {
                        chunk_key,
                        lossy_compression: meta.lossy_compression,
                        msg,
                        recipients: meta.recipients,
                    }) {
                        handoff_dropped_counter.inc_by(1 + chunks.len() as u64);
                        let _ = chunk_lifecycle.lock().expect("Poisoned").complete(
                            chunk_key,
                            None,
                            ChunkLifecycleTerminal::Dropped,
                            None,
                        );
                        tracing::warn!(?e, "cannot send serialized chunk to sender");
                        let mut lifecycle = chunk_lifecycle.lock().expect("Poisoned");
                        for (_, remaining_key, _) in chunks {
                            let _ = lifecycle.complete(
                                remaining_key,
                                None,
                                ChunkLifecycleTerminal::Dropped,
                                None,
                            );
                        }
                        break;
                    };
                }
            });
        }
    }
}
