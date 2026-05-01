use crate::{
    Tick,
    chunk_lifecycle::{ChunkLifecycleHandle, ChunkLifecycleTerminal},
    chunk_serialize::{
        AdmittedSerializableChunk, ChunkSendEntry, ChunkSerializeQueue, SerializedChunk,
    },
    client::Client,
    metrics::{ChunkLifecycleMetrics, NetworkRequestMetrics},
    settings::{
        DEFAULT_CHUNK_SERIALIZE_BATCH_SIZE_PER_JOB,
        DEFAULT_CHUNK_SERIALIZE_DISTINCT_BUDGET_PER_SERIALIZE_TICK,
        DEFAULT_CHUNK_SERIALIZE_INTERVAL_TICKS,
        DEFAULT_CHUNK_SERIALIZE_MAX_JOBS_PER_SERIALIZE_TICK, Settings,
    },
};
use common::{comp::Presence, event::EventBus, slowjob::SlowJobPool, terrain::TerrainGrid};
use common_ecs::{Job, Origin, Phase, System};
use common_net::msg::{SerializedTerrainChunk, ServerGeneral};
use hashbrown::{HashMap, HashSet, hash_map::Entry};
use network::StreamParams;
use specs::{Entity, Read, ReadExpect, ReadStorage, WriteExpect};
use std::sync::Arc;
use vek::*;

struct Metadata {
    recipients: Vec<Entity>,
    lossy_compression: bool,
    params: StreamParams,
}

enum QueuedChunkState {
    WaitingForFirstLiveRecipient,
    Live(Metadata),
}

#[derive(Default)]
struct ChunkSerializeAdmission {
    serializable_chunks: Vec<(Vec2<i32>, Metadata)>,
    dropped_without_live_recipients: Vec<Vec2<i32>>,
    terrain_missing_chunks: Vec<(Vec2<i32>, usize)>,
    deferred_distinct: usize,
}

#[derive(Default)]
struct ChunkSerializeSpawnPlan<T> {
    batches: Vec<Vec<T>>,
    deferred_chunks: usize,
}

fn chunk_serialize_distinct_budget(server_settings: &Settings) -> Option<usize> {
    match server_settings.chunk_serialize_distinct_budget_per_serialize_tick {
        Some(0) => DEFAULT_CHUNK_SERIALIZE_DISTINCT_BUDGET_PER_SERIALIZE_TICK,
        budget => budget,
    }
}

fn chunk_serialize_batch_size(server_settings: &Settings) -> usize {
    match server_settings.chunk_serialize_batch_size_per_job {
        0 => DEFAULT_CHUNK_SERIALIZE_BATCH_SIZE_PER_JOB,
        batch_size => batch_size,
    }
}

fn chunk_serialize_interval_ticks(server_settings: &Settings) -> u64 {
    match server_settings.chunk_serialize_interval_ticks {
        0 => DEFAULT_CHUNK_SERIALIZE_INTERVAL_TICKS,
        interval => interval,
    }
}

fn chunk_serialize_max_jobs(server_settings: &Settings) -> Option<usize> {
    match server_settings.chunk_serialize_max_jobs_per_serialize_tick {
        Some(0) => DEFAULT_CHUNK_SERIALIZE_MAX_JOBS_PER_SERIALIZE_TICK,
        budget => budget,
    }
}

fn pack_serializable_chunks_into_batches<T>(batch_size: usize, chunks: Vec<T>) -> Vec<Vec<T>> {
    let batch_size = batch_size.max(1);
    let mut chunks = chunks.into_iter().peekable();
    let mut batches = Vec::new();

    while chunks.peek().is_some() {
        batches.push(chunks.by_ref().take(batch_size).collect());
    }

    batches
}

fn take_serialize_ready_chunks_into_batches<T>(
    max_jobs: Option<usize>,
    batch_size: usize,
    ready_chunks: &mut Vec<T>,
) -> ChunkSerializeSpawnPlan<T> {
    let batch_size = batch_size.max(1);
    let chunks_to_spawn = match max_jobs {
        None => ready_chunks.len(),
        Some(max_jobs) => ready_chunks.len().min(max_jobs.saturating_mul(batch_size)),
    };
    let chunks = ready_chunks.drain(..chunks_to_spawn).collect();

    ChunkSerializeSpawnPlan {
        batches: pack_serializable_chunks_into_batches(batch_size, chunks),
        deferred_chunks: ready_chunks.len(),
    }
}

fn count_distinct_live_chunk_requests(
    entries: &[ChunkSendEntry],
    mut has_live_client: impl FnMut(Entity) -> bool,
) -> u64 {
    let mut first_live_seen = HashMap::<Vec2<i32>, bool>::new();
    let mut distinct = 0u64;

    for entry in entries {
        match first_live_seen.entry(entry.chunk_key) {
            Entry::Vacant(vacant) => {
                let has_live = has_live_client(entry.entity);
                if has_live {
                    distinct += 1;
                }
                vacant.insert(has_live);
            },
            Entry::Occupied(mut occupied) => {
                if !*occupied.get() && has_live_client(entry.entity) {
                    *occupied.get_mut() = true;
                    distinct += 1;
                }
            },
        }
    }

    distinct
}

