#[cfg(not(feature = "worldgen"))]
use crate::test_world::World;
use crate::{
    Settings, Tick,
    chunk_lifecycle::{ChunkLifecycleHandle, ChunkLifecycleSource},
    chunk_serialize::ChunkSendEntry,
    client::Client,
};
use common::{
    comp::{Pos, Presence},
    event::EventBus,
};
use common_ecs::{Job, Origin, Phase, System};
use common_net::msg::{CompressedData, ServerGeneral};
use common_state::TerrainChanges;
use rayon::prelude::*;
use specs::{Entities, Join, Read, ReadExpect, ReadStorage};
use std::sync::Arc;
#[cfg(feature = "worldgen")] use world::World;

/// This systems sends modified chunks (existing chunks that had a new chunk
/// generated) to clients as well as block modifications in existing chunks.
#[derive(Default)]
pub struct Sys;
impl<'a> System<'a> for Sys {
    type SystemData = (
        Entities<'a>,
        Read<'a, Tick>,
        ReadExpect<'a, Arc<World>>,
        Read<'a, Settings>,
        Read<'a, TerrainChanges>,
        ReadExpect<'a, EventBus<ChunkSendEntry>>,
        ReadExpect<'a, ChunkLifecycleHandle>,
        ReadStorage<'a, Pos>,
        ReadStorage<'a, Presence>,
        ReadStorage<'a, Client>,
    );

    const NAME: &'static str = "terrain_sync";
    const ORIGIN: Origin = Origin::Server;
    const PHASE: Phase = Phase::Create;

    fn run(
        _job: &mut Job<Self>,
        (
            entities,
            tick,
            world,
            server_settings,
            terrain_changes,
            chunk_send_bus,
            chunk_lifecycle,
            positions,
            presences,
            clients,
        ): Self::SystemData,
    ) {
        let tick = tick.0;
        let max_view_distance = server_settings.max_view_distance.unwrap_or(u32::MAX);
        let runtime_topology = world.runtime_topology_descriptor();
        let (presences_position_entities, _) = super::terrain::prepare_player_presences(
            &runtime_topology,
            max_view_distance,
            &entities,
            &positions,
            &presences,
            &clients,
        );
        let max_loaded_chunk_vd = super::terrain::max_loaded_chunk_vd(max_view_distance);

        // Sync changed chunks
        terrain_changes.modified_chunks.par_iter().for_each_init(
            || (chunk_send_bus.emitter(), chunk_lifecycle.clone()),
            |(chunk_send_emitter, chunk_lifecycle), &chunk_key| {
                // We only have to check players inside the maximum view distance of the server
                // of our own position.
                //
                // We start by partitioning by X, finding only entities in chunks within the X
                // range of us.  These are guaranteed in bounds due to restrictions on max view
                // distance (namely: the square of any chunk coordinate plus the max view
                // distance along both axes must fit in an i32).
                super::terrain::loaded_entities_for_chunk(
                    &presences_position_entities,
                    &runtime_topology,
                    chunk_key,
                    max_loaded_chunk_vd,
                )
                .for_each(|entity| {
                    chunk_lifecycle.lock().expect("Poisoned").record_source(
                        chunk_key,
                        ChunkLifecycleSource::TerrainSync,
                        tick,
                    );
                    chunk_send_emitter.emit(ChunkSendEntry { entity, chunk_key });
                });
            },
        );

        // TODO: Don't send all changed blocks to all clients
        // Sync changed blocks
        if !terrain_changes.modified_blocks.is_empty() {
            let mut lazy_msg = None;
            for (_, client) in (&presences, &clients).join() {
                if lazy_msg.is_none() {
                    lazy_msg = Some(client.prepare(ServerGeneral::TerrainBlockUpdates(
                        CompressedData::compress(&terrain_changes.modified_blocks, 1),
                    )));
                }
                lazy_msg.as_ref().map(|msg| client.send_prepared(msg));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "worldgen")]
    use super::super::test_support::make_test_client;
    use super::*;
    use common::{
        ViewDistances,
        character::CharacterId,
        comp::{Pos, Presence, PresenceKind},
        vol::RectVolSize,
    };
    use common_ecs::{SysMetrics, run_now};
    use specs::{Builder, WorldExt};
    use std::sync::Arc;
    use vek::*;

    fn pos_in_chunk(chunk_key: Vec2<i32>) -> Pos {
        let chunk_size = common::terrain::TerrainChunkSize::RECT_SIZE.map(|e| e as f32);
        let wpos2d = chunk_key.map(|coord| coord as f32) * chunk_size + Vec2::broadcast(1.0);
        Pos(wpos2d.with_z(0.0))
    }

    fn presence_with_vd(terrain_vd: u32, character_id: i64) -> Presence {
        Presence::new(
            ViewDistances {
                terrain: terrain_vd,
                entity: terrain_vd,
            },
            PresenceKind::Character(CharacterId(character_id)),
        )
    }

    #[cfg(feature = "worldgen")]
    #[test]
    fn terrain_sync_sys_sends_only_visible_modified_chunks_and_records_source() {
        let (near_client_support, near_client) = make_test_client();
        let (far_client_support, far_client) = make_test_client();
        let settings = Settings::default();
        let (world, _) = World::empty();
        let world = Arc::new(world);
        let target_chunk = Vec2::zero();
        let lifecycle = crate::chunk_lifecycle::new_chunk_lifecycle_handle();

        let mut ecs = specs::World::new();
        ecs.register::<Pos>();
        ecs.register::<Presence>();
        ecs.register::<Client>();

        ecs.insert(SysMetrics::default());
        ecs.insert(Tick(77));
        ecs.insert(settings);
        ecs.insert(Arc::clone(&world));
        ecs.insert({
            let mut terrain_changes = TerrainChanges::default();
            terrain_changes.modified_chunks.insert(target_chunk);
            terrain_changes
        });
        ecs.insert(EventBus::<ChunkSendEntry>::default());
        ecs.insert(lifecycle.clone());

        let near_entity = ecs
            .create_entity()
            .with(pos_in_chunk(target_chunk))
            .with(presence_with_vd(6, 1))
            .with(near_client)
            .build();
        let _far_entity = ecs
            .create_entity()
            .with(pos_in_chunk(Vec2::new(12, 12)))
            .with(presence_with_vd(1, 2))
            .with(far_client)
            .build();

        run_now::<Sys>(&ecs);

        let send_entries = ecs
            .read_resource::<EventBus<ChunkSendEntry>>()
            .recv_all()
            .collect::<Vec<_>>();
        assert_eq!(send_entries, vec![ChunkSendEntry {
            entity: near_entity,
            chunk_key: target_chunk,
        }]);

        let lifecycle_table = lifecycle.lock().expect("poisoned chunk lifecycle");
        let entry = lifecycle_table
            .entry(target_chunk)
            .expect("terrain_sync should record source for modified chunk");
        assert_eq!(entry.first_seen_tick, 77);
        assert!(
            entry
                .source_mask
                .contains(ChunkLifecycleSource::TerrainSync)
        );

        drop(lifecycle_table);
        drop(ecs);
        drop(near_client_support);
        drop(far_client_support);
    }
}
