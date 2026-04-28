use crate::sim::{WorldSim, marine_semantics::WaterBodyKind, site_suitability::is_water_occupied};
use common::terrain::CoordinateConversions;
use vek::Vec2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CoastalNeighborhoodProfile {
    pub ocean_access: bool,
    pub freshwater_access: bool,
    pub exposed_land_chunks: i32,
    pub estuary_trade_mix_score: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PortPotential {
    pub ocean_access: bool,
    pub exposed_land_chunks: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TradeCorridorPotential {
    estuary_trade_mix_score: i32,
}

impl PortPotential {
    pub(crate) fn is_candidate(self) -> bool { self.ocean_access && self.exposed_land_chunks > 0 }
}

impl TradeCorridorPotential {
    pub(crate) fn from_estuary_trade_mix_score(estuary_trade_mix_score: i32) -> Self {
        Self {
            estuary_trade_mix_score,
        }
    }

    pub(crate) fn trading_score(self) -> i32 { self.estuary_trade_mix_score }
}

impl CoastalNeighborhoodProfile {
    pub(crate) fn port_potential(self) -> PortPotential {
        PortPotential {
            ocean_access: self.ocean_access,
            exposed_land_chunks: self.exposed_land_chunks,
        }
    }

    pub(crate) fn trade_corridor_potential(self) -> TradeCorridorPotential {
        TradeCorridorPotential::from_estuary_trade_mix_score(self.estuary_trade_mix_score)
    }

    pub(crate) fn hideout_candidate(self) -> bool { self.ocean_access }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WaterNeighborhoodProfile {
    river_chunks: i32,
    lake_chunks: i32,
    ocean_chunks: i32,
    exposed_land_chunks: i32,
}

impl WaterNeighborhoodProfile {
    const RESOURCE_RADIUS: i32 = 1;

    fn neighborhood_offsets() -> impl Iterator<Item = Vec2<i32>> {
        (-Self::RESOURCE_RADIUS..=Self::RESOURCE_RADIUS).flat_map(|x| {
            (-Self::RESOURCE_RADIUS..=Self::RESOURCE_RADIUS).map(move |y| Vec2::new(x, y))
        })
    }

    pub(crate) fn collect(sim: &WorldSim, loc: Vec2<i32>, center_alt: f32) -> Self {
        let mut profile = Self::default();
        for offset in Self::neighborhood_offsets() {
            let check_loc = loc + offset.cpos_to_wpos();
            if let Some(chunk) = sim.get(check_loc) {
                if num::abs(center_alt - chunk.alt) < 200.0 {
                    profile.observe(
                        WaterBodyKind::from_chunk(chunk),
                        chunk.water_alt > chunk.alt,
                    );
                }
            }
        }
        profile
    }

    fn observe(&mut self, water_body_kind: WaterBodyKind, is_submerged: bool) {
        match water_body_kind {
            WaterBodyKind::DryLand => {
                if !is_water_occupied(water_body_kind, is_submerged) {
                    self.exposed_land_chunks += 1;
                }
            },
            WaterBodyKind::River => self.river_chunks += 1,
            WaterBodyKind::Lake => self.lake_chunks += 1,
            WaterBodyKind::Ocean => self.ocean_chunks += 1,
        }
    }

    pub(crate) fn has_river(self) -> bool { self.river_chunks > 1 }

    pub(crate) fn has_lake(self) -> bool { self.lake_chunks > 1 }

    pub(crate) fn fish_score(self) -> i32 { self.lake_chunks + self.ocean_chunks }

    pub(crate) fn is_water_rich(self) -> bool { self.lake_chunks + self.river_chunks > 2 }

    pub(crate) fn coastal_profile(self) -> CoastalNeighborhoodProfile {
        CoastalNeighborhoodProfile {
            ocean_access: self.ocean_chunks > 0,
            freshwater_access: self.has_river() || self.has_lake(),
            exposed_land_chunks: self.exposed_land_chunks,
            estuary_trade_mix_score: self.estuary_trade_mix_score(),
        }
    }

    fn estuary_trade_mix_score(self) -> i32 {
        std::cmp::min(
            std::cmp::min(self.exposed_land_chunks, self.ocean_chunks),
            self.river_chunks,
        )
    }
}

pub(crate) fn coastal_profile_at_site(
    sim: &WorldSim,
    loc: Vec2<i32>,
) -> Option<CoastalNeighborhoodProfile> {
    let center_alt = sim.get(loc)?.alt;
    Some(WaterNeighborhoodProfile::collect(sim, loc, center_alt).coastal_profile())
}

#[cfg(test)]
mod tests {
    use super::{
        CoastalNeighborhoodProfile, PortPotential, TradeCorridorPotential, WaterNeighborhoodProfile,
    };
    use crate::sim::marine_semantics::WaterBodyKind;
    use vek::Vec2;

    #[test]
    fn port_potential_requires_ocean_access_and_exposed_land() {
        assert!(
            !PortPotential {
                ocean_access: false,
                exposed_land_chunks: 2,
            }
            .is_candidate()
        );

        assert!(
            !PortPotential {
                ocean_access: true,
                exposed_land_chunks: 0,
            }
            .is_candidate()
        );

        assert!(
            PortPotential {
                ocean_access: true,
                exposed_land_chunks: 1,
            }
            .is_candidate()
        );
    }

    #[test]
    fn coastal_profile_derives_port_potential_without_trade_fields() {
        let profile = CoastalNeighborhoodProfile {
            ocean_access: true,
            freshwater_access: true,
            exposed_land_chunks: 3,
            estuary_trade_mix_score: 2,
        };

        assert_eq!(profile.port_potential(), PortPotential {
            ocean_access: true,
            exposed_land_chunks: 3,
        });
    }

    #[test]
    fn coastal_profile_derives_trade_corridor_potential_without_port_fields() {
        let profile = CoastalNeighborhoodProfile {
            ocean_access: true,
            freshwater_access: true,
            exposed_land_chunks: 3,
            estuary_trade_mix_score: 2,
        };

        assert_eq!(profile.trade_corridor_potential(), TradeCorridorPotential {
            estuary_trade_mix_score: 2,
        });
        assert_eq!(profile.trade_corridor_potential().trading_score(), 2);
    }

    #[test]
    fn hideout_candidate_tracks_ocean_access() {
        assert!(
            !CoastalNeighborhoodProfile {
                ocean_access: false,
                freshwater_access: true,
                exposed_land_chunks: 4,
                estuary_trade_mix_score: 0,
            }
            .hideout_candidate()
        );

        assert!(
            CoastalNeighborhoodProfile {
                ocean_access: true,
                freshwater_access: false,
                exposed_land_chunks: 0,
                estuary_trade_mix_score: 0,
            }
            .hideout_candidate()
        );
    }

    #[test]
    fn water_profile_preserves_exposed_land_and_estuary_mix() {
        let mut profile = WaterNeighborhoodProfile::default();
        profile.observe(WaterBodyKind::DryLand, false);
        profile.observe(WaterBodyKind::DryLand, true);
        profile.observe(WaterBodyKind::River, false);
        profile.observe(WaterBodyKind::River, false);
        profile.observe(WaterBodyKind::Ocean, false);

        let coastal = profile.coastal_profile();
        assert!(coastal.ocean_access);
        assert!(coastal.freshwater_access);
        assert_eq!(coastal.exposed_land_chunks, 1);
        assert_eq!(coastal.estuary_trade_mix_score, 1);
        assert_eq!(coastal.trade_corridor_potential().trading_score(), 1);
        assert_eq!(profile.fish_score(), 1);
    }

    #[test]
    fn neighborhood_offsets_are_symmetric_and_include_center() {
        let offsets = WaterNeighborhoodProfile::neighborhood_offsets().collect::<Vec<_>>();

        assert_eq!(offsets.len(), 9);
        assert!(offsets.contains(&Vec2::new(0, 0)));
        assert!(offsets.contains(&Vec2::new(-1, -1)));
        assert!(offsets.contains(&Vec2::new(-1, 1)));
        assert!(offsets.contains(&Vec2::new(1, -1)));
        assert!(offsets.contains(&Vec2::new(1, 1)));
    }
}
