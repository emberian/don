// SPDX-License-Identifier: GPL-3.0-or-later
//! Recovered group-level order installation for the economy `Group::action_*` rows.
//!
//! Three rows of the economy/production opcode family install unit orders directly from
//! the wire: `REPAIR` (16), `TRADE` (17) and `BOARD_SHIP` (15).  Before this module the
//! bridge routed all three through one generic "install the named `OrderIndex` on every
//! live member" shape.  That shape is wrong in ways this module makes explicit and
//! recovers:
//!
//! * all three reset `GroupData::form` to `-1` before installing anything;
//! * `action_repair` installs a **`CAST_SPELL` companion** alongside every `REPAIR`
//!   whose member can cast spell type `0x293`, and then retires the *repair target's*
//!   orders;
//! * `action_board_ship` installs `AWAIT_BOARD` **on the ship** for every boarding
//!   passenger and clears the ship's order list before the first one;
//! * `action_trade` carries **two** object endpoints, not one, and its member filter is
//!   selected by which virtual the trade destination answers.
//!
//! # Provenance and tier
//!
//! Everything below is `[measured]` by capstone disassembly of
//! `ron-bin/riseofnations.exe` (sha256 `30478a44…625079`, image base `0x00400000`),
//! named and layouted from `ron-bin/sbl/rise.pdb`.  **Tier C**: nothing here has been
//! executed against retail, and no differential run exists.  See
//! `docs/assembly/economy-group-actions.md`.
//!
//! | function | VA | bytes |
//! |---|---|---:|
//! | `Group::action_board_ship` | `0x00700010` | 1,149 |
//! | `Group::action_trade` | `0x00701CC0` | 1,022 |
//! | `Group::action_repair` | `0x007020C0` | 999 |
//! | `Unit::add_board_order` | `0x005E4D10` | 168 |
//! | `Unit::add_await_board_order` | `0x005E4C80` | 137 |
//! | `Unit::add_trade_order` | `0x005E4DC0` | — |
//! | `Unit::add_repair_order` | `0x005E4FF0` | 533 |
//! | `Unit::add_cast_order` | `0x005E4A60` | 541 |
//!
//! # Devirtualisation facts that the PDB alone does not give you
//!
//! MSVC folded several one-instruction virtuals, so the PDB name on a slot is the name of
//! *some* function with that body, not the name of the override being called.  Read at the
//! instruction level:
//!
//! | body VA | emitted body | meaning where these actions use it |
//! |---|---|---|
//! | `0x0041BFF0` | `xor eax,eax; ret` | constant `0` |
//! | `0x0041E0E0` | `mov eax,1; ret` | constant `1` |
//! | `0x0041C000` | `mov eax,ecx; ret` | identity — `Build`'s `+0xB0` returns `this` |
//! | `0x0046CDA0` | `movzx eax,[ecx+8]; and eax,1` | `ObjectData` flag byte `+0x08` bit 0 |
//! | `0x00472350` | `movzx eax,[ecx+8]; and eax,4` | `WallData::is_active`, flag byte bit 2 |
//! | `0x0046CE30` | `movzx eax,[ecx+0x82]; shr eax,15` | `UnitData::is_on_map` |
//! | `0x0046CE90` | `jmp [type->vt+0x130]` | `UnitData::is_caravan` |
//! | `0x006424E0` | `is(0x19E, 0)` | `WallData::is_trade` |
//! | `0x004722F0` | `is(0x1B0, 0)` | `WallData::is_sea_trade` |
//!
//! So `Unit` answers `1` on vslot `+0x08` and `0` on `+0x0C`, while `Build` answers `0` on
//! `+0x08` and its validity bit on `+0x0C`.  The member gate `vt[+0x08] && vt[+0xBC]` is
//! therefore exactly **"a live unit that is on the map"**, and buildings in the selection
//! are skipped by construction.
//!
//! # `Region::is_coast` is not a coastline test
//!
//! `Region::is_coast` `0x00680F90` takes the *other* region id and returns
//!
//! ```text
//! self == other                                   -> 1
//! self >= 0x40 && other >= 0x40                   -> 0
//! self <  0x40 && other <  0x40                   -> 0
//! self >= 0x40                                    -> bit (self-0x40) of regions[other].bits[+0x64]
//! otherwise                                       -> bit (other-0x40) of regions[self ].bits[+0x64]
//! ```
//!
//! i.e. "same region, or a land/water pair that touch".  All three actions call it as
//! `regions[get_tregion(member)].is_coast(get_tregion(target))`, where
//! `get_tregion` is `WorldData::get_tregion` `0x006B52E0` applied to the tile pair
//! `div_3_table[(coord ^ 0x63637) >> 6]` (`div_3_table` is `0x00CAE5FC`).  The bridge holds
//! no world, so that whole composite is one host fact — [`MemberFacts::regions_touch`].
//!
//! # Unresolved facts are named, never guessed
//!
//! Every conditional read the retail body performs is a [`Fact`].  `Fact::Unknown` is not
//! `Fact::Known(false)`.  A host that answers nothing produces a plan whose installed
//! order set is **bit-identical to the pre-recovery bridge** — that is the deliberate
//! resolution policy, stated per gate in [`Fact::or_legacy`] — and whose
//! [`EconomyPlan::unresolved`] names every fact that was not available.  A plan is
//! retail-exact only when `unresolved` is empty; [`EconomyPlan::is_exact`] is the test, and
//! a host that must fail closed checks it.

