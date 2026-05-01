#[cfg(feature = "persistent_world")]
use crate::TerrainPersistence;
#[cfg(not(feature = "worldgen"))]
use crate::test_world::{IndexOwned, World};
use tracing::error;
#[cfg(feature = "worldgen")]
use world::{IndexOwned, World};

#[cfg(feature = "worldgen")] use crate::rtsim;
use crate::{
    ChunkRequest, Tick,
    chunk_generator::ChunkGenerator,
    chunk_lifecycle::{ChunkLifecycleHandle, ChunkLifecycleTerminal},
    chunk_serialize::ChunkSendEntry,
    client::Client,
    metrics::NetworkRequestMetrics,
    presence::RepositionToFreeSpace,
    settings::{DEFAULT_COMPLETED_CHUNK_INTAKE_BUDGET_PER_TICK, Settings},
};
use common::{
    SkillSetBuilder,
    calendar::Calendar,
    combat::{DeathEffects, RiderEffects},
    comp::{
        self, BehaviorCapability, Content, ForceUpdate, Pos, Presence, Waypoint, agent,
        biped_small, bird_medium,
    },
    event::{
        CreateNpcEvent, CreateNpcGroupEvent, CreateSpecialEntityEvent, EmitExt, EventBus,
        NpcBuilder,
    },
    event_emitters,
    generation::{EntityInfo, EntitySpawn, SpecialEntity},
    lottery::LootSpec,
    resources::{Time, TimeOfDay},
    slowjob::SlowJobPool,
    terrain::{CoordinateConversions, TerrainChunkSize, TerrainGrid},
    util::Dir,
    vol::RectVolSize,
};

use common_ecs::{Job, Origin, Phase, System};
use common_net::msg::{ServerGeneral, world_msg::RuntimeTopologyDescriptor};
use common_state::TerrainChanges;
use comp::Behavior;
use core::cmp::Reverse;
use itertools::Itertools;
use rayon::{iter::Either, prelude::*};
use specs::{
    Entities, Entity, Join, LendJoin, ParJoin, Read, ReadExpect, ReadStorage, SystemData, Write,
    WriteExpect, WriteStorage, shred, storage::GenericReadStorage,
};
use std::{collections::HashSet, f32::consts::TAU, sync::Arc};
use vek::*;

#[cfg(feature = "persistent_world")]
pub type TerrainPersistenceData<'a> = Option<Write<'a, TerrainPersistence>>;
#[cfg(not(feature = "persistent_world"))]
pub type TerrainPersistenceData<'a> = ();

pub const SAFE_ZONE_RADIUS: f32 = 200.0;

#[cfg(feature = "worldgen")]
type RtSimData<'a> = WriteExpect<'a, rtsim::RtSim>;
#[cfg(not(feature = "worldgen"))]
type RtSimData<'a> = ();

event_emitters! {
    struct Events[Emitters] {
        create_npc: CreateNpcEvent,
        create_npc_group: CreateNpcGroupEvent,
        create_waypoint: CreateSpecialEntityEvent,
    }
}

#[derive(SystemData)]
pub struct Data<'a> {
    events: Events<'a>,
    tick: Read<'a, Tick>,
    server_settings: Read<'a, Settings>,
    time_of_day: Read<'a, TimeOfDay>,
    calendar: Read<'a, Calendar>,
    slow_jobs: ReadExpect<'a, SlowJobPool>,
    index: ReadExpect<'a, IndexOwned>,
    world: ReadExpect<'a, Arc<World>>,
    chunk_send_bus: ReadExpect<'a, EventBus<ChunkSendEntry>>,
    chunk_generator: WriteExpect<'a, ChunkGenerator>,
    chunk_lifecycle: ReadExpect<'a, ChunkLifecycleHandle>,
    chunk_lifecycle_metrics: ReadExpect<'a, crate::metrics::ChunkLifecycleMetrics>,
    network_request_metrics: ReadExpect<'a, NetworkRequestMetrics>,
    terrain: WriteExpect<'a, TerrainGrid>,
    terrain_changes: Write<'a, TerrainChanges>,
    chunk_requests: Write<'a, Vec<ChunkRequest>>,
    rtsim: RtSimData<'a>,
    #[cfg(feature = "persistent_world")]
    terrain_persistence: TerrainPersistenceData<'a>,
    positions: WriteStorage<'a, Pos>,
    presences: ReadStorage<'a, Presence>,
    clients: ReadStorage<'a, Client>,
    entities: Entities<'a>,
    reposition_entities: WriteStorage<'a, RepositionToFreeSpace>,
    forced_updates: WriteStorage<'a, ForceUpdate>,
    waypoints: WriteStorage<'a, Waypoint>,
    time: ReadExpect<'a, Time>,
}

/// This system will handle loading generated chunks and unloading
/// unneeded chunks.
///     1. Inserts newly generated chunks into the TerrainGrid
///     2. Sends new chunks to nearby clients
///     3. Handles the chunk's supplement (e.g. npcs)
///     4. Removes chunks outside the range of players
#[derive(Default)]
pub struct Sys;
impl<'a> System<'a> for Sys {
    type SystemData = Data<'a>;

    const NAME: &'static str = "terrain";
    const ORIGIN: Origin = Origin::Server;
    const PHASE: Phase = Phase::Create;

    fn run(_job: &mut Job<Self>, mut data: Self::SystemData) {
        let mut emitters = data.events.get_emitters();

        // Generate requested chunks
        //
        // Submit requests for chunks right before receiving finished chunks so that we
        // don't create duplicate work for chunks that just finished but are not
        // yet added to the terrain.
        data.chunk_lifecycle_metrics
            .chunk_requests_len
            .set(data.chunk_requests.len() as i64);
        let world = Arc::clone(&data.world);
        let index = data.index.clone();
        let time_of_day = *data.time_of_day;
        let calendar = data.calendar.clone();
        let tick = data.tick.0;
        let request_submission = submit_chunk_requests_to_generation_up_to(
            chunk_generation_submit_budget(&data.server_settings),
            &mut data.chunk_requests,
            |request, budget_available| {
                if data.terrain.get_key_arc(request.key).is_some() {
                    return ChunkRequestSubmissionDecision::CoalescedLoaded;
                }
                if data.chunk_generator.is_pending(request.key) {
                    return ChunkRequestSubmissionDecision::CoalescedPending;
                }
                if !budget_available {
                    return ChunkRequestSubmissionDecision::Deferred;
                }

                data.chunk_generator.generate_chunk(
                    Some(request.entity),
                    request.key,
                    &data.slow_jobs,
                    Arc::clone(&world),
                    &data.rtsim,
                    index.clone(),
                    (time_of_day, calendar.clone()),
                    tick,
                );
                ChunkRequestSubmissionDecision::Admitted
            },
        );
        if request_submission.deferred > 0 {
            data.network_request_metrics
                .chunks_generation_budget_deferred
                .inc_by(request_submission.deferred as u64);
        }

        let mut rng = rand::rng();
        // Fetch any generated `TerrainChunk`s and insert them into the terrain.
        // Also, send the chunk data to anybody that is close by.
        let mut new_chunks = Vec::new();
        drive_completed_chunk_intake_up_to(
            completed_chunk_intake_budget(&data.server_settings),
            || {
                let Some((key, res)) = data.chunk_generator.recv_new_chunk(data.tick.0) else {
                    return false;
                };

                #[cfg_attr(not(feature = "persistent_world"), expect(unused_mut))]
                let (mut chunk, supplement) = match res {
                    Ok((chunk, supplement)) => {
                        data.chunk_generator.record_chunk_generated();
                        (chunk, supplement)
                    },
                    Err(Some(entity)) => {
                        data.chunk_generator.record_chunk_failed();
                        let _ = data.chunk_lifecycle.lock().expect("Poisoned").complete(
                            key,
                            Some(data.tick.0),
                            ChunkLifecycleTerminal::GenerateErr,
                            None,
                        );
                        if let Some(client) = data.clients.get(entity) {
                            client.send_fallible(ServerGeneral::TerrainChunkUpdate {
                                key,
                                chunk: Err(()),
                            });
                        }
                        return true;
                    },
                    Err(None) => {
                        data.chunk_generator.record_chunk_failed();
                        let _ = data.chunk_lifecycle.lock().expect("Poisoned").complete(
                            key,
                            Some(data.tick.0),
                            ChunkLifecycleTerminal::GenerateErr,
                            None,
                        );
                        return true;
                    },
                };

                // Apply changes from terrain persistence to this chunk
                #[cfg(feature = "persistent_world")]
                if let Some(terrain_persistence) = data.terrain_persistence.as_mut() {
                    terrain_persistence.apply_changes(key, &mut chunk);
                }

                // Arcify the chunk
                let chunk = Arc::new(chunk);

                // Add to list of chunks to send to nearby players.
                new_chunks.push(key);

                // TODO: code duplication for chunk insertion between here and state.rs
                // Insert the chunk into terrain changes
                if data.terrain.insert(key, chunk).is_some() {
                    data.terrain_changes.modified_chunks.insert(key);
                } else {
                    data.terrain_changes.new_chunks.insert(key);
                    #[cfg(feature = "worldgen")]
                    data.rtsim
                        .hook_load_chunk(key, supplement.rtsim_max_resources, &data.world);
                }

                // Handle chunk supplement
                for entity_spawn in supplement.entity_spawns {
                    // Check this because it's a common source of weird bugs
                    let check_pos = |pos: Vec3<f32>| {
                        assert!(
                            data.terrain
                                .pos_key(pos.map(|e| e.floor() as i32))
                                .map2(key, |e, tgt| (e - tgt).abs() <= 1)
                                .reduce_and(),
                            "Chunk spawned entity that wasn't nearby",
                        )
                    };

                    match entity_spawn {
                        EntitySpawn::Entity(entity) => {
                            check_pos(entity.pos);

                            let data = SpawnEntityData::from_entity_info(*entity);
                            match data {
                                SpawnEntityData::Special(pos, entity) => {
                                    emitters.emit(CreateSpecialEntityEvent { pos, entity });
                                },
                                SpawnEntityData::Npc(data) => {
                                    let (npc_builder, pos) = data.to_npc_builder();

                                    emitters.emit(CreateNpcEvent {
                                        pos,
                                        ori: comp::Ori::from(Dir::random_2d(&mut rng)),
                                        npc: npc_builder.with_anchor(comp::Anchor::Chunk(key)),
                                    });
                                },
                            }
                        },
                        EntitySpawn::Group(group) => {
                            for entity in group.iter() {
                                check_pos(entity.pos);
                            }

                            let create_npc_events = group
                                .into_iter()
                                .filter_map(|entity| {
                                    match SpawnEntityData::from_entity_info(entity) {
                                        SpawnEntityData::Special(..) => None,
                                        SpawnEntityData::Npc(data) => {
                                            let (npc_builder, pos) = data.to_npc_builder();
                                            Some(CreateNpcEvent {
                                                pos,
                                                ori: comp::Ori::from(Dir::random_2d(&mut rng)),
                                                npc: npc_builder
                                                    .with_anchor(comp::Anchor::Chunk(key)),
                                            })
                                        },
                                    }
                                })
                                .collect::<Vec<_>>();

                            emitters.emit(CreateNpcGroupEvent {
                                npcs: create_npc_events,
                            });
                        },
                    }
                }

                true
            },
        );

        // TODO: Consider putting this in another system since this forces us to take
        // positions by write rather than read access.
        let repositioned = (&data.entities, &mut data.positions, (&mut data.forced_updates).maybe(), &data.reposition_entities)
            // TODO: Consider using par_bridge() because Rayon has very poor work splitting for
            // sparse joins.
            .par_join()
            .filter_map(|(entity, pos, force_update, reposition)| {
                // NOTE: We use regular as casts rather than as_ because we want to saturate on
                // overflow.
                let entity_pos = pos.0.map(|x| x as i32);
                // If an entity is marked as needing repositioning once the chunk loads (e.g.
                // from having just logged in), reposition them.
                let chunk_pos = TerrainGrid::chunk_key(entity_pos);
                let chunk = data.terrain.get_key(chunk_pos)?;
                let new_pos = if reposition.needs_ground {
                    data.terrain.try_find_ground(entity_pos)
                } else {
                    data.terrain.try_find_space(entity_pos)
                }.map(|x| x.as_::<f32>()).unwrap_or_else(|| chunk.find_accessible_pos(entity_pos.xy(), false));
                pos.0 = new_pos;
                force_update.map(|force_update| force_update.update());
                Some((entity, new_pos, reposition.modify_waypoints))
            })
            .collect::<Vec<_>>();

        for (entity, new_pos, modify_waypoints) in repositioned {
            if modify_waypoints && let Some(waypoint) = data.waypoints.get_mut(entity) {
                *waypoint = Waypoint::new(new_pos, *data.time);
            }

            data.reposition_entities.remove(entity);
        }

        let max_view_distance = data.server_settings.max_view_distance.unwrap_or(u32::MAX);
        let runtime_topology = data.world.runtime_topology_descriptor();
        let (presences_position_entities, presences_positions) = prepare_player_presences(
            &runtime_topology,
            max_view_distance,
            &data.entities,
            &data.positions,
            &data.presences,
            &data.clients,
        );
        let max_loaded_chunk_vd = max_loaded_chunk_vd(max_view_distance);

        // Send the chunks to all nearby players.
        new_chunks.par_iter().for_each_init(
            || data.chunk_send_bus.emitter(),
            |chunk_send_emitter, chunk_key| {
                // We only have to check players inside the maximum view distance of the server
                // of our own position.
                //
                // We start by partitioning by X, finding only entities in chunks within the X
                // range of us.  These are guaranteed in bounds due to restrictions on max view
                // distance (namely: the square of any chunk coordinate plus the max view
                // distance along both axes must fit in an i32).
                loaded_entities_for_chunk(
                    &presences_position_entities,
                    &runtime_topology,
                    *chunk_key,
                    max_loaded_chunk_vd,
                )
                .for_each(|entity| {
                    chunk_send_emitter.emit(ChunkSendEntry {
                        entity,
                        chunk_key: *chunk_key,
                    });
                });
            },
        );

        let tick = (data.tick.0 % 16) as i32;

        // Remove chunks that are too far from players.
        //
        // Note that all chunks involved here (both terrain chunks and pending chunks)
        // are guaranteed in bounds.  This simplifies the rest of the logic
        // here.
        let chunks_to_remove = data.terrain
            .par_keys()
            .copied()
            // There may be lots of pending chunks, so don't check them all.  This should be okay
            // as long as we're maintaining a reasonable tick rate.
            .chain(data.chunk_generator.par_pending_chunks())
            // Don't check every chunk every tick (spread over 16 ticks)
            //
            // TODO: Investigate whether we can add support for performing this filtering directly
            // within hashbrown (basically, specify we want to iterate through just buckets with
            // hashes in a particular range).  This could provide significiant speedups since we
            // could avoid having to iterate through a bunch of buckets we don't care about.
            //
            // TODO: Make the percentage of the buckets that we go through adjust dynamically
            // depending on the current number of chunks.  In the worst case, we might want to scan
            // just 1/256 of the chunks each tick, for example.
            .filter(|k| k.x % 4 + (k.y % 4) * 4 == tick)
            .filter(|&chunk_key| {
                // We only have to check players inside the maximum view distance of the server of
                // our own position.
                //
                // We start by partitioning by X, finding only entities in chunks within the X
                // range of us.  These are guaranteed in bounds due to restrictions on max view
                // distance (namely: the square of any chunk coordinate plus the max view distance
                // along both axes must fit in an i32).
                !chunk_visible_to_any_loaded_presence(
                    &runtime_topology,
                    chunk_key,
                    max_loaded_chunk_vd,
                    &presences_positions,
                )
            })
            .collect::<Vec<_>>();

        let chunks_to_remove = chunks_to_remove
            .into_iter()
            .filter_map(|key| {
                // Register the unloading of this chunk from terrain persistence
                #[cfg(feature = "persistent_world")]
                if let Some(terrain_persistence) = data.terrain_persistence.as_mut() {
                    terrain_persistence.unload_chunk(key);
                }

                data.chunk_generator.cancel_if_pending(key, data.tick.0);

                // If you want to trigger any behaivour on unload, do it in `Server::tick` by
                // reading `TerrainChanges::removed_chunks` since chunks can also be removed
                // using eg. /reload_chunks

                // TODO: code duplication for chunk insertion between here and state.rs
                data.terrain.remove(key).inspect(|_| {
                    data.terrain_changes.removed_chunks.insert(key);
                })
            })
            .collect::<Vec<_>>();
        if !chunks_to_remove.is_empty() {
            // Drop chunks in a background thread.
            data.slow_jobs.spawn("CHUNK_DROP", move || {
                drop(chunks_to_remove);
            });
        }
    }
}

