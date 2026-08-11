// SPDX-License-Identifier: GPL-3.0-or-later
//! `Group::action_scramble` `0x007111C0` (894 B) and `Group::action_launch_patrol`
//! `0x00703580` (2,043 B) — the two group receivers that launch aircraft **out of the
//! objects the selection contains**, rather than ordering the selected objects themselves.
//!
//! # Why these two are one derivation
//!
//! Both bodies run the same three-part shape and share their two install branches
//! instruction-for-instruction:
//!
//! 1. the `ScenarioData::ignore_orders` `0x00CC02F8` prune, through the `Group` vtable
//!    `+0x10` (`Group::kill`) — and **neither calls `Group::action_begin`** (vtable `+0x14`
//!    never appears in either body), so neither clears `GroupData::disband`;
//! 2. for each of `GroupData::list[0 .. num]`, a walk of that member's
//!    `ObjectData::inside_down` `+0x28` / `inside_down_who` `+0x3E` chain;
//! 3. per contained object, the same four predicates and the same two install branches,
//!    selected on `UnitTypeData::unit_flags` `+0x2B4 & 0x20`.
//!
//! # The measured walk
//!
//! ```text
//! 00711251  movsx eax, word ptr [ebp-0x18]        ; GroupData::list[i]      (+0x8CC, stride 2)
//! 0071126b  mov   eax, [0xC0618C]                 ; GameAccess::objects
//! 00711270  mov   eax, [eax + who*0x1C + 0x14]    ; Objects::lists[who] element pointer
//! 00711277  movsx esi, word ptr [eax + 0x28]      ; ObjectData::inside_down
//! 0071127b  movsx ecx, byte ptr [eax + 0x3E]      ; ObjectData::inside_down_who
//! 00711282  test  esi, esi / js                   ; chain ends at a negative link
//! ...
//! 007114ff  mov   eax, [0xC0618C]                 ; and the chain continues from the
//! 0071150b  movsx esi, word ptr [eax + 0x28]      ; object just visited, not from the
//! 0071150f  movsx ecx, byte ptr [eax + 0x3E]      ; group member
//! ```
//!
//! So the objects these two receivers order are **not** the group members. A selection of
//! airbases scrambles the aircraft *inside* the airbases; a selection of aircraft sitting
//! on the map has an empty `inside_down` chain and scrambles nothing.
//!
//! # Per-candidate predicates, in emitted order
//!
//! | # | test | evidence |
//! |---|---|---|
//! | 1 | object vtable `+0x18` (`is_unit`, constant 1 on `UnitData`) | `0x007112A7`, `0x007036A1` |
//! | 2 | `ObjectTypeData::domain` `+0x218` `== 2` | `0x007112C1`, `0x007036B9` |
//! | 3 | `UnitData::is_busy` `0x0060A370` `== 0` | `0x007112D0`, `0x007036D0` |
//! | 4 | `ObjectTypeData::obj_masks` `+0x1E4 & 0x08000000` clear | `0x007112EC`, `0x007036EC` |
//!
//! `action_scramble` then requires `UnitData::mana_burn` `+0x96 == 0` (`0x00711308`).
//! `action_launch_patrol` instead runs, in this order, an optional type filter and then the
//! same `+0x96` test *unless* its fourth argument is non-zero (`0x00703745..0x0070375F`).
//!
//! # `action_launch_patrol`'s six arguments are all on the wire
//!
//! `?action_launch_patrol@Group@@QAEXVCoord@@0W4QueuePos@@HHH@Z`, and
//! `CommandPackage::process_launch_patrol` `0x00949230` logs them through
//! `SyncLogger::logToMemory<int&,int&,enum QueuePos&,int&,int&,int&>` from
//! `cmd+1/+5/+9/+0xD/+0x11/+0x15` and pushes exactly those six at `0x00949335..0x0094936A`.
//! `LaunchPatrolCommand` is a 25-byte packet, which is what those six dwords plus the
//! opcode byte come to.
//!
//! Their measured meanings, named here after what they *do* because the PDB gives them no
//! names:
//!
//! * argument 4 (`cmd+0xD`), [`LaunchPatrolRequest::force_all`] — skips the `mana_burn`
//!   gate, forces the every-candidate install arm, and suppresses the single-best install.
//! * argument 5 (`cmd+0x11`), [`LaunchPatrolRequest::bombers_only`] — requires
//!   `ObjectData::is(BOMBER = 0x130, 0)`.
//! * argument 6 (`cmd+0x15`), [`LaunchPatrolRequest::fighters_only`] — requires
//!   `ObjectData::is(BIPLANE = 0x11F, 0)`, and **takes precedence**: when it is non-zero the
//!   bomber test is not emitted at all (`0x007036FC..0x0070370B`).
//!
//! # The launch cost, and the one plane that actually launches
//!
//! When neither `force_all` nor `QueuePos == 1` is set, the loop only *scores* candidates
//! and installs one order after the walk, on the cheapest. The score is computed from the
//! **group member's** position, not the aircraft's, so every aircraft in one hangar shares
//! its base distance:
//!
//! ```text
//! 00703792  esi = member->y_internal ^ 0x63637 ; esi -= to_y
//! 007037A1  eax = member->x_internal ^ 0x63637 ; eax -= to_x
//! 007037A9..B7  ecx = |dx| ; edx = |dy|
//! 007037B9  call vector_dist                    ; __fastcall(ecx, edx)
//! 007037CE  if  ObjectData::is(BIPLANE)    cost /= 10
//! 0070380F  elif ObjectData::is(HELICOPTER) cost /= 4
//! 0070384B  if the aircraft has a current order with a non-zero type, cost *= 200
//! 0070389B  if (cost < best) best = (this candidate)
//! ```
//!
//! `vector_dist` `0x0046CFF0` and `find_angle` `0x0092D130` are both `__fastcall(ecx, edx)`
//! despite their `__cdecl` PDB signatures; this module calls the already-ported
//! [`crate::systems::movement`] versions.
//!
//! # The two install branches
//!
//! `UnitTypeData::unit_flags & 0x20` — the same bit `recall_action_frontier` calls
//! `HELICOPTER_TYPE_FLAG` — selects between them.
//!
//! * **clear** → `Unit::add_air_patrol_order` `0x005E4350`
//!   `(Coord x, Coord y, int home_o, int home_who, int 1, QueuePos)`. Its sixth argument is
//!   **never read**: the body is `ret 0x18` and no instruction touches `[ebp+0x1C]`. All
//!   three call sites push a leftover register into that slot — `0x007114E8` pushes the
//!   base's `y`, `0x00703C64` and `0x00703C87` push a `Unit*`. `order_dispatch`'s
//!   [`crate::systems::order_dispatch::install_air_patrol`] already records that the
//!   installer ignores its `QueuePos`; this is the call-site half of the same fact.
//! * **set** → the body inlines `Unit::add_move_facing_order`'s `MOVE_TO` construction:
//!   `unit_masks &= ~0x04000000`, `Unit::close_orders(0)`, `Unit::clear_partial_path`,
//!   `Unit::update_action`, then `OrdersMemManager::get_obj(1)` — index 1, `MOVE_TO`, the
//!   immediate at `0x007113D2` / `0x0070395D` / `0x00703B5E` — storing
//!   `div_3_table[coord >> 4] * 0x30 + 0x18` into both `x`/`y` and `dest_x`/`dest_y`.
//!
//! The home object handed to `add_air_patrol_order` differs by arm, and this is measured,
//! not inferred:
//!
//! | arm | home argument | site |
//! |---|---|---|
//! | `action_scramble` | `ObjectData::get_inside` `0x00651A80` of the aircraft | `0x007114EB`/`0x007114F3` |
//! | `launch_patrol`, every-candidate | the **group member** and `GroupData::who` | `0x00703C6B`/`0x00703C6C` |
//! | `launch_patrol`, single best | `ObjectData::get_inside` of that aircraft | `0x00703AD7` |
//!
//! # What this module deliberately does not decide
//!
//! Every fact it needs is supplied by the caller and every unanswerable one is a named
//! [`AirLaunchBoundary`] rather than a default, so a host that does not model containment,
//! `UnitData::is_busy`, `ObjectData::is` or `+0x96` refuses the whole command instead of
//! launching a guessed set of aircraft.
//!
//! Tier C. Nothing here has been executed against retail.