fn admit_chunk_serialize_queue_up_to(
    budget: Option<usize>,
    queued_entries: &mut Vec<ChunkSendEntry>,
    mut client_params: impl FnMut(Entity) -> Option<StreamParams>,
    mut lossy_terrain_compression: impl FnMut(Entity) -> bool,
    mut terrain_has_chunk: impl FnMut(Vec2<i32>) -> bool,
) -> ChunkSerializeAdmission {
    let budget = budget.unwrap_or(usize::MAX);
    let mut order = Vec::new();
    let mut states = HashMap::<Vec2<i32>, QueuedChunkState>::new();

    for entry in queued_entries.iter() {
        match states.entry(entry.chunk_key) {
            Entry::Vacant(vacant) => {
                order.push(entry.chunk_key);
                if let Some(params) = client_params(entry.entity) {
                    vacant.insert(QueuedChunkState::Live(Metadata {
                        recipients: vec![entry.entity],
                        lossy_compression: lossy_terrain_compression(entry.entity),
                        params,
                    }));
                } else {
                    vacant.insert(QueuedChunkState::WaitingForFirstLiveRecipient);
                }
            },
            Entry::Occupied(mut occupied) => match occupied.get_mut() {
                QueuedChunkState::WaitingForFirstLiveRecipient => {
                    if let Some(params) = client_params(entry.entity) {
                        *occupied.get_mut() = QueuedChunkState::Live(Metadata {
                            recipients: vec![entry.entity],
                            lossy_compression: lossy_terrain_compression(entry.entity),
                            params,
                        });
                    }
                },
                QueuedChunkState::Live(meta) => {
                    meta.lossy_compression &= lossy_terrain_compression(entry.entity);
                    meta.recipients.push(entry.entity);
                },
            },
        }
    }

    let mut deferred_keys = HashSet::new();
    let mut admission = ChunkSerializeAdmission::default();

    for chunk_key in order {
        let Some(state) = states.remove(&chunk_key) else {
            continue;
        };

        match state {
            QueuedChunkState::WaitingForFirstLiveRecipient => {
                admission.dropped_without_live_recipients.push(chunk_key);
            },
            QueuedChunkState::Live(meta) => {
                if !terrain_has_chunk(chunk_key) {
                    admission
                        .terrain_missing_chunks
                        .push((chunk_key, meta.recipients.len()));
                    continue;
                }

                if admission.serializable_chunks.len() < budget {
                    admission.serializable_chunks.push((chunk_key, meta));
                } else {
                    deferred_keys.insert(chunk_key);
                }
            },
        }
    }

    queued_entries.retain(|entry| deferred_keys.contains(&entry.chunk_key));
    admission.deferred_distinct = deferred_keys.len();
    admission
}

/// This system will handle sending terrain to clients by
/// collecting chunks that need to be send for a single generation run and then
/// trigger a SlowJob for serialisation.
#[derive(Default)]
pub struct Sys;
impl<'a> System<'a> for Sys {
    type SystemData = (
        Read<'a, Tick>,
        Read<'a, Settings>,
        ReadStorage<'a, Client>,
        ReadStorage<'a, Presence>,
        ReadExpect<'a, EventBus<ChunkSendEntry>>,
        ReadExpect<'a, ChunkLifecycleMetrics>,
        ReadExpect<'a, NetworkRequestMetrics>,
        ReadExpect<'a, SlowJobPool>,
        ReadExpect<'a, TerrainGrid>,
        ReadExpect<'a, crossbeam_channel::Sender<SerializedChunk>>,
        ReadExpect<'a, ChunkLifecycleHandle>,
        WriteExpect<'a, ChunkSerializeQueue>,
    );

    const NAME: &'static str = "chunk_serialize";
    const ORIGIN: Origin = Origin::Server;
    const PHASE: Phase = Phase::Create;

