use crate::{CONFIG, recipe::TopologyId};
use common::terrain::{MapSizeLg, TerrainChunk, vec2_as_uniform_idx};
use vek::*;

#[derive(Clone, Copy, Debug)]
pub(crate) struct WorldTopology {
    id: TopologyId,
    map_size_lg: MapSizeLg,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DefaultChunkKind {
    BoundedOcean,
}

impl DefaultChunkKind {
    pub(crate) fn build(self) -> TerrainChunk {
        match self {
            Self::BoundedOcean => TerrainChunk::water(CONFIG.sea_level as i32),
        }
    }
}

impl WorldTopology {
    pub(crate) const fn new(id: TopologyId, map_size_lg: MapSizeLg) -> Self {
        Self { id, map_size_lg }
    }

    #[allow(dead_code)]
    pub(crate) const fn wraps_x(self) -> bool {
        matches!(
            self.id,
            TopologyId::WrapToroidalExpV1 | TopologyId::WrapCylindricalExpV1
        )
    }

    #[allow(dead_code)]
    pub(crate) const fn wraps_y(self) -> bool { matches!(self.id, TopologyId::WrapToroidalExpV1) }

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

    pub(crate) fn chunk_aabr(self) -> Aabr<i32> {
        let size = self.map_size_lg.chunks().map(i32::from);
        Aabr {
            min: Vec2::zero(),
            max: size,
        }
    }

