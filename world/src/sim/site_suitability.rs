use super::{
    WorldSim,
    marine_semantics::{
        AquaticFaunaProfile, AquaticSpawnPotential, CoastalZone, MarineEcologyProfile,
        OceanDepthBand, WaterAccessClass, WaterBodyKind, marine_adjacency_at_site,
    },
    subterranean_semantics::CaveEntrancePotential,
};
use crate::{all::ForestKind, column::ColumnSample};
use common::terrain::BiomeKind;
use vek::Vec2;

pub(crate) fn is_water_occupied(water_body_kind: WaterBodyKind, is_submerged: bool) -> bool {
    !matches!(water_body_kind, WaterBodyKind::DryLand) || is_submerged
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WorldSemanticSample {
    pub alt: f32,
    pub water_level: f32,
    pub temp: f32,
    #[expect(
        dead_code,
        reason = "shared world semantic facts are staged ahead of wider consumers"
    )]
    pub humidity: f32,
    pub biome: BiomeKind,
    pub forest_kind: ForestKind,
    pub spawn_rate: f32,
    pub tree_density: f32,
    pub rockiness: f32,
    pub cliff_height: f32,
    pub gradient: Option<f32>,
    pub water_body_kind: WaterBodyKind,
    pub marine_adjacent: bool,
    pub water_access_class: WaterAccessClass,
    pub is_submerged: bool,
    pub water_occupied: bool,
}

