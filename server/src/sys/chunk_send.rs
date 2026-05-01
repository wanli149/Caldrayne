use crate::{
    Tick,
    chunk_lifecycle::{ChunkLifecycleHandle, ChunkLifecycleTerminal},
    chunk_serialize::SerializedChunk,
    client::Client,
    metrics::{ChunkLifecycleMetrics, NetworkRequestMetrics},
    settings::{DEFAULT_CHUNK_SEND_BUDGET_PER_TICK, Settings},
};

use common_ecs::{Job, Origin, Phase, System};
use specs::{Read, ReadExpect, ReadStorage};

fn chunk_send_budget(server_settings: &Settings) -> Option<usize> {
    match server_settings.chunk_send_budget_per_tick {
        Some(0) => DEFAULT_CHUNK_SEND_BUDGET_PER_TICK,
        budget => budget,
    }
}

/// Drain serialized terrain payloads from the serialize-to-send queue and
/// deliver them to live clients.
#[derive(Default)]
pub struct Sys;
impl<'a> System<'a> for Sys {
    type SystemData = (
        Read<'a, Tick>,
        ReadStorage<'a, Client>,
        ReadExpect<'a, ChunkLifecycleHandle>,
        ReadExpect<'a, ChunkLifecycleMetrics>,
        ReadExpect<'a, NetworkRequestMetrics>,
        ReadExpect<'a, Settings>,
        ReadExpect<'a, crossbeam_channel::Receiver<SerializedChunk>>,
    );

    const NAME: &'static str = "chunk_send";
    const ORIGIN: Origin = Origin::Server;
    const PHASE: Phase = Phase::Create;

