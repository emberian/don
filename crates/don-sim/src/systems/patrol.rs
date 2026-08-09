//! Retail patrol order state and executor transitions.
//!
//! This module is a transcription of the two live patrol order classes and executors in
//! `riseofnations.exe`:
//!
//! * `AirPatrolOrder` (104 B), installed by `Unit::add_air_patrol_order` `0x005E4350`
//!   and executed by `Unit::do_air_patrol` `0x005EA620`;
//! * `GroupPatrolOrder` (100 B), installed by `Unit::add_patrol_order` `0x005E4560`
//!   and executed by `Unit::do_patrol` `0x005F1910`.
//!
//! The waypoint arrays are deliberately dynamic. Retail stores two `SimpleArray<Coord>`
//! instances and `Group::action_patrol` grows both arrays when a shift-patrol extends the
//! active route. A fixed waypoint cap would be a new gameplay rule.
//!
//! The module stops at explicit subsystem boundaries. Airframe integration remains the
//! caller's `Unit::do_air_physics` responsibility, and group motion remains the caller's
//! `Group::action_move_to` responsibility. The values handed across those boundaries are
//! the exact arguments recovered from the call sites; no straight-line patrol substitute
//! lives here.

use crate::systems::air::{AirOrderWalk, AIR_PATROL_WAYPOINT_ARRIVE};
use crate::systems::map_terrain::{floor_div, COORD_PER_UCELL};
use crate::systems::movement::vector_dist;
use crate::trig::find_angle;

/// PDB `sizeof(PatrolOrder)`.
pub const SIZEOF_PATROL_ORDER: usize = 76;
/// PDB `sizeof(GroupPatrolOrder)`.
pub const SIZEOF_GROUP_PATROL_ORDER: usize = 100;
/// PDB `sizeof(AirPatrolOrder)`.
pub const SIZEOF_AIR_PATROL_ORDER: usize = 104;

/// The fields of the `PatrolOrder` primary base.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PatrolPoints {
    /// `PatrolOrder::x_pos` at `+4`.
    pub x: Vec<i32>,
    /// `PatrolOrder::y_pos` at `+32`.
    pub y: Vec<i32>,
    /// `PatrolOrder::waypoint` at `+60`.
    pub waypoint: i32,
}

impl PatrolPoints {
    pub fn one(x: i32, y: i32) -> Self {
        Self {
            x: vec![x],
            y: vec![y],
            waypoint: 0,
        }
    }

    pub fn two(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        Self {
            x: vec![x0, x1],
            y: vec![y0, y1],
            waypoint: 0,
        }
    }

    /// `SimpleArray<Coord>::add` on both arrays, as used by both group action paths.
    pub fn push(&mut self, x: i32, y: i32) {
        self.x.push(x);
        self.y.push(y);
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.x.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.x.is_empty()
    }

    /// The constructors and `redo_patrol_order` keep the arrays the same length. Exposing
    /// the invariant makes malformed recovered/save data fail visibly before an executor
    /// indexes one array with the other's count, as retail itself assumes it may do.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.x.len() != self.y.len() {
            return Err("patrol x/y waypoint arrays have different lengths");
        }
        if self.x.is_empty() {
            return Err("live patrol order has no waypoints");
        }
        Ok(())
    }

    /// Clamp the cursor exactly as the head of `Unit::do_air_patrol` does. Ground patrol
    /// advances before reading instead; see [`advance_ground_waypoint`].
    pub fn clamp_air_cursor(&mut self) -> usize {
        let n = self.x.len();
        debug_assert_eq!(n, self.y.len());
        if n == 0 {
            self.waypoint = 0;
            return 0;
        }
        if self.waypoint < 0 || self.waypoint as usize >= n {
            self.waypoint = 0;
        }
        self.waypoint as usize
    }

    fn advance_ground_waypoint(&mut self) -> usize {
        let n = self.x.len();
        debug_assert!(n > 0);
        debug_assert_eq!(n, self.y.len());
        let next = self.waypoint.wrapping_add(1);
        self.waypoint = if next < 0 || next as usize >= n {
            0
        } else {
            next
        };
        self.waypoint as usize
    }
}

/// The `GroupOrder` secondary base of `GroupPatrolOrder`, at concrete offsets
/// `+68..+84`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GroupOrderWalk {
    pub oxx: i32,
    pub whose: i32,
    pub id: i32,
    pub form_id: i32,
    pub group_angle: i32,
}