use crate::command::group_action_entry::{ucell_order_destination, EntryGate, EntryProgram};
use crate::order::OrderIndex;
use crate::systems::groups_guys::formation_order_coord;
use crate::systems::movement::{find_angle, vector_dist};

/// `Group::action_scramble` `0x007111C0`, 894 bytes [PDB `S_GPROC32`].
pub const GROUP_ACTION_SCRAMBLE_VA: u32 = 0x0071_11c0;
pub const GROUP_ACTION_SCRAMBLE_BYTES: usize = 894;
/// `Group::action_launch_patrol` `0x00703580`, 2,043 bytes [PDB `S_GPROC32`].
pub const GROUP_ACTION_LAUNCH_PATROL_VA: u32 = 0x0070_3580;
pub const GROUP_ACTION_LAUNCH_PATROL_BYTES: usize = 2_043;
/// `Unit::add_air_patrol_order` `0x005E4350`, 527 bytes.
pub const UNIT_ADD_AIR_PATROL_ORDER_VA: u32 = 0x005e_4350;
/// `CommandPackage::process_scramble` `0x00947AE0`; `ScrambleCommand` is opcode 36.
pub const PROCESS_SCRAMBLE_VA: u32 = 0x0094_7ae0;
/// `CommandPackage::process_launch_patrol` `0x00949230`; `LaunchPatrolCommand` is opcode 11.
pub const PROCESS_LAUNCH_PATROL_VA: u32 = 0x0094_9230;
/// `LaunchPatrolCommand` is 25 bytes: the opcode plus six dwords.
pub const LAUNCH_PATROL_WIRE_SIZE: usize = 25;
/// `ScrambleCommand` is one byte: the opcode and nothing else.
pub const SCRAMBLE_WIRE_SIZE: usize = 1;