// TODO: better name
#[derive(Debug)]
pub struct NpcData {
    pub pos: Pos,
    pub stats: comp::Stats,
    pub skill_set: comp::SkillSet,
    pub health: Option<comp::Health>,
    pub poise: comp::Poise,
    pub inventory: comp::inventory::Inventory,
    pub agent: Option<comp::Agent>,
    pub body: comp::Body,
    pub alignment: comp::Alignment,
    pub scale: comp::Scale,
    pub loot: LootSpec<String>,
    pub pets: Vec<(NpcData, Vec3<f32>)>,
    pub death_effects: Option<DeathEffects>,
    pub rider_effects: Option<RiderEffects>,
    pub rider: Option<Box<NpcData>>,
}

/// Convinient structure to use when you need to create new npc
/// from EntityInfo
// TODO: better name?
// TODO: if this is send around network, optimize the large_enum_variant
#[expect(clippy::large_enum_variant)] // TODO: evaluate
#[derive(Debug)]
pub enum SpawnEntityData {
    Npc(NpcData),
    Special(Vec3<f32>, SpecialEntity),
}

impl SpawnEntityData {
    pub fn from_entity_info(entity: EntityInfo) -> Self {
        let EntityInfo {
            // flags
            special_entity,
            has_agency,
            agent_mark,
            alignment,
            no_flee,
            idle_wander_factor,
            aggro_range_multiplier,
            // stats
            body,
            name,
            scale,
            pos,
            loot,
            // tools and skills
            skillset_asset,
            loadout: mut loadout_builder,
            inventory: items,
            make_loadout,
            trading_information: economy,
            pets,
            rider,
            death_effects,
            rider_effects,
        } = entity;

        if let Some(special) = special_entity {
            return Self::Special(pos, special);
        }

        let name = name.unwrap_or_else(Content::dummy);
        let stats = comp::Stats::new(name, body);

        let skill_set = {
            let skillset_builder = SkillSetBuilder::default();
            if let Some(skillset_asset) = skillset_asset {
                skillset_builder.with_asset_expect(&skillset_asset).build()
            } else {
                skillset_builder.build()
            }
        };

        let inventory = {
            // Evaluate lazy function for loadout creation
            if let Some(make_loadout) = make_loadout {
                loadout_builder =
                    loadout_builder.with_creator(make_loadout, economy.as_ref(), None);
            }
            let loadout = loadout_builder.build();
            let mut inventory = comp::inventory::Inventory::with_loadout(loadout, body);
            for (num, mut item) in items {
                if let Err(e) = item.set_amount(num) {
                    tracing::warn!(
                        "error during creating inventory for {name:?} at {pos}: {e:?}",
                        name = &stats.name,
                    );
                }
                if let Err(e) = inventory.push(item) {
                    tracing::warn!(
                        "error during creating inventory for {name:?} at {pos}: {e:?}",
                        name = &stats.name,
                    );
                }
            }

            inventory
        };

        let health = Some(comp::Health::new(body));
        let poise = comp::Poise::new(body);

        // Allow Humanoid, BirdMedium, and Parrot to speak
        let can_speak = match body {
            comp::Body::Humanoid(_) => true,
            comp::Body::BipedSmall(biped_small) => {
                matches!(biped_small.species, biped_small::Species::Flamekeeper)
            },
            comp::Body::BirdMedium(bird_medium) => match bird_medium.species {
                bird_medium::Species::Parrot => alignment == comp::Alignment::Npc,
                _ => false,
            },
            _ => false,
        };

        let trade_for_site = if matches!(agent_mark, Some(agent::Mark::Merchant)) {
            economy.map(|e| e.id)
        } else {
            None
        };

        let agent = has_agency.then(|| {
            let mut agent = comp::Agent::from_body(&body).with_behavior(
                Behavior::default()
                    .maybe_with_capabilities(can_speak.then_some(BehaviorCapability::SPEAK))
                    .maybe_with_capabilities(trade_for_site.map(|_| BehaviorCapability::TRADE))
                    .with_trade_site(trade_for_site),
            );

            // Non-humanoids get a patrol origin to stop them moving too far
            if !matches!(body, comp::Body::Humanoid(_)) {
                agent = agent.with_patrol_origin(pos);
            }

            agent
                .with_no_flee_if(matches!(agent_mark, Some(agent::Mark::Guard)) || no_flee)
                .with_idle_wander_factor(idle_wander_factor)
                .with_aggro_range_multiplier(aggro_range_multiplier)
        });

        let agent = if matches!(alignment, comp::Alignment::Enemy)
            && matches!(body, comp::Body::Humanoid(_))
        {
            agent.map(|a| a.with_aggro_no_warn().with_no_flee_if(true))
        } else {
            agent
        };

        SpawnEntityData::Npc(NpcData {
            pos: Pos(pos),
            stats,
            skill_set,
            health,
            poise,
            inventory,
            agent,
            body,
            alignment,
            scale: comp::Scale(scale),
            loot,
            pets: {
                let pet_count = pets.len() as f32;
                pets.into_iter()
                    .enumerate()
                    .flat_map(|(i, pet)| {
                        Some((
                            SpawnEntityData::from_entity_info(pet)
                                .into_npc_data_inner()
                                .inspect_err(|data| {
                                    error!("Pets must be SpawnEntityData::Npc, but found: {data:?}")
                                })
                                .ok()?,
                            Vec2::one()
                                .rotated_z(TAU * (i as f32 / pet_count))
                                .with_z(0.0)
                                * ((pet_count * 3.0) / TAU),
                        ))
                    })
                    .collect()
            },
            rider: rider.and_then(|e| {
                Some(Box::new(
                    SpawnEntityData::from_entity_info(*e)
                        .into_npc_data_inner()
                        .ok()?,
                ))
            }),
            death_effects,
            rider_effects,
        })
    }

    #[expect(clippy::result_large_err)]
    pub fn into_npc_data_inner(self) -> Result<NpcData, Self> {
        match self {
            SpawnEntityData::Npc(inner) => Ok(inner),
            other => Err(other),
        }
    }
}