/// Executable state of `GroupPatrolOrder`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GroupPatrolOrder {
    pub points: PatrolPoints,
    pub group: GroupOrderWalk,
}

/// Executable state of `AirPatrolOrder`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AirPatrolOrder {
    pub points: PatrolPoints,
    /// The secondary `AirOrder` base at concrete `AirPatrolOrder+64`.
    pub air: AirOrderWalk,
}

/// `StrafeOrder` state written by `Unit::add_strafe_order` `0x005E48C0`. Patrol needs this
/// payload because its executor inserts a live `STRAFE` order at the front of its own
/// queue; reducing it to only a target id would lose checksum-visible `AirOrder` fields.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StrafeOrder {
    pub target_o: i32,
    pub target_who: i32,
    pub target_uid: u16,
    pub def_x: i32,
    pub def_y: i32,
    pub mandatory: u8,
    pub defensive: u8,
    pub in_range: u8,
    pub ever_in_range: u8,
    pub new_ord: u8,
    pub air: AirOrderWalk,
    pub xx: i32,
    pub yy: i32,
}

/// Normalize a fine `Coord` to the centre of its containing `UCoord` cell. This is the
/// exact `div_3_table[c >> 4]`, then `u * 48 + 24`, sequence in the patrol installer and
/// ground executor. [`floor_div`] preserves the negative-coordinate behavior of the
/// runtime lookup table.
#[inline]
pub const fn ucell_centre(coord: i32) -> i32 {
    floor_div(coord, COORD_PER_UCELL) * COORD_PER_UCELL + COORD_PER_UCELL / 2
}

/// Construct the order body installed by `Unit::add_patrol_order`.
///
/// The order always starts with exactly two U-cell-centred points and `waypoint == 0`.
/// `group_angle` is not written by the installer and therefore retains its constructor
/// value (zero in the recovered manager objects).
pub fn new_group_patrol(
    start_x: i32,
    start_y: i32,
    target_x: i32,
    target_y: i32,
    id: i32,
    form_id: i32,
    oxx: i32,
    whose: i32,
) -> GroupPatrolOrder {
    GroupPatrolOrder {
        points: PatrolPoints::two(
            ucell_centre(start_x),
            ucell_centre(start_y),
            ucell_centre(target_x),
            ucell_centre(target_y),
        ),
        group: GroupOrderWalk {
            oxx,
            whose,
            id,
            form_id,
            group_angle: 0,
        },
    }
}

/// Construct the order body installed by the true-plane arm of
/// `Unit::add_air_patrol_order`.
///
/// When a live home object exists, retail stores the waypoint relative to that object's
/// current position. With no home it stores the absolute coordinate. The initial cruise
/// altitude is the literal `0x640` and `returning` is cleared.
pub fn new_air_patrol(
    target_x: i32,
    target_y: i32,
    home_o: i32,
    home_who: i32,
    home_pos: Option<(i32, i32)>,
) -> AirPatrolOrder {
    let (x, y) = match home_pos {
        Some((hx, hy)) => (target_x.wrapping_sub(hx), target_y.wrapping_sub(hy)),
        None => (target_x, target_y),
    };
    AirPatrolOrder {
        points: PatrolPoints::one(x, y),
        air: AirOrderWalk {
            oxx: home_o,
            whose: home_who,
            cruising_alt: 0x640,
            sharp_turn: 0,
            old: 0,
            returning: 0,
        },
    }
}

/// Exact `MoveOrder` fields initialized by the ungrouped arm of `Unit::do_patrol`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AttackToLeg {
    pub x: i32,
    pub y: i32,
    pub angle: i32,
    pub dest: i32,
    pub tolerance: i32,
    pub pause: i32,
    pub retry: i32,
    pub attempts: i32,
    pub timer: i32,
    pub facing: i32,
    pub dest_x: i32,
    pub dest_y: i32,
    pub last_x: i32,
    pub last_y: i32,
    pub off_x: i16,
    pub off_y: i16,
}

/// The group-layer call made by the grouped arm of `Unit::do_patrol`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupMoveRequest {
    pub group_slot: i16,
    pub x: i32,
    pub y: i32,
    /// `QueuePos::QUEUE_FIRST` (0).
    pub queue_pos: u8,
    /// `OrderIndex::ATTACK_TO` (2).
    pub order_index: u8,
    pub set_angle: i32,
    pub angle: i32,
    pub use_form: i32,
    pub form: i32,
    pub width: i32,
    pub disembark: i32,
}

