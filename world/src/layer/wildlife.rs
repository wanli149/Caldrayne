use crate::{
    CONFIG, IndexRef,
    all::ForestKind,
    column::ColumnSample,
    sim::{
        SimChunk,
        marine_semantics::AquaticSpawnProfile,
        site_suitability::{SpawnPotential, SpeciesSpawnSuitability},
    },
    util::close,
};
use common::{
    assets::{AssetExt, Ron},
    calendar::{Calendar, CalendarEvent},
    generation::{ChunkSupplement, EntityInfo, EntitySpawn},
    resources::TimeOfDay,
    terrain::Block,
    time::DayPeriod,
    vol::{ReadVol, RectSizedVol, WriteVol},
};
use rand::prelude::*;
use serde::Deserialize;
use std::{f32, iter};
use vek::*;

type Weight = u32;
type Min = u8;
type Max = u8;

#[derive(Clone, Debug, Deserialize)]
pub struct SpawnEntry {
    /// User-facing info for wiki, statistical tools, etc.
    pub name: String,
    pub note: String,
    /// Rules describing what and when to spawn
    pub rules: Vec<Pack>,
}

impl SpawnEntry {
    pub fn from(asset_specifier: &str) -> Self {
        Ron::load_expect_cloned(asset_specifier).into_inner()
    }

    pub fn request(
        &self,
        requested_period: DayPeriod,
        calendar: Option<&Calendar>,
        is_underwater: bool,
        is_ice: bool,
    ) -> Option<Pack> {
        self.rules
            .iter()
            .find(|pack| {
                let time_match = pack.day_period.contains(&requested_period);
                let calendar_match = if let Some(calendar) = calendar {
                    pack.calendar_events
                        .as_ref()
                        .is_none_or(|events| events.iter().any(|event| calendar.is_event(*event)))
                } else {
                    false
                };
                let mode_match = match pack.spawn_mode {
                    SpawnMode::Land => !is_underwater,
                    SpawnMode::Ice => is_ice,
                    SpawnMode::Water | SpawnMode::Underwater => is_underwater,
                    SpawnMode::Air(_) => true,
                };
                time_match && calendar_match && mode_match
            })
            .cloned()
    }
}

/// Dataset of animals to spawn
///
/// Example:
/// ```text
///        Pack(
///            groups: [
///                (3, (1, 2, "common.entity.wild.aggressive.frostfang")),
///                (1, (1, 1, "common.entity.wild.aggressive.snow_leopard")),
///                (1, (1, 1, "common.entity.wild.aggressive.yale")),
///                (1, (1, 1, "common.entity.wild.aggressive.grolgar")),
///            ],
///            spawn_mode: Land,
///            day_period: [Night, Morning, Noon, Evening],
///        ),
/// ```
/// Groups:
/// ```text
///                (3, (1, 2, "common.entity.wild.aggressive.frostfang")),
/// ```
/// (3, ...) means that it has x3 chance to spawn (3/6 when every other has
/// 1/6).
///
/// (.., (1, 2, ...)) is `1..=2` group size which means that it has
/// chance to spawn as single mob or in pair
///
/// (..., (..., "common.entity.wild.aggressive.frostfang")) corresponds
/// to `assets/common/entity/wild/aggressive/frostfang.ron` file with
/// EntityConfig
///
/// Spawn mode:
/// `spawn_mode: Land` means mobs spawn on land at the surface (i.e: cows)
/// `spawn_mode: means mobs spawn on the surface of water ice
/// `spawn_mode: Water` means mobs spawn *in* water at a random depth (i.e:
/// fish) `spawn_mode: Underwater` means mobs spawn at the bottom of a body of
/// water (i.e: crabs) `spawn_mode: Air(32)` means mobs spawn in the air above
/// either land or water, with a maximum altitude of 32
///
/// Day period:
/// `day_period: [Night, Morning, Noon, Evening]`
/// means that mobs from this pack may be spawned in any day period without
/// exception
#[derive(Clone, Debug, Deserialize)]
pub struct Pack {
    pub groups: Vec<(Weight, (Min, Max, String))>,
    pub spawn_mode: SpawnMode,
    pub day_period: Vec<DayPeriod>,
    #[serde(default)]
    pub calendar_events: Option<Vec<CalendarEvent>>, /* None implies that the group isn't
                                                      * limited by calendar events */
}

#[derive(Copy, Clone, Debug, Deserialize)]
pub enum SpawnMode {
    Land,
    Ice,
    Water,
    Underwater,
    Air(f32),
}

impl Pack {
    pub fn generate(&self, pos: Vec3<f32>, dynamic_rng: &mut impl Rng) -> EntitySpawn {
        let (_, (from, to, entity_asset)) = self
            .groups
            .choose_weighted(dynamic_rng, |(p, _group)| *p)
            .expect("Failed to choose group");
        let entity = EntityInfo::at(pos).with_asset_expect(entity_asset, dynamic_rng, None);
        let group_size = dynamic_rng.random_range(*from..=*to);

        if group_size > 1 {
            let group = iter::repeat_n(entity, group_size as usize).collect::<Vec<_>>();

            EntitySpawn::Group(group)
        } else {
            EntitySpawn::Entity(Box::new(entity))
        }
    }
}

pub type DensityFn = fn(&SimChunk, &ColumnSample) -> f32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WildlifeRuntimeGate {
    pub is_underwater: bool,
    pub is_ice: bool,
    spawn_potential: SpawnPotential,
}

impl WildlifeRuntimeGate {
    pub fn from_column_sample(col_sample: &ColumnSample<'_>) -> Self {
        let species_spawn = SpeciesSpawnSuitability::from_column_sample(col_sample);
        Self {
            is_underwater: species_spawn.is_underwater,
            is_ice: species_spawn.is_ice,
            spawn_potential: species_spawn.spawn_potential,
        }
    }