    fn run(
        _job: &mut Job<Self>,
        (
            tick,
            clients,
            chunk_lifecycle,
            chunk_lifecycle_metrics,
            network_metrics,
            server_settings,
            chunk_receiver,
        ): Self::SystemData,
    ) {
        let mut lossy_served = 0u64;
        let mut lossless_served = 0u64;
        let mut lossy_failed = 0u64;
        let mut lossless_failed = 0u64;
        let queued_at_intake = chunk_receiver.len();
        let send_budget = chunk_send_budget(&server_settings).unwrap_or(usize::MAX);
        chunk_lifecycle_metrics
            .send_queue_len
            .set(queued_at_intake as i64);
        network_metrics
            .chunks_send_budget_deferred
            .inc_by(queued_at_intake.saturating_sub(send_budget) as u64);
        for sc in chunk_receiver.try_iter().take(send_budget) {
            let expected_recipients = sc.recipients.len() as u64;
            let mut attempted_recipients = 0u64;
            let mut failed_recipients = 0u64;

            for recipient in sc.recipients {
                if let Some(client) = clients.get(recipient) {
                    attempted_recipients += 1;
                    if client.send_prepared(&sc.msg).is_err() {
                        failed_recipients += 1;
                    }
                }
            }

            let delivered_recipients = attempted_recipients.saturating_sub(failed_recipients);
            let missing_recipients = expected_recipients.saturating_sub(attempted_recipients);
            failed_recipients += missing_recipients;

            if sc.lossy_compression {
                lossy_served += delivered_recipients;
                lossy_failed += failed_recipients;
            } else {
                lossless_served += delivered_recipients;
                lossless_failed += failed_recipients;
            }

            let terminal = if expected_recipients == 0 || attempted_recipients == 0 {
                ChunkLifecycleTerminal::Dropped
            } else if failed_recipients > 0 {
                ChunkLifecycleTerminal::PartialSendFail
            } else {
                ChunkLifecycleTerminal::SentOk
            };
            let mut lifecycle = chunk_lifecycle.lock().expect("Poisoned");
            let _ = lifecycle.complete(
                sc.chunk_key,
                Some(tick.0),
                terminal,
                Some(expected_recipients as usize),
            );
        }
        network_metrics.chunks_served_lossy.inc_by(lossy_served);
        network_metrics
            .chunks_served_lossless
            .inc_by(lossless_served);
        network_metrics
            .chunks_send_failed_lossy
            .inc_by(lossy_failed);
        network_metrics
            .chunks_send_failed_lossless
            .inc_by(lossless_failed);
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "worldgen")]
    use super::super::test_support::make_test_client;
    use super::*;
    use crate::chunk_lifecycle::ChunkLifecycleSource;
    use common_ecs::{SysMetrics, run_now};
    use common_net::msg::ServerGeneral;
    use prometheus::Registry;
    use specs::{Builder, WorldExt};
    use vek::*;

    #[cfg(feature = "worldgen")]
    #[test]
    fn chunk_send_sys_classifies_send_terminals_and_updates_queue_metrics() {
        let (client_support, client) = make_test_client();
        let ok_msg = client.prepare(ServerGeneral::SetViewDistance(6));
        let partial_msg = client.prepare(ServerGeneral::SetViewDistance(7));
        let dropped_msg = client.prepare(ServerGeneral::SetViewDistance(8));

        let registry = Registry::new();
        let chunk_lifecycle_metrics =
            crate::metrics::ChunkLifecycleMetrics::new(&registry).expect("chunk lifecycle metrics");
        let network_metrics =
            crate::metrics::NetworkRequestMetrics::new(&registry).expect("network metrics");
        let lifecycle = crate::chunk_lifecycle::new_chunk_lifecycle_handle();
        let (serialized_sender, serialized_receiver) = crossbeam_channel::unbounded();

        let ok_chunk = Vec2::new(1, 0);
        let partial_chunk = Vec2::new(2, 0);
        let dropped_chunk = Vec2::new(3, 0);

        let mut ecs = specs::World::new();
        ecs.register::<Client>();

        ecs.insert(SysMetrics::default());
        ecs.insert(Tick(50));
        ecs.insert(lifecycle.clone());
        ecs.insert(chunk_lifecycle_metrics);
        ecs.insert(network_metrics);
        ecs.insert(crate::settings::Settings::default());
        ecs.insert(serialized_receiver);

        let live_entity = ecs.create_entity().with(client).build();
        let missing_entity = ecs.create_entity().build();

        {
            let mut table = lifecycle.lock().expect("poisoned chunk lifecycle");
            for (key, recipients) in [
                (ok_chunk, 1_usize),
                (partial_chunk, 2_usize),
                (dropped_chunk, 1_usize),
            ] {
                table.record_source(key, ChunkLifecycleSource::TerrainSync, 49);
                table.record_serialize_queued(key, 49, recipients);
                table.record_serialize_done(key, 49);
            }
        }

        serialized_sender
            .send(SerializedChunk {
                chunk_key: ok_chunk,
                lossy_compression: false,
                msg: ok_msg,
                recipients: vec![live_entity],
            })
            .expect("queue sent-ok chunk");
        serialized_sender
            .send(SerializedChunk {
                chunk_key: partial_chunk,
                lossy_compression: false,
                msg: partial_msg,
                recipients: vec![live_entity, missing_entity],
            })
            .expect("queue partial-fail chunk");
        serialized_sender
            .send(SerializedChunk {
                chunk_key: dropped_chunk,
                lossy_compression: false,
                msg: dropped_msg,
                recipients: vec![missing_entity],
            })
            .expect("queue dropped chunk");

        run_now::<Sys>(&ecs);

        {
            let chunk_lifecycle_metrics = ecs.read_resource::<ChunkLifecycleMetrics>();
            assert_eq!(chunk_lifecycle_metrics.send_queue_len.get(), 3);
        }
        {
            let network_metrics = ecs.read_resource::<NetworkRequestMetrics>();
            assert_eq!(network_metrics.chunks_served_lossless.get(), 2);
            assert_eq!(network_metrics.chunks_send_failed_lossless.get(), 2);
            assert_eq!(network_metrics.chunks_served_lossy.get(), 0);
            assert_eq!(network_metrics.chunks_send_failed_lossy.get(), 0);
            assert_eq!(network_metrics.chunks_send_budget_deferred.get(), 0);
        }

        let table = lifecycle.lock().expect("poisoned chunk lifecycle");
        assert_eq!(table.active_entries_len(), 0);
        let abnormal = table
            .abnormal_summary()
            .expect("partial send failure should be recorded as abnormal");
        assert_eq!(abnormal.recent_abnormal_count(), 1);
        assert_eq!(abnormal.latest_chunk_key(), [
            partial_chunk.x,
            partial_chunk.y
        ]);
        assert_eq!(abnormal.latest_terminal_str(), "partial_send_fail");
        assert_eq!(abnormal.latest_tick(), Some(50));

        drop(table);
        drop(ecs);
        drop(client_support);
    }

    #[cfg(feature = "worldgen")]
    #[test]
    fn chunk_send_sys_respects_configured_send_budget_across_ticks() {
        let (client_support, client) = make_test_client();
        let first_msg = client.prepare(ServerGeneral::SetViewDistance(9));
        let second_msg = client.prepare(ServerGeneral::SetViewDistance(10));
        let third_msg = client.prepare(ServerGeneral::SetViewDistance(11));

        let registry = Registry::new();
        let chunk_lifecycle_metrics =
            crate::metrics::ChunkLifecycleMetrics::new(&registry).expect("chunk lifecycle metrics");
        let network_metrics =
            crate::metrics::NetworkRequestMetrics::new(&registry).expect("network metrics");
        let lifecycle = crate::chunk_lifecycle::new_chunk_lifecycle_handle();
        let (serialized_sender, serialized_receiver) = crossbeam_channel::unbounded();

        let first_chunk = Vec2::new(11, 0);
        let second_chunk = Vec2::new(12, 0);
        let third_chunk = Vec2::new(13, 0);

        let mut ecs = specs::World::new();
        ecs.register::<Client>();

        let mut settings = crate::settings::Settings::default();
        settings.chunk_send_budget_per_tick = Some(2);

        ecs.insert(SysMetrics::default());
        ecs.insert(Tick(50));
        ecs.insert(lifecycle.clone());
        ecs.insert(chunk_lifecycle_metrics);
        ecs.insert(network_metrics);
        ecs.insert(settings);
        ecs.insert(serialized_receiver);

        let live_entity = ecs.create_entity().with(client).build();
        let missing_entity = ecs.create_entity().build();

        {
            let mut table = lifecycle.lock().expect("poisoned chunk lifecycle");
            for (key, recipients) in [
                (first_chunk, 1_usize),
                (second_chunk, 2_usize),
                (third_chunk, 1_usize),
            ] {
                table.record_source(key, ChunkLifecycleSource::TerrainSync, 49);
                table.record_serialize_queued(key, 49, recipients);
                table.record_serialize_done(key, 49);
            }
        }

        serialized_sender
            .send(SerializedChunk {
                chunk_key: first_chunk,
                lossy_compression: false,
                msg: first_msg,
                recipients: vec![live_entity],
            })
            .expect("queue first chunk");
        serialized_sender
            .send(SerializedChunk {
                chunk_key: second_chunk,
                lossy_compression: false,
                msg: second_msg,
                recipients: vec![live_entity, missing_entity],
            })
            .expect("queue second chunk");
        serialized_sender
            .send(SerializedChunk {
                chunk_key: third_chunk,
                lossy_compression: false,
                msg: third_msg,
                recipients: vec![missing_entity],
            })
            .expect("queue third chunk");

        run_now::<Sys>(&ecs);

        {
            let chunk_lifecycle_metrics = ecs.read_resource::<ChunkLifecycleMetrics>();
            assert_eq!(chunk_lifecycle_metrics.send_queue_len.get(), 3);
        }
        {
            let network_metrics = ecs.read_resource::<NetworkRequestMetrics>();
            assert_eq!(network_metrics.chunks_served_lossless.get(), 2);
            assert_eq!(network_metrics.chunks_send_failed_lossless.get(), 1);
            assert_eq!(network_metrics.chunks_served_lossy.get(), 0);
            assert_eq!(network_metrics.chunks_send_failed_lossy.get(), 0);
            assert_eq!(network_metrics.chunks_send_budget_deferred.get(), 1);
        }
        {
            let receiver = ecs.read_resource::<crossbeam_channel::Receiver<SerializedChunk>>();
            assert_eq!(receiver.len(), 1);
        }
        {
            let table = lifecycle.lock().expect("poisoned chunk lifecycle");
            assert!(table.entry(first_chunk).is_none());
            assert!(table.entry(second_chunk).is_none());
            let third_entry = table
                .entry(third_chunk)
                .expect("third chunk should remain active until the next tick");
            assert_eq!(third_entry.serialize_done_tick, Some(49));
            assert_eq!(third_entry.send_attempted_tick, None);
            assert_eq!(table.active_entries_len(), 1);
        }

        ecs.write_resource::<Tick>().0 = 51;
        run_now::<Sys>(&ecs);

        {
            let chunk_lifecycle_metrics = ecs.read_resource::<ChunkLifecycleMetrics>();
            assert_eq!(chunk_lifecycle_metrics.send_queue_len.get(), 1);
        }
        {
            let network_metrics = ecs.read_resource::<NetworkRequestMetrics>();
            assert_eq!(network_metrics.chunks_served_lossless.get(), 2);
            assert_eq!(network_metrics.chunks_send_failed_lossless.get(), 2);
            assert_eq!(network_metrics.chunks_served_lossy.get(), 0);
            assert_eq!(network_metrics.chunks_send_failed_lossy.get(), 0);
            assert_eq!(network_metrics.chunks_send_budget_deferred.get(), 1);
        }
        {
            let receiver = ecs.read_resource::<crossbeam_channel::Receiver<SerializedChunk>>();
            assert_eq!(receiver.len(), 0);
        }

        let table = lifecycle.lock().expect("poisoned chunk lifecycle");
        assert_eq!(table.active_entries_len(), 0);
        assert!(table.entry(third_chunk).is_none());
        let abnormal = table
            .abnormal_summary()
            .expect("partial send failure should remain the latest abnormal record");
        assert_eq!(abnormal.recent_abnormal_count(), 1);
        assert_eq!(abnormal.latest_chunk_key(), [
            second_chunk.x,
            second_chunk.y
        ]);
        assert_eq!(abnormal.latest_terminal_str(), "partial_send_fail");
        assert_eq!(abnormal.latest_tick(), Some(50));

        drop(table);
        drop(ecs);
        drop(client_support);
    }
}