/// Main action selected by one `Unit::do_patrol` call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroundPatrolAction {
    /// Ungrouped: insert this `ATTACK_TO` movement leg at the queue front.
    InsertAttackTo(AttackToLeg),
    /// Grouped and the order's `(whose, oxx)` identifies this unit: delegate to the group.
    MoveGroup(GroupMoveRequest),
    /// Grouped but the stored identity does not identify this unit: `set_anim(0,0,1)`.
    IdleAnimation,
}

/// The complete externally visible result of one ground-patrol executor call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroundPatrolStep {
    pub action: GroundPatrolAction,
    /// The trailing `inside_down >= 0 && is(TypeIndex(0x15f), true)` branch creates a
    /// singleton temporary group and calls `Group::action_scramble`. The host supplies the
    /// predicate; the exact contained object index is returned here.
    pub scramble_inside_down: Option<i16>,
}

/// Execute the state transition of `Unit::do_patrol` `0x005F1910`.
#[allow(clippy::too_many_arguments)]
pub fn step_group_patrol(
    order: &mut GroupPatrolOrder,
    unit_x: i32,
    unit_y: i32,
    unit_o: i16,
    unit_who: u8,
    group_slot: i16,
    inside_down: i16,
    inside_is_scramblable: bool,
) -> GroundPatrolStep {
    order
        .points
        .validate()
        .expect("retail GroupPatrolOrder requires paired, non-empty waypoint arrays");
    let i = order.points.advance_ground_waypoint();
    let (px, py) = (order.points.x[i], order.points.y[i]);
    let scramble_inside_down = (inside_down >= 0 && inside_is_scramblable).then_some(inside_down);

    let action = if group_slot < 0 {
        let x = ucell_centre(px);
        let y = ucell_centre(py);
        GroundPatrolAction::InsertAttackTo(AttackToLeg {
            x,
            y,
            // The call is `find_angle(waypoint_x-unit_x, waypoint_y-unit_y)`; the register
            // order in Ghidra is misleading, while Capstone and the function signature agree.
            angle: find_angle(px.wrapping_sub(unit_x), py.wrapping_sub(unit_y)),
            dest: 0,
            tolerance: 0,
            pause: 0,
            retry: 0,
            attempts: 0,
            timer: 0,
            facing: -1,
            dest_x: x,
            dest_y: y,
            last_x: -1,
            last_y: -1,
            // MSVC emits signed `idiv`; the remainder keeps the dividend's sign.
            off_x: (x % 0x300) as i16,
            off_y: (y % 0x300) as i16,
        })
    } else if order.group.oxx == unit_o as i32 && order.group.whose == unit_who as i32 {
        GroundPatrolAction::MoveGroup(GroupMoveRequest {
            group_slot,
            x: px,
            y: py,
            queue_pos: 0,
            order_index: 2,
            set_angle: 0,
            angle: 0,
            use_form: 0,
            form: -1,
            width: -1,
            disembark: 0,
        })
    } else {
        GroundPatrolAction::IdleAnimation
    };

    GroundPatrolStep {
        action,
        scramble_inside_down,
    }
}

/// Resolve one stored air-patrol waypoint to a world coordinate. Non-animal aircraft store
/// waypoints relative to a live home object; animal flyers (the bird types) always use the
/// stored values directly. `WorldData::restrict` is represented by the caller-provided map
/// bounds, because those are runtime world state.
pub fn air_patrol_target(
    order: &mut AirPatrolOrder,
    is_animal: bool,
    home_pos: Option<(i32, i32)>,
    world_max_x: i32,
    world_max_y: i32,
) -> (i32, i32) {
    order
        .points
        .validate()
        .expect("retail AirPatrolOrder requires paired, non-empty waypoint arrays");
    let i = order.points.clamp_air_cursor();
    if order.air.returning != 0 {
        return (-1, -1);
    }
    let (mut x, mut y) = (order.points.x[i], order.points.y[i]);
    if !is_animal {
        if let Some((hx, hy)) = home_pos {
            x = x.wrapping_add(hx).clamp(0, world_max_x.saturating_sub(1));
            y = y.wrapping_add(hy).clamp(0, world_max_y.saturating_sub(1));
        }
    }
    (x, y)
}

/// A live target returned by the retail air/unit or building search callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AirPatrolTarget {
    pub o: i32,
    pub who: i32,
    pub uid: u16,
    pub x: i32,
    pub y: i32,
    pub domain: i32,
    /// For a building result, the bit in `ObjectTypeData+0x62` for the patroller's owner.
    pub owner_target_bit: bool,
}

