use crate::recipe::TopologyId;
use common::terrain::{MapSizeLg, vec2_as_uniform_idx};
use vek::*;

#[derive(Clone, Copy, Debug)]
pub(crate) struct WorldTopology {
    id: TopologyId,
    map_size_lg: MapSizeLg,
}

impl WorldTopology {
    pub(crate) const fn new(id: TopologyId, map_size_lg: MapSizeLg) -> Self {
        Self { id, map_size_lg }
    }

    pub(crate) fn normalize_chunk(self, chunk_pos: Vec2<i32>) -> Option<Vec2<i32>> {
        let world_size = self.map_size_lg.chunks().map(i32::from);
        match self.id {
            TopologyId::BoundedPlaneV1 => self
                .map_size_lg
                .contains_chunk(chunk_pos)
                .then_some(chunk_pos),
            TopologyId::WrapToroidalExpV1 => Some(Vec2::new(
                chunk_pos.x.rem_euclid(world_size.x),
                chunk_pos.y.rem_euclid(world_size.y),
            )),
            TopologyId::WrapCylindricalExpV1 => (chunk_pos.y >= 0 && chunk_pos.y < world_size.y)
                .then_some(Vec2::new(chunk_pos.x.rem_euclid(world_size.x), chunk_pos.y)),
        }
    }

    pub(crate) fn chunk_index(self, chunk_pos: Vec2<i32>) -> Option<usize> {
        self.normalize_chunk(chunk_pos)
            .map(|chunk_pos| vec2_as_uniform_idx(self.map_size_lg, chunk_pos))
    }

    pub(crate) fn local_chunks(
        self,
        chunk_pos: Vec2<i32>,
        grid_radius: i32,
    ) -> impl Clone + Iterator<Item = Vec2<i32>> {
        let grid_bounds = 2 * grid_radius + 1;
        (0..grid_bounds * grid_bounds).filter_map(move |index| {
            self.normalize_chunk(Vec2::new(
                chunk_pos.x + (index % grid_bounds) - grid_radius,
                chunk_pos.y + (index / grid_bounds) - grid_radius,
            ))
        })
    }

    /// Mirrors `WorldSim::get_base_z`'s historical bounded-plane margin gate.
    /// This is intentionally narrower than a generic "full local grid is valid"
    /// check, because `get_base_z` tolerates filtered edge neighbors.
    pub(crate) fn supports_base_z_chunk(self, chunk_pos: Vec2<i32>) -> bool {
        let world_size = self.map_size_lg.chunks().map(i32::from);
        match self.id {
            TopologyId::BoundedPlaneV1 => chunk_pos
                .map2(world_size, |coord, size| coord > 0 && coord < size - 2)
                .reduce_and(),
            TopologyId::WrapToroidalExpV1 => true,
            TopologyId::WrapCylindricalExpV1 => chunk_pos.y > 0 && chunk_pos.y < world_size.y - 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map_size_lg() -> MapSizeLg { MapSizeLg::new(Vec2::new(4, 4)).unwrap() }

    #[test]
    fn bounded_plane_normalize_rejects_oob() {
        let topology = WorldTopology::new(TopologyId::BoundedPlaneV1, map_size_lg());

        assert_eq!(
            topology.normalize_chunk(Vec2::new(0, 0)),
            Some(Vec2::new(0, 0))
        );
        assert_eq!(topology.normalize_chunk(Vec2::new(-1, 0)), None);
        assert_eq!(topology.normalize_chunk(Vec2::new(0, 16)), None);
    }

    #[test]
    fn toroidal_normalize_wraps_both_axes() {
        let topology = WorldTopology::new(TopologyId::WrapToroidalExpV1, map_size_lg());

        assert_eq!(
            topology.normalize_chunk(Vec2::new(-1, 16)),
            Some(Vec2::new(15, 0))
        );
        assert_eq!(
            topology.normalize_chunk(Vec2::new(17, -2)),
            Some(Vec2::new(1, 14))
        );
    }

    #[test]
    fn cylindrical_normalize_wraps_x_only() {
        let topology = WorldTopology::new(TopologyId::WrapCylindricalExpV1, map_size_lg());

        assert_eq!(
            topology.normalize_chunk(Vec2::new(-1, 7)),
            Some(Vec2::new(15, 7))
        );
        assert_eq!(topology.normalize_chunk(Vec2::new(3, -1)), None);
        assert_eq!(topology.normalize_chunk(Vec2::new(3, 16)), None);
    }

    #[test]
    fn bounded_plane_supports_base_z_requires_margin() {
        let topology = WorldTopology::new(TopologyId::BoundedPlaneV1, map_size_lg());

        assert!(topology.supports_base_z_chunk(Vec2::new(1, 1)));
        assert!(topology.supports_base_z_chunk(Vec2::new(13, 13)));
        assert!(!topology.supports_base_z_chunk(Vec2::new(0, 1)));
        assert!(!topology.supports_base_z_chunk(Vec2::new(14, 13)));
    }
}