/// `ObjectTypeData::domain` `+0x218` value both receivers require.
pub const AIR_DOMAIN: i32 = 2;
/// `UnitTypeData::unit_flags` `+0x2B4` bit that routes to the inline `MOVE_TO` branch.
pub const HELICOPTER_TYPE_FLAG: u32 = 0x20;
/// `ObjectTypeData::obj_masks` `+0x1E4` bit that disqualifies a contained object.
pub const MISSILE_OBJECT_MASK: u32 = 0x0800_0000;
/// `UnitData::unit_masks` `+0x68` bit cleared before the inline `MOVE_TO` install.
pub const UNIT_MASK_LAUNCH_CLEAR: u32 = 0x0400_0000;

/// `TypeIndex::BIPLANE` [`schema/types.json`], the `fighters_only` filter and the `/10`
/// cost class.
pub const TYPE_BIPLANE: i32 = 0x11f;
/// `TypeIndex::BOMBER`, the `bombers_only` filter.
pub const TYPE_BOMBER: i32 = 0x130;
/// `TypeIndex::HELICOPTER`, the `/4` cost class.
pub const TYPE_HELICOPTER: i32 = 0x136;

/// The `QueuePos` value that forces the every-candidate install arm (`cmp [ebp+0x10], 1`
/// at `0x007038AF` and again at `0x00703AA4`).
pub const LAUNCH_ALL_QUEUE_POS: i32 = 1;
/// `mov eax, 0x98967F` at `0x00703616` — the initial best-cost sentinel.
pub const LAUNCH_COST_SENTINEL: i32 = 9_999_999;
/// `imul esi, esi, 0xC8` at `0x0070388E`.
pub const BUSY_ORDER_COST_FACTOR: i32 = 200;

/// The feedback reason slot `[ebp-0x18]`, initialised to 4 at `0x007035AD`.
///
/// Only `3` is ever written (`0x00703769`); the arms for `1` and `2` at `0x00703CC6` and
/// `0x00703CCB` are therefore **unreachable in this body** — a full scan of the 2,043 bytes
/// finds no other store to that slot. They are kept here because the arms exist and their
/// string ordinals are decodable, not because a launch can produce them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LaunchReason(pub i32);

impl LaunchReason {
    /// No refusal was recorded; `0x00703CD3` falls straight through to the return.
    pub const NONE: LaunchReason = LaunchReason(4);
    /// At least one candidate was refused because `force_all == 0` and
    /// `UnitData::mana_burn` `+0x96` was non-zero.
    pub const MANA_BURN: LaunchReason = LaunchReason(3);

    /// The `loc_str_array_orig` ordinal `MessageWin::add_feedback` is given, if any.
    ///
    /// `[0x00C8CD00]` is that table's element pointer over 20-byte `String` records, so a
    /// byte offset divided by 20 is the ordinal [standing board finding, re-checked: every
    /// offset here is an exact multiple of 20].
    pub fn feedback_ordinal(self) -> Option<u32> {
        match self.0 {
            1 => Some(0x841c / 20),
            2 => Some(0x83f4 / 20),
            3 => Some(0x8408 / 20),
            _ => None,
        }
    }
}

/// The entry prefix of `Group::action_scramble`.
///
/// It is *not* in `group_action_entry::ENTRY_PROGRAMS`, which covers the nine
/// movement/attack rows; the gate alphabet is shared, the row is declared here so that
/// module stays the movement family's.
///
/// `IgnoreOrdersPrune` then `NumPositive` — `0x007111C6` and `0x0071123F`. There is no
/// `ActionBegin`, no destination clamp, no `buildings` gate and no `form` clear.
pub const SCRAMBLE_ENTRY: EntryProgram = EntryProgram {
    action: "scramble",
    va: GROUP_ACTION_SCRAMBLE_VA,
    gates: &[EntryGate::IgnoreOrdersPrune, EntryGate::NumPositive],
};

/// One link of a group member's `ObjectData::inside_down` chain.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ContainedFacts {
    /// Engine addressing: `(inside_down_who, inside_down)`.
    pub object: (u8, i16),
    /// Object vtable `+0x18`. Constant 1 on `UnitData`, constant 0 on `BuildData`.
    pub is_unit: bool,
    /// `ObjectTypeData::domain` `+0x218`.
    pub domain: i32,
    /// `UnitData::is_busy` `0x0060A370`. `None` is "the host does not answer".
    pub busy: Option<bool>,
    /// `ObjectTypeData::obj_masks` `+0x1E4`. `None` is "the host does not answer".
    pub object_masks: Option<u32>,
    /// `UnitTypeData::unit_flags` `+0x2B4`.
    pub unit_flags: u32,
    /// `UnitData::mana_burn` `+0x96`. `None` is "the host does not answer".
    pub mana_burn: Option<i16>,
    /// `ObjectData::get_inside` `0x00651A80`: outer `None` is unanswered, inner `None` is
    /// retail's negative return.
    pub inside: Option<Option<(i16, u8)>>,
    /// `ObjectData::is(BIPLANE, 0)`; `None` is unanswered.
    pub is_biplane: Option<bool>,
    /// `ObjectData::is(BOMBER, 0)`; `None` is unanswered.
    pub is_bomber: Option<bool>,
    /// `ObjectData::is(HELICOPTER, 0)`; `None` is unanswered.
    pub is_helicopter: Option<bool>,
    /// The aircraft's own world Coord, already un-XORed.
    pub pos: (i32, i32),
    /// The aircraft has a current order whose `UnitOrder` vtable `+0x10` type is non-zero
    /// (`0x0070384B..0x0070388C`).
    pub busy_order: bool,
}