impl NpcData {
    pub fn to_npc_builder(self) -> (NpcBuilder, comp::Pos) {
        let NpcData {
            pos,
            stats,
            skill_set,
            health,
            poise,
            inventory,
            agent,
            body,
            alignment,
            scale,
            loot,
            pets,
            death_effects,
            rider_effects,
            rider,
        } = self;

        (
            NpcBuilder::new(stats, body, alignment)
                .with_skill_set(skill_set)
                .with_health(health)
                .with_poise(poise)
                .with_inventory(inventory)
                .with_agent(agent)
                .with_scale(scale)
                .with_loot(loot)
                .with_pets(
                    pets.into_iter()
                        .map(|(pet, offset)| (pet.to_npc_builder().0, offset))
                        .collect::<Vec<_>>(),
                )
                .with_rider(rider.map(|rider| rider.to_npc_builder().0))
                .with_death_effects(death_effects)
                .with_rider_effects(rider_effects),
            pos,
        )
    }
}

pub fn convert_to_loaded_vd(vd: u32, max_view_distance: u32) -> i32 {
    // Hardcoded max VD to prevent stupid view distances from creating overflows.
    // This must be a value ≤
    // √(i32::MAX - 2 * ((1 << (MAX_WORLD_BLOCKS_LG - TERRAIN_CHUNK_BLOCKS_LG) - 1)²
    // - 1)) / 2
    //
    // since otherwise we could end up overflowing.  Since it is a requirement that
    // each dimension (in chunks) has to fit in a i16, we can derive √((1<<31)-1
    // - 2*((1<<15)-1)^2) / 2 ≥ 1 << 7 as the absolute limit.
    //
    // TODO: Make this more official and use it elsewhere.
    const MAX_VD: u32 = 1 << 7;

    // This fuzzy threshold prevents chunks rapidly unloading and reloading when
    // players move over a chunk border.
    const UNLOAD_THRESHOLD: u32 = 2;

    // NOTE: This cast is safe for the reasons mentioned above.
    (vd.clamp(crate::MIN_VD, max_view_distance)
        .saturating_add(UNLOAD_THRESHOLD))
    .min(MAX_VD) as i32
}

fn completed_chunk_intake_budget(server_settings: &Settings) -> usize {
    match server_settings.completed_chunk_intake_budget_per_tick {
        0 => DEFAULT_COMPLETED_CHUNK_INTAKE_BUDGET_PER_TICK,
        budget => budget,
    }
}

fn chunk_generation_submit_budget(server_settings: &Settings) -> Option<usize> {
    server_settings.chunk_generation_submit_budget_per_tick
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ChunkRequestSubmissionStats {
    admitted: usize,
    deferred: usize,
    coalesced_loaded: usize,
    coalesced_pending: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChunkRequestSubmissionDecision {
    CoalescedLoaded,
    CoalescedPending,
    Admitted,
    Deferred,
}

fn submit_chunk_requests_to_generation_up_to(
    budget: Option<usize>,
    chunk_requests: &mut Vec<ChunkRequest>,
    mut decide: impl FnMut(&ChunkRequest, bool) -> ChunkRequestSubmissionDecision,
) -> ChunkRequestSubmissionStats {
    let budget = budget.unwrap_or(usize::MAX);
    let mut stats = ChunkRequestSubmissionStats::default();
    let mut processed = 0;

    while processed < chunk_requests.len() {
        let request = &chunk_requests[processed];
        let budget_available = stats.admitted < budget;
        match decide(request, budget_available) {
            ChunkRequestSubmissionDecision::CoalescedLoaded => {
                stats.coalesced_loaded += 1;
                processed += 1;
            },
            ChunkRequestSubmissionDecision::CoalescedPending => {
                stats.coalesced_pending += 1;
                processed += 1;
            },
            ChunkRequestSubmissionDecision::Admitted => {
                stats.admitted += 1;
                processed += 1;
            },
            ChunkRequestSubmissionDecision::Deferred => break,
        }
    }

    if processed > 0 {
        chunk_requests.drain(..processed);
    }
    stats.deferred = chunk_requests.len();
    stats
}

fn drive_completed_chunk_intake_up_to(
    budget: usize,
    mut intake_one: impl FnMut() -> bool,
) -> usize {
    let mut completed = 0;
    while completed < budget && intake_one() {
        completed += 1;
    }
    completed
}

pub fn chunk_key_in_request_vd(
    player_wpos2d: Vec2<f32>,
    view_distance: u32,
    chunk_key: Vec2<i32>,
) -> bool {
    let chunk_center_wpos2d = (chunk_key.map(|e| e as f64 + 0.5)
        * TerrainChunkSize::RECT_SIZE.map(|e| e as f64))
    .as_::<f32>();
    let request_radius =
        (view_distance as f32 - 1.0 + 2.5 * 2.0_f32.sqrt()) * TerrainChunkSize::RECT_SIZE.x as f32;

    player_wpos2d.distance_squared(chunk_center_wpos2d) < request_radius.powi(2)
}

#[allow(dead_code)]
pub(crate) fn query_chunk_key_aabr_contains_chunk_key(
    query_chunk_key_aabr: Aabr<i32>,
    chunk_key: Vec2<i32>,
) -> bool {
    (query_chunk_key_aabr.min.x..=query_chunk_key_aabr.max.x).contains(&chunk_key.x)
        && (query_chunk_key_aabr.min.y..=query_chunk_key_aabr.max.y).contains(&chunk_key.y)
}

#[allow(dead_code)]
pub(crate) fn chunk_key_in_request_vd_and_query_domain(
    query_chunk_key_aabr: Aabr<i32>,
    player_wpos2d: Vec2<f32>,
    view_distance: u32,
    chunk_key: Vec2<i32>,
) -> bool {
    query_chunk_key_aabr_contains_chunk_key(query_chunk_key_aabr, chunk_key)
        && chunk_key_in_request_vd(player_wpos2d, view_distance, chunk_key)
}

fn canonical_request_distance_from_player_wpos2d(
    runtime_topology: &RuntimeTopologyDescriptor,
    player_wpos2d: Vec2<f32>,
    chunk_key: Vec2<i32>,
) -> Option<f32> {
    let chunk_size = TerrainChunkSize::RECT_SIZE.map(|e| e as f32);
    let query_dims = runtime_topology.query_chunk_dimensions();
    let query_min = runtime_topology.query_chunk_key_aabr.min;
    let raw_player_chunk = player_wpos2d
        .map(|coord| coord.floor() as i32)
        .wpos_to_cpos();
    let player_local_wpos = player_wpos2d.map2(chunk_size, |coord, size| coord.rem_euclid(size));
    let axis_delta = |player_coord: i32,
                      canonical_chunk_coord: i32,
                      axis_min: i32,
                      axis_len: i32,
                      wraps: bool| {
        if wraps {
            let canonical_player_coord = (player_coord - axis_min).rem_euclid(axis_len) + axis_min;
            shortest_wrapped_axis_delta(canonical_chunk_coord - canonical_player_coord, axis_len)
        } else {
            canonical_chunk_coord - player_coord
        }
    };
    let delta = Vec2::new(
        axis_delta(
            raw_player_chunk.x,
            chunk_key.x,
            query_min.x,
            query_dims.x,
            runtime_topology.wraps_x(),
        ),
        axis_delta(
            raw_player_chunk.y,
            chunk_key.y,
            query_min.y,
            query_dims.y,
            runtime_topology.wraps_y(),
        ),
    );
    let canonical_chunk_center_relative_wpos = delta.map(|coord| coord as f32 + 0.5) * chunk_size;

    Some(player_local_wpos.distance_squared(canonical_chunk_center_relative_wpos))
}

fn shortest_wrapped_axis_delta(delta: i32, axis_len: i32) -> i32 {
    let wrapped_positive = delta.rem_euclid(axis_len);
    let wrapped_negative = wrapped_positive - axis_len;
    let positive_abs = wrapped_positive.unsigned_abs();
    let negative_abs = wrapped_negative.unsigned_abs();
    if positive_abs < negative_abs {
        wrapped_positive
    } else if negative_abs < positive_abs {
        wrapped_negative
    } else if delta >= 0 {
        wrapped_positive
    } else {
        wrapped_negative
    }
}

pub(crate) fn canonical_request_chunk_key_in_vd(
    runtime_topology: &RuntimeTopologyDescriptor,
    player_wpos2d: Vec2<f32>,
    view_distance: u32,
    requested_chunk_key: Vec2<i32>,
) -> Option<Vec2<i32>> {
    let canonical_chunk_key = runtime_topology.normalize_query_chunk_key(requested_chunk_key)?;
    let request_radius =
        (view_distance as f32 - 1.0 + 2.5 * 2.0_f32.sqrt()) * TerrainChunkSize::RECT_SIZE.x as f32;

    canonical_request_distance_from_player_wpos2d(
        runtime_topology,
        player_wpos2d,
        canonical_chunk_key,
    )
    .filter(|distance_squared| *distance_squared < request_radius.powi(2))
    .map(|_| canonical_chunk_key)
}

pub(crate) fn canonical_request_chunk_keys_in_vd(
    runtime_topology: &RuntimeTopologyDescriptor,
    player_wpos2d: Vec2<f32>,
    view_distance: u32,
    candidate_chunk_keys: impl IntoIterator<Item = Vec2<i32>>,
) -> Vec<Vec2<i32>> {
    let mut seen = HashSet::new();
    let mut canonical_chunk_keys = Vec::new();

    for candidate_chunk_key in candidate_chunk_keys {
        if let Some(canonical_chunk_key) = canonical_request_chunk_key_in_vd(
            runtime_topology,
            player_wpos2d,
            view_distance,
            candidate_chunk_key,
        ) && seen.insert(canonical_chunk_key)
        {
            canonical_chunk_keys.push(canonical_chunk_key);
        }
    }

    canonical_chunk_keys
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LoadedChunkVisibility {
    player_chunk_pos: Vec2<i16>,
    loaded_vd_sqr: i32,
}

impl LoadedChunkVisibility {
    fn new(player_chunk_pos: Vec2<i16>, loaded_vd_sqr: i32) -> Self {
        Self {
            player_chunk_pos,
            loaded_vd_sqr,
        }
    }

    pub(crate) fn chunk_pos(self) -> Vec2<i16> { self.player_chunk_pos }

    pub(crate) fn loaded_vd_sqr(self) -> i32 { self.loaded_vd_sqr }

    pub fn contains_chunk(
        self,
        runtime_topology: &RuntimeTopologyDescriptor,
        chunk_pos: Vec2<i32>,
    ) -> bool {
        runtime_topology
            .query_chunk_key_delta(self.player_chunk_pos.as_::<i32>(), chunk_pos)
            .map(|delta| delta.magnitude_squared() <= self.loaded_vd_sqr)
            .unwrap_or(false)
    }
}

pub fn max_loaded_chunk_vd(max_view_distance: u32) -> i32 {
    convert_to_loaded_vd(u32::MAX, max_view_distance)
}

pub(crate) fn loaded_visibility_x_window<'a, T>(
    entries: &'a [T],
    runtime_topology: &RuntimeTopologyDescriptor,
    chunk_key: Vec2<i32>,
    max_loaded_chunk_vd: i32,
    loaded_visibility_of: impl Fn(&T) -> LoadedChunkVisibility,
) -> &'a [T] {
    if runtime_topology.wraps_x() {
        return entries;
    }

    let min_chunk_x = chunk_key.x - max_loaded_chunk_vd;
    let max_chunk_x = chunk_key.x + max_loaded_chunk_vd;
    let start = entries.partition_point(|entry| {
        i32::from(loaded_visibility_of(entry).chunk_pos().x) < min_chunk_x
    });
    let end = entries.partition_point(|entry| {
        i32::from(loaded_visibility_of(entry).chunk_pos().x) < max_chunk_x
    });
    &entries[start..end]
}

pub(crate) fn qualified_loaded_entries_for_chunk<'a, T>(
    entries: &'a [T],
    runtime_topology: &'a RuntimeTopologyDescriptor,
    chunk_key: Vec2<i32>,
    max_loaded_chunk_vd: i32,
    loaded_visibility_of: impl Fn(&T) -> LoadedChunkVisibility + Copy + 'a,
) -> impl Iterator<Item = &'a T> + 'a {
    loaded_visibility_x_window(
        entries,
        runtime_topology,
        chunk_key,
        max_loaded_chunk_vd,
        loaded_visibility_of,
    )
    .iter()
    .filter(move |entry| loaded_visibility_of(entry).contains_chunk(runtime_topology, chunk_key))
}