use crate::order::OrderIndex;
use crate::systems::groups_guys::{GroupData, GROUP_MAX_MEMBERS};

// ---------------------------------------------------------------------------
// Retail addresses and constants
// ---------------------------------------------------------------------------

pub const GROUP_ACTION_BOARD_SHIP_VA: u32 = 0x0070_0010;
pub const GROUP_ACTION_TRADE_VA: u32 = 0x0070_1cc0;
pub const GROUP_ACTION_REPAIR_VA: u32 = 0x0070_20c0;
pub const UNIT_ADD_BOARD_ORDER_VA: u32 = 0x005e_4d10;
pub const UNIT_ADD_AWAIT_BOARD_ORDER_VA: u32 = 0x005e_4c80;
pub const UNIT_ADD_TRADE_ORDER_VA: u32 = 0x005e_4dc0;
pub const UNIT_ADD_REPAIR_ORDER_VA: u32 = 0x005e_4ff0;
pub const UNIT_ADD_CAST_ORDER_VA: u32 = 0x005e_4a60;
pub const REGION_IS_COAST_VA: u32 = 0x0068_0f90;
pub const WORLD_GET_TREGION_VA: u32 = 0x006b_52e0;
pub const SPELLTYPE_IS_CASTABLE_VA: u32 = 0x0067_5bc0;
pub const OBJECTDATA_CAN_CARRY_VA: u32 = 0x0064_83c0;
pub const UNITDATA_IS_BUSY_VA: u32 = 0x0060_a370;
pub const GROUPDATA_COUNT_VA: u32 = 0x0071_1720;

pub const BOARD_SHIP_WIRE_BYTES: usize = 9;
pub const REPAIR_WIRE_BYTES: usize = 13;
pub const TRADE_WIRE_BYTES: usize = 21;

/// `QueuePos`, `[measured, PDB LF_FIELDLIST 0x216F]`.
pub const QUEUE_FIRST: i32 = 0;
pub const QUEUE_LAST: i32 = 1;
pub const QUEUE_NEW: i32 = 2;

/// `Group::action_repair` accepts only members whose `ObjectTypeData +0x04` is one of
/// these two `[measured, 0x007022C3]`.
pub const REPAIR_MEMBER_TYPE_CLASSES: [i32; 2] = [0x32, 0x33];

/// The spell type index `action_repair` offers alongside every `REPAIR`
/// `[measured, 0x0070236B and 0x0070240A]`.  It reaches `spelltypes[0x293]` through
/// `GameAccess::spelltypes` `0x00C061A8` (`[[0xC061A8]+0x10]+0xA4C`).
pub const REPAIR_CAST_SPELL_TYPE: i32 = 0x293;

/// `UnitData::unit_masks +0x68` bit cleared on the repair target
/// `[measured, and dword [edi+0x68], 0xFBFFFFFF at 0x0070245D]`.
pub const CAST_SPELL_ACTIVE_MASK: u32 = 0x0400_0000;

/// `WallData::is_trade` = `ObjectData::is(0x19E, 0)`.
pub const TRADE_MARKET_TYPE: i32 = 0x19e;
/// `WallData::is_sea_trade` = `ObjectData::is(0x1B0, 0)`.
pub const TRADE_SEA_DESTINATION_TYPE: i32 = 0x1b0;
/// The type a member must `is()` on the sea-trade branch `[measured, 0x00702012]`.
pub const TRADE_SEA_MEMBER_TYPE: i32 = 0x13e;
/// `GroupData::count(0x11, …)`'s two accepted type arguments `[measured, 0x00701E2x]`.
pub const TRADE_GROUP_COUNT_INDEX: i32 = 0x11;
pub const TRADE_GROUP_COUNT_TYPES: [i32; 2] = [0x3b, 0x13e];

/// `ObjectTypeData::domain +0x218`; `action_board_ship` rejects domain 2
/// `[measured, cmp dword [eax+0x218], 2 at 0x00700272]`.  `UnitData::is_plane`
/// `0x0046CE40` reads the same field, so domain 2 is AIR.
pub const BOARD_SHIP_REJECTED_DOMAIN: i32 = 2;

// ---------------------------------------------------------------------------
// Host facts
// ---------------------------------------------------------------------------

/// One conditional read the retail body performs on state the bridge does not hold.
///
/// `Unknown` carries the name of the retail read, never a substituted value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fact<T> {
    Known(T),
    Unknown(&'static str),
}

