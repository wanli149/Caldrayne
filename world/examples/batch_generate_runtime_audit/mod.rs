use std::{collections::BTreeMap, fs, path::PathBuf};

use common::{
    generation::{EntityInfo, EntitySpawn},
    terrain::{Block, BlockKind, SpriteKind, TerrainChunk, TerrainChunkSize},
    vol::{ReadVol, RectVolSize, WriteVol},
};
use serde::{Deserialize, Serialize};
use vek::{Rgb, Vec2, Vec3};

const FIXED_OVERLAY_FIXTURE_PATH: &str =
    "server/tests/data/terrain_overlay/fixed_overlay_fixture_v1.ron";

#[derive(Serialize)]
pub struct AuditChunkRuntimeMatrixFile {
    pub run_id: String,
    pub seed: u32,
    pub gen_opts: super::GenOpts,
    pub recipe: super::AuditRecipeSummary,
    pub sample_chunks: usize,
    pub runtime_audit_mode: String,
    pub strict_determinism: bool,
    pub fixed_overlay_fixture: AuditFixedOverlayFixtureSummary,
    pub sampled_chunks: Vec<AuditChunkRuntimeSample>,
}

#[derive(Serialize)]
pub struct AuditFixedOverlayFixtureSummary {
    pub fixture_id: String,
    pub contract_path: String,
    pub operation_count: usize,
}

#[derive(Serialize)]
pub struct AuditChunkRuntimeSample {
    pub chunk_pos: [i32; 2],
    pub raw_worldgen: AuditChunkRuntimeVariant,
    pub empty_overlay: AuditChunkRuntimeVariant,
    pub fixed_overlay: AuditChunkRuntimeVariant,
}

#[derive(Serialize)]
pub struct AuditChunkRuntimeVariant {
    pub overlay_mode: String,
    pub overlay_blocks_applied: usize,
    pub overlay_operations_skipped: usize,
    pub min_z: i32,
    pub max_z: i32,
    pub sub_chunks: usize,
    pub block_total: u64,
    pub non_air_blocks: u64,
    pub sprite_total: u64,
    pub block_kind_counts: BTreeMap<String, u64>,
    pub sprite_kind_counts: BTreeMap<String, u64>,
    pub entity_signature_count: usize,
    pub entity_signatures: Vec<String>,
}

#[derive(Deserialize)]
struct FixedOverlayFixture {
    fixture_id: String,
    operations: Vec<FixedOverlayOperation>,
}

#[derive(Deserialize)]
struct FixedOverlayOperation {
    rel_xy: [u32; 2],
    target: FixedOverlayTarget,
    block: FixedOverlayBlock,
}

#[derive(Deserialize)]
enum FixedOverlayTarget {
    TopMostNonAir,
}

#[derive(Deserialize)]
enum FixedOverlayBlock {
    Solid { kind: BlockKind, color: [u8; 3] },
    Air { sprite: SpriteKind },
    Water { sprite: SpriteKind },
}

#[derive(Default)]
struct OverlayApplySummary {
    applied: usize,
    skipped: usize,
}

pub fn build_runtime_chunk_matrix(
    run_id: &str,
    world: &super::World,
    index_ref: super::IndexRef,
    gen_opts: &super::GenOpts,
    sample_chunks: usize,
) -> AuditChunkRuntimeMatrixFile {
    let fixture = load_fixed_overlay_fixture();
    let sampled_chunks = super::sampled_chunk_positions(world.sim().get_size(), sample_chunks)
        .into_iter()
        .map(|chunk_pos| build_runtime_chunk_sample(world, index_ref, chunk_pos, &fixture))
        .collect();

    AuditChunkRuntimeMatrixFile {
        run_id: run_id.to_owned(),
        seed: world.sim().seed,
        gen_opts: gen_opts.clone(),
        recipe: super::build_recipe_summary(world),
        sample_chunks,
        runtime_audit_mode: "sampled_runtime_overlay_matrix_v1".to_owned(),
        strict_determinism: true,
        fixed_overlay_fixture: AuditFixedOverlayFixtureSummary {
            fixture_id: fixture.fixture_id.clone(),
            contract_path: FIXED_OVERLAY_FIXTURE_PATH.to_owned(),
            operation_count: fixture.operations.len(),
        },
        sampled_chunks,
    }
}

fn build_runtime_chunk_sample(
    world: &super::World,
    index_ref: super::IndexRef,
    chunk_pos: Vec2<i32>,
    fixture: &FixedOverlayFixture,
) -> AuditChunkRuntimeSample {
    let (raw_chunk, supplement) = world
        .generate_chunk(index_ref, chunk_pos, None, || false, None)
        .expect("runtime audit chunk generation should succeed");
    let empty_overlay_chunk = raw_chunk.clone();
    let mut fixed_overlay_chunk = raw_chunk.clone();
    let empty_overlay_summary = OverlayApplySummary::default();
    let fixed_overlay_summary = apply_fixed_overlay_fixture(&mut fixed_overlay_chunk, fixture);

    let entity_signatures = summarize_entity_spawns(&supplement.entity_spawns);

    AuditChunkRuntimeSample {
        chunk_pos: [chunk_pos.x, chunk_pos.y],
        raw_worldgen: summarize_runtime_variant(
            "raw_worldgen",
            &raw_chunk,
            OverlayApplySummary::default(),
            &entity_signatures,
        ),
        empty_overlay: summarize_runtime_variant(
            "empty_overlay_server_chunk",
            &empty_overlay_chunk,
            empty_overlay_summary,
            &entity_signatures,
        ),
        fixed_overlay: summarize_runtime_variant(
            "fixed_overlay_server_chunk",
            &fixed_overlay_chunk,
            fixed_overlay_summary,
            &entity_signatures,
        ),
    }
}