impl ContainedFacts {
    /// A fully answered candidate that passes predicates 1..4, for tests and for hosts
    /// that build facts field by field.
    pub fn eligible(object: (u8, i16), pos: (i32, i32)) -> Self {
        Self {
            object,
            is_unit: true,
            domain: AIR_DOMAIN,
            busy: Some(false),
            object_masks: Some(0),
            unit_flags: 0,
            mana_burn: Some(0),
            inside: Some(None),
            is_biplane: Some(false),
            is_bomber: Some(false),
            is_helicopter: Some(false),
            pos,
            busy_order: false,
        }
    }
}

/// One `GroupData::list` entry plus the chain hanging off it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MemberFacts {
    /// `(GroupData::who, GroupData::list[i])`.
    pub object: (u8, i16),
    /// The member's own world Coord — the position `action_scramble` patrols over and the
    /// position `action_launch_patrol` measures its distance from.
    pub pos: (i32, i32),
    /// `ObjectData::inside_down` chain in walk order. Empty when the head link is negative
    /// or the host does not model containment (see [`MemberFacts::containment_answered`]).
    pub contained: Vec<ContainedFacts>,
    /// Whether the host answered the containment question at all. `false` makes both
    /// planners refuse instead of treating the member as empty.
    pub containment_answered: bool,
}

/// Everything either receiver reads, captured before it writes anything.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct AirLaunchFacts {
    pub who: u8,
    pub members: Vec<MemberFacts>,
}

/// A fact the host could not supply. Both planners fail closed on the first one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AirLaunchBoundary {
    /// `ObjectData::inside_down`/`inside_down_who` are not modelled by this host.
    ContainmentUnanswered { member: (u8, i16) },
    /// `UnitData::is_busy` `0x0060A370`.
    BusyUnanswered { object: (u8, i16) },
    /// `ObjectTypeData::obj_masks` `+0x1E4`.
    ObjectMasksUnanswered { object: (u8, i16) },
    /// `UnitData::mana_burn` `+0x96`.
    ManaBurnUnanswered { object: (u8, i16) },
    /// `ObjectData::is(type_index, 0)` `0x00653790`.
    TypeQueryUnanswered { object: (u8, i16), type_index: i32 },
    /// `ObjectData::get_inside` `0x00651A80`.
    InsideUnanswered { object: (u8, i16) },
}

/// One order these receivers install, in the branch retail chose.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AirLaunchInstall {
    /// `Unit::add_air_patrol_order(x, y, home_o, home_who, 1, <unread>)`.
    AirPatrol {
        plane: (u8, i16),
        /// The commanded Coord, before `add_air_patrol_order`'s own conversion.
        x: i32,
        y: i32,
        /// Arguments 3 and 4. `None` is retail's `(-1, -1)` pair.
        home: Option<(i16, u8)>,
        /// Argument 5, `push 1` at every site: the installer's `group` flag.
        group_flag: bool,
    },
    /// The inlined `Unit::add_move_facing_order` `MOVE_TO` construction.
    MoveFacing {
        plane: (u8, i16),
        /// The commanded Coord the angle is measured to.
        raw: (i32, i32),
        /// What the order actually stores: `div_3_table[c >> 4] * 0x30 + 0x18` per axis.
        stored: (i32, i32),
        /// `find_angle(raw_x - plane_x, raw_y - plane_y)`.
        angle: i32,
        /// `unit_masks &= !UNIT_MASK_LAUNCH_CLEAR` precedes the install.
        clear_unit_mask: u32,
    },
}

impl AirLaunchInstall {
    /// The `OrderIndex` retail allocates through `OrdersMemManager::get_obj` for this arm.
    pub fn kind(&self) -> OrderIndex {
        match self {
            AirLaunchInstall::AirPatrol { .. } => OrderIndex::AirPatrol,
            AirLaunchInstall::MoveFacing { .. } => OrderIndex::MoveTo,
        }
    }

    pub fn plane(&self) -> (u8, i16) {
        match self {
            AirLaunchInstall::AirPatrol { plane, .. }
            | AirLaunchInstall::MoveFacing { plane, .. } => *plane,
        }
    }
}

/// `Group::action_scramble`'s whole effect.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ScramblePlan {
    pub installs: Vec<AirLaunchInstall>,
}

