use super::SiteKind;
use crate::{
    config::CONFIG,
    sim::{
        WorldSim,
        site_suitability::{SuitabilitySet, WorldSemanticSample},
    },
    site::coastal_suitability::{PortPotential, TradeCorridorPotential, WaterNeighborhoodProfile},
};
use common::terrain::{BiomeKind, CoordinateConversions};
use vek::Vec2;

#[derive(Clone, Copy, Debug)]
pub(crate) struct SiteTerrainContext {
    pub semantic_sample: WorldSemanticSample,
    pub suitabilities: SuitabilitySet,
}

impl SiteTerrainContext {
    pub(crate) fn at_site_loc(sim: &WorldSim, loc: Vec2<i32>) -> Option<Self> {
        let semantic_sample = WorldSemanticSample::at_site_loc(sim, loc)?;
        let suitabilities = SuitabilitySet::from_semantic_sample(semantic_sample);

        Some(Self {
            semantic_sample,
            suitabilities,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TownEconomicProfile {
    town_resource_profile: TownResourceProfile,
    trade_corridor_potential: TradeCorridorPotential,
    port_potential: PortPotential,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TownEconomicOutput {
    pub farming_score: i32,
    pub fishing_score: i32,
    pub mining_score: i32,
    pub forestry_score: i32,
    pub building_materials: bool,
    pub potable_water: bool,
    pub aquifer: bool,
    pub heating: bool,
    pub trade_corridor_score: i32,
    pub port_candidate: bool,
}

#[derive(Clone, Copy, Debug)]
struct TownResourceProfile {
    food_potential: FoodPotential,
    extraction_potential: ExtractionPotential,
    water_supply_potential: WaterSupplyPotential,
    heating: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TownFoundationCandidate {
    terrain: SiteTerrainContext,
    economic_profile: TownEconomicProfile,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FoodPotential {
    farming_score: i32,
    fishing_score: i32,
}

impl FoodPotential {
    pub(crate) fn from_scores(farming_score: i32, fishing_score: i32) -> Self {
        Self {
            farming_score,
            fishing_score,
        }
    }

    pub(crate) fn food_score(self) -> i32 { self.farming_score + self.fishing_score }

    pub(crate) fn farming_score(self) -> i32 { self.farming_score }

    pub(crate) fn fishing_score(self) -> i32 { self.fishing_score }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ExtractionPotential {
    mining_score: i32,
    forestry_score: i32,
    building_materials: bool,
}

impl ExtractionPotential {
    pub(crate) fn from_scores(
        mining_score: i32,
        forestry_score: i32,
        building_materials: bool,
    ) -> Self {
        Self {
            mining_score,
            forestry_score,
            building_materials,
        }
    }

    pub(crate) fn mining_score(self) -> i32 { self.mining_score }

    pub(crate) fn forestry_score(self) -> i32 { self.forestry_score }

    pub(crate) fn has_building_materials(self) -> bool { self.building_materials }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct WaterSupplyPotential {
    potable_water: bool,
    aquifer: bool,
}

impl WaterSupplyPotential {
    pub(crate) fn from_facts(potable_water: bool, aquifer: bool) -> Self {
        Self {
            potable_water,
            aquifer,
        }
    }

    pub(crate) fn has_potable_water(self) -> bool { self.potable_water }

    pub(crate) fn has_aquifer(self) -> bool { self.aquifer }
}

impl TownEconomicProfile {
    pub(crate) fn at_site_loc(sim: &WorldSim, loc: Vec2<i32>) -> Option<Self> {
        sim.get(loc).map(|chunk| {
            let water_profile = WaterNeighborhoodProfile::collect(sim, loc, chunk.alt);
            let coastal_profile = water_profile.coastal_profile();
            let port_potential = coastal_profile.port_potential();
            let trade_corridor_potential = coastal_profile.trade_corridor_potential();
            let mut rock_chunks = 0;
            let mut tree_chunks = 0;
            let mut farmable_chunks = 0;
            let mut farmable_needs_irrigation_chunks = 0;
            for x in (-1)..1 {
                for y in (-1)..1 {
                    let check_loc = loc + Vec2::new(x, y).cpos_to_wpos();
                    sim.get(check_loc).map(|c| {
                        if num::abs(chunk.alt - c.alt) < 200.0 {
                            if c.tree_density > 0.7 {
                                tree_chunks += 1;
                            }
                            if c.rockiness < 0.3 && c.temp > CONFIG.snow_temp {
                                if c.surface_veg > 0.5 {
                                    farmable_chunks += 1;
                                } else {
                                    match c.get_biome() {
                                        BiomeKind::Savannah | BiomeKind::Desert => {
                                            farmable_needs_irrigation_chunks += 1
                                        },
                                        _ => {},
                                    }
                                }
                            }
                        }
                        // Mining is different since presumably you dig into the hillside
                        if c.rockiness > 0.7 && c.alt - chunk.alt > -10.0 {
                            rock_chunks += 1;
                        }
                    });
                }
            }
            let has_river = water_profile.has_river();
            let has_lake = water_profile.has_lake();
            let vegetation_implies_potable_water =
                chunk.tree_density > 0.4 && !matches!(chunk.get_biome(), BiomeKind::Swamp);
            let has_aquifer = chunk.rockiness > 1.2;
            let warm_or_firewood = chunk.temp > CONFIG.snow_temp || tree_chunks > 2;
            let has_potable_water =
                has_river || (has_lake && chunk.alt > 100.0) || vegetation_implies_potable_water;
            let has_building_materials = tree_chunks > 0
                || rock_chunks > 0
                || chunk.temp > CONFIG.tropical_temp && (has_river || has_lake);
            let water_rich = water_profile.is_water_rich();
            let can_grow_rice = water_rich
                && chunk.humidity + 1.0 > CONFIG.jungle_hum
                && chunk.temp + 1.0 > CONFIG.tropical_temp;
            let farming_score = if can_grow_rice {
                farmable_chunks * 2
            } else {
                farmable_chunks
            } + if water_rich {
                farmable_needs_irrigation_chunks
            } else {
                0
            };
            let fish_score = water_profile.fish_score();
            let food_potential = FoodPotential::from_scores(farming_score, fish_score);
            let mining_score = if tree_chunks > 1 { rock_chunks } else { 0 };
            let forestry_score = if has_river { tree_chunks } else { 0 };
            let extraction_potential = ExtractionPotential::from_scores(
                mining_score,
                forestry_score,
                has_building_materials,
            );
            let water_supply_potential =
                WaterSupplyPotential::from_facts(has_potable_water, has_aquifer);

            Self {
                town_resource_profile: TownResourceProfile {
                    food_potential,
                    extraction_potential,
                    water_supply_potential,
                    heating: warm_or_firewood,
                },
                trade_corridor_potential,
                port_potential,
            }
        })
    }

    pub(crate) fn score(&self) -> f32 {
        self.town_resource_profile.score()
            + (self.trade_corridor_potential.trading_score() as f32 + 1.0).log2()
    }

    pub(crate) fn output(self) -> TownEconomicOutput {
        TownEconomicOutput {
            farming_score: self.town_resource_profile.food_potential.farming_score(),
            fishing_score: self.town_resource_profile.food_potential.fishing_score(),
            mining_score: self
                .town_resource_profile
                .extraction_potential
                .mining_score(),
            forestry_score: self
                .town_resource_profile
                .extraction_potential
                .forestry_score(),
            building_materials: self
                .town_resource_profile
                .extraction_potential
                .has_building_materials(),
            potable_water: self
                .town_resource_profile
                .water_supply_potential
                .has_potable_water(),
            aquifer: self
                .town_resource_profile
                .water_supply_potential
                .has_aquifer(),
            heating: self.town_resource_profile.heating,
            trade_corridor_score: self.trade_corridor_potential.trading_score(),
            port_candidate: self.port_potential.is_candidate(),
        }
    }

    pub(crate) fn supports_site_kind(&self, site_kind: SiteKind) -> bool {
        self.town_resource_profile.supports_site_kind(site_kind)
            && (!matches!(site_kind, SiteKind::CoastalTown) || self.port_potential.is_candidate())
    }
}

impl TownResourceProfile {
    fn score(&self) -> f32 {
        3.0 * (self.food_potential.food_score() as f32 + 1.0).log2()
            + 2.0 * (self.extraction_potential.forestry_score() as f32 + 1.0).log2()
            + (self.extraction_potential.mining_score() as f32 + 1.0).log2()
    }

    fn supports_site_kind(&self, site_kind: SiteKind) -> bool {
        // aquifer and has_many_rocks was added to make mesa clifftowns suitable for
        // towns
        (self.water_supply_potential.has_potable_water()
            || (self.water_supply_potential.has_aquifer()
                && matches!(site_kind, SiteKind::CliffTown)))
            && self.extraction_potential.has_building_materials()
            && self.heating
    }
}

impl TownFoundationCandidate {
    pub(crate) fn at_site_loc(sim: &WorldSim, loc: Vec2<i32>) -> Option<Self> {
        let terrain = SiteTerrainContext::at_site_loc(sim, loc)?;
        Self::from_terrain(sim, loc, terrain)
    }

    pub(crate) fn from_terrain(
        sim: &WorldSim,
        loc: Vec2<i32>,
        terrain: SiteTerrainContext,
    ) -> Option<Self> {
        TownEconomicProfile::at_site_loc(sim, loc).map(|economic_profile| Self {
            terrain,
            economic_profile,
        })
    }

    pub(crate) fn score(self) -> f32 { self.economic_profile.score() }

    pub(crate) fn economic_output(self) -> TownEconomicOutput { self.economic_profile.output() }

    pub(crate) fn supports_site_kind(self, site_kind: SiteKind) -> bool {
        self.terrain.suitabilities.settlement.on_land
            && self.economic_profile.supports_site_kind(site_kind)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ExtractionPotential, FoodPotential, SiteTerrainContext, TownEconomicOutput,
        TownEconomicProfile, TownFoundationCandidate, TownResourceProfile, WaterSupplyPotential,
    };
    use crate::{
        all::ForestKind,
        sim::{
            marine_semantics::{WaterAccessClass, WaterBodyKind},
            site_suitability::{SuitabilitySet, WorldSemanticSample},
        },
        site::{
            SiteKind,
            coastal_suitability::{PortPotential, TradeCorridorPotential},
        },
    };
    use common::terrain::BiomeKind;

    fn terrain_context() -> SiteTerrainContext {
        let semantic_sample = WorldSemanticSample {
            alt: 128.0,
            water_level: 128.0,
            temp: 0.0,
            humidity: 0.5,
            biome: BiomeKind::Grassland,
            forest_kind: ForestKind::Oak,
            spawn_rate: 1.0,
            tree_density: 0.5,
            rockiness: 0.5,
            cliff_height: 0.0,
            gradient: Some(0.25),
            water_body_kind: WaterBodyKind::DryLand,
            marine_adjacent: false,
            water_access_class: WaterAccessClass::Inland,
            is_submerged: false,
            water_occupied: false,
        };

        SiteTerrainContext {
            suitabilities: SuitabilitySet::from_semantic_sample(semantic_sample),
            semantic_sample,
        }
    }

    #[test]
    fn coastal_town_support_requires_port_candidate() {
        let terrain = terrain_context();
        let profile = TownEconomicProfile {
            town_resource_profile: TownResourceProfile {
                food_potential: FoodPotential::from_scores(3, 0),
                extraction_potential: ExtractionPotential::from_scores(1, 1, true),
                water_supply_potential: WaterSupplyPotential::from_facts(true, false),
                heating: true,
            },
            trade_corridor_potential: TradeCorridorPotential::default(),
            port_potential: PortPotential {
                ocean_access: false,
                exposed_land_chunks: 2,
            },
        };

        assert!(!profile.supports_site_kind(SiteKind::CoastalTown));
        assert!(profile.supports_site_kind(SiteKind::Refactor));
        assert!(terrain.suitabilities.settlement.on_land);
    }

    #[test]
    fn town_score_consumes_trade_corridor_potential() {
        let base = TownEconomicProfile {
            town_resource_profile: TownResourceProfile {
                food_potential: FoodPotential::from_scores(3, 0),
                extraction_potential: ExtractionPotential::from_scores(1, 1, true),
                water_supply_potential: WaterSupplyPotential::from_facts(true, false),
                heating: true,
            },
            trade_corridor_potential: TradeCorridorPotential::default(),
            port_potential: PortPotential {
                ocean_access: true,
                exposed_land_chunks: 2,
            },
        };
        let trade_rich = TownEconomicProfile {
            trade_corridor_potential: TradeCorridorPotential::from_estuary_trade_mix_score(2),
            ..base
        };

        assert!(trade_rich.score() > base.score());
    }

    #[test]
    fn food_potential_preserves_farming_and_fishing_components() {
        let food = FoodPotential::from_scores(3, 2);
        assert_eq!(food.food_score(), 5);
    }

    #[test]
    fn town_score_consumes_food_potential() {
        let base = TownEconomicProfile {
            town_resource_profile: TownResourceProfile {
                food_potential: FoodPotential::from_scores(1, 0),
                extraction_potential: ExtractionPotential::from_scores(1, 1, true),
                water_supply_potential: WaterSupplyPotential::from_facts(true, false),
                heating: true,
            },
            trade_corridor_potential: TradeCorridorPotential::default(),
            port_potential: PortPotential {
                ocean_access: true,
                exposed_land_chunks: 2,
            },
        };
        let food_rich = TownEconomicProfile {
            town_resource_profile: TownResourceProfile {
                food_potential: FoodPotential::from_scores(3, 2),
                ..base.town_resource_profile
            },
            ..base
        };

        assert!(food_rich.score() > base.score());
    }

    #[test]
    fn extraction_potential_preserves_mining_forestry_and_materials() {
        let extraction = ExtractionPotential::from_scores(2, 3, true);
        assert_eq!(extraction.mining_score(), 2);
        assert_eq!(extraction.forestry_score(), 3);
        assert!(extraction.has_building_materials());
    }

    #[test]
    fn water_supply_potential_preserves_potable_water_and_aquifer() {
        let water_supply = WaterSupplyPotential::from_facts(true, false);
        assert!(water_supply.has_potable_water());
        assert!(!water_supply.has_aquifer());
    }

    #[test]
    fn town_score_consumes_extraction_potential() {
        let base = TownEconomicProfile {
            town_resource_profile: TownResourceProfile {
                food_potential: FoodPotential::from_scores(1, 0),
                extraction_potential: ExtractionPotential::from_scores(0, 0, true),
                water_supply_potential: WaterSupplyPotential::from_facts(true, false),
                heating: true,
            },
            trade_corridor_potential: TradeCorridorPotential::default(),
            port_potential: PortPotential {
                ocean_access: true,
                exposed_land_chunks: 2,
            },
        };
        let resource_rich = TownEconomicProfile {
            town_resource_profile: TownResourceProfile {
                extraction_potential: ExtractionPotential::from_scores(3, 2, true),
                ..base.town_resource_profile
            },
            ..base
        };

        assert!(resource_rich.score() > base.score());
    }

    #[test]
    fn town_foundation_candidate_preserves_economic_profile_score() {
        let terrain = terrain_context();
        let economic_profile = TownEconomicProfile {
            town_resource_profile: TownResourceProfile {
                food_potential: FoodPotential::from_scores(2, 1),
                extraction_potential: ExtractionPotential::from_scores(1, 1, true),
                water_supply_potential: WaterSupplyPotential::from_facts(true, false),
                heating: true,
            },
            trade_corridor_potential: TradeCorridorPotential::from_estuary_trade_mix_score(2),
            port_potential: PortPotential {
                ocean_access: true,
                exposed_land_chunks: 2,
            },
        };
        let candidate = TownFoundationCandidate {
            terrain,
            economic_profile,
        };

        assert_eq!(candidate.score(), economic_profile.score());
    }

    #[test]
    fn town_foundation_candidate_preserves_economic_profile_site_kind_support() {
        let terrain = terrain_context();
        let economic_profile = TownEconomicProfile {
            town_resource_profile: TownResourceProfile {
                food_potential: FoodPotential::from_scores(1, 0),
                extraction_potential: ExtractionPotential::from_scores(1, 1, true),
                water_supply_potential: WaterSupplyPotential::from_facts(false, true),
                heating: true,
            },
            trade_corridor_potential: TradeCorridorPotential::default(),
            port_potential: PortPotential {
                ocean_access: true,
                exposed_land_chunks: 2,
            },
        };
        let candidate = TownFoundationCandidate {
            terrain,
            economic_profile,
        };

        assert_eq!(
            candidate.supports_site_kind(SiteKind::CliffTown),
            economic_profile.supports_site_kind(SiteKind::CliffTown),
        );
        assert_eq!(
            candidate.supports_site_kind(SiteKind::Refactor),
            economic_profile.supports_site_kind(SiteKind::Refactor),
        );
    }

    #[test]
    fn cliff_town_support_allows_aquifer_without_potable_water() {
        let terrain = terrain_context();
        let profile = TownEconomicProfile {
            town_resource_profile: TownResourceProfile {
                food_potential: FoodPotential::from_scores(1, 0),
                extraction_potential: ExtractionPotential::from_scores(1, 1, true),
                water_supply_potential: WaterSupplyPotential::from_facts(false, true),
                heating: true,
            },
            trade_corridor_potential: TradeCorridorPotential::default(),
            port_potential: PortPotential {
                ocean_access: true,
                exposed_land_chunks: 2,
            },
        };

        assert!(profile.supports_site_kind(SiteKind::CliffTown));
        assert!(terrain.suitabilities.settlement.on_land);
    }

    #[test]
    fn non_cliff_town_support_requires_potable_water() {
        let terrain = terrain_context();
        let profile = TownEconomicProfile {
            town_resource_profile: TownResourceProfile {
                food_potential: FoodPotential::from_scores(1, 0),
                extraction_potential: ExtractionPotential::from_scores(1, 1, true),
                water_supply_potential: WaterSupplyPotential::from_facts(false, true),
                heating: true,
            },
            trade_corridor_potential: TradeCorridorPotential::default(),
            port_potential: PortPotential {
                ocean_access: true,
                exposed_land_chunks: 2,
            },
        };

        assert!(!profile.supports_site_kind(SiteKind::Refactor));
        assert!(terrain.suitabilities.settlement.on_land);
    }

    #[test]
    fn town_foundation_candidate_keeps_on_land_gate_outside_economic_profile() {
        let mut terrain = terrain_context();
        terrain.suitabilities.settlement.on_land = false;
        let economic_profile = TownEconomicProfile {
            town_resource_profile: TownResourceProfile {
                food_potential: FoodPotential::from_scores(2, 1),
                extraction_potential: ExtractionPotential::from_scores(1, 1, true),
                water_supply_potential: WaterSupplyPotential::from_facts(true, false),
                heating: true,
            },
            trade_corridor_potential: TradeCorridorPotential::default(),
            port_potential: PortPotential {
                ocean_access: true,
                exposed_land_chunks: 2,
            },
        };
        let candidate = TownFoundationCandidate {
            terrain,
            economic_profile,
        };

        assert!(economic_profile.supports_site_kind(SiteKind::Refactor));
        assert!(!candidate.supports_site_kind(SiteKind::Refactor));
    }

    #[test]
    fn town_economic_output_preserves_raw_components() {
        let profile = TownEconomicProfile {
            town_resource_profile: TownResourceProfile {
                food_potential: FoodPotential::from_scores(3, 2),
                extraction_potential: ExtractionPotential::from_scores(4, 1, true),
                water_supply_potential: WaterSupplyPotential::from_facts(true, false),
                heating: true,
            },
            trade_corridor_potential: TradeCorridorPotential::from_estuary_trade_mix_score(2),
            port_potential: PortPotential {
                ocean_access: true,
                exposed_land_chunks: 2,
            },
        };

        assert_eq!(profile.output(), TownEconomicOutput {
            farming_score: 3,
            fishing_score: 2,
            mining_score: 4,
            forestry_score: 1,
            building_materials: true,
            potable_water: true,
            aquifer: false,
            heating: true,
            trade_corridor_score: 2,
            port_candidate: true,
        },);
    }

    #[test]
    fn town_foundation_candidate_preserves_economic_output() {
        let terrain = terrain_context();
        let economic_profile = TownEconomicProfile {
            town_resource_profile: TownResourceProfile {
                food_potential: FoodPotential::from_scores(2, 1),
                extraction_potential: ExtractionPotential::from_scores(1, 3, true),
                water_supply_potential: WaterSupplyPotential::from_facts(true, true),
                heating: false,
            },
            trade_corridor_potential: TradeCorridorPotential::from_estuary_trade_mix_score(1),
            port_potential: PortPotential {
                ocean_access: true,
                exposed_land_chunks: 1,
            },
        };
        let candidate = TownFoundationCandidate {
            terrain,
            economic_profile,
        };

        assert_eq!(candidate.economic_output(), economic_profile.output());
    }

    #[test]
    fn town_resource_profile_preserves_score_components() {
        let base = TownResourceProfile {
            food_potential: FoodPotential::from_scores(1, 0),
            extraction_potential: ExtractionPotential::from_scores(0, 0, true),
            water_supply_potential: WaterSupplyPotential::from_facts(true, false),
            heating: true,
        };
        let richer = TownResourceProfile {
            food_potential: FoodPotential::from_scores(3, 2),
            extraction_potential: ExtractionPotential::from_scores(3, 2, true),
            ..base
        };

        assert!(richer.score() > base.score());
    }

    #[test]
    fn town_resource_profile_support_requires_materials_and_heating() {
        let cliff_town_ready = TownResourceProfile {
            food_potential: FoodPotential::from_scores(1, 0),
            extraction_potential: ExtractionPotential::from_scores(1, 1, true),
            water_supply_potential: WaterSupplyPotential::from_facts(false, true),
            heating: true,
        };
        let no_materials = TownResourceProfile {
            extraction_potential: ExtractionPotential::from_scores(1, 1, false),
            ..cliff_town_ready
        };
        let no_heating = TownResourceProfile {
            heating: false,
            ..cliff_town_ready
        };

        assert!(cliff_town_ready.supports_site_kind(SiteKind::CliffTown));
        assert!(!no_materials.supports_site_kind(SiteKind::CliffTown));
        assert!(!no_heating.supports_site_kind(SiteKind::CliffTown));
    }
}
