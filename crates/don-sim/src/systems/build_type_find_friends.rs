//! Exact type-owned prefix of `BuildTypeData::find_friends` `0x00639270`.
//!
//! The complete retail body is 1,103 bytes. Its first branch bypasses the Tower/Lookout checks
//! when `ObjectTypeData::attack == 0`; an armed type continues only when it is a Tower or
//! Lookout. This module owns the resulting type-only zero return, the Woodcutter/build-flags
//! normalization, and the out-of-bounds ring prefix. The golden unarmed Market continues through
//! those gates and stops here at its first `ObjectsData::find_building_placed_at` call.
//!
//! No scratch field is mutated here. The typed child records the point where retail would first
//! write `ObjectsData+0x200 = -1`; a later owner must stage that child atomically.
//!
//! Source ledger: supported PE SHA-256
//! `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`;
//! `BuildTypeData::find_friends` `[0x00639270, 0x006396bf)` is 1,103 bytes with SHA-256
//! `9fde3f03e4f4880bc18ed629dc2175f6d1f1d376fa1f196d9a64b1784c2d287e`;
//! its type-only gate `[0x00639270, 0x006392ae)` is 62 bytes with SHA-256
//! `45734820be78b53b4a7473ffa7f3e784e21c04eca94543df5ad6467e9f26a830`.
//! The first child, `ObjectsData::find_building_placed_at` `[0x00658c80, 0x00658e60)`,
//! is 480 bytes with SHA-256
//! `125fca1a859ba30aa9cdf27e585b111918316817041875e0779d61b51aab56ed`.

#![forbid(unsafe_code)]

use super::air_patrol_building_search_frontier::WORLD_CELL_SEARCH_OFFSETS;
use super::bhs_type_table::{TypeBody, TypeBuiltinState, TypeDomain};
use super::map_terrain::{World, WorldChecksum};
use super::production::runtime::{LiveProductionRuntime, LiveTypeClass};

pub const BUILD_TYPE_FIND_FRIENDS_VA: u32 = 0x0063_9270;
pub const BUILD_TYPE_FIND_FRIENDS_END_VA: u32 = 0x0063_96bf;
pub const BUILD_TYPE_FIND_FRIENDS_BYTES: u32 =
    BUILD_TYPE_FIND_FRIENDS_END_VA - BUILD_TYPE_FIND_FRIENDS_VA;
pub const BUILD_TYPE_FIND_FRIENDS_FIRST_OBJECT_CALL_VA: u32 = 0x0063_9335;
pub const OBJECTS_FIND_BUILDING_PLACED_AT_VA: u32 = 0x0065_8c80;
pub const OBJECTS_FIND_BUILDING_PLACED_AT_END_VA: u32 = 0x0065_8e60;
pub const OBJECTS_FIND_BUILDING_PLACED_AT_BYTES: u32 =
    OBJECTS_FIND_BUILDING_PLACED_AT_END_VA - OBJECTS_FIND_BUILDING_PLACED_AT_VA;
pub const OBJECTS_SELECTED_OWNER_OFFSET: u32 = 0x200;

pub const TOWER_TYPE: usize = 439;
pub const LOOKOUT_TYPE: usize = 521;
pub const WOODCUTTER_TYPE: usize = 418;
pub const IGNORE_CITY_BUILD_FLAG: u32 = 0x10;

/// One generic `BuildTypeData::find_friends` call.
///
/// The x/y members are the values behind the first two `const WCoord&` arguments. At the known
/// caller, retail pushes the logical referents in the order `owner, city_filter, y, x`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildTypeFindFriendsRequest {
    pub call_va: u32,
    pub callee_va: u32,
    pub type_index: i32,
    pub candidate_world_cell: [i32; 2],
    pub city_filter: i32,
    pub owner: i32,
}

impl BuildTypeFindFriendsRequest {
    pub const fn native_push_order(self) -> [i32; 4] {
        [
            self.owner,
            self.city_filter,
            self.candidate_world_cell[1],
            self.candidate_world_cell[0],
        ]
    }
}