impl WorldSemanticSample {
    pub(crate) fn at_site_loc(sim: &WorldSim, loc: Vec2<i32>) -> Option<Self> {
        let chunk = sim.get(loc)?;
        let water_body_kind = WaterBodyKind::from_chunk(chunk);
        let is_submerged = chunk.water_alt > chunk.alt;
        let water_occupied = is_water_occupied(water_body_kind, is_submerged);
        let marine_adjacent = marine_adjacency_at_site(sim, loc, chunk.alt);
        let water_access_class = WaterAccessClass::from_semantic_facts(
            water_body_kind,
            is_submerged,
            chunk.river.near_water(),
            chunk.water_alt,
            marine_adjacent,
        );

        Some(Self {
            alt: chunk.alt,
            water_level: chunk.water_alt,
            temp: chunk.temp,
            humidity: chunk.humidity,
            biome: chunk.get_biome(),
            forest_kind: chunk.forest_kind,
            spawn_rate: chunk.spawn_rate,
            tree_density: chunk.tree_density,
            rockiness: chunk.rockiness,
            cliff_height: chunk.cliff_height,
            gradient: sim.get_gradient_approx(loc),
            water_body_kind,
            marine_adjacent,
            water_access_class,
            is_submerged,
            water_occupied,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SuitabilitySet {
    pub settlement: SettlementSuitability,
    pub coastal_site: CoastalSiteSuitability,
    #[allow(
        dead_code,
        reason = "formal marine ecology output is staged ahead of wider marine_generation \
                  consumers"
    )]
    pub marine_ecology: MarineEcologySuitability,
    pub marine_site: MarineSiteSuitability,
    pub species_spawn: SpeciesSpawnSuitability,
    pub subterranean_entrance: SubterraneanEntranceSuitability,
    pub resource_extraction: ResourceExtractionSuitability,
}

impl SuitabilitySet {
    pub(crate) fn from_semantic_sample(sample: WorldSemanticSample) -> Self {
        let on_land = !sample.water_occupied;
        let cliff_exposure = sample.cliff_height > 0.0;
        let marine_ecology_profile = MarineEcologyProfile::from_world_facts(
            sample.water_level,
            sample.marine_adjacent,
            sample.biome,
            sample.alt,
        );
        let aquatic_spawn_potential = AquaticSpawnPotential::from_semantic_facts(
            sample.water_body_kind,
            sample.water_access_class,
        );
        let cave_entrance_potential =
            CaveEntrancePotential::from_world_facts(sample.cliff_height, sample.water_occupied);
        let aquatic_fauna_profile =
            AquaticFaunaProfile::from_profiles(aquatic_spawn_potential, marine_ecology_profile);
        let marine_ecology = MarineEcologySuitability {
            coastal_zone: marine_ecology_profile.coastal_zone,
            depth_band: marine_ecology_profile.depth_band,
        };

        Self {
            settlement: SettlementSuitability {
                on_land,
                flat_enough: sample.gradient.is_some_and(|grad| grad < 1.0),
                away_from_water: !sample.water_access_class.blocks_inland_site(),
                away_from_cliffs: sample.cliff_height <= 0.0,
            },
            coastal_site: CoastalSiteSuitability {
                settlement_shoreline_band: marine_ecology.coastal_zone.supports_settlement_site(),
                hideout_shoreline_band: marine_ecology.coastal_zone.supports_hideout_site(),
            },
            marine_ecology,
            marine_site: MarineSiteSuitability::from_semantic_profiles(
                marine_ecology,
                aquatic_fauna_profile,
            ),
            species_spawn: SpeciesSpawnSuitability {
                wooded: sample.tree_density > 0.4,
                spawn_potential: SpawnPotential::from_facts(
                    sample.spawn_rate,
                    sample.gradient < Some(1.3),
                ),
                forest_kind: sample.forest_kind,
                is_underwater: sample.is_submerged,
                is_ice: false,
            },
            subterranean_entrance: SubterraneanEntranceSuitability::from_semantic_profiles(
                cave_entrance_potential,
                cliff_exposure && sample.is_submerged,
            ),
            resource_extraction: ResourceExtractionSuitability {
                rocky_ground: sample.rockiness > 0.7,
                stone_rich_ground: sample.rockiness > 1.2,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SettlementSuitability {
    pub on_land: bool,
    pub flat_enough: bool,
    pub away_from_water: bool,
    pub away_from_cliffs: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CoastalSiteSuitability {
    pub settlement_shoreline_band: bool,
    pub hideout_shoreline_band: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MarineEcologySuitability {
    pub coastal_zone: CoastalZone,
    pub depth_band: OceanDepthBand,
}

impl MarineEcologySuitability {
    pub(crate) fn littoral_shelf(self) -> bool {
        matches!(self.depth_band, OceanDepthBand::LittoralShelf)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MarineSiteSuitability {
    pub chapel_site_candidate: bool,
    pub sahagin_site_candidate: bool,
}

impl MarineSiteSuitability {
    fn from_semantic_profiles(
        marine_ecology: MarineEcologySuitability,
        aquatic_fauna: AquaticFaunaProfile,
    ) -> Self {
        Self {
            chapel_site_candidate: marine_ecology.littoral_shelf() && aquatic_fauna.coastal_fauna,
            sahagin_site_candidate: aquatic_fauna.shelf_fauna,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SpeciesSpawnSuitability {
    pub wooded: bool,
    pub spawn_potential: SpawnPotential,
    #[expect(
        dead_code,
        reason = "spawn suitability contract is staged ahead of wildlife consumers"
    )]
    pub forest_kind: ForestKind,
    pub is_underwater: bool,
    pub is_ice: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SpawnPotential {
    pub spawn_rate: f32,
    pub gentle_gradient: bool,
}

impl SpawnPotential {
    pub(crate) fn from_facts(spawn_rate: f32, gentle_gradient: bool) -> Self {
        Self {
            spawn_rate,
            gentle_gradient,
        }
    }

    pub(crate) fn scaled_density(self, density: f32) -> Option<f32> {
        (density > 0.0 && self.gentle_gradient).then_some(density * self.spawn_rate)
    }

    pub(crate) fn passes_runtime_gate(self, density: f32, rng_draw: f32) -> bool {
        rng_draw < density * self.spawn_rate && self.gentle_gradient
    }
}

impl SpeciesSpawnSuitability {
    pub(crate) fn from_column_sample(col_sample: &ColumnSample<'_>) -> Self {
        let is_underwater = col_sample.water_level > col_sample.alt;
        let is_ice = col_sample.ice_depth > 0.5 && is_underwater;

        Self {
            wooded: col_sample.tree_density > 0.4,
            spawn_potential: SpawnPotential::from_facts(
                col_sample.spawn_rate,
                col_sample.gradient < Some(1.3),
            ),
            forest_kind: col_sample.forest_kind,
            is_underwater,
            is_ice,
        }
    }

    #[allow(
        dead_code,
        reason = "species spawn contract keeps the runtime gate entry staged ahead of wider \
                  consumers"
    )]
    pub(crate) fn passes_runtime_gate(self, density: f32, rng_draw: f32) -> bool {
        self.spawn_potential.passes_runtime_gate(density, rng_draw)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SubterraneanEntranceSuitability {
    pub surface_cave_entrance: bool,
    #[allow(
        dead_code,
        reason = "underwater cavity semantics are staged ahead of wider subterranean consumers"
    )]
    pub underwater_cavity_potential: bool,
}

impl SubterraneanEntranceSuitability {
    fn from_semantic_profiles(
        cave_entrance_potential: CaveEntrancePotential,
        underwater_cavity_potential: bool,
    ) -> Self {
        Self {
            surface_cave_entrance: cave_entrance_potential.surface_cave_entrance,
            underwater_cavity_potential,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResourceExtractionSuitability {
    pub rocky_ground: bool,
    pub stone_rich_ground: bool,
}

#[cfg(test)]
mod tests {
    use super::{
        SpawnPotential, SpeciesSpawnSuitability, SuitabilitySet, WorldSemanticSample,
        is_water_occupied,
    };
    use crate::{
        all::ForestKind,
        config::CONFIG,
        sim::marine_semantics::{CoastalZone, OceanDepthBand, WaterAccessClass, WaterBodyKind},
    };
    use common::terrain::BiomeKind;

    fn sample_with(water_body_kind: WaterBodyKind) -> WorldSemanticSample {
        WorldSemanticSample {
            alt: 128.0,
            water_level: 128.0,
            temp: 0.0,
            humidity: 0.5,
            biome: BiomeKind::Grassland,
            forest_kind: ForestKind::Oak,
            spawn_rate: 1.0,
            tree_density: 0.5,
            rockiness: 0.8,
            cliff_height: 0.0,
            gradient: Some(0.25),
            water_body_kind,
            marine_adjacent: matches!(water_body_kind, WaterBodyKind::Ocean),
            water_access_class: WaterAccessClass::Inland,
            is_submerged: false,
            water_occupied: is_water_occupied(water_body_kind, false),
        }
    }

    #[test]
    fn settlement_suitability_requires_dry_land() {
        let dry_land = SuitabilitySet::from_semantic_sample(sample_with(WaterBodyKind::DryLand));
        assert!(dry_land.settlement.on_land);

        let river = SuitabilitySet::from_semantic_sample(sample_with(WaterBodyKind::River));
        assert!(!river.settlement.on_land);
    }

    #[test]
    fn water_occupied_distinguishes_river_and_submerged_dry_land() {
        assert!(!is_water_occupied(WaterBodyKind::DryLand, false));
        assert!(is_water_occupied(WaterBodyKind::River, false));
        assert!(is_water_occupied(WaterBodyKind::DryLand, true));
    }

    #[test]
    fn coastal_band_flags_are_derived_from_sea_level_delta() {
        let mut shoreline = sample_with(WaterBodyKind::DryLand);
        shoreline.water_level = CONFIG.sea_level + 2.5;
        shoreline.marine_adjacent = true;
        shoreline.water_access_class = WaterAccessClass::CoastalShoreline;
        let shoreline = SuitabilitySet::from_semantic_sample(shoreline);
        assert!(shoreline.coastal_site.settlement_shoreline_band);
        assert!(shoreline.coastal_site.hideout_shoreline_band);

        let mut deep_inland = sample_with(WaterBodyKind::DryLand);
        deep_inland.water_level = CONFIG.sea_level + 10.0;
        let deep_inland = SuitabilitySet::from_semantic_sample(deep_inland);
        assert!(!deep_inland.coastal_site.settlement_shoreline_band);
        assert!(!deep_inland.coastal_site.hideout_shoreline_band);
    }

    #[test]
    fn coastal_band_requires_marine_adjacency() {
        let mut pseudo_coast = sample_with(WaterBodyKind::DryLand);
        pseudo_coast.water_level = CONFIG.sea_level + 2.5;
        pseudo_coast.marine_adjacent = false;
        pseudo_coast.water_access_class = WaterAccessClass::FreshwaterShoreline;

        let pseudo_coast = SuitabilitySet::from_semantic_sample(pseudo_coast);
        assert!(!pseudo_coast.coastal_site.settlement_shoreline_band);
        assert!(!pseudo_coast.coastal_site.hideout_shoreline_band);
        assert!(!pseudo_coast.settlement.away_from_water);
    }

    #[test]
    fn inland_settlement_water_gate_uses_water_access_class() {
        let inland = SuitabilitySet::from_semantic_sample(sample_with(WaterBodyKind::DryLand));
        assert!(inland.settlement.away_from_water);

        let mut freshwater_edge = sample_with(WaterBodyKind::DryLand);
        freshwater_edge.water_access_class = WaterAccessClass::FreshwaterShoreline;
        let freshwater_edge = SuitabilitySet::from_semantic_sample(freshwater_edge);
        assert!(!freshwater_edge.settlement.away_from_water);

        let mut coast = sample_with(WaterBodyKind::DryLand);
        coast.water_level = CONFIG.sea_level + 2.5;
        coast.marine_adjacent = true;
        coast.water_access_class = WaterAccessClass::CoastalShoreline;
        let coast = SuitabilitySet::from_semantic_sample(coast);
        assert!(coast.settlement.away_from_water);
    }

    #[test]
    fn species_spawn_suitability_tracks_runtime_gate_inputs() {
        let suitability = SpeciesSpawnSuitability {
            wooded: true,
            spawn_potential: SpawnPotential::from_facts(0.75, true),
            forest_kind: ForestKind::Oak,
            is_underwater: true,
            is_ice: true,
        };

        assert!(suitability.passes_runtime_gate(1.0, 0.5));
        assert!(!suitability.passes_runtime_gate(1.0, 0.9));
        assert!(suitability.is_underwater);
        assert!(suitability.is_ice);
    }

    #[test]
    fn spawn_potential_scales_density_and_runtime_gate() {
        let passable = SpawnPotential::from_facts(0.75, true);
        assert_eq!(passable.scaled_density(0.8), Some(0.6));
        assert!(passable.passes_runtime_gate(0.8, 0.59));
        assert!(!passable.passes_runtime_gate(0.8, 0.61));

        let blocked = SpawnPotential::from_facts(0.75, false);
        assert_eq!(blocked.scaled_density(0.8), None);
        assert!(!blocked.passes_runtime_gate(0.8, 0.0));
    }

    #[test]
    fn river_chunk_stays_non_submerged_in_static_semantics() {
        let mut river = sample_with(WaterBodyKind::River);
        river.water_access_class = WaterAccessClass::FreshwaterShoreline;
        let suitability = SuitabilitySet::from_semantic_sample(river);
        assert!(!suitability.settlement.on_land);
        assert!(!suitability.settlement.away_from_water);
        assert!(!suitability.species_spawn.is_underwater);
    }

    #[test]
    fn marine_site_candidates_derive_from_habitat_bands() {
        let mut chapel = sample_with(WaterBodyKind::Ocean);
        chapel.biome = BiomeKind::Ocean;
        chapel.alt = CONFIG.sea_level;
        chapel.water_level = CONFIG.sea_level + 2.5;
        chapel.water_access_class = WaterAccessClass::MarineSubmerged;
        chapel.is_submerged = true;
        let chapel = SuitabilitySet::from_semantic_sample(chapel);
        assert_eq!(
            chapel.marine_ecology.coastal_zone,
            CoastalZone::SettlementShoreline
        );
        assert_eq!(
            chapel.marine_ecology.depth_band,
            OceanDepthBand::LittoralShelf
        );
        assert_ne!(chapel.marine_ecology.depth_band, OceanDepthBand::NonOcean);
        assert!(chapel.marine_ecology.littoral_shelf());
        assert!(chapel.marine_site.chapel_site_candidate);
        assert!(!chapel.marine_site.sahagin_site_candidate);

        let mut sahagin = sample_with(WaterBodyKind::Ocean);
        sahagin.biome = BiomeKind::Ocean;
        sahagin.alt = CONFIG.sea_level - 42.0;
        sahagin.water_access_class = WaterAccessClass::MarineSubmerged;
        sahagin.is_submerged = true;
        let sahagin = SuitabilitySet::from_semantic_sample(sahagin);
        assert_eq!(
            sahagin.marine_ecology.depth_band,
            OceanDepthBand::SahaginShelf
        );
        assert!(!sahagin.marine_site.chapel_site_candidate);
        assert!(sahagin.marine_site.sahagin_site_candidate);

        let mut blocked_sahagin = sample_with(WaterBodyKind::Ocean);
        blocked_sahagin.biome = BiomeKind::Ocean;
        blocked_sahagin.alt = CONFIG.sea_level - 42.0;
        let blocked_sahagin = SuitabilitySet::from_semantic_sample(blocked_sahagin);
        assert_eq!(
            blocked_sahagin.marine_ecology.depth_band,
            OceanDepthBand::SahaginShelf
        );
        assert!(!blocked_sahagin.marine_site.sahagin_site_candidate);
    }

    #[test]
    fn chapel_candidate_requires_coastal_aquatic_habitat() {
        let mut blocked_chapel = sample_with(WaterBodyKind::Ocean);
        blocked_chapel.biome = BiomeKind::Ocean;
        blocked_chapel.alt = CONFIG.sea_level;
        blocked_chapel.water_level = CONFIG.sea_level + 2.5;
        let blocked_chapel = SuitabilitySet::from_semantic_sample(blocked_chapel);
        assert!(blocked_chapel.marine_ecology.littoral_shelf());
        assert!(!blocked_chapel.marine_site.chapel_site_candidate);
    }

    #[test]
    fn chapel_candidate_rejects_dry_coastal_shoreline() {
        let mut dry_coast = sample_with(WaterBodyKind::DryLand);
        dry_coast.water_level = CONFIG.sea_level + 2.5;
        dry_coast.marine_adjacent = true;
        dry_coast.water_access_class = WaterAccessClass::CoastalShoreline;
        let dry_coast = SuitabilitySet::from_semantic_sample(dry_coast);
        assert!(!dry_coast.marine_site.chapel_site_candidate);
    }

    #[test]
    fn dry_coastal_shoreline_keeps_town_and_hideout_bands_but_not_marine_sites() {
        let mut dry_coast = sample_with(WaterBodyKind::DryLand);
        dry_coast.water_level = CONFIG.sea_level + 2.5;
        dry_coast.marine_adjacent = true;
        dry_coast.water_access_class = WaterAccessClass::CoastalShoreline;
        let dry_coast = SuitabilitySet::from_semantic_sample(dry_coast);

        assert!(dry_coast.settlement.on_land);
        assert!(dry_coast.settlement.away_from_water);
        assert!(dry_coast.coastal_site.settlement_shoreline_band);
        assert!(dry_coast.coastal_site.hideout_shoreline_band);
        assert!(!dry_coast.marine_site.chapel_site_candidate);
        assert!(!dry_coast.marine_site.sahagin_site_candidate);
    }

    #[test]
    fn subterranean_entrance_distinguishes_surface_and_underwater_access() {
        let mut dry_cliff = sample_with(WaterBodyKind::DryLand);
        dry_cliff.cliff_height = 20.0;
        let dry_cliff = SuitabilitySet::from_semantic_sample(dry_cliff);
        assert!(dry_cliff.subterranean_entrance.surface_cave_entrance);
        assert!(!dry_cliff.subterranean_entrance.underwater_cavity_potential);

        let mut underwater_cliff = sample_with(WaterBodyKind::Ocean);
        underwater_cliff.biome = BiomeKind::Ocean;
        underwater_cliff.alt = CONFIG.sea_level - 20.0;
        underwater_cliff.water_level = CONFIG.sea_level;
        underwater_cliff.water_access_class = WaterAccessClass::MarineSubmerged;
        underwater_cliff.is_submerged = true;
        underwater_cliff.water_occupied = true;
        underwater_cliff.cliff_height = 20.0;
        let underwater_cliff = SuitabilitySet::from_semantic_sample(underwater_cliff);
        assert!(!underwater_cliff.subterranean_entrance.surface_cave_entrance);
        assert!(
            underwater_cliff
                .subterranean_entrance
                .underwater_cavity_potential
        );
    }

    #[test]
    fn subterranean_surface_entrance_rejects_water_occupied_cliffs() {
        let mut river_cliff = sample_with(WaterBodyKind::River);
        river_cliff.cliff_height = 20.0;
        river_cliff.water_access_class = WaterAccessClass::FreshwaterShoreline;
        let river_cliff = SuitabilitySet::from_semantic_sample(river_cliff);
        assert!(!river_cliff.subterranean_entrance.surface_cave_entrance);
        assert!(
            !river_cliff
                .subterranean_entrance
                .underwater_cavity_potential
        );
    }

    #[test]
    fn resource_extraction_distinguishes_rocky_and_stone_rich_ground() {
        let rocky_ground =
            SuitabilitySet::from_semantic_sample(sample_with(WaterBodyKind::DryLand));
        assert!(rocky_ground.resource_extraction.rocky_ground);
        assert!(!rocky_ground.resource_extraction.stone_rich_ground);

        let mut stone_rich = sample_with(WaterBodyKind::DryLand);
        stone_rich.rockiness = 1.3;
        let stone_rich = SuitabilitySet::from_semantic_sample(stone_rich);
        assert!(stone_rich.resource_extraction.rocky_ground);
        assert!(stone_rich.resource_extraction.stone_rich_ground);
    }
}
