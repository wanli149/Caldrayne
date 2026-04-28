#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CaveEntrancePotential {
    pub surface_cave_entrance: bool,
}

impl CaveEntrancePotential {
    pub(crate) fn from_world_facts(cliff_height: f32, water_occupied: bool) -> Self {
        Self {
            surface_cave_entrance: cliff_height > 0.0 && !water_occupied,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CaveEntrancePotential;

    #[test]
    fn cave_entrance_potential_requires_dry_cliff_exposure() {
        let dry_cliff = CaveEntrancePotential::from_world_facts(20.0, false);
        assert!(dry_cliff.surface_cave_entrance);

        let water_occupied_cliff = CaveEntrancePotential::from_world_facts(20.0, true);
        assert!(!water_occupied_cliff.surface_cave_entrance);

        let dry_flatland = CaveEntrancePotential::from_world_facts(0.0, false);
        assert!(!dry_flatland.surface_cave_entrance);
    }
}