fn has_qualified_loaded_entry_for_chunk<'a, T>(
    entries: &'a [T],
    runtime_topology: &'a RuntimeTopologyDescriptor,
    chunk_key: Vec2<i32>,
    max_loaded_chunk_vd: i32,
    loaded_visibility_of: impl Fn(&T) -> LoadedChunkVisibility + Copy + 'a,
) -> bool {
    qualified_loaded_entries_for_chunk(
        entries,
        runtime_topology,
        chunk_key,
        max_loaded_chunk_vd,
        loaded_visibility_of,
    )
    .next()
    .is_some()
}

fn chunk_visible_to_any_loaded_presence(
    runtime_topology: &RuntimeTopologyDescriptor,
    chunk_key: Vec2<i32>,
    max_loaded_chunk_vd: i32,
    presences: &[LoadedChunkVisibility],
) -> bool {
    has_qualified_loaded_entry_for_chunk(
        presences,
        runtime_topology,
        chunk_key,
        max_loaded_chunk_vd,
        |loaded_visibility| *loaded_visibility,
    )
}

pub(crate) fn loaded_entities_for_chunk<'a, T: Copy>(
    entries: &'a [(LoadedChunkVisibility, T)],
    runtime_topology: &'a RuntimeTopologyDescriptor,
    chunk_key: Vec2<i32>,
    max_loaded_chunk_vd: i32,
) -> impl Iterator<Item = T> + 'a {
    qualified_loaded_entries_for_chunk(
        entries,
        runtime_topology,
        chunk_key,
        max_loaded_chunk_vd,
        |(loaded_visibility, _)| *loaded_visibility,
    )
    .map(|(_, value)| *value)
}

/// Returns: (loaded_chunk_visibility, entity, is_client)
fn prepare_for_vd_check(
    runtime_topology: &RuntimeTopologyDescriptor,
    max_view_distance: u32,
    entity: Entity,
    presence: &Presence,
    pos: &Pos,
    client: Option<u32>,
) -> Option<(LoadedChunkVisibility, Entity, bool)> {
    let is_client = client.is_some();
    let pos = pos.0;
    let vd = presence.terrain_view_distance.current();

    // NOTE: We use regular as casts rather than as_ because we want to saturate on
    // overflow.
    let player_pos = pos.map(|x| x as i32);
    let player_chunk_pos = TerrainGrid::chunk_key(player_pos);
    let canonical_player_chunk_pos =
        runtime_topology.normalize_query_chunk_key(player_chunk_pos)?;
    let player_vd = convert_to_loaded_vd(vd, max_view_distance);
    let world_aabr_in_chunks = runtime_topology.query_chunk_key_aabr;

    // We filter out positions that are *clearly* way out of range from
    // consideration. This is pretty easy to do, and means we don't have to
    // perform expensive overflow checks elsewhere (otherwise, a player
    // sufficiently far off the map could cause chunks they were nowhere near to
    // stay loaded, parallel universes style).
    //
    // One could also imagine snapping a player to the part of the map nearest to
    // them. We don't currently do this in case we rely elsewhere on players
    // always being near the chunks they're keeping loaded, but it would allow
    // us to use u32 exclusively so it's tempting.
    let player_aabr_in_chunks = Aabr {
        min: player_chunk_pos - player_vd,
        max: player_chunk_pos + player_vd,
    };

    (world_aabr_in_chunks.max.x >= player_aabr_in_chunks.min.x
        && world_aabr_in_chunks.min.x <= player_aabr_in_chunks.max.x
        && world_aabr_in_chunks.max.y >= player_aabr_in_chunks.min.y
        && world_aabr_in_chunks.min.y <= player_aabr_in_chunks.max.y)
        // The cast to i32 here is definitely safe thanks to MAX_VD limiting us to fit
        // within i32^2.
        //
        // The cast from each coordinate to i16 should also be correct here.  This is because valid
        // world chunk coordinates are no greater than 1 << 14 - 1; since we verified that the
        // player is within world bounds modulo player_vd, which is guaranteed to never let us
        // overflow an i16 when added to a u14, safety of the cast follows.
        .then(|| {
            (
                LoadedChunkVisibility::new(canonical_player_chunk_pos.as_::<i16>(), player_vd.pow(2)),
                entity,
                is_client,
            )
        })
}

