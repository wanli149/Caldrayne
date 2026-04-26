use crate::{
    Tick,
    chunk_lifecycle::{ChunkLifecycleHandle, ChunkLifecycleTerminal},
    chunk_serialize::SerializedChunk,
    client::Client,
    metrics::{ChunkLifecycleMetrics, NetworkRequestMetrics},
};

use common_ecs::{Job, Origin, Phase, System};
use specs::{Read, ReadExpect, ReadStorage};

/// This system will handle sending terrain to clients by
/// collecting chunks that need to be send for a single generation run and then
/// trigger a SlowJob for serialisation.
#[derive(Default)]
pub struct Sys;
impl<'a> System<'a> for Sys {
    type SystemData = (
        Read<'a, Tick>,
        ReadStorage<'a, Client>,
        ReadExpect<'a, ChunkLifecycleHandle>,
        ReadExpect<'a, ChunkLifecycleMetrics>,
        ReadExpect<'a, NetworkRequestMetrics>,
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
            chunk_receiver,
        ): Self::SystemData,
    ) {
        let mut lossy_served = 0u64;
        let mut lossless_served = 0u64;
        let mut lossy_failed = 0u64;
        let mut lossless_failed = 0u64;
        chunk_lifecycle_metrics
            .send_queue_len
            .set(chunk_receiver.len() as i64);
        for sc in chunk_receiver.try_iter() {
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