/// The six wire fields of `LaunchPatrolCommand`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LaunchPatrolRequest {
    /// `cmd+1`.
    pub to_x: i32,
    /// `cmd+5`.
    pub to_y: i32,
    /// `cmd+9`, the raw `QueuePos` word. Compared against `1` twice; never used as a
    /// queueing mode, because the installer it reaches ignores its `QueuePos`.
    pub queue: i32,
    /// `cmd+0xD`.
    pub force_all: i32,
    /// `cmd+0x11`.
    pub bombers_only: i32,
    /// `cmd+0x15`.
    pub fighters_only: i32,
}

impl LaunchPatrolRequest {
    /// `q == 1 || force_all != 0` — the every-candidate install arm at `0x007038AF`.
    pub fn launch_all(&self) -> bool {
        self.queue == LAUNCH_ALL_QUEUE_POS || self.force_all != 0
    }
}

/// The cheapest candidate the walk scored.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LaunchCandidate {
    pub plane: (u8, i16),
    pub cost: i32,
}

/// `Group::action_launch_patrol`'s whole effect.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct LaunchPatrolPlan {
    pub installs: Vec<AirLaunchInstall>,
    /// `[ebp-0x30]`/`[ebp-0x38]`/`[ebp-0x1C]` after the walk, when one was recorded.
    pub best: Option<LaunchCandidate>,
    /// `[ebp-0x34]`, set by the every-candidate arm.
    pub launched_any: bool,
    /// `[ebp-0x18]` at the return.
    pub reason: LaunchReason,
    /// The `MessageWin::add_feedback` ordinal, when the local player gets one. Retail
    /// gates this on `who == Console::who` `[[0x00C06210]+0x298]`, which is presentation;
    /// the planner reports it unconditionally and the caller decides.
    pub feedback_ordinal: Option<u32>,
}

/// `vector_dist(|bx - tx|, |by - ty|)`, then the two type divisors and the busy factor.
///
/// The divisions are the signed C `/`, which is what `sar edx, 2` plus the sign fixups at
/// `0x007037FC` and `0x00703838` implement; distances here are non-negative anyway.
pub fn launch_cost(
    base: (i32, i32),
    to: (i32, i32),
    is_biplane: bool,
    is_helicopter: bool,
    busy_order: bool,
) -> i32 {
    let dx = base.0.wrapping_sub(to.0).saturating_abs();
    let dy = base.1.wrapping_sub(to.1).saturating_abs();
    let mut cost = vector_dist(dx, dy);
    if is_biplane {
        cost /= 10;
    } else if is_helicopter {
        cost /= 4;
    }
    if busy_order {
        cost = cost.wrapping_mul(BUSY_ORDER_COST_FACTOR);
    }
    cost
}

/// The `x`/`y` an inlined `add_move_facing_order` stores, per axis.
fn stored_coord(c: i32) -> i32 {
    ucell_order_destination(formation_order_coord(c))
}

/// Predicates 1..4, shared by both receivers, in emitted order.
///
/// `Ok(false)` is retail skipping the candidate; `Err` is a fact the host withheld.
fn passes_common_predicates(c: &ContainedFacts) -> Result<bool, AirLaunchBoundary> {
    if !c.is_unit {
        return Ok(false);
    }
    if c.domain != AIR_DOMAIN {
        return Ok(false);
    }
    let Some(busy) = c.busy else {
        return Err(AirLaunchBoundary::BusyUnanswered { object: c.object });
    };
    if busy {
        return Ok(false);
    }
    let Some(masks) = c.object_masks else {
        return Err(AirLaunchBoundary::ObjectMasksUnanswered { object: c.object });
    };
    Ok(masks & MISSILE_OBJECT_MASK == 0)
}

/// Build the install for one aircraft, given the commanded Coord and the home pair.
fn install_for(c: &ContainedFacts, raw: (i32, i32), home: Option<(i16, u8)>) -> AirLaunchInstall {
    if c.unit_flags & HELICOPTER_TYPE_FLAG == 0 {
        AirLaunchInstall::AirPatrol {
            plane: c.object,
            x: raw.0,
            y: raw.1,
            home,
            group_flag: true,
        }
    } else {
        AirLaunchInstall::MoveFacing {
            plane: c.object,
            raw,
            stored: (stored_coord(raw.0), stored_coord(raw.1)),
            angle: find_angle(raw.0.wrapping_sub(c.pos.0), raw.1.wrapping_sub(c.pos.1)),
            clear_unit_mask: UNIT_MASK_LAUNCH_CLEAR,
        }
    }
}

