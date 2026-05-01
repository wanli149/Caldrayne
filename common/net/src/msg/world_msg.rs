use common::{grid::Grid, map::Marker, terrain::TerrainChunk, trade::Good};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use vek::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeTopologyDescriptor {
    /// Runtime topology identifier for the loaded world.
    pub topology_id: String,
    /// Canonical finite chunk-key query domain under the current topology
    /// policy. This does not encode runtime load state.
    pub query_chunk_key_aabr: Aabr<i32>,
    /// Finite chunk-key subdomain where runtime chunk generation is guaranteed
    /// to produce a real non-default chunk product under the current topology
    /// and generation contract.
    pub runtime_chunk_product_key_aabr: Aabr<i32>,
    /// Policy used when a runtime consumer has no world chunk product available
    /// under the current topology contract. This includes keys outside the
    /// canonical query domain and queryable keys that intentionally fall back
    /// to the default chunk product.
    pub missing_world_bounds_policy: MissingWorldBoundsPolicy,
}

impl RuntimeTopologyDescriptor {
    pub fn wraps_x(&self) -> bool {
        matches!(
            self.topology_id.as_str(),
            "wrap_toroidal_exp_v1" | "wrap_cylindrical_exp_v1"
        )
    }

    pub fn wraps_y(&self) -> bool { self.topology_id.as_str() == "wrap_toroidal_exp_v1" }

    pub fn query_chunk_dimensions(&self) -> Vec2<i32> {
        self.query_chunk_key_aabr.max - self.query_chunk_key_aabr.min + 1
    }

    pub fn contains_query_chunk_key(&self, chunk_key: Vec2<i32>) -> bool {
        let bounds = self.query_chunk_key_aabr;
        (bounds.min.x..=bounds.max.x).contains(&chunk_key.x)
            && (bounds.min.y..=bounds.max.y).contains(&chunk_key.y)
    }

    pub fn normalize_query_chunk_key(&self, chunk_key: Vec2<i32>) -> Option<Vec2<i32>> {
        let dims = self.query_chunk_dimensions();
        let min = self.query_chunk_key_aabr.min;
        let normalize_axis = |coord: i32, axis_min: i32, axis_len: i32, wraps: bool| {
            if wraps {
                Some((coord - axis_min).rem_euclid(axis_len) + axis_min)
            } else if (axis_min..axis_min + axis_len).contains(&coord) {
                Some(coord)
            } else {
                None
            }
        };

        Some(Vec2::new(
            normalize_axis(chunk_key.x, min.x, dims.x, self.wraps_x())?,
            normalize_axis(chunk_key.y, min.y, dims.y, self.wraps_y())?,
        ))
    }

    pub fn query_chunk_key_delta(&self, from: Vec2<i32>, to: Vec2<i32>) -> Option<Vec2<i32>> {
        let from = self.normalize_query_chunk_key(from)?;
        let to = self.normalize_query_chunk_key(to)?;
        let dims = self.query_chunk_dimensions();
        Some(Vec2::new(
            shortest_axis_delta(to.x - from.x, dims.x, self.wraps_x()),
            shortest_axis_delta(to.y - from.y, dims.y, self.wraps_y()),
        ))
    }