/// Type reads in their exact short-circuit shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildTypeFindFriendsTypeReads {
    pub type_rows: usize,
    pub mutation_revision: u64,
    /// `ObjectTypeData::attack`, `BuildTypeData+0x1e8`.
    pub attack: i32,
    /// `is(Tower=439, 0)`, absent when zero attack bypasses the call.
    pub tower_relation: Option<bool>,
    /// `is(Lookout=521, 0)`, reached only after nonzero attack and a false Tower result.
    pub lookout_relation: Option<bool>,
    /// `is(Woodcutter=418, 0)`, absent when the first gate already returned.
    pub woodcutter_relation: Option<bool>,
    /// `BuildTypeData+0x2c0`, absent when an earlier gate returned.
    pub build_flags: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildTypeFindFriendsOutOfBoundsProbe {
    pub circle_offset: i32,
    pub world_cell: [i32; 2],
}

/// The first dynamic child after every source-owned type and bounds gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectsFindBuildingPlacedAtBoundary {
    pub call_va: u32,
    pub callee_va: u32,
    /// Logical values in x86 push order:
    /// `excluded_owner, excluded_object, owner_filter, tile_y, tile_x`.
    pub native_push_order: [i32; 5],
    pub circle_offset: i32,
    pub world_cell: [i32; 2],
    pub tile: [i32; 2],
    pub owner_filter: i32,
    pub excluded_object: i32,
    pub excluded_owner: i32,
    /// The callee's first mutation is unconditional and does not read the prior word.
    pub selected_owner_offset: u32,
    pub selected_owner_first_write: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildTypeFindFriendsStop {
    /// Native return at `0x006392a4` before any World/Object/Build read.
    ReturnedAtTypeGate,
    /// All eight ring-one World cells were outside the map; the native counter remains zero.
    ReturnedAfterBoundsExhaustion,
    /// First in-bounds ring cell is ready for `ObjectsData::find_building_placed_at`.
    FirstObjectLookup,
}

/// Read-only exact prefix/result receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildTypeFindFriendsReceipt {
    pub request: BuildTypeFindFriendsRequest,
    /// Logical values in the order x86 pushes them at the parent call site.
    pub native_push_order: [i32; 4],
    pub type_reads: BuildTypeFindFriendsTypeReads,
    /// Effective local City filter after the `build_flags & 0x10` normalization.
    pub effective_city_filter: Option<i32>,
    /// Present exactly when native begins reading World dimensions.
    pub world_checksum: Option<WorldChecksum>,
    pub out_of_bounds: Vec<BuildTypeFindFriendsOutOfBoundsProbe>,
    pub stop: BuildTypeFindFriendsStop,
    pub returned: Option<i32>,
    pub first_child: Option<ObjectsFindBuildingPlacedAtBoundary>,
}

impl BuildTypeFindFriendsReceipt {
    pub fn validates(&self) -> bool {
        if self.request.call_va == 0
            || self.request.callee_va != BUILD_TYPE_FIND_FRIENDS_VA
            || self.request.owner < 0
            || self.request.owner >= 8
            || self.native_push_order != self.request.native_push_order()
        {
            return false;
        }
        match self.stop {
            BuildTypeFindFriendsStop::ReturnedAtTypeGate => {
                self.returned == Some(0)
                    && self.first_child.is_none()
                    && self.world_checksum.is_none()
                    && self.effective_city_filter.is_none()
                    && self.out_of_bounds.is_empty()
                    && self.type_reads.build_flags.is_none()
            }
            BuildTypeFindFriendsStop::ReturnedAfterBoundsExhaustion => {
                self.returned == Some(0)
                    && self.first_child.is_none()
                    && self.world_checksum.is_some()
                    && self.effective_city_filter.is_some()
                    && self.out_of_bounds.len() == 8
                    && self.type_reads.build_flags.is_some()
            }
            BuildTypeFindFriendsStop::FirstObjectLookup => {
                self.returned.is_none()
                    && self.world_checksum.is_some()
                    && self.effective_city_filter.is_some()
                    && self.type_reads.build_flags.is_some()
                    && self.first_child.is_some_and(|child| {
                        child.call_va == BUILD_TYPE_FIND_FRIENDS_FIRST_OBJECT_CALL_VA
                            && child.callee_va == OBJECTS_FIND_BUILDING_PLACED_AT_VA
                            && child.native_push_order
                                == [
                                    child.excluded_owner,
                                    child.excluded_object,
                                    child.owner_filter,
                                    child.tile[1],
                                    child.tile[0],
                                ]
                            && child.owner_filter == self.request.owner
                            && child.excluded_object == -1
                            && child.excluded_owner == -1
                            && child.selected_owner_offset == OBJECTS_SELECTED_OWNER_OFFSET
                            && child.selected_owner_first_write == -1
                    })
            }
        }
    }