fn summarize_runtime_variant(
    overlay_mode: &str,
    chunk: &TerrainChunk,
    overlay_summary: OverlayApplySummary,
    entity_signatures: &[String],
) -> AuditChunkRuntimeVariant {
    let volume = super::summarize_chunk_volume(chunk);
    AuditChunkRuntimeVariant {
        overlay_mode: overlay_mode.to_owned(),
        overlay_blocks_applied: overlay_summary.applied,
        overlay_operations_skipped: overlay_summary.skipped,
        min_z: chunk.get_min_z(),
        max_z: chunk.get_max_z(),
        sub_chunks: chunk.sub_chunks_len(),
        block_total: volume.block_total,
        non_air_blocks: volume.non_air_blocks,
        sprite_total: volume.sprite_total,
        block_kind_counts: volume.block_kind_counts,
        sprite_kind_counts: volume.sprite_kind_counts,
        entity_signature_count: entity_signatures.len(),
        entity_signatures: entity_signatures.to_vec(),
    }
}

fn load_fixed_overlay_fixture() -> FixedOverlayFixture {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("world crate should live under repo root")
        .to_path_buf();
    let fixture_path = repo_root.join(FIXED_OVERLAY_FIXTURE_PATH);
    let fixture_text = fs::read_to_string(&fixture_path).unwrap_or_else(|error| {
        panic!(
            "failed to read fixed overlay fixture {}: {error}",
            fixture_path.display()
        )
    });
    ron::from_str(&fixture_text).unwrap_or_else(|error| {
        panic!(
            "failed to parse fixed overlay fixture {}: {error}",
            fixture_path.display()
        )
    })
}

fn apply_fixed_overlay_fixture(
    chunk: &mut TerrainChunk,
    fixture: &FixedOverlayFixture,
) -> OverlayApplySummary {
    let mut summary = OverlayApplySummary::default();
    for operation in &fixture.operations {
        if apply_overlay_operation(chunk, operation) {
            summary.applied += 1;
        } else {
            summary.skipped += 1;
        }
    }
    summary
}

fn apply_overlay_operation(chunk: &mut TerrainChunk, operation: &FixedOverlayOperation) -> bool {
    let rel_xy = Vec2::new(operation.rel_xy[0] as i32, operation.rel_xy[1] as i32);
    if rel_xy.x < 0
        || rel_xy.y < 0
        || rel_xy.x >= TerrainChunkSize::RECT_SIZE.x as i32
        || rel_xy.y >= TerrainChunkSize::RECT_SIZE.y as i32
    {
        return false;
    }

    let Some(target) = resolve_overlay_target(chunk, rel_xy, &operation.target) else {
        return false;
    };

    let block = match &operation.block {
        FixedOverlayBlock::Solid { kind, color } => {
            Block::new(*kind, Rgb::new(color[0], color[1], color[2]))
        },
        FixedOverlayBlock::Air { sprite } => Block::air(*sprite),
        FixedOverlayBlock::Water { sprite } => Block::water(*sprite),
    };

    chunk.set(target, block).is_ok()
}

fn resolve_overlay_target(
    chunk: &TerrainChunk,
    rel_xy: Vec2<i32>,
    target: &FixedOverlayTarget,
) -> Option<Vec3<i32>> {
    match target {
        FixedOverlayTarget::TopMostNonAir => {
            for z in (chunk.get_min_z()..chunk.get_max_z()).rev() {
                let pos = rel_xy.with_z(z);
                if chunk
                    .get(pos)
                    .is_ok_and(|block| block.kind() != BlockKind::Air)
                {
                    return Some(pos);
                }
            }
            None
        },
    }
}

fn summarize_entity_spawns(entity_spawns: &[EntitySpawn]) -> Vec<String> {
    let mut entity_signatures = Vec::new();
    for entity_spawn in entity_spawns {
        match entity_spawn {
            EntitySpawn::Entity(entity) => collect_entity_signature(entity, &mut entity_signatures),
            EntitySpawn::Group(group) => {
                for entity in group {
                    collect_entity_signature(entity, &mut entity_signatures);
                }
            },
        }
    }
    entity_signatures.sort();
    entity_signatures
}

fn collect_entity_signature(entity: &EntityInfo, entity_signatures: &mut Vec<String>) {
    entity_signatures.push(format!(
        "pos_bits={},{},{};body={:?};alignment={:?};scale_bits={};inventory={};pets={};rider={};\
         special={};agency={};no_flee={}",
        entity.pos.x.to_bits(),
        entity.pos.y.to_bits(),
        entity.pos.z.to_bits(),
        entity.body,
        entity.alignment,
        entity.scale.to_bits(),
        entity.inventory.len(),
        entity.pets.len(),
        entity.rider.is_some(),
        entity.special_entity.is_some(),
        entity.has_agency,
        entity.no_flee,
    ));

    for pet in &entity.pets {
        collect_entity_signature(pet, entity_signatures);
    }

    if let Some(rider) = &entity.rider {
        collect_entity_signature(rider, entity_signatures);
    }
}