    pub fn contains_runtime_chunk_product_key(&self, chunk_key: Vec2<i32>) -> bool {
        let bounds = self.runtime_chunk_product_key_aabr;
        (bounds.min.x..=bounds.max.x).contains(&chunk_key.x)
            && (bounds.min.y..=bounds.max.y).contains(&chunk_key.y)
    }
}

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MissingWorldBoundsPolicy {
    BoundedOceanDefaultChunk,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// World map information.  Note that currently, we always send the whole thing
/// in one go, but the structure aims to try to provide information as locally
/// as possible, so that in the future we can split up large maps into multiple
/// WorldMapMsg fragments.
///
/// TODO: Update message format to make fragmentable, allowing us to send more
/// information without running into bandwidth issues.
///
/// TODO: Add information for rivers (currently, we just prerender them on the
/// server, but this is not a great solution for LoD.  The map rendering code is
/// already set up to be able to take advantage of the river rendering being
/// split out, but the format is a little complicated for space reasons and it
/// may take some tweaking to get right, so we avoid sending it for now).
///
/// TODO: measure explicit compression schemes that might save space, e.g.
/// repeating the "small angles" optimization that works well on more detailed
/// shadow maps intended for height maps.
pub struct WorldMapMsg {
    /// Log base 2 of world map dimensions (width × height) in chunks.
    ///
    /// NOTE: Invariant: chunk count fits in a u16.
    pub dimensions_lg: Vec2<u32>,
    /// Max height (used to scale altitudes).
    pub max_height: f32,
    /// RGB+A; the alpha channel is currently unused, but will be used in the
    /// future. Entries are in the usual chunk order.
    pub rgba: Grid<u32>,
    /// Altitudes: bits 2 to 0 are unused, then bits 15 to 3 are used for
    /// altitude. The remainder are currently unused, but we have plans to
    /// use 7 bits for water depth (using an integer f7 encoding), and we
    /// will find other uses for the remaining 12 bits.
    pub alt: Grid<u32>,
    /// Horizon mapping. This is a variant of shadow mapping that is
    /// specifically designed for height maps; it takes advantage of their
    /// regular structure (e.g. no holes) to compress all information needed
    /// to decide when to cast a sharp shadow into a single nagle, the "horizon
    /// angle." This is the smallest angle with the ground at which light can
    /// pass through any occluders to reach the chunk, in some chosen
    /// horizontal direction. This would not be sufficient for a more
    /// complicated 3D structure, but it works for height maps since:
    ///
    /// 1. they have no gaps, so as soon as light can shine through it will
    ///    always be able to do so, and
    /// 2. we only care about lighting from the top, and only from the east and
    ///    west (since at a large scale like this we mostly just want to handle
    ///    variable sunlight; moonlight would present more challenges but we
    ///    currently have no plans to try to cast accurate shadows in
    ///    moonlight).
    ///
    /// Our chosen format is two pairs of vectors,
    /// with the first pair representing west-facing light (casting shadows on
    /// the left side)  and the second representing east-facing light
    /// (casting shadows on the east side).
    ///
    /// The pair of vectors consists of (with each vector in the usual chunk
    /// order):
    ///
    /// * Horizon angle pointing east (1 byte, scaled so 1 unit = 255° / 360).
    ///   We might consider switching to tangent if that represents the
    ///   information we care about better.
    /// * Approximate (floor) height of maximal occluder. We currently use this
    ///   to try to deliver some approximation of soft shadows, which isn't that
    ///   big a deal on the world map but is probably needed in order to ensure
    ///   smooth transitions between chunks in LoD view. Additionally, when we
    ///   start using the shadow information to do local lighting on the world
    ///   map, we'll want a quick way to test where we can go out of shadow at
    ///   arbitrary heights (since the player and other entities cajn find
    ///   themselves far from the ground at times). While this is only an
    ///   approximation to a proper distance map, hopefully it will give us
    ///   something  that feels reasonable enough for Veloren's style.
    ///
    /// NOTE: On compression.
    ///
    /// Horizon mapping has a lot of advantages for height maps (simple, easy to
    /// understand, doesn't require any fancy math or approximation beyond
    /// precision loss), though it loses a few of them by having to store
    /// distance to occluder as well. However, just storing tons
    /// and tons of regular shadow maps (153 for a full day cycle, stored at
    /// irregular intervals) combined with clever explicit compression and
    /// avoiding recording sharp local shadows (preferring retracing for
    /// these), yielded a compression rate of under 3 bits per column! Since
    /// we likely want to avoid per-column shadows for worlds of the sizes we
    /// want, we'd still need to store *some* extra information to create
    /// soft shadows, but it would still be nice to try to drive down our
    /// size as much as possible given how compressible shadows of height
    /// maps seem to be in practice. Therefore, we try to take advantage of the
    /// way existing compression algorithms tend to work to see if we can
    /// achieve significant gains without doing a lot of custom work.
    ///
    /// Specifically, since our rays are cast east/west, we expect that for each
    /// row, the horizon angles in each direction should be sequences of
    /// monotonically increasing values (as chunks approach a tall
    /// occluder), followed by sequences of no shadow, repeated
    /// until the end of the map. Monotonic sequences and same-byte sequences
    /// are usually easy to compress and existing algorithms are more likely
    /// to be able to deal with them than jumbled data.  If we were to keep
    /// both directions in the same vector, off-the-shelf compression would
    /// probably be less effective.
    ///
    /// For related reasons, rather than storing distances as in a standard
    /// distance map (which would lead to monotonically *decreasing* values
    /// as we approached the occluder from a given direction), we store the
    /// estimated *occluder height.* The idea here is that we replace the
    /// monotonic sequences with constant sequences, which are extremely
    /// straightforward to compress and mostly handled automatically by anything
    /// that does run-length encoding (i.e. most off-the-shelf compression
    /// algorithms).
    ///
    /// We still need to benchmark this properly, as there's no guarantee our
    /// current compression algorithms will actually work well on this data
    /// in practice. It's possible that some other permutation (e.g. more
    /// bits reserved for "distance to occluder" in exchange for an even
    /// more predictible sequence) would end up compressing better than storing
    /// angles, or that we don't need as much precision as we currently have
    /// (256 possible angles).
    pub horizons: [(Vec<u8>, Vec<u8>); 2],
    pub sites: Vec<Marker>,
    pub possible_starting_sites: Vec<SiteId>,
    pub pois: Vec<PoiInfo>,
    /// Runtime topology/query-domain descriptor for chunk-level world access.
    pub runtime_topology: RuntimeTopologyDescriptor,
    /// Default chunk used when no world chunk product is available for a query
    /// under the current topology contract. At present this is still the
    /// bounded ocean fallback product, and sea level (used to provide a base
    /// altitude) is the lower bound of this chunk.
    pub default_chunk: Arc<TerrainChunk>,
}

impl WorldMapMsg {
    pub fn topology_id(&self) -> &str { &self.runtime_topology.topology_id }

    pub fn query_chunk_key_aabr(&self) -> Aabr<i32> { self.runtime_topology.query_chunk_key_aabr }

    pub fn wraps_x(&self) -> bool { self.runtime_topology.wraps_x() }

    pub fn wraps_y(&self) -> bool { self.runtime_topology.wraps_y() }

    pub fn query_chunk_dimensions(&self) -> Vec2<i32> {
        self.runtime_topology.query_chunk_dimensions()
    }

    pub fn contains_query_chunk_key(&self, chunk_key: Vec2<i32>) -> bool {
        self.runtime_topology.contains_query_chunk_key(chunk_key)
    }

    pub fn normalize_query_chunk_key(&self, chunk_key: Vec2<i32>) -> Option<Vec2<i32>> {
        self.runtime_topology.normalize_query_chunk_key(chunk_key)
    }

    pub fn query_chunk_key_delta(&self, from: Vec2<i32>, to: Vec2<i32>) -> Option<Vec2<i32>> {
        self.runtime_topology.query_chunk_key_delta(from, to)
    }

    pub fn runtime_chunk_product_key_aabr(&self) -> Aabr<i32> {
        self.runtime_topology.runtime_chunk_product_key_aabr
    }

    pub fn contains_runtime_chunk_product_key(&self, chunk_key: Vec2<i32>) -> bool {
        self.runtime_topology
            .contains_runtime_chunk_product_key(chunk_key)
    }

    pub fn missing_world_bounds_policy(&self) -> MissingWorldBoundsPolicy {
        self.runtime_topology.missing_world_bounds_policy
    }

    pub fn default_chunk_for_missing_world_bounds(&self) -> Arc<TerrainChunk> {
        Arc::clone(&self.default_chunk)
    }

    pub fn default_chunk_sea_level(&self) -> f32 { self.default_chunk.get_min_z() as f32 }
}

pub type SiteId = common::trade::SiteId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomyInfo {
    pub id: SiteId,
    pub population: u32,
    pub stock: HashMap<Good, f32>,
    pub labor_values: HashMap<Good, f32>,
    pub values: HashMap<Good, f32>,
    pub labors: Vec<f32>,
    pub last_exports: HashMap<Good, f32>,
    pub resources: HashMap<Good, f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_chunk_helpers_preserve_current_bounded_ocean_contract() {
        let msg = WorldMapMsg {
            dimensions_lg: Vec2::zero(),
            max_height: 1.0,
            rgba: Grid::new(Vec2::one(), 0),
            alt: Grid::new(Vec2::one(), 0),
            horizons: [(vec![0], vec![0]), (vec![0], vec![0])],
            sites: Vec::new(),
            possible_starting_sites: Vec::new(),
            pois: Vec::new(),
            runtime_topology: RuntimeTopologyDescriptor {
                topology_id: "bounded_plane_v1".to_owned(),
                query_chunk_key_aabr: Aabr {
                    min: Vec2::zero(),
                    max: Vec2::new(3, 3),
                },
                runtime_chunk_product_key_aabr: Aabr {
                    min: Vec2::new(1, 1),
                    max: Vec2::new(2, 2),
                },
                missing_world_bounds_policy: MissingWorldBoundsPolicy::BoundedOceanDefaultChunk,
            },
            default_chunk: Arc::new(TerrainChunk::water(123)),
        };

        assert_eq!(msg.topology_id(), "bounded_plane_v1");
        assert_eq!(msg.query_chunk_key_aabr(), Aabr {
            min: Vec2::zero(),
            max: Vec2::new(3, 3),
        });
        assert!(msg.contains_query_chunk_key(Vec2::zero()));
        assert!(msg.contains_query_chunk_key(Vec2::new(1, 0)));
        assert_eq!(msg.runtime_chunk_product_key_aabr(), Aabr {
            min: Vec2::new(1, 1),
            max: Vec2::new(2, 2),
        });
        assert!(!msg.contains_runtime_chunk_product_key(Vec2::zero()));
        assert!(msg.contains_runtime_chunk_product_key(Vec2::new(1, 1)));
        assert_eq!(
            msg.missing_world_bounds_policy(),
            MissingWorldBoundsPolicy::BoundedOceanDefaultChunk
        );
        let cloned_default_chunk = msg.default_chunk_for_missing_world_bounds();
        assert!(Arc::ptr_eq(&msg.default_chunk, &cloned_default_chunk));
        assert_eq!(msg.default_chunk_sea_level(), 123.0);
    }

    #[test]
    fn runtime_topology_descriptor_normalizes_and_wraps_query_chunk_keys() {
        let toroidal = RuntimeTopologyDescriptor {
            topology_id: "wrap_toroidal_exp_v1".to_owned(),
            query_chunk_key_aabr: Aabr {
                min: Vec2::zero(),
                max: Vec2::new(15, 15),
            },
            runtime_chunk_product_key_aabr: Aabr {
                min: Vec2::zero(),
                max: Vec2::new(15, 15),
            },
            missing_world_bounds_policy: MissingWorldBoundsPolicy::BoundedOceanDefaultChunk,
        };
        let cylindrical = RuntimeTopologyDescriptor {
            topology_id: "wrap_cylindrical_exp_v1".to_owned(),
            query_chunk_key_aabr: Aabr {
                min: Vec2::zero(),
                max: Vec2::new(15, 15),
            },
            runtime_chunk_product_key_aabr: Aabr {
                min: Vec2::zero(),
                max: Vec2::new(15, 13),
            },
            missing_world_bounds_policy: MissingWorldBoundsPolicy::BoundedOceanDefaultChunk,
        };
        let bounded = RuntimeTopologyDescriptor {
            topology_id: "bounded_plane_v1".to_owned(),
            query_chunk_key_aabr: Aabr {
                min: Vec2::zero(),
                max: Vec2::new(15, 15),
            },
            runtime_chunk_product_key_aabr: Aabr {
                min: Vec2::one(),
                max: Vec2::new(13, 13),
            },
            missing_world_bounds_policy: MissingWorldBoundsPolicy::BoundedOceanDefaultChunk,
        };

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
        assert_eq!(bounded.normalize_query_chunk_key(Vec2::new(-1, 0)), None);
    }

    #[test]
    fn runtime_topology_descriptor_exposes_shortest_query_chunk_delta() {
        let toroidal = RuntimeTopologyDescriptor {
            topology_id: "wrap_toroidal_exp_v1".to_owned(),
            query_chunk_key_aabr: Aabr {
                min: Vec2::zero(),
                max: Vec2::new(15, 15),
            },
            runtime_chunk_product_key_aabr: Aabr {
                min: Vec2::zero(),
                max: Vec2::new(15, 15),
            },
            missing_world_bounds_policy: MissingWorldBoundsPolicy::BoundedOceanDefaultChunk,
        };
        let cylindrical = RuntimeTopologyDescriptor {
            topology_id: "wrap_cylindrical_exp_v1".to_owned(),
            query_chunk_key_aabr: Aabr {
                min: Vec2::zero(),
                max: Vec2::new(15, 15),
            },
            runtime_chunk_product_key_aabr: Aabr {
                min: Vec2::zero(),
                max: Vec2::new(15, 13),
            },
            missing_world_bounds_policy: MissingWorldBoundsPolicy::BoundedOceanDefaultChunk,
        };
        let bounded = RuntimeTopologyDescriptor {
            topology_id: "bounded_plane_v1".to_owned(),
            query_chunk_key_aabr: Aabr {
                min: Vec2::zero(),
                max: Vec2::new(15, 15),
            },
            runtime_chunk_product_key_aabr: Aabr {
                min: Vec2::one(),
                max: Vec2::new(13, 13),
            },
            missing_world_bounds_policy: MissingWorldBoundsPolicy::BoundedOceanDefaultChunk,
        };

        assert!(toroidal.wraps_x());
        assert!(toroidal.wraps_y());
        assert_eq!(
            toroidal.query_chunk_key_delta(Vec2::new(0, 0), Vec2::new(15, 15)),
            Some(Vec2::new(-1, -1))
        );
        assert_eq!(
            cylindrical.query_chunk_key_delta(Vec2::new(0, 7), Vec2::new(15, 7)),
            Some(Vec2::new(-1, 0))
        );
        assert_eq!(
            bounded.query_chunk_key_delta(Vec2::new(1, 1), Vec2::new(14, 1)),
            Some(Vec2::new(13, 0))
        );
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoiInfo {
    pub kind: PoiKind,
    pub wpos: Vec2<i32>,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[repr(u8)]
pub enum PoiKind {
    Peak(u32),
    Lake(u32),
}