/// Work inserted by the post-physics half of `Unit::do_air_patrol`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirPatrolAction {
    Continue,
    KillCurrent,
    InsertStrafe {
        target: AirPatrolTarget,
        mandatory: u8,
    },
    PrimeAnimalSpellTime,
}

/// Inputs whose values come from adjacent, separately derived systems after
/// `Unit::do_air_physics` returns non-zero.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AirPatrolAfterPhysics {
    pub actor_x: i32,
    pub actor_y: i32,
    pub actor_o: i16,
    pub frame: i32,
    pub is_animal: bool,
    pub spell_time: i16,
    pub order_list_len: usize,
    /// Result of the mod-16 `find_new_air_target` / `find_new_bomber_target` call, when due.
    pub unit_target: Option<AirPatrolTarget>,
    /// Result of the mod-32 `ObjectsData::find_building_at` call, when due.
    pub building_target: Option<AirPatrolTarget>,
}

/// The half of `Unit::do_air_patrol` after `Unit::do_air_physics` reports success.
///
/// Target *search* remains a world callback, exactly like damage and gathering in the unit
/// dispatcher. This function owns the executor's cadence and acceptance conditions.
pub fn step_air_patrol_after_physics(
    order: &mut AirPatrolOrder,
    flight_target: (i32, i32),
    input: &AirPatrolAfterPhysics,
) -> AirPatrolAction {
    order
        .points
        .validate()
        .expect("retail AirPatrolOrder requires paired, non-empty waypoint arrays");
    if input.is_animal {
        return if input.spell_time == 0 {
            AirPatrolAction::PrimeAnimalSpellTime
        } else {
            AirPatrolAction::Continue
        };
    }

    if order.air.returning == 0 {
        let i = order.points.clamp_air_cursor();
        if vector_dist(
            flight_target.0.wrapping_sub(input.actor_x),
            flight_target.1.wrapping_sub(input.actor_y),
        ) < AIR_PATROL_WAYPOINT_ARRIVE
        {
            if i + 1 < order.points.len() {
                order.points.waypoint += 1;
            } else if input.order_list_len > 1 {
                return AirPatrolAction::KillCurrent;
            }
        }

        if (input.actor_o as i32).wrapping_add(input.frame) % 16 == 0 {
            if let Some(target) = input.unit_target {
                let at_last = order.points.waypoint as usize + 1 == order.points.len();
                if at_last || target.domain == 2 {
                    return AirPatrolAction::InsertStrafe {
                        target,
                        mandatory: 0,
                    };
                }
            }
        }
    }

    if (input.actor_o as i32).wrapping_add(input.frame) % 32 == 0 {
        if let Some(target) = input.building_target {
            if target.owner_target_bit {
                return AirPatrolAction::InsertStrafe {
                    target,
                    mandatory: 1,
                };
            }
        }
    }

    AirPatrolAction::Continue
}