    pub fn scaled_density(self, density: f32) -> Option<f32> {
        self.spawn_potential.scaled_density(density)
    }

    pub fn passes_runtime_gate(self, density: f32, rng_draw: f32) -> bool {
        self.spawn_potential.passes_runtime_gate(density, rng_draw)
    }
}

fn is_hardwood_forest(forest_kind: ForestKind) -> bool {
    matches!(
        forest_kind,
        ForestKind::Palm | ForestKind::Mangrove | ForestKind::Swamp
    )
}

fn hardwood_forest_floor(col: &ColumnSample<'_>) -> bool { is_hardwood_forest(col.forest_kind) }

fn is_frostwood_forest(forest_kind: ForestKind) -> bool {
    matches!(forest_kind, ForestKind::Frostpine)
}

fn frostwood_forest_floor(col: &ColumnSample<'_>) -> bool { is_frostwood_forest(col.forest_kind) }

fn freshwater_shoreline_density(
    temp: f32,
    target_temp: f32,
    spread: f32,
    aquatic_spawn: AquaticSpawnProfile,
    alt: f32,
) -> f32 {
    close(temp, target_temp, spread)
        * if aquatic_spawn.freshwater_shoreline && alt > CONFIG.sea_level + 20.0 {
            0.001
        } else {
            0.0
        }
}

fn ocean_surface_density(
    temp: f32,
    target_temp: f32,
    spread: f32,
    aquatic_spawn: AquaticSpawnProfile,
) -> f32 {
    close(temp, target_temp, spread) / 10.0
        * if aquatic_spawn.ocean_surface {
            0.001
        } else {
            0.0
        }
}

fn beach_shoreline_density(
    temp: f32,
    target_temp: f32,
    spread: f32,
    aquatic_spawn: AquaticSpawnProfile,
) -> f32 {
    close(temp, target_temp, spread) / 10.0
        * if aquatic_spawn.coastal_shoreline {
            0.001
        } else {
            0.0
        }
}

fn open_ocean_density(
    temp: f32,
    target_temp: f32,
    spread: f32,
    aquatic_spawn: AquaticSpawnProfile,
) -> f32 {
    close(temp, target_temp, spread) / 10.0 * if aquatic_spawn.open_ocean { 0.001 } else { 0.0 }
}

pub fn spawn_manifest() -> Vec<(&'static str, DensityFn)> {
    const BASE_DENSITY: f32 = 1.0e-5; // Base wildlife density
    // NOTE: Order matters.
    // Entries with more specific requirements
    // and overall scarcity should come first, where possible.
    vec![
        // **Tundra**
        // Rock animals
        ("world.wildlife.spawn.tundra.rock", |c, col| {
            close(c.temp, CONFIG.snow_temp, 0.15) * BASE_DENSITY * col.rock_density * 1.0
        }),
        // Core animals
        ("world.wildlife.spawn.tundra.core", |c, _col| {
            close(c.temp, CONFIG.snow_temp, 0.15) * BASE_DENSITY * 0.5
        }),
        // Core animals events
        (
            "world.wildlife.spawn.calendar.christmas.tundra.core",
            |c, _col| close(c.temp, CONFIG.snow_temp, 0.15) * BASE_DENSITY * 0.5,
        ),
        (
            "world.wildlife.spawn.calendar.halloween.tundra.core",
            |c, _col| close(c.temp, CONFIG.snow_temp, 0.15) * BASE_DENSITY * 1.0,
        ),
        (
            "world.wildlife.spawn.calendar.april_fools.tundra.core",
            |c, _col| close(c.temp, CONFIG.snow_temp, 0.15) * BASE_DENSITY * 0.5,
        ),
        (
            "world.wildlife.spawn.calendar.easter.tundra.core",
            |c, _col| close(c.temp, CONFIG.snow_temp, 0.15) * BASE_DENSITY * 0.5,
        ),
        // Snowy animals
        ("world.wildlife.spawn.tundra.snow", |c, col| {
            close(c.temp, CONFIG.snow_temp, 0.3) * BASE_DENSITY * col.snow_cover as i32 as f32 * 1.0
        }),
        // Snowy animals event
        (
            "world.wildlife.spawn.calendar.christmas.tundra.snow",
            |c, col| {
                close(c.temp, CONFIG.snow_temp, 0.3)
                    * BASE_DENSITY
                    * col.snow_cover as i32 as f32
                    * 1.0
            },
        ),
        (
            "world.wildlife.spawn.calendar.halloween.tundra.snow",
            |c, col| {
                close(c.temp, CONFIG.snow_temp, 0.3)
                    * BASE_DENSITY
                    * col.snow_cover as i32 as f32
                    * 1.5
            },
        ),
        (
            "world.wildlife.spawn.calendar.april_fools.tundra.snow",
            |c, col| {
                close(c.temp, CONFIG.snow_temp, 0.3)
                    * BASE_DENSITY
                    * col.snow_cover as i32 as f32
                    * 1.0
            },
        ),
        (
            "world.wildlife.spawn.calendar.easter.tundra.snow",
            |c, col| {
                close(c.temp, CONFIG.snow_temp, 0.3)
                    * BASE_DENSITY
                    * col.snow_cover as i32 as f32
                    * 1.0
            },
        ),
        // Forest animals
        ("world.wildlife.spawn.tundra.forest", |c, col| {
            close(c.temp, CONFIG.snow_temp, 0.3)
                * col.tree_density
                * BASE_DENSITY
                * 1.4
                * f32::from(frostwood_forest_floor(col))
        }),
        // River wildlife
        ("world.wildlife.spawn.tundra.river", |c, col| {
            let aquatic_spawn = AquaticSpawnProfile::from_column_sample(col);
            freshwater_shoreline_density(col.temp, CONFIG.snow_temp, 0.3, aquatic_spawn, c.alt)
        }),
        // Forest animals event
        (
            "world.wildlife.spawn.calendar.christmas.tundra.forest",
            |c, col| {
                close(c.temp, CONFIG.snow_temp, 0.3)
                    * col.tree_density
                    * BASE_DENSITY
                    * 1.4
                    * f32::from(frostwood_forest_floor(col))
            },
        ),
        (
            "world.wildlife.spawn.calendar.halloween.tundra.forest",
            |c, col| {
                close(c.temp, CONFIG.snow_temp, 0.3)
                    * col.tree_density
                    * BASE_DENSITY
                    * 2.0
                    * f32::from(frostwood_forest_floor(col))
            },
        ),
        (
            "world.wildlife.spawn.calendar.april_fools.tundra.forest",
            |c, col| {
                close(c.temp, CONFIG.snow_temp, 0.3)
                    * col.tree_density
                    * BASE_DENSITY
                    * 1.4
                    * f32::from(frostwood_forest_floor(col))
            },
        ),
        (
            "world.wildlife.spawn.calendar.easter.tundra.forest",
            |c, col| {
                close(c.temp, CONFIG.snow_temp, 0.3)
                    * col.tree_density
                    * BASE_DENSITY
                    * 1.4
                    * f32::from(frostwood_forest_floor(col))
            },
        ),
        // **Taiga**
        // Forest core animals
        ("world.wildlife.spawn.taiga.core_forest", |c, col| {
            close(c.temp, CONFIG.snow_temp + 0.2, 0.2)
                * col.tree_density
                * BASE_DENSITY
                * 0.4
                * f32::from(frostwood_forest_floor(col))
        }),
        // Forest core animals event
        (
            "world.wildlife.spawn.calendar.christmas.taiga.core_forest",
            |c, col| {
                close(c.temp, CONFIG.snow_temp + 0.2, 0.2)
                    * col.tree_density
                    * BASE_DENSITY
                    * 0.4
                    * f32::from(frostwood_forest_floor(col))
            },
        ),
        (
            "world.wildlife.spawn.calendar.halloween.taiga.core",
            |c, col| {
                close(c.temp, CONFIG.snow_temp + 0.2, 0.2)
                    * col.tree_density
                    * BASE_DENSITY
                    * 0.8
                    * f32::from(frostwood_forest_floor(col))
            },
        ),
        (
            "world.wildlife.spawn.calendar.april_fools.taiga.core",
            |c, col| {
                close(c.temp, CONFIG.snow_temp + 0.2, 0.2)
                    * col.tree_density
                    * BASE_DENSITY
                    * 0.4
                    * f32::from(frostwood_forest_floor(col))
            },
        ),
        (
            "world.wildlife.spawn.calendar.easter.taiga.core",
            |c, col| {
                close(c.temp, CONFIG.snow_temp + 0.2, 0.2)
                    * col.tree_density
                    * BASE_DENSITY
                    * 0.4
                    * f32::from(frostwood_forest_floor(col))
            },
        ),
        // Core animals
        ("world.wildlife.spawn.taiga.core", |c, _col| {
            close(c.temp, CONFIG.snow_temp + 0.2, 0.2) * BASE_DENSITY * 1.0
        }),
        // Forest area animals
        ("world.wildlife.spawn.taiga.forest", |c, col| {
            close(c.temp, CONFIG.snow_temp + 0.2, 0.6)
                * col.tree_density
                * BASE_DENSITY
                * 0.9
                * f32::from(frostwood_forest_floor(col))
        }),
        // Area animals
        ("world.wildlife.spawn.taiga.area", |c, _col| {
            close(c.temp, CONFIG.snow_temp + 0.2, 0.6) * BASE_DENSITY * 5.0
        }),
        // Water animals
        ("world.wildlife.spawn.taiga.water", |c, col| {
            close(c.temp, CONFIG.snow_temp, 0.15) * col.tree_density * BASE_DENSITY * 5.0
        }),
        // River wildlife
        ("world.wildlife.spawn.taiga.river", |c, col| {
            let aquatic_spawn = AquaticSpawnProfile::from_column_sample(col);
            freshwater_shoreline_density(
                col.temp,
                CONFIG.snow_temp + 0.2,
                0.6,
                aquatic_spawn,
                c.alt,
            )
        }),
        // **Temperate**
        // Area rare
        ("world.wildlife.spawn.temperate.rare", |c, _col| {
            close(c.temp, CONFIG.temperate_temp, 0.8) * BASE_DENSITY * 0.08
        }),
        // Plains
        ("world.wildlife.spawn.temperate.plains", |c, _col| {
            close(c.temp, CONFIG.temperate_temp, 0.8)
                * close(c.tree_density, 0.0, 0.1)
                * BASE_DENSITY
                * 5.0
        }),
        // River wildlife
        ("world.wildlife.spawn.temperate.river", |c, col| {
            let aquatic_spawn = AquaticSpawnProfile::from_column_sample(col);
            freshwater_shoreline_density(col.temp, CONFIG.temperate_temp, 0.6, aquatic_spawn, c.alt)
        }),
        // Forest animals
        ("world.wildlife.spawn.temperate.wood", |c, col| {
            close(c.temp, CONFIG.temperate_temp + 0.1, 0.5) * col.tree_density * BASE_DENSITY * 5.0
        }),
        // Rainforest animals
        ("world.wildlife.spawn.temperate.rainforest", |c, _col| {
            close(c.temp, CONFIG.temperate_temp + 0.1, 0.6)
                * close(c.humidity, CONFIG.forest_hum, 0.6)
                * BASE_DENSITY
                * 5.0
        }),
        // Temperate Rainforest animals event
        (
            "world.wildlife.spawn.calendar.halloween.temperate.rainforest",
            |c, _col| {
                close(c.temp, CONFIG.temperate_temp + 0.1, 0.6)
                    * close(c.humidity, CONFIG.forest_hum, 0.6)
                    * BASE_DENSITY
                    * 5.0
            },
        ),
        (
            "world.wildlife.spawn.calendar.april_fools.temperate.rainforest",
            |c, _col| {
                close(c.temp, CONFIG.temperate_temp + 0.1, 0.6)
                    * close(c.humidity, CONFIG.forest_hum, 0.6)
                    * BASE_DENSITY
                    * 4.0
            },
        ),
        (
            "world.wildlife.spawn.calendar.easter.temperate.rainforest",
            |c, _col| {
                close(c.temp, CONFIG.temperate_temp + 0.1, 0.6)
                    * close(c.humidity, CONFIG.forest_hum, 0.6)
                    * BASE_DENSITY
                    * 4.0
            },
        ),
        // Ocean animals
        ("world.wildlife.spawn.temperate.ocean", |_c, col| {
            let aquatic_spawn = AquaticSpawnProfile::from_column_sample(col);
            ocean_surface_density(col.temp, CONFIG.temperate_temp, 1.0, aquatic_spawn)
        }),
        // Ocean beach animals
        ("world.wildlife.spawn.temperate.beach", |_c, col| {
            let aquatic_spawn = AquaticSpawnProfile::from_column_sample(col);
            beach_shoreline_density(col.temp, CONFIG.temperate_temp, 1.0, aquatic_spawn)
        }),
        // **Jungle**
        // Rainforest animals
        ("world.wildlife.spawn.jungle.rainforest", |c, col| {
            close(c.temp, CONFIG.tropical_temp + 0.2, 0.2)
                * close(c.humidity, CONFIG.jungle_hum, 0.2)
                * f32::from(hardwood_forest_floor(col))
                * BASE_DENSITY
                * 2.8
        }),
        // Rainforest area animals
        ("world.wildlife.spawn.jungle.rainforest_area", |c, col| {
            close(c.temp, CONFIG.tropical_temp + 0.2, 0.3)
                * close(c.humidity, CONFIG.jungle_hum, 0.2)
                * f32::from(hardwood_forest_floor(col))
                * BASE_DENSITY
                * 8.0
        }),
        // Jungle animals event
        (
            "world.wildlife.spawn.calendar.halloween.jungle.area",
            |c, col| {
                close(c.temp, CONFIG.tropical_temp + 0.2, 0.3)
                    * close(c.humidity, CONFIG.jungle_hum, 0.2)
                    * f32::from(hardwood_forest_floor(col))
                    * BASE_DENSITY
                    * 10.0
            },
        ),
        (
            "world.wildlife.spawn.calendar.april_fools.jungle.area",
            |c, col| {
                close(c.temp, CONFIG.tropical_temp + 0.2, 0.3)
                    * close(c.humidity, CONFIG.jungle_hum, 0.2)
                    * f32::from(hardwood_forest_floor(col))
                    * BASE_DENSITY
                    * 8.0
            },
        ),
        (
            "world.wildlife.spawn.calendar.easter.jungle.area",
            |c, col| {
                close(c.temp, CONFIG.tropical_temp + 0.2, 0.3)
                    * close(c.humidity, CONFIG.jungle_hum, 0.2)
                    * f32::from(hardwood_forest_floor(col))
                    * BASE_DENSITY
                    * 8.0
            },
        ),
        // **Tropical**
        // River animals
        ("world.wildlife.spawn.tropical.river", |c, col| {
            let aquatic_spawn = AquaticSpawnProfile::from_column_sample(col);
            freshwater_shoreline_density(col.temp, CONFIG.tropical_temp, 0.5, aquatic_spawn, c.alt)
        }),
        // Ocean animals
        ("world.wildlife.spawn.tropical.ocean", |_c, col| {
            let aquatic_spawn = AquaticSpawnProfile::from_column_sample(col);
            ocean_surface_density(col.temp, CONFIG.tropical_temp, 0.1, aquatic_spawn)
        }),
        // Ocean beach animals
        ("world.wildlife.spawn.tropical.beach", |_c, col| {
            let aquatic_spawn = AquaticSpawnProfile::from_column_sample(col);
            beach_shoreline_density(col.temp, CONFIG.tropical_temp, 1.0, aquatic_spawn)
        }),
        // Arctic ocean animals
        ("world.wildlife.spawn.arctic.ocean", |_c, col| {
            let aquatic_spawn = AquaticSpawnProfile::from_column_sample(col);
            open_ocean_density(col.temp, CONFIG.snow_temp, 0.25, aquatic_spawn)
        }),
        // Rainforest area animals
        ("world.wildlife.spawn.tropical.rainforest", |c, _col| {
            close(c.temp, CONFIG.tropical_temp + 0.1, 0.4)
                * close(c.humidity, CONFIG.jungle_hum, 0.4)
                * BASE_DENSITY
                * 2.0
        }),
        // Tropical Rainforest animals event
        (
            "world.wildlife.spawn.calendar.halloween.tropical.rainforest",
            |c, _col| {
                close(c.temp, CONFIG.tropical_temp + 0.1, 0.4)
                    * close(c.humidity, CONFIG.jungle_hum, 0.4)
                    * BASE_DENSITY
                    * 3.5
            },
        ),
        (
            "world.wildlife.spawn.calendar.april_fools.tropical.rainforest",
            |c, _col| {
                close(c.temp, CONFIG.tropical_temp + 0.1, 0.4)
                    * close(c.humidity, CONFIG.jungle_hum, 0.4)
                    * BASE_DENSITY
                    * 2.0
            },
        ),
        // Rock animals
        ("world.wildlife.spawn.tropical.rock", |c, col| {
            close(c.temp, CONFIG.tropical_temp + 0.1, 0.5) * col.rock_density * BASE_DENSITY * 5.0
        }),
        // **Desert**
        // Area animals
        ("world.wildlife.spawn.desert.area", |c, _col| {
            close(c.temp, CONFIG.desert_temp + 0.1, 0.4)
                * close(c.humidity, CONFIG.desert_hum, 0.4)
                * BASE_DENSITY
                * 0.8
        }),
        // Wasteland animals
        ("world.wildlife.spawn.desert.wasteland", |c, _col| {
            close(c.temp, CONFIG.desert_temp + 0.2, 0.3)
                * close(c.humidity, CONFIG.desert_hum, 0.5)
                * BASE_DENSITY
                * 1.3
        }),
        // River animals
        ("world.wildlife.spawn.desert.river", |c, col| {
            let aquatic_spawn = AquaticSpawnProfile::from_column_sample(col);
            freshwater_shoreline_density(
                col.temp,
                CONFIG.desert_temp + 0.2,
                0.3,
                aquatic_spawn,
                c.alt,
            )
        }),
        // Hot area desert
        ("world.wildlife.spawn.desert.hot", |c, _col| {
            close(c.temp, CONFIG.desert_temp + 0.2, 0.3) * BASE_DENSITY * 3.8
        }),
        // Rock animals
        ("world.wildlife.spawn.desert.rock", |c, col| {
            close(c.temp, CONFIG.desert_temp + 0.2, 0.05) * col.rock_density * BASE_DENSITY * 4.0
        }),
    ]
}

pub fn apply_wildlife_supplement<'a, R: Rng>(
    // NOTE: Used only for dynamic elements like chests and entities!
    dynamic_rng: &mut R,
    wpos2d: Vec2<i32>,
    mut get_column: impl FnMut(Vec2<i32>) -> Option<&'a ColumnSample<'a>>,
    vol: &(impl RectSizedVol<Vox = Block> + ReadVol + WriteVol),
    index: IndexRef,
    chunk: &SimChunk,
    supplement: &mut ChunkSupplement,
    time: Option<&(TimeOfDay, Calendar)>,
) {
    let scatter = &index.wildlife_spawns;
    // Configurable density multiplier
    let wildlife_density_modifier = index.features.wildlife_density;

    for y in 0..vol.size_xy().y as i32 {
        for x in 0..vol.size_xy().x as i32 {
            let offs = Vec2::new(x, y);

            let wpos2d = wpos2d + offs;

            // Sample terrain
            let col_sample = if let Some(col_sample) = get_column(offs) {
                col_sample
            } else {
                continue;
            };

            let runtime_gate = WildlifeRuntimeGate::from_column_sample(col_sample);
            let (current_day_period, calendar) = if let Some((time, calendar)) = time {
                (DayPeriod::from(time.0), Some(calendar))
            } else {
                (DayPeriod::Noon, None)
            };

            let entity_group = scatter
                .iter()
                .filter_map(|(entry, get_density)| {
                    let density = get_density(chunk, col_sample) * wildlife_density_modifier;
                    (density > 0.0)
                        .then(|| {
                            entry
                                .read()
                                .0
                                .request(
                                    current_day_period,
                                    calendar,
                                    runtime_gate.is_underwater,
                                    runtime_gate.is_ice,
                                )
                                .and_then(|pack| {
                                    runtime_gate
                                        .passes_runtime_gate(density, dynamic_rng.random::<f32>())
                                        .then_some(pack)
                                })
                        })
                        .flatten()
                })
                .collect::<Vec<_>>() // TODO: Don't allocate
                .choose_mut(dynamic_rng)
                .cloned();

            if let Some(pack) = entity_group {
                let desired_alt = match pack.spawn_mode {
                    SpawnMode::Land | SpawnMode::Underwater => col_sample.alt,
                    SpawnMode::Ice => col_sample.water_level + 1.0 + col_sample.ice_depth,
                    SpawnMode::Water => dynamic_rng.random_range(
                        col_sample.alt..col_sample.water_level.max(col_sample.alt + 0.1),
                    ),
                    SpawnMode::Air(height) => {
                        col_sample.alt.max(col_sample.water_level)
                            + dynamic_rng.random::<f32>() * height
                    },
                };

                let spawn_offset = |offs_wpos2d: Vec2<i32>| {
                    // Clamp position to chunk
                    let offs_wpos2d = (offs + offs_wpos2d)
                        .clamped(Vec2::zero(), vol.size_xy().map(|e| e as i32) - 1)
                        - offs;

                    // Find the intersection between ground and air, if there is one near the
                    // surface
                    let z_offset = (0..16)
                        .map(|z| if z % 2 == 0 { z } else { -z } / 2)
                        .find(|z| {
                            (0..2).all(|z2| {
                                vol.get(
                                    Vec3::new(offs.x, offs.y, desired_alt as i32)
                                        + offs_wpos2d.with_z(z + z2),
                                )
                                .map(|b| !b.is_solid())
                                .unwrap_or(true)
                            })
                        });

                    z_offset.map(|z_offset| offs_wpos2d.with_z(z_offset).map(|e| e as f32))
                };

                let mut entity_spawn = pack.generate(
                    (wpos2d.map(|e| e as f32) + 0.5).with_z(desired_alt),
                    dynamic_rng,
                );
                match entity_spawn {
                    EntitySpawn::Entity(ref mut entity) => {
                        // Choose a nearby position
                        let offs_wpos2d = (Vec2::new(0.0, 1.0)
                            * (5.0 + dynamic_rng.random::<f32>().powf(0.5) * 5.0))
                            .map(|e| e as i32);

                        if let Some(spawn_offset) = spawn_offset(offs_wpos2d) {
                            entity.pos += spawn_offset;
                            supplement.add_entity_spawn(entity_spawn);
                        }
                    },
                    EntitySpawn::Group(ref mut group) => {
                        let group_size = group.len();
                        for e in (0..group.len()).rev() {
                            // Choose a nearby position
                            let offs_wpos2d = (Vec2::new(
                                (e as f32 / group_size as f32 * 2.0 * f32::consts::PI).sin(),
                                (e as f32 / group_size as f32 * 2.0 * f32::consts::PI).cos(),
                            ) * (5.0
                                + dynamic_rng.random::<f32>().powf(0.5) * 5.0))
                                .map(|e| e as i32);

                            if let Some(spawn_offset) = spawn_offset(offs_wpos2d) {
                                group[e].pos += spawn_offset;
                            } else {
                                group.remove(e);
                            }
                        }

                        if !group.is_empty() {
                            supplement.add_entity_spawn(entity_spawn);
                        }
                    },
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        all::ForestKind,
        column::ColumnSample,
        sim::{RiverData, RiverKind, SimChunk},
    };
    use hashbrown::HashMap;
    use vek::{Rgb, Vec2, Vec3};

    fn river_data(kind: Option<RiverKind>) -> RiverData {
        RiverData {
            velocity: Vec3::zero(),
            spline_derivative: Vec2::zero(),
            river_kind: kind,
            neighbor_rivers: Vec::new(),
        }
    }

    fn sim_chunk_with_river(
        kind: Option<RiverKind>,
        alt: f32,
        water_alt: f32,
        temp: f32,
    ) -> SimChunk {
        SimChunk {
            chaos: 0.0,
            alt,
            basement: alt,
            water_alt,
            downhill: None,
            flux: 0.0,
            temp,
            humidity: 0.5,
            rockiness: 0.0,
            tree_density: 0.5,
            forest_kind: ForestKind::Oak,
            spawn_rate: 1.0,
            river: river_data(kind),
            surface_veg: 1.0,
            sites: Vec::new(),
            place: None,
            poi: None,
            path: Default::default(),
            cliff_height: 0.0,
            spot: None,
            contains_waypoint: false,
        }
    }

    fn column_sample<'a>(
        chunk: &'a SimChunk,
        water_dist: Option<f32>,
        marine_adjacent: bool,
    ) -> ColumnSample<'a> {
        ColumnSample {
            alt: chunk.alt,
            riverless_alt: chunk.alt,
            basement: chunk.basement,
            chaos: chunk.chaos,
            water_level: chunk.water_alt,
            warp_factor: 0.0,
            surface_color: Rgb::broadcast(0.0),
            sub_surface_color: Rgb::broadcast(0.0),
            tree_density: chunk.tree_density,
            forest_kind: chunk.forest_kind,
            marble: 0.0,
            marble_mid: 0.0,
            marble_small: 0.0,
            rock_density: 0.0,
            temp: chunk.temp,
            humidity: chunk.humidity,
            spawn_rate: chunk.spawn_rate,
            stone_col: Rgb::broadcast(0),
            water_dist,
            marine_adjacent,
            gradient: Some(0.0),
            path: None,
            snow_cover: false,
            cliff_offset: 0.0,
            cliff_height: chunk.cliff_height,
            water_vel: Vec3::zero(),
            ice_depth: 0.0,
            chunk,
        }
    }

    fn density_for(entry: &str, chunk: &SimChunk, col: &ColumnSample<'_>) -> f32 {
        spawn_manifest()
            .into_iter()
            .find(|(name, _)| *name == entry)
            .unwrap_or_else(|| panic!("missing wildlife entry {entry}"))
            .1(chunk, col)
    }

    // Checks that each entry in spawn manifest is loadable
    #[test]
    fn test_load_entries() {
        let scatter = spawn_manifest();
        for (entry, _) in scatter.into_iter() {
            drop(SpawnEntry::from(entry));
        }
    }

    // Check that each spawn entry has unique name
    #[test]
    fn test_name_uniqueness() {
        let scatter = spawn_manifest();
        let mut names = HashMap::new();
        for (entry, _) in scatter.into_iter() {
            let SpawnEntry { name, .. } = SpawnEntry::from(entry);
            if let Some(old_entry) = names.insert(name, entry) {
                panic!("{}: Found name duplicate with {}", entry, old_entry);
            }
        }
    }

    // Checks that each entity is loadable
    #[test]
    fn test_load_entities() {
        let scatter = spawn_manifest();
        for (entry, _) in scatter.into_iter() {
            let SpawnEntry { rules, .. } = SpawnEntry::from(entry);
            for pack in rules {
                let Pack { groups, .. } = pack;
                for group in &groups {
                    println!("{}:", entry);
                    let (_, (_, _, asset)) = group;
                    let dummy_pos = Vec3::new(0.0, 0.0, 0.0);
                    let mut dummy_rng = rand::rng();
                    let entity =
                        EntityInfo::at(dummy_pos).with_asset_expect(asset, &mut dummy_rng, None);
                    drop(entity);
                }
            }
        }
    }

    // Checks that group distribution has valid form
    #[test]
    fn test_group_choose() {
        let scatter = spawn_manifest();
        for (entry, _) in scatter.into_iter() {
            let SpawnEntry { rules, .. } = SpawnEntry::from(entry);
            for pack in rules {
                let Pack { groups, .. } = pack;
                let dynamic_rng = &mut rand::rng();
                let _ = groups
                    .choose_weighted(dynamic_rng, |(p, _group)| *p)
                    .unwrap_or_else(|err| {
                        panic!("{}: Failed to choose random group. Err: {}", entry, err)
                    });
            }
        }
    }

    #[test]
    fn runtime_gate_scales_density_and_respects_gradient() {
        let passable = WildlifeRuntimeGate {
            is_underwater: false,
            is_ice: false,
            spawn_potential: SpawnPotential::from_facts(0.5, true),
        };
        assert_eq!(passable.scaled_density(0.8), Some(0.4));
        assert!(passable.passes_runtime_gate(0.8, 0.39));
        assert!(!passable.passes_runtime_gate(0.8, 0.41));

        let blocked = WildlifeRuntimeGate {
            spawn_potential: SpawnPotential::from_facts(0.5, false),
            ..passable
        };
        assert_eq!(blocked.scaled_density(0.8), None);
        assert!(!blocked.passes_runtime_gate(0.8, 0.0));
    }

    #[test]
    fn freshwater_shoreline_density_rejects_coast_and_low_altitude() {
        let freshwater = freshwater_shoreline_density(
            CONFIG.temperate_temp,
            CONFIG.temperate_temp,
            0.6,
            AquaticSpawnProfile {
                freshwater_shoreline: true,
                coastal_shoreline: false,
                ocean_surface: false,
                open_ocean: false,
            },
            CONFIG.sea_level + 40.0,
        );
        assert!(freshwater > 0.0);

        let coastal = freshwater_shoreline_density(
            CONFIG.temperate_temp,
            CONFIG.temperate_temp,
            0.6,
            AquaticSpawnProfile {
                freshwater_shoreline: false,
                coastal_shoreline: true,
                ocean_surface: false,
                open_ocean: false,
            },
            CONFIG.sea_level + 40.0,
        );
        assert_eq!(coastal, 0.0);

        let low_alt_freshwater = freshwater_shoreline_density(
            CONFIG.temperate_temp,
            CONFIG.temperate_temp,
            0.6,
            AquaticSpawnProfile {
                freshwater_shoreline: true,
                coastal_shoreline: false,
                ocean_surface: false,
                open_ocean: false,
            },
            CONFIG.sea_level + 5.0,
        );
        assert_eq!(low_alt_freshwater, 0.0);
    }

    #[test]
    fn aquatic_density_helpers_distinguish_freshwater_beach_surface_and_open_ocean() {
        let freshwater = AquaticSpawnProfile {
            freshwater_shoreline: true,
            coastal_shoreline: false,
            ocean_surface: false,
            open_ocean: false,
        };
        assert!(
            freshwater_shoreline_density(
                CONFIG.temperate_temp,
                CONFIG.temperate_temp,
                0.6,
                freshwater,
                CONFIG.sea_level + 40.0,
            ) > 0.0
        );
        assert_eq!(
            ocean_surface_density(
                CONFIG.temperate_temp,
                CONFIG.temperate_temp,
                1.0,
                freshwater
            ),
            0.0
        );

        let beach = AquaticSpawnProfile {
            freshwater_shoreline: false,
            ocean_surface: false,
            coastal_shoreline: true,
            open_ocean: false,
        };
        assert!(
            beach_shoreline_density(CONFIG.temperate_temp, CONFIG.temperate_temp, 1.0, beach) > 0.0
        );
        assert_eq!(
            ocean_surface_density(CONFIG.temperate_temp, CONFIG.temperate_temp, 1.0, beach),
            0.0
        );

        let far_ocean = AquaticSpawnProfile {
            freshwater_shoreline: false,
            open_ocean: true,
            ocean_surface: false,
            coastal_shoreline: false,
        };
        assert!(open_ocean_density(CONFIG.snow_temp, CONFIG.snow_temp, 0.25, far_ocean) > 0.0);
        assert_eq!(
            ocean_surface_density(CONFIG.snow_temp, CONFIG.snow_temp, 0.25, far_ocean),
            0.0
        );
        assert_eq!(
            beach_shoreline_density(CONFIG.temperate_temp, CONFIG.temperate_temp, 1.0, far_ocean),
            0.0
        );

        let surface = AquaticSpawnProfile {
            freshwater_shoreline: false,
            open_ocean: true,
            ocean_surface: true,
            coastal_shoreline: false,
        };
        assert!(
            ocean_surface_density(CONFIG.temperate_temp, CONFIG.temperate_temp, 1.0, surface) > 0.0
        );
        assert!(open_ocean_density(CONFIG.snow_temp, CONFIG.snow_temp, 0.25, surface) > 0.0);
        assert_eq!(
            beach_shoreline_density(CONFIG.temperate_temp, CONFIG.temperate_temp, 1.0, surface),
            0.0
        );
    }

    #[test]
    fn manifest_aquatic_entries_route_by_aquatic_spawn_profile() {
        let river_chunk = sim_chunk_with_river(
            Some(RiverKind::River {
                cross_section: Vec2::one(),
            }),
            CONFIG.sea_level + 40.0,
            CONFIG.sea_level,
            CONFIG.temperate_temp,
        );
        let river_col = column_sample(&river_chunk, Some(0.5), false);
        assert!(
            density_for(
                "world.wildlife.spawn.temperate.river",
                &river_chunk,
                &river_col
            ) > 0.0
        );
        assert_eq!(
            density_for(
                "world.wildlife.spawn.temperate.beach",
                &river_chunk,
                &river_col
            ),
            0.0
        );
        assert_eq!(
            density_for(
                "world.wildlife.spawn.temperate.ocean",
                &river_chunk,
                &river_col
            ),
            0.0
        );

        let coast_chunk = sim_chunk_with_river(
            None,
            CONFIG.sea_level + 2.5,
            CONFIG.sea_level + 2.5,
            CONFIG.temperate_temp,
        );
        let coast_col = column_sample(&coast_chunk, Some(0.5), true);
        assert_eq!(
            density_for(
                "world.wildlife.spawn.temperate.river",
                &coast_chunk,
                &coast_col
            ),
            0.0
        );
        assert!(
            density_for(
                "world.wildlife.spawn.temperate.beach",
                &coast_chunk,
                &coast_col
            ) > 0.0
        );
        assert_eq!(
            density_for(
                "world.wildlife.spawn.temperate.ocean",
                &coast_chunk,
                &coast_col
            ),
            0.0
        );

        let ocean_chunk = sim_chunk_with_river(
            Some(RiverKind::Ocean),
            CONFIG.sea_level - 10.0,
            CONFIG.sea_level,
            CONFIG.temperate_temp,
        );
        let ocean_col = column_sample(&ocean_chunk, Some(0.5), true);
        assert_eq!(
            density_for(
                "world.wildlife.spawn.temperate.river",
                &ocean_chunk,
                &ocean_col
            ),
            0.0
        );
        assert_eq!(
            density_for(
                "world.wildlife.spawn.temperate.beach",
                &ocean_chunk,
                &ocean_col
            ),
            0.0
        );
        assert!(
            density_for(
                "world.wildlife.spawn.temperate.ocean",
                &ocean_chunk,
                &ocean_col
            ) > 0.0
        );
    }

    #[test]
    fn frostwood_helper_distinguishes_frostpine_forest_kind() {
        assert!(is_frostwood_forest(ForestKind::Frostpine));
        assert!(!is_frostwood_forest(ForestKind::Oak));
        assert!(!is_frostwood_forest(ForestKind::Palm));
    }

    #[test]
    fn hardwood_helper_distinguishes_palm_mangrove_swamp() {
        assert!(is_hardwood_forest(ForestKind::Palm));
        assert!(is_hardwood_forest(ForestKind::Mangrove));
        assert!(is_hardwood_forest(ForestKind::Swamp));
        assert!(!is_hardwood_forest(ForestKind::Oak));
        assert!(!is_hardwood_forest(ForestKind::Frostpine));
    }

    #[test]
    fn tundra_and_taiga_forest_entries_require_frostpine_forest_kind() {
        fn assert_entry_requires_frostpine(entry: &str, temp: f32) {
            let mut frostpine_chunk =
                sim_chunk_with_river(None, CONFIG.sea_level + 60.0, CONFIG.sea_level + 60.0, temp);
            frostpine_chunk.forest_kind = ForestKind::Frostpine;
            let frostpine_col = column_sample(&frostpine_chunk, None, false);
            assert!(
                density_for(entry, &frostpine_chunk, &frostpine_col) > 0.0,
                "{entry} should stay active for Frostpine forest chunks"
            );

            let mut oak_chunk = frostpine_chunk;
            oak_chunk.forest_kind = ForestKind::Oak;
            let oak_col = column_sample(&oak_chunk, None, false);
            assert_eq!(
                density_for(entry, &oak_chunk, &oak_col),
                0.0,
                "{entry} should reject non-frostwood forest kinds"
            );
        }

        for entry in [
            "world.wildlife.spawn.tundra.forest",
            "world.wildlife.spawn.calendar.christmas.tundra.forest",
            "world.wildlife.spawn.calendar.halloween.tundra.forest",
            "world.wildlife.spawn.calendar.april_fools.tundra.forest",
            "world.wildlife.spawn.calendar.easter.tundra.forest",
        ] {
            assert_entry_requires_frostpine(entry, CONFIG.snow_temp);
        }

        for entry in [
            "world.wildlife.spawn.taiga.core_forest",
            "world.wildlife.spawn.calendar.christmas.taiga.core_forest",
            "world.wildlife.spawn.calendar.halloween.taiga.core",
            "world.wildlife.spawn.calendar.april_fools.taiga.core",
            "world.wildlife.spawn.calendar.easter.taiga.core",
            "world.wildlife.spawn.taiga.forest",
        ] {
            assert_entry_requires_frostpine(entry, CONFIG.snow_temp + 0.2);
        }
    }

    #[test]
    fn jungle_rainforest_entries_require_hardwood_forest_kind() {
        fn assert_entry_requires_hardwood(entry: &str) {
            let mut mangrove_chunk = sim_chunk_with_river(
                None,
                CONFIG.sea_level + 60.0,
                CONFIG.sea_level + 60.0,
                CONFIG.tropical_temp + 0.2,
            );
            mangrove_chunk.humidity = CONFIG.jungle_hum;
            mangrove_chunk.forest_kind = ForestKind::Mangrove;
            let mangrove_col = column_sample(&mangrove_chunk, None, false);
            assert!(
                density_for(entry, &mangrove_chunk, &mangrove_col) > 0.0,
                "{entry} should stay active for hardwood jungle chunks"
            );

            let mut oak_chunk = mangrove_chunk;
            oak_chunk.forest_kind = ForestKind::Oak;
            let oak_col = column_sample(&oak_chunk, None, false);
            assert_eq!(
                density_for(entry, &oak_chunk, &oak_col),
                0.0,
                "{entry} should reject non-hardwood forest kinds"
            );
        }

        for entry in [
            "world.wildlife.spawn.jungle.rainforest",
            "world.wildlife.spawn.jungle.rainforest_area",
            "world.wildlife.spawn.calendar.halloween.jungle.area",
            "world.wildlife.spawn.calendar.april_fools.jungle.area",
            "world.wildlife.spawn.calendar.easter.jungle.area",
        ] {
            assert_entry_requires_hardwood(entry);
        }
    }
}
