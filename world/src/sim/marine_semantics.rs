use super::{RiverData, SimChunk, WorldSim};
use crate::{column::ColumnSample, config::CONFIG};
use common::terrain::BiomeKind;
use vek::Vec2;

const RUNTIME_NEAR_WATER_DISTANCE: f32 = 1.0;
const RUNTIME_MARINE_ADJACENCY_DISTANCE: f32 = 30.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WaterBodyKind {
    DryLand,
    River,
    Lake,
    Ocean,
}

impl WaterBodyKind {
    pub(crate) fn from_river_data(river: &RiverData) -> Self {
        if river.is_ocean() {
            Self::Ocean
        } else if river.is_lake() {
            Self::Lake
        } else if river.is_river() {
            Self::River
        } else {
            Self::DryLand
        }
    }

    pub(crate) fn from_chunk(chunk: &SimChunk) -> Self { Self::from_river_data(&chunk.river) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CoastalZone {
    SubmergedOrTidal,
    HideoutShoreline,
    SettlementShoreline,
    Inland,
}

impl CoastalZone {
    pub(crate) fn from_water_level(water_level: f32) -> Self {
        let sea_level_delta = water_level - CONFIG.sea_level;

        if (2.0..3.5).contains(&sea_level_delta) {
            Self::SettlementShoreline
        } else if (0.5..3.5).contains(&sea_level_delta) {
            Self::HideoutShoreline
        } else if sea_level_delta < 0.5 {
            Self::SubmergedOrTidal
        } else {
            Self::Inland
        }
    }

    pub(crate) fn supports_settlement_site(self) -> bool {
        matches!(self, Self::SettlementShoreline)
    }

    pub(crate) fn supports_hideout_site(self) -> bool {
        matches!(self, Self::HideoutShoreline | Self::SettlementShoreline)
    }

    pub(crate) fn from_semantic_facts(water_level: f32, marine_adjacent: bool) -> Self {
        if marine_adjacent {
            Self::from_water_level(water_level)
        } else {
            Self::Inland
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WaterAccessClass {
    Inland,
    FreshwaterShoreline,
    CoastalShoreline,
    FreshwaterSubmerged,
    MarineSubmerged,
}

impl WaterAccessClass {
    pub(crate) fn from_world_facts(
        water_body_kind: WaterBodyKind,
        is_submerged: bool,
        near_water: bool,
        water_level: f32,
    ) -> Self {
        Self::from_semantic_facts(
            water_body_kind,
            is_submerged,
            near_water,
            water_level,
            matches!(water_body_kind, WaterBodyKind::Ocean),
        )
    }

    pub(crate) fn from_semantic_facts(
        water_body_kind: WaterBodyKind,
        is_submerged: bool,
        near_water: bool,
        water_level: f32,
        marine_adjacent: bool,
    ) -> Self {
        if is_submerged {
            return match water_body_kind {
                WaterBodyKind::Ocean => Self::MarineSubmerged,
                WaterBodyKind::DryLand | WaterBodyKind::River | WaterBodyKind::Lake => {
                    Self::FreshwaterSubmerged
                },
            };
        }

        if near_water && matches!(water_body_kind, WaterBodyKind::River | WaterBodyKind::Lake) {
            Self::FreshwaterShoreline
        } else if near_water
            && marine_adjacent
            && matches!(
                water_body_kind,
                WaterBodyKind::DryLand | WaterBodyKind::Ocean
            )
            && matches!(
                CoastalZone::from_water_level(water_level),
                CoastalZone::HideoutShoreline | CoastalZone::SettlementShoreline
            )
        {
            Self::CoastalShoreline
        } else if near_water {
            Self::FreshwaterShoreline
        } else {
            Self::Inland
        }
    }

    pub(crate) fn from_column_sample(col: &ColumnSample<'_>) -> Self {
        Self::from_semantic_facts(
            WaterBodyKind::from_chunk(col.chunk),
            col.water_level > col.alt,
            runtime_near_water(col.water_dist),
            col.water_level,
            col.marine_adjacent,
        )
    }

    pub(crate) fn blocks_inland_site(self) -> bool {
        matches!(
            self,
            Self::FreshwaterShoreline | Self::FreshwaterSubmerged | Self::MarineSubmerged
        )
    }
}

pub(crate) fn marine_adjacency_at_site(sim: &WorldSim, loc: Vec2<i32>, center_alt: f32) -> bool {
    (-1..=1)
        .flat_map(|x| (-1..=1).map(move |y| Vec2::new(x, y)))
        .any(|offset| {
            let check_loc = loc + offset;
            sim.get(check_loc).is_some_and(|chunk| {
                num::abs(center_alt - chunk.alt) < 200.0
                    && matches!(WaterBodyKind::from_chunk(chunk), WaterBodyKind::Ocean)
            })
        })
}

pub(crate) fn marine_adjacent_from_ocean_distance(ocean_dist: Option<f32>) -> bool {
    ocean_dist.is_some_and(|dist| dist < RUNTIME_MARINE_ADJACENCY_DISTANCE)
}

fn runtime_near_water(water_dist: Option<f32>) -> bool {
    water_dist.is_some_and(|dist| dist < RUNTIME_NEAR_WATER_DISTANCE)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OceanDepthBand {
    NonOcean,
    LittoralShelf,
    SahaginShelf,
    OtherOcean,
}

impl OceanDepthBand {
    pub(crate) fn from_world_facts(biome: BiomeKind, alt: f32) -> Self {
        if !matches!(biome, BiomeKind::Ocean) {
            return Self::NonOcean;
        }

        let depth_below_sea_level = CONFIG.sea_level - alt;
        if depth_below_sea_level < 1.0 {
            Self::LittoralShelf
        } else if (40.0..45.0).contains(&depth_below_sea_level) {
            Self::SahaginShelf
        } else {
            Self::OtherOcean
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MarineEcologyProfile {
    pub coastal_zone: CoastalZone,
    pub depth_band: OceanDepthBand,
}

impl MarineEcologyProfile {
    pub(crate) fn from_world_facts(
        water_level: f32,
        marine_adjacent: bool,
        biome: BiomeKind,
        alt: f32,
    ) -> Self {
        Self {
            coastal_zone: CoastalZone::from_semantic_facts(water_level, marine_adjacent),
            depth_band: OceanDepthBand::from_world_facts(biome, alt),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AquaticSpawnPotential {
    pub freshwater_shoreline: bool,
    pub river_channel: bool,
    pub lake_water: bool,
    pub coastal_shoreline: bool,
    pub submerged_freshwater: bool,
    pub submerged_marine: bool,
    pub open_ocean: bool,
}

impl AquaticSpawnPotential {
    pub(crate) fn from_semantic_facts(
        water_body_kind: WaterBodyKind,
        water_access_class: WaterAccessClass,
    ) -> Self {
        Self {
            freshwater_shoreline: matches!(
                water_access_class,
                WaterAccessClass::FreshwaterShoreline
            ),
            river_channel: matches!(water_body_kind, WaterBodyKind::River),
            lake_water: matches!(water_body_kind, WaterBodyKind::Lake),
            coastal_shoreline: matches!(water_access_class, WaterAccessClass::CoastalShoreline),
            submerged_freshwater: matches!(
                water_access_class,
                WaterAccessClass::FreshwaterSubmerged
            ),
            submerged_marine: matches!(water_access_class, WaterAccessClass::MarineSubmerged),
            open_ocean: matches!(water_body_kind, WaterBodyKind::Ocean),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AquaticFaunaProfile {
    #[allow(
        dead_code,
        reason = "formal fauna profile is staged ahead of wider aquatic consumers"
    )]
    pub freshwater_fauna: bool,
    #[allow(
        dead_code,
        reason = "formal fauna profile is staged ahead of wider aquatic consumers"
    )]
    pub coastal_fauna: bool,
    pub shelf_fauna: bool,
    #[allow(
        dead_code,
        reason = "formal fauna profile is staged ahead of wider aquatic consumers"
    )]
    pub pelagic_fauna: bool,
}

impl AquaticFaunaProfile {
    pub(crate) fn from_profiles(
        aquatic_spawn: AquaticSpawnPotential,
        marine_ecology: MarineEcologyProfile,
    ) -> Self {
        let littoral_shelf = matches!(marine_ecology.depth_band, OceanDepthBand::LittoralShelf);
        let sahagin_shelf = matches!(marine_ecology.depth_band, OceanDepthBand::SahaginShelf);
        let submerged_marine_open_water =
            aquatic_spawn.submerged_marine && aquatic_spawn.open_ocean;

        Self {
            freshwater_fauna: aquatic_spawn.freshwater_shoreline
                || aquatic_spawn.river_channel
                || aquatic_spawn.lake_water
                || aquatic_spawn.submerged_freshwater,
            coastal_fauna: aquatic_spawn.coastal_shoreline
                || (littoral_shelf && submerged_marine_open_water),
            shelf_fauna: sahagin_shelf && submerged_marine_open_water,
            pelagic_fauna: matches!(marine_ecology.depth_band, OceanDepthBand::OtherOcean)
                && submerged_marine_open_water,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AquaticSpawnProfile {
    pub freshwater_shoreline: bool,
    pub coastal_shoreline: bool,
    pub ocean_surface: bool,
    pub open_ocean: bool,
}

impl AquaticSpawnProfile {
    pub(crate) fn from_column_sample(col: &ColumnSample<'_>) -> Self {
        Self::from_runtime_facts(
            col.water_dist,
            WaterBodyKind::from_chunk(col.chunk),
            WaterAccessClass::from_column_sample(col),
        )
    }

    fn from_runtime_facts(
        water_dist: Option<f32>,
        water_body_kind: WaterBodyKind,
        water_access_class: WaterAccessClass,
    ) -> Self {
        let open_ocean = matches!(water_body_kind, WaterBodyKind::Ocean);

        Self {
            freshwater_shoreline: matches!(
                water_access_class,
                WaterAccessClass::FreshwaterShoreline
            ),
            coastal_shoreline: matches!(water_access_class, WaterAccessClass::CoastalShoreline),
            open_ocean,
            ocean_surface: open_ocean && runtime_near_water(water_dist),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AquaticFaunaProfile, AquaticSpawnPotential, AquaticSpawnProfile, CoastalZone,
        MarineEcologyProfile, OceanDepthBand, WaterAccessClass, WaterBodyKind,
    };
    use crate::config::CONFIG;
    use common::terrain::BiomeKind;

    #[test]
    fn coastal_zone_preserves_existing_shoreline_bands() {
        assert_eq!(
            CoastalZone::from_water_level(CONFIG.sea_level + 0.25),
            CoastalZone::SubmergedOrTidal
        );
        assert_eq!(
            CoastalZone::from_water_level(CONFIG.sea_level + 1.5),
            CoastalZone::HideoutShoreline
        );
        assert_eq!(
            CoastalZone::from_water_level(CONFIG.sea_level + 2.5),
            CoastalZone::SettlementShoreline
        );
        assert_eq!(
            CoastalZone::from_water_level(CONFIG.sea_level + 10.0),
            CoastalZone::Inland
        );
        assert_eq!(
            CoastalZone::from_semantic_facts(CONFIG.sea_level + 2.5, false),
            CoastalZone::Inland
        );
    }

    #[test]
    fn coastal_zone_exact_boundaries_preserve_shoreline_band_handoffs() {
        assert_eq!(
            CoastalZone::from_water_level(CONFIG.sea_level + 0.5),
            CoastalZone::HideoutShoreline
        );
        assert_eq!(
            CoastalZone::from_water_level(CONFIG.sea_level + 2.0),
            CoastalZone::SettlementShoreline
        );
        assert_eq!(
            CoastalZone::from_water_level(CONFIG.sea_level + 3.5),
            CoastalZone::Inland
        );
    }

    #[test]
    fn water_access_class_distinguishes_inland_freshwater_coast_and_underwater() {
        assert_eq!(
            WaterAccessClass::from_world_facts(
                WaterBodyKind::DryLand,
                false,
                false,
                CONFIG.sea_level + 10.0,
            ),
            WaterAccessClass::Inland
        );
        assert_eq!(
            WaterAccessClass::from_world_facts(
                WaterBodyKind::DryLand,
                false,
                true,
                CONFIG.sea_level + 10.0,
            ),
            WaterAccessClass::FreshwaterShoreline
        );
        assert_eq!(
            WaterAccessClass::from_world_facts(
                WaterBodyKind::River,
                false,
                true,
                CONFIG.sea_level + 10.0,
            ),
            WaterAccessClass::FreshwaterShoreline
        );
        assert_eq!(
            WaterAccessClass::from_world_facts(
                WaterBodyKind::DryLand,
                false,
                true,
                CONFIG.sea_level + 2.5,
            ),
            WaterAccessClass::FreshwaterShoreline
        );
        assert_eq!(
            WaterAccessClass::from_world_facts(
                WaterBodyKind::DryLand,
                false,
                false,
                CONFIG.sea_level + 2.5,
            ),
            WaterAccessClass::Inland
        );
        assert_eq!(
            WaterAccessClass::from_world_facts(
                WaterBodyKind::Lake,
                false,
                true,
                CONFIG.sea_level + 2.5,
            ),
            WaterAccessClass::FreshwaterShoreline
        );
        assert_eq!(
            WaterAccessClass::from_world_facts(
                WaterBodyKind::River,
                false,
                true,
                CONFIG.sea_level + 2.5,
            ),
            WaterAccessClass::FreshwaterShoreline
        );
        assert_eq!(
            WaterAccessClass::from_world_facts(
                WaterBodyKind::Lake,
                true,
                true,
                CONFIG.sea_level + 10.0,
            ),
            WaterAccessClass::FreshwaterSubmerged
        );
        assert_eq!(
            WaterAccessClass::from_world_facts(WaterBodyKind::Ocean, true, true, CONFIG.sea_level,),
            WaterAccessClass::MarineSubmerged
        );
        assert_eq!(
            WaterAccessClass::from_semantic_facts(
                WaterBodyKind::DryLand,
                false,
                true,
                CONFIG.sea_level + 2.5,
                false,
            ),
            WaterAccessClass::FreshwaterShoreline
        );
        assert_eq!(
            WaterAccessClass::from_semantic_facts(
                WaterBodyKind::DryLand,
                false,
                true,
                CONFIG.sea_level + 2.5,
                true,
            ),
            WaterAccessClass::CoastalShoreline
        );
    }

    #[test]
    fn water_access_class_preserves_submerged_priority_and_marine_shoreline_requirements() {
        assert_eq!(
            WaterAccessClass::from_semantic_facts(
                WaterBodyKind::DryLand,
                true,
                true,
                CONFIG.sea_level + 2.5,
                true,
            ),
            WaterAccessClass::FreshwaterSubmerged
        );
        assert_eq!(
            WaterAccessClass::from_semantic_facts(
                WaterBodyKind::DryLand,
                false,
                true,
                CONFIG.sea_level + 2.0,
                true,
            ),
            WaterAccessClass::CoastalShoreline
        );
        assert_eq!(
            WaterAccessClass::from_semantic_facts(
                WaterBodyKind::DryLand,
                false,
                true,
                CONFIG.sea_level + 2.0,
                false,
            ),
            WaterAccessClass::FreshwaterShoreline
        );
        assert_eq!(
            WaterAccessClass::from_semantic_facts(
                WaterBodyKind::DryLand,
                false,
                true,
                CONFIG.sea_level + 10.0,
                true,
            ),
            WaterAccessClass::FreshwaterShoreline
        );
    }

    #[test]
    fn ocean_depth_band_preserves_current_site_thresholds() {
        assert_eq!(
            OceanDepthBand::from_world_facts(BiomeKind::Ocean, CONFIG.sea_level),
            OceanDepthBand::LittoralShelf
        );
        assert_eq!(
            OceanDepthBand::from_world_facts(BiomeKind::Ocean, CONFIG.sea_level - 42.0),
            OceanDepthBand::SahaginShelf
        );
        assert_eq!(
            OceanDepthBand::from_world_facts(BiomeKind::Forest, CONFIG.sea_level),
            OceanDepthBand::NonOcean
        );
    }

    #[test]
    fn ocean_depth_band_exact_boundaries_preserve_littoral_and_sahagin_ranges() {
        assert_eq!(
            OceanDepthBand::from_world_facts(BiomeKind::Ocean, CONFIG.sea_level - 0.99),
            OceanDepthBand::LittoralShelf
        );
        assert_eq!(
            OceanDepthBand::from_world_facts(BiomeKind::Ocean, CONFIG.sea_level - 1.0),
            OceanDepthBand::OtherOcean
        );
        assert_eq!(
            OceanDepthBand::from_world_facts(BiomeKind::Ocean, CONFIG.sea_level - 40.0),
            OceanDepthBand::SahaginShelf
        );
        assert_eq!(
            OceanDepthBand::from_world_facts(BiomeKind::Ocean, CONFIG.sea_level - 45.0),
            OceanDepthBand::OtherOcean
        );
    }

    #[test]
    fn marine_ecology_profile_preserves_current_depth_bands() {
        let littoral = MarineEcologyProfile::from_world_facts(
            CONFIG.sea_level,
            true,
            BiomeKind::Ocean,
            CONFIG.sea_level,
        );
        assert_ne!(littoral.depth_band, OceanDepthBand::NonOcean);
        assert_eq!(littoral.depth_band, OceanDepthBand::LittoralShelf);

        let sahagin = MarineEcologyProfile::from_world_facts(
            CONFIG.sea_level,
            true,
            BiomeKind::Ocean,
            CONFIG.sea_level - 42.0,
        );
        assert_ne!(sahagin.depth_band, OceanDepthBand::NonOcean);
        assert_eq!(sahagin.depth_band, OceanDepthBand::SahaginShelf);
    }

    #[test]
    fn marine_ecology_profile_combines_coast_and_depth_facts() {
        let littoral = MarineEcologyProfile::from_world_facts(
            CONFIG.sea_level + 2.5,
            true,
            BiomeKind::Ocean,
            CONFIG.sea_level,
        );
        assert_eq!(littoral.coastal_zone, CoastalZone::SettlementShoreline);
        assert_eq!(littoral.depth_band, OceanDepthBand::LittoralShelf);
    }

    #[test]
    fn aquatic_spawn_profile_separates_freshwater_beach_surface_and_open_ocean() {
        let freshwater = AquaticSpawnProfile::from_runtime_facts(
            Some(0.5),
            WaterBodyKind::River,
            WaterAccessClass::FreshwaterShoreline,
        );
        assert!(freshwater.freshwater_shoreline);
        assert!(!freshwater.coastal_shoreline);
        assert!(!freshwater.ocean_surface);
        assert!(!freshwater.open_ocean);

        let far_ocean = AquaticSpawnProfile::from_runtime_facts(
            Some(10.0),
            WaterBodyKind::Ocean,
            WaterAccessClass::MarineSubmerged,
        );
        assert!(!far_ocean.freshwater_shoreline);
        assert!(far_ocean.open_ocean);
        assert!(!far_ocean.ocean_surface);
        assert!(!far_ocean.coastal_shoreline);

        let ocean_surface = AquaticSpawnProfile::from_runtime_facts(
            Some(0.5),
            WaterBodyKind::Ocean,
            WaterAccessClass::MarineSubmerged,
        );
        assert!(!ocean_surface.freshwater_shoreline);
        assert!(ocean_surface.open_ocean);
        assert!(ocean_surface.ocean_surface);
        assert!(!ocean_surface.coastal_shoreline);

        let beach = AquaticSpawnProfile::from_runtime_facts(
            Some(10.0),
            WaterBodyKind::DryLand,
            WaterAccessClass::CoastalShoreline,
        );
        assert!(!beach.freshwater_shoreline);
        assert!(!beach.open_ocean);
        assert!(!beach.ocean_surface);
        assert!(beach.coastal_shoreline);

        let low_alt_lake = AquaticSpawnProfile::from_runtime_facts(
            Some(10.0),
            WaterBodyKind::Lake,
            WaterAccessClass::FreshwaterShoreline,
        );
        assert!(low_alt_lake.freshwater_shoreline);
        assert!(!low_alt_lake.coastal_shoreline);
    }

    #[test]
    fn aquatic_spawn_potential_distinguishes_static_water_spaces() {
        let river = AquaticSpawnPotential::from_semantic_facts(
            WaterBodyKind::River,
            WaterAccessClass::FreshwaterShoreline,
        );
        assert!(river.freshwater_shoreline);
        assert!(river.river_channel);
        assert!(!river.lake_water);
        assert!(!river.coastal_shoreline);
        assert!(!river.submerged_freshwater);
        assert!(!river.submerged_marine);
        assert!(!river.open_ocean);

        let lake = AquaticSpawnPotential::from_semantic_facts(
            WaterBodyKind::Lake,
            WaterAccessClass::FreshwaterSubmerged,
        );
        assert!(!lake.freshwater_shoreline);
        assert!(!lake.river_channel);
        assert!(lake.lake_water);
        assert!(!lake.coastal_shoreline);
        assert!(lake.submerged_freshwater);
        assert!(!lake.submerged_marine);
        assert!(!lake.open_ocean);

        let coast = AquaticSpawnPotential::from_semantic_facts(
            WaterBodyKind::DryLand,
            WaterAccessClass::CoastalShoreline,
        );
        assert!(!coast.freshwater_shoreline);
        assert!(!coast.river_channel);
        assert!(!coast.lake_water);
        assert!(coast.coastal_shoreline);
        assert!(!coast.submerged_freshwater);
        assert!(!coast.submerged_marine);
        assert!(!coast.open_ocean);

        let ocean = AquaticSpawnPotential::from_semantic_facts(
            WaterBodyKind::Ocean,
            WaterAccessClass::MarineSubmerged,
        );
        assert!(!ocean.freshwater_shoreline);
        assert!(!ocean.river_channel);
        assert!(!ocean.lake_water);
        assert!(!ocean.coastal_shoreline);
        assert!(!ocean.submerged_freshwater);
        assert!(ocean.submerged_marine);
        assert!(ocean.open_ocean);
    }

    #[test]
    fn aquatic_fauna_profile_projects_shared_water_spaces_into_habitats() {
        let freshwater = AquaticFaunaProfile::from_profiles(
            AquaticSpawnPotential::from_semantic_facts(
                WaterBodyKind::River,
                WaterAccessClass::FreshwaterShoreline,
            ),
            MarineEcologyProfile::from_world_facts(
                CONFIG.sea_level + 40.0,
                false,
                BiomeKind::Lake,
                CONFIG.sea_level + 20.0,
            ),
        );
        assert!(freshwater.freshwater_fauna);
        assert!(!freshwater.coastal_fauna);
        assert!(!freshwater.shelf_fauna);
        assert!(!freshwater.pelagic_fauna);

        let coastal = AquaticFaunaProfile::from_profiles(
            AquaticSpawnPotential::from_semantic_facts(
                WaterBodyKind::Ocean,
                WaterAccessClass::MarineSubmerged,
            ),
            MarineEcologyProfile::from_world_facts(
                CONFIG.sea_level,
                true,
                BiomeKind::Ocean,
                CONFIG.sea_level,
            ),
        );
        assert!(!coastal.freshwater_fauna);
        assert!(coastal.coastal_fauna);
        assert!(!coastal.shelf_fauna);
        assert!(!coastal.pelagic_fauna);

        let sahagin = AquaticFaunaProfile::from_profiles(
            AquaticSpawnPotential::from_semantic_facts(
                WaterBodyKind::Ocean,
                WaterAccessClass::MarineSubmerged,
            ),
            MarineEcologyProfile::from_world_facts(
                CONFIG.sea_level,
                true,
                BiomeKind::Ocean,
                CONFIG.sea_level - 42.0,
            ),
        );
        assert!(!sahagin.freshwater_fauna);
        assert!(!sahagin.coastal_fauna);
        assert!(sahagin.shelf_fauna);
        assert!(!sahagin.pelagic_fauna);

        let pelagic = AquaticFaunaProfile::from_profiles(
            AquaticSpawnPotential::from_semantic_facts(
                WaterBodyKind::Ocean,
                WaterAccessClass::MarineSubmerged,
            ),
            MarineEcologyProfile::from_world_facts(
                CONFIG.sea_level,
                true,
                BiomeKind::Ocean,
                CONFIG.sea_level - 10.0,
            ),
        );
        assert!(!pelagic.freshwater_fauna);
        assert!(!pelagic.coastal_fauna);
        assert!(!pelagic.shelf_fauna);
        assert!(pelagic.pelagic_fauna);
    }

    #[test]
    fn marine_adjacent_distance_uses_explicit_ocean_proximity() {
        assert!(super::marine_adjacent_from_ocean_distance(Some(10.0)));
        assert!(!super::marine_adjacent_from_ocean_distance(Some(35.0)));
        assert!(!super::marine_adjacent_from_ocean_distance(None));
    }
}