/// `Group::action_scramble` `0x007111C0`, from the `num > 0` test onwards.
///
/// The scenario prune is [`SCRAMBLE_ENTRY`]'s job, not this function's.
pub fn plan_scramble(facts: &AirLaunchFacts) -> Result<ScramblePlan, AirLaunchBoundary> {
    let mut installs = Vec::new();
    for member in &facts.members {
        if !member.containment_answered {
            return Err(AirLaunchBoundary::ContainmentUnanswered {
                member: member.object,
            });
        }
        for c in &member.contained {
            if !passes_common_predicates(c)? {
                continue;
            }
            let Some(mana_burn) = c.mana_burn else {
                return Err(AirLaunchBoundary::ManaBurnUnanswered { object: c.object });
            };
            if mana_burn != 0 {
                continue;
            }
            let Some(inside) = c.inside else {
                return Err(AirLaunchBoundary::InsideUnanswered { object: c.object });
            };
            installs.push(install_for(c, member.pos, inside));
        }
    }
    Ok(ScramblePlan { installs })
}

/// `Group::action_launch_patrol` `0x00703580`, from the `num > 0` test onwards.
pub fn plan_launch_patrol(
    request: &LaunchPatrolRequest,
    facts: &AirLaunchFacts,
) -> Result<LaunchPatrolPlan, AirLaunchBoundary> {
    let to = (request.to_x, request.to_y);
    let launch_all = request.launch_all();
    let mut plan = LaunchPatrolPlan {
        reason: LaunchReason::NONE,
        ..LaunchPatrolPlan::default()
    };
    let mut best_cost = LAUNCH_COST_SENTINEL;

    for member in &facts.members {
        if !member.containment_answered {
            return Err(AirLaunchBoundary::ContainmentUnanswered {
                member: member.object,
            });
        }
        for c in &member.contained {
            if !passes_common_predicates(c)? {
                continue;
            }
            // 0x007036FC: the fighter filter shadows the bomber filter entirely.
            if request.fighters_only != 0 {
                let Some(is_it) = c.is_biplane else {
                    return Err(AirLaunchBoundary::TypeQueryUnanswered {
                        object: c.object,
                        type_index: TYPE_BIPLANE,
                    });
                };
                if !is_it {
                    continue;
                }
            } else if request.bombers_only != 0 {
                let Some(is_it) = c.is_bomber else {
                    return Err(AirLaunchBoundary::TypeQueryUnanswered {
                        object: c.object,
                        type_index: TYPE_BOMBER,
                    });
                };
                if !is_it {
                    continue;
                }
            }
            if request.force_all == 0 {
                let Some(mana_burn) = c.mana_burn else {
                    return Err(AirLaunchBoundary::ManaBurnUnanswered { object: c.object });
                };
                if mana_burn != 0 {
                    if plan.reason.0 > LaunchReason::MANA_BURN.0 {
                        plan.reason = LaunchReason::MANA_BURN;
                    }
                    continue;
                }
            }

            let (Some(is_biplane), Some(is_helicopter)) = (c.is_biplane, c.is_helicopter) else {
                return Err(AirLaunchBoundary::TypeQueryUnanswered {
                    object: c.object,
                    type_index: if c.is_biplane.is_none() {
                        TYPE_BIPLANE
                    } else {
                        TYPE_HELICOPTER
                    },
                });
            };
            let cost = launch_cost(member.pos, to, is_biplane, is_helicopter, c.busy_order);
            if request.force_all == 0 && cost < best_cost {
                best_cost = cost;
                plan.best = Some(LaunchCandidate {
                    plane: c.object,
                    cost,
                });
            }
            if launch_all {
                plan.launched_any = true;
                // 0x00703C6B/0x00703C6C: the home pair is the group member itself.
                let home = Some((member.object.1, member.object.0));
                plan.installs.push(install_for(c, to, home));
            }
        }
    }

    // 0x00703A92: a recorded best or any every-candidate install suppresses the feedback.
    if plan.best.is_none() && !plan.launched_any {
        plan.feedback_ordinal = plan.reason.feedback_ordinal();
        return Ok(plan);
    }
    // 0x00703AA4/0x00703AAE: the single-best install is skipped in both launch-all modes.
    if request.queue == LAUNCH_ALL_QUEUE_POS || request.force_all != 0 {
        return Ok(plan);
    }
    if let Some(best) = plan.best {
        let candidate = facts
            .members
            .iter()
            .flat_map(|m| m.contained.iter())
            .find(|c| c.object == best.plane)
            .expect("the best candidate came from this fact set");
        let Some(inside) = candidate.inside else {
            return Err(AirLaunchBoundary::InsideUnanswered {
                object: candidate.object,
            });
        };
        plan.installs.push(install_for(candidate, to, inside));
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(pos: (i32, i32), contained: Vec<ContainedFacts>) -> MemberFacts {
        MemberFacts {
            object: (2, 7),
            pos,
            contained,
            containment_answered: true,
        }
    }

    fn facts(members: Vec<MemberFacts>) -> AirLaunchFacts {
        AirLaunchFacts { who: 2, members }
    }

    #[test]
    fn scramble_orders_the_contained_aircraft_not_the_selected_member() {
        let plane = ContainedFacts {
            inside: Some(Some((7, 2))),
            ..ContainedFacts::eligible((2, 40), (1_000, 2_000))
        };
        let plan = plan_scramble(&facts(vec![base((4_800, 9_600), vec![plane])])).unwrap();
        assert_eq!(plan.installs.len(), 1);
        assert_eq!(
            plan.installs[0],
            AirLaunchInstall::AirPatrol {
                plane: (2, 40),
                x: 4_800,
                y: 9_600,
                home: Some((7, 2)),
                group_flag: true,
            }
        );
        // The member itself is never an install target.
        assert!(plan.installs.iter().all(|i| i.plane() != (2, 7)));
    }

    #[test]
    fn an_empty_containment_chain_scrambles_nothing() {
        let plan = plan_scramble(&facts(vec![base((4_800, 9_600), Vec::new())])).unwrap();
        assert!(plan.installs.is_empty());
    }

    #[test]
    fn each_withheld_fact_is_its_own_named_boundary() {
        let member = |c: ContainedFacts| facts(vec![base((0, 0), vec![c])]);
        let object = (2, 40);
        let cases: Vec<(ContainedFacts, AirLaunchBoundary)> = vec![
            (
                ContainedFacts {
                    busy: None,
                    ..ContainedFacts::eligible(object, (0, 0))
                },
                AirLaunchBoundary::BusyUnanswered { object },
            ),
            (
                ContainedFacts {
                    object_masks: None,
                    ..ContainedFacts::eligible(object, (0, 0))
                },
                AirLaunchBoundary::ObjectMasksUnanswered { object },
            ),
            (
                ContainedFacts {
                    mana_burn: None,
                    ..ContainedFacts::eligible(object, (0, 0))
                },
                AirLaunchBoundary::ManaBurnUnanswered { object },
            ),
            (
                ContainedFacts {
                    inside: None,
                    ..ContainedFacts::eligible(object, (0, 0))
                },
                AirLaunchBoundary::InsideUnanswered { object },
            ),
        ];
        for (c, expected) in cases {
            assert_eq!(plan_scramble(&member(c)).unwrap_err(), expected);
        }
        let mut unanswered = base((0, 0), Vec::new());
        unanswered.containment_answered = false;
        assert_eq!(
            plan_scramble(&facts(vec![unanswered])).unwrap_err(),
            AirLaunchBoundary::ContainmentUnanswered { member: (2, 7) }
        );
    }

    #[test]
    fn the_four_common_predicates_each_skip_a_candidate() {
        let object = (2, 40);
        let rejects = [
            ContainedFacts {
                is_unit: false,
                ..ContainedFacts::eligible(object, (0, 0))
            },
            ContainedFacts {
                domain: 0,
                ..ContainedFacts::eligible(object, (0, 0))
            },
            ContainedFacts {
                busy: Some(true),
                ..ContainedFacts::eligible(object, (0, 0))
            },
            ContainedFacts {
                object_masks: Some(MISSILE_OBJECT_MASK),
                ..ContainedFacts::eligible(object, (0, 0))
            },
            ContainedFacts {
                mana_burn: Some(1),
                ..ContainedFacts::eligible(object, (0, 0))
            },
        ];
        for c in rejects {
            let plan = plan_scramble(&facts(vec![base((0, 0), vec![c])])).unwrap();
            assert!(plan.installs.is_empty());
        }
    }

    #[test]
    fn the_helicopter_flag_selects_the_inline_move_to_branch() {
        let heli = ContainedFacts {
            unit_flags: HELICOPTER_TYPE_FLAG,
            ..ContainedFacts::eligible((2, 40), (4_000, 9_000))
        };
        let plan = plan_scramble(&facts(vec![base((4_800, 9_600), vec![heli])])).unwrap();
        let AirLaunchInstall::MoveFacing {
            raw, stored, angle, ..
        } = plan.installs[0]
        else {
            panic!("unit_flags & 0x20 must take the inline MOVE_TO branch");
        };
        assert_eq!(plan.installs[0].kind(), OrderIndex::MoveTo);
        assert_eq!(raw, (4_800, 9_600));
        assert_eq!(stored, (stored_coord(4_800), stored_coord(9_600)));
        assert_eq!(angle, find_angle(800, 600));
        // 4800 >> 4 = 300; div3 = 100; 100 * 0x30 + 0x18 = 4824.
        assert_eq!(stored, (4_824, 9_624));
    }

    fn plane_at(o: i16, base_pos: (i32, i32)) -> MemberFacts {
        MemberFacts {
            object: (2, o),
            pos: base_pos,
            contained: vec![ContainedFacts::eligible((2, o + 100), base_pos)],
            containment_answered: true,
        }
    }

    #[test]
    fn without_launch_all_only_the_cheapest_candidate_gets_an_order() {
        let request = LaunchPatrolRequest {
            to_x: 0,
            to_y: 0,
            queue: 0,
            ..LaunchPatrolRequest::default()
        };
        let far = plane_at(1, (10_000, 0));
        let near = plane_at(2, (100, 0));
        let plan = plan_launch_patrol(&request, &facts(vec![far, near])).unwrap();
        assert_eq!(plan.best.map(|b| b.plane), Some((2, 102)));
        assert_eq!(plan.installs.len(), 1);
        assert_eq!(plan.installs[0].plane(), (2, 102));
        assert!(!plan.launched_any);
    }

    #[test]
    fn queue_pos_one_launches_every_candidate_and_homes_them_on_their_member() {
        let request = LaunchPatrolRequest {
            to_x: 500,
            to_y: 600,
            queue: LAUNCH_ALL_QUEUE_POS,
            ..LaunchPatrolRequest::default()
        };
        let plan = plan_launch_patrol(
            &request,
            &facts(vec![plane_at(1, (0, 0)), plane_at(2, (9, 9))]),
        )
        .unwrap();
        assert!(plan.launched_any);
        assert_eq!(plan.installs.len(), 2);
        assert_eq!(
            plan.installs[0],
            AirLaunchInstall::AirPatrol {
                plane: (2, 101),
                x: 500,
                y: 600,
                home: Some((1, 2)),
                group_flag: true,
            }
        );
        assert_eq!(
            plan.installs[1],
            AirLaunchInstall::AirPatrol {
                plane: (2, 102),
                x: 500,
                y: 600,
                home: Some((2, 2)),
                group_flag: true,
            }
        );
    }

    #[test]
    fn force_all_bypasses_the_mana_burn_gate_and_records_no_best() {
        let mut member = plane_at(1, (0, 0));
        member.contained[0].mana_burn = Some(9);
        let request = LaunchPatrolRequest {
            force_all: 1,
            ..LaunchPatrolRequest::default()
        };
        let plan = plan_launch_patrol(&request, &facts(vec![member.clone()])).unwrap();
        assert_eq!(plan.installs.len(), 1);
        assert!(plan.best.is_none());
        assert_eq!(plan.reason, LaunchReason::NONE);

        let plain = LaunchPatrolRequest::default();
        let refused = plan_launch_patrol(&plain, &facts(vec![member])).unwrap();
        assert!(refused.installs.is_empty());
        assert_eq!(refused.reason, LaunchReason::MANA_BURN);
        assert_eq!(refused.feedback_ordinal, Some(1_690));
    }

    #[test]
    fn fighters_only_shadows_bombers_only() {
        let mut fighter = plane_at(1, (0, 0));
        fighter.contained[0].is_biplane = Some(true);
        let mut bomber = plane_at(2, (0, 0));
        bomber.contained[0].is_bomber = Some(true);
        let set = facts(vec![fighter, bomber]);

        let both = LaunchPatrolRequest {
            queue: LAUNCH_ALL_QUEUE_POS,
            bombers_only: 1,
            fighters_only: 1,
            ..LaunchPatrolRequest::default()
        };
        let plan = plan_launch_patrol(&both, &set).unwrap();
        assert_eq!(plan.installs.len(), 1);
        assert_eq!(plan.installs[0].plane(), (2, 101));

        let bombers = LaunchPatrolRequest {
            queue: LAUNCH_ALL_QUEUE_POS,
            bombers_only: 1,
            ..LaunchPatrolRequest::default()
        };
        let plan = plan_launch_patrol(&bombers, &set).unwrap();
        assert_eq!(plan.installs.len(), 1);
        assert_eq!(plan.installs[0].plane(), (2, 102));
    }

    #[test]
    fn a_busy_current_order_multiplies_the_cost_by_two_hundred() {
        let clean = launch_cost((1_000, 0), (0, 0), false, false, false);
        assert_eq!(
            launch_cost((1_000, 0), (0, 0), false, false, true),
            clean * BUSY_ORDER_COST_FACTOR
        );
        assert_eq!(
            launch_cost((1_000, 0), (0, 0), true, false, false),
            clean / 10
        );
        assert_eq!(
            launch_cost((1_000, 0), (0, 0), false, true, false),
            clean / 4
        );
        // The biplane divisor wins when a type answers both.
        assert_eq!(
            launch_cost((1_000, 0), (0, 0), true, true, false),
            clean / 10
        );
    }

    #[test]
    fn the_scramble_entry_program_has_no_action_begin() {
        assert_eq!(SCRAMBLE_ENTRY.va, GROUP_ACTION_SCRAMBLE_VA);
        assert_eq!(
            SCRAMBLE_ENTRY.gates,
            &[EntryGate::IgnoreOrdersPrune, EntryGate::NumPositive]
        );
        assert!(!SCRAMBLE_ENTRY
            .gates
            .iter()
            .any(|g| matches!(g, EntryGate::ActionBegin)));
    }

    #[test]
    fn only_reason_three_is_reachable_and_its_ordinals_are_multiples_of_twenty() {
        assert_eq!(LaunchReason::NONE.feedback_ordinal(), None);
        assert_eq!(LaunchReason::MANA_BURN.feedback_ordinal(), Some(1_690));
        assert_eq!(LaunchReason(1).feedback_ordinal(), Some(1_691));
        assert_eq!(LaunchReason(2).feedback_ordinal(), Some(1_689));
        for offset in [0x841c, 0x83f4, 0x8408] {
            assert_eq!(offset % 20, 0);
        }
    }
}