    /// Re-run the read-only producer against the canonical owners and require byte-for-byte
    /// receipt identity. This binds mutable type rows and the optional World checksum without
    /// inventing a second digest format.
    pub fn validates_against(
        &self,
        types: &TypeBuiltinState,
        production: &LiveProductionRuntime,
        world: &World,
    ) -> bool {
        produce_build_type_find_friends_prefix(types, production, world, self.request)
            .is_ok_and(|expected| expected == *self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuildTypeFindFriendsError {
    MissingCallSite,
    WrongCallee { expected: u32, actual: u32 },
    OwnerOutsidePlayableRange { owner: i32 },
    MissingType { type_index: i32 },
    TypeIsNotBuilding { type_index: i32 },
    MissingProductionType { type_index: i32 },
    ProductionTypeMismatch { type_index: i32 },
    InvalidWorldShape,
}

#[inline]
fn relation(types: &TypeBuiltinState, type_index: usize, query: usize) -> bool {
    types.types.rows()[type_index]
        .is_list
        .iter()
        .any(|&candidate| usize::from(candidate) == query)
}

/// Execute every source-owned instruction through either a complete zero return or the first
/// placed-building lookup.
pub fn produce_build_type_find_friends_prefix(
    types: &TypeBuiltinState,
    production: &LiveProductionRuntime,
    world: &World,
    request: BuildTypeFindFriendsRequest,
) -> Result<BuildTypeFindFriendsReceipt, BuildTypeFindFriendsError> {
    if request.call_va == 0 {
        return Err(BuildTypeFindFriendsError::MissingCallSite);
    }
    if request.callee_va != BUILD_TYPE_FIND_FRIENDS_VA {
        return Err(BuildTypeFindFriendsError::WrongCallee {
            expected: BUILD_TYPE_FIND_FRIENDS_VA,
            actual: request.callee_va,
        });
    }
    if !(0..8).contains(&request.owner) {
        return Err(BuildTypeFindFriendsError::OwnerOutsidePlayableRange {
            owner: request.owner,
        });
    }
    let type_index = usize::try_from(request.type_index)
        .ok()
        .filter(|&index| index < types.types.rows().len())
        .ok_or(BuildTypeFindFriendsError::MissingType {
            type_index: request.type_index,
        })?;
    let row = &types.types.rows()[type_index];
    let TypeBody::Build { object, .. } = &row.body else {
        return Err(BuildTypeFindFriendsError::TypeIsNotBuilding {
            type_index: request.type_index,
        });
    };
    if row.domain() != TypeDomain::Build {
        return Err(BuildTypeFindFriendsError::TypeIsNotBuilding {
            type_index: request.type_index,
        });
    }
    let target = production
        .types
        .get(type_index)
        .and_then(Option::as_ref)
        .ok_or(BuildTypeFindFriendsError::MissingProductionType {
            type_index: request.type_index,
        })?;
    if target.type_index != request.type_index || target.class != LiveTypeClass::Building {
        return Err(BuildTypeFindFriendsError::ProductionTypeMismatch {
            type_index: request.type_index,
        });
    }

    let attack = object.attack;
    let tower_relation = (attack != 0).then(|| relation(types, type_index, TOWER_TYPE));
    let lookout_relation =
        (tower_relation == Some(false)).then(|| relation(types, type_index, LOOKOUT_TYPE));
    let passes_first_gate =
        attack == 0 || tower_relation == Some(true) || lookout_relation == Some(true);
    if !passes_first_gate {
        let receipt = BuildTypeFindFriendsReceipt {
            request,
            native_push_order: request.native_push_order(),
            type_reads: BuildTypeFindFriendsTypeReads {
                type_rows: types.types.rows().len(),
                mutation_revision: types.mutation_revision(),
                attack,
                tower_relation,
                lookout_relation,
                woodcutter_relation: None,
                build_flags: None,
            },
            effective_city_filter: None,
            world_checksum: None,
            out_of_bounds: Vec::new(),
            stop: BuildTypeFindFriendsStop::ReturnedAtTypeGate,
            returned: Some(0),
            first_child: None,
        };
        debug_assert!(receipt.validates());
        return Ok(receipt);
    }

    let woodcutter_relation = relation(types, type_index, WOODCUTTER_TYPE);
    if woodcutter_relation {
        let receipt = BuildTypeFindFriendsReceipt {
            request,
            native_push_order: request.native_push_order(),
            type_reads: BuildTypeFindFriendsTypeReads {
                type_rows: types.types.rows().len(),
                mutation_revision: types.mutation_revision(),
                attack,
                tower_relation,
                lookout_relation,
                woodcutter_relation: Some(true),
                build_flags: None,
            },
            effective_city_filter: None,
            world_checksum: None,
            out_of_bounds: Vec::new(),
            stop: BuildTypeFindFriendsStop::ReturnedAtTypeGate,
            returned: Some(0),
            first_child: None,
        };
        debug_assert!(receipt.validates());
        return Ok(receipt);
    }

    let expected_cells = usize::try_from(world.xs).ok().and_then(|xs| {
        usize::try_from(world.ys)
            .ok()
            .and_then(|ys| xs.checked_mul(ys))
    });
    if world.xs <= 0
        || world.ys <= 0
        || expected_cells != Some(world.wdata.len())
        || world.tile_xs != world.xs.saturating_mul(4)
        || world.tile_ys != world.ys.saturating_mul(4)
    {
        return Err(BuildTypeFindFriendsError::InvalidWorldShape);
    }
    let build_flags = target.build_flags;
    let effective_city_filter = if build_flags & IGNORE_CITY_BUILD_FLAG != 0 {
        -1
    } else {
        request.city_filter
    };
    let world_checksum = world.checksum_sections();
    let mut out_of_bounds = Vec::new();
    for (circle_offset, &(dx, dy)) in WORLD_CELL_SEARCH_OFFSETS.iter().enumerate().skip(1) {
        let world_cell = [
            request.candidate_world_cell[0].wrapping_add(dx),
            request.candidate_world_cell[1].wrapping_add(dy),
        ];
        if !world.valid_w(world_cell[0], world_cell[1]) {
            out_of_bounds.push(BuildTypeFindFriendsOutOfBoundsProbe {
                circle_offset: circle_offset as i32,
                world_cell,
            });
            continue;
        }
        let tile = [
            world_cell[0].wrapping_mul(4).wrapping_add(2),
            world_cell[1].wrapping_mul(4).wrapping_add(2),
        ];
        let child = ObjectsFindBuildingPlacedAtBoundary {
            call_va: BUILD_TYPE_FIND_FRIENDS_FIRST_OBJECT_CALL_VA,
            callee_va: OBJECTS_FIND_BUILDING_PLACED_AT_VA,
            native_push_order: [-1, -1, request.owner, tile[1], tile[0]],
            circle_offset: circle_offset as i32,
            world_cell,
            tile,
            owner_filter: request.owner,
            excluded_object: -1,
            excluded_owner: -1,
            selected_owner_offset: OBJECTS_SELECTED_OWNER_OFFSET,
            selected_owner_first_write: -1,
        };
        let receipt = BuildTypeFindFriendsReceipt {
            request,
            native_push_order: request.native_push_order(),
            type_reads: BuildTypeFindFriendsTypeReads {
                type_rows: types.types.rows().len(),
                mutation_revision: types.mutation_revision(),
                attack,
                tower_relation,
                lookout_relation,
                woodcutter_relation: Some(false),
                build_flags: Some(build_flags),
            },
            effective_city_filter: Some(effective_city_filter),
            world_checksum: Some(world_checksum),
            out_of_bounds,
            stop: BuildTypeFindFriendsStop::FirstObjectLookup,
            returned: None,
            first_child: Some(child),
        };
        debug_assert!(receipt.validates());
        return Ok(receipt);
    }

    let receipt = BuildTypeFindFriendsReceipt {
        request,
        native_push_order: request.native_push_order(),
        type_reads: BuildTypeFindFriendsTypeReads {
            type_rows: types.types.rows().len(),
            mutation_revision: types.mutation_revision(),
            attack,
            tower_relation,
            lookout_relation,
            woodcutter_relation: Some(false),
            build_flags: Some(build_flags),
        },
        effective_city_filter: Some(effective_city_filter),
        world_checksum: Some(world_checksum),
        out_of_bounds,
        stop: BuildTypeFindFriendsStop::ReturnedAfterBoundsExhaustion,
        returned: Some(0),
        first_child: None,
    };
    debug_assert!(receipt.validates());
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::bhs_type_table::{
        LeaderTypeMasks, TribeRoster, TypeBackup, TypeRow, TypeTable, BUILD_BEGIN, BUILD_END,
        NUM_TRIBES, NUM_TYPES, REGULAR_UNIT_BEGIN, REGULAR_UNIT_END,
    };
    use crate::systems::production::runtime::LiveProductionType;

    const MARKET: usize = 436;
    const CALL_VA: u32 = 0x006e_1f5f;

    fn owners() -> (TypeBuiltinState, LiveProductionRuntime) {
        let mut rows: Vec<_> = (0..NUM_TYPES).map(TypeRow::empty).collect();
        rows[MARKET].name = "Market".into();
        let backups = rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                ((REGULAR_UNIT_BEGIN..REGULAR_UNIT_END).contains(&index)
                    || (BUILD_BEGIN..BUILD_END).contains(&index))
                .then(|| TypeBackup::capture_pristine(row))
            })
            .collect();
        let types = TypeBuiltinState::new(
            TypeTable::new(rows, backups).unwrap(),
            TribeRoster::new((0..NUM_TRIBES).map(|i| format!("Tribe {i}")).collect()).unwrap(),
            std::array::from_fn(|_| LeaderTypeMasks::default()),
        );
        let mut production = LiveProductionRuntime::default();
        production.types[MARKET] = Some(LiveProductionType {
            build_flags: 0x8000_1201,
            ..LiveProductionType::in_place_building(MARKET as i32, 420)
        });
        (types, production)
    }

    fn request() -> BuildTypeFindFriendsRequest {
        BuildTypeFindFriendsRequest {
            call_va: CALL_VA,
            callee_va: BUILD_TYPE_FIND_FRIENDS_VA,
            type_index: MARKET as i32,
            candidate_world_cell: [4, 4],
            city_filter: 0,
            owner: 0,
        }
    }

    #[test]
    fn market_reaches_first_placed_building_child() {
        let (types, production) = owners();
        let world = World::init_default_rules(8, 8);
        let receipt =
            produce_build_type_find_friends_prefix(&types, &production, &world, request()).unwrap();
        assert!(receipt.validates());
        assert!(receipt.validates_against(&types, &production, &world));
        assert_eq!(receipt.native_push_order, [0, 0, 4, 4]);
        assert_eq!(receipt.type_reads.attack, 0);
        assert_eq!(receipt.type_reads.tower_relation, None);
        assert_eq!(receipt.type_reads.lookout_relation, None);
        assert_eq!(receipt.type_reads.woodcutter_relation, Some(false));
        assert_eq!(receipt.type_reads.build_flags, Some(0x8000_1201));
        assert_eq!(receipt.effective_city_filter, Some(0));
        assert_eq!(receipt.stop, BuildTypeFindFriendsStop::FirstObjectLookup);
        assert_eq!(receipt.returned, None);
        assert!(receipt.world_checksum.is_some());
        let child = receipt.first_child.unwrap();
        assert_eq!(child.circle_offset, 1);
        assert_eq!(child.world_cell, [3, 3]);
        assert_eq!(child.tile, [14, 14]);
        assert_eq!(
            (
                child.owner_filter,
                child.excluded_object,
                child.excluded_owner
            ),
            (0, -1, -1)
        );
    }

    #[test]
    fn armed_non_tower_non_lookout_returns_zero_before_world_read() {
        let (mut types, production) = owners();
        let TypeBody::Build { object, .. } = &mut types.types.row_mut(MARKET).body else {
            unreachable!()
        };
        object.attack = 1;
        let world = World::init_default_rules(8, 8);
        let receipt =
            produce_build_type_find_friends_prefix(&types, &production, &world, request()).unwrap();
        assert!(receipt.validates_against(&types, &production, &world));
        assert_eq!(receipt.type_reads.tower_relation, Some(false));
        assert_eq!(receipt.type_reads.lookout_relation, Some(false));
        assert_eq!(receipt.type_reads.woodcutter_relation, None);
        assert_eq!(receipt.type_reads.build_flags, None);
        assert_eq!(receipt.effective_city_filter, None);
        assert_eq!(receipt.stop, BuildTypeFindFriendsStop::ReturnedAtTypeGate);
        assert_eq!(receipt.returned, Some(0));
        assert_eq!(receipt.world_checksum, None);
        assert!(receipt.out_of_bounds.is_empty());
        assert_eq!(receipt.first_child, None);
    }
}