impl<T: Copy> Fact<T> {
    pub const fn known(v: T) -> Self {
        Self::Known(v)
    }

    pub const fn unknown(name: &'static str) -> Self {
        Self::Unknown(name)
    }

    pub fn value(self) -> Option<T> {
        match self {
            Self::Known(v) => Some(v),
            Self::Unknown(_) => None,
        }
    }

    pub fn missing(self) -> Option<&'static str> {
        match self {
            Self::Known(_) => None,
            Self::Unknown(name) => Some(name),
        }
    }
}

impl Fact<bool> {
    /// Resolve a gate, recording the retail read when it was unavailable.
    ///
    /// `legacy` is the answer that keeps the plan's installed order set identical to the
    /// pre-recovery bridge, so a host which supplies no facts is unchanged by this module.
    /// It is a **policy**, not a claim about retail: an `Unknown` gate always lands in
    /// [`EconomyPlan::unresolved`], and a plan with a non-empty `unresolved` is not
    /// retail-exact.
    fn or_legacy(self, legacy: bool, unresolved: &mut Vec<&'static str>) -> bool {
        match self {
            Self::Known(v) => v,
            Self::Unknown(name) => {
                if !unresolved.contains(&name) {
                    unresolved.push(name);
                }
                legacy
            }
        }
    }
}

/// Facts about one selected member, read in the order the retail loop reads them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemberFacts {
    /// `ObjectData::list[who][o]`, the member slot.
    pub o: i16,
    /// Object vslot `+0x08` — a live `Unit`, not a `Build`.  The bridge answers this from
    /// its own table, so it is never `Unknown`.
    pub live_unit: bool,
    /// Object vslot `+0xBC`, `UnitData::is_on_map`.
    pub on_map: bool,
    /// `ObjectTypeData +0x04`, the type class word `action_repair` compares against
    /// [`REPAIR_MEMBER_TYPE_CLASSES`].
    pub type_class: Fact<i32>,
    /// `ObjectTypeData::domain +0x218`.
    pub domain: Fact<i32>,
    /// `UnitData::is_busy` `0x0060A370`.
    pub busy: Fact<bool>,
    /// `regions[get_tregion(member)].is_coast(get_tregion(target))`.
    pub regions_touch: Fact<bool>,
    /// `SpellTypeData::is_castable(o, who, 0)` on `spelltypes[0x293]`.
    pub repair_spell_castable: Fact<bool>,
    /// `UnitData::is_caravan`, object vslot `+0xD0`.
    pub is_caravan: Fact<bool>,
    /// `ObjectData::is(0x13E, 0)`.
    pub is_sea_trade_member: Fact<bool>,
    /// `ObjectData::can_carry(ship, o, who)` `0x006483C0`, asked of the *ship*.
    pub ship_can_carry: Fact<bool>,
    /// The member's world position, already un-XORed. `add_cast_order` stores it verbatim.
    pub x: i32,
    pub y: i32,
}

impl MemberFacts {
    /// A member about which the host volunteered nothing beyond its own liveness.
    pub fn opaque(o: i16, live_unit: bool, on_map: bool, x: i32, y: i32) -> Self {
        MemberFacts {
            o,
            live_unit,
            on_map,
            type_class: Fact::unknown("ObjectTypeData+0x04"),
            domain: Fact::unknown("ObjectTypeData::domain+0x218"),
            busy: Fact::unknown("UnitData::is_busy 0x0060A370"),
            regions_touch: Fact::unknown("Region::is_coast 0x00680F90"),
            repair_spell_castable: Fact::unknown("SpellTypeData::is_castable 0x00675BC0"),
            is_caravan: Fact::unknown("UnitData::is_caravan vslot+0xD0"),
            is_sea_trade_member: Fact::unknown("ObjectData::is(0x13E)"),
            ship_can_carry: Fact::unknown("ObjectData::can_carry 0x006483C0"),
            x,
            y,
        }
    }
}

/// Facts about the addressed object of `TRADE`, read before any member is touched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeTargetFacts {
    /// Object vslot `+0x0C` — a live `Build`.
    pub live_building: Fact<bool>,
    /// Object vslot `+0x4C`, `WallData::is_active` (flag byte `+0x08` bit 2).
    pub active: Fact<bool>,
    /// The **entry gate** read: `target->vt[+0xB0]()->vt[+0x24]()`
    /// `[measured, 0x00701DB3..0x00701DF1]`.  This is a different receiver from
    /// [`Self::is_trade`] below: the member loop re-reads `+0x24` on the target *itself*
    /// `[measured, 0x00701FB6]`.  They coincide only because `Build`'s `+0xB0` is
    /// `mov eax,ecx; ret` `0x0041C000`; a subclass that returns a contained `BuildData`
    /// from `+0xB0` would make them disagree, and this module does not model that class.
    pub build_is_trade: Fact<bool>,
    /// The **member-loop** read: `target->vt[+0x24]()`, `WallData::is_trade` = `is(0x19E, 0)`.
    pub is_trade: Fact<bool>,
    /// Object vslot `+0x28`, `WallData::is_sea_trade` = `is(0x1B0, 0)`.
    pub is_sea_trade: Fact<bool>,
    /// `GroupData::count(0x11, 0x3B, 0) != 0 || GroupData::count(0x11, 0x13E, 0) != 0`.
    pub group_has_trader: Fact<bool>,
}