/// Build the exact patrol-originated `StrafeOrder` body. Both patrol call sites pass
/// `QueuePos::QUEUE_FIRST`, `group = 0`; `mandatory` distinguishes the unit scan (0) from
/// the building scan (1).
pub fn patrol_strafe_order(
    target: AirPatrolTarget,
    home_o: i32,
    home_who: i32,
    mandatory: u8,
) -> StrafeOrder {
    StrafeOrder {
        target_o: target.o,
        target_who: target.who,
        target_uid: target.uid,
        def_x: 0,
        def_y: 0,
        mandatory,
        defensive: 0,
        in_range: 0,
        ever_in_range: 0,
        new_ord: 0,
        air: AirOrderWalk {
            oxx: home_o,
            whose: home_who,
            cruising_alt: 0x640,
            sharp_turn: 0,
            old: 0,
            returning: 0,
        },
        xx: target.x,
        yy: target.y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdb_sizes_and_offsets_are_not_replaced_with_fixed_route_caps() {
        assert_eq!(SIZEOF_PATROL_ORDER, 76);
        assert_eq!(SIZEOF_GROUP_PATROL_ORDER, 100);
        assert_eq!(SIZEOF_AIR_PATROL_ORDER, 104);
        let mut p = PatrolPoints::one(1, 2);
        for i in 0..1024 {
            p.push(i, -i);
        }
        assert_eq!(p.len(), 1025);
        assert!(p.validate().is_ok());
    }

    #[test]
    fn ground_constructor_normalizes_both_points_to_ucell_centres() {
        let p = new_group_patrol(0, 47, 48, -1, 7, 8, 9, 1);
        assert_eq!(p.points.x, vec![24, 72]);
        assert_eq!(p.points.y, vec![24, -24]);
        assert_eq!(p.group.id, 7);
        assert_eq!(p.group.form_id, 8);
        assert_eq!((p.group.oxx, p.group.whose), (9, 1));
        assert_eq!(p.group.group_angle, 0);
    }

    #[test]
    fn ungrouped_ground_patrol_advances_before_inserting_attack_to() {
        let mut p = new_group_patrol(24, 24, 120, 72, 0, 0, 4, 1);
        let s = step_group_patrol(&mut p, 24, 24, 4, 1, -1, -1, false);
        let GroundPatrolAction::InsertAttackTo(m) = s.action else {
            panic!("ungrouped patrol must insert ATTACK_TO");
        };
        assert_eq!(p.points.waypoint, 1);
        assert_eq!((m.x, m.y), (120, 72));
        assert_eq!((m.dest_x, m.dest_y), (120, 72));
        assert_eq!(m.facing, -1);
        assert_eq!((m.last_x, m.last_y), (-1, -1));
        assert_eq!((m.off_x, m.off_y), (120, 72));
    }

    #[test]
    fn grouped_ground_patrol_uses_exact_action_move_to_arguments() {
        let mut p = new_group_patrol(24, 24, 120, 72, 0, 0, 4, 1);
        let s = step_group_patrol(&mut p, 24, 24, 4, 1, 3, -1, false);
        let GroundPatrolAction::MoveGroup(g) = s.action else {
            panic!("matching grouped patrol must delegate");
        };
        assert_eq!(g.group_slot, 3);
        assert_eq!((g.x, g.y), (120, 72));
        assert_eq!(g.queue_pos, 0);
        assert_eq!(g.order_index, 2);
        assert_eq!((g.form, g.width, g.disembark), (-1, -1, 0));
    }

    #[test]
    fn air_constructor_stores_relative_waypoint_and_reconstitutes_it() {
        let mut p = new_air_patrol(1000, 2000, 5, 1, Some((300, 700)));
        assert_eq!(p.points.x, vec![700]);
        assert_eq!(p.points.y, vec![1300]);
        assert_eq!(p.air.cruising_alt, 0x640);
        assert_eq!(
            air_patrol_target(&mut p, false, Some((300, 700)), 5000, 5000),
            (1000, 2000)
        );
    }

    #[test]
    fn last_air_waypoint_only_retires_when_another_order_is_queued() {
        let mut p = new_air_patrol(100, 100, -1, -1, None);
        let mut i = AirPatrolAfterPhysics {
            actor_x: 100,
            actor_y: 100,
            actor_o: 1,
            frame: 1,
            order_list_len: 1,
            ..AirPatrolAfterPhysics::default()
        };
        assert_eq!(
            step_air_patrol_after_physics(&mut p, (100, 100), &i),
            AirPatrolAction::Continue
        );
        i.order_list_len = 2;
        assert_eq!(
            step_air_patrol_after_physics(&mut p, (100, 100), &i),
            AirPatrolAction::KillCurrent
        );
    }

    #[test]
    fn scan_cadence_and_acceptance_match_the_two_executor_branches() {
        let mut p = AirPatrolOrder {
            points: PatrolPoints::two(0, 0, 1000, 1000),
            ..AirPatrolOrder::default()
        };
        let ground = AirPatrolTarget {
            o: 7,
            who: 2,
            uid: 9,
            x: 50,
            y: 60,
            domain: 0,
            owner_target_bit: true,
        };
        let mut i = AirPatrolAfterPhysics {
            actor_o: 3,
            frame: 13,
            order_list_len: 1,
            unit_target: Some(ground),
            ..AirPatrolAfterPhysics::default()
        };
        // Ground-domain targets are rejected before the last patrol waypoint.
        assert_eq!(
            step_air_patrol_after_physics(&mut p, (1000, 1000), &i),
            AirPatrolAction::Continue
        );
        p.points.waypoint = 1;
        assert_eq!(
            step_air_patrol_after_physics(&mut p, (1000, 1000), &i),
            AirPatrolAction::InsertStrafe {
                target: ground,
                mandatory: 0
            }
        );
        i.frame = 29; // 3 + 29 == 32, both cadence gates open.
        i.unit_target = None;
        i.building_target = Some(ground);
        assert_eq!(
            step_air_patrol_after_physics(&mut p, (1000, 1000), &i),
            AirPatrolAction::InsertStrafe {
                target: ground,
                mandatory: 1
            }
        );
    }
}