    fn run(
        _job: &mut Job<Self>,
        (
            tick,
            server_settings,
            clients,
            presences,
            chunk_send_queues_bus,
            chunk_lifecycle_metrics,
            network_metrics,
            slow_jobs,
            terrain,
            chunk_sender,
            chunk_lifecycle,
            mut queued_entries,
        ): Self::SystemData,
    ) {
        // Only operate on the configured cadence (default: twice per second).
        // TODO: move out of this system and now even spawn this.
        if tick
            .0
            .rem_euclid(chunk_serialize_interval_ticks(&server_settings))
            != 0
        {
            return;
        }

        let new_entries = chunk_send_queues_bus.recv_all().collect::<Vec<_>>();
        let requests = new_entries.len() as u64;
        let distinct_requests = count_distinct_live_chunk_requests(&new_entries, |entity| {
            clients.get(entity).is_some()
        });
        queued_entries.entries.extend(new_entries);

        network_metrics
            .chunks_serialisation_requests
            .inc_by(requests);
        network_metrics
            .chunks_distinct_serialisation_requests
            .inc_by(distinct_requests);

        let admission = admit_chunk_serialize_queue_up_to(
            chunk_serialize_distinct_budget(&server_settings),
            &mut queued_entries.entries,
            |entity| clients.get(entity).map(|client| client.terrain_params()),
            |entity| {
                presences
                    .get(entity)
                    .map(|presence| presence.lossy_terrain_compression)
                    .unwrap_or(true)
            },
            |chunk_key| terrain.get_key_arc_real(chunk_key).is_some(),
        );

        if !admission.dropped_without_live_recipients.is_empty() {
            let mut lifecycle = chunk_lifecycle.lock().expect("Poisoned");
            for chunk_key in admission.dropped_without_live_recipients {
                let _ = lifecycle.complete(
                    chunk_key,
                    Some(tick.0),
                    ChunkLifecycleTerminal::Dropped,
                    Some(0),
                );
            }
        }
        if !admission.terrain_missing_chunks.is_empty() {
            let mut lifecycle = chunk_lifecycle.lock().expect("Poisoned");
            for (chunk_key, recipient_count) in admission.terrain_missing_chunks {
                let _ = lifecycle.complete(
                    chunk_key,
                    Some(tick.0),
                    ChunkLifecycleTerminal::Dropped,
                    Some(recipient_count),
                );
            }
        }
        if admission.deferred_distinct > 0 {
            network_metrics
                .chunks_serialize_budget_deferred
                .inc_by(admission.deferred_distinct as u64);
        }

        // Admitted chunks become terrain-backed serialize-ready work. The
        // later spawn gate only controls how many ready batches are handed to
        // CHUNK_SERIALIZER this cadence tick; it does not change distinct
        // admission or raw-entry carry-over semantics.
        let mut admitted_ready_chunks = Vec::with_capacity(admission.serializable_chunks.len());
        for (chunk_key, mut meta) in admission.serializable_chunks {
            let chunk = terrain
                .get_key_arc_real(chunk_key)
                .expect("serializable admission only retains terrain-backed chunks");
            meta.recipients.sort_unstable();
            meta.recipients.dedup();
            admitted_ready_chunks.push(AdmittedSerializableChunk {
                chunk: Arc::clone(chunk),
                chunk_key,
                lossy_compression: meta.lossy_compression,
                recipients: meta.recipients,
                params: meta.params,
            });
        }

        if !admitted_ready_chunks.is_empty() {
            {
                let mut lifecycle = chunk_lifecycle.lock().expect("Poisoned");
                for ready_chunk in &admitted_ready_chunks {
                    lifecycle.record_serialize_queued(
                        ready_chunk.chunk_key,
                        tick.0,
                        ready_chunk.recipients.len(),
                    );
                }
            }
            queued_entries.ready_chunks.extend(admitted_ready_chunks);
        }

        chunk_lifecycle_metrics
            .serialize_spawn_queue_len
            .set(queued_entries.ready_chunks.len() as i64);
        let spawn_plan = take_serialize_ready_chunks_into_batches(
            chunk_serialize_max_jobs(&server_settings),
            chunk_serialize_batch_size(&server_settings),
            &mut queued_entries.ready_chunks,
        );
        if spawn_plan.deferred_chunks > 0 {
            network_metrics
                .chunks_serialize_spawn_budget_deferred
                .inc_by(spawn_plan.deferred_chunks as u64);
        }

        for chunks in spawn_plan.batches {
            let chunk_sender = chunk_sender.clone();
            let chunk_lifecycle = chunk_lifecycle.clone();
            let handoff_dropped_counter = network_metrics.chunks_serialize_handoff_dropped.clone();
            let serialize_done_tick = tick.0;
            slow_jobs.spawn("CHUNK_SERIALIZER", move || {
                let mut chunks = chunks.into_iter();
                while let Some(chunk) = chunks.next() {
                    let msg = Client::prepare_chunk_update_msg(
                        ServerGeneral::TerrainChunkUpdate {
                            key: chunk.chunk_key,
                            chunk: Ok(SerializedTerrainChunk::via_heuristic(
                                &chunk.chunk,
                                chunk.lossy_compression,
                            )),
                        },
                        &chunk.params,
                    );
                    chunk_lifecycle
                        .lock()
                        .expect("Poisoned")
                        .record_serialize_done(chunk.chunk_key, serialize_done_tick);
                    if let Err(e) = chunk_sender.send(SerializedChunk {
                        chunk_key: chunk.chunk_key,
                        lossy_compression: chunk.lossy_compression,
                        msg,
                        recipients: chunk.recipients,
                    }) {
                        handoff_dropped_counter.inc_by(1 + chunks.len() as u64);
                        let _ = chunk_lifecycle.lock().expect("Poisoned").complete(
                            chunk.chunk_key,
                            None,
                            ChunkLifecycleTerminal::Dropped,
                            None,
                        );
                        tracing::warn!(?e, "cannot send serialized chunk to sender");
                        let mut lifecycle = chunk_lifecycle.lock().expect("Poisoned");
                        for remaining in chunks {
                            let _ = lifecycle.complete(
                                remaining.chunk_key,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk_lifecycle::ChunkLifecycleSource;
    use common::{
        ViewDistances,
        character::CharacterId,
        comp::{Presence, PresenceKind},
        event::EventBus,
        slowjob::SlowJobPool,
        terrain::{TerrainChunk, TerrainGrid},
    };
    use common_ecs::{SysMetrics, run_now};
    use prometheus::Registry;
    use specs::{Builder, WorldExt};
    use std::{sync::Arc, time::Duration};
    #[cfg(feature = "worldgen")] use world::World;

    #[cfg(feature = "worldgen")]
    use super::super::test_support::make_test_client;

    fn presence_with_vd(terrain_vd: u32, character_id: i64) -> Presence {
        Presence::new(
            ViewDistances {
                terrain: terrain_vd,
                entity: terrain_vd,
            },
            PresenceKind::Character(CharacterId(character_id)),
        )
    }

    fn configure_slow_jobs() -> SlowJobPool {
        let threadpool = Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .build()
                .expect("rayon pool"),
        );
        let slow_jobs = SlowJobPool::new(1, 0, threadpool);
        slow_jobs.configure("CHUNK_SERIALIZER", |limit| limit.max(1));
        slow_jobs
    }

    #[test]
    fn chunk_serialize_distinct_budget_defaults_to_unbounded_and_reads_settings() {
        let mut settings = crate::settings::Settings::default();
        assert_eq!(
            chunk_serialize_distinct_budget(&settings),
            DEFAULT_CHUNK_SERIALIZE_DISTINCT_BUDGET_PER_SERIALIZE_TICK
        );

        settings.chunk_serialize_distinct_budget_per_serialize_tick = Some(4);
        assert_eq!(chunk_serialize_distinct_budget(&settings), Some(4));

        settings.chunk_serialize_distinct_budget_per_serialize_tick = Some(0);
        assert_eq!(
            chunk_serialize_distinct_budget(&settings),
            DEFAULT_CHUNK_SERIALIZE_DISTINCT_BUDGET_PER_SERIALIZE_TICK
        );
    }

    #[test]
    fn chunk_serialize_batch_size_defaults_to_default_and_reads_settings() {
        let mut settings = crate::settings::Settings::default();
        assert_eq!(
            chunk_serialize_batch_size(&settings),
            DEFAULT_CHUNK_SERIALIZE_BATCH_SIZE_PER_JOB
        );

        settings.chunk_serialize_batch_size_per_job = 4;
        assert_eq!(chunk_serialize_batch_size(&settings), 4);

        settings.chunk_serialize_batch_size_per_job = 0;
        assert_eq!(
            chunk_serialize_batch_size(&settings),
            DEFAULT_CHUNK_SERIALIZE_BATCH_SIZE_PER_JOB
        );
    }

    #[test]
    fn chunk_serialize_interval_ticks_defaults_to_default_and_reads_settings() {
        let mut settings = crate::settings::Settings::default();
        assert_eq!(
            chunk_serialize_interval_ticks(&settings),
            DEFAULT_CHUNK_SERIALIZE_INTERVAL_TICKS
        );

        settings.chunk_serialize_interval_ticks = 5;
        assert_eq!(chunk_serialize_interval_ticks(&settings), 5);

        settings.chunk_serialize_interval_ticks = 0;
        assert_eq!(
            chunk_serialize_interval_ticks(&settings),
            DEFAULT_CHUNK_SERIALIZE_INTERVAL_TICKS
        );
    }

    #[test]
    fn chunk_serialize_max_jobs_defaults_to_unbounded_and_reads_settings() {
        let mut settings = crate::settings::Settings::default();
        assert_eq!(
            chunk_serialize_max_jobs(&settings),
            DEFAULT_CHUNK_SERIALIZE_MAX_JOBS_PER_SERIALIZE_TICK
        );

        settings.chunk_serialize_max_jobs_per_serialize_tick = Some(3);
        assert_eq!(chunk_serialize_max_jobs(&settings), Some(3));

        settings.chunk_serialize_max_jobs_per_serialize_tick = Some(0);
        assert_eq!(
            chunk_serialize_max_jobs(&settings),
            DEFAULT_CHUNK_SERIALIZE_MAX_JOBS_PER_SERIALIZE_TICK
        );
    }

    #[test]
    fn pack_serializable_chunks_into_batches_preserves_order_and_respects_batch_size() {
        assert_eq!(
            pack_serializable_chunks_into_batches(2, vec![1, 2, 3, 4, 5]),
            vec![vec![1, 2], vec![3, 4], vec![5]]
        );
        assert_eq!(
            pack_serializable_chunks_into_batches(1, vec![1, 2, 3]),
            vec![vec![1], vec![2], vec![3]]
        );
    }

    #[test]
    fn take_serialize_ready_chunks_into_batches_respects_max_jobs_and_preserves_fifo_order() {
        let mut ready_chunks = vec![1, 2, 3, 4, 5];

        let plan = take_serialize_ready_chunks_into_batches(Some(2), 2, &mut ready_chunks);

        assert_eq!(plan.batches, vec![vec![1, 2], vec![3, 4]]);
        assert_eq!(plan.deferred_chunks, 1);
        assert_eq!(ready_chunks, vec![5]);
    }

    #[cfg(feature = "worldgen")]
    #[test]
    fn chunk_serialize_queue_admission_preserves_fifo_tail_and_coalesces_late_duplicates() {
        let (client_support_a, client_a) = make_test_client();
        let (client_support_b, client_b) = make_test_client();
        let (client_support_c, client_c) = make_test_client();

        let mut world = specs::World::new();
        world.register::<Client>();
        world.register::<Presence>();

        let entity_a = world
            .create_entity()
            .with(client_a)
            .with(presence_with_vd(6, 1))
            .build();
        let entity_b = world
            .create_entity()
            .with(client_b)
            .with(presence_with_vd(6, 2))
            .build();
        let entity_c = world
            .create_entity()
            .with(client_c)
            .with(presence_with_vd(6, 3))
            .build();
        let mut queued_entries = vec![
            ChunkSendEntry {
                entity: entity_a,
                chunk_key: Vec2::new(1, 0),
            },
            ChunkSendEntry {
                entity: entity_b,
                chunk_key: Vec2::new(2, 0),
            },
            ChunkSendEntry {
                entity: entity_c,
                chunk_key: Vec2::new(1, 0),
            },
            ChunkSendEntry {
                entity: entity_b,
                chunk_key: Vec2::new(3, 0),
            },
        ];
        let clients = world.read_storage::<Client>();
        let presences = world.read_storage::<Presence>();
        let terrain_keys = [Vec2::new(1, 0), Vec2::new(2, 0), Vec2::new(3, 0)]
            .into_iter()
            .collect::<HashSet<_>>();

        let admission = admit_chunk_serialize_queue_up_to(
            Some(1),
            &mut queued_entries,
            |entity| clients.get(entity).map(|client| client.terrain_params()),
            |entity| {
                presences
                    .get(entity)
                    .map(|presence| presence.lossy_terrain_compression)
                    .unwrap_or(true)
            },
            |chunk_key| terrain_keys.contains(&chunk_key),
        );

        assert_eq!(admission.serializable_chunks.len(), 1);
        assert_eq!(admission.serializable_chunks[0].0, Vec2::new(1, 0));
        assert_eq!(admission.serializable_chunks[0].1.recipients, vec![
            entity_a, entity_c
        ]);
        assert_eq!(admission.deferred_distinct, 2);
        assert!(admission.dropped_without_live_recipients.is_empty());
        assert!(admission.terrain_missing_chunks.is_empty());
        assert_eq!(
            queued_entries
                .iter()
                .map(|entry| entry.chunk_key)
                .collect::<Vec<_>>(),
            vec![Vec2::new(2, 0), Vec2::new(3, 0)]
        );

        drop(presences);
        drop(clients);
        drop(client_support_a);
        drop(client_support_b);
        drop(client_support_c);
    }

    #[cfg(feature = "worldgen")]
    #[test]
    fn chunk_serialize_queue_admission_drops_dead_and_missing_chunks_without_spending_budget() {
        let (client_support_a, client_a) = make_test_client();
        let (client_support_b, client_b) = make_test_client();

        let mut world = specs::World::new();
        world.register::<Client>();
        world.register::<Presence>();

        let dead_entity = world.create_entity().build();
        let live_entity_a = world
            .create_entity()
            .with(client_a)
            .with(presence_with_vd(6, 1))
            .build();
        let live_entity_b = world
            .create_entity()
            .with(client_b)
            .with(presence_with_vd(6, 2))
            .build();
        let mut queued_entries = vec![
            ChunkSendEntry {
                entity: dead_entity,
                chunk_key: Vec2::new(1, 0),
            },
            ChunkSendEntry {
                entity: live_entity_a,
                chunk_key: Vec2::new(2, 0),
            },
            ChunkSendEntry {
                entity: live_entity_b,
                chunk_key: Vec2::new(3, 0),
            },
        ];
        let clients = world.read_storage::<Client>();
        let presences = world.read_storage::<Presence>();
        let terrain_keys = [Vec2::new(3, 0)].into_iter().collect::<HashSet<_>>();

        let admission = admit_chunk_serialize_queue_up_to(
            Some(1),
            &mut queued_entries,
            |entity| clients.get(entity).map(|client| client.terrain_params()),
            |entity| {
                presences
                    .get(entity)
                    .map(|presence| presence.lossy_terrain_compression)
                    .unwrap_or(true)
            },
            |chunk_key| terrain_keys.contains(&chunk_key),
        );

        assert_eq!(admission.serializable_chunks.len(), 1);
        assert_eq!(admission.serializable_chunks[0].0, Vec2::new(3, 0));
        assert_eq!(admission.dropped_without_live_recipients, vec![Vec2::new(
            1, 0
        )]);
        assert_eq!(admission.terrain_missing_chunks, vec![(Vec2::new(2, 0), 1)]);
        assert_eq!(admission.deferred_distinct, 0);
        assert!(queued_entries.is_empty());

        drop(presences);
        drop(clients);
        drop(client_support_a);
        drop(client_support_b);
    }

    #[cfg(feature = "worldgen")]
    #[test]
    fn chunk_serialize_queue_admission_waits_for_late_live_recipient_before_dropping_key() {
        let (client_support, client) = make_test_client();

        let mut world = specs::World::new();
        world.register::<Client>();
        world.register::<Presence>();

        let dead_entity = world.create_entity().build();
        let live_entity = world
            .create_entity()
            .with(client)
            .with(presence_with_vd(6, 1))
            .build();
        let mut queued_entries = vec![
            ChunkSendEntry {
                entity: dead_entity,
                chunk_key: Vec2::new(1, 0),
            },
            ChunkSendEntry {
                entity: live_entity,
                chunk_key: Vec2::new(1, 0),
            },
        ];
        let clients = world.read_storage::<Client>();
        let presences = world.read_storage::<Presence>();

        let admission = admit_chunk_serialize_queue_up_to(
            Some(1),
            &mut queued_entries,
            |entity| clients.get(entity).map(|client| client.terrain_params()),
            |entity| {
                presences
                    .get(entity)
                    .map(|presence| presence.lossy_terrain_compression)
                    .unwrap_or(true)
            },
            |chunk_key| chunk_key == Vec2::new(1, 0),
        );

        assert_eq!(admission.serializable_chunks.len(), 1);
        assert_eq!(admission.serializable_chunks[0].0, Vec2::new(1, 0));
        assert_eq!(admission.serializable_chunks[0].1.recipients, vec![
            live_entity
        ]);
        assert!(admission.dropped_without_live_recipients.is_empty());
        assert!(admission.terrain_missing_chunks.is_empty());
        assert_eq!(admission.deferred_distinct, 0);
        assert!(queued_entries.is_empty());

        drop(presences);
        drop(clients);
        drop(client_support);
    }

    #[cfg(feature = "worldgen")]
    #[test]
    fn chunk_serialize_sys_dedupes_recipients_and_records_serialize_lifecycle() {
        let (client_support_a, client_a) = make_test_client();
        let (client_support_b, client_b) = make_test_client();
        let registry = Registry::new();
        let chunk_lifecycle_metrics =
            crate::metrics::ChunkLifecycleMetrics::new(&registry).expect("chunk lifecycle metrics");
        let network_metrics =
            crate::metrics::NetworkRequestMetrics::new(&registry).expect("network metrics");
        let lifecycle = crate::chunk_lifecycle::new_chunk_lifecycle_handle();
        let (world, _) = World::empty();
        let world = Arc::new(world);
        let target_chunk = Vec2::new(2, -3);
        let (serialized_sender, serialized_receiver) = crossbeam_channel::unbounded();

        let mut terrain = TerrainGrid::new(
            world.sim().map_size_lg(),
            Arc::new(world.sim().generate_oob_chunk()),
        )
        .expect("terrain grid");
        terrain.insert(target_chunk, Arc::new(TerrainChunk::water(0)));

        let mut ecs = specs::World::new();
        ecs.register::<Client>();
        ecs.register::<Presence>();

        ecs.insert(SysMetrics::default());
        ecs.insert(Tick(30));
        ecs.insert(crate::settings::Settings::default());
        ecs.insert(EventBus::<ChunkSendEntry>::default());
        ecs.insert(crate::chunk_serialize::ChunkSerializeQueue::default());
        ecs.insert(chunk_lifecycle_metrics);
        ecs.insert(network_metrics);
        ecs.insert(configure_slow_jobs());
        ecs.insert(terrain);
        ecs.insert(serialized_sender);
        ecs.insert(lifecycle.clone());

        let entity_a = ecs
            .create_entity()
            .with(client_a)
            .with(presence_with_vd(6, 1))
            .build();
        let entity_b = ecs
            .create_entity()
            .with(client_b)
            .with(presence_with_vd(6, 2))
            .build();

        lifecycle
            .lock()
            .expect("poisoned chunk lifecycle")
            .record_source(target_chunk, ChunkLifecycleSource::TerrainSync, 29);

        let chunk_send_bus = ecs.read_resource::<EventBus<ChunkSendEntry>>();
        chunk_send_bus.emit_now(ChunkSendEntry {
            entity: entity_a,
            chunk_key: target_chunk,
        });
        chunk_send_bus.emit_now(ChunkSendEntry {
            entity: entity_a,
            chunk_key: target_chunk,
        });
        chunk_send_bus.emit_now(ChunkSendEntry {
            entity: entity_b,
            chunk_key: target_chunk,
        });
        drop(chunk_send_bus);

        run_now::<Sys>(&ecs);

        let serialized: SerializedChunk = serialized_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("serialized chunk should be handed off");
        assert_eq!(serialized.chunk_key, target_chunk);
        assert!(!serialized.lossy_compression);
        assert_eq!(serialized.recipients, vec![entity_a, entity_b]);
        assert!(serialized_receiver.try_recv().is_err());

        let lifecycle_table = lifecycle.lock().expect("poisoned chunk lifecycle");
        let entry = lifecycle_table
            .entry(target_chunk)
            .expect("serialize lifecycle entry should remain active until send");
        assert_eq!(entry.first_seen_tick, 29);
        assert_eq!(entry.serialize_queued_tick, Some(30));
        assert_eq!(entry.serialize_done_tick, Some(30));
        assert_eq!(entry.recipient_count, 2);
        assert!(
            entry
                .source_mask
                .contains(ChunkLifecycleSource::TerrainSync)
        );

        drop(lifecycle_table);
        drop(ecs);
        drop(client_support_a);
        drop(client_support_b);
    }

    #[cfg(feature = "worldgen")]
    #[test]
    fn chunk_serialize_sys_respects_configured_distinct_budget_across_serialize_ticks() {
        let (client_support, client) = make_test_client();
        let registry = Registry::new();
        let chunk_lifecycle_metrics =
            crate::metrics::ChunkLifecycleMetrics::new(&registry).expect("chunk lifecycle metrics");
        let network_metrics =
            crate::metrics::NetworkRequestMetrics::new(&registry).expect("network metrics");
        let lifecycle = crate::chunk_lifecycle::new_chunk_lifecycle_handle();
        let (world, _) = World::empty();
        let world = Arc::new(world);
        let first_chunk = Vec2::new(2, -3);
        let second_chunk = Vec2::new(3, -3);
        let (serialized_sender, serialized_receiver) = crossbeam_channel::unbounded();

        let mut terrain = TerrainGrid::new(
            world.sim().map_size_lg(),
            Arc::new(world.sim().generate_oob_chunk()),
        )
        .expect("terrain grid");
        terrain.insert(first_chunk, Arc::new(TerrainChunk::water(0)));
        terrain.insert(second_chunk, Arc::new(TerrainChunk::water(0)));

        let mut ecs = specs::World::new();
        ecs.register::<Client>();
        ecs.register::<Presence>();

        let mut settings = crate::settings::Settings::default();
        settings.chunk_serialize_distinct_budget_per_serialize_tick = Some(1);

        ecs.insert(SysMetrics::default());
        ecs.insert(Tick(30));
        ecs.insert(settings);
        ecs.insert(EventBus::<ChunkSendEntry>::default());
        ecs.insert(crate::chunk_serialize::ChunkSerializeQueue::default());
        ecs.insert(chunk_lifecycle_metrics);
        ecs.insert(network_metrics);
        ecs.insert(configure_slow_jobs());
        ecs.insert(terrain);
        ecs.insert(serialized_sender);
        ecs.insert(lifecycle.clone());

        let entity = ecs
            .create_entity()
            .with(client)
            .with(presence_with_vd(6, 1))
            .build();

        {
            let mut table = lifecycle.lock().expect("poisoned chunk lifecycle");
            table.record_source(first_chunk, ChunkLifecycleSource::TerrainSync, 29);
            table.record_source(second_chunk, ChunkLifecycleSource::TerrainSync, 29);
        }

        let chunk_send_bus = ecs.read_resource::<EventBus<ChunkSendEntry>>();
        chunk_send_bus.emit_now(ChunkSendEntry {
            entity,
            chunk_key: first_chunk,
        });
        chunk_send_bus.emit_now(ChunkSendEntry {
            entity,
            chunk_key: second_chunk,
        });
        drop(chunk_send_bus);

        run_now::<Sys>(&ecs);

        let first_serialized: SerializedChunk = serialized_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("first chunk should be serialized on first eligible tick");
        assert_eq!(first_serialized.chunk_key, first_chunk);
        assert!(serialized_receiver.try_recv().is_err());
        {
            let queued = ecs.read_resource::<crate::chunk_serialize::ChunkSerializeQueue>();
            assert_eq!(queued.entries.len(), 1);
            assert_eq!(queued.entries[0].chunk_key, second_chunk);
            assert!(queued.ready_chunks.is_empty());
        }
        {
            let network_metrics = ecs.read_resource::<NetworkRequestMetrics>();
            assert_eq!(network_metrics.chunks_serialize_budget_deferred.get(), 1);
        }

        ecs.write_resource::<Tick>().0 = 31;
        run_now::<Sys>(&ecs);
        assert!(serialized_receiver.try_recv().is_err());
        {
            let queued = ecs.read_resource::<crate::chunk_serialize::ChunkSerializeQueue>();
            assert_eq!(queued.entries.len(), 1);
            assert_eq!(queued.entries[0].chunk_key, second_chunk);
            assert!(queued.ready_chunks.is_empty());
        }

        ecs.write_resource::<Tick>().0 = 45;
        run_now::<Sys>(&ecs);

        let second_serialized: SerializedChunk = serialized_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("deferred chunk should be serialized on next eligible tick");
        assert_eq!(second_serialized.chunk_key, second_chunk);
        assert!(serialized_receiver.try_recv().is_err());
        {
            let queued = ecs.read_resource::<crate::chunk_serialize::ChunkSerializeQueue>();
            assert!(queued.entries.is_empty());
            assert!(queued.ready_chunks.is_empty());
        }

        let lifecycle_table = lifecycle.lock().expect("poisoned chunk lifecycle");
        let first_entry = lifecycle_table
            .entry(first_chunk)
            .expect("first chunk lifecycle entry should still be active before send");
        assert_eq!(first_entry.serialize_queued_tick, Some(30));
        assert_eq!(first_entry.serialize_done_tick, Some(30));
        let second_entry = lifecycle_table
            .entry(second_chunk)
            .expect("second chunk lifecycle entry should be queued on second eligible tick");
        assert_eq!(second_entry.serialize_queued_tick, Some(45));
        assert_eq!(second_entry.serialize_done_tick, Some(45));

        drop(lifecycle_table);
        drop(ecs);
        drop(client_support);
    }

    #[cfg(feature = "worldgen")]
    #[test]
    fn chunk_serialize_sys_respects_configured_max_jobs_across_serialize_ticks() {
        let (client_support, client) = make_test_client();
        let registry = Registry::new();
        let chunk_lifecycle_metrics =
            crate::metrics::ChunkLifecycleMetrics::new(&registry).expect("chunk lifecycle metrics");
        let network_metrics =
            crate::metrics::NetworkRequestMetrics::new(&registry).expect("network metrics");
        let lifecycle = crate::chunk_lifecycle::new_chunk_lifecycle_handle();
        let (world, _) = World::empty();
        let world = Arc::new(world);
        let first_chunk = Vec2::new(5, -3);
        let second_chunk = Vec2::new(6, -3);
        let (serialized_sender, serialized_receiver) = crossbeam_channel::unbounded();

        let mut terrain = TerrainGrid::new(
            world.sim().map_size_lg(),
            Arc::new(world.sim().generate_oob_chunk()),
        )
        .expect("terrain grid");
        terrain.insert(first_chunk, Arc::new(TerrainChunk::water(0)));
        terrain.insert(second_chunk, Arc::new(TerrainChunk::water(0)));

        let mut ecs = specs::World::new();
        ecs.register::<Client>();
        ecs.register::<Presence>();

        let mut settings = crate::settings::Settings::default();
        settings.chunk_serialize_batch_size_per_job = 1;
        settings.chunk_serialize_max_jobs_per_serialize_tick = Some(1);

        ecs.insert(SysMetrics::default());
        ecs.insert(Tick(30));
        ecs.insert(settings);
        ecs.insert(EventBus::<ChunkSendEntry>::default());
        ecs.insert(crate::chunk_serialize::ChunkSerializeQueue::default());
        ecs.insert(chunk_lifecycle_metrics);
        ecs.insert(network_metrics);
        ecs.insert(configure_slow_jobs());
        ecs.insert(terrain);
        ecs.insert(serialized_sender);
        ecs.insert(lifecycle.clone());

        let entity = ecs
            .create_entity()
            .with(client)
            .with(presence_with_vd(6, 1))
            .build();

        {
            let mut table = lifecycle.lock().expect("poisoned chunk lifecycle");
            table.record_source(first_chunk, ChunkLifecycleSource::TerrainSync, 29);
            table.record_source(second_chunk, ChunkLifecycleSource::TerrainSync, 29);
        }

        let chunk_send_bus = ecs.read_resource::<EventBus<ChunkSendEntry>>();
        chunk_send_bus.emit_now(ChunkSendEntry {
            entity,
            chunk_key: first_chunk,
        });
        chunk_send_bus.emit_now(ChunkSendEntry {
            entity,
            chunk_key: second_chunk,
        });
        drop(chunk_send_bus);

        run_now::<Sys>(&ecs);

        let first_serialized: SerializedChunk = serialized_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("first chunk should be serialized on first eligible tick");
        assert_eq!(first_serialized.chunk_key, first_chunk);
        assert!(serialized_receiver.try_recv().is_err());
        {
            let queued = ecs.read_resource::<crate::chunk_serialize::ChunkSerializeQueue>();
            assert!(queued.entries.is_empty());
            assert_eq!(queued.ready_chunks.len(), 1);
            assert_eq!(queued.ready_chunks[0].chunk_key, second_chunk);
        }
        {
            let chunk_lifecycle_metrics = ecs.read_resource::<ChunkLifecycleMetrics>();
            assert_eq!(chunk_lifecycle_metrics.serialize_spawn_queue_len.get(), 2);
        }
        {
            let network_metrics = ecs.read_resource::<NetworkRequestMetrics>();
            assert_eq!(
                network_metrics.chunks_serialize_spawn_budget_deferred.get(),
                1
            );
        }
        {
            let lifecycle_table = lifecycle.lock().expect("poisoned chunk lifecycle");
            let first_entry = lifecycle_table
                .entry(first_chunk)
                .expect("first chunk lifecycle entry should remain active before send");
            assert_eq!(first_entry.serialize_queued_tick, Some(30));
            assert_eq!(first_entry.serialize_done_tick, Some(30));
            let second_entry = lifecycle_table
                .entry(second_chunk)
                .expect("second chunk lifecycle entry should wait in ready backlog");
            assert_eq!(second_entry.serialize_queued_tick, Some(30));
            assert_eq!(second_entry.serialize_done_tick, None);
        }

        ecs.write_resource::<Tick>().0 = 45;
        run_now::<Sys>(&ecs);

        let second_serialized: SerializedChunk = serialized_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("second chunk should be serialized on next eligible tick");
        assert_eq!(second_serialized.chunk_key, second_chunk);
        assert!(serialized_receiver.try_recv().is_err());
        {
            let queued = ecs.read_resource::<crate::chunk_serialize::ChunkSerializeQueue>();
            assert!(queued.entries.is_empty());
            assert!(queued.ready_chunks.is_empty());
        }
        {
            let chunk_lifecycle_metrics = ecs.read_resource::<ChunkLifecycleMetrics>();
            assert_eq!(chunk_lifecycle_metrics.serialize_spawn_queue_len.get(), 1);
        }
        {
            let network_metrics = ecs.read_resource::<NetworkRequestMetrics>();
            assert_eq!(
                network_metrics.chunks_serialize_spawn_budget_deferred.get(),
                1
            );
        }

        let lifecycle_table = lifecycle.lock().expect("poisoned chunk lifecycle");
        let second_entry = lifecycle_table
            .entry(second_chunk)
            .expect("second chunk lifecycle entry should still be active before send");
        assert_eq!(second_entry.serialize_queued_tick, Some(30));
        assert_eq!(second_entry.serialize_done_tick, Some(45));

        drop(lifecycle_table);
        drop(ecs);
        drop(client_support);
    }

    #[cfg(feature = "worldgen")]
    #[test]
    fn chunk_serialize_sys_respects_configured_serialize_cadence() {
        let (client_support, client) = make_test_client();
        let registry = Registry::new();
        let chunk_lifecycle_metrics =
            crate::metrics::ChunkLifecycleMetrics::new(&registry).expect("chunk lifecycle metrics");
        let network_metrics =
            crate::metrics::NetworkRequestMetrics::new(&registry).expect("network metrics");
        let lifecycle = crate::chunk_lifecycle::new_chunk_lifecycle_handle();
        let (world, _) = World::empty();
        let world = Arc::new(world);
        let target_chunk = Vec2::new(4, -2);
        let (serialized_sender, serialized_receiver) = crossbeam_channel::unbounded();

        let mut terrain = TerrainGrid::new(
            world.sim().map_size_lg(),
            Arc::new(world.sim().generate_oob_chunk()),
        )
        .expect("terrain grid");
        terrain.insert(target_chunk, Arc::new(TerrainChunk::water(0)));

        let mut ecs = specs::World::new();
        ecs.register::<Client>();
        ecs.register::<Presence>();

        let mut settings = crate::settings::Settings::default();
        settings.chunk_serialize_interval_ticks = 5;

        ecs.insert(SysMetrics::default());
        ecs.insert(Tick(4));
        ecs.insert(settings);
        ecs.insert(EventBus::<ChunkSendEntry>::default());
        ecs.insert(crate::chunk_serialize::ChunkSerializeQueue::default());
        ecs.insert(chunk_lifecycle_metrics);
        ecs.insert(network_metrics);
        ecs.insert(configure_slow_jobs());
        ecs.insert(terrain);
        ecs.insert(serialized_sender);
        ecs.insert(lifecycle.clone());

        let entity = ecs
            .create_entity()
            .with(client)
            .with(presence_with_vd(6, 1))
            .build();

        lifecycle
            .lock()
            .expect("poisoned chunk lifecycle")
            .record_source(target_chunk, ChunkLifecycleSource::TerrainSync, 3);

        let chunk_send_bus = ecs.read_resource::<EventBus<ChunkSendEntry>>();
        chunk_send_bus.emit_now(ChunkSendEntry {
            entity,
            chunk_key: target_chunk,
        });
        drop(chunk_send_bus);

        run_now::<Sys>(&ecs);

        assert!(serialized_receiver.try_recv().is_err());
        {
            let queued = ecs.read_resource::<crate::chunk_serialize::ChunkSerializeQueue>();
            assert!(queued.entries.is_empty());
            assert!(queued.ready_chunks.is_empty());
        }
        {
            let network_metrics = ecs.read_resource::<NetworkRequestMetrics>();
            assert_eq!(network_metrics.chunks_serialisation_requests.get(), 0);
            assert_eq!(
                network_metrics.chunks_distinct_serialisation_requests.get(),
                0
            );
        }
        {
            let lifecycle_table = lifecycle.lock().expect("poisoned chunk lifecycle");
            let entry = lifecycle_table
                .entry(target_chunk)
                .expect("lifecycle entry should remain active before cadence tick");
            assert_eq!(entry.serialize_queued_tick, None);
            assert_eq!(entry.serialize_done_tick, None);
        }

        ecs.write_resource::<Tick>().0 = 5;
        run_now::<Sys>(&ecs);

        let serialized: SerializedChunk = serialized_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("serialized chunk should be handed off on cadence tick");
        assert_eq!(serialized.chunk_key, target_chunk);
        assert!(serialized_receiver.try_recv().is_err());
        {
            let network_metrics = ecs.read_resource::<NetworkRequestMetrics>();
            assert_eq!(network_metrics.chunks_serialisation_requests.get(), 1);
            assert_eq!(
                network_metrics.chunks_distinct_serialisation_requests.get(),
                1
            );
        }

        let lifecycle_table = lifecycle.lock().expect("poisoned chunk lifecycle");
        let entry = lifecycle_table
            .entry(target_chunk)
            .expect("serialize lifecycle entry should remain active until send");
        assert_eq!(entry.serialize_queued_tick, Some(5));
        assert_eq!(entry.serialize_done_tick, Some(5));

        drop(lifecycle_table);
        drop(ecs);
        drop(client_support);
    }
}