impl TradeTargetFacts {
    pub fn opaque() -> Self {
        TradeTargetFacts {
            live_building: Fact::unknown("ObjectData vslot+0x0C"),
            active: Fact::unknown("WallData::is_active vslot+0x4C"),
            build_is_trade: Fact::unknown("WallData::is_trade vslot+0xB0->+0x24"),
            is_trade: Fact::unknown("WallData::is_trade vslot+0x24"),
            is_sea_trade: Fact::unknown("WallData::is_sea_trade vslot+0x28"),
            group_has_trader: Fact::unknown("GroupData::count 0x00711720"),
        }
    }
}

// ---------------------------------------------------------------------------
// Plans
// ---------------------------------------------------------------------------

/// One ordered mutation the receiver must apply.  Order is load-bearing: the two
/// `REPAIR` arms below produce the same final queue only because their `QueuePos`
/// arguments differ, and the ship's `AWAIT_BOARD` follows the passenger's `BOARD_SHIP`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EconomyStep {
    /// `mov dword [group+0x10], -1` — `GroupData::form`.
    SetGroupForm(i32),
    /// `Unit::add_cast_order(-1, -1, x, y, spell, queue, 1)` on the member.
    AddCastOrder {
        o: i16,
        x: i32,
        y: i32,
        spell: i32,
        queue: i32,
    },
    /// `Unit::add_repair_order(ox, whom, queue, 1)` on the member.
    AddRepairOrder {
        o: i16,
        target_o: i32,
        target_who: i32,
        queue: i32,
    },
    /// `Unit::add_trade_order(ox, whom, oxx, whose, queue, 1)` on the member.
    ///
    /// Both endpoints are carried because retail carries both: `TradeOrder` has
    /// `(ox, whom, uid)` at `+0x08..+0x12` **and** `(oxx, whose, uid2)` at
    /// `+0x14..+0x26` (`crates/don-sim/src/systems/trade_order_frontier.rs`).
    AddTradeOrder {
        o: i16,
        ox: i32,
        whom: i32,
        oxx: i32,
        whose: i32,
        queue: i32,
    },
    /// `Unit::add_board_order(ox, who, queue, <unread>)` on the passenger.
    AddBoardOrder { o: i16, ship_o: i32, queue: i32 },
    /// `Unit::clear_orders()` `0x005E3860` on the ship, before the first passenger boards.
    ClearShipOrders { ship_o: i32 },
    /// `Unit::add_await_board_order(o, who, <unread>, 1)` on the **ship**.
    ///
    /// `0x005E4C80` never reads its third argument and has no `QUEUE_NEW` arm: it always
    /// appends, with the order's flag bit `0x04` set because the fourth argument is `1`.
    AddAwaitBoardOrder { ship_o: i32, passenger_o: i16 },
    /// The five-step retirement `action_repair` performs on `units[whom][ox]` — the repair
    /// *target*, not the member — after each accepted member `[measured, 0x0070244A]`.
    RetireRepairTarget { target_o: i32, target_who: i32 },
}

impl EconomyStep {
    /// The `OrderIndex` this step allocates, when it allocates one.
    pub fn installs(self) -> Option<OrderIndex> {
        match self {
            EconomyStep::AddCastOrder { .. } => Some(OrderIndex::CastSpell),
            EconomyStep::AddRepairOrder { .. } => Some(OrderIndex::Repair),
            EconomyStep::AddTradeOrder { .. } => Some(OrderIndex::TradeRoute),
            EconomyStep::AddBoardOrder { .. } => Some(OrderIndex::BoardShip),
            EconomyStep::AddAwaitBoardOrder { .. } => Some(OrderIndex::AwaitBoard),
            _ => None,
        }
    }
}

/// A recovered `Group::action_*` body, as ordered steps plus the facts it could not read.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct EconomyPlan {
    pub steps: Vec<EconomyStep>,
    /// Retail reads the host did not answer, in first-encounter order, deduplicated.
    pub unresolved: Vec<&'static str>,
    /// Members the local-player presentation tail would have counted as "no room".
    /// `action_board_ship` only ever uses this for a `MessageWin` call, so the bridge
    /// carries it as evidence rather than acting on it.
    pub blocked_members: u32,
}

impl EconomyPlan {
    /// Whether every gate the retail body evaluates was actually available.
    pub fn is_exact(&self) -> bool {
        self.unresolved.is_empty()
    }
}