pub fn prepare_player_presences<'a, P>(
    runtime_topology: &RuntimeTopologyDescriptor,
    max_view_distance: u32,
    entities: &Entities<'a>,
    positions: P,
    presences: &ReadStorage<'a, Presence>,
    clients: &ReadStorage<'a, Client>,
) -> (
    Vec<(LoadedChunkVisibility, Entity)>,
    Vec<LoadedChunkVisibility>,
)
where
    P: GenericReadStorage<Component = Pos> + Join<Type = &'a Pos>,
{
    // We start by collecting presences and positions from players, because they are
    // very sparse in the entity list and therefore iterating over them for each
    // chunk can be quite slow.
    let (mut presences_positions_entities, mut presences_positions): (Vec<_>, Vec<_>) =
        (entities, presences, positions, clients.mask().maybe())
            .join()
            .filter_map(|(entity, presence, position, client)| {
                prepare_for_vd_check(
                    runtime_topology,
                    max_view_distance,
                    entity,
                    presence,
                    position,
                    client,
                )
            })
            .partition_map(|(player_data, entity, is_client)| {
                // For chunks with clients, we need to record their entity, because they might
                // be used for insertion.  These elements fit in 8 bytes, so
                // this should be pretty cache-friendly.
                if is_client {
                    Either::Left((player_data, entity))
                } else {
                    // For chunks without clients, we only need to record the position and view
                    // distance.  These elements fit in 4 bytes, which is even cache-friendlier.
                    Either::Right(player_data)
                }
            });

    // We sort the presence lists by X position, so we can efficiently filter out
    // players nowhere near the chunk.  This is basically a poor substitute for
    // the effects of a proper KDTree, but a proper KDTree has too much overhead
    // to be worth using for such a short list (~ 1000 players at most).  We
    // also sort by y and reverse view distance; this will become important later.
    presences_positions_entities.sort_unstable_by_key(|&(loaded_visibility, _)| {
        (
            loaded_visibility.chunk_pos().x,
            loaded_visibility.chunk_pos().y,
            Reverse(loaded_visibility.loaded_vd_sqr()),
        )
    });
    presences_positions.sort_unstable_by_key(|&loaded_visibility| {
        (
            loaded_visibility.chunk_pos().x,
            loaded_visibility.chunk_pos().y,
            Reverse(loaded_visibility.loaded_vd_sqr()),
        )
    });
    // For the vast majority of chunks (present and pending ones), we'll only ever
    // need the position and view distance.  So we extend it with these from the
    // list of client chunks, and then do some further work to improve
    // performance (taking advantage of the fact that they don't require
    // entities).
    presences_positions.extend(
        presences_positions_entities
            .iter()
            .map(|&(loaded_visibility, _)| loaded_visibility),
    );
    // Since both lists were previously sorted, we use stable sort over unstable
    // sort, as it's faster in that case (theoretically a proper merge operation
    // would be ideal, but it's not worth pulling in a library for).
    presences_positions.sort_by_key(|&loaded_visibility| {
        (
            loaded_visibility.chunk_pos().x,
            loaded_visibility.chunk_pos().y,
            Reverse(loaded_visibility.loaded_vd_sqr()),
        )
    });
    // Now that the list is sorted, we deduplicate players in the same chunk (this
    // is why we need to sort y as well as x; dedup only works if the list is
    // sorted by the element we use to dedup).  Importantly, we can then use
    // only the *first* element as a substitute for all the players in the
    // chunk, because we *also* sorted from greatest to lowest view
    // distance, and dedup_by removes all but the first matching element.  In the
    // common case where a few chunks are very crowded, this further reduces the
    // work required per chunk.
    presences_positions.dedup_by_key(|loaded_visibility| loaded_visibility.chunk_pos());

    (presences_positions_entities, presences_positions)
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "worldgen")]
    use super::super::test_support::{ClientSupport, make_test_client};
    use super::{
        ChunkRequestSubmissionDecision, LoadedChunkVisibility, canonical_request_chunk_key_in_vd,
        canonical_request_chunk_keys_in_vd, chunk_generation_submit_budget,
        chunk_key_in_request_vd, chunk_key_in_request_vd_and_query_domain,
        chunk_visible_to_any_loaded_presence, completed_chunk_intake_budget, convert_to_loaded_vd,
        drive_completed_chunk_intake_up_to, has_qualified_loaded_entry_for_chunk,
        loaded_entities_for_chunk, loaded_visibility_x_window, max_loaded_chunk_vd,
        prepare_for_vd_check, prepare_player_presences, qualified_loaded_entries_for_chunk,
        query_chunk_key_aabr_contains_chunk_key, submit_chunk_requests_to_generation_up_to,
    };
    use crate::settings::{
        DEFAULT_CHUNK_GENERATION_SUBMIT_BUDGET_PER_TICK,
        DEFAULT_COMPLETED_CHUNK_INTAKE_BUDGET_PER_TICK,
    };
    use common::{
        ViewDistances,
        calendar::Calendar,
        character::CharacterId,
        comp::{ForceUpdate, Pos, Presence, PresenceKind, Waypoint},
        event::{CreateNpcEvent, CreateNpcGroupEvent, CreateSpecialEntityEvent, EventBus},
        generation::ChunkSupplement,
        resources::{Time, TimeOfDay},
        slowjob::SlowJobPool,
        spiral::Spiral2d,
        terrain::{CoordinateConversions, TerrainChunk, TerrainChunkSize, TerrainGrid},
        vol::RectVolSize,
    };
    use common_ecs::{SysMetrics, run_now};
    use common_net::msg::world_msg::{MissingWorldBoundsPolicy, RuntimeTopologyDescriptor};
    use common_state::TerrainChanges;
    use prometheus::Registry;
    use specs::{Builder, WorldExt};
    use std::{
        collections::HashSet,
        fs,
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
        thread,
        time::Duration,
    };
    use vek::*;

    fn runtime_topology_descriptor(topology_id: &str) -> RuntimeTopologyDescriptor {
        RuntimeTopologyDescriptor {
            topology_id: topology_id.to_owned(),
            query_chunk_key_aabr: Aabr {
                min: Vec2::zero(),
                max: Vec2::new(15, 15),
            },
            runtime_chunk_product_key_aabr: Aabr {
                min: Vec2::one(),
                max: Vec2::new(13, 13),
            },
            missing_world_bounds_policy: MissingWorldBoundsPolicy::BoundedOceanDefaultChunk,
        }
    }

    fn pos_in_chunk(chunk_key: Vec2<i32>) -> Pos {
        let chunk_size = TerrainChunkSize::RECT_SIZE.map(|e| e as f32);
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
    fn next_rtsim_data_dir() -> PathBuf {
        static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

        let dir = std::env::temp_dir().join(format!(
            "caldrayne-terrain-sys-intake-{}",
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).expect("create temp rtsim dir");
        dir
    }

    #[cfg(feature = "worldgen")]
    fn configure_slow_jobs() -> SlowJobPool {
        let threadpool = Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .build()
                .expect("rayon pool"),
        );
        let slow_jobs = SlowJobPool::new(1, 0, threadpool);
        slow_jobs.configure("CHUNK_GENERATOR", |limit| limit.max(1));
        slow_jobs.configure("CHUNK_DROP", |limit| limit.max(1));
        slow_jobs
    }

    #[cfg(feature = "worldgen")]
    fn wait_for_completed_chunk_result(ecs: &specs::World) {
        for _ in 0..100 {
            if ecs
                .read_resource::<crate::chunk_generator::ChunkGenerator>()
                .terrain_intake_queue_len()
                > 0
            {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }

        panic!("timed out waiting for completed chunk generation result");
    }

    #[cfg(feature = "worldgen")]
    fn wait_for_completed_chunk_results(ecs: &specs::World, expected: usize) {
        for _ in 0..1_000 {
            if ecs
                .read_resource::<crate::chunk_generator::ChunkGenerator>()
                .terrain_intake_queue_len()
                >= expected
            {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }

        panic!("timed out waiting for {expected} completed chunk generation results");
    }

    #[cfg(feature = "worldgen")]
    fn queue_generated_chunk_result_for_intake(
        ecs: &mut specs::World,
        key: Vec2<i32>,
        requester: Option<specs::Entity>,
    ) {
        let world = Arc::clone(&ecs.read_resource::<Arc<super::World>>());
        let index = (*ecs.read_resource::<super::IndexOwned>()).clone();
        let time_of_day = *ecs.read_resource::<TimeOfDay>();
        let calendar = (*ecs.read_resource::<Calendar>()).clone();
        let tick = ecs.read_resource::<crate::Tick>().0;
        let slow_jobs = ecs.read_resource::<SlowJobPool>();
        let rtsim = ecs.read_resource::<crate::rtsim::RtSim>();
        let mut chunk_generator = ecs.write_resource::<crate::chunk_generator::ChunkGenerator>();

        chunk_generator.generate_chunk(
            requester,
            key,
            &slow_jobs,
            world,
            &rtsim,
            index,
            (time_of_day, calendar),
            tick,
        );
    }

    #[cfg(feature = "worldgen")]
    fn queue_completed_chunk_result_for_intake_test(
        ecs: &mut specs::World,
        key: Vec2<i32>,
        requester: Option<specs::Entity>,
    ) {
        let tick = ecs.read_resource::<crate::Tick>().0;
        let mut chunk_generator = ecs.write_resource::<crate::chunk_generator::ChunkGenerator>();
        chunk_generator.queue_completed_chunk_for_test(
            key,
            Ok((TerrainChunk::water(0), ChunkSupplement::default())),
            tick,
        );
        let _ = requester;
    }

    #[cfg(feature = "worldgen")]
    fn make_terrain_sys_harness() -> (
        ClientSupport,
        specs::World,
        specs::Entity,
        Vec2<i32>,
        PathBuf,
    ) {
        let (client_support, client) = make_test_client();
        let data_dir = next_rtsim_data_dir();
        let settings = crate::settings::Settings::default();
        let (world, index) = super::World::empty();
        let world = Arc::new(world);
        let target_chunk = Vec2::zero();
        let registry = Registry::new();
        let lifecycle = crate::chunk_lifecycle::new_chunk_lifecycle_handle();
        let chunk_gen_metrics =
            crate::metrics::ChunkGenMetrics::new(&registry).expect("chunk gen metrics");
        let chunk_lifecycle_metrics =
            crate::metrics::ChunkLifecycleMetrics::new(&registry).expect("chunk lifecycle metrics");
        let network_request_metrics =
            crate::metrics::NetworkRequestMetrics::new(&registry).expect("network metrics");
        let rtsim = crate::rtsim::RtSim::new(
            &settings.world,
            index.as_index_ref(),
            world.as_ref(),
            data_dir.clone(),
        )
        .expect("rtsim for terrain sys test");

        let mut ecs = specs::World::new();
        ecs.register::<Pos>();
        ecs.register::<Presence>();
        ecs.register::<crate::client::Client>();
        ecs.register::<crate::presence::RepositionToFreeSpace>();
        ecs.register::<ForceUpdate>();
        ecs.register::<Waypoint>();

        ecs.insert(SysMetrics::default());
        ecs.insert(crate::Tick(0));
        ecs.insert(settings);
        ecs.insert(TimeOfDay::default());
        ecs.insert(Calendar::default());
        ecs.insert(Time::default());
        ecs.insert(configure_slow_jobs());
        ecs.insert(index);
        ecs.insert(Arc::clone(&world));
        ecs.insert(EventBus::<crate::chunk_serialize::ChunkSendEntry>::default());
        ecs.insert(EventBus::<CreateNpcEvent>::default());
        ecs.insert(EventBus::<CreateNpcGroupEvent>::default());
        ecs.insert(EventBus::<CreateSpecialEntityEvent>::default());
        ecs.insert(lifecycle.clone());
        ecs.insert(chunk_lifecycle_metrics);
        ecs.insert(network_request_metrics);
        ecs.insert(crate::chunk_generator::ChunkGenerator::new(
            chunk_gen_metrics,
            lifecycle,
        ));
        ecs.insert(
            TerrainGrid::new(
                world.sim().map_size_lg(),
                Arc::new(world.sim().generate_oob_chunk()),
            )
            .expect("terrain grid"),
        );
        ecs.insert(TerrainChanges::default());
        ecs.insert(Vec::<crate::ChunkRequest>::new());
        ecs.insert(rtsim);

        let player_entity = ecs
            .create_entity()
            .with(pos_in_chunk(target_chunk))
            .with(presence_with_vd(6, 1))
            .with(client)
            .build();

        (client_support, ecs, player_entity, target_chunk, data_dir)
    }

    fn legacy_chunk_key_in_request_vd(
        player_wpos2d: Vec2<f32>,
        view_distance: u32,
        chunk_key: Vec2<i32>,
    ) -> bool {
        player_wpos2d.map(f64::from).distance_squared(
            chunk_key.map(|e| e as f64 + 0.5) * TerrainChunkSize::RECT_SIZE.map(|e| e as f64),
        ) < ((view_distance as f64 - 1.0 + 2.5 * 2.0_f64.sqrt())
            * TerrainChunkSize::RECT_SIZE.x as f64)
            .powi(2)
    }

    #[test]
    fn chunk_key_in_request_vd_matches_legacy_formula() {
        let player_positions = [Vec2::new(0.0, 0.0), Vec2::new(17.25, -9.5)];
        let view_distances = [1, 2, 4, 8];
        let chunk_keys = [
            Vec2::new(-2, -1),
            Vec2::new(-1, 0),
            Vec2::new(0, 0),
            Vec2::new(1, 1),
            Vec2::new(3, -2),
            Vec2::new(7, 7),
        ];

        for player_wpos2d in player_positions {
            for view_distance in view_distances {
                for chunk_key in chunk_keys {
                    assert_eq!(
                        chunk_key_in_request_vd(player_wpos2d, view_distance, chunk_key),
                        legacy_chunk_key_in_request_vd(player_wpos2d, view_distance, chunk_key),
                        "player={player_wpos2d:?} vd={view_distance} chunk={chunk_key:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn current_min_vd_warmup_candidates_stay_within_request_boundary() {
        let player_wpos2d_cases = [Vec2::new(0, 0), Vec2::new(31, 31), Vec2::new(96, -15)];

        for player_wpos2d in player_wpos2d_cases {
            let player_chunk = player_wpos2d.wpos_to_cpos();
            let legacy_candidates = Spiral2d::new()
                .take((crate::MIN_VD as usize + 1).pow(2))
                .map(|rpos| player_chunk + rpos)
                .collect::<Vec<_>>();
            let helper_filtered = legacy_candidates
                .iter()
                .copied()
                .filter(|&chunk_key| {
                    chunk_key_in_request_vd(player_wpos2d.as_::<f32>(), crate::MIN_VD, chunk_key)
                })
                .collect::<Vec<_>>();

            assert_eq!(
                helper_filtered, legacy_candidates,
                "player={player_wpos2d:?}"
            );
        }
    }

    #[test]
    fn completed_chunk_intake_driver_stops_at_budget_boundary() {
        let mut remaining = std::collections::VecDeque::from([1_u8, 2, 3, 4]);

        let completed = drive_completed_chunk_intake_up_to(2, || remaining.pop_front().is_some());

        assert_eq!(completed, 2);
        assert_eq!(remaining.into_iter().collect::<Vec<_>>(), vec![3, 4]);
    }

    #[test]
    fn completed_chunk_intake_driver_stops_when_source_is_exhausted() {
        let mut remaining = std::collections::VecDeque::from([1_u8, 2, 3]);

        let completed = drive_completed_chunk_intake_up_to(8, || remaining.pop_front().is_some());

        assert_eq!(completed, 3);
        assert!(remaining.is_empty());
    }

    #[test]
    fn completed_chunk_intake_budget_reads_settings_and_falls_back_to_default() {
        let mut settings = crate::settings::Settings::default();
        settings.completed_chunk_intake_budget_per_tick = 7;
        assert_eq!(completed_chunk_intake_budget(&settings), 7);

        settings.completed_chunk_intake_budget_per_tick = 0;
        assert_eq!(
            completed_chunk_intake_budget(&settings),
            DEFAULT_COMPLETED_CHUNK_INTAKE_BUDGET_PER_TICK
        );
    }

    #[test]
    fn chunk_generation_submit_budget_defaults_to_unbounded_and_reads_settings() {
        let mut settings = crate::settings::Settings::default();
        assert_eq!(
            chunk_generation_submit_budget(&settings),
            DEFAULT_CHUNK_GENERATION_SUBMIT_BUDGET_PER_TICK
        );

        settings.chunk_generation_submit_budget_per_tick = Some(5);
        assert_eq!(chunk_generation_submit_budget(&settings), Some(5));
    }

    #[test]
    fn chunk_request_submission_preserves_fifo_tail_after_budget_boundary() {
        let mut world = specs::World::new();
        let entities = (0..3)
            .map(|_| world.create_entity().build())
            .collect::<Vec<_>>();
        let mut chunk_requests = vec![
            crate::ChunkRequest {
                entity: entities[0],
                key: Vec2::new(1, 0),
            },
            crate::ChunkRequest {
                entity: entities[1],
                key: Vec2::new(2, 0),
            },
            crate::ChunkRequest {
                entity: entities[2],
                key: Vec2::new(3, 0),
            },
        ];
        let mut submitted = Vec::new();

        let stats = submit_chunk_requests_to_generation_up_to(
            Some(2),
            &mut chunk_requests,
            |request, budget_available| {
                if budget_available {
                    submitted.push(request.key);
                    ChunkRequestSubmissionDecision::Admitted
                } else {
                    ChunkRequestSubmissionDecision::Deferred
                }
            },
        );

        assert_eq!(submitted, vec![Vec2::new(1, 0), Vec2::new(2, 0)]);
        assert_eq!(chunk_requests.len(), 1);
        assert_eq!(chunk_requests[0].key, Vec2::new(3, 0));
        assert_eq!(stats.admitted, 2);
        assert_eq!(stats.deferred, 1);
        assert_eq!(stats.coalesced_loaded, 0);
        assert_eq!(stats.coalesced_pending, 0);
    }

    #[test]
    fn chunk_request_submission_coalesces_loaded_and_pending_without_spending_budget() {
        let mut world = specs::World::new();
        let entities = (0..5)
            .map(|_| world.create_entity().build())
            .collect::<Vec<_>>();
        let mut chunk_requests = vec![
            crate::ChunkRequest {
                entity: entities[0],
                key: Vec2::new(1, 0),
            },
            crate::ChunkRequest {
                entity: entities[1],
                key: Vec2::new(2, 0),
            },
            crate::ChunkRequest {
                entity: entities[2],
                key: Vec2::new(3, 0),
            },
            crate::ChunkRequest {
                entity: entities[3],
                key: Vec2::new(3, 0),
            },
            crate::ChunkRequest {
                entity: entities[4],
                key: Vec2::new(4, 0),
            },
        ];
        let loaded = HashSet::from([Vec2::new(1, 0)]);
        let mut pending = HashSet::from([Vec2::new(2, 0)]);
        let mut submitted = Vec::new();

        let stats = submit_chunk_requests_to_generation_up_to(
            Some(1),
            &mut chunk_requests,
            |request, budget_available| {
                if loaded.contains(&request.key) {
                    return ChunkRequestSubmissionDecision::CoalescedLoaded;
                }
                if pending.contains(&request.key) {
                    return ChunkRequestSubmissionDecision::CoalescedPending;
                }
                if !budget_available {
                    return ChunkRequestSubmissionDecision::Deferred;
                }
                pending.insert(request.key);
                submitted.push(request.key);
                ChunkRequestSubmissionDecision::Admitted
            },
        );

        assert_eq!(submitted, vec![Vec2::new(3, 0)]);
        assert_eq!(chunk_requests.len(), 1);
        assert_eq!(chunk_requests[0].key, Vec2::new(4, 0));
        assert_eq!(stats.admitted, 1);
        assert_eq!(stats.deferred, 1);
        assert_eq!(stats.coalesced_loaded, 1);
        assert_eq!(stats.coalesced_pending, 2);
    }

    #[cfg(feature = "worldgen")]
    #[test]
    fn terrain_sys_intake_classifies_new_and_modified_chunks_and_emits_nearby_send_entries() {
        let (client_support, mut ecs, player_entity, target_chunk, data_dir) =
            make_terrain_sys_harness();

        queue_generated_chunk_result_for_intake(&mut ecs, target_chunk, Some(player_entity));
        wait_for_completed_chunk_result(&ecs);

        run_now::<super::Sys>(&ecs);

        {
            let terrain = ecs.read_resource::<TerrainGrid>();
            assert!(terrain.contains_key_real(target_chunk));
        }
        {
            let terrain_changes = ecs.read_resource::<TerrainChanges>();
            assert!(terrain_changes.new_chunks.contains(&target_chunk));
            assert!(!terrain_changes.modified_chunks.contains(&target_chunk));
        }
        {
            let chunk_generator = ecs.read_resource::<crate::chunk_generator::ChunkGenerator>();
            assert_eq!(chunk_generator.terrain_intake_queue_len(), 0);
            assert_eq!(chunk_generator.pending_chunks().count(), 0);
        }
        {
            let send_entries = ecs
                .read_resource::<EventBus<crate::chunk_serialize::ChunkSendEntry>>()
                .recv_all()
                .collect::<Vec<_>>();
            assert_eq!(send_entries, vec![crate::chunk_serialize::ChunkSendEntry {
                entity: player_entity,
                chunk_key: target_chunk,
            }]);
        }

        ecs.write_resource::<TerrainChanges>().clear();
        let cleared_send_entries = ecs
            .read_resource::<EventBus<crate::chunk_serialize::ChunkSendEntry>>()
            .recv_all()
            .count();
        assert_eq!(cleared_send_entries, 0);

        queue_generated_chunk_result_for_intake(&mut ecs, target_chunk, Some(player_entity));
        wait_for_completed_chunk_result(&ecs);

        run_now::<super::Sys>(&ecs);

        {
            let terrain = ecs.read_resource::<TerrainGrid>();
            assert!(terrain.contains_key_real(target_chunk));
        }
        {
            let terrain_changes = ecs.read_resource::<TerrainChanges>();
            assert!(!terrain_changes.new_chunks.contains(&target_chunk));
            assert!(terrain_changes.modified_chunks.contains(&target_chunk));
        }
        {
            let send_entries = ecs
                .read_resource::<EventBus<crate::chunk_serialize::ChunkSendEntry>>()
                .recv_all()
                .collect::<Vec<_>>();
            assert_eq!(send_entries, vec![crate::chunk_serialize::ChunkSendEntry {
                entity: player_entity,
                chunk_key: target_chunk,
            }]);
        }

        drop(ecs);
        drop(client_support);
        let _ = fs::remove_dir_all(data_dir);
    }

    #[cfg(feature = "worldgen")]
    #[test]
    fn terrain_sys_completed_chunk_intake_respects_budget_boundary() {
        let (client_support, mut ecs, player_entity, _target_chunk, data_dir) =
            make_terrain_sys_harness();
        let budget = DEFAULT_COMPLETED_CHUNK_INTAKE_BUDGET_PER_TICK;
        let total_chunks = budget + 1;
        let chunk_keys = (1..=13)
            .flat_map(|y| (1..=13).map(move |x| Vec2::new(x, y)))
            .take(total_chunks)
            .collect::<Vec<_>>();

        {
            let mut terrain = ecs.write_resource::<TerrainGrid>();
            for &chunk_key in &chunk_keys {
                terrain.insert(chunk_key, Arc::new(TerrainChunk::water(-1)));
            }
        }

        for &chunk_key in &chunk_keys {
            queue_completed_chunk_result_for_intake_test(&mut ecs, chunk_key, Some(player_entity));
        }
        wait_for_completed_chunk_results(&ecs, total_chunks);

        run_now::<super::Sys>(&ecs);

        {
            let terrain_changes = ecs.read_resource::<TerrainChanges>();
            assert_eq!(terrain_changes.new_chunks.len(), 0);
            assert_eq!(terrain_changes.modified_chunks.len(), budget);
        }
        {
            let chunk_generator = ecs.read_resource::<crate::chunk_generator::ChunkGenerator>();
            assert_eq!(chunk_generator.terrain_intake_queue_len(), 1);
            assert_eq!(chunk_generator.pending_chunks().count(), 1);
        }

        ecs.write_resource::<TerrainChanges>().clear();
        let _ = ecs
            .read_resource::<EventBus<crate::chunk_serialize::ChunkSendEntry>>()
            .recv_all()
            .count();

        run_now::<super::Sys>(&ecs);

        {
            let terrain_changes = ecs.read_resource::<TerrainChanges>();
            assert_eq!(terrain_changes.new_chunks.len(), 0);
            assert_eq!(terrain_changes.modified_chunks.len(), 1);
        }
        {
            let chunk_generator = ecs.read_resource::<crate::chunk_generator::ChunkGenerator>();
            assert_eq!(chunk_generator.terrain_intake_queue_len(), 0);
            assert_eq!(chunk_generator.pending_chunks().count(), 0);
        }

        drop(ecs);
        drop(client_support);
        let _ = fs::remove_dir_all(data_dir);
    }

    #[cfg(feature = "worldgen")]
    #[test]
    fn terrain_sys_completed_chunk_intake_uses_configured_budget() {
        let (client_support, mut ecs, player_entity, _target_chunk, data_dir) =
            make_terrain_sys_harness();
        let budget = 3;
        let total_chunks = budget + 1;
        let chunk_keys = (1..=4)
            .flat_map(|y| (1..=4).map(move |x| Vec2::new(x, y)))
            .take(total_chunks)
            .collect::<Vec<_>>();

        ecs.write_resource::<crate::settings::Settings>()
            .completed_chunk_intake_budget_per_tick = budget;

        {
            let mut terrain = ecs.write_resource::<TerrainGrid>();
            for &chunk_key in &chunk_keys {
                terrain.insert(chunk_key, Arc::new(TerrainChunk::water(-1)));
            }
        }

        for &chunk_key in &chunk_keys {
            queue_completed_chunk_result_for_intake_test(&mut ecs, chunk_key, Some(player_entity));
        }
        wait_for_completed_chunk_results(&ecs, total_chunks);

        run_now::<super::Sys>(&ecs);

        {
            let terrain_changes = ecs.read_resource::<TerrainChanges>();
            assert_eq!(terrain_changes.new_chunks.len(), 0);
            assert_eq!(terrain_changes.modified_chunks.len(), budget);
        }
        {
            let chunk_generator = ecs.read_resource::<crate::chunk_generator::ChunkGenerator>();
            assert_eq!(chunk_generator.terrain_intake_queue_len(), 1);
            assert_eq!(chunk_generator.pending_chunks().count(), 1);
        }

        ecs.write_resource::<TerrainChanges>().clear();
        let _ = ecs
            .read_resource::<EventBus<crate::chunk_serialize::ChunkSendEntry>>()
            .recv_all()
            .count();

        run_now::<super::Sys>(&ecs);

        {
            let terrain_changes = ecs.read_resource::<TerrainChanges>();
            assert_eq!(terrain_changes.new_chunks.len(), 0);
            assert_eq!(terrain_changes.modified_chunks.len(), 1);
        }
        {
            let chunk_generator = ecs.read_resource::<crate::chunk_generator::ChunkGenerator>();
            assert_eq!(chunk_generator.terrain_intake_queue_len(), 0);
            assert_eq!(chunk_generator.pending_chunks().count(), 0);
        }

        drop(ecs);
        drop(client_support);
        let _ = fs::remove_dir_all(data_dir);
    }

    #[cfg(feature = "worldgen")]
    #[test]
    fn terrain_sys_chunk_generation_submission_uses_configured_budget() {
        let (client_support, ecs, player_entity, _target_chunk, data_dir) =
            make_terrain_sys_harness();
        let budget = 2;
        let query_chunk_key_aabr = {
            let world = ecs.read_resource::<Arc<super::World>>();
            world.query_chunk_key_aabr()
        };
        assert!(query_chunk_key_aabr.max.x > query_chunk_key_aabr.min.x);
        assert!(query_chunk_key_aabr.max.y > query_chunk_key_aabr.min.y);
        let chunk_keys = [
            query_chunk_key_aabr.min,
            Vec2::new(query_chunk_key_aabr.max.x, query_chunk_key_aabr.min.y),
            Vec2::new(query_chunk_key_aabr.min.x, query_chunk_key_aabr.max.y),
        ];

        ecs.write_resource::<crate::settings::Settings>()
            .chunk_generation_submit_budget_per_tick = Some(budget);
        {
            let mut chunk_requests = ecs.write_resource::<Vec<crate::ChunkRequest>>();
            for chunk_key in chunk_keys {
                chunk_requests.push(crate::ChunkRequest {
                    entity: player_entity,
                    key: chunk_key,
                });
            }
        }

        run_now::<super::Sys>(&ecs);

        {
            let chunk_generator = ecs.read_resource::<crate::chunk_generator::ChunkGenerator>();
            assert_eq!(chunk_generator.requested_count(), budget as u64);
        }
        {
            let chunk_requests = ecs.read_resource::<Vec<crate::ChunkRequest>>();
            assert_eq!(chunk_requests.len(), 1);
            assert_eq!(chunk_requests[0].key, chunk_keys[2]);
        }
        {
            let network_metrics = ecs.read_resource::<crate::metrics::NetworkRequestMetrics>();
            assert_eq!(network_metrics.chunks_generation_budget_deferred.get(), 1);
        }

        drop(ecs);
        drop(client_support);
        let _ = fs::remove_dir_all(data_dir);
    }

    #[test]
    fn query_chunk_key_aabr_contains_chunk_key_matches_bounded_bounds() {
        let query_chunk_key_aabr = Aabr {
            min: Vec2::zero(),
            max: Vec2::new(15, 15),
        };

        assert!(query_chunk_key_aabr_contains_chunk_key(
            query_chunk_key_aabr,
            Vec2::new(0, 0),
        ));
        assert!(query_chunk_key_aabr_contains_chunk_key(
            query_chunk_key_aabr,
            Vec2::new(15, 15),
        ));
        assert!(!query_chunk_key_aabr_contains_chunk_key(
            query_chunk_key_aabr,
            Vec2::new(-1, 0),
        ));
        assert!(!query_chunk_key_aabr_contains_chunk_key(
            query_chunk_key_aabr,
            Vec2::new(16, 15),
        ));
    }

    #[test]
    fn request_domain_helper_rejects_query_invalid_min_vd_edge_candidates() {
        let query_chunk_key_aabr = Aabr {
            min: Vec2::zero(),
            max: Vec2::new(15, 15),
        };
        let player_wpos2d = Vec2::new(0.0, 0.0);
        let player_chunk = player_wpos2d.as_::<i32>().wpos_to_cpos();
        let legacy_candidates = Spiral2d::new()
            .take((crate::MIN_VD as usize + 1).pow(2))
            .map(|rpos| player_chunk + rpos)
            .collect::<Vec<_>>();

        assert!(legacy_candidates.iter().any(|&chunk_key| {
            !query_chunk_key_aabr_contains_chunk_key(query_chunk_key_aabr, chunk_key)
        }));

        let gated_candidates = legacy_candidates
            .iter()
            .copied()
            .filter(|&chunk_key| {
                chunk_key_in_request_vd_and_query_domain(
                    query_chunk_key_aabr,
                    player_wpos2d,
                    crate::MIN_VD,
                    chunk_key,
                )
            })
            .collect::<Vec<_>>();

        assert!(!gated_candidates.is_empty());
        assert!(gated_candidates.iter().all(|&chunk_key| {
            query_chunk_key_aabr_contains_chunk_key(query_chunk_key_aabr, chunk_key)
        }));
    }

    #[test]
    fn canonical_request_chunk_key_in_vd_matches_bounded_request_gate() {
        let runtime_topology = runtime_topology_descriptor("bounded_plane_v1");
        let player_positions = [Vec2::new(0.0, 0.0), Vec2::new(17.25, -9.5)];
        let view_distances = [1, 2, 4, 8];
        let chunk_keys = [
            Vec2::new(-2, -1),
            Vec2::new(-1, 0),
            Vec2::new(0, 0),
            Vec2::new(1, 1),
            Vec2::new(3, -2),
            Vec2::new(7, 7),
            Vec2::new(15, 15),
            Vec2::new(16, 0),
        ];

        for player_wpos2d in player_positions {
            for view_distance in view_distances {
                for chunk_key in chunk_keys {
                    let legacy = query_chunk_key_aabr_contains_chunk_key(
                        runtime_topology.query_chunk_key_aabr,
                        chunk_key,
                    )
                    .then_some(chunk_key)
                    .filter(|&chunk_key| {
                        chunk_key_in_request_vd(player_wpos2d, view_distance, chunk_key)
                    });
                    assert_eq!(
                        canonical_request_chunk_key_in_vd(
                            &runtime_topology,
                            player_wpos2d,
                            view_distance,
                            chunk_key,
                        ),
                        legacy,
                        "player={player_wpos2d:?} vd={view_distance} chunk={chunk_key:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn canonical_request_chunk_key_in_vd_normalizes_wrap_equivalents() {
        let toroidal = runtime_topology_descriptor("wrap_toroidal_exp_v1");
        let cylindrical = runtime_topology_descriptor("wrap_cylindrical_exp_v1");
        let player_wpos2d = Vec2::new(0.0, 0.0);

        assert_eq!(
            canonical_request_chunk_key_in_vd(&toroidal, player_wpos2d, 1, Vec2::new(-1, 0)),
            Some(Vec2::new(15, 0))
        );
        assert_eq!(
            canonical_request_chunk_key_in_vd(&toroidal, player_wpos2d, 1, Vec2::new(15, 0)),
            Some(Vec2::new(15, 0))
        );
        assert_eq!(
            canonical_request_chunk_key_in_vd(&toroidal, player_wpos2d, 1, Vec2::new(0, -1)),
            Some(Vec2::new(0, 15))
        );
        assert_eq!(
            canonical_request_chunk_key_in_vd(&cylindrical, player_wpos2d, 1, Vec2::new(-1, 0)),
            Some(Vec2::new(15, 0))
        );
        assert_eq!(
            canonical_request_chunk_key_in_vd(&cylindrical, player_wpos2d, 1, Vec2::new(0, -1)),
            None
        );
    }

    #[test]
    fn canonical_request_chunk_keys_in_vd_matches_bounded_filter_order() {
        let bounded = runtime_topology_descriptor("bounded_plane_v1");
        let player_wpos2d = Vec2::new(17.25, -9.5);
        let candidates = [
            Vec2::new(-2, -1),
            Vec2::new(-1, 0),
            Vec2::new(0, 0),
            Vec2::new(1, 1),
            Vec2::new(3, -2),
            Vec2::new(7, 7),
            Vec2::new(15, 15),
            Vec2::new(16, 0),
        ];

        let expected = candidates
            .into_iter()
            .filter_map(|chunk_key| {
                query_chunk_key_aabr_contains_chunk_key(bounded.query_chunk_key_aabr, chunk_key)
                    .then_some(chunk_key)
                    .filter(|&chunk_key| chunk_key_in_request_vd(player_wpos2d, 4, chunk_key))
            })
            .collect::<Vec<_>>();

        assert_eq!(
            canonical_request_chunk_keys_in_vd(&bounded, player_wpos2d, 4, candidates),
            expected
        );
    }

    #[test]
    fn canonical_request_chunk_keys_in_vd_dedupes_wrap_equivalent_candidates() {
        let toroidal = runtime_topology_descriptor("wrap_toroidal_exp_v1");
        let player_wpos2d = Vec2::new(0.0, 0.0);
        let candidates = [
            Vec2::new(-1, 0),
            Vec2::new(15, 0),
            Vec2::new(0, -1),
            Vec2::new(0, 15),
            Vec2::new(0, 0),
        ];

        assert_eq!(
            canonical_request_chunk_keys_in_vd(&toroidal, player_wpos2d, 1, candidates),
            vec![Vec2::new(15, 0), Vec2::new(0, 15), Vec2::new(0, 0)]
        );
    }

    #[test]
    fn loaded_chunk_visibility_contains_chunk_matches_legacy_formula() {
        let runtime_topology = runtime_topology_descriptor("bounded_plane_v1");
        let visibilities = [
            LoadedChunkVisibility::new(Vec2::new(0, 0), 0),
            LoadedChunkVisibility::new(Vec2::new(3, 2), 4),
            LoadedChunkVisibility::new(Vec2::new(7, 9), 49),
        ];
        let chunk_keys = [
            Vec2::new(0, 0),
            Vec2::new(1, 0),
            Vec2::new(4, 2),
            Vec2::new(3, 7),
            Vec2::new(10, 11),
        ];

        for visibility in visibilities {
            for chunk_key in chunk_keys {
                let adjusted_dist_sqr =
                    (visibility.chunk_pos().as_::<i32>() - chunk_key).magnitude_squared();
                assert_eq!(
                    visibility.contains_chunk(&runtime_topology, chunk_key),
                    adjusted_dist_sqr <= visibility.loaded_vd_sqr(),
                    "visibility={visibility:?} chunk={chunk_key:?}"
                );
            }
        }
    }

    #[test]
    fn loaded_visibility_x_window_matches_legacy_partition_window() {
        let runtime_topology = runtime_topology_descriptor("bounded_plane_v1");
        let visibilities = vec![
            LoadedChunkVisibility::new(Vec2::new(0, 0), 4),
            LoadedChunkVisibility::new(Vec2::new(2, 1), 9),
            LoadedChunkVisibility::new(Vec2::new(5, 2), 16),
            LoadedChunkVisibility::new(Vec2::new(8, 3), 25),
            LoadedChunkVisibility::new(Vec2::new(11, 0), 36),
            LoadedChunkVisibility::new(Vec2::new(14, 1), 49),
        ];
        let chunk_key = Vec2::new(8, 8);
        let max_loaded_chunk_vd = 4;

        let min_chunk_x = chunk_key.x - max_loaded_chunk_vd;
        let max_chunk_x = chunk_key.x + max_loaded_chunk_vd;
        let start = visibilities.partition_point(|loaded_visibility| {
            i32::from(loaded_visibility.chunk_pos().x) < min_chunk_x
        });
        let end = visibilities.partition_point(|loaded_visibility| {
            i32::from(loaded_visibility.chunk_pos().x) < max_chunk_x
        });

        assert_eq!(
            loaded_visibility_x_window(
                &visibilities,
                &runtime_topology,
                chunk_key,
                max_loaded_chunk_vd,
                |loaded_visibility| *loaded_visibility,
            ),
            &visibilities[start..end],
        );
    }

    #[test]
    fn qualified_loaded_entries_for_chunk_matches_legacy_filter() {
        let runtime_topology = runtime_topology_descriptor("bounded_plane_v1");
        let entries = vec![
            (LoadedChunkVisibility::new(Vec2::new(0, 0), 4), 1_u8),
            (LoadedChunkVisibility::new(Vec2::new(2, 1), 9), 2_u8),
            (LoadedChunkVisibility::new(Vec2::new(5, 2), 16), 3_u8),
            (LoadedChunkVisibility::new(Vec2::new(8, 3), 25), 4_u8),
            (LoadedChunkVisibility::new(Vec2::new(11, 0), 36), 5_u8),
            (LoadedChunkVisibility::new(Vec2::new(14, 1), 49), 6_u8),
        ];
        let chunk_key = Vec2::new(8, 8);
        let max_loaded_chunk_vd = 4;

        let min_chunk_x = chunk_key.x - max_loaded_chunk_vd;
        let max_chunk_x = chunk_key.x + max_loaded_chunk_vd;
        let start = entries.partition_point(|(loaded_visibility, _)| {
            i32::from(loaded_visibility.chunk_pos().x) < min_chunk_x
        });
        let end = entries.partition_point(|(loaded_visibility, _)| {
            i32::from(loaded_visibility.chunk_pos().x) < max_chunk_x
        });
        let expected = entries[start..end]
            .iter()
            .filter(|(loaded_visibility, _)| {
                loaded_visibility.contains_chunk(&runtime_topology, chunk_key)
            })
            .copied()
            .collect::<Vec<_>>();

        assert_eq!(
            qualified_loaded_entries_for_chunk(
                &entries,
                &runtime_topology,
                chunk_key,
                max_loaded_chunk_vd,
                |(loaded_visibility, _)| *loaded_visibility,
            )
            .copied()
            .collect::<Vec<_>>(),
            expected,
        );
    }

    #[test]
    fn has_qualified_loaded_entry_for_chunk_matches_legacy_any() {
        let runtime_topology = runtime_topology_descriptor("bounded_plane_v1");
        let entries = vec![
            (LoadedChunkVisibility::new(Vec2::new(0, 0), 4), 1_u8),
            (LoadedChunkVisibility::new(Vec2::new(2, 1), 9), 2_u8),
            (LoadedChunkVisibility::new(Vec2::new(5, 2), 16), 3_u8),
            (LoadedChunkVisibility::new(Vec2::new(8, 3), 25), 4_u8),
            (LoadedChunkVisibility::new(Vec2::new(11, 0), 36), 5_u8),
            (LoadedChunkVisibility::new(Vec2::new(14, 1), 49), 6_u8),
        ];
        let chunk_key = Vec2::new(8, 8);
        let max_loaded_chunk_vd = 4;

        let min_chunk_x = chunk_key.x - max_loaded_chunk_vd;
        let max_chunk_x = chunk_key.x + max_loaded_chunk_vd;
        let start = entries.partition_point(|(loaded_visibility, _)| {
            i32::from(loaded_visibility.chunk_pos().x) < min_chunk_x
        });
        let end = entries.partition_point(|(loaded_visibility, _)| {
            i32::from(loaded_visibility.chunk_pos().x) < max_chunk_x
        });
        let expected = entries[start..end].iter().any(|(loaded_visibility, _)| {
            loaded_visibility.contains_chunk(&runtime_topology, chunk_key)
        });

        assert_eq!(
            has_qualified_loaded_entry_for_chunk(
                &entries,
                &runtime_topology,
                chunk_key,
                max_loaded_chunk_vd,
                |(loaded_visibility, _)| *loaded_visibility,
            ),
            expected,
        );
    }

    #[test]
    fn loaded_entities_for_chunk_matches_legacy_filter_map() {
        let runtime_topology = runtime_topology_descriptor("bounded_plane_v1");
        let entries = vec![
            (LoadedChunkVisibility::new(Vec2::new(0, 0), 4), 1_u8),
            (LoadedChunkVisibility::new(Vec2::new(2, 1), 9), 2_u8),
            (LoadedChunkVisibility::new(Vec2::new(5, 2), 16), 3_u8),
            (LoadedChunkVisibility::new(Vec2::new(8, 3), 25), 4_u8),
            (LoadedChunkVisibility::new(Vec2::new(11, 0), 36), 5_u8),
            (LoadedChunkVisibility::new(Vec2::new(14, 1), 49), 6_u8),
        ];
        let chunk_key = Vec2::new(8, 8);
        let max_loaded_chunk_vd = 4;

        let min_chunk_x = chunk_key.x - max_loaded_chunk_vd;
        let max_chunk_x = chunk_key.x + max_loaded_chunk_vd;
        let start = entries.partition_point(|(loaded_visibility, _)| {
            i32::from(loaded_visibility.chunk_pos().x) < min_chunk_x
        });
        let end = entries.partition_point(|(loaded_visibility, _)| {
            i32::from(loaded_visibility.chunk_pos().x) < max_chunk_x
        });
        let expected = entries[start..end]
            .iter()
            .filter_map(|(loaded_visibility, entity)| {
                loaded_visibility
                    .contains_chunk(&runtime_topology, chunk_key)
                    .then_some(*entity)
            })
            .collect::<Vec<_>>();

        assert_eq!(
            loaded_entities_for_chunk(&entries, &runtime_topology, chunk_key, max_loaded_chunk_vd)
                .collect::<Vec<_>>(),
            expected,
        );
    }

    #[test]
    fn loaded_chunk_visibility_contains_chunk_wraps_toroidal_equivalents() {
        let runtime_topology = runtime_topology_descriptor("wrap_toroidal_exp_v1");
        let visibility = LoadedChunkVisibility::new(Vec2::new(0, 0), 1);

        assert!(visibility.contains_chunk(&runtime_topology, Vec2::new(15, 0)));
        assert!(visibility.contains_chunk(&runtime_topology, Vec2::new(0, 15)));
        assert!(!visibility.contains_chunk(&runtime_topology, Vec2::new(14, 0)));
        assert!(!visibility.contains_chunk(&runtime_topology, Vec2::new(0, 14)));
    }

    #[test]
    fn loaded_chunk_visibility_contains_chunk_respects_cylindrical_axes() {
        let runtime_topology = runtime_topology_descriptor("wrap_cylindrical_exp_v1");
        let visibility = LoadedChunkVisibility::new(Vec2::new(0, 0), 1);

        assert!(visibility.contains_chunk(&runtime_topology, Vec2::new(15, 0)));
        assert!(!visibility.contains_chunk(&runtime_topology, Vec2::new(0, 15)));
    }

    #[test]
    fn loaded_visibility_x_window_returns_full_slice_when_x_wraps() {
        let runtime_topology = runtime_topology_descriptor("wrap_toroidal_exp_v1");
        let visibilities = vec![
            LoadedChunkVisibility::new(Vec2::new(0, 0), 4),
            LoadedChunkVisibility::new(Vec2::new(3, 1), 4),
            LoadedChunkVisibility::new(Vec2::new(9, -2), 4),
            LoadedChunkVisibility::new(Vec2::new(15, 0), 4),
        ];

        assert_eq!(
            loaded_visibility_x_window(
                &visibilities,
                &runtime_topology,
                Vec2::new(0, 0),
                2,
                |loaded_visibility| *loaded_visibility,
            ),
            visibilities.as_slice(),
        );
    }

    #[test]
    fn loaded_entities_for_chunk_includes_wrap_adjacent_presences() {
        let runtime_topology = runtime_topology_descriptor("wrap_toroidal_exp_v1");
        let entries = vec![
            (LoadedChunkVisibility::new(Vec2::new(0, 0), 1), 1_u8),
            (LoadedChunkVisibility::new(Vec2::new(15, 15), 2), 2_u8),
            (LoadedChunkVisibility::new(Vec2::new(7, 7), 1), 3_u8),
        ];

        assert_eq!(
            loaded_entities_for_chunk(&entries, &runtime_topology, Vec2::new(15, 0), 1)
                .collect::<Vec<_>>(),
            vec![1, 2],
        );
    }

    #[test]
    fn prepare_player_presences_canonicalizes_and_dedupes_wrapped_chunks() {
        let max_view_distance = 8;
        let cases = [
            (
                "wrap_toroidal_exp_v1",
                vec![
                    (Vec2::new(-1, 0), 6_u32, 1_i64),
                    (Vec2::new(15, 0), 8_u32, 2_i64),
                ],
                vec![LoadedChunkVisibility::new(
                    Vec2::new(15, 0),
                    convert_to_loaded_vd(8, max_view_distance).pow(2),
                )],
            ),
            (
                "wrap_cylindrical_exp_v1",
                vec![
                    (Vec2::new(-1, 0), 6_u32, 3_i64),
                    (Vec2::new(15, 0), 8_u32, 4_i64),
                    (Vec2::new(0, -1), 7_u32, 5_i64),
                ],
                vec![LoadedChunkVisibility::new(
                    Vec2::new(15, 0),
                    convert_to_loaded_vd(8, max_view_distance).pow(2),
                )],
            ),
        ];

        for (topology_id, presences_to_spawn, expected_presences) in cases {
            let runtime_topology = runtime_topology_descriptor(topology_id);
            let mut world = specs::World::new();
            world.register::<Pos>();
            world.register::<Presence>();
            world.register::<crate::client::Client>();

            for (chunk_key, terrain_vd, character_id) in presences_to_spawn {
                world
                    .create_entity()
                    .with(pos_in_chunk(chunk_key))
                    .with(presence_with_vd(terrain_vd, character_id))
                    .build();
            }

            let entities = world.entities();
            let positions = world.read_storage::<Pos>();
            let presences = world.read_storage::<Presence>();
            let clients = world.read_storage::<crate::client::Client>();

            let (client_entries, prepared_presences) = prepare_player_presences(
                &runtime_topology,
                max_view_distance,
                &entities,
                &positions,
                &presences,
                &clients,
            );

            assert!(
                client_entries.is_empty(),
                "topology={topology_id} should not synthesize client entries without Client \
                 components"
            );
            assert_eq!(
                prepared_presences, expected_presences,
                "topology={topology_id}"
            );
        }
    }

    #[test]
    fn focused_seam_audit_visibility_contract_consistent_across_topologies() {
        let max_view_distance = 8;

        #[derive(Clone, Copy)]
        struct Case {
            topology_id: &'static str,
            raw_player_chunk: Vec2<i32>,
            target_chunk: Vec2<i32>,
            expect_canonical_player_chunk: Option<Vec2<i16>>,
        }

        let cases = [
            Case {
                topology_id: "bounded_plane_v1",
                raw_player_chunk: Vec2::new(0, 0),
                target_chunk: Vec2::new(0, 0),
                expect_canonical_player_chunk: Some(Vec2::new(0, 0)),
            },
            Case {
                topology_id: "wrap_toroidal_exp_v1",
                raw_player_chunk: Vec2::new(-1, 0),
                target_chunk: Vec2::new(15, 0),
                expect_canonical_player_chunk: Some(Vec2::new(15, 0)),
            },
            Case {
                topology_id: "wrap_toroidal_exp_v1",
                raw_player_chunk: Vec2::new(0, -1),
                target_chunk: Vec2::new(0, 15),
                expect_canonical_player_chunk: Some(Vec2::new(0, 15)),
            },
            Case {
                topology_id: "wrap_cylindrical_exp_v1",
                raw_player_chunk: Vec2::new(-1, 0),
                target_chunk: Vec2::new(15, 0),
                expect_canonical_player_chunk: Some(Vec2::new(15, 0)),
            },
            Case {
                topology_id: "wrap_cylindrical_exp_v1",
                raw_player_chunk: Vec2::new(0, -1),
                target_chunk: Vec2::new(0, 15),
                expect_canonical_player_chunk: None,
            },
        ];

        let mut world = specs::World::new();
        let entity = world.create_entity().build();

        for case in cases {
            let runtime_topology = runtime_topology_descriptor(case.topology_id);
            let presence = presence_with_vd(6, 99);
            let pos = pos_in_chunk(case.raw_player_chunk);
            let prepared = prepare_for_vd_check(
                &runtime_topology,
                max_view_distance,
                entity,
                &presence,
                &pos,
                Some(0),
            );

            match case.expect_canonical_player_chunk {
                Some(expected_chunk_pos) => {
                    let (loaded_visibility, prepared_entity, is_client) =
                        prepared.expect("presence should survive seam admission");
                    let max_loaded_chunk_vd = max_loaded_chunk_vd(max_view_distance);
                    let presences = [loaded_visibility];
                    let loaded_entities = [(loaded_visibility, 7_u8)];

                    assert_eq!(
                        loaded_visibility.chunk_pos(),
                        expected_chunk_pos,
                        "topology={} raw_player_chunk={:?}",
                        case.topology_id,
                        case.raw_player_chunk
                    );
                    assert_eq!(prepared_entity, entity);
                    assert!(is_client);
                    assert!(
                        chunk_visible_to_any_loaded_presence(
                            &runtime_topology,
                            case.target_chunk,
                            max_loaded_chunk_vd,
                            &presences,
                        ),
                        "topology={} target_chunk={:?} should stay visible to unload path",
                        case.topology_id,
                        case.target_chunk
                    );
                    assert_eq!(
                        loaded_entities_for_chunk(
                            &loaded_entities,
                            &runtime_topology,
                            case.target_chunk,
                            max_loaded_chunk_vd,
                        )
                        .collect::<Vec<_>>(),
                        vec![7],
                        "topology={} target_chunk={:?} should stay deliverable to terrain_sync \
                         path",
                        case.topology_id,
                        case.target_chunk
                    );
                },
                None => {
                    assert!(
                        prepared.is_none(),
                        "topology={} raw_player_chunk={:?} should stay query-invalid on its \
                         non-wrapped axis",
                        case.topology_id,
                        case.raw_player_chunk
                    );
                },
            }
        }
    }
}