    pub(crate) fn chunk_key_aabr(self) -> Aabr<i32> {
        let size = self.map_size_lg.chunks().map(i32::from);
        Aabr {
            min: Vec2::zero(),
            max: size - 1,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn query_chunk_dimensions(self) -> Vec2<i32> { self.chunk_key_aabr().max + 1 }

    #[allow(dead_code)]
    pub(crate) fn normalize_query_chunk_key(self, chunk_key: Vec2<i32>) -> Option<Vec2<i32>> {
        self.normalize_chunk(chunk_key)
    }

    #[allow(dead_code)]
    pub(crate) fn query_chunk_key_delta(self, from: Vec2<i32>, to: Vec2<i32>) -> Option<Vec2<i32>> {
        let from = self.normalize_query_chunk_key(from)?;
        let to = self.normalize_query_chunk_key(to)?;
        let dims = self.query_chunk_dimensions();
        Some(Vec2::new(
            shortest_axis_delta(to.x - from.x, dims.x, self.wraps_x()),
            shortest_axis_delta(to.y - from.y, dims.y, self.wraps_y()),
        ))
    }

    /// Finite canonical chunk-key subdomain where runtime chunk generation is
    /// guaranteed to have a real non-default chunk product under the current
    /// topology/generation contract.
    pub(crate) fn runtime_chunk_product_key_aabr(self) -> Aabr<i32> {
        let size = self.map_size_lg.chunks().map(i32::from);
        match self.id {
            TopologyId::BoundedPlaneV1 => Aabr {
                min: Vec2::one(),
                max: size - Vec2::new(3, 3),
            },
            TopologyId::WrapToroidalExpV1 => self.chunk_key_aabr(),
            TopologyId::WrapCylindricalExpV1 => Aabr {
                min: Vec2::new(0, 1),
                max: Vec2::new(size.x - 1, size.y - 3),
            },
        }
    }

    pub(crate) fn contains_runtime_chunk_product_key(self, chunk_pos: Vec2<i32>) -> bool {
        let bounds = self.runtime_chunk_product_key_aabr();
        (bounds.min.x..=bounds.max.x).contains(&chunk_pos.x)
            && (bounds.min.y..=bounds.max.y).contains(&chunk_pos.y)
    }

    pub(crate) const fn default_chunk_kind(self) -> DefaultChunkKind {
        match self.id {
            TopologyId::BoundedPlaneV1
            | TopologyId::WrapToroidalExpV1
            | TopologyId::WrapCylindricalExpV1 => DefaultChunkKind::BoundedOcean,
        }
    }

    fn interpolation_anchor(self, chunk_pos: Vec2<f64>) -> Vec2<i32> {
        match self.id {
            // Preserve current bounded behavior while moving the anchor rule
            // behind the topology seam.
            TopologyId::BoundedPlaneV1 => chunk_pos.map(|coord| coord.max(0.0) as i32),
            TopologyId::WrapToroidalExpV1 | TopologyId::WrapCylindricalExpV1 => {
                chunk_pos.map(|coord| coord.floor() as i32)
            },
        }
    }

    pub(crate) fn interpolation_chunk(
        self,
        chunk_pos: Vec2<f64>,
        offset: Vec2<i32>,
    ) -> Option<Vec2<i32>> {
        self.normalize_chunk(self.interpolation_anchor(chunk_pos) + offset)
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
}

#[allow(dead_code)]
fn shortest_axis_delta(delta: i32, axis_len: i32, wraps: bool) -> i32 {
    if !wraps {
        return delta;
    }

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
    fn runtime_chunk_product_domain_matches_current_generation_margin_contract() {
        let topology = WorldTopology::new(TopologyId::BoundedPlaneV1, map_size_lg());

        assert_eq!(topology.runtime_chunk_product_key_aabr(), Aabr {
            min: Vec2::one(),
            max: Vec2::new(13, 13),
        });
        assert!(topology.contains_runtime_chunk_product_key(Vec2::new(1, 1)));
        assert!(topology.contains_runtime_chunk_product_key(Vec2::new(13, 13)));
        assert!(!topology.contains_runtime_chunk_product_key(Vec2::new(0, 1)));
        assert!(!topology.contains_runtime_chunk_product_key(Vec2::new(14, 13)));
    }

    #[test]
    fn runtime_chunk_product_domain_stays_finite_for_all_current_topologies() {
        let bounded = WorldTopology::new(TopologyId::BoundedPlaneV1, map_size_lg());
        let toroidal = WorldTopology::new(TopologyId::WrapToroidalExpV1, map_size_lg());
        let cylindrical = WorldTopology::new(TopologyId::WrapCylindricalExpV1, map_size_lg());

        assert_eq!(bounded.runtime_chunk_product_key_aabr(), Aabr {
            min: Vec2::one(),
            max: Vec2::new(13, 13),
        });
        assert_eq!(toroidal.runtime_chunk_product_key_aabr(), Aabr {
            min: Vec2::zero(),
            max: Vec2::new(15, 15),
        });
        assert_eq!(cylindrical.runtime_chunk_product_key_aabr(), Aabr {
            min: Vec2::new(0, 1),
            max: Vec2::new(15, 13),
        });
    }

    #[test]
    fn canonical_chunk_aabr_stays_finite_for_all_current_topologies() {
        let expected = Aabr {
            min: Vec2::zero(),
            max: Vec2::new(16, 16),
        };

        for topology_id in [
            TopologyId::BoundedPlaneV1,
            TopologyId::WrapToroidalExpV1,
            TopologyId::WrapCylindricalExpV1,
        ] {
            let topology = WorldTopology::new(topology_id, map_size_lg());
            assert_eq!(topology.chunk_aabr(), expected);
        }
    }

    #[test]
    fn canonical_chunk_key_aabr_stays_finite_for_all_current_topologies() {
        let expected = Aabr {
            min: Vec2::zero(),
            max: Vec2::new(15, 15),
        };

        for topology_id in [
            TopologyId::BoundedPlaneV1,
            TopologyId::WrapToroidalExpV1,
            TopologyId::WrapCylindricalExpV1,
        ] {
            let topology = WorldTopology::new(topology_id, map_size_lg());
            assert_eq!(topology.chunk_key_aabr(), expected);
        }
    }

    #[test]
    fn wrap_axes_match_current_topology_variants() {
        let bounded = WorldTopology::new(TopologyId::BoundedPlaneV1, map_size_lg());
        let toroidal = WorldTopology::new(TopologyId::WrapToroidalExpV1, map_size_lg());
        let cylindrical = WorldTopology::new(TopologyId::WrapCylindricalExpV1, map_size_lg());

        assert!(!bounded.wraps_x());
        assert!(!bounded.wraps_y());
        assert!(toroidal.wraps_x());
        assert!(toroidal.wraps_y());
        assert!(cylindrical.wraps_x());
        assert!(!cylindrical.wraps_y());
    }

    #[test]
    fn normalize_query_chunk_key_matches_canonical_topology_policy() {
        let bounded = WorldTopology::new(TopologyId::BoundedPlaneV1, map_size_lg());
        let toroidal = WorldTopology::new(TopologyId::WrapToroidalExpV1, map_size_lg());
        let cylindrical = WorldTopology::new(TopologyId::WrapCylindricalExpV1, map_size_lg());

        assert_eq!(bounded.normalize_query_chunk_key(Vec2::new(-1, 0)), None);
        assert_eq!(
            toroidal.normalize_query_chunk_key(Vec2::new(-1, 16)),
            Some(Vec2::new(15, 0))
        );
        assert_eq!(
            cylindrical.normalize_query_chunk_key(Vec2::new(-1, 7)),
            Some(Vec2::new(15, 7))
        );
        assert_eq!(
            cylindrical.normalize_query_chunk_key(Vec2::new(3, -1)),
            None
        );
    }

    #[test]
    fn query_chunk_key_delta_uses_shortest_wrapped_axis_distance() {
        let bounded = WorldTopology::new(TopologyId::BoundedPlaneV1, map_size_lg());
        let toroidal = WorldTopology::new(TopologyId::WrapToroidalExpV1, map_size_lg());
        let cylindrical = WorldTopology::new(TopologyId::WrapCylindricalExpV1, map_size_lg());

        assert_eq!(
            bounded.query_chunk_key_delta(Vec2::new(1, 1), Vec2::new(14, 1)),
            Some(Vec2::new(13, 0))
        );
        assert_eq!(
            toroidal.query_chunk_key_delta(Vec2::new(0, 0), Vec2::new(15, 15)),
            Some(Vec2::new(-1, -1))
        );
        assert_eq!(
            cylindrical.query_chunk_key_delta(Vec2::new(0, 7), Vec2::new(15, 7)),
            Some(Vec2::new(-1, 0))
        );
        assert_eq!(
            cylindrical.query_chunk_key_delta(Vec2::new(0, 0), Vec2::new(15, -1)),
            None
        );
    }

    #[test]
    fn default_chunk_kind_stays_bounded_ocean_for_all_current_topologies() {
        for topology_id in [
            TopologyId::BoundedPlaneV1,
            TopologyId::WrapToroidalExpV1,
            TopologyId::WrapCylindricalExpV1,
        ] {
            let topology = WorldTopology::new(topology_id, map_size_lg());
            assert_eq!(
                topology.default_chunk_kind(),
                DefaultChunkKind::BoundedOcean
            );
        }
    }

    #[test]
    fn bounded_interpolation_anchor_preserves_edge_clamp_behavior() {
        let topology = WorldTopology::new(TopologyId::BoundedPlaneV1, map_size_lg());

        assert_eq!(
            topology.interpolation_chunk(Vec2::new(-0.25, 7.5), Vec2::new(0, 0)),
            Some(Vec2::new(0, 7))
        );
        assert_eq!(
            topology.interpolation_chunk(Vec2::new(-0.25, 7.5), Vec2::new(-1, 0)),
            None
        );
    }

    #[test]
    fn wrapping_interpolation_chunk_normalizes_offsets() {
        let toroidal = WorldTopology::new(TopologyId::WrapToroidalExpV1, map_size_lg());
        let cylindrical = WorldTopology::new(TopologyId::WrapCylindricalExpV1, map_size_lg());

        assert_eq!(
            toroidal.interpolation_chunk(Vec2::new(-0.25, 16.1), Vec2::new(0, 0)),
            Some(Vec2::new(15, 0))
        );
        assert_eq!(
            cylindrical.interpolation_chunk(Vec2::new(-0.25, 7.5), Vec2::new(0, 0)),
            Some(Vec2::new(15, 7))
        );
        assert_eq!(
            cylindrical.interpolation_chunk(Vec2::new(3.25, -0.1), Vec2::new(0, 0)),
            None
        );
    }
}