/// The `QueuePos` retail hands to the per-member `add_*_order` call.
///
/// All three bodies compute `queued == QUEUE_LAST ? QUEUE_LAST : QUEUE_NEW`; `QUEUE_FIRST`
/// never reaches this point because the group layer converts it by re-entering with
/// `QUEUE_NEW` `[measured, 0x00702083, 0x0070034E, 0x007023AA]`.
pub fn member_queue(queued: i32) -> i32 {
    if queued == QUEUE_LAST {
        QUEUE_LAST
    } else {
        QUEUE_NEW
    }
}

fn live_members(group: &GroupData) -> usize {
    group.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize
}

/// Members must be the group's live prefix, in list order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EconomyPlanError {
    MemberCount,
    MemberIdentity,
}

fn check_members(group: &GroupData, members: &[MemberFacts]) -> Result<(), EconomyPlanError> {
    let n = live_members(group);
    if members.len() != n {
        return Err(EconomyPlanError::MemberCount);
    }
    if members
        .iter()
        .zip(&group.list[..n])
        .any(|(facts, &o)| facts.o != o)
    {
        return Err(EconomyPlanError::MemberIdentity);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// REPAIR — Group::action_repair 0x007020C0
// ---------------------------------------------------------------------------

/// `Group::action_repair(ox, whom, queued)`, the arm reached with `queued != QUEUE_FIRST`.
///
/// `ScenarioData::ignore_orders` prelude and concrete `Group::action_begin` run before
/// this and are the caller's; the first step here is retail's `group.form = -1`.
///
/// Per accepted member, retail emits either
///
/// ```text
/// QUEUE_LAST : [cast(0x293, QUEUE_LAST)]? , repair(QUEUE_LAST)
/// otherwise  : repair(QUEUE_NEW) , [cast(0x293, QUEUE_FIRST)]?
/// ```
///
/// `[measured, 0x00702336 selects the arm; 0x0070235E/0x007023A6 and 0x007023CE/0x00702402
/// are the two call pairs]`.  The call order is load-bearing and is *not* symmetric:
/// `QUEUE_NEW` is the argument that makes `Unit::add_repair_order` retire the member's
/// existing list, so the repair call has to come first on that arm or the cast order would
/// be retired with everything else.
///
/// **Not recovered here:** where each order lands in the member's list.
/// `Unit::add_cast_order` `0x005E4A60` special-cases only `queued == 2`; for `queued == 0`
/// it does the same plain `LinkListBase<UnitOrder*>::add` `0x0046D5A0` as `queued == 1`,
/// plus a `Unit::clear_partial_path`.  `0x0046D5A0` inserts *before the list's current
/// node* and then makes the new node current, which this lane did not chase far enough to
/// state a resulting index for.  The plan therefore carries the exact `QueuePos` retail
/// passes and leaves list placement to the receiver's own queue model.
pub fn plan_repair(
    group: &GroupData,
    ox: i32,
    whom: i32,
    queued: i32,
    members: &[MemberFacts],
) -> Result<EconomyPlan, EconomyPlanError> {
    check_members(group, members)?;
    let mut plan = EconomyPlan {
        steps: vec![EconomyStep::SetGroupForm(-1)],
        ..EconomyPlan::default()
    };
    let queue = member_queue(queued);
    for m in members {
        if !m.live_unit || !m.on_map {
            continue;
        }
        let class_ok = match m.type_class {
            Fact::Known(k) => REPAIR_MEMBER_TYPE_CLASSES.contains(&k),
            Fact::Unknown(name) => Fact::Unknown(name).or_legacy(true, &mut plan.unresolved),
        };
        if !class_ok {
            continue;
        }
        if !m
            .regions_touch
            .or_legacy(true, &mut plan.unresolved)
        {
            continue;
        }
        let castable = m
            .repair_spell_castable
            .or_legacy(false, &mut plan.unresolved);
        let cast = EconomyStep::AddCastOrder {
            o: m.o,
            x: m.x,
            y: m.y,
            spell: REPAIR_CAST_SPELL_TYPE,
            queue: if queue == QUEUE_LAST {
                QUEUE_LAST
            } else {
                QUEUE_FIRST
            },
        };
        let repair = EconomyStep::AddRepairOrder {
            o: m.o,
            target_o: ox,
            target_who: whom,
            queue,
        };
        if queue == QUEUE_LAST {
            if castable {
                plan.steps.push(cast);
            }
            plan.steps.push(repair);
        } else {
            plan.steps.push(repair);
            if castable {
                plan.steps.push(cast);
            }
        }
        plan.steps.push(EconomyStep::RetireRepairTarget {
            target_o: ox,
            target_who: whom,
        });
    }
    Ok(plan)
}

// ---------------------------------------------------------------------------
// TRADE — Group::action_trade 0x00701CC0
// ---------------------------------------------------------------------------

/// Which member filter `action_trade` selects, decided by the destination's own virtuals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradeMemberFilter {
    /// Destination answers `is_trade`: members must be caravans.
    Caravan,
    /// Destination answers `is_sea_trade` only: members must `is(0x13E)` **and** their
    /// region must touch the destination's.
    SeaTrader,
    /// Destination answers neither: retail applies no member filter at all.
    None,
}

/// `Group::action_trade(ox, whom, oxx, whose, queued)`.
///
/// Returns `None` when an entry gate rejects the whole command — retail returns without
/// touching `GroupData::form`, so a rejected command is a complete no-op after
/// `action_begin`.
pub fn plan_trade(
    group: &GroupData,
    ox: i32,
    whom: i32,
    oxx: i32,
    whose: i32,
    queued: i32,
    target: TradeTargetFacts,
    members: &[MemberFacts],
) -> Result<Option<EconomyPlan>, EconomyPlanError> {
    check_members(group, members)?;
    let mut unresolved: Vec<&'static str> = Vec::new();
    if !target.live_building.or_legacy(true, &mut unresolved) {
        return Ok(None);
    }
    if !target.active.or_legacy(true, &mut unresolved) {
        return Ok(None);
    }
    if !target.build_is_trade.or_legacy(true, &mut unresolved) {
        return Ok(None);
    }
    if !target.group_has_trader.or_legacy(true, &mut unresolved) {
        return Ok(None);
    }

    // The member loop re-reads `+0x24` on the target itself, and falls through to `+0x28`
    // only when that answers zero `[measured, 0x00701FB6 / 0x00702005]`.
    let filter = if target.is_trade.or_legacy(true, &mut unresolved) {
        TradeMemberFilter::Caravan
    } else if target.is_sea_trade.or_legacy(false, &mut unresolved) {
        TradeMemberFilter::SeaTrader
    } else {
        TradeMemberFilter::None
    };

    let mut plan = EconomyPlan {
        steps: vec![EconomyStep::SetGroupForm(-1)],
        unresolved,
        blocked_members: 0,
    };
    let queue = member_queue(queued);
    for m in members {
        if !m.live_unit || !m.on_map {
            continue;
        }
        let accepted = match filter {
            TradeMemberFilter::Caravan => m.is_caravan.or_legacy(true, &mut plan.unresolved),
            TradeMemberFilter::SeaTrader => {
                m.is_sea_trade_member
                    .or_legacy(true, &mut plan.unresolved)
                    && m.regions_touch.or_legacy(true, &mut plan.unresolved)
            }
            TradeMemberFilter::None => true,
        };
        if !accepted {
            continue;
        }
        plan.steps.push(EconomyStep::AddTradeOrder {
            o: m.o,
            ox,
            whom,
            oxx,
            whose,
            queue,
        });
    }
    Ok(Some(plan))
}

// ---------------------------------------------------------------------------
// BOARD_SHIP — Group::action_board_ship 0x00700010
// ---------------------------------------------------------------------------

/// `Group::action_board_ship(ox, queued)`.
///
/// The ship belongs to `group.who`: `BoardShipCommand` carries no owner field, and the
/// body resolves the ship as `objects[group.who][ox]` `[measured, 0x00700184]`.
///
/// Retail's per-member sequence is `add_board_order` on the passenger, then — only before
/// the first successful boarder — `Unit::clear_orders()` on the ship, then
/// `add_await_board_order` on the ship for that passenger.  The bridge reproduces that
/// order because the ship's own list is cleared *between* the first passenger's order and
/// the first `AWAIT_BOARD`.
pub fn plan_board_ship(
    group: &GroupData,
    ox: i32,
    queued: i32,
    members: &[MemberFacts],
) -> Result<EconomyPlan, EconomyPlanError> {
    check_members(group, members)?;
    let mut plan = EconomyPlan {
        steps: vec![EconomyStep::SetGroupForm(-1)],
        ..EconomyPlan::default()
    };
    let queue = member_queue(queued);
    let mut boarded = 0u32;
    for m in members {
        if !m.live_unit || !m.on_map {
            continue;
        }
        let domain_ok = match m.domain {
            Fact::Known(d) => d != BOARD_SHIP_REJECTED_DOMAIN,
            Fact::Unknown(name) => Fact::Unknown(name).or_legacy(true, &mut plan.unresolved),
        };
        if !domain_ok {
            continue;
        }
        if m.busy.or_legacy(false, &mut plan.unresolved) {
            continue;
        }
        if !m.regions_touch.or_legacy(true, &mut plan.unresolved) {
            continue;
        }
        if !m.ship_can_carry.or_legacy(true, &mut plan.unresolved) {
            plan.blocked_members += 1;
            continue;
        }
        plan.steps.push(EconomyStep::AddBoardOrder {
            o: m.o,
            ship_o: ox,
            queue,
        });
        if boarded == 0 {
            plan.steps.push(EconomyStep::ClearShipOrders { ship_o: ox });
        }
        boarded += 1;
        plan.steps.push(EconomyStep::AddAwaitBoardOrder {
            ship_o: ox,
            passenger_o: m.o,
        });
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group_with(members: &[i16], who: u8) -> GroupData {
        let mut g = GroupData {
            who,
            num: members.len() as i32,
            form: 7,
            ..GroupData::default()
        };
        for (i, &o) in members.iter().enumerate() {
            g.list[i] = o;
        }
        g
    }

    fn live(o: i16) -> MemberFacts {
        MemberFacts::opaque(o, true, true, 100, 200)
    }

    #[test]
    fn repair_emits_the_exact_call_pair_for_each_queue_arm() {
        let g = group_with(&[4], 1);
        let mut m = live(4);
        m.type_class = Fact::known(0x32);
        m.regions_touch = Fact::known(true);
        m.repair_spell_castable = Fact::known(true);

        let last = plan_repair(&g, 9, 2, QUEUE_LAST, &[m]).unwrap();
        assert!(last.is_exact());
        assert_eq!(
            last.steps,
            vec![
                EconomyStep::SetGroupForm(-1),
                EconomyStep::AddCastOrder {
                    o: 4,
                    x: 100,
                    y: 200,
                    spell: REPAIR_CAST_SPELL_TYPE,
                    queue: QUEUE_LAST,
                },
                EconomyStep::AddRepairOrder {
                    o: 4,
                    target_o: 9,
                    target_who: 2,
                    queue: QUEUE_LAST,
                },
                EconomyStep::RetireRepairTarget {
                    target_o: 9,
                    target_who: 2,
                },
            ]
        );

        let new = plan_repair(&g, 9, 2, QUEUE_NEW, &[m]).unwrap();
        assert_eq!(
            new.steps,
            vec![
                EconomyStep::SetGroupForm(-1),
                EconomyStep::AddRepairOrder {
                    o: 4,
                    target_o: 9,
                    target_who: 2,
                    queue: QUEUE_NEW,
                },
                EconomyStep::AddCastOrder {
                    o: 4,
                    x: 100,
                    y: 200,
                    spell: REPAIR_CAST_SPELL_TYPE,
                    queue: QUEUE_FIRST,
                },
                EconomyStep::RetireRepairTarget {
                    target_o: 9,
                    target_who: 2,
                },
            ]
        );
    }

    #[test]
    fn repair_rejects_a_member_outside_the_two_type_classes() {
        let g = group_with(&[4], 1);
        let mut m = live(4);
        m.type_class = Fact::known(0x31);
        m.regions_touch = Fact::known(true);
        m.repair_spell_castable = Fact::known(false);
        let plan = plan_repair(&g, 9, 2, QUEUE_NEW, &[m]).unwrap();
        assert!(plan.is_exact());
        assert_eq!(plan.steps, vec![EconomyStep::SetGroupForm(-1)]);
    }

    #[test]
    fn an_unanswered_gate_is_named_and_never_silently_true() {
        let g = group_with(&[4], 1);
        let plan = plan_repair(&g, 9, 2, QUEUE_NEW, &[live(4)]).unwrap();
        assert!(!plan.is_exact());
        assert!(plan.unresolved.contains(&"ObjectTypeData+0x04"));
        assert!(plan
            .unresolved
            .contains(&"SpellTypeData::is_castable 0x00675BC0"));
        // The legacy resolution installs REPAIR and no cast: identical to the shape the
        // bridge had before this module existed.
        assert_eq!(
            plan.steps
                .iter()
                .filter_map(|s| s.installs())
                .collect::<Vec<_>>(),
            vec![OrderIndex::Repair]
        );
    }

    #[test]
    fn trade_entry_gates_reject_the_whole_command_without_touching_form() {
        let g = group_with(&[4], 1);
        let mut t = TradeTargetFacts::opaque();
        t.live_building = Fact::known(true);
        t.active = Fact::known(false);
        t.build_is_trade = Fact::known(true);
        t.is_trade = Fact::known(true);
        t.group_has_trader = Fact::known(true);
        assert_eq!(
            plan_trade(&g, 1, 2, 3, 4, QUEUE_NEW, t, &[live(4)]).unwrap(),
            None
        );
    }

    #[test]
    fn trade_carries_both_endpoints_and_selects_the_caravan_filter() {
        let g = group_with(&[4, 5], 1);
        let mut t = TradeTargetFacts::opaque();
        t.live_building = Fact::known(true);
        t.active = Fact::known(true);
        t.build_is_trade = Fact::known(true);
        t.is_trade = Fact::known(true);
        t.is_sea_trade = Fact::known(false);
        t.group_has_trader = Fact::known(true);
        let mut caravan = live(4);
        caravan.is_caravan = Fact::known(true);
        let mut other = live(5);
        other.is_caravan = Fact::known(false);
        let plan = plan_trade(&g, 1, 2, 3, 4, QUEUE_LAST, t, &[caravan, other])
            .unwrap()
            .unwrap();
        assert!(plan.is_exact());
        assert_eq!(
            plan.steps,
            vec![
                EconomyStep::SetGroupForm(-1),
                EconomyStep::AddTradeOrder {
                    o: 4,
                    ox: 1,
                    whom: 2,
                    oxx: 3,
                    whose: 4,
                    queue: QUEUE_LAST,
                },
            ]
        );
    }

    #[test]
    fn trade_sea_branch_needs_the_type_and_the_touching_region() {
        let g = group_with(&[4], 1);
        let mut t = TradeTargetFacts::opaque();
        t.live_building = Fact::known(true);
        t.active = Fact::known(true);
        // The entry gate reads `+0xB0`'s `+0x24`; the member loop re-reads the target's
        // own `+0x24`. A sea-trade dock is exactly the case where they disagree.
        t.build_is_trade = Fact::known(true);
        t.is_trade = Fact::known(false);
        t.is_sea_trade = Fact::known(true);
        t.group_has_trader = Fact::known(true);
        let mut m = live(4);
        m.is_sea_trade_member = Fact::known(true);
        m.regions_touch = Fact::known(false);
        let plan = plan_trade(&g, 1, 2, 3, 4, QUEUE_NEW, t, &[m])
            .unwrap()
            .unwrap();
        assert_eq!(plan.steps, vec![EconomyStep::SetGroupForm(-1)]);
    }

    #[test]
    fn board_ship_clears_the_ship_once_and_awaits_every_passenger() {
        let g = group_with(&[4, 5], 1);
        let mut a = live(4);
        let mut b = live(5);
        for m in [&mut a, &mut b] {
            m.domain = Fact::known(0);
            m.busy = Fact::known(false);
            m.regions_touch = Fact::known(true);
            m.ship_can_carry = Fact::known(true);
        }
        let plan = plan_board_ship(&g, 12, QUEUE_NEW, &[a, b]).unwrap();
        assert!(plan.is_exact());
        assert_eq!(
            plan.steps,
            vec![
                EconomyStep::SetGroupForm(-1),
                EconomyStep::AddBoardOrder {
                    o: 4,
                    ship_o: 12,
                    queue: QUEUE_NEW,
                },
                EconomyStep::ClearShipOrders { ship_o: 12 },
                EconomyStep::AddAwaitBoardOrder {
                    ship_o: 12,
                    passenger_o: 4,
                },
                EconomyStep::AddBoardOrder {
                    o: 5,
                    ship_o: 12,
                    queue: QUEUE_NEW,
                },
                EconomyStep::AddAwaitBoardOrder {
                    ship_o: 12,
                    passenger_o: 5,
                },
            ]
        );
    }

    #[test]
    fn board_ship_counts_a_full_ship_instead_of_boarding() {
        let g = group_with(&[4], 1);
        let mut m = live(4);
        m.domain = Fact::known(0);
        m.busy = Fact::known(false);
        m.regions_touch = Fact::known(true);
        m.ship_can_carry = Fact::known(false);
        let plan = plan_board_ship(&g, 12, QUEUE_NEW, &[m]).unwrap();
        assert!(plan.is_exact());
        assert_eq!(plan.blocked_members, 1);
        assert_eq!(plan.steps, vec![EconomyStep::SetGroupForm(-1)]);
    }

    #[test]
    fn board_ship_rejects_air_domain_and_busy_members() {
        let g = group_with(&[4, 5], 1);
        let mut air = live(4);
        air.domain = Fact::known(BOARD_SHIP_REJECTED_DOMAIN);
        air.busy = Fact::known(false);
        air.regions_touch = Fact::known(true);
        air.ship_can_carry = Fact::known(true);
        let mut busy = live(5);
        busy.domain = Fact::known(0);
        busy.busy = Fact::known(true);
        busy.regions_touch = Fact::known(true);
        busy.ship_can_carry = Fact::known(true);
        let plan = plan_board_ship(&g, 12, QUEUE_NEW, &[air, busy]).unwrap();
        assert_eq!(plan.steps, vec![EconomyStep::SetGroupForm(-1)]);
    }

    #[test]
    fn a_member_list_that_disagrees_with_the_group_is_refused() {
        let g = group_with(&[4, 5], 1);
        assert_eq!(
            plan_board_ship(&g, 12, QUEUE_NEW, &[live(4)]),
            Err(EconomyPlanError::MemberCount)
        );
        assert_eq!(
            plan_board_ship(&g, 12, QUEUE_NEW, &[live(4), live(6)]),
            Err(EconomyPlanError::MemberIdentity)
        );
    }

    #[test]
    fn member_queue_normalises_exactly_as_the_three_bodies_do() {
        assert_eq!(member_queue(QUEUE_LAST), QUEUE_LAST);
        assert_eq!(member_queue(QUEUE_NEW), QUEUE_NEW);
        assert_eq!(member_queue(QUEUE_FIRST), QUEUE_NEW);
        assert_eq!(member_queue(99), QUEUE_NEW);
    }
}
