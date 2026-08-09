//! The command→order bridge: how a decoded command packet becomes unit orders.
//!
//! # The chain, end to end
//!
//! ```text
//!   .rcx / wire bytes
//!        │  don-net::decode_commands  (opcode + payload)
//!        ▼
//!   CommandPackage::process_all   0x0094C500
//!        │  one CommandPackage::process_<x>(<X>Command*) per opcode, 82 of them
//!        ▼
//!   groups.list[ package.group ]           ← the receiver, NOT groups[play]
//!        │  Group::action_<x>(...)         42 of them, 209 direct call sites
//!        ▼
//!   Unit::add_<k>_order(..., QueuePos)     22 of them
//!        │  OrdersMemManager::get_obj(OrderIndex)   0x00730AC0
//!        ▼
//!   UnitData::orderlist                    consumed by Unit::work → Unit::do_job
//! ```
//!
//! [`crate::order`] owns the last box of that diagram. This module owns the three above
//! it — the part that was missing, and the reason `don-env` could emit actions and
//! `don-replay` could decode a stream while neither could make a unit do anything.
//!
//! # Provenance
//!
//! Everything marked `[measured]` was disassembled with capstone on this Mac against
//! `ron-bin/riseofnations.exe` (sha256 `30478a44…625079`) and named from
//! `ron-bin/sbl/rise.pdb`. Structure taken from `re/decomp-all/*.c` is marked
//! `[structure]`: Ghidra output is a hypothesis about shape, never a source of values.
//! **Nothing here has been executed against retail.** Tier C.
//!
//! ## The five load-bearing measurements
//!
//! 1. **`CommandPackage::process_<x>` returns `sizeof(<X>Command)`.** Traced over all 82
//!    handlers: 79 end in `mov eax, <imm>` (or `xor eax,eax`) whose value equals the PDB
//!    `sizeof` exactly, with **zero** mismatches. The other three compute it, and they are
//!    exactly the three variable-length commands — `GroupCommand` (`3 + 2·num`),
//!    `SplineCommand`, `ChatCommand`. That is how `process_all` walks a packet, and it is
//!    an independent confirmation of `schema/command-wire.json` from *code* rather than
//!    from the type stream. [measured]
//!
//! 2. **The receiver is `groups.list[package.group]`, not `groups[play]`.** Every
//!    order-issuing handler ends in
//!    ```text
//!    mov  eax, [ebx + 0x0c]        ; CommandPackage::group
//!    test eax, eax
//!    js   skip                     ; group < 0 -> the command is dropped
//!    imul ecx, eax, 0x9d4          ; sizeof(Group) == 2516
//!    add  ecx, [0x00e85f20]        ; groups.list
//!    call Group::action_<x>
//!    ```
//!    [measured, `CommandPackage::process_halt` `0x00949140` at `0x00949201`.]
//!
//! 3. **`CommandPackage::group` is written by opcode 0.** `process_group` `0x0094A0C0`
//!    builds a stack-local `Group` from the selection list, interns it with
//!    `Groups::push_group` `0x0070F9E0`, and stores the returned slot in `this->group`;
//!    on an empty result it stores `-1`. So a packet is *one selection followed by the
//!    actions that apply to it*, and a packet with no `GroupCommand` addresses whatever
//!    slot the previous one left behind. [structure + measured]
//!
//! 4. **`Unit::add_<k>_order` allocates the `OrderIndex` its name says.** Scanning every
//!    `push <imm>; call OrdersMemManager::get_obj` site in `.text` gives a total function:
//!    `add_guard_order`→`GUARD`, `add_gather_order`→`GATHER`, and so on. Three are
//!    parameterised: `add_move_facing_order` picks `ATTACK_TO`/`EXPLORE_TO`/`FLEE_TO`,
//!    else `MOVE_TO`; `add_group_move_order` picks `GROUP_ATTACK_TO` when its selector is
//!    `2`, else `GROUP_MOVE`; `add_patrol_order` allocates **`GROUP_PATROL`**, never
//!    `PATROL`. [measured]
//!
//! 5. **Four `OrderIndex` values are constructed nowhere in `.text`**: `NONE` (0, the
//!    empty-list sentinel), `PATROL` (5), `CHANGE_FORM` (18) and `GROUP_ATTACK` (20). The
//!    only non-constant allocation is `copy_order` `0x0072F900`, which duplicates an
//!    existing order and so cannot originate a kind. `PATROL` was already known dead from
//!    the `do_job` jump table; `CHANGE_FORM` and `GROUP_ATTACK` are the same discovery from
//!    the allocation side, and their executors `Unit::do_form_change` `0x005E8670` and
//!    `Unit::do_group_attack` `0x005E75A0` are therefore unreachable. [measured]
//!
//! # `QueuePos`, and the insert dance
//!
//! `QueuePos` is `{QUEUE_FIRST = 0, QUEUE_LAST = 1, QUEUE_NEW = 2}` [measured, PDB
//! `LF_FIELDLIST 0x216F`]. `Unit::add_<k>_order` reads it directly: `QUEUE_NEW` clears the
//! unit's order list first (`close_orders` + `clear_partial_path` + `update_action`),
//! `QUEUE_LAST` appends.
//!
//! `QUEUE_FIRST` is *not* handled per unit. Ten `Group::action_<x>` share this shape
//! [structure, e.g. `Group::action_follow` `0x006FD510`]:
//!
//! ```text
//! if (queued == QUEUE_FIRST) {
//!     OrderList saved;
//!     set_up_insert(&saved);        // 0x0070E520, stash the leader's order list
//!     action_halt(0);               // 0x0070D0C0, clear the whole group
//!     action_<x>(args, QUEUE_NEW);  // re-enter, now unambiguous
//!     finish_insert(&saved);        // 0x0070E620, replay the stash as group actions
//!     return;
//! }
//! ```
//!
//! `Group::finish_insert` is why `action_guard`, `action_follow`, `action_attack`,
//! `action_move_near`, `action_board_ship`, `action_trade`, `action_gather`,
//! `action_garrison`, `action_spell`, `action_attack_ground` and `action_swarm_around` all
//! list it as a caller: it *re-issues* the saved orders through the same action API. The
//! bridge reproduces that shape rather than inventing a per-unit "insert at front".
//!
//! # What is not here
//!
//! The order *executors* live in [`crate::systems::order_dispatch`]. This module owns the
//! wire/group installation side and writes the same executable queue shape, including the
//! dynamic patrol payloads that cannot be represented by a flat order tag.

use crate::order::{FollowOrderPayload, Order, OrderIndex, ORDER_GROUP};
use crate::systems::groups_guys::{
    formation_order_coord, plan_action_buildmask, plan_action_disband, plan_action_halt,
    plan_action_set_transport, plan_action_stance, plan_action_unitmask, resolve_form, vector_dist,
    BuildMaskMemberFacts, BuildMaskStep, DisbandMemberFacts, DisbandPlan, DisbandStep, Formation,
    FormationMember, GroupBuildMaskReceipt, GroupBuildMaskRequest, GroupData,
    GroupSetTransportReceipt, GroupSetTransportRequest, GroupStanceReceipt, GroupStanceRequest,
    GroupStateTransactionStatus, GroupUnitMaskReceipt, GroupUnitMaskRequest, HaltMemberFacts,
    HaltPlan, HaltStep, MemberState, SetTransportMemberFacts, SetTransportStep, StanceMemberFacts,
    StanceStep, UnitMaskMemberFacts, UnitMaskStep, GROUP_MAX_MEMBERS,
};
use crate::systems::order_dispatch::{
    install_air_patrol, install_group_patrol, OrderQueue, OrderRec, PatrolInstall, UnitWork,
};

// These command proof modules deliberately landed outside `systems.rs` so their recovery
// tests could not be mistaken for dispatcher integration.  The command bridge is now their
// sole executable owner; keeping the path declarations here avoids exposing them as tick or
// world systems.
#[path = "systems/diplomacy_command_plans.rs"]
pub mod diplomacy_command_plans;
#[path = "systems/direct_entity_command_integration.rs"]
pub mod direct_entity_command_integration;
#[path = "systems/follow_action.rs"]
pub mod follow_action;
#[path = "systems/group_action_frontier.rs"]
pub mod group_action_frontier;
#[path = "systems/late_command_plans.rs"]
pub mod late_command_plans;
#[path = "systems/object_command_plans.rs"]
pub mod object_command_plans;
#[path = "systems/setup_diplomacy.rs"]
pub mod setup_diplomacy;
#[path = "systems/tail_command_transactions.rs"]
pub mod tail_command_transactions;

use self::diplomacy_command_plans::{
    DiplomacyCommandReceipt, DiplomacyCommandRequest, DiplomacyCommandState,
};
use self::direct_entity_command_integration::{DirectEntityFleetReceipt, DirectEntityFleetRequest};
use self::follow_action::{
    decode_follow, plan_follow, FollowEffect, FollowMemberFacts, FollowReceipt, FollowRequest,
    FollowTargetFacts, FollowTransactionStatus,
};
use self::group_action_frontier::{
    plan_stop_spell, GroupActionTransactionStatus, OpenGroupActionCommand, StopSpellMemberFacts,
    StopSpellReceipt, StopSpellRequest, StopSpellStep,
};
use self::late_command_plans::{
    CannonTimeFacts, CannonTimeReceipt, CannonTimeRequest, PlanStatus as LateCommandPlanStatus,
};
use self::object_command_plans::{
    RenameCityCommand, RenameCityTransactionReceipt, RenameCityTransactionStatus,
};
use self::tail_command_transactions::{TailCommandFacts, TailCommandReceipt, TailCommandRequest};

/// Owner slots, as `Objects::process_all` iterates them.
pub const NUM_OWNER_SLOTS: usize = 10;

/// `sizeof(Group)` — the stride every `groups.list[i]` computation uses [measured,
/// `imul ecx, eax, 0x9d4`].
pub const SIZEOF_GROUP: usize = 2516;

/// Stride between owners in the `groups` pool. `Groups::get_open_slot` `0x006FA460`
/// computes its byte base as `who * 0x27500`, and `0x27500 / 0x9D4 == 0x40` [measured].
pub const GROUP_SLOTS_STRIDE: usize = 64;

/// How many of each owner's 64 slots the allocator actually scans: the loop bound is
/// `base + 0x2E` [measured, `Groups::get_open_slot`].
pub const GROUP_SLOTS_SCANNED: usize = 46;

// ---------------------------------------------------------------------------
// QueuePos
// ---------------------------------------------------------------------------

/// `QueuePos`, the `queued` field carried by most order commands.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub enum QueuePos {
    /// Put this order at the head, keeping what was already queued behind it. At the
    /// group layer this is the stash / halt / re-issue-as-`New` / replay dance.
    #[default]
    First = 0,
    /// Append behind everything already queued (shift-click).
    Last = 1,
    /// Clear the unit's order list, then install. The only value `Unit::add_*_order`
    /// treats specially on its own.
    New = 2,
}

impl QueuePos {
    pub fn from_i64(v: i64) -> QueuePos {
        match v {
            1 => QueuePos::Last,
            2 => QueuePos::New,
            _ => QueuePos::First,
        }
    }
}

// ---------------------------------------------------------------------------
// The opcode table
// ---------------------------------------------------------------------------

/// Which retail object a `CommandPackage::process_*` hands the command to.
///
/// This is the engine's own split, taken from the call each handler makes, not a guess
/// about what the command "means".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Receiver {
    /// `groups.list[package.group].action_<x>(...)`.
    Group,
    /// `leaders[who].action_<x>(...)`.
    Leader,
    /// `Unit::action_*` / `Build::action_unqueue` on one addressed object.
    Unit,
    /// `Game::action_cheat_*`.
    Game,
    /// The handler calls no `*::action_*` at all — it writes state inline
    /// (`Player::resign`, `Leader::do_sell`, `HotKeyGroups::copy_group`, the lockstep
    /// channels, the presentation channels).
    None,
}

/// The wire length of one command.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WireLen {
    /// `process_<x>` returns this constant, and it equals the PDB `sizeof` [measured].
    Fixed(u16),
    /// `process_<x>` computes its return value: `GroupCommand`, `SplineCommand`,
    /// `ChatCommand`.
    Variable,
}

/// One row of the 82-opcode dispatch table.
#[derive(Clone, Copy, Debug)]
pub struct OpDef {
    pub op: u8,
    /// PDB struct name.
    pub name: &'static str,
    /// `CommandPackage::<method>`.
    pub method: &'static str,
    pub method_va: u32,
    pub receiver: Receiver,
    /// The `<x>` of `Group::action_<x>` / `Leader::action_<x>`, when there is one.
    pub action: Option<&'static str>,
    pub wire: WireLen,
}

impl OpDef {
    /// Does this opcode reach a `Group::action_*`? 35 of the 82 do.
    #[inline]
    pub fn is_group_action(&self) -> bool {
        matches!(self.receiver, Receiver::Group)
    }
}

// ---------------------------------------------------------------------------
// The Group::action_* table
// ---------------------------------------------------------------------------

/// How much of one `Group::action_*` this module reproduces.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Port {
    /// The complete simulation-side effect is reproduced. Presentation/logging calls are
    /// deliberately outside the headless bridge.
    Complete,
    /// The order installed on each eligible member, its `OrderIndex`, its target/coords
    /// and its `QueuePos` handling are reproduced. Eligibility predicates that need
    /// unit-type data we do not hold here are delegated to [`Fleet`].
    Orders,
    /// Reproduced, and retail installs no order either — it edits group or unit state
    /// (`stance`, `unitmask`, `set_transport`) or clears orders (`halt`).
    State,
    /// The packet is decoded and the recovered state mutation is routed through
    /// [`Fleet`], but the host must still supply an exact capability predicate or state
    /// column before the row can be called complete.
    StateWired,
    /// Dispatched and counted, body not ported. Hitting this increments a counter rather
    /// than pretending to act.
    Todo,
    /// Reached only from the local UI or the scenario script API, never from a
    /// `CommandPackage` handler. Out of the bridge's scope.
    NotOnTheWire,
}

/// One `Group::action_*`.
#[derive(Clone, Copy, Debug)]
pub struct ActionDef {
    /// The `<x>` of `Group::action_<x>`.
    pub name: &'static str,
    pub va: u32,
    pub size: u32,
    /// Direct `call`/`jmp` sites in `.text` [measured].
    pub call_sites: u32,
    /// `OrderIndex` values this action installs *directly*, via a `Unit::add_*_order` it
    /// calls itself [measured, through `OrdersMemManager::get_obj`].
    pub installs: &'static [OrderIndex],
    /// Other `Group::action_*` it calls; those contribute their own orders.
    pub delegates: &'static [&'static str],
    pub port: Port,
}

impl ActionDef {
    pub fn find(name: &str) -> Option<&'static ActionDef> {
        GROUP_ACTIONS.iter().find(|a| a.name == name)
    }
}

/// How much of a non-group `CommandPackage::process_*` row the bridge carries.
/// Most rows write state inline; recovered Leader/object/product actions use a validated
/// atomic [`Fleet`] transaction instead of entering the `Group::action_*` table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InlinePort {
    /// Every deterministic simulation-side state mutation and gate is reproduced.
    /// Logging, UI, audio, and wall-clock pacing remain outside the headless core.
    Complete,
    /// The wire path and core state transition execute, but an adjacent product/runtime
    /// side effect remains explicit and prevents a closure-green claim.
    StateWired,
}

#[derive(Clone, Copy, Debug)]
pub struct InlineDef {
    pub op: u8,
    pub name: &'static str,
    pub port: InlinePort,
}

impl InlineDef {
    pub fn find(op: u8) -> Option<&'static InlineDef> {
        INLINE_COMMANDS.iter().find(|d| d.op == op)
    }
}

include!("command_tables.rs");

/// `Unit::add_<k>_order` → the `OrderIndex` it allocates from `OrdersMemManager::get_obj`
/// `0x00730AC0` [measured, every `push imm; call` site in `.text`].
///
/// The three multi-valued rows are the parameterised ones; see the module header.
pub static ADD_ORDER_KINDS: [(&str, u32, &[OrderIndex]); 22] = [
    ("add_think_order", 0x005E3DF0, &[OrderIndex::Think]),
    ("add_guard_order", 0x005E3E40, &[OrderIndex::Guard]),
    ("add_follow_order", 0x005E3F60, &[OrderIndex::Follow]),
    ("add_garrison_order", 0x005E4080, &[OrderIndex::Garrison]),
    (
        "add_spec_anim_order",
        0x005E4160,
        &[OrderIndex::SpecialAnim],
    ),
    (
        "add_air_attack_ground_order",
        0x005E41C0,
        &[OrderIndex::AirAttackGround],
    ),
    (
        "add_attack_ground_order",
        0x005E4290,
        &[OrderIndex::AttackGround],
    ),
    ("add_air_patrol_order", 0x005E4350, &[OrderIndex::AirPatrol]),
    ("add_patrol_order", 0x005E4560, &[OrderIndex::GroupPatrol]),
    (
        "add_group_move_order",
        0x005E4710,
        &[OrderIndex::GroupMove, OrderIndex::GroupAttackTo],
    ),
    ("add_strafe_order", 0x005E48C0, &[OrderIndex::Strafe]),
    ("add_cast_order", 0x005E4A60, &[OrderIndex::CastSpell]),
    (
        "add_await_board_order",
        0x005E4C80,
        &[OrderIndex::AwaitBoard],
    ),
    ("add_board_order", 0x005E4D10, &[OrderIndex::BoardShip]),
    ("add_trade_order", 0x005E4DC0, &[OrderIndex::TradeRoute]),
    ("add_repair_order", 0x005E4FF0, &[OrderIndex::Repair]),
    ("add_build_order", 0x005E5210, &[OrderIndex::BuildAt]),
    ("add_attack_order", 0x005E5410, &[OrderIndex::Attack]),
    (
        "add_move_facing_order",
        0x005E55C0,
        &[
            OrderIndex::MoveTo,
            OrderIndex::AttackTo,
            OrderIndex::ExploreTo,
            OrderIndex::FleeTo,
        ],
    ),
    (
        "add_move_order",
        0x00616ED0,
        &[
            OrderIndex::MoveTo,
            OrderIndex::AttackTo,
            OrderIndex::ExploreTo,
            OrderIndex::FleeTo,
        ],
    ),
    ("add_gather_order", 0x0061A5C0, &[OrderIndex::Gather]),
    ("update_guard_order", 0x005E3220, &[]),
];

/// `OrderIndex` values with no construction site anywhere in `.text` [measured].
pub const UNCONSTRUCTED_ORDERS: [OrderIndex; 4] = [
    OrderIndex::None,
    OrderIndex::Patrol,
    OrderIndex::ChangeForm,
    OrderIndex::GroupAttack,
];

// ---------------------------------------------------------------------------
// Wire length
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WireError {
    UnknownOpcode(u8),
    Truncated { offset: usize, need: usize },
    BadLength { offset: usize, len: i64 },
}

/// The length `CommandPackage::process_all` would advance by, i.e. what the matching
/// `process_<x>` returns.
///
/// The three variable forms are transcribed from their handlers:
///
/// * op 0 `GroupCommand`  — `return num * 2 + 3` [measured, `0x0094A6E0`].
/// * op 51 `SplineCommand` — `6 + 8 * len`, `len` the `u16` at `+4`.
/// * op 68 `ChatCommand`  — `19 + 2 * len`, `len` the `i32` at `+13`, bounded by the
///   512-byte send buffer.
pub fn wire_len(buf: &[u8]) -> Result<usize, WireError> {
    let need = |n: usize| -> Result<(), WireError> {
        if buf.len() < n {
            Err(WireError::Truncated { offset: 0, need: n })
        } else {
            Ok(())
        }
    };
    need(1)?;
    let op = buf[0];
    let def = OPCODES
        .get(op as usize)
        .ok_or(WireError::UnknownOpcode(op))?;
    match def.wire {
        WireLen::Fixed(n) => Ok(n as usize),
        WireLen::Variable => match op {
            0 => {
                need(2)?;
                Ok(3 + 2 * buf[1] as usize)
            }
            51 => {
                need(6)?;
                Ok(6 + 8 * u16::from_le_bytes([buf[4], buf[5]]) as usize)
            }
            68 => {
                need(17)?;
                let n = i32::from_le_bytes([buf[13], buf[14], buf[15], buf[16]]);
                if !(0..=512).contains(&n) {
                    return Err(WireError::BadLength {
                        offset: 0,
                        len: n as i64,
                    });
                }
                Ok(19 + 2 * n as usize)
            }
            _ => Err(WireError::UnknownOpcode(op)),
        },
    }
}

/// Read a little-endian `i32` at `off`, or `None` past the end.
#[inline]
fn i32_at(b: &[u8], off: usize) -> Option<i32> {
    b.get(off..off + 4)
        .map(|s| i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

#[inline]
fn u16_at(b: &[u8], off: usize) -> Option<u16> {
    b.get(off..off + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
}

#[inline]
fn i8_at(b: &[u8], off: usize) -> Option<i8> {
    b.get(off).map(|v| *v as i8)
}

#[inline]
fn i16_at(b: &[u8], off: usize) -> Option<i16> {
    b.get(off..off + 2)
        .map(|s| i16::from_le_bytes([s[0], s[1]]))
}

// ---------------------------------------------------------------------------
// The world this bridge writes into
// ---------------------------------------------------------------------------

/// Player slots scanned by the negative-owner arm of `Game::action_cheat_init_unit`.
pub const CHEAT_INIT_PLAYER_SLOTS: usize = 8;

/// Exact trailing arguments passed to `UnitType::find_nearby_spot` by opcode 67 after
/// `(origin_x, origin_y, &out_x, &out_y)`.
pub const CHEAT_INIT_NEARBY_TAIL: [i32; 12] =
    [0, 0xC00, 0, 0x5555_5555, 3, -1, -1, 0, 0, -1, 0, -1];

/// Whole command request crossing the world-owned `Objects::init_unit` boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheatInitUnitRequest {
    /// Non-negative selects one owner directly; every negative value selects all valid
    /// player slots 0..7.
    pub who: i32,
    pub type_index: i32,
    pub x: i32,
    pub y: i32,
    /// Snapshot of `PlayerData::valid & 1`, used only by the negative-owner arm.
    pub valid_players: [bool; CHEAT_INIT_PLAYER_SLOTS],
}

/// Exact `UnitType::find_nearby_spot` call for one valid player in the all-player arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheatInitUnitNearbyRequest {
    pub owner: u8,
    pub type_index: i32,
    pub origin_x: i32,
    pub origin_y: i32,
    pub tail: [i32; 12],
}

/// Exact `Objects::init_unit` call made by either arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheatInitUnitAllocationRequest {
    pub owner: i32,
    pub type_index: i32,
    pub x: i32,
    pub y: i32,
    pub tail: [i32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheatInitUnitNearbyOutcome {
    /// Retail's zero return, carrying the two output coordinates.
    Found { x: i32, y: i32 },
    /// Any non-zero return; no allocation follows for this owner.
    NotFound,
}

/// Ordered world calls made by one applied opcode-67 transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheatInitUnitStepReceipt {
    Nearby {
        request: CheatInitUnitNearbyRequest,
        outcome: CheatInitUnitNearbyOutcome,
    },
    Allocation {
        request: CheatInitUnitAllocationRequest,
        /// Raw `Objects::init_unit` return. The retail action ignores allocation failure.
        object_id: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheatInitUnitTransactionStatus {
    /// The host preflighted and atomically applied the complete ordered call sequence.
    Applied,
    /// No world mutation occurred because this host does not expose the transaction.
    Unavailable,
}

/// Host receipt for the world-owned portion of `Game::action_cheat_init_unit`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheatInitUnitTransactionReceipt {
    pub request: CheatInitUnitRequest,
    pub status: CheatInitUnitTransactionStatus,
    pub steps: Vec<CheatInitUnitStepReceipt>,
}

impl CheatInitUnitTransactionReceipt {
    pub fn unavailable(request: CheatInitUnitRequest) -> Self {
        Self {
            request,
            status: CheatInitUnitTransactionStatus::Unavailable,
            steps: Vec::new(),
        }
    }

    /// Validate the complete ordered call trace against the recovered retail branches.
    pub fn validates(&self, expected: CheatInitUnitRequest) -> bool {
        if self.request != expected {
            return false;
        }
        if self.status == CheatInitUnitTransactionStatus::Unavailable {
            return self.steps.is_empty();
        }

        if expected.who >= 0 {
            let [CheatInitUnitStepReceipt::Allocation { request, .. }] = self.steps.as_slice()
            else {
                return false;
            };
            return *request
                == (CheatInitUnitAllocationRequest {
                    owner: expected.who,
                    type_index: expected.type_index,
                    x: expected.x,
                    y: expected.y,
                    tail: [-1; 3],
                });
        }

        let mut cursor = 0usize;
        for owner in 0..CHEAT_INIT_PLAYER_SLOTS {
            if !expected.valid_players[owner] {
                continue;
            }
            let expected_nearby = CheatInitUnitNearbyRequest {
                owner: owner as u8,
                type_index: expected.type_index,
                origin_x: expected.x,
                origin_y: expected.y,
                tail: CHEAT_INIT_NEARBY_TAIL,
            };
            let Some(CheatInitUnitStepReceipt::Nearby { request, outcome }) =
                self.steps.get(cursor)
            else {
                return false;
            };
            if *request != expected_nearby {
                return false;
            }
            cursor += 1;
            if let CheatInitUnitNearbyOutcome::Found { x, y } = *outcome {
                let Some(CheatInitUnitStepReceipt::Allocation { request, .. }) =
                    self.steps.get(cursor)
                else {
                    return false;
                };
                if *request
                    != (CheatInitUnitAllocationRequest {
                        owner: owner as i32,
                        type_index: expected.type_index,
                        x,
                        y,
                        tail: [-1; 3],
                    })
                {
                    return false;
                }
                cursor += 1;
            }
        }
        cursor == self.steps.len()
    }
}

/// Bridge-owned validation record retaining both expected and host-observed identities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheatInitUnitReceiptRecord {
    pub expected: CheatInitUnitRequest,
    pub observed: CheatInitUnitTransactionReceipt,
    pub valid: bool,
}

/// Bridge-owned evidence for one addressed market/entity command transaction.
///
/// The callback owns the atomic host mutation.  Retaining both the expected request and
/// returned receipt makes a forged identity or frame visible without treating an invalid
/// receipt as command completion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectEntityReceiptRecord {
    pub expected: DirectEntityFleetRequest,
    pub observed: DirectEntityFleetReceipt,
    pub valid: bool,
}

/// Bridge-owned evidence for one late-control command transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TailCommandReceiptRecord {
    pub expected: TailCommandRequest,
    pub facts: TailCommandFacts,
    pub observed: TailCommandReceipt,
    pub valid: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GroupHaltTransactionRequest {
    pub group: GroupData,
    pub flags: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupHaltTransactionStatus {
    Applied,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GroupHaltTransactionReceipt {
    pub request: GroupHaltTransactionRequest,
    pub status: GroupHaltTransactionStatus,
    pub group_after_ignore_orders: Option<GroupData>,
    pub members: Vec<HaltMemberFacts>,
    pub plan: Option<HaltPlan>,
}

impl GroupHaltTransactionReceipt {
    pub fn unavailable(request: GroupHaltTransactionRequest) -> Self {
        Self {
            request,
            status: GroupHaltTransactionStatus::Unavailable,
            group_after_ignore_orders: None,
            members: Vec::new(),
            plan: None,
        }
    }

    pub fn validates(&self, expected: &GroupHaltTransactionRequest) -> bool {
        if &self.request != expected {
            return false;
        }
        match self.status {
            GroupHaltTransactionStatus::Unavailable => {
                self.group_after_ignore_orders.is_none()
                    && self.members.is_empty()
                    && self.plan.is_none()
            }
            GroupHaltTransactionStatus::Applied => {
                let (Some(group), Some(observed)) =
                    (self.group_after_ignore_orders.as_ref(), self.plan.as_ref())
                else {
                    return false;
                };
                plan_action_halt(group, expected.flags, &self.members)
                    .is_ok_and(|recomputed| recomputed == *observed)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GroupDisbandTransactionRequest {
    pub group: GroupData,
    pub all: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupDisbandTransactionStatus {
    Applied,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GroupDisbandTransactionReceipt {
    pub request: GroupDisbandTransactionRequest,
    pub status: GroupDisbandTransactionStatus,
    pub group_after_ignore_orders: Option<GroupData>,
    pub validate_disband: Option<bool>,
    pub owner_is_local: Option<bool>,
    pub members: Vec<DisbandMemberFacts>,
    pub plan: Option<DisbandPlan>,
}

impl GroupDisbandTransactionReceipt {
    pub fn unavailable(request: GroupDisbandTransactionRequest) -> Self {
        Self {
            request,
            status: GroupDisbandTransactionStatus::Unavailable,
            group_after_ignore_orders: None,
            validate_disband: None,
            owner_is_local: None,
            members: Vec::new(),
            plan: None,
        }
    }

    pub fn validates(&self, expected: &GroupDisbandTransactionRequest) -> bool {
        if &self.request != expected {
            return false;
        }
        match self.status {
            GroupDisbandTransactionStatus::Unavailable => {
                self.group_after_ignore_orders.is_none()
                    && self.validate_disband.is_none()
                    && self.owner_is_local.is_none()
                    && self.members.is_empty()
                    && self.plan.is_none()
            }
            GroupDisbandTransactionStatus::Applied => {
                let (Some(group), Some(validate), Some(local), Some(observed)) = (
                    self.group_after_ignore_orders.as_ref(),
                    self.validate_disband,
                    self.owner_is_local,
                    self.plan.as_ref(),
                ) else {
                    return false;
                };
                plan_action_disband(group, expected.all, validate, local, &self.members)
                    .is_ok_and(|recomputed| recomputed == *observed)
            }
        }
    }
}

/// The object-side interface `Group::action_*` needs.
///
/// The retail actions reach the world through `objects.lists[who][o]` and a pile of
/// virtual predicates. Rather than pull `World` in, the bridge asks for exactly the
/// questions the ported actions ask, so it can run against a test fleet, against
/// `don-env`'s `EnvWorld`, or eventually against the real SoA columns.
///
/// `o` is the engine's own address for an object: an index into that owner's list.
pub trait Fleet {
    fn alive(&self, who: u8, o: i16) -> bool;
    /// `ObjectData::is_unit`. Buildings take some actions (queue_up, gather point) and
    /// refuse the movement ones.
    fn is_unit(&self, who: u8, o: i16) -> bool;
    fn is_building(&self, who: u8, o: i16) -> bool;
    /// `UnitData::is_on_map` (virtual `+0xBC`). Formation actions use it both to reject
    /// wholly off-map groups and in the first pass of `GroupData::find_leader`.
    fn is_on_map(&self, who: u8, o: i16) -> bool {
        self.alive(who, o)
    }
    /// `UnitData::is_captain` (virtual `+0xE8`), the formation-leader eligibility bit.
    fn is_captain(&self, who: u8, o: i16) -> bool {
        self.is_unit(who, o)
    }
    /// `ObjectData`'s virtual `+0x20`, used by `Group::normalize` to evict objects which
    /// no longer belong in selections.
    fn leaves_groups(&self, _who: u8, _o: i16) -> bool {
        false
    }
    /// `FormData::type_cat(type.form_type, who, who)`. `find_leader` selects the lowest
    /// category, retaining list order on ties; retail initializes the best category to 18.
    fn form_category(&self, _who: u8, _o: i16) -> i32 {
        0
    }
    /// Effective type facts for `Form::categorize` at this destination. Returning `None`
    /// keeps the command on the already-recovered flat order-installation path; formation
    /// offsets are never guessed from the category alone.
    fn formation_member(
        &self,
        _who: u8,
        _o: i16,
        _water_destination: bool,
    ) -> Option<FormationMember> {
        None
    }
    /// The terrain predicate passed to `Form::categorize` by `Group::compute_form`.
    fn formation_water_destination(&self, _x: i32, _y: i32) -> bool {
        false
    }
    /// Game option bit `GameData +0x821 & 8`, which forces formation facing to zero.
    fn force_formation_facing_zero(&self) -> bool {
        false
    }
    /// `UnitData::form`, the signed byte at `+0xAA`.
    fn form(&self, _who: u8, _o: i16) -> i8 {
        0
    }
    fn set_form(&mut self, _who: u8, _o: i16, _form: i8) {}
    /// `UnitData::angle`, the dword at `+0x50`.
    fn angle(&self, _who: u8, _o: i16) -> i32 {
        0
    }
    /// `UnitTypeData::role`, OR-ed into `GroupData::role` by `Group::add`.
    fn role(&self, who: u8, o: i16) -> i32 {
        let _ = (who, o);
        0
    }
    /// `ObjectTypeData::domain` at `+0x218`.
    fn domain(&self, _who: u8, _o: i16) -> i32 {
        0
    }
    /// Live `UnitData::unit_masks` at `+0x68`.
    fn unit_masks(&self, _who: u8, _o: i16) -> u32 {
        0
    }
    fn set_unit_masks(&mut self, _who: u8, _o: i16, _masks: u32) {}
    /// `UnitData::can_ever_transport` `0x0046F290`. Land units return true without
    /// consulting any other state; non-land hosts override this for their type/ability
    /// branch.
    fn can_ever_transport(&self, who: u8, o: i16) -> bool {
        self.is_unit(who, o) && self.domain(who, o) == 0
    }
    /// `BuildData::build_masks`, the `u16` at `+0x60`. `None` makes BUILD_MASK
    /// fail closed for a host which has not connected that state column.
    fn build_masks(&self, _who: u8, _o: i16) -> Option<u16> {
        None
    }
    fn set_build_masks(&mut self, _who: u8, _o: i16, _masks: u16) -> bool {
        false
    }
    /// `WallData::valid_buildmask` `0x0063E2A0`: whether this building admits this UI build-mask
    /// selector. The predicate is type/object dependent, so absence is false rather than
    /// a guessed capability.
    fn can_toggle_build_mask(&self, _who: u8, _o: i16, _mask: u16) -> bool {
        false
    }
    /// Can this object be given movement orders at all? Retail asks
    /// `UnitData::get_speed() > 0` plus a stack of "entering/exiting", "is_blown" and
    /// garrison predicates; a port supplies whichever of those it holds.
    fn can_move(&self, who: u8, o: i16) -> bool {
        self.is_unit(who, o)
    }
    /// `UnitData::is_plane` `0x0046CE40`, not merely air domain. Helicopters return false.
    fn is_plane(&self, _who: u8, _o: i16) -> bool {
        false
    }
    /// Live home object carried by the aircraft's `AirOrder` base, when one exists.
    /// `(home_o, home_who, x, y)` uses engine object addressing and world Coord units.
    fn air_patrol_home(&self, _who: u8, _o: i16) -> Option<(i32, i32, i32, i32)> {
        None
    }
    /// `ObjectData::group`, the `short` at `+0x80`: the slot this object currently
    /// belongs to, or `-1`.
    fn group_of(&self, who: u8, o: i16) -> i16;
    fn set_group_of(&mut self, who: u8, o: i16, slot: i16);
    /// `ObjectData::uid`, the `unsigned short` at `+0x30`. `process_group`'s empty-list
    /// path re-selects by `(o, uid)` pairs, so a recycled slot is not silently re-selected.
    fn uid(&self, who: u8, o: i16) -> u16;
    /// World coordinates, already un-XORed (`ObjectData` stores them `^ 0x00063637`).
    fn pos(&self, who: u8, o: i16) -> (i32, i32);
    /// `WorldData::valid` as observed by `UnitData::get_final_loc`. A rejected order
    /// destination falls back to the unit's current position.
    fn valid_pos(&self, _x: i32, _y: i32) -> bool {
        true
    }
    /// `UnitData::get_final_loc` `0x00608040`: the first move destination or live target
    /// location in queue order, falling back to the unit's current location.
    fn final_pos(&self, who: u8, o: i16) -> (i32, i32) {
        let current = self.pos(who, o);
        let Some(queue) = self.orders(who, o) else {
            return current;
        };
        for order in queue.iter() {
            let candidate = if order.is_move() {
                Some((order.x, order.y))
            } else if order.is_targeted()
                && order.target_who >= 0
                && order.target_o >= 0
                && self.alive(order.target_who as u8, order.target_o as i16)
            {
                Some(self.pos(order.target_who as u8, order.target_o as i16))
            } else {
                None
            };
            if let Some((x, y)) = candidate {
                return if self.valid_pos(x, y) {
                    (x, y)
                } else {
                    current
                };
            }
        }
        current
    }
    fn orders(&self, who: u8, o: i16) -> Option<&OrderQueue>;
    fn orders_mut(&mut self, who: u8, o: i16) -> Option<&mut OrderQueue>;
    /// Whether an order can be installed without crossing a host-owned lifecycle which
    /// could fail midway through a group action. Product hosts use this as the
    /// transaction preflight; the in-memory bridge table has no external lifecycle.
    fn can_install_order(&self, who: u8, o: i16, _queue: QueuePos) -> bool {
        self.orders(who, o).is_some()
    }
    /// Install one already-constructed order after [`Fleet::can_install_order`] succeeds.
    /// Hosts with retirement side effects override this instead of exposing their queue
    /// for mutation behind the lifecycle's back.
    fn install_order_rec(&mut self, who: u8, o: i16, order: OrderRec, queue: QueuePos) -> bool {
        let Some(list) = self.orders_mut(who, o) else {
            return false;
        };
        if queue == QueuePos::New {
            list.clear();
        }
        list.push_back(order);
        true
    }
    /// `Unit::set_stance` — `action_stance` writes it and installs no order.
    fn set_stance(&mut self, who: u8, o: i16, stance: i8);
    /// `Object::disband` `0x006455C0`.
    fn disband(&mut self, who: u8, o: i16);

    /// Atomic world boundary for opcode 67. `Applied` hosts must preflight and commit the
    /// complete retail sequence described by the returned ordered steps; `Unavailable`
    /// must perform no mutation. The bridge validates every echoed identity and branch.
    fn apply_cheat_init_unit_transaction(
        &mut self,
        request: CheatInitUnitRequest,
    ) -> CheatInitUnitTransactionReceipt {
        CheatInitUnitTransactionReceipt::unavailable(request)
    }

    /// Atomic host boundary for command rows 46 through 49.
    ///
    /// Buy and sell hosts may return a validated complete economy transaction.  Unqueue
    /// and come-out can prove their inactive/stale no-op arm, but their reached
    /// production/containment action tails deliberately remain open.
    fn apply_direct_entity_command_transaction(
        &mut self,
        request: DirectEntityFleetRequest,
    ) -> DirectEntityFleetReceipt {
        DirectEntityFleetReceipt::unavailable(request)
    }

    /// Atomic receiver boundary for complete `Group::action_stop_spell`.
    fn apply_stop_spell_transaction(&mut self, request: StopSpellRequest) -> StopSpellReceipt {
        StopSpellReceipt::unavailable(request)
    }

    /// Atomic receiver boundary for complete `Group::action_follow`. Applied hosts commit
    /// the validated halt/install/insert lifecycle before returning; unavailable hosts leave
    /// group, queues, paths and actions untouched.
    fn apply_follow_transaction(&mut self, request: FollowRequest) -> FollowReceipt {
        FollowReceipt::unavailable(request)
    }

    fn apply_group_halt_transaction(
        &mut self,
        request: GroupHaltTransactionRequest,
    ) -> GroupHaltTransactionReceipt {
        GroupHaltTransactionReceipt::unavailable(request)
    }

    fn apply_group_disband_transaction(
        &mut self,
        request: GroupDisbandTransactionRequest,
    ) -> GroupDisbandTransactionReceipt {
        GroupDisbandTransactionReceipt::unavailable(request)
    }

    fn apply_group_stance_transaction(
        &mut self,
        request: GroupStanceRequest,
    ) -> GroupStanceReceipt {
        GroupStanceReceipt::unavailable(request)
    }

    fn apply_group_set_transport_transaction(
        &mut self,
        request: GroupSetTransportRequest,
    ) -> GroupSetTransportReceipt {
        GroupSetTransportReceipt::unavailable(request)
    }

    fn apply_group_unitmask_transaction(
        &mut self,
        request: GroupUnitMaskRequest,
    ) -> GroupUnitMaskReceipt {
        GroupUnitMaskReceipt::unavailable(request)
    }

    fn apply_group_buildmask_transaction(
        &mut self,
        request: GroupBuildMaskRequest,
    ) -> GroupBuildMaskReceipt {
        GroupBuildMaskReceipt::unavailable(request)
    }

    /// Atomic host boundary for opcode 76. Applied hosts execute every external/world
    /// step in the validated plan as one transaction; Unavailable performs no mutation.
    fn apply_pause_transaction(
        &mut self,
        request: PauseTransactionRequest,
    ) -> PauseTransactionReceipt {
        PauseTransactionReceipt::unavailable(request)
    }

    /// Atomic host boundary for opcode 75. `Applied` means the object lookup, optional
    /// city-name write, all reached hot-key normalizations/stamps, and both presentation
    /// updates committed as the single validated plan. `Unavailable` performs no mutation.
    fn apply_rename_city_transaction(
        &mut self,
        request: RenameCityCommand,
        _frame: i32,
    ) -> RenameCityTransactionReceipt {
        RenameCityTransactionReceipt::unavailable(request)
    }

    /// Snapshot the complete host-owned state read by opcode 77. The subsequent atomic
    /// callback must reject the request if this snapshot is no longer current.
    fn cannon_time_facts(&self, _request: CannonTimeRequest) -> Option<CannonTimeFacts> {
        None
    }

    /// Atomic host boundary for opcode 77. A `Planned` receipt is accepted here only when
    /// the host has committed every ordered state, UI, audio, and wall-clock effect and
    /// echoes the exact facts supplied by the bridge.
    fn apply_cannon_time_transaction(
        &mut self,
        request: CannonTimeRequest,
        _facts: CannonTimeFacts,
    ) -> CannonTimeReceipt {
        CannonTimeReceipt::unavailable(request)
    }

    /// Snapshot the diplomacy image used to plan one opcode 37..45 transaction.
    fn diplomacy_command_state(&self) -> Option<DiplomacyCommandState> {
        None
    }

    /// Atomic host boundary for the diplomacy cohort. Hosts may return `Applied` only
    /// after verifying `request.before` is still current and committing a planner `Apply`
    /// decision. Declaration, acceptance, and hostile-rejection boundary decisions must
    /// remain `Unavailable` until their resource/`set_diplo` tails are recovered.
    fn apply_diplomacy_command_transaction(
        &mut self,
        request: DiplomacyCommandRequest,
    ) -> DiplomacyCommandReceipt {
        DiplomacyCommandReceipt::unavailable(request)
    }

    /// Preflight facts for rows 70, 71, 73, 78, and 80. Boundary decisions authorize no
    /// mutation; no-tail decisions may commit only through the atomic callback below.
    fn tail_command_facts(&self, _request: &TailCommandRequest) -> Option<TailCommandFacts> {
        None
    }

    fn apply_tail_command_transaction(
        &mut self,
        request: TailCommandRequest,
        _facts: TailCommandFacts,
    ) -> TailCommandReceipt {
        TailCommandReceipt::unavailable(request)
    }
}

/// One object, holding only what [`Fleet`] exposes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Slot {
    pub alive: bool,
    pub is_unit: bool,
    pub is_building: bool,
    pub can_move: bool,
    pub is_plane: bool,
    pub is_on_map: bool,
    /// Object virtual `get_captain()` used only by FOLLOW's self-target gate. `None`
    /// denotes this slot itself.
    pub follow_captain_o: Option<i32>,
    /// `ObjectData::inside_down/inside_down_who` captured by `add_follow_order`. `None`
    /// makes the secondary FOLLOW identity equal the primary identity.
    pub follow_inside_down: Option<(i32, i32)>,
    pub is_captain: bool,
    pub leaves_groups: bool,
    pub form_category: i32,
    pub formation_member: Option<FormationMember>,
    pub form: i8,
    pub angle: i32,
    pub role: i32,
    pub domain: i32,
    pub unit_flags: u32,
    pub entering_or_exiting: bool,
    pub halt_flag_4_veto: bool,
    pub special: bool,
    pub spy: bool,
    pub unit_masks: u32,
    pub can_ever_transport: bool,
    pub build_masks: u16,
    pub build_mask_capabilities: u16,
    /// Object flag byte at `+0x08`; state-action planners currently read/set bits 0/0x10.
    pub object_flags: u8,
    /// Effective ObjectData virtual `get_stance_type` result.
    pub stance_type: i32,
    pub stance_update_order_present: bool,
    pub stance_update_order_mandatory: bool,
    pub stance_update_action_present: bool,
    pub stance_update_action_mandatory: bool,
    /// Effective concrete type index used by direct action receivers.
    pub type_index: i32,
    /// Unit word at `+0x98`, cleared by `action_stop_spell`.
    pub spell_word_0x98: u16,
    pub build_active: bool,
    pub can_make_disband: bool,
    pub can_make_depopulate: bool,
    pub group: i16,
    pub uid: u16,
    pub x: i32,
    pub y: i32,
    pub stance: i8,
    pub orders: OrderQueue,
}

impl Slot {
    /// A live, movable unit at `(x, y)`.
    pub fn unit(uid: u16, x: i32, y: i32) -> Slot {
        Slot {
            alive: true,
            is_unit: true,
            object_flags: 1,
            can_move: true,
            is_on_map: true,
            is_captain: true,
            can_ever_transport: true,
            group: -1,
            uid,
            x,
            y,
            ..Slot::default()
        }
    }

    /// A live building: alive, not a unit for movement purposes.
    pub fn building(uid: u16, x: i32, y: i32) -> Slot {
        Slot {
            alive: true,
            is_unit: true,
            is_building: true,
            object_flags: 1,
            can_move: false,
            group: -1,
            uid,
            x,
            y,
            ..Slot::default()
        }
    }

    /// A true plane. Air-domain helicopters intentionally continue to use [`Slot::unit`].
    pub fn plane(uid: u16, x: i32, y: i32) -> Slot {
        Slot {
            is_plane: true,
            ..Slot::unit(uid, x, y)
        }
    }
}

/// A dense `[who][o]` object table. Enough to run the bridge, and the shape
/// `objects.lists[who]` has.
#[derive(Clone, Debug, Default)]
pub struct ObjectTable {
    lists: Vec<Vec<Slot>>,
    leader_flags: [u32; NUM_OWNER_SLOTS],
    local_who: Option<u8>,
    pause_steps: Vec<PauseStep>,
    stop_spell_gpiece_update: bool,
}

impl ObjectTable {
    pub fn new(per_owner: usize) -> ObjectTable {
        ObjectTable {
            lists: (0..NUM_OWNER_SLOTS)
                .map(|_| vec![Slot::default(); per_owner])
                .collect(),
            leader_flags: [0; NUM_OWNER_SLOTS],
            local_who: None,
            pause_steps: Vec::new(),
            stop_spell_gpiece_update: false,
        }
    }

    pub fn set_leader_flags(&mut self, who: u8, flags: u32) {
        if let Some(slot) = self.leader_flags.get_mut(who as usize) {
            *slot = flags;
        }
    }

    pub fn set_local_who(&mut self, who: Option<u8>) {
        self.local_who = who;
    }

    pub fn take_pause_steps(&mut self) -> Vec<PauseStep> {
        std::mem::take(&mut self.pause_steps)
    }

    pub fn take_stop_spell_gpiece_update(&mut self) -> bool {
        std::mem::take(&mut self.stop_spell_gpiece_update)
    }

    pub fn put(&mut self, who: u8, o: i16, s: Slot) {
        self.lists[who as usize][o as usize] = s;
    }

    pub fn get(&self, who: u8, o: i16) -> Option<&Slot> {
        if o < 0 {
            return None;
        }
        self.lists.get(who as usize)?.get(o as usize)
    }

    pub fn get_mut(&mut self, who: u8, o: i16) -> Option<&mut Slot> {
        if o < 0 {
            return None;
        }
        self.lists.get_mut(who as usize)?.get_mut(o as usize)
    }
}

impl Fleet for ObjectTable {
    fn alive(&self, who: u8, o: i16) -> bool {
        self.get(who, o).is_some_and(|s| s.alive)
    }
    fn is_unit(&self, who: u8, o: i16) -> bool {
        self.get(who, o).is_some_and(|s| s.is_unit)
    }
    fn is_building(&self, who: u8, o: i16) -> bool {
        self.get(who, o).is_some_and(|s| s.is_building)
    }
    fn is_on_map(&self, who: u8, o: i16) -> bool {
        self.get(who, o).is_some_and(|s| s.is_on_map)
    }
    fn is_captain(&self, who: u8, o: i16) -> bool {
        self.get(who, o).is_some_and(|s| s.is_captain)
    }
    fn leaves_groups(&self, who: u8, o: i16) -> bool {
        self.get(who, o).is_some_and(|s| s.leaves_groups)
    }
    fn form_category(&self, who: u8, o: i16) -> i32 {
        self.get(who, o).map_or(18, |s| s.form_category)
    }
    fn formation_member(
        &self,
        who: u8,
        o: i16,
        _water_destination: bool,
    ) -> Option<FormationMember> {
        let slot = self.get(who, o)?;
        let mut member = slot.formation_member?;
        member.angle = slot.angle;
        member.category = slot.form_category;
        Some(member)
    }
    fn form(&self, who: u8, o: i16) -> i8 {
        self.get(who, o).map_or(0, |s| s.form)
    }
    fn set_form(&mut self, who: u8, o: i16, form: i8) {
        if let Some(s) = self.get_mut(who, o) {
            s.form = form;
        }
    }
    fn angle(&self, who: u8, o: i16) -> i32 {
        self.get(who, o).map_or(0, |s| s.angle)
    }
    fn role(&self, who: u8, o: i16) -> i32 {
        self.get(who, o).map_or(0, |s| s.role)
    }
    fn domain(&self, who: u8, o: i16) -> i32 {
        self.get(who, o).map_or(0, |s| s.domain)
    }
    fn unit_masks(&self, who: u8, o: i16) -> u32 {
        self.get(who, o).map_or(0, |s| s.unit_masks)
    }
    fn set_unit_masks(&mut self, who: u8, o: i16, masks: u32) {
        if let Some(s) = self.get_mut(who, o) {
            s.unit_masks = masks;
        }
    }
    fn can_ever_transport(&self, who: u8, o: i16) -> bool {
        self.get(who, o)
            .is_some_and(|s| s.is_unit && s.can_ever_transport)
    }
    fn build_masks(&self, who: u8, o: i16) -> Option<u16> {
        self.get(who, o)
            .filter(|s| s.is_building)
            .map(|s| s.build_masks)
    }
    fn set_build_masks(&mut self, who: u8, o: i16, masks: u16) -> bool {
        let Some(slot) = self.get_mut(who, o).filter(|s| s.is_building) else {
            return false;
        };
        slot.build_masks = masks;
        true
    }
    fn can_toggle_build_mask(&self, who: u8, o: i16, mask: u16) -> bool {
        self.get(who, o)
            .is_some_and(|s| s.is_building && s.build_mask_capabilities & mask != 0)
    }
    fn can_move(&self, who: u8, o: i16) -> bool {
        self.get(who, o).is_some_and(|s| s.can_move)
    }
    fn is_plane(&self, who: u8, o: i16) -> bool {
        self.get(who, o).is_some_and(|s| s.is_plane)
    }
    fn group_of(&self, who: u8, o: i16) -> i16 {
        self.get(who, o).map_or(-1, |s| s.group)
    }
    fn set_group_of(&mut self, who: u8, o: i16, slot: i16) {
        if let Some(s) = self.get_mut(who, o) {
            s.group = slot;
        }
    }
    fn uid(&self, who: u8, o: i16) -> u16 {
        self.get(who, o).map_or(0xFFFF, |s| s.uid)
    }
    fn pos(&self, who: u8, o: i16) -> (i32, i32) {
        self.get(who, o).map_or((0, 0), |s| (s.x, s.y))
    }
    fn orders(&self, who: u8, o: i16) -> Option<&OrderQueue> {
        self.get(who, o).map(|s| &s.orders)
    }
    fn orders_mut(&mut self, who: u8, o: i16) -> Option<&mut OrderQueue> {
        self.get_mut(who, o).map(|s| &mut s.orders)
    }
    fn set_stance(&mut self, who: u8, o: i16, stance: i8) {
        if let Some(s) = self.get_mut(who, o) {
            s.stance = stance;
        }
    }
    fn disband(&mut self, who: u8, o: i16) {
        if let Some(s) = self.get_mut(who, o) {
            s.alive = false;
            s.group = -1;
            s.orders.clear();
        }
    }

    fn apply_stop_spell_transaction(&mut self, request: StopSpellRequest) -> StopSpellReceipt {
        let group_after_ignore_orders = request.group.clone();
        let n = group_after_ignore_orders
            .num
            .clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
        let members: Vec<_> = group_after_ignore_orders.list[..n]
            .iter()
            .map(|&o| {
                let slot = self.get(group_after_ignore_orders.who, o);
                StopSpellMemberFacts {
                    o,
                    valid_unit: slot.is_some_and(|slot| slot.alive && slot.is_unit),
                    on_map: slot.is_some_and(|slot| slot.is_on_map),
                    current_order: slot
                        .and_then(|slot| slot.orders.current())
                        .map(|order| order.kind),
                    unit_masks: slot.map_or(0, |slot| slot.unit_masks),
                    type_index: slot.map_or(-1, |slot| slot.type_index),
                }
            })
            .collect();
        let Ok(plan) = plan_stop_spell(&group_after_ignore_orders, &members) else {
            return StopSpellReceipt::unavailable(request);
        };
        for step in &plan.steps {
            match *step {
                StopSpellStep::SetUnitMasks { o, value } => {
                    if let Some(slot) = self.get_mut(plan.group.who, o) {
                        slot.unit_masks = value;
                    }
                }
                StopSpellStep::CloseOrders { o, .. } => {
                    if let Some(slot) = self.get_mut(plan.group.who, o) {
                        slot.orders.clear();
                    }
                }
                StopSpellStep::ClearSpellWord98 { o } => {
                    if let Some(slot) = self.get_mut(plan.group.who, o) {
                        slot.spell_word_0x98 = 0;
                    }
                }
                StopSpellStep::SetObjectsFlag22c | StopSpellStep::UpdateGpiece => {
                    self.stop_spell_gpiece_update = true;
                }
                StopSpellStep::ClearPathAnchor { .. }
                | StopSpellStep::ClearPartialPath { .. }
                | StopSpellStep::UpdateAction { .. } => {}
            }
        }
        StopSpellReceipt {
            request,
            status: GroupActionTransactionStatus::Applied,
            group_after_ignore_orders: Some(group_after_ignore_orders),
            members,
            plan: Some(plan),
        }
    }

    fn apply_follow_transaction(&mut self, request: FollowRequest) -> FollowReceipt {
        let group_after_ignore_orders = request.group.clone();
        let target_key = u8::try_from(request.command.target_who)
            .ok()
            .zip(i16::try_from(request.command.target_o).ok());
        let target_slot = target_key.and_then(|(who, o)| self.get(who, o)).cloned();
        let target = target_key.map(|(_who, o)| FollowTargetFacts {
            queried_o: request.command.target_o,
            queried_who: request.command.target_who,
            valid_unit: target_slot
                .as_ref()
                .is_some_and(|slot| slot.alive && slot.is_unit),
            on_map: target_slot.as_ref().is_some_and(|slot| slot.is_on_map),
            is_plane: target_slot.as_ref().is_some_and(|slot| slot.is_plane),
            canonical_o: target_slot
                .as_ref()
                .and_then(|slot| slot.follow_captain_o)
                .unwrap_or(i32::from(o)),
        });
        let n = group_after_ignore_orders
            .num
            .clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
        let members: Vec<_> = group_after_ignore_orders.list[..n]
            .iter()
            .map(|&o| {
                let slot = self.get(group_after_ignore_orders.who, o);
                FollowMemberFacts {
                    o,
                    valid_unit: slot.is_some_and(|slot| slot.alive && slot.is_unit),
                    on_map: slot.is_some_and(|slot| slot.is_on_map),
                    is_plane: slot.is_some_and(|slot| slot.is_plane),
                }
            })
            .collect();
        let Ok(plan) = plan_follow(&request, &group_after_ignore_orders, target, &members) else {
            return FollowReceipt::unavailable(request);
        };

        // This compact host cannot replay Group::finish_insert's arbitrary saved group
        // actions. Fail closed before mutation when set_up_insert would capture any.
        if request.command.queued == 0
            && group_after_ignore_orders.list[..n].iter().any(|&o| {
                self.orders(group_after_ignore_orders.who, o)
                    .is_some_and(|orders| orders.iter().any(|order| order.flags & ORDER_GROUP != 0))
            })
        {
            return FollowReceipt::unavailable(request);
        }

        let mut halt_plan = None;
        if plan
            .effects
            .iter()
            .any(|effect| matches!(effect, FollowEffect::ActionHalt { .. }))
        {
            let mut halt_group = group_after_ignore_orders.clone();
            halt_group.disband = 0;
            let halt_members: Vec<_> = halt_group.list[..n]
                .iter()
                .map(|&o| {
                    let slot = self.get(halt_group.who, o);
                    HaltMemberFacts {
                        o,
                        valid_unit: slot.is_some_and(|slot| slot.alive && slot.is_unit),
                        on_map: slot.is_some_and(|slot| slot.is_on_map),
                        is_plane: slot.is_some_and(|slot| slot.is_plane),
                        domain: slot.map_or(0, |slot| slot.domain),
                        unit_flags: slot.map_or(0, |slot| slot.unit_flags),
                        entering_or_exiting: slot.is_some_and(|slot| slot.entering_or_exiting),
                        flag_4_veto: slot.is_some_and(|slot| slot.halt_flag_4_veto),
                        special: slot.is_some_and(|slot| slot.special),
                        spy: slot.is_some_and(|slot| slot.spy),
                    }
                })
                .collect();
            let Ok(preflight) = plan_action_halt(&halt_group, 0, &halt_members) else {
                return FollowReceipt::unavailable(request);
            };
            halt_plan = Some(preflight);
        }

        for effect in &plan.effects {
            if let FollowEffect::AddFollowOrder {
                actor_who,
                actor_o,
                target_o,
                target_who,
                queued,
            } = *effect
            {
                if !matches!(queued, 1 | 2)
                    || self.orders(actor_who, actor_o).is_none()
                    || target_o != request.command.target_o
                    || target_who != request.command.target_who
                {
                    return FollowReceipt::unavailable(request);
                }
                let Some(target_slot) = target_slot.as_ref() else {
                    return FollowReceipt::unavailable(request);
                };
                if let Some((secondary_o, secondary_who)) = target_slot.follow_inside_down {
                    let Some((who, o)) = u8::try_from(secondary_who)
                        .ok()
                        .zip(i16::try_from(secondary_o).ok())
                    else {
                        return FollowReceipt::unavailable(request);
                    };
                    if self.get(who, o).is_none() {
                        return FollowReceipt::unavailable(request);
                    }
                }
            }
        }

        for effect in &plan.effects {
            match *effect {
                FollowEffect::SetUpInsert | FollowEffect::FinishInsert => {}
                FollowEffect::ActionHalt { flags } => {
                    assert_eq!(flags, 0);
                    for step in &halt_plan.as_ref().expect("halt plan was preflighted").steps {
                        match *step {
                            HaltStep::ClearUnitMask { who, o, mask } => {
                                self.get_mut(who, o)
                                    .expect("halt identity vanished")
                                    .unit_masks &= !mask;
                            }
                            HaltStep::CloseOrders { who, o, .. } => {
                                self.get_mut(who, o)
                                    .expect("halt identity vanished")
                                    .orders
                                    .clear();
                            }
                            HaltStep::ClearPathAnchor { .. }
                            | HaltStep::ClearPartialPath { .. }
                            | HaltStep::UpdateAction { .. } => {}
                        }
                    }
                }
                FollowEffect::AddFollowOrder {
                    actor_who,
                    actor_o,
                    target_o,
                    target_who,
                    queued,
                } => {
                    let target_slot = target_slot.as_ref().expect("follow target was preflighted");
                    let (oxx, whose, uid2) = match target_slot.follow_inside_down {
                        Some((o, who)) => {
                            let slot = self
                                .get(who as u8, o as i16)
                                .expect("secondary follow identity was preflighted");
                            (o, who, slot.uid)
                        }
                        None => (target_o, target_who, target_slot.uid),
                    };
                    let order = OrderRec::follow(FollowOrderPayload {
                        ox: target_o,
                        whom: target_who,
                        uid: target_slot.uid,
                        oxx,
                        whose,
                        uid2,
                    });
                    let orders = self
                        .orders_mut(actor_who, actor_o)
                        .expect("follow actor was preflighted");
                    if queued == 2 {
                        orders.clear();
                    }
                    orders.push_back(order);
                }
            }
        }

        FollowReceipt {
            request,
            status: FollowTransactionStatus::Applied,
            group_after_ignore_orders: Some(group_after_ignore_orders),
            target,
            members,
            plan: Some(plan),
        }
    }

    fn apply_group_halt_transaction(
        &mut self,
        request: GroupHaltTransactionRequest,
    ) -> GroupHaltTransactionReceipt {
        let n = request.group.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
        let members: Vec<_> = request.group.list[..n]
            .iter()
            .map(|&o| {
                let slot = self.get(request.group.who, o);
                HaltMemberFacts {
                    o,
                    valid_unit: slot.is_some_and(|slot| slot.alive && slot.is_unit),
                    on_map: slot.is_some_and(|slot| slot.is_on_map),
                    is_plane: slot.is_some_and(|slot| slot.is_plane),
                    domain: slot.map_or(0, |slot| slot.domain),
                    unit_flags: slot.map_or(0, |slot| slot.unit_flags),
                    entering_or_exiting: slot.is_some_and(|slot| slot.entering_or_exiting),
                    flag_4_veto: slot.is_some_and(|slot| slot.halt_flag_4_veto),
                    special: slot.is_some_and(|slot| slot.special),
                    spy: slot.is_some_and(|slot| slot.spy),
                }
            })
            .collect();
        let Ok(plan) = plan_action_halt(&request.group, request.flags, &members) else {
            return GroupHaltTransactionReceipt::unavailable(request);
        };

        for step in &plan.steps {
            match *step {
                HaltStep::ClearUnitMask { who, o, mask } => {
                    if let Some(slot) = self.get_mut(who, o) {
                        slot.unit_masks &= !mask;
                    }
                }
                HaltStep::CloseOrders { who, o, .. } => {
                    if let Some(slot) = self.get_mut(who, o) {
                        slot.orders.clear();
                    }
                }
                HaltStep::ClearPathAnchor { .. }
                | HaltStep::ClearPartialPath { .. }
                | HaltStep::UpdateAction { .. } => {}
            }
        }
        GroupHaltTransactionReceipt {
            request: request.clone(),
            status: GroupHaltTransactionStatus::Applied,
            group_after_ignore_orders: Some(request.group.clone()),
            members,
            plan: Some(plan),
        }
    }

    fn apply_group_disband_transaction(
        &mut self,
        request: GroupDisbandTransactionRequest,
    ) -> GroupDisbandTransactionReceipt {
        let n = request.group.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
        let members: Vec<_> = request.group.list[..n]
            .iter()
            .map(|&o| {
                let slot = self.get(request.group.who, o);
                DisbandMemberFacts {
                    o,
                    active: slot.is_some_and(|slot| slot.alive),
                    is_build: slot.is_some_and(|slot| slot.is_building),
                    build_active: slot.is_some_and(|slot| slot.build_active),
                    can_make_disband: slot.is_some_and(|slot| slot.can_make_disband),
                    can_make_depopulate: slot.is_some_and(|slot| slot.can_make_depopulate),
                }
            })
            .collect();
        let Ok(plan) = plan_action_disband(&request.group, request.all, false, false, &members)
        else {
            return GroupDisbandTransactionReceipt::unavailable(request);
        };
        if plan
            .steps
            .iter()
            .any(|step| matches!(step, DisbandStep::QueueDisband { .. }))
        {
            return GroupDisbandTransactionReceipt::unavailable(request);
        }
        for step in &plan.steps {
            if let DisbandStep::DisbandObject { who, o, .. } = *step {
                self.disband(who, o);
            }
        }
        GroupDisbandTransactionReceipt {
            request: request.clone(),
            status: GroupDisbandTransactionStatus::Applied,
            group_after_ignore_orders: Some(request.group.clone()),
            validate_disband: Some(false),
            owner_is_local: Some(false),
            members,
            plan: Some(plan),
        }
    }

    fn apply_group_stance_transaction(
        &mut self,
        request: GroupStanceRequest,
    ) -> GroupStanceReceipt {
        let n = request.group.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
        let members: Vec<_> = request.group.list[..n]
            .iter()
            .map(|&o| {
                let slot = self.get(request.group.who, o);
                StanceMemberFacts {
                    o,
                    active: slot.is_some_and(|slot| slot.object_flags & 1 != 0),
                    valid_unit: slot.is_some_and(|slot| slot.alive && slot.is_unit),
                    is_captain: slot.is_some_and(|slot| slot.is_captain),
                    on_map: slot.is_some_and(|slot| slot.is_on_map),
                    object_stance_type: slot.map_or(-1, |slot| slot.stance_type),
                    current_stance: slot.map_or(0, |slot| i32::from(slot.stance)),
                    is_build: slot.is_some_and(|slot| slot.is_building),
                    build_stance_type: slot.map_or(-1, |slot| {
                        if slot.is_building {
                            slot.stance_type
                        } else {
                            -1
                        }
                    }),
                    is_unit: slot.is_some_and(|slot| slot.is_unit),
                    unit_stance_type: slot.map_or(-1, |slot| {
                        if slot.is_unit {
                            slot.stance_type
                        } else {
                            -1
                        }
                    }),
                    is_plane: slot.is_some_and(|slot| slot.is_plane),
                    update_order_first_present: slot
                        .is_some_and(|slot| slot.stance_update_order_present),
                    update_order_second_mandatory: slot
                        .is_some_and(|slot| slot.stance_update_order_mandatory),
                    update_action_first_present: slot
                        .is_some_and(|slot| slot.stance_update_action_present),
                    update_action_second_mandatory: slot
                        .is_some_and(|slot| slot.stance_update_action_mandatory),
                }
            })
            .collect();
        let preferred_stance_type = if request.group.buildings != 0 {
            members
                .first()
                .map_or(-1, |member| member.object_stance_type)
        } else {
            let choose = |require_on_map: bool| {
                request.group.list[..n]
                    .iter()
                    .filter_map(|&o| self.get(request.group.who, o).map(|slot| (o, slot)))
                    .filter(|(_, slot)| {
                        slot.is_unit && slot.is_captain && (!require_on_map || slot.is_on_map)
                    })
                    .min_by_key(|(_, slot)| slot.form_category)
                    .map(|(_, slot)| slot.stance_type)
            };
            choose(true).or_else(|| choose(false)).unwrap_or(-1)
        };
        let leader_flags = self
            .leader_flags
            .get(request.group.who as usize)
            .copied()
            .unwrap_or(0);
        let Ok(plan) = plan_action_stance(
            &request.group,
            request.stance,
            preferred_stance_type,
            leader_flags,
            &members,
        ) else {
            return GroupStanceReceipt::unavailable(request);
        };
        if plan.steps.iter().any(|step| {
            matches!(
                step,
                StanceStep::ClearMandatory { .. }
                    | StanceStep::UpdateOrder { .. }
                    | StanceStep::UpdateAction { .. }
                    | StanceStep::Repath { .. }
                    | StanceStep::KillCurrentOrder { .. }
            )
        }) {
            return GroupStanceReceipt::unavailable(request);
        }
        for step in &plan.steps {
            match *step {
                StanceStep::WriteBuildStance { who, o, value }
                | StanceStep::WriteUnitStance { who, o, value } => {
                    if let Some(slot) = self.get_mut(who, o) {
                        slot.stance = value;
                    }
                }
                StanceStep::SetObjectFlag { who, o, mask } => {
                    if let Some(slot) = self.get_mut(who, o) {
                        slot.object_flags |= mask;
                    }
                }
                StanceStep::ClearOrders { who, o } => {
                    if let Some(slot) = self.get_mut(who, o) {
                        slot.orders.clear();
                    }
                }
                StanceStep::ClearMandatory { .. }
                | StanceStep::UpdateOrder { .. }
                | StanceStep::UpdateAction { .. }
                | StanceStep::Repath { .. }
                | StanceStep::KillCurrentOrder { .. } => {
                    unreachable!("unsupported stance tails were rejected before mutation")
                }
            }
        }
        GroupStanceReceipt {
            request: request.clone(),
            status: GroupStateTransactionStatus::Applied,
            preferred_stance_type: Some(preferred_stance_type),
            leader_flags: Some(leader_flags),
            members,
            plan: Some(plan),
        }
    }

    fn apply_group_set_transport_transaction(
        &mut self,
        request: GroupSetTransportRequest,
    ) -> GroupSetTransportReceipt {
        let flags = self
            .leader_flags
            .get(request.group.who as usize)
            .copied()
            .unwrap_or(0);
        let transport_level = if flags & 0x100 != 0 {
            3
        } else if flags & 0x200 != 0 {
            2
        } else {
            ((flags >> 10) & 1) as u8
        };
        let group_after_ignore_orders = request.group.clone();
        let n = request.group.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
        let members: Vec<_> = request.group.list[..n]
            .iter()
            .map(|&o| {
                let slot = self.get(request.group.who, o);
                SetTransportMemberFacts {
                    o,
                    valid_unit: slot.is_some_and(|slot| slot.alive && slot.is_unit),
                    can_ever_transport: slot.is_some_and(|slot| slot.can_ever_transport),
                    unit_masks: slot.map_or(0, |slot| slot.unit_masks),
                }
            })
            .collect();
        let Ok(plan) = plan_action_set_transport(
            &group_after_ignore_orders,
            request.flag,
            transport_level,
            &members,
        ) else {
            return GroupSetTransportReceipt::unavailable(request);
        };
        for step in &plan.steps {
            let SetTransportStep::WriteUnitMasks { who, o, value } = *step;
            if let Some(slot) = self.get_mut(who, o) {
                slot.unit_masks = value;
            }
        }
        GroupSetTransportReceipt {
            request: request.clone(),
            status: GroupStateTransactionStatus::Applied,
            group_after_ignore_orders: Some(group_after_ignore_orders),
            transport_level: Some(transport_level),
            members,
            plan: Some(plan),
        }
    }

    fn apply_group_unitmask_transaction(
        &mut self,
        request: GroupUnitMaskRequest,
    ) -> GroupUnitMaskReceipt {
        let n = request.group.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
        let members: Vec<_> = request.group.list[..n]
            .iter()
            .map(|&o| {
                let slot = self.get(request.group.who, o);
                UnitMaskMemberFacts {
                    o,
                    valid_unit: slot.is_some_and(|slot| slot.alive && slot.is_unit),
                    is_plane: slot.is_some_and(|slot| slot.is_plane),
                    unit_masks: slot.map_or(0, |slot| slot.unit_masks),
                }
            })
            .collect();
        let Ok(plan) = plan_action_unitmask(&request.group, request.mask, request.set, &members)
        else {
            return GroupUnitMaskReceipt::unavailable(request);
        };
        for step in &plan.steps {
            match *step {
                UnitMaskStep::WriteUnitMasks { who, o, value } => {
                    if let Some(slot) = self.get_mut(who, o) {
                        slot.unit_masks = value;
                    }
                }
                UnitMaskStep::SetObjectFlag { who, o, mask } => {
                    if let Some(slot) = self.get_mut(who, o) {
                        slot.object_flags |= mask;
                    }
                }
                UnitMaskStep::ClearUnitMasks { who, o, mask } => {
                    if let Some(slot) = self.get_mut(who, o) {
                        slot.unit_masks &= !mask;
                    }
                }
                UnitMaskStep::CloseOrders { who, o, .. } => {
                    if let Some(slot) = self.get_mut(who, o) {
                        slot.orders.clear();
                    }
                }
                UnitMaskStep::ClearPathAnchor { .. }
                | UnitMaskStep::ClearPartialPath { .. }
                | UnitMaskStep::UpdateAction { .. } => {}
            }
        }
        GroupUnitMaskReceipt {
            request: request.clone(),
            status: GroupStateTransactionStatus::Applied,
            members,
            plan: Some(plan),
        }
    }

    fn apply_group_buildmask_transaction(
        &mut self,
        request: GroupBuildMaskRequest,
    ) -> GroupBuildMaskReceipt {
        let n = request.group.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
        let members: Vec<_> = request.group.list[..n]
            .iter()
            .map(|&o| {
                let slot = self.get(request.group.who, o);
                BuildMaskMemberFacts {
                    o,
                    valid_build: slot.is_some_and(|slot| slot.alive && slot.is_building),
                    valid_buildmask: slot.is_some_and(|slot| {
                        slot.is_building && slot.build_mask_capabilities & request.mask != 0
                    }),
                    build_masks: slot.map_or(0, |slot| slot.build_masks),
                }
            })
            .collect();
        let owner_is_local = self.local_who == Some(request.group.who);
        let Ok(plan) = plan_action_buildmask(
            &request.group,
            request.mask,
            request.set,
            owner_is_local,
            &members,
        ) else {
            return GroupBuildMaskReceipt::unavailable(request);
        };
        for step in &plan.steps {
            match *step {
                BuildMaskStep::WriteBuildMasks { who, o, value } => {
                    if let Some(slot) = self.get_mut(who, o) {
                        slot.build_masks = value;
                    }
                }
                BuildMaskStep::Feedback { .. } => {}
            }
        }
        GroupBuildMaskReceipt {
            request: request.clone(),
            status: GroupStateTransactionStatus::Applied,
            owner_is_local: Some(owner_is_local),
            members,
            plan: Some(plan),
        }
    }

    fn apply_pause_transaction(
        &mut self,
        request: PauseTransactionRequest,
    ) -> PauseTransactionReceipt {
        let facts = PauseHostFacts::default();
        let Some(plan) = plan_pause(&request, &facts) else {
            return PauseTransactionReceipt::unavailable(request);
        };
        self.pause_steps.extend(plan.steps.iter().cloned());
        PauseTransactionReceipt {
            request: request.clone(),
            status: PauseTransactionStatus::Applied,
            facts: Some(facts),
            plan: Some(plan),
        }
    }
}

// ---------------------------------------------------------------------------
// Groups — the pool that CommandPackage::group indexes
// ---------------------------------------------------------------------------

/// `Groups` `0x00E85F10`: the pool of `Group` slots, `groups.list` at `+0x10`.
///
/// Retail sizes the pool by owner: `Groups::get_open_slot` `0x006FA460` bases its scan at
/// `who * 0x40` and bounds it at `+0x2E`, so each owner has 64 strides of which 46 are
/// reachable by the allocator [measured].
#[derive(Clone, Debug)]
pub struct Groups {
    slots: Vec<GroupData>,
    /// `groups.cur[who]` at `0x00E85F2C + who*4`: the slot `push_group` last used for
    /// this owner, and the one `get_open_slot` refuses to recycle [measured].
    cur: [i32; NUM_OWNER_SLOTS],
}

impl Default for Groups {
    fn default() -> Groups {
        Groups::new()
    }
}

impl Groups {
    pub fn new() -> Groups {
        Groups {
            // `Group::clear(slot)` preserves/writes this identity at `GroupData+0x04`;
            // `Groups::copy_group` deliberately does not copy it from the transient
            // selection. GROUP_MOVE embeds it in the order id.
            slots: (0..NUM_OWNER_SLOTS * GROUP_SLOTS_STRIDE)
                .map(|id| GroupData {
                    id: id as i32,
                    ..GroupData::default()
                })
                .collect(),
            cur: [-1; NUM_OWNER_SLOTS],
        }
    }

    #[inline]
    pub fn get(&self, slot: i32) -> Option<&GroupData> {
        usize::try_from(slot).ok().and_then(|i| self.slots.get(i))
    }

    #[inline]
    pub fn get_mut(&mut self, slot: i32) -> Option<&mut GroupData> {
        usize::try_from(slot)
            .ok()
            .and_then(move |i| self.slots.get_mut(i))
    }

    #[inline]
    pub fn cur(&self, who: u8) -> i32 {
        self.cur[who as usize % NUM_OWNER_SLOTS]
    }

    /// `Groups::get_open_slot` `0x006FA460`.
    ///
    /// Scans this owner's `[who*64, who*64+46)` window for the slot with the smallest
    /// `stamp` — a least-recently-touched policy — never returning `cur[who]`. Retail also
    /// prefers a slot whose `Group::get_num() == 1` and skips groups still marked
    /// `buildings`; those predicates need the object side, so this port takes the
    /// `stamp` rule only and says so.
    pub fn get_open_slot(&self, who: u8) -> i32 {
        let base = who as usize % NUM_OWNER_SLOTS * GROUP_SLOTS_STRIDE;
        let mut best = base as i32;
        let mut best_stamp = i32::MAX;
        for i in base..base + GROUP_SLOTS_SCANNED {
            if i as i32 == self.cur(who) {
                continue;
            }
            let s = self.slots[i].stamp;
            if s < best_stamp {
                best_stamp = s;
                best = i as i32;
            }
        }
        best
    }

    /// `Groups::push_group(who, Group* g, int singular)` `0x0070F9E0`.
    ///
    /// Two behaviours worth keeping, both [structure]:
    ///
    /// * `singular == 0 && g.num < 2` clears every member's `ObjectData::group` and
    ///   returns `-1` — a one-unit selection gets **no** slot unless the caller insists.
    ///   `process_group` passes `1`, so UI selections always intern.
    /// * every member's `ObjectData::group` is repointed at the new slot, and if it
    ///   already belonged to a *different* slot it is removed from that one first.
    pub fn push_group(
        &mut self,
        who: u8,
        g: &GroupData,
        singular: bool,
        frame: i32,
        f: &mut dyn Fleet,
    ) -> i32 {
        let n = g.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
        if !singular && g.num < 2 {
            for &o in &g.list[..n] {
                if f.alive(who, o) {
                    f.set_group_of(who, o, -1);
                }
            }
            return -1;
        }
        let mut slot = self.cur(who);
        let reuse = self
            .get(slot)
            .is_some_and(|c| c.num == g.num && c.list[..n] == g.list[..n]);
        if !reuse {
            slot = self.get_open_slot(who);
            let dst = slot as usize;
            let id = self.slots[dst].id;
            self.slots[dst] = g.clone();
            self.slots[dst].id = id;
            self.slots[dst].stamp = frame;
            self.cur[who as usize % NUM_OWNER_SLOTS] = slot;
        }
        for &o in &g.list[..n] {
            if !f.alive(who, o) {
                continue;
            }
            let prev = f.group_of(who, o);
            if prev >= 0 && prev as i32 != slot {
                if let Some(old) = self.get_mut(prev as i32) {
                    old.remove_member(o);
                }
            }
            f.set_group_of(who, o, slot as i16);
        }
        slot
    }
}

/// `GroupData::remove` by object index, keeping the parallel arrays compact.
trait RemoveMember {
    fn remove_member(&mut self, o: i16);
}

impl RemoveMember for GroupData {
    fn remove_member(&mut self, o: i16) {
        let n = self.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
        if let Some(i) = self.list[..n].iter().position(|&m| m == o) {
            for j in i..n - 1 {
                self.list[j] = self.list[j + 1];
                self.off_x[j] = self.off_x[j + 1];
                self.off_y[j] = self.off_y[j + 1];
                self.curr_x[j] = self.curr_x[j + 1];
                self.curr_y[j] = self.curr_y[j + 1];
                self.angles[j] = self.angles[j + 1];
            }
            self.num -= 1;
        }
    }
}

// ---------------------------------------------------------------------------
// The bridge
// ---------------------------------------------------------------------------

/// `CommandPackage`'s per-package scratch, as the handlers read it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Package {
    /// `+0x00` simulation frame.
    pub stamp: u32,
    /// `+0x04` issuing player slot.
    pub play: i32,
    /// `+0x0C` the `groups` slot every handler in this package addresses. `-1` drops the
    /// command. Written by opcode 0.
    pub group: i32,
}

impl Package {
    pub fn new(play: i32, stamp: u32) -> Package {
        Package {
            stamp,
            play,
            group: -1,
        }
    }
}

/// What one command did, so a run can report the bridge's real yield rather than a guess.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BridgeStats {
    /// Commands whose opcode was recognised and whose handler ran.
    pub dispatched: u64,
    /// Commands dropped because `package.group < 0` — retail drops these too.
    pub no_group: u64,
    /// Commands reaching a `Group::action_*` this module has ported.
    pub acted: u64,
    /// Commands reaching a `Group::action_*` that is [`Port::Todo`].
    pub unported: u64,
    /// Commands whose handler calls no `*::action_*` (lockstep, chat, camera, cheats we
    /// do not implement).
    pub inert: u64,
    /// Non-group state, receipt, or atomic transaction handlers reproduced by this bridge.
    pub inline_state: u64,
    /// Orders actually installed on a unit.
    pub orders_installed: u64,
    /// Order lists cleared (`action_halt`, `QueuePos::New`).
    pub orders_cleared: u64,
    /// Selections interned by opcode 0.
    pub selections: u64,
    /// Per-`OrderIndex` install counts.
    pub by_order: [u64; crate::order::NUM_UNIT_ORDERS],
    /// Per-opcode dispatch counts.
    pub by_opcode: [u32; NUM_OPCODES],
    /// Per-action dispatch counts, index-aligned with [`GROUP_ACTIONS`].
    pub by_action: [u32; NUM_GROUP_ACTIONS],
}

impl Default for BridgeStats {
    fn default() -> BridgeStats {
        BridgeStats {
            dispatched: 0,
            no_group: 0,
            acted: 0,
            unported: 0,
            inert: 0,
            inline_state: 0,
            orders_installed: 0,
            orders_cleared: 0,
            selections: 0,
            by_order: [0; crate::order::NUM_UNIT_ORDERS],
            by_opcode: [0; NUM_OPCODES],
            by_action: [0; NUM_GROUP_ACTIONS],
        }
    }
}

impl BridgeStats {
    pub fn merge(&mut self, o: &BridgeStats) {
        self.dispatched += o.dispatched;
        self.no_group += o.no_group;
        self.acted += o.acted;
        self.unported += o.unported;
        self.inert += o.inert;
        self.inline_state += o.inline_state;
        self.orders_installed += o.orders_installed;
        self.orders_cleared += o.orders_cleared;
        self.selections += o.selections;
        for i in 0..crate::order::NUM_UNIT_ORDERS {
            self.by_order[i] += o.by_order[i];
        }
        for i in 0..NUM_OPCODES {
            self.by_opcode[i] += o.by_opcode[i];
        }
        for i in 0..NUM_GROUP_ACTIONS {
            self.by_action[i] += o.by_action[i];
        }
    }

    /// Share of group-addressed commands that reached a ported action.
    pub fn ported_fraction(&self) -> f64 {
        let d = self.acted + self.unported;
        if d == 0 {
            return 0.0;
        }
        self.acted as f64 / d as f64
    }
}

pub const PLAYER_SPEED_FIELDS: usize = 8;
pub const NUM_NETWORK_PLAYERS: usize = 8;
pub const HOTKEY_GROUP_SLOTS: usize = 162;
pub const CHEAT_TECH_COUNT: usize = 0x326;
pub const CHEAT_TECH_BYTES: usize = CHEAT_TECH_COUNT.div_ceil(8);
pub const RESOURCE_BUCKETS: usize = 6;
pub const RESOURCE_BUCKET_XOR: u32 = 0x8221;

/// External `SoundRef::play` request selected by `Game::action_cheat_zero_buckets`.
///
/// The command bridge owns the exact response selection and sound-stream consumption.
/// Audio remains outside the headless simulation, so the product layer drains these
/// receipts and performs the corresponding sound request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheatResponseReceipt {
    pub who: u8,
    /// Index selected from `DAT_00EB2604`, before resolving its sound-reference ID.
    pub response_slot: u32,
    /// Index into the retail `SoundRef` array at `0x00ECB000`.
    pub sound_ref: i32,
}

/// External `SoundGlobal::play` request emitted by `Game::action_cheat_warning`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheatWarningReceipt {
    pub who: i32,
    /// `SoundGlobalCat` 99. `SoundGlobal::play` owns its subsequent sound-RNG selection.
    pub sound_category: i32,
}

/// Ordered presentation/diagnostic evidence emitted by command handlers whose retail
/// tails do not mutate walked simulation state.
///
/// Coordinates and text retain their raw wire representation. `delivered_to` records the
/// exact leader slots which pass the symmetric chat-status gates in `process_ping` and
/// `process_spline`; the product layer remains responsible for drawing the ping/spline,
/// chat text, and camera motion and for writing diagnostic logs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandSideEffectReceipt {
    Ping {
        package_play: i32,
        sender_who: Option<u8>,
        x: i32,
        y: i32,
        delivered_to: Vec<u8>,
    },
    Spline {
        package_play: i32,
        sender_who: Option<u8>,
        spline_type: u8,
        spline_flags: u8,
        spline_cmd: u8,
        points: Vec<(i32, i32)>,
        delivered_to: Vec<u8>,
    },
    CheckRandom {
        seed: u32,
    },
    Chat {
        package_play: i32,
        sender_who: Option<u8>,
        bits: u32,
        taunt: i32,
        taunt_num: i32,
        text_len: u32,
        /// The command carries `text_len + 1` UTF-16 code units, including the terminator.
        utf16_with_nul: Vec<u16>,
    },
    Camera {
        package_play: i32,
        zoom: u8,
        x: i32,
        y: i32,
        local_sender: bool,
    },
    Marwan {
        start: u8,
    },
}

/// The deterministic pause columns read and written by `TurnControl::process_pause`
/// `0x00956990` and `TurnControl::toggle_pause` `0x00957AB0`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PauseState {
    pub paused: bool,
    pub pause_delay: i32,
    pub network: bool,
    pub immediate_process: bool,
    pub pause_override: bool,
    pub pauses: [u8; NUM_OWNER_SLOTS],
    pub restart_delay: i32,
    /// `Game+0x821 & 0x02`.
    pub restart_gate_2: bool,
    /// `Game+0x821 & 0x04`; retail clears this before `Game::balance.next()`.
    pub restart_gate_4: bool,
    /// `Game+0x820 & 0x40`, set if the balance restart finishes.
    pub chat_filter_bypass: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PauseTransactionRequest {
    pub play: i32,
    /// Raw byte at `PauseCommand+1`; retail compares it with the one-bit paused flag.
    pub requested: u8,
    pub local_play: i32,
    pub state: PauseState,
}

/// Host facts/callback outcomes which are outside the command bridge's owned state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PauseHostFacts {
    pub sound_system_present: bool,
    pub interface_present: bool,
    /// Result of the side-effecting embedded `Game::balance.next()` call. It must be
    /// present exactly when the restart-gate branch is reached.
    pub unit_balance_next_result: Option<i32>,
    /// Exact leader status dwords, required only when `unit_balance_next_result == 0`.
    pub leader_status_words: Option<[u32; NUM_NETWORK_PLAYERS]>,
}

/// Instruction-ordered state and external effects of one pause command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PauseStep {
    DuplicateDiagnostic {
        requested: u8,
    },
    GlobalSound {
        category: i32,
    },
    SoundSystemPause,
    SoundSystemResume,
    SetPaused {
        value: bool,
    },
    SetPauseDelay {
        value: i32,
    },
    PauseLimitMessage {
        play: i32,
    },
    IncrementPauseCount {
        play: u8,
        value: u8,
    },
    PauseMessage {
        play: u8,
        override_template: bool,
        pauses_remaining: i32,
    },
    ClearRestartGate4,
    SetRestartDelay {
        value: i32,
    },
    UnitBalanceNext {
        result: i32,
    },
    SetChatFilterBypass,
    LeaderVictory {
        who: u8,
        victory_type: i32,
        instant: i32,
    },
    ResetSoloFrameClock,
    PauseNotice,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PausePlan {
    pub state: PauseState,
    pub steps: Vec<PauseStep>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseTransactionStatus {
    Applied,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PauseTransactionReceipt {
    pub request: PauseTransactionRequest,
    pub status: PauseTransactionStatus,
    pub facts: Option<PauseHostFacts>,
    pub plan: Option<PausePlan>,
}

impl PauseTransactionReceipt {
    pub fn unavailable(request: PauseTransactionRequest) -> Self {
        Self {
            request,
            status: PauseTransactionStatus::Unavailable,
            facts: None,
            plan: None,
        }
    }

    pub fn validates(&self, expected: &PauseTransactionRequest) -> bool {
        if &self.request != expected {
            return false;
        }
        match self.status {
            PauseTransactionStatus::Unavailable => self.facts.is_none() && self.plan.is_none(),
            PauseTransactionStatus::Applied => {
                let (Some(facts), Some(observed)) = (&self.facts, &self.plan) else {
                    return false;
                };
                plan_pause(expected, facts).is_some_and(|plan| plan == *observed)
            }
        }
    }
}

/// Pure recovered planner for `TurnControl::process_pause` / `toggle_pause`.
pub fn plan_pause(request: &PauseTransactionRequest, facts: &PauseHostFacts) -> Option<PausePlan> {
    let mut state = request.state.clone();
    let mut steps = Vec::new();
    if state.paused as u8 == request.requested {
        if facts.unit_balance_next_result.is_some() || facts.leader_status_words.is_some() {
            return None;
        }
        steps.push(PauseStep::DuplicateDiagnostic {
            requested: request.requested,
        });
        return Some(PausePlan { state, steps });
    }

    let mut restart_reached = false;
    if state.paused {
        if !state.immediate_process {
            state.paused = false;
            steps.push(PauseStep::SetPaused { value: false });
            if state.pause_delay == 0 {
                state.pause_delay = 2;
                steps.push(PauseStep::SetPauseDelay { value: 2 });
            }
            if facts.sound_system_present {
                steps.push(PauseStep::SoundSystemResume);
            }
            if state.network {
                steps.push(PauseStep::GlobalSound { category: 87 });
                if state.restart_gate_2 && state.restart_gate_4 {
                    restart_reached = true;
                    state.restart_gate_4 = false;
                    steps.push(PauseStep::ClearRestartGate4);
                    if state.restart_delay == 0 {
                        state.restart_delay = 2;
                        steps.push(PauseStep::SetRestartDelay { value: 2 });
                    }
                    let result = facts.unit_balance_next_result?;
                    steps.push(PauseStep::UnitBalanceNext { result });
                    if result == 0 {
                        let leaders = facts.leader_status_words?;
                        state.chat_filter_bypass = true;
                        steps.push(PauseStep::SetChatFilterBypass);
                        state.restart_delay = 0;
                        steps.push(PauseStep::SetRestartDelay { value: 0 });
                        for (who, status) in leaders.into_iter().enumerate() {
                            if status & 0x63 == 3 {
                                steps.push(PauseStep::LeaderVictory {
                                    who: who as u8,
                                    victory_type: 0,
                                    instant: 0,
                                });
                            }
                        }
                    } else if facts.leader_status_words.is_some() {
                        return None;
                    }
                }
            }
        }
    } else if !state.network {
        state.paused = true;
        steps.push(PauseStep::SetPaused { value: true });
        state.pause_delay = 0;
        steps.push(PauseStep::SetPauseDelay { value: 0 });
        if facts.sound_system_present {
            steps.push(PauseStep::SoundSystemPause);
        }
    } else {
        let play_slot = usize::try_from(request.play)
            .ok()
            .filter(|&play| play < NUM_OWNER_SLOTS);
        let allowed = request.play < 0
            || play_slot.is_some_and(|play| state.pauses[play] < 10)
            || state.pause_override;
        if allowed {
            steps.push(PauseStep::GlobalSound { category: 87 });
            state.paused = true;
            steps.push(PauseStep::SetPaused { value: true });
            state.pause_delay = 0;
            steps.push(PauseStep::SetPauseDelay { value: 0 });
            if facts.sound_system_present {
                steps.push(PauseStep::SoundSystemPause);
            }
            if let Some(play) = play_slot {
                state.pauses[play] = state.pauses[play].wrapping_add(1);
                steps.push(PauseStep::IncrementPauseCount {
                    play: play as u8,
                    value: state.pauses[play],
                });
                steps.push(PauseStep::PauseMessage {
                    play: play as u8,
                    override_template: state.pause_override,
                    pauses_remaining: 10 - i32::from(state.pauses[play]),
                });
            }
        } else if request.local_play == request.play {
            steps.push(PauseStep::PauseLimitMessage { play: request.play });
            steps.push(PauseStep::GlobalSound { category: 64 });
        }
    }

    if !restart_reached
        && (facts.unit_balance_next_result.is_some() || facts.leader_status_words.is_some())
    {
        return None;
    }
    if !state.network {
        steps.push(PauseStep::ResetSoloFrameClock);
    }
    if facts.interface_present {
        steps.push(PauseStep::PauseNotice);
    }
    Some(PausePlan { state, steps })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotKeyCamera {
    /// Raw IEEE-754 bits from `HotKeyCommand::x/y`; retaining bits preserves NaN payloads.
    pub x_bits: u32,
    pub y_bits: u32,
    pub zoom: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HotKeySlot {
    pub group: GroupData,
    pub camera: Option<HotKeyCamera>,
}

/// `TurnControl`'s five eight-player telemetry arrays written by `TurnDataCommand`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TurnDataState {
    pub flags: u8,
    pub last_wait_times: [u32; NUM_NETWORK_PLAYERS],
    pub last_lag_times: [u32; NUM_NETWORK_PLAYERS],
    pub last_average_frame_times: [u32; NUM_NETWORK_PLAYERS],
    pub last_ping_times: [u32; NUM_NETWORK_PLAYERS],
    pub last_forced_loads: [u32; NUM_NETWORK_PLAYERS],
}

/// State written inline by command handlers without an `action_*` receiver.
///
/// `speed` is `TurnControl+0x30`. `network`, `speed_locked`, and `immediate_process`
/// name the exact `Game+0x820/0x20/0x821` gates read by the handlers. The eight player
/// counters are the `u32` fields at `PlayerData+0x48..+0x68`. `ai_speed` and `ai_off`
/// are the `GameAccess` globals at `0x00C061C0/0x00C061C4`; `checksums` is the
/// `CommandPackage` peer-total table at `0x00CBEE90` [measured].
#[derive(Clone, Debug, PartialEq)]
pub struct InlineCommandState {
    pub speed: i32,
    pub network: bool,
    pub speed_locked: bool,
    pub paused: bool,
    pub pause_delay: i32,
    pub immediate_process: bool,
    pub pause_override: bool,
    pub pauses: [u8; NUM_OWNER_SLOTS],
    pub player_speed: [[u32; PLAYER_SPEED_FIELDS]; NUM_OWNER_SLOTS],
    pub ai_speed: i32,
    /// Retail stores an `int`, and toggles any non-zero value back to zero.
    pub ai_off: i32,
    pub checksums: [u32; NUM_NETWORK_PLAYERS],
    /// `CommandPackage::checksum_recheck` at `0x00CC00B0` gates opcode 58's store.
    pub checksum_recheck: i32,
    /// `Player::who` for each package play slot; ChatSet indexes its state by this map.
    pub player_who: [u8; NUM_NETWORK_PLAYERS],
    /// `LeaderData::valid & 1`, used by the presentation delivery loops.
    pub leader_valid: [bool; NUM_NETWORK_PLAYERS],
    /// `LeaderData::play` (`+0x08`) for the local spline delivery predicate.
    pub leader_play: [i32; NUM_NETWORK_PLAYERS],
    /// Eight recipient status words for each `who` in the global chat matrix.
    pub chat_status: [[u32; NUM_NETWORK_PLAYERS]; NUM_NETWORK_PLAYERS],
    pub local_play: i32,
    /// `Console+0x298`, distinct from the current command player's `Console+0x2A0`.
    pub display_play: i32,
    pub reveal_map: bool,
    /// `Game+0x820 & 0x40`, which bypasses both directed chat-status delivery gates.
    pub chat_filter_bypass: bool,
    pub accum_cheated: [u8; NUM_OWNER_SLOTS],
    /// `PlayerData::valid & 1` for the eight player slots scanned by opcode 67.
    pub player_valid: [bool; CHEAT_INIT_PLAYER_SLOTS],
    pub tech_bits: [[u8; CHEAT_TECH_BYTES]; NUM_NETWORK_PLAYERS],
    pub tech_status: [i32; NUM_NETWORK_PLAYERS],
    pub resource_buckets_encoded: [[u32; RESOURCE_BUCKETS]; NUM_NETWORK_PLAYERS],
    /// `SoundGlobal::random` at `0x00E85F0C`, consumed by opcode 66 in network mode.
    pub sound_random: crate::rng::Random,
    /// Retail's `DAT_00EB2604` response-ID list and `DAT_00ECAFF4` SoundRef count.
    pub cheat_response_sound_refs: Vec<i32>,
    pub sound_ref_count: i32,
    /// Product-facing requests for the external `SoundRef::play` tail.
    pub cheat_response_receipts: Vec<CheatResponseReceipt>,
    /// Product-facing fixed sound requests from `action_cheat_warning`.
    pub cheat_warning_receipts: Vec<CheatWarningReceipt>,
    /// Validated world transaction evidence for opcode 67.
    pub cheat_init_unit_receipts: Vec<CheatInitUnitReceiptRecord>,
    /// Validated host transaction evidence for opcodes 46 through 49.
    pub direct_entity_receipts: Vec<DirectEntityReceiptRecord>,
    /// Validated no-tail/boundary evidence for opcodes 70, 71, 73, 78, and 80.
    pub tail_command_receipts: Vec<TailCommandReceiptRecord>,
    pub command_side_effect_receipts: Vec<CommandSideEffectReceipt>,
    pub turn_data: TurnDataState,
    pub mp_log: bool,
    pub restart_delay: i32,
    pub restart_gate_2: bool,
    pub restart_gate_4: bool,
    pub hotkeys: Vec<HotKeySlot>,
}

impl Default for InlineCommandState {
    fn default() -> Self {
        InlineCommandState {
            speed: crate::schedule::SPEED_NORMAL as i32,
            network: false,
            speed_locked: false,
            paused: false,
            pause_delay: 0,
            immediate_process: false,
            pause_override: false,
            pauses: [0; NUM_OWNER_SLOTS],
            player_speed: [[0; PLAYER_SPEED_FIELDS]; NUM_OWNER_SLOTS],
            ai_speed: 1,
            ai_off: 0,
            checksums: [0; NUM_NETWORK_PLAYERS],
            checksum_recheck: 0,
            player_who: std::array::from_fn(|play| play as u8),
            leader_valid: [false; NUM_NETWORK_PLAYERS],
            leader_play: std::array::from_fn(|play| play as i32),
            chat_status: [[0; NUM_NETWORK_PLAYERS]; NUM_NETWORK_PLAYERS],
            local_play: 0,
            display_play: 0,
            reveal_map: false,
            chat_filter_bypass: false,
            accum_cheated: [0; NUM_OWNER_SLOTS],
            player_valid: [false; CHEAT_INIT_PLAYER_SLOTS],
            tech_bits: [[0; CHEAT_TECH_BYTES]; NUM_NETWORK_PLAYERS],
            tech_status: [0; NUM_NETWORK_PLAYERS],
            resource_buckets_encoded: [[RESOURCE_BUCKET_XOR; RESOURCE_BUCKETS];
                NUM_NETWORK_PLAYERS],
            sound_random: crate::rng::Random::new(0),
            cheat_response_sound_refs: Vec::new(),
            sound_ref_count: 0,
            cheat_response_receipts: Vec::new(),
            cheat_warning_receipts: Vec::new(),
            cheat_init_unit_receipts: Vec::new(),
            direct_entity_receipts: Vec::new(),
            tail_command_receipts: Vec::new(),
            command_side_effect_receipts: Vec::new(),
            turn_data: TurnDataState::default(),
            mp_log: false,
            restart_delay: 0,
            restart_gate_2: false,
            restart_gate_4: false,
            hotkeys: (0..HOTKEY_GROUP_SLOTS)
                .map(|id| HotKeySlot {
                    group: GroupData {
                        id: id as i32,
                        ..GroupData::default()
                    },
                    camera: None,
                })
                .collect(),
        }
    }
}

/// `HotKeyGroups::copy_group` `0x00715120`. Retail copies only these scalar fields and
/// the live prefixes of six parallel arrays; every other destination field survives.
fn copy_hotkey_group(dst: &mut GroupData, src: &GroupData, frame: i32) {
    dst.who = src.who;
    dst.num = src.num;
    dst.ox = src.ox;
    dst.oy = src.oy;
    dst.o_dist = src.o_dist;
    dst.o_angle = src.o_angle;
    dst.buildings = src.buildings;
    dst.speed = src.speed;
    dst.stamp = frame;
    let n = src.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
    dst.list[..n].copy_from_slice(&src.list[..n]);
    dst.angles[..n].copy_from_slice(&src.angles[..n]);
    dst.off_x[..n].copy_from_slice(&src.off_x[..n]);
    dst.off_y[..n].copy_from_slice(&src.off_y[..n]);
    dst.curr_x[..n].copy_from_slice(&src.curr_x[..n]);
    dst.curr_y[..n].copy_from_slice(&src.curr_y[..n]);
}

/// The command→order bridge.
pub struct Bridge {
    pub groups: Groups,
    /// `CommandPackage::process_group`'s per-player re-selection cache: the `(o, uid)`
    /// pairs of the last non-empty selection. `0x00CBEE88` holds the counts and
    /// `0x00CBEEB0` / `0x00CBF6B0` the two parallel arrays [structure].
    last_selection: [Vec<(i16, u16)>; NUM_OWNER_SLOTS],
    pub stats: BridgeStats,
    pub inline: InlineCommandState,
    /// `Game::frame`, stamped into interned groups.
    pub frame: i32,
}

impl Default for Bridge {
    fn default() -> Bridge {
        Bridge::new()
    }
}

impl Bridge {
    pub fn new() -> Bridge {
        Bridge {
            groups: Groups::new(),
            last_selection: std::array::from_fn(|_| Vec::new()),
            stats: BridgeStats::default(),
            inline: InlineCommandState::default(),
            frame: 0,
        }
    }

    /// Drain external sound-response calls selected by completed command processing.
    pub fn take_cheat_response_receipts(&mut self) -> Vec<CheatResponseReceipt> {
        std::mem::take(&mut self.inline.cheat_response_receipts)
    }

    pub fn take_cheat_warning_receipts(&mut self) -> Vec<CheatWarningReceipt> {
        std::mem::take(&mut self.inline.cheat_warning_receipts)
    }

    pub fn take_cheat_init_unit_receipts(&mut self) -> Vec<CheatInitUnitReceiptRecord> {
        std::mem::take(&mut self.inline.cheat_init_unit_receipts)
    }

    pub fn take_direct_entity_receipts(&mut self) -> Vec<DirectEntityReceiptRecord> {
        std::mem::take(&mut self.inline.direct_entity_receipts)
    }

    pub fn take_tail_command_receipts(&mut self) -> Vec<TailCommandReceiptRecord> {
        std::mem::take(&mut self.inline.tail_command_receipts)
    }

    pub fn take_command_side_effect_receipts(&mut self) -> Vec<CommandSideEffectReceipt> {
        std::mem::take(&mut self.inline.command_side_effect_receipts)
    }

    /// `CommandPackage::process_all` `0x0094C500`: walk a payload, dispatching each
    /// command and advancing by exactly what its handler returns.
    ///
    /// Padding between commands (`Obfuscation`) is `don-net`'s business and is expected to
    /// have been stripped; this takes a contiguous command list.
    pub fn process_all(
        &mut self,
        pkg: &mut Package,
        payload: &[u8],
        f: &mut dyn Fleet,
    ) -> Result<(), WireError> {
        let mut i = 0usize;
        while i < payload.len() {
            let l = wire_len(&payload[i..]).map_err(|e| match e {
                WireError::Truncated { need, .. } => WireError::Truncated { offset: i, need },
                WireError::BadLength { len, .. } => WireError::BadLength { offset: i, len },
                other => other,
            })?;
            if i + l > payload.len() {
                return Err(WireError::Truncated { offset: i, need: l });
            }
            self.process_one(pkg, &payload[i..i + l], f);
            i += l;
        }
        Ok(())
    }

    /// One `CommandPackage::process_<x>`.
    pub fn process_one(&mut self, pkg: &mut Package, cmd: &[u8], f: &mut dyn Fleet) {
        let Some(&op) = cmd.first() else { return };
        let Some(def) = OPCODES.get(op as usize) else {
            return;
        };
        self.stats.dispatched += 1;
        self.stats.by_opcode[op as usize] += 1;

        if op == 0 {
            self.process_group(pkg, cmd, f);
            return;
        }
        if InlineDef::find(op).is_some() {
            self.process_inline(pkg, cmd, f);
            self.stats.inline_state += 1;
            return;
        }
        if !def.is_group_action() {
            self.stats.inert += 1;
            return;
        }
        // Every group handler is guarded by `if (this->group >= 0)` [measured].
        if pkg.group < 0 {
            self.stats.no_group += 1;
            return;
        }
        let Some(name) = def.action else {
            self.stats.inert += 1;
            return;
        };
        let Some(ai) = GROUP_ACTIONS.iter().position(|a| a.name == name) else {
            self.stats.inert += 1;
            return;
        };
        self.stats.by_action[ai] += 1;
        if GROUP_ACTIONS[ai].port == Port::Todo {
            self.stats.unported += 1;
            return;
        }
        self.stats.acted += 1;
        self.dispatch_action(pkg.group, name, cmd, f);
    }

    /// Non-group `CommandPackage::process_*` handlers. Rows with an external receiver
    /// cross one validated atomic [`Fleet`] transaction rather than mutating a prefix.
    fn process_inline(&mut self, pkg: &Package, cmd: &[u8], f: &mut dyn Fleet) {
        match cmd[0] {
            34 => self.process_hotkey(pkg, cmd),
            37..=45 => self.process_diplomacy(cmd, f),
            46..=49 => self.process_direct_entity_command(cmd, f),
            50 => self.process_ping(pkg, cmd),
            51 => self.process_spline(pkg, cmd),
            // SpeedSetCommand: signed speed dword @+1. Presentation callbacks update
            // wall-clock pacing, but `TurnControl+0x30` is the only deterministic state.
            52 => {
                if let Some(speed) = i32_at(cmd, 1) {
                    self.set_speed(speed);
                }
            }
            // The command handlers cap ordinary speed-up at Fast (index 3), not the
            // Hyper Fast index accepted by TurnControl::speed_up's other caller.
            53 => {
                if self.inline.speed < 3 && self.speed_change_allowed() {
                    self.inline.speed = self.inline.speed.wrapping_add(1);
                }
            }
            54 => {
                if self.inline.speed != 0 && self.speed_change_allowed() {
                    self.inline.speed = self.inline.speed.wrapping_sub(1);
                }
            }
            // MPLogCommand toggles Game semaphore bit 0x20 and the restart delay at
            // Game+0x81C. Its logging call is presentation-only.
            55 => {
                if !self.inline.mp_log {
                    self.inline.mp_log = true;
                    self.inline.restart_delay = 0;
                } else {
                    self.inline.mp_log = false;
                    if self.inline.restart_delay == 0 {
                        self.inline.restart_delay = 2;
                    }
                }
            }
            // CheckRandomCommand is deliberately log-only in this retail build. It reads
            // the seed dword at +1 for SyncLogger output but performs no comparison/store.
            56 => {
                if let Some(seed) = i32_at(cmd, 1) {
                    self.inline
                        .command_side_effect_receipts
                        .push(CommandSideEffectReceipt::CheckRandom { seed: seed as u32 });
                }
            }
            // CheckSumsCommand logs all sixteen channel words, then stores the final
            // `total` word in CommandPackage::checksums[package.play].
            57 => {
                let (Ok(play), Some(total)) = (usize::try_from(pkg.play), i32_at(cmd, 61)) else {
                    return;
                };
                if let Some(checksum) = self.inline.checksums.get_mut(play) {
                    *checksum = total as u32;
                }
            }
            // NextCheckSumCommand's type byte is diagnostic-only. During an active
            // recheck retail logs the value but deliberately leaves the peer table alone.
            58 => {
                let (Ok(play), Some(value)) = (usize::try_from(pkg.play), i32_at(cmd, 2)) else {
                    return;
                };
                if self.inline.checksum_recheck == 0 {
                    if let Some(checksum) = self.inline.checksums.get_mut(play) {
                        *checksum = value as u32;
                    }
                }
            }
            59 => {
                if let Some(who) = i32_at(cmd, 1) {
                    self.process_cheat_view_all(who);
                }
            }
            60 => {
                if let Some(who) = i32_at(cmd, 1) {
                    self.process_cheat_techs(who, true);
                }
            }
            61 => {
                if let Some(who) = i32_at(cmd, 1) {
                    self.process_cheat_techs(who, false);
                }
            }
            // The three AI controls write a diagnostic log before this branch. Their only
            // simulation mutation is gated off in network play.
            62 => {
                if !self.inline.network {
                    self.inline.ai_speed = self.inline.ai_speed.wrapping_add(1).min(10);
                }
            }
            63 => {
                if !self.inline.network {
                    self.inline.ai_speed = 1;
                }
            }
            64 => {
                if !self.inline.network {
                    self.inline.ai_off = i32::from(self.inline.ai_off == 0);
                }
            }
            65 => {
                if let Some(who) = i32_at(cmd, 1) {
                    self.process_cheat_increase_buckets(who);
                }
            }
            66 => {
                if let Some(who) = i32_at(cmd, 1) {
                    self.process_cheat_zero_buckets(who);
                }
            }
            67 => self.process_cheat_init_unit(cmd, f),
            68 => self.process_chat(pkg, cmd),
            // ChatSetCommand replaces all eight recipient status words for the sender's
            // Player::who row. These values later gate chat and ping delivery.
            69 => {
                let Ok(play) = usize::try_from(pkg.play) else {
                    return;
                };
                let (Some(&who), Some(status)) = (
                    self.inline.player_who.get(play),
                    cmd.get(1..1 + NUM_NETWORK_PLAYERS),
                ) else {
                    return;
                };
                let Some(row) = self.inline.chat_status.get_mut(who as usize) else {
                    return;
                };
                for (dst, &src) in row.iter_mut().zip(status) {
                    *dst = src as u32;
                }
            }
            70 | 71 | 73 | 78 | 80 => self.process_tail_command(cmd, f),
            // CameraCommand logs the remote viewpoint and may update only the local
            // Console/Scene zoom and scroll. It has no headless simulation mutation.
            72 => {
                let (Some(&zoom), Some(x), Some(y)) = (cmd.get(1), i32_at(cmd, 2), i32_at(cmd, 6))
                else {
                    return;
                };
                let local_sender = pkg.play == self.inline.local_play;
                self.inline
                    .command_side_effect_receipts
                    .push(CommandSideEffectReceipt::Camera {
                        package_play: pkg.play,
                        zoom,
                        x,
                        y,
                        local_sender,
                    });
            }
            74 => self.process_turn_data(pkg, cmd),
            75 => self.process_rename_city(cmd, f),
            76 => {
                if let Some(&state) = cmd.get(1) {
                    self.process_pause(pkg.play, state, f);
                }
            }
            77 => self.process_cannon_time(pkg, cmd, f),
            79 => {
                let Ok(play) = usize::try_from(pkg.play) else {
                    return;
                };
                let Some(accum) = self.inline.player_speed.get_mut(play) else {
                    return;
                };
                let Some(delta) = cmd.get(1..1 + PLAYER_SPEED_FIELDS) else {
                    return;
                };
                for (total, &add) in accum.iter_mut().zip(delta) {
                    *total = total.wrapping_add(add as u32);
                }
            }
            // MarwanCommand only writes its start byte to the diagnostic log.
            81 => {
                if let Some(&start) = cmd.get(1) {
                    self.inline
                        .command_side_effect_receipts
                        .push(CommandSideEffectReceipt::Marwan { start });
                }
            }
            _ => unreachable!("inline command table and dispatcher disagree"),
        }
    }

    fn sender_who(&self, play: i32) -> Option<u8> {
        usize::try_from(play)
            .ok()
            .and_then(|play| self.inline.player_who.get(play).copied())
            .filter(|&who| (who as usize) < NUM_NETWORK_PLAYERS)
    }

    /// Shared symmetric chat-status gate in `process_ping` and `process_spline`.
    ///
    /// Retail accepts a recipient when sender->recipient is zero and
    /// recipient->sender is not two. Game semaphore bit `0x40` bypasses both tests.
    fn presentation_delivery_allowed(&self, sender: u8, recipient: usize) -> bool {
        self.inline
            .leader_valid
            .get(recipient)
            .copied()
            .unwrap_or(false)
            && (self.inline.chat_filter_bypass
                || (self.inline.chat_status[sender as usize][recipient] == 0
                    && self.inline.chat_status[recipient][sender as usize] != 2))
    }

    /// `CommandPackage::process_ping` `0x009453F0`.
    fn process_ping(&mut self, pkg: &Package, cmd: &[u8]) {
        let (Some(x), Some(y)) = (i32_at(cmd, 1), i32_at(cmd, 5)) else {
            return;
        };
        let sender_who = self.sender_who(pkg.play);
        let delivered_to = sender_who.map_or_else(Vec::new, |sender| {
            (0..NUM_NETWORK_PLAYERS)
                .filter(|&recipient| self.presentation_delivery_allowed(sender, recipient))
                .map(|recipient| recipient as u8)
                .collect()
        });
        self.inline
            .command_side_effect_receipts
            .push(CommandSideEffectReceipt::Ping {
                package_play: pkg.play,
                sender_who,
                x,
                y,
                delivered_to,
            });
    }

    /// `CommandPackage::process_spline` `0x00945140`.
    fn process_spline(&mut self, pkg: &Package, cmd: &[u8]) {
        let (Some(&spline_type), Some(&spline_flags), Some(&spline_cmd), Some(len)) =
            (cmd.get(1), cmd.get(2), cmd.get(3), u16_at(cmd, 4))
        else {
            return;
        };
        let Some(raw_points) = cmd.get(6..6 + len as usize * 8) else {
            return;
        };
        let points: Vec<_> = raw_points
            .chunks_exact(8)
            .map(|point| {
                (
                    i32::from_le_bytes(point[..4].try_into().unwrap()),
                    i32::from_le_bytes(point[4..].try_into().unwrap()),
                )
            })
            .collect();
        let sender_who = self.sender_who(pkg.play);
        let delivered_to = sender_who.map_or_else(Vec::new, |sender| {
            if !self.inline.leader_valid[sender as usize] {
                return Vec::new();
            }
            (0..NUM_NETWORK_PLAYERS)
                .filter(|&recipient| {
                    self.presentation_delivery_allowed(sender, recipient)
                        && self.inline.leader_play[recipient] == self.inline.display_play
                })
                .map(|recipient| recipient as u8)
                .collect()
        });
        self.inline
            .command_side_effect_receipts
            .push(CommandSideEffectReceipt::Spline {
                package_play: pkg.play,
                sender_who,
                spline_type,
                spline_flags,
                spline_cmd,
                points,
                delivered_to,
            });
    }

    /// `CommandPackage::process_chat` `0x009454F0`.
    ///
    /// Chat display, taunts, cross-play moderation, and counters are presentation-only.
    /// Preserve the raw recipient bits and all `len + 1` UTF-16 code units for the host.
    fn process_chat(&mut self, pkg: &Package, cmd: &[u8]) {
        let (Some(bits), Some(taunt), Some(taunt_num), Some(text_len)) = (
            i32_at(cmd, 1),
            i32_at(cmd, 5),
            i32_at(cmd, 9),
            i32_at(cmd, 13),
        ) else {
            return;
        };
        let Ok(text_len_usize) = usize::try_from(text_len) else {
            return;
        };
        let Some(raw_text) = cmd.get(17..17 + (text_len_usize + 1) * 2) else {
            return;
        };
        let utf16_with_nul = raw_text
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let sender_who = self.sender_who(pkg.play);
        self.inline
            .command_side_effect_receipts
            .push(CommandSideEffectReceipt::Chat {
                package_play: pkg.play,
                sender_who,
                bits: bits as u32,
                taunt,
                taunt_num,
                text_len: text_len as u32,
                utf16_with_nul,
            });
    }

    /// `Game::action_cheat_view_all` `0x00592CD0`, excluding UI invalidation and the
    /// network cheat-warning callback. The warning's accumulated telemetry byte remains.
    fn process_cheat_view_all(&mut self, who: i32) {
        if self.inline.reveal_map {
            self.inline.reveal_map = false;
            if self.inline.restart_delay == 0 {
                self.inline.restart_delay = 2;
            }
        } else {
            self.inline.reveal_map = true;
            self.inline.restart_delay = 0;
        }
        self.process_cheat_warning(who);
    }

    /// `Game::action_cheat_warning`'s simulation-visible telemetry mutation.
    fn process_cheat_warning(&mut self, who: i32) {
        let Ok(who) = usize::try_from(who) else {
            return;
        };
        let Some(accum) = self.inline.accum_cheated.get_mut(who) else {
            return;
        };
        *accum = accum.wrapping_add(1);
        if self.inline.network {
            self.inline
                .cheat_warning_receipts
                .push(CheatWarningReceipt {
                    who: who as i32,
                    sound_category: 99,
                });
        }
    }

    /// `Game::action_cheat_{give,zero}_techs` `0x00593180/0x00593120`.
    fn process_cheat_techs(&mut self, who: i32, give: bool) {
        let Ok(who) = usize::try_from(who) else {
            return;
        };
        let (Some(bits), Some(status)) = (
            self.inline.tech_bits.get_mut(who),
            self.inline.tech_status.get_mut(who),
        ) else {
            return;
        };
        for tech in 0..CHEAT_TECH_COUNT {
            let mask = 1 << (tech & 7);
            if give {
                bits[tech >> 3] |= mask;
                *status = 0;
            } else {
                bits[tech >> 3] &= !mask;
                if *status == 0 {
                    *status = 2;
                }
            }
        }
        self.process_cheat_warning(who as i32);
    }

    /// `Game::action_cheat_increase_buckets` `0x00592FF0`.
    fn process_cheat_increase_buckets(&mut self, who: i32) {
        let Ok(who) = usize::try_from(who) else {
            return;
        };
        let Some(buckets) = self.inline.resource_buckets_encoded.get_mut(who) else {
            return;
        };
        for encoded in buckets {
            *encoded = (*encoded ^ RESOURCE_BUCKET_XOR).wrapping_add(1000) ^ RESOURCE_BUCKET_XOR;
        }
        self.process_cheat_warning(who as i32);
    }

    /// `Game::action_cheat_zero_buckets` `0x00592E70`.
    ///
    /// Retail passes `(0, response_count - 1)` to the half-open `Random::get`. Thus a
    /// one-entry list consumes no draw, and the final entry of every longer list is
    /// unreachable. This oddity is intentional and observable in the sound RNG stream.
    fn process_cheat_zero_buckets(&mut self, who: i32) {
        let Ok(who) = usize::try_from(who) else {
            return;
        };
        let Some(buckets) = self.inline.resource_buckets_encoded.get_mut(who) else {
            return;
        };
        buckets.fill(RESOURCE_BUCKET_XOR);
        self.process_cheat_warning(who as i32);

        if !self.inline.network || self.inline.cheat_response_sound_refs.is_empty() {
            return;
        }
        let Ok(response_count) = i32::try_from(self.inline.cheat_response_sound_refs.len()) else {
            return;
        };
        let response_slot = self.inline.sound_random.get(0, response_count - 1);
        let Ok(response_slot_index) = usize::try_from(response_slot) else {
            return;
        };
        let Some(&sound_ref) = self
            .inline
            .cheat_response_sound_refs
            .get(response_slot_index)
        else {
            return;
        };
        if sound_ref < 0 || sound_ref >= self.inline.sound_ref_count {
            return;
        }
        self.inline
            .cheat_response_receipts
            .push(CheatResponseReceipt {
                who: who as u8,
                response_slot: response_slot as u32,
                sound_ref,
            });
    }

    /// `Game::action_cheat_init_unit` `0x00592D90` through a typed atomic world receipt.
    fn process_cheat_init_unit(&mut self, cmd: &[u8], f: &mut dyn Fleet) {
        let (Some(who), Some(type_index), Some(x), Some(y)) = (
            i32_at(cmd, 1),
            i32_at(cmd, 5),
            i32_at(cmd, 9),
            i32_at(cmd, 13),
        ) else {
            return;
        };
        let expected = CheatInitUnitRequest {
            who,
            type_index,
            x,
            y,
            valid_players: self.inline.player_valid,
        };
        let observed = f.apply_cheat_init_unit_transaction(expected);
        let valid = observed.validates(expected);
        self.inline
            .cheat_init_unit_receipts
            .push(CheatInitUnitReceiptRecord {
                expected,
                observed,
                valid,
            });

        // The negative-owner loop leaves its counter at eight and passes that exact value
        // to action_cheat_warning, irrespective of how many players were valid/spawned.
        self.process_cheat_warning(if who < 0 {
            CHEAT_INIT_PLAYER_SLOTS as i32
        } else {
            who
        });
    }

    /// `CommandPackage::process_turn_data` `0x00943D20`.
    fn process_turn_data(&mut self, pkg: &Package, cmd: &[u8]) {
        let (Some(ping), Some(average), Some(wait), Some(lag), Some(forced)) = (
            u16_at(cmd, 1),
            u16_at(cmd, 3),
            u16_at(cmd, 5),
            u16_at(cmd, 7),
            u16_at(cmd, 9),
        ) else {
            return;
        };
        let Ok(play) = usize::try_from(pkg.play) else {
            return;
        };
        if play >= NUM_NETWORK_PLAYERS {
            return;
        }

        if ping & 0x80 != 0 && pkg.play != self.inline.local_play && !self.inline.reveal_map {
            let who = self.inline.player_who[play] as i32;
            self.process_cheat_view_all(who);
        }

        self.inline.turn_data.flags |= 1 << play;
        self.inline.turn_data.last_forced_loads[play] = forced as u32;
        self.inline.turn_data.last_average_frame_times[play] = average as u32;
        self.inline.turn_data.last_ping_times[play] = ping as u32;
        self.inline.turn_data.last_wait_times[play] = wait as u32;
        self.inline.turn_data.last_lag_times[play] = lag as u32;
    }

    /// `CommandPackage::process_hotkey` `0x009474D0`.
    ///
    /// The command indexes the 162-entry `HotKeyGroups` array directly. `clear == 0`
    /// copies the current selection's recovered `GroupData` subset and clears its camera
    /// bookmark; nonzero clear empties the group and optionally installs raw x/y/zoom.
    fn process_hotkey(&mut self, pkg: &Package, cmd: &[u8]) {
        let (Some(group), Some(clear), Some(valid), Some(x), Some(y), Some(zoom)) = (
            i32_at(cmd, 1),
            i32_at(cmd, 5),
            i32_at(cmd, 9),
            i32_at(cmd, 13),
            i32_at(cmd, 17),
            i32_at(cmd, 21),
        ) else {
            return;
        };
        let Ok(slot) = usize::try_from(group) else {
            return;
        };
        if slot >= self.inline.hotkeys.len() {
            return;
        }
        if clear == 0 {
            let Some(source) = self.groups.get(pkg.group).cloned() else {
                return;
            };
            copy_hotkey_group(&mut self.inline.hotkeys[slot].group, &source, self.frame);
            self.inline.hotkeys[slot].camera = None;
            return;
        }
        let hotkey = &mut self.inline.hotkeys[slot];
        hotkey.group.num = 0;
        hotkey.camera = (valid != 0).then_some(HotKeyCamera {
            x_bits: x as u32,
            y_bits: y as u32,
            zoom,
        });
    }

    #[inline]
    fn speed_change_allowed(&self) -> bool {
        !self.inline.network || !self.inline.speed_locked
    }

    fn set_speed(&mut self, speed: i32) {
        if self.inline.speed != speed && self.speed_change_allowed() {
            self.inline.speed = speed;
        }
    }

    /// Transactional `TurnControl::process_pause` `0x00956990` /
    /// `TurnControl::toggle_pause` `0x00957AB0` receiver.
    fn process_pause(&mut self, play: i32, requested: u8, f: &mut dyn Fleet) {
        let request = PauseTransactionRequest {
            play,
            requested,
            local_play: self.inline.local_play,
            state: PauseState {
                paused: self.inline.paused,
                pause_delay: self.inline.pause_delay,
                network: self.inline.network,
                immediate_process: self.inline.immediate_process,
                pause_override: self.inline.pause_override,
                pauses: self.inline.pauses,
                restart_delay: self.inline.restart_delay,
                restart_gate_2: self.inline.restart_gate_2,
                restart_gate_4: self.inline.restart_gate_4,
                chat_filter_bypass: self.inline.chat_filter_bypass,
            },
        };
        let receipt = f.apply_pause_transaction(request.clone());
        if receipt.status != PauseTransactionStatus::Applied || !receipt.validates(&request) {
            return;
        }
        let Some(plan) = receipt.plan else { return };
        self.inline.paused = plan.state.paused;
        self.inline.pause_delay = plan.state.pause_delay;
        self.inline.network = plan.state.network;
        self.inline.immediate_process = plan.state.immediate_process;
        self.inline.pause_override = plan.state.pause_override;
        self.inline.pauses = plan.state.pauses;
        self.inline.restart_delay = plan.state.restart_delay;
        self.inline.restart_gate_2 = plan.state.restart_gate_2;
        self.inline.restart_gate_4 = plan.state.restart_gate_4;
        self.inline.chat_filter_bypass = plan.state.chat_filter_bypass;
    }

    /// Opcode 75 through the complete atomic object/hot-key/presentation transaction.
    fn process_rename_city(&mut self, cmd: &[u8], f: &mut dyn Fleet) {
        let Ok(request) = object_command_plans::decode_rename_city(cmd) else {
            return;
        };
        let receipt = f.apply_rename_city_transaction(request.clone(), self.frame);
        if receipt.status == RenameCityTransactionStatus::Applied
            && (!receipt.validates(&request)
                || !receipt
                    .facts
                    .as_ref()
                    .is_some_and(|facts| facts.frame == self.frame))
        {
            return;
        }
    }

    /// Opcode 77 through the complete atomic TurnControl/product transaction. The bridge
    /// mirrors the speed column it already owns only after a validated host commit.
    fn process_cannon_time(&mut self, pkg: &Package, cmd: &[u8], f: &mut dyn Fleet) {
        let Some(request) = late_command_plans::decode_cannon_time(pkg.play, cmd) else {
            return;
        };
        let Some(facts) = f.cannon_time_facts(request) else {
            return;
        };
        if facts.package_player_who != self.sender_who(pkg.play)
            || facts.frame != self.frame
            || facts.state.current_speed != self.inline.speed
        {
            return;
        }
        let receipt = f.apply_cannon_time_transaction(request, facts);
        if receipt.status != LateCommandPlanStatus::Planned
            || receipt.facts.as_ref() != Some(&facts)
            || !receipt.validates(&request)
        {
            return;
        }
        if let Some(plan) = receipt.plan {
            self.inline.speed = plan.state.current_speed;
        }
    }

    /// Opcodes 37..45 through one host-owned diplomacy image and atomic commit. The
    /// receipt validator rejects every planner boundary, so incomplete declaration,
    /// acceptance, and hostile-rejection branches cannot partially mutate state here.
    fn process_diplomacy(&mut self, cmd: &[u8], f: &mut dyn Fleet) {
        let Some(before) = f.diplomacy_command_state() else {
            return;
        };
        if before.frame != self.frame {
            return;
        }
        let request = DiplomacyCommandRequest {
            before,
            wire: cmd.to_vec(),
        };
        let receipt = f.apply_diplomacy_command_transaction(request.clone());
        if !receipt.validates(&request) {
            return;
        }
    }

    /// Rows 46 through 49 through the frozen direct market/entity transaction boundary.
    ///
    /// Market rows carry a complete deterministic economy tail.  Addressed unqueue and
    /// come-out rows use the same exact decoder and callback, but their receipt protocol
    /// can report completion only for the inactive/stale no-op arm; a reached action is
    /// retained as an open tail and therefore remains closure-red.
    fn process_direct_entity_command(&mut self, cmd: &[u8], f: &mut dyn Fleet) {
        let expected = match cmd.first().copied() {
            Some(46 | 47) => {
                let Some(request) =
                    direct_entity_command_integration::plans::decode_market_command(cmd)
                else {
                    return;
                };
                DirectEntityFleetRequest::Market {
                    request,
                    frame: self.frame,
                }
            }
            Some(48 | 49) => {
                let Some(request) =
                    direct_entity_command_integration::plans::decode_direct_entity_command(cmd)
                else {
                    return;
                };
                DirectEntityFleetRequest::Entity {
                    request,
                    frame: self.frame,
                }
            }
            _ => return,
        };
        let observed = f.apply_direct_entity_command_transaction(expected);
        let valid = observed.validates(expected);
        self.inline
            .direct_entity_receipts
            .push(DirectEntityReceiptRecord {
                expected,
                observed,
                valid,
            });
    }

    /// Remaining non-group rows through a branch-complete, fail-closed transaction.
    /// Applied receipts are possible only for a recomputable no-tail branch; every
    /// lifecycle/cascade/parser/drop boundary remains unavailable and closure-red.
    fn process_tail_command(&mut self, cmd: &[u8], f: &mut dyn Fleet) {
        let Ok(expected) = tail_command_transactions::decode_tail_command(cmd) else {
            return;
        };
        let Some(facts) = f.tail_command_facts(&expected) else {
            return;
        };
        let observed = f.apply_tail_command_transaction(expected.clone(), facts.clone());
        let valid = observed.validates_for(&expected, &facts);
        self.inline
            .tail_command_receipts
            .push(TailCommandReceiptRecord {
                expected,
                facts,
                observed,
                valid,
            });
    }

    /// `CommandPackage::process_group` `0x0094A0C0`, opcode 0.
    ///
    /// Wire: `u8 op | u8 num | i8 who | i16 list[num]`, `3 + 2*num` bytes.
    ///
    /// `num == 0` means "re-select what this player had", replayed from the `(o, uid)`
    /// cache so a recycled object index cannot be silently re-selected [structure].
    fn process_group(&mut self, pkg: &mut Package, cmd: &[u8], f: &mut dyn Fleet) {
        let (Some(&num), Some(who)) = (cmd.get(1), i8_at(cmd, 2)) else {
            return;
        };
        if who < 0 || who as usize >= NUM_OWNER_SLOTS {
            pkg.group = -1;
            return;
        }
        let who = who as u8;
        let mut g = GroupData::default();
        let mut chosen: Vec<(i16, u16)> = Vec::new();

        if num == 0 {
            let cache = std::mem::take(&mut self.last_selection[who as usize]);
            for &(o, uid) in &cache {
                if f.alive(who, o) && f.uid(who, o) == uid {
                    let added = g.add(o, who, f.is_building(who, o), f.role(who, o), self.frame);
                    if added {
                        chosen.push((o, uid));
                    }
                }
            }
            self.last_selection[who as usize] = cache;
        } else {
            for i in 0..num as usize {
                let Some(o) = i16_at(cmd, 3 + 2 * i) else {
                    break;
                };
                if o < 0 || !f.alive(who, o) {
                    continue;
                }
                if g.add(o, who, f.is_building(who, o), f.role(who, o), self.frame) {
                    chosen.push((o, f.uid(who, o)));
                }
            }
            self.last_selection[who as usize] = chosen.clone();
        }

        if chosen.is_empty() {
            pkg.group = -1;
            return;
        }
        pkg.group = self.groups.push_group(who, &g, true, self.frame, f);
        self.stats.selections += 1;
    }

    fn dispatch_action(&mut self, slot: i32, name: &str, cmd: &[u8], f: &mut dyn Fleet) {
        let mut act = Action {
            groups: &mut self.groups,
            slot,
            stats: &mut self.stats,
            frame: self.frame,
        };
        act.run(name, cmd, f);
    }
}

// ---------------------------------------------------------------------------
// Group::action_*
// ---------------------------------------------------------------------------

/// One `Group::action_*` invocation, holding the receiver and the counters.
struct Action<'a> {
    groups: &'a mut Groups,
    slot: i32,
    stats: &'a mut BridgeStats,
    frame: i32,
}

impl Action<'_> {
    /// The member list of the receiver, as `(who, o)` pairs.
    fn members(&self) -> (u8, Vec<i16>) {
        match self.groups.get(self.slot) {
            None => (0, Vec::new()),
            Some(g) => {
                let n = g.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
                (g.who, g.list[..n].to_vec())
            }
        }
    }

    /// `Unit::add_<k>_order`'s `QueuePos` handling, per unit.
    ///
    /// `QUEUE_NEW` clears the list first; `QUEUE_LAST` appends. `QUEUE_FIRST` never
    /// reaches here — [`Self::with_queue_first`] converts it at the group layer, exactly
    /// as retail does.
    fn install(&mut self, who: u8, o: i16, order: Order, q: QueuePos, f: &mut dyn Fleet) {
        self.install_rec(who, o, OrderRec::from(order), q, f);
    }

    fn install_rec(&mut self, who: u8, o: i16, order: OrderRec, q: QueuePos, f: &mut dyn Fleet) {
        let Some(was_empty) = f.orders(who, o).map(OrderQueue::is_empty) else {
            return;
        };
        let kind = order.kind;
        if !f.install_order_rec(who, o, order, q) {
            return;
        }
        if q == QueuePos::New && !was_empty {
            self.stats.orders_cleared += 1;
        }
        self.stats.orders_installed += 1;
        self.stats.by_order[kind.index()] += 1;
    }

    /// `Group::set_up_insert` `0x0070E520` / `action_halt(0)` / re-enter with `QUEUE_NEW`
    /// / `Group::finish_insert` `0x0070E620`.
    ///
    /// Returns `true` when it handled the call, in which case the caller must return.
    fn with_queue_first(
        &mut self,
        q: QueuePos,
        f: &mut dyn Fleet,
        body: &mut dyn FnMut(&mut Action<'_>, QueuePos, &mut dyn Fleet),
    ) -> bool {
        if q != QueuePos::First {
            return false;
        }
        let (who, list) = self.members();
        // set_up_insert: stash every member's current order list.
        let saved: Vec<(i16, Vec<OrderRec>)> = list
            .iter()
            .filter_map(|&o| f.orders(who, o).map(|l| (o, l.iter().cloned().collect())))
            .collect();
        if !self.action_halt(0, f) {
            return true;
        }
        {
            let mut inner = Action {
                groups: self.groups,
                slot: self.slot,
                stats: self.stats,
                frame: self.frame,
            };
            body(&mut inner, QueuePos::New, f);
        }
        // finish_insert: replay the stash behind the new head.
        for (o, orders) in saved {
            if let Some(l) = f.orders_mut(who, o) {
                for ord in orders {
                    let kind = ord.kind;
                    l.push_back(ord);
                    self.stats.orders_installed += 1;
                    self.stats.by_order[kind.index()] += 1;
                }
            }
        }
        true
    }

    fn run(&mut self, name: &str, cmd: &[u8], f: &mut dyn Fleet) {
        match name {
            "begin" => self.action_begin(),
            "move_to" => {
                // MoveToCommand: to_x@1 to_y@5 set_angle@9 angle@13 orders@17 queued@18
                // form@19 width@20 disembark@21. process_move_to passes them to
                // action_move_to(to_x, to_y, queued, set_angle, angle, orders, 1, form,
                // width, disembark) [structure, 0x009497C0].
                let (Some(x), Some(y)) = (i32_at(cmd, 1), i32_at(cmd, 5)) else {
                    return;
                };
                let set_angle = i32_at(cmd, 9).unwrap_or(0) != 0;
                let angle = i32_at(cmd, 13).unwrap_or(0);
                let orders = i8_at(cmd, 17).unwrap_or(0) as i64;
                let q = QueuePos::from_i64(i8_at(cmd, 18).unwrap_or(0) as i64);
                let form = i8_at(cmd, 19).unwrap_or(-1) as i32;
                let width = i8_at(cmd, 20).unwrap_or(-1) as i32;
                let disembark = i8_at(cmd, 21).unwrap_or(0) != 0;
                self.action_move_near(
                    x, y, 0, q, set_angle, angle, orders, form, width, disembark, f,
                );
            }
            "move_near" => {
                // MoveNearCommand adds tolerance@9 and shifts the tail by four
                // [structure, 0x009495C0].
                let (Some(x), Some(y), Some(tol)) =
                    (i32_at(cmd, 1), i32_at(cmd, 5), i32_at(cmd, 9))
                else {
                    return;
                };
                let set_angle = i32_at(cmd, 13).unwrap_or(0) != 0;
                let angle = i32_at(cmd, 17).unwrap_or(0);
                let orders = i8_at(cmd, 21).unwrap_or(0) as i64;
                let q = QueuePos::from_i64(i8_at(cmd, 22).unwrap_or(0) as i64);
                let form = i8_at(cmd, 23).unwrap_or(-1) as i32;
                let width = i8_at(cmd, 24).unwrap_or(-1) as i32;
                let disembark = i8_at(cmd, 25).unwrap_or(0) != 0;
                self.action_move_near(
                    x, y, tol, q, set_angle, angle, orders, form, width, disembark, f,
                );
            }
            "attack" => {
                // AttackCommand: ox@1 whom@5 ignore@9 queued@13; the handler calls
                // action_attack(ox, whom, 1, queued, ignore) [structure, 0x00949C30].
                let (Some(ox), Some(whom)) = (i32_at(cmd, 1), i32_at(cmd, 5)) else {
                    return;
                };
                let q = QueuePos::from_i64(i32_at(cmd, 13).unwrap_or(0) as i64);
                self.action_attack(ox, whom, q, f);
            }
            "attack_ground" => {
                let (Some(x), Some(y)) = (i32_at(cmd, 1), i32_at(cmd, 5)) else {
                    return;
                };
                let q = QueuePos::from_i64(i8_at(cmd, 9).unwrap_or(0) as i64);
                self.action_ground(OrderIndex::AttackGround, x, y, q, f);
            }
            "halt" => {
                let _ = self.action_halt(0, f);
            }
            "transport" | "city_gather" | "gather_point" | "eject_all" | "alarm" => {
                if let Some(command) = group_action_frontier::decode_open_group_action(cmd) {
                    self.action_open_frontier(command);
                }
            }
            "stop_spell" => {
                if group_action_frontier::decode_stop_spell(cmd) {
                    self.action_stop_spell(f);
                }
            }
            "set_transport" => {
                if let Some(flag) = i32_at(cmd, 1) {
                    self.action_set_transport(flag, f);
                }
            }
            "stance" => {
                let s = i32_at(cmd, 1).unwrap_or(0);
                self.action_stance(s, f);
            }
            "form" => {
                // FormCommand (13 B): form:i32@1, rotate:i32@5, queued:i32@9.
                // `process_form` forwards all three unchanged and supplies the package's
                // group plus a zero recursion guard [measured, 0x00949D90].
                let (Some(form), Some(rotate), Some(queued)) =
                    (i32_at(cmd, 1), i32_at(cmd, 5), i32_at(cmd, 9))
                else {
                    return;
                };
                self.action_form(form, rotate, queued, f);
            }
            "follow" => {
                if let Some(command) = decode_follow(cmd) {
                    self.action_follow(command, f);
                }
            }
            "guard" => {
                let (Some(ox), Some(whom)) = (i32_at(cmd, 1), i32_at(cmd, 5)) else {
                    return;
                };
                let q = QueuePos::from_i64(i32_at(cmd, 9).unwrap_or(0) as i64);
                self.action_target(OrderIndex::Guard, ox, whom, q, f);
            }
            "garrison" => {
                let (Some(ox), Some(whom)) = (i32_at(cmd, 1), i32_at(cmd, 5)) else {
                    return;
                };
                let q = QueuePos::from_i64(i32_at(cmd, 9).unwrap_or(0) as i64);
                self.action_target(OrderIndex::Garrison, ox, whom, q, f);
            }
            "repair" => {
                let (Some(ox), Some(whom)) = (i32_at(cmd, 1), i32_at(cmd, 5)) else {
                    return;
                };
                let q = QueuePos::from_i64(i32_at(cmd, 9).unwrap_or(0) as i64);
                self.action_target(OrderIndex::Repair, ox, whom, q, f);
            }
            "gather" => {
                // GatherCommand: ox@1 queued@5. The gatherable belongs to the gaia owner,
                // so `whom` is not on the wire.
                let Some(ox) = i32_at(cmd, 1) else { return };
                let q = QueuePos::from_i64(i32_at(cmd, 5).unwrap_or(0) as i64);
                self.action_target(OrderIndex::Gather, ox, -1, q, f);
            }
            "board_ship" => {
                let Some(ox) = i32_at(cmd, 1) else { return };
                let q = QueuePos::from_i64(i32_at(cmd, 5).unwrap_or(0) as i64);
                self.action_target(OrderIndex::BoardShip, ox, -1, q, f);
            }
            "trade" => {
                let (Some(ox), Some(whom)) = (i32_at(cmd, 1), i32_at(cmd, 5)) else {
                    return;
                };
                let q = QueuePos::from_i64(i32_at(cmd, 17).unwrap_or(0) as i64);
                self.action_target(OrderIndex::TradeRoute, ox, whom, q, f);
            }
            "patrol" => {
                // PatrolCommand: to_x@1 to_y@5 queued@9. `Unit::add_patrol_order`
                // allocates GROUP_PATROL, never PATROL [measured].
                let (Some(x), Some(y)) = (i32_at(cmd, 1), i32_at(cmd, 5)) else {
                    return;
                };
                let q = QueuePos::from_i64(i8_at(cmd, 9).unwrap_or(0) as i64);
                self.action_patrol(x, y, q, false, f);
            }
            "launch_patrol" => {
                let (Some(x), Some(y)) = (i32_at(cmd, 1), i32_at(cmd, 5)) else {
                    return;
                };
                let q = QueuePos::from_i64(i32_at(cmd, 9).unwrap_or(0) as i64);
                self.action_patrol(x, y, q, true, f);
            }
            "scramble" => {
                let (who, list) = self.members();
                for o in list {
                    if f.alive(who, o) && f.can_move(who, o) && f.is_plane(who, o) {
                        let (x, y) = f.pos(who, o);
                        self.install_air_patrol_member(who, o, x, y, QueuePos::New, f);
                    }
                }
            }
            "disband" => {
                let all = i32_at(cmd, 1).unwrap_or(0);
                let _ = self.action_disband(all != 0, f);
            }
            "unitmask" => {
                if let (Some(mask), Some(set)) = (i32_at(cmd, 1), i32_at(cmd, 5)) {
                    self.action_unitmask(mask as u32, set, f);
                }
            }
            "buildmask" => {
                if let (Some(mask), Some(set)) = (i32_at(cmd, 1), i32_at(cmd, 5)) {
                    self.action_buildmask(mask as u16, set, f);
                }
            }
            _ => {
                self.stats.unported += 1;
            }
        }
    }

    /// Deterministic prefix for five world-owning group actions.  The typed plan exposes
    /// the exact downstream owner and keeps these rows `StateWired` until that tail lands.
    fn action_open_frontier(&mut self, command: OpenGroupActionCommand) {
        let Some(group) = self.groups.get(self.slot).cloned() else {
            return;
        };
        let plan = group_action_frontier::plan_open_group_action(&group, command);
        if let Some(group) = self.groups.get_mut(self.slot) {
            *group = plan.group;
        }
    }

    fn action_stop_spell(&mut self, f: &mut dyn Fleet) {
        let Some(group) = self.groups.get(self.slot).cloned() else {
            return;
        };
        let request = StopSpellRequest {
            group,
            frame: self.frame,
        };
        let receipt = f.apply_stop_spell_transaction(request.clone());
        if receipt.status != GroupActionTransactionStatus::Applied || !receipt.validates(&request) {
            return;
        }
        let Some(plan) = receipt.plan else { return };
        self.stats.orders_cleared += plan
            .steps
            .iter()
            .filter(|step| matches!(step, StopSpellStep::CloseOrders { .. }))
            .count() as u64;
        if let Some(group) = self.groups.get_mut(self.slot) {
            *group = plan.group;
        }
    }

    fn action_follow(&mut self, command: follow_action::FollowCommand, f: &mut dyn Fleet) {
        let Some(group) = self.groups.get(self.slot).cloned() else {
            return;
        };
        let request = FollowRequest { group, command };
        let receipt = f.apply_follow_transaction(request.clone());
        if receipt.status != FollowTransactionStatus::Applied || !receipt.validates(&request) {
            return;
        }
        let Some(plan) = receipt.plan else { return };
        let installed = plan
            .effects
            .iter()
            .filter(|effect| matches!(effect, FollowEffect::AddFollowOrder { .. }))
            .count() as u64;
        self.stats.orders_installed += installed;
        self.stats.by_order[OrderIndex::Follow.index()] += installed;
        if let Some(group) = self.groups.get_mut(self.slot) {
            *group = plan.group;
        }
    }

    /// `Group::action_begin` `0x00714100`: one store, `GroupData::disband = 0`.
    fn action_begin(&mut self) {
        if let Some(group) = self.groups.get_mut(self.slot) {
            group.disband = 0;
        }
    }

    /// Transactional `Group::action_set_transport` `0x007024B0` receiver.
    ///
    /// The host owns scenario ignore-orders, the exact leader `0x100/0x200/0x400`
    /// transport ladder, capability facts, and all unit writes. The bridge accepts only
    /// a recomputable complete plan, then commits its echoed group state.
    fn action_set_transport(&mut self, flag: i32, f: &mut dyn Fleet) {
        let Some(group) = self.groups.get(self.slot).cloned() else {
            return;
        };
        let request = GroupSetTransportRequest { group, flag };
        let receipt = f.apply_group_set_transport_transaction(request.clone());
        if receipt.status != GroupStateTransactionStatus::Applied || !receipt.validates(&request) {
            return;
        }
        let Some(plan) = receipt.plan else { return };
        if let Some(group) = self.groups.get_mut(self.slot) {
            *group = plan.group;
        }
    }

    /// `Group::action_unitmask` `0x006FCB90`.
    ///
    /// The second wire dword is unread. The typed planner preserves the loop-carried
    /// set/clear decision and the full mask-`0x100` action/path retirement tail.
    fn action_unitmask(&mut self, mask: u32, set: i32, f: &mut dyn Fleet) {
        let Some(group) = self.groups.get(self.slot).cloned() else {
            return;
        };
        let n = group.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
        let nonempty_before: Vec<i16> = group.list[..n]
            .iter()
            .copied()
            .filter(|&o| {
                f.orders(group.who, o)
                    .is_some_and(|orders| !orders.is_empty())
            })
            .collect();
        let request = GroupUnitMaskRequest { group, mask, set };
        let receipt = f.apply_group_unitmask_transaction(request.clone());
        if receipt.status != GroupStateTransactionStatus::Applied || !receipt.validates(&request) {
            return;
        }
        let Some(plan) = receipt.plan else { return };
        self.stats.orders_cleared += plan
            .steps
            .iter()
            .filter(|step| {
                matches!(
                    step,
                    UnitMaskStep::CloseOrders { o, .. } if nonempty_before.contains(o)
                )
            })
            .count() as u64;
        if let Some(group) = self.groups.get_mut(self.slot) {
            *group = plan.group;
        }
    }

    /// `Group::action_buildmask` `0x006FC9A0`.
    ///
    /// The second dword is unread. `WallData::valid_buildmask` facts, the loop-carried
    /// toggle, and local feedback are owned by the atomic host receipt.
    fn action_buildmask(&mut self, mask: u16, set: i32, f: &mut dyn Fleet) {
        let Some(group) = self.groups.get(self.slot).cloned() else {
            return;
        };
        let request = GroupBuildMaskRequest { group, mask, set };
        let receipt = f.apply_group_buildmask_transaction(request.clone());
        if receipt.status != GroupStateTransactionStatus::Applied || !receipt.validates(&request) {
            return;
        }
        let Some(plan) = receipt.plan else { return };
        if let Some(group) = self.groups.get_mut(self.slot) {
            *group = plan.group;
        }
    }

    /// `GroupData::is_on_map` `0x0070C450`, the first guard in `Group::action_form`.
    fn group_is_on_map(&self, f: &dyn Fleet) -> bool {
        let Some(g) = self.groups.get(self.slot) else {
            return false;
        };
        if g.num == 0 {
            return false;
        }
        if g.buildings != 0 {
            return true;
        }
        let n = g.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
        g.list[..n]
            .iter()
            .copied()
            .any(|o| f.alive(g.who, o) && f.is_captain(g.who, o) && f.is_on_map(g.who, o))
    }

    /// The `Group::normalize` virtual call at `0x0070724D`, before formation leader
    /// selection. The object-side predicates are all explicit [`Fleet`] hosts.
    fn normalize_for_action(&mut self, f: &dyn Fleet) {
        let Some(g) = self.groups.get(self.slot) else {
            return;
        };
        let who = g.who;
        let group_id = g.id;
        let priority = g.priority;
        let n = g.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
        let states: Vec<(i16, MemberState)> = g.list[..n]
            .iter()
            .copied()
            .map(|o| {
                let state = if !f.alive(who, o) {
                    MemberState::Dead
                } else if f.leaves_groups(who, o) {
                    MemberState::LeavesGroups
                } else if priority == 0
                    && group_id >= 0
                    && (!f.is_unit(who, o) || f.group_of(who, o) != group_id as i16)
                {
                    MemberState::NotOurUnit
                } else {
                    MemberState::Keep
                };
                (o, state)
            })
            .collect();
        if let Some(g) = self.groups.get_mut(self.slot) {
            g.normalize(&|o| {
                states
                    .iter()
                    .find_map(|&(member, state)| (member == o).then_some(state))
                    .unwrap_or(MemberState::Dead)
            });
        }
    }

    /// `GroupData::find_leader` `0x0070CCB0`: prefer the lowest `FormCatIndex`, first
    /// requiring an on-map captain and then retrying without the on-map predicate.
    fn form_leader(&self, f: &dyn Fleet) -> Option<i16> {
        let g = self.groups.get(self.slot)?;
        let n = g.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
        for require_on_map in [true, false] {
            let mut leader = None;
            let mut best_category = 18;
            for &o in &g.list[..n] {
                if !f.alive(g.who, o)
                    || !f.is_captain(g.who, o)
                    || (require_on_map && !f.is_on_map(g.who, o))
                {
                    continue;
                }
                let category = f.form_category(g.who, o);
                if leader.is_none() || category < best_category {
                    leader = Some(o);
                    best_category = category;
                }
            }
            if leader.is_some() {
                return leader;
            }
        }
        None
    }

    /// `GroupData::get_form_option` `0x0070BEB0`. Multi-member groups use the most
    /// frequent form in `0..5`, retaining the lower form on ties; a singular group uses
    /// the selected leader's signed `UnitData::form` byte directly.
    fn form_option(&self, leader: i16, f: &dyn Fleet) -> i32 {
        let Some(g) = self.groups.get(self.slot) else {
            return 0;
        };
        let n = g.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
        let live = g.list[..n].iter().filter(|&&o| f.alive(g.who, o)).count();
        if live == 0 {
            return 0;
        }
        if live == 1 {
            return if f.alive(g.who, leader) && f.is_unit(g.who, leader) {
                f.form(g.who, leader) as i32
            } else {
                0
            };
        }
        let mut counts = [0i32; 6];
        for &o in &g.list[..n] {
            if !f.is_unit(g.who, o) {
                continue;
            }
            let form = f.form(g.who, o) as i32;
            if (0..6).contains(&form) {
                counts[form as usize] += 1;
            }
        }
        let mut best = 0usize;
        for candidate in 1..5 {
            if counts[candidate] > counts[best] {
                best = candidate;
            }
        }
        best as i32
    }

    fn write_member_forms(&self, form: i32, f: &mut dyn Fleet) {
        let (who, list) = self.members();
        for o in list {
            if f.alive(who, o) && f.is_unit(who, o) && !f.is_building(who, o) {
                f.set_form(who, o, form as i8);
            }
        }
    }

    /// `GroupData::get_loc_to` `0x0070C5D0` for the unit-group path used by
    /// `action_form`: the leader's final queued destination, except that a nearby saved
    /// group origin (strictly less than 385 Coord units away) wins.
    fn form_destination(&self, leader: i16, f: &dyn Fleet) -> Option<(i32, i32)> {
        let g = self.groups.get(self.slot)?;
        let mut destination = f.final_pos(g.who, leader);
        if g.ox >= 0
            && g.oy >= 0
            && vector_dist(
                g.ox.wrapping_sub(destination.0),
                g.oy.wrapping_sub(destination.1),
            ) < 0x181
        {
            destination = (g.ox, g.oy);
        }
        Some(destination)
    }

    /// `Group::action_form` `0x00707220`, recovered through its state write and the
    /// exact parameters of its `action_halt` / `action_move_to` delegates.
    ///
    /// `action_move_to` now consumes exact per-member destinations for the five forms this
    /// hotkey can select when the fleet supplies complete formation type facts. The row
    /// remains [`Port::Orders`] because the retail-only Square/Wedge/Mob and subordinate
    /// sorting paths are still unavailable.
    fn action_form(&mut self, form: i32, rotate: i32, queued: i32, f: &mut dyn Fleet) {
        if !self.group_is_on_map(f) {
            return;
        }
        self.normalize_for_action(f);
        let Some(group) = self.groups.get(self.slot) else {
            return;
        };
        if group.buildings != 0 || group.num < 1 {
            return;
        }
        let who = group.who;
        let Some(leader) = self.form_leader(f) else {
            return;
        };
        if f.is_building(who, leader) {
            return;
        }

        let current = self.form_option(leader, f);
        if form == -2 {
            // This write precedes `get_unit()` and therefore survives even if that lookup
            // fails in retail. The earlier leader guard makes the lookup succeed here.
            if let Some(g) = self.groups.get_mut(self.slot) {
                g.form = -2;
            }
        }
        let resolved = resolve_form(form, current, f.form(who, leader) as i32);
        if matches!(resolved, 6 | 7 | 9) {
            // Square depends on the unrecovered `FormData::space[4][18]` table, Wedge
            // consumes a caller stack cell which is uninitialized in the shipped build,
            // and Mob needs its evolving binary-angle ring. Refuse the whole command so
            // neither UnitData::form nor the order queues are partially changed.
            return;
        }
        let (_, members) = self.members();
        if members.iter().copied().any(|o| {
            f.alive(who, o) && f.can_move(who, o) && !f.can_install_order(who, o, QueuePos::Last)
        }) {
            return;
        }

        // Unlike the other queue-first actions, FORM runs this insert dance for both
        // QUEUE_FIRST and QUEUE_NEW. `set_up_insert` copies only the leader's GROUP-flagged
        // orders; a non-empty copy suppresses the recursive move and is replayed after the
        // form write. We retain each member's corresponding group nodes so the flattened
        // queues preserve their already-materialized per-member destinations.
        if queued == QueuePos::First as i32 || queued == QueuePos::New as i32 {
            let (who, members) = self.members();
            let leader_has_group_order = f
                .orders(who, leader)
                .is_some_and(|orders| orders.iter().any(OrderRec::is_group));
            let saved: Vec<(i16, Vec<OrderRec>)> = members
                .iter()
                .copied()
                .map(|o| {
                    let orders = f
                        .orders(who, o)
                        .map(|queue| {
                            queue
                                .iter()
                                .filter(|order| order.is_group())
                                .cloned()
                                .collect()
                        })
                        .unwrap_or_default();
                    (o, orders)
                })
                .collect();
            if !self.action_halt(0, f) {
                return;
            }
            self.write_member_forms(resolved, f);
            if leader_has_group_order {
                for (o, orders) in saved {
                    let Some(queue) = f.orders_mut(who, o) else {
                        continue;
                    };
                    for order in orders {
                        let kind = order.kind;
                        queue.push_back(order);
                        self.stats.orders_installed += 1;
                        self.stats.by_order[kind.index()] += 1;
                    }
                }
            } else if let Some((x, y)) = self.form_destination(leader, f) {
                // Recursive `action_form(..., QUEUE_NEW, ..., guard=1)` always delegates
                // as `action_move_to(x, y, QUEUE_LAST, rotate!=0, angle, MOVE_TO, ...)`.
                let angle = if rotate == 0 {
                    0
                } else {
                    let g = self
                        .groups
                        .get(self.slot)
                        .expect("formation group vanished");
                    let base = if (x, y) == (g.ox, g.oy) {
                        g.o_angle
                    } else {
                        f.angle(who, leader)
                    };
                    base.wrapping_add(rotate)
                };
                self.action_move_near(
                    x,
                    y,
                    0,
                    QueuePos::Last,
                    rotate != 0,
                    angle,
                    1,
                    -1,
                    -1,
                    false,
                    f,
                );
            }
            return;
        }

        self.write_member_forms(resolved, f);
        if queued != QueuePos::Last as i32 && queued != QueuePos::New as i32 {
            return;
        }
        if let Some((x, y)) = self.form_destination(leader, f) {
            let angle = if rotate == 0 {
                0
            } else {
                let g = self
                    .groups
                    .get(self.slot)
                    .expect("formation group vanished");
                let base = if (x, y) == (g.ox, g.oy) {
                    g.o_angle
                } else {
                    f.angle(g.who, leader)
                };
                base.wrapping_add(rotate)
            };
            self.action_move_near(
                x,
                y,
                0,
                QueuePos::Last,
                rotate != 0,
                angle,
                1,
                -1,
                -1,
                false,
                f,
            );
        }
    }

    /// `Group::action_move_near` `0x00704990` (9,205 B, 23 call sites), including the
    /// recovered captain-only formation path used by all five FORM-hotkey shapes.
    ///
    /// The `orders` argument is an `OrderIndex` and is threaded all the way to
    /// `Unit::add_move_facing_order` `0x005E55C0`, whose selection is [measured]:
    ///
    /// ```text
    /// if (orders == ATTACK_TO && unittype.flags[+0x2C8] & 0x10) {
    ///     unit.flags |= 0x04000000;  orders = EXPLORE_TO;
    /// }
    /// switch (orders) { 2 -> ATTACK_TO; 3 -> EXPLORE_TO; 4 -> FLEE_TO; default -> MOVE_TO }
    /// ```
    ///
    /// Wedge/Square/Mob and subordinate (non-captain) sorting remain explicit gaps, as do
    /// garrison-into-transport and the disembark executor. GROUP_MOVE construction is
    /// recovered through its complete per-member predicate and `GroupOrder` payload. A
    /// fleet which does not provide complete [`FormationMember`] facts retains the flat
    /// order spine; the bridge never invents spacing values.
    #[allow(clippy::too_many_arguments)]
    fn action_move_near(
        &mut self,
        x: i32,
        y: i32,
        tolerance: i32,
        q: QueuePos,
        set_angle: bool,
        angle: i32,
        orders: i64,
        form: i32,
        width: i32,
        disembark: bool,
        f: &mut dyn Fleet,
    ) {
        self.normalize_for_action(f);
        let mut body = |a: &mut Action<'_>, q: QueuePos, f: &mut dyn Fleet| {
            let kind = move_order_kind(orders);
            let (who, list) = a.members();
            if list
                .iter()
                .copied()
                .any(|o| f.alive(who, o) && f.can_move(who, o) && !f.can_install_order(who, o, q))
            {
                return;
            }
            let water = f.formation_water_destination(x, y);
            let profiles: Option<Vec<FormationMember>> = list
                .iter()
                .copied()
                .map(|o| {
                    (f.alive(who, o)
                        && f.can_move(who, o)
                        && f.is_on_map(who, o)
                        && f.is_captain(who, o))
                    .then(|| f.formation_member(who, o, water))
                    .flatten()
                })
                .collect();

            let resolved_form = if form == 9 || form == -1 {
                // `GroupData::get_form` `0x0070B9F0`: `-1` is both the initial
                // sentinel and a real signed UnitData value. Consequently an early
                // `-1` is skipped, while a `-1` after a non-negative form makes the
                // group mixed. Preserve that order-sensitive quirk.
                let mut common = -1i32;
                for &o in &list {
                    if !f.alive(who, o) || !f.is_on_map(who, o) {
                        continue;
                    }
                    let member_form = f.form(who, o) as i32;
                    if member_form != common {
                        let had_form = common >= 0;
                        common = member_form;
                        if had_form {
                            common = -1;
                            break;
                        }
                    }
                }
                common.max(0)
            } else {
                form
            };
            let resolved_width = if width == -1 {
                profiles
                    .as_ref()
                    .and_then(|members| {
                        let (sum, count) = members
                            .iter()
                            .filter(|member| member.width != -1)
                            .fold((0i32, 0i32), |(sum, count), member| {
                                (sum.wrapping_add(member.width), count + 1)
                            });
                        (count != 0).then(|| sum / count)
                    })
                    .unwrap_or(50)
            } else {
                width
            };

            let leader = a.form_leader(f);
            let old = leader.map(|leader| {
                let mut old = if q == QueuePos::Last {
                    f.final_pos(who, leader)
                } else {
                    f.pos(who, leader)
                };
                if let Some(group) = a.groups.get(a.slot) {
                    if group.ox >= 0
                        && group.oy >= 0
                        && vector_dist(group.ox.wrapping_sub(old.0), group.oy.wrapping_sub(old.1))
                            < 0x181
                    {
                        old = (group.ox, group.oy);
                    }
                }
                old
            });

            let computed = profiles.as_ref().and_then(|profiles| {
                let old = old?;
                let group = a.groups.get_mut(a.slot)?;
                group.compute_form(
                    profiles,
                    x,
                    y,
                    resolved_form,
                    resolved_width,
                    set_angle,
                    angle,
                    old.0,
                    old.1,
                    f.force_formation_facing_zero(),
                )
            });
            // Complete type facts opt into the exact formation transaction. A shape whose
            // recovered solver is not authoritative must not silently collapse every unit
            // onto the click point. Hosts without facts remain on the older flat spine.
            if profiles.is_some() && computed.is_none() {
                return;
            }
            if q == QueuePos::Last || q == QueuePos::New {
                if let Some((_, actual_angle)) = computed.as_ref() {
                    if let Some(group) = a.groups.get_mut(a.slot) {
                        group.o_angle = *actual_angle;
                        group.ox = x;
                        group.oy = y;
                        group.disband = 0;
                    }
                }
            }

            let group_order_id = a.groups.get(a.slot).map(|group| {
                a.frame
                    .wrapping_mul(10)
                    .wrapping_add(group.id)
                    .wrapping_mul(100)
                    .wrapping_add(group.order_num)
            });
            let group_leader = computed
                .as_ref()
                .and_then(|(layout, _)| list.get(layout.leader_index).copied());

            for (i, o) in list.iter().copied().enumerate() {
                if !f.alive(who, o) || !f.can_move(who, o) {
                    continue;
                }
                let (to_x, to_y, member_angle) = if let Some((layout, actual_angle)) = &computed {
                    let angle_offset = a.groups.get(a.slot).map_or(0, |group| {
                        (group.angles[i] as i32).wrapping_mul(0x0100_0000)
                    });
                    (
                        formation_order_coord(layout.to_x[i]),
                        formation_order_coord(layout.to_y[i]),
                        actual_angle.wrapping_add(angle_offset),
                    )
                } else {
                    (x, y, angle)
                };
                let profile = profiles.as_ref().and_then(|profiles| profiles.get(i));
                let masks = f.unit_masks(who, o);
                let promote_group = computed.is_some()
                    && matches!(orders, 1 | 2)
                    && resolved_form != Formation::Mob as i32
                    && list.len() > 1
                    && profile.is_some_and(|member| !member.modern_infantry)
                    && (f.role(who, o) & 0x10 == 0 || masks & 0x0004_0000 != 0)
                    && masks & 4 == 0
                    && f.domain(who, o) != 1;

                let mut ord = if promote_group {
                    let layout = &computed.as_ref().expect("checked above").0;
                    OrderRec {
                        kind: if orders == 2 {
                            OrderIndex::GroupAttackTo
                        } else {
                            OrderIndex::GroupMove
                        },
                        flags: 1 | if form != 0 { 4 } else { 0 } | if disembark { 0x20 } else { 0 },
                        x: to_x,
                        y: to_y,
                        angle: member_angle,
                        facing: i32::from(layout.reverse),
                        dest_x: to_x,
                        dest_y: to_y,
                        orig_x: x,
                        orig_y: y,
                        group_oxx: i32::from(group_leader.expect("computed layout has leader")),
                        group_whose: i32::from(who),
                        group_id: group_order_id.expect("computed layout has group"),
                        group_form_id: i32::from(set_angle),
                        group_angle: member_angle,
                        ..OrderRec::default()
                    }
                } else {
                    let ord = Order {
                        kind,
                        x: to_x,
                        y: to_y,
                        tolerance,
                        ..Order::default()
                    };
                    let mut ord = OrderRec::from(ord);
                    ord.angle = member_angle;
                    // `action_move_near` pushes literal 1 as
                    // `Unit::add_move_facing_order`'s fifth argument at 0x00705F98.
                    ord.facing = 1;
                    ord
                };
                ord.angle = member_angle;
                a.install_rec(who, o, ord, q, f);
                f.set_unit_masks(who, o, masks & !0x400);
            }

            if computed.is_some() {
                if let Some(leader) = leader {
                    let update_angle = f.angle(who, leader);
                    if let Some(group) = a.groups.get_mut(a.slot) {
                        group.update_positions(update_angle);
                    }
                }
            }
            if let Some(group) = a.groups.get_mut(a.slot) {
                group.order_num = group.order_num.wrapping_add(1);
            }
        };
        if self.with_queue_first(q, f, &mut body) {
            return;
        }
        body(self, q, f);
    }

    /// `Group::action_attack` `0x00712490` (3,833 B, 16 call sites).
    ///
    /// Retail splits the group: members that can reach the target get `ATTACK`, members
    /// that cannot are sent at it with `action_move_to`, and casters get `CAST_SPELL`.
    /// The reach test is `Unit::find_attack_pos` `0x00601280`, which is uncited by any
    /// Rust file, so this port installs `ATTACK` on every member and does not model the
    /// split. That is a known and stated gap, not an approximation of one.
    fn action_attack(&mut self, ox: i32, whom: i32, q: QueuePos, f: &mut dyn Fleet) {
        let mut body = |a: &mut Action<'_>, q: QueuePos, f: &mut dyn Fleet| {
            let (who, list) = a.members();
            for o in list {
                if !f.alive(who, o) {
                    continue;
                }
                let ord = Order {
                    kind: OrderIndex::Attack,
                    target_who: whom as i8,
                    target_o: ox as i16,
                    ..Order::default()
                };
                a.install(who, o, ord, q, f);
            }
        };
        if self.with_queue_first(q, f, &mut body) {
            return;
        }
        body(self, q, f);
    }

    /// The shared shape of `action_guard` / `action_garrison` / `action_repair` /
    /// `action_gather` / `action_board_ship` / `action_trade`: one
    /// `Unit::add_<k>_order(ox, whom, queued)` per member, with the `QUEUE_FIRST` dance in
    /// front. All seven are [structure] from their decompiled bodies; all seven install
    /// exactly the one `OrderIndex` [measured, `ADD_ORDER_KINDS`].
    fn action_target(
        &mut self,
        kind: OrderIndex,
        ox: i32,
        whom: i32,
        q: QueuePos,
        f: &mut dyn Fleet,
    ) {
        let mut body = |a: &mut Action<'_>, q: QueuePos, f: &mut dyn Fleet| {
            let (who, list) = a.members();
            for o in list {
                if !f.alive(who, o) {
                    continue;
                }
                let ord = Order {
                    kind,
                    target_who: if whom < 0 { who as i8 } else { whom as i8 },
                    target_o: ox as i16,
                    ..Order::default()
                };
                a.install(who, o, ord, q, f);
            }
        };
        if self.with_queue_first(q, f, &mut body) {
            return;
        }
        body(self, q, f);
    }

    /// `action_attack_ground` / `action_patrol` / `action_launch_patrol`: a coordinate
    /// order with no object target.
    fn action_ground(&mut self, kind: OrderIndex, x: i32, y: i32, q: QueuePos, f: &mut dyn Fleet) {
        let mut body = |a: &mut Action<'_>, q: QueuePos, f: &mut dyn Fleet| {
            let (who, list) = a.members();
            for o in list {
                if !f.alive(who, o) {
                    continue;
                }
                let ord = Order {
                    kind,
                    x,
                    y,
                    ..Order::default()
                };
                a.install(who, o, ord, q, f);
            }
        };
        if self.with_queue_first(q, f, &mut body) {
            return;
        }
        body(self, q, f);
    }

    /// The patrol-specific group actions. These deliberately do not use
    /// [`Self::with_queue_first`]: both recovered patrol paths are exceptions to that
    /// generic stash/replay dance.
    fn action_patrol(&mut self, x: i32, y: i32, q: QueuePos, launch_only: bool, f: &mut dyn Fleet) {
        let (who, list) = self.members();
        let (group_id, form_id) = self
            .groups
            .get(self.slot)
            .map_or((-1, -1), |g| (g.id, g.form));
        for o in list {
            if !f.alive(who, o) || !f.can_move(who, o) {
                continue;
            }
            let is_plane = f.is_plane(who, o);
            if launch_only && !is_plane {
                continue;
            }
            if is_plane {
                self.install_air_patrol_member(who, o, x, y, q, f);
                continue;
            }
            let (ux, uy) = f.pos(who, o);
            let Some(queue) = f.orders_mut(who, o) else {
                continue;
            };
            let before = queue.len();
            let mut unit = UnitWork::at(who, o, ux, uy);
            unit.orders = std::mem::take(queue);
            let result = install_group_patrol(
                &mut unit, ux, uy, x, y, group_id, form_id, o as i32, who as i32, q,
            );
            *queue = unit.orders;
            self.record_patrol_install(result, before, OrderIndex::GroupPatrol);
        }
    }

    fn install_air_patrol_member(
        &mut self,
        who: u8,
        o: i16,
        x: i32,
        y: i32,
        q: QueuePos,
        f: &mut dyn Fleet,
    ) {
        let (ux, uy) = f.pos(who, o);
        let home = f.air_patrol_home(who, o);
        let (home_o, home_who, home_pos) =
            home.map_or((-1, -1, None), |h| (h.0, h.1, Some((h.2, h.3))));
        let Some(queue) = f.orders_mut(who, o) else {
            return;
        };
        let before = queue.len();
        let mut unit = UnitWork::at(who, o, ux, uy);
        unit.orders = std::mem::take(queue);
        let result = install_air_patrol(&mut unit, x, y, home_o, home_who, home_pos, true, q);
        *queue = unit.orders;
        self.record_patrol_install(result, before, OrderIndex::AirPatrol);
    }

    fn record_patrol_install(&mut self, result: PatrolInstall, before: usize, kind: OrderIndex) {
        match result {
            PatrolInstall::ExtendedWaypoints => {}
            PatrolInstall::Replaced => {
                if before != 0 {
                    self.stats.orders_cleared += 1;
                }
                self.stats.orders_installed += 1;
                self.stats.by_order[kind.index()] += 1;
            }
            PatrolInstall::AppendedOrder => {
                self.stats.orders_installed += 1;
                self.stats.by_order[kind.index()] += 1;
            }
        }
    }

    /// `Group::action_halt(int flags)` `0x0070D0C0` (685 B, 14 call sites).
    ///
    /// The exact fact snapshot and ordered lifecycle effects are applied atomically by
    /// the world host. The bridge accepts only a recomputable planner receipt.
    fn action_halt(&mut self, flags: i32, f: &mut dyn Fleet) -> bool {
        let Some(group) = self.groups.get(self.slot).cloned() else {
            return false;
        };
        let n = group.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
        let nonempty_before: Vec<i16> = group.list[..n]
            .iter()
            .copied()
            .filter(|&o| {
                f.orders(group.who, o)
                    .is_some_and(|orders| !orders.is_empty())
            })
            .collect();
        let request = GroupHaltTransactionRequest { group, flags };
        let receipt = f.apply_group_halt_transaction(request.clone());
        if receipt.status != GroupHaltTransactionStatus::Applied || !receipt.validates(&request) {
            return false;
        }
        let Some(plan) = receipt.plan else {
            return false;
        };
        self.stats.orders_cleared += plan
            .steps
            .iter()
            .filter(|step| {
                matches!(
                    step,
                    HaltStep::CloseOrders { o, .. } if nonempty_before.contains(o)
                )
            })
            .count() as u64;
        if let Some(group) = self.groups.get_mut(self.slot) {
            *group = plan.group;
            true
        } else {
            false
        }
    }

    /// `Group::action_stance(int stance)` `0x0070D440` (928 B, 8 call sites).
    ///
    /// The atomic host owns `is_on_map`, representative/scanned stance type, modal option,
    /// negative cycling, building/unit filters, and the type-zero mandatory-order tail.
    /// Only a recomputable complete plan may update the addressed Bridge group.
    fn action_stance(&mut self, stance: i32, f: &mut dyn Fleet) {
        let Some(group) = self.groups.get(self.slot).cloned() else {
            return;
        };
        let request = GroupStanceRequest { group, stance };
        let receipt = f.apply_group_stance_transaction(request.clone());
        if receipt.status != GroupStateTransactionStatus::Applied || !receipt.validates(&request) {
            return;
        }
        let Some(plan) = receipt.plan else { return };
        if let Some(group) = self.groups.get_mut(self.slot) {
            *group = plan.group;
        }
    }

    /// `Group::action_disband(int all)` `0x0070E260` (693 B, 3 call sites).
    ///
    /// The host preflights scenario-ignore-orders, validation, active-building queueing,
    /// direct disband, and feedback before committing. Retail leaves dead identities in
    /// the group until a later normalize pass; the accepted plan therefore never compacts.
    fn action_disband(&mut self, all: bool, f: &mut dyn Fleet) -> bool {
        let Some(group) = self.groups.get(self.slot).cloned() else {
            return false;
        };
        let request = GroupDisbandTransactionRequest { group, all };
        let receipt = f.apply_group_disband_transaction(request.clone());
        if receipt.status != GroupDisbandTransactionStatus::Applied || !receipt.validates(&request)
        {
            return false;
        }
        let Some(plan) = receipt.plan else {
            return false;
        };
        if let Some(group) = self.groups.get_mut(self.slot) {
            *group = plan.group;
            true
        } else {
            false
        }
    }
}

/// `Unit::add_move_facing_order` `0x005E55C0`'s kind selection [measured].
///
/// The type-flag promotion of `ATTACK_TO` to `EXPLORE_TO` needs `UnitType+0x2C8 & 0x10`,
/// which this module does not hold, so it is *not* applied here; a caller that has the
/// type data should apply it before calling.
#[inline]
pub fn move_order_kind(orders: i64) -> OrderIndex {
    match orders {
        2 => OrderIndex::AttackTo,
        3 => OrderIndex::ExploreTo,
        4 => OrderIndex::FleeTo,
        _ => OrderIndex::MoveTo,
    }
}

// ---------------------------------------------------------------------------
// Command builders — the issuing side, so the bridge can be driven without a replay
// ---------------------------------------------------------------------------

/// Build the wire bytes of a command, PDB layout, so `don-env` (or a test) can drive the
/// bridge through the same path a replay does instead of poking orders directly.
pub mod build {
    use super::QueuePos;

    /// `GroupCommand` (0): `op | num | who | i16 list[num]`.
    pub fn group(who: i8, list: &[i16]) -> Vec<u8> {
        let mut v = vec![0u8, list.len() as u8, who as u8];
        for o in list {
            v.extend_from_slice(&o.to_le_bytes());
        }
        v
    }

    /// `FormCommand` (3): `op | i32 form | i32 rotate | i32 queued`.
    pub fn form(form: i32, rotate: i32, q: QueuePos) -> Vec<u8> {
        let mut v = vec![3u8];
        v.extend_from_slice(&form.to_le_bytes());
        v.extend_from_slice(&rotate.to_le_bytes());
        v.extend_from_slice(&(q as i32).to_le_bytes());
        v
    }

    /// `MoveToCommand` (7), 22 bytes.
    pub fn move_to(x: i32, y: i32, q: QueuePos, orders: i8) -> Vec<u8> {
        let mut v = vec![7u8];
        v.extend_from_slice(&x.to_le_bytes()); // to_x   @1
        v.extend_from_slice(&y.to_le_bytes()); // to_y   @5
        v.extend_from_slice(&0i32.to_le_bytes()); // set_angle @9
        v.extend_from_slice(&0i32.to_le_bytes()); // angle  @13
        v.push(orders as u8); // orders @17
        v.push(q as u8); // queued @18
        v.push(0); // form   @19
        v.push(0); // width  @20
        v.push(0); // disembark @21
        v
    }

    /// `MoveNearCommand` (8), 26 bytes.
    pub fn move_near(x: i32, y: i32, tolerance: i32, q: QueuePos, orders: i8) -> Vec<u8> {
        let mut v = vec![8u8];
        v.extend_from_slice(&x.to_le_bytes()); // to_x @1
        v.extend_from_slice(&y.to_le_bytes()); // to_y @5
        v.extend_from_slice(&tolerance.to_le_bytes()); // tolerance @9
        v.extend_from_slice(&0i32.to_le_bytes()); // set_angle @13
        v.extend_from_slice(&0i32.to_le_bytes()); // angle @17
        v.push(orders as u8); // orders @21
        v.push(q as u8); // queued @22
        v.push(0); // form @23
        v.push(0); // width @24
        v.push(0); // disembark @25
        v
    }

    /// `AttackCommand` (4), 17 bytes.
    pub fn attack(ox: i32, whom: i32, q: QueuePos) -> Vec<u8> {
        let mut v = vec![4u8];
        v.extend_from_slice(&ox.to_le_bytes());
        v.extend_from_slice(&whom.to_le_bytes());
        v.extend_from_slice(&0i32.to_le_bytes()); // ignore @9
        v.extend_from_slice(&(q as i32).to_le_bytes()); // queued @13
        v
    }

    /// `PatrolCommand` (10), 10 bytes.
    pub fn patrol(x: i32, y: i32, q: QueuePos) -> Vec<u8> {
        let mut v = vec![10u8];
        v.extend_from_slice(&x.to_le_bytes());
        v.extend_from_slice(&y.to_le_bytes());
        v.push(q as u8);
        v
    }

    /// `LaunchPatrolCommand` (11), 25 bytes.
    pub fn launch_patrol(x: i32, y: i32, q: QueuePos) -> Vec<u8> {
        let mut v = vec![11u8];
        v.extend_from_slice(&x.to_le_bytes());
        v.extend_from_slice(&y.to_le_bytes());
        v.extend_from_slice(&(q as i32).to_le_bytes());
        v.extend_from_slice(&[0u8; 12]); // shift, ctrl, alt
        v
    }

    /// `HaltCommand` (12), 1 byte.
    pub fn halt() -> Vec<u8> {
        vec![12u8]
    }

    /// `StanceCommand` (2), 5 bytes.
    pub fn stance(s: i32) -> Vec<u8> {
        let mut v = vec![2u8];
        v.extend_from_slice(&s.to_le_bytes());
        v
    }

    /// `GuardCommand` (31) / `FollowCommand` (30) / `GarrisonCommand` (20) /
    /// `RepairCommand` (16) — all `op | i32 ox | i32 whom | i32 queued`.
    pub fn target(op: u8, ox: i32, whom: i32, q: QueuePos) -> Vec<u8> {
        let mut v = vec![op];
        v.extend_from_slice(&ox.to_le_bytes());
        v.extend_from_slice(&whom.to_le_bytes());
        v.extend_from_slice(&(q as i32).to_le_bytes());
        v
    }

    /// `DisbandCommand` (21), 5 bytes.
    pub fn disband(all: i32) -> Vec<u8> {
        let mut v = vec![21u8];
        v.extend_from_slice(&all.to_le_bytes());
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::order_dispatch::PatrolPayload;

    fn fleet(n: usize) -> ObjectTable {
        let mut t = ObjectTable::new(n);
        for o in 0..n as i16 {
            t.put(1, o, Slot::unit(100 + o as u16, o as i32 * 16, 0));
        }
        t
    }

    fn select(b: &mut Bridge, p: &mut Package, f: &mut ObjectTable, list: &[i16]) {
        b.process_all(p, &build::group(1, list), f).unwrap();
    }

    // -- tables -------------------------------------------------------------

    #[test]
    fn the_opcode_table_is_dense_and_indexed_by_opcode() {
        assert_eq!(NUM_OPCODES, 82);
        for (i, d) in OPCODES.iter().enumerate() {
            assert_eq!(d.op as usize, i, "{} is out of order", d.name);
        }
        assert_eq!(OPCODES[0].name, "GroupCommand");
        assert_eq!(OPCODES[57].name, "CheckSumsCommand");
        assert!(matches!(OPCODES[57].wire, WireLen::Fixed(65)));
    }

    /// The measurement that makes the wire table trustworthy: `process_<x>` returns
    /// `sizeof(<X>Command)`, and exactly three handlers compute it instead.
    #[test]
    fn exactly_three_opcodes_are_variable_length() {
        let var: Vec<u8> = OPCODES
            .iter()
            .filter(|d| d.wire == WireLen::Variable)
            .map(|d| d.op)
            .collect();
        assert_eq!(var, vec![0, 51, 68]);
    }

    /// The engine's own receiver split, by the call each handler makes.
    #[test]
    fn the_receiver_split_is_35_group_11_leader_2_unit_7_game() {
        let count = |r: Receiver| OPCODES.iter().filter(|d| d.receiver == r).count();
        assert_eq!(count(Receiver::Group), 35);
        assert_eq!(count(Receiver::Leader), 11);
        assert_eq!(count(Receiver::Unit), 2);
        assert_eq!(count(Receiver::Game), 7);
        assert_eq!(count(Receiver::None), 27);
        assert_eq!(35 + 11 + 2 + 7 + 27, NUM_OPCODES);
        // BEGIN / ALARM / UNITMASK / BUILDMASK are group-scoped, not player-scoped:
        // BEGIN reaches action_begin through the group vtable; the other three call
        // Group::action_* directly, exactly like MOVE_TO's handler does.
        for op in [1usize, 27, 32, 33] {
            assert!(OPCODES[op].is_group_action(), "opcode {op}");
        }
        assert_eq!(OPCODES[1].action, Some("begin"));
        // …and HOTKEY is not: process_hotkey calls HotKeyGroups::copy_group.
        assert!(!OPCODES[34].is_group_action());
        assert_eq!(OPCODES[34].action, None);
    }

    #[test]
    fn the_action_table_holds_every_group_action() {
        assert_eq!(NUM_GROUP_ACTIONS, 42);
        assert_eq!(GROUP_ACTIONS.iter().map(|a| a.call_sites).sum::<u32>(), 209);
        let mn = ActionDef::find("move_near").unwrap();
        assert_eq!(mn.va, 0x00704990);
        assert_eq!(mn.size, 9205);
        assert_eq!(mn.call_sites, 23);
        // move_to is a 49-byte forwarder and is called *more* often than move_near.
        let mt = ActionDef::find("move_to").unwrap();
        assert_eq!((mt.size, mt.call_sites), (49, 34));
        assert_eq!(mt.delegates, &["move_near"]);
    }

    /// Every opcode that names an action must name one that exists.
    #[test]
    fn every_named_group_action_resolves() {
        for d in OPCODES.iter().filter(|d| d.is_group_action()) {
            let n = d
                .action
                .unwrap_or_else(|| panic!("{} has no action", d.name));
            assert!(ActionDef::find(n).is_some(), "{} -> {n}", d.name);
        }
    }

    /// `add_patrol_order` allocating `GROUP_PATROL` is the allocation-side confirmation
    /// of `order.rs`'s jump-table finding that `OrderIndex::PATROL` is dead.
    #[test]
    fn patrol_change_form_and_group_attack_are_never_constructed() {
        assert!(UNCONSTRUCTED_ORDERS.contains(&OrderIndex::Patrol));
        assert!(UNCONSTRUCTED_ORDERS.contains(&OrderIndex::ChangeForm));
        assert!(UNCONSTRUCTED_ORDERS.contains(&OrderIndex::GroupAttack));
        let (_, _, k) = ADD_ORDER_KINDS
            .iter()
            .find(|(n, _, _)| *n == "add_patrol_order")
            .unwrap();
        assert_eq!(*k, &[OrderIndex::GroupPatrol]);
        // and nothing in the table allocates one of the four dead kinds.
        for (n, _, kinds) in ADD_ORDER_KINDS.iter() {
            for k in kinds.iter() {
                assert!(!UNCONSTRUCTED_ORDERS.contains(k), "{n} allocates {k}");
            }
        }
    }

    #[test]
    fn move_order_kind_matches_the_measured_switch() {
        assert_eq!(move_order_kind(2), OrderIndex::AttackTo);
        assert_eq!(move_order_kind(3), OrderIndex::ExploreTo);
        assert_eq!(move_order_kind(4), OrderIndex::FleeTo);
        for v in [0i64, 1, 5, 19, 27, -1] {
            assert_eq!(move_order_kind(v), OrderIndex::MoveTo, "orders={v}");
        }
    }

    // -- wire length --------------------------------------------------------

    #[test]
    fn wire_len_agrees_with_the_pdb_sizeof_for_every_fixed_opcode() {
        for d in OPCODES.iter() {
            if let WireLen::Fixed(n) = d.wire {
                let mut b = vec![0u8; n as usize];
                b[0] = d.op;
                assert_eq!(wire_len(&b), Ok(n as usize), "{}", d.name);
            }
        }
    }

    #[test]
    fn variable_lengths_follow_their_handlers() {
        // GroupCommand: 3 + 2*num.
        assert_eq!(wire_len(&[0, 0, 1]), Ok(3));
        assert_eq!(wire_len(&build::group(1, &[5, 6, 7])), Ok(9));
        // SplineCommand: 6 + 8*len.
        assert_eq!(wire_len(&[51, 0, 0, 0, 2, 0]), Ok(22));
        // ChatCommand: 19 + 2*len, rejected past the 512-byte send buffer.
        let mut chat = vec![0u8; 19];
        chat[0] = 68;
        chat[13..17].copy_from_slice(&4i32.to_le_bytes());
        assert_eq!(wire_len(&chat), Ok(27));
        chat[13..17].copy_from_slice(&99999i32.to_le_bytes());
        assert!(matches!(wire_len(&chat), Err(WireError::BadLength { .. })));
    }

    #[test]
    fn an_unknown_opcode_is_an_error_not_a_guess() {
        assert_eq!(wire_len(&[200]), Err(WireError::UnknownOpcode(200)));
        assert!(matches!(wire_len(&[]), Err(WireError::Truncated { .. })));
    }

    // -- selection ----------------------------------------------------------

    #[test]
    fn a_command_with_no_selection_is_dropped_exactly_as_retail_drops_it() {
        let mut b = Bridge::new();
        let mut f = fleet(8);
        let mut p = Package::new(1, 0);
        assert_eq!(p.group, -1);
        b.process_all(&mut p, &build::move_to(64, 64, QueuePos::New, 0), &mut f)
            .unwrap();
        assert_eq!(b.stats.no_group, 1);
        assert_eq!(b.stats.orders_installed, 0);
        for o in 0..8 {
            assert!(f.orders(1, o).unwrap().is_empty());
        }
    }

    #[test]
    fn opcode_zero_interns_a_group_and_backlinks_every_member() {
        let mut b = Bridge::new();
        let mut f = fleet(8);
        let mut p = Package::new(1, 0);
        select(&mut b, &mut p, &mut f, &[1, 2, 3]);
        assert!(p.group >= 0);
        assert_eq!(b.stats.selections, 1);
        let g = b.groups.get(p.group).unwrap();
        assert_eq!(g.num, 3);
        assert_eq!(g.who, 1);
        assert_eq!(&g.list[..3], &[1, 2, 3]);
        for o in [1i16, 2, 3] {
            assert_eq!(f.group_of(1, o), p.group as i16);
        }
        assert_eq!(f.group_of(1, 0), -1);
    }

    #[test]
    fn slots_come_from_this_owners_window() {
        let mut b = Bridge::new();
        let mut f = fleet(8);
        let mut p = Package::new(1, 0);
        select(&mut b, &mut p, &mut f, &[1, 2]);
        let lo = (1 * GROUP_SLOTS_STRIDE) as i32;
        assert!(
            (lo..lo + GROUP_SLOTS_SCANNED as i32).contains(&p.group),
            "slot {} outside owner 1's window",
            p.group
        );
    }

    #[test]
    fn an_empty_group_command_reselects_by_object_and_uid() {
        let mut b = Bridge::new();
        let mut f = fleet(8);
        let mut p = Package::new(1, 0);
        select(&mut b, &mut p, &mut f, &[4, 5]);
        let first = p.group;
        // Recycle object 5 into a different entity: same index, new uid.
        f.put(1, 5, Slot::unit(999, 0, 0));
        let mut p2 = Package::new(1, 1);
        select(&mut b, &mut p2, &mut f, &[]);
        let g = b.groups.get(p2.group).unwrap();
        assert_eq!(g.num, 1, "the recycled slot must not be re-selected");
        assert_eq!(g.list[0], 4);
        let _ = first;
    }

    // -- orders -------------------------------------------------------------

    #[test]
    fn move_to_installs_one_move_order_per_member() {
        let mut b = Bridge::new();
        let mut f = fleet(8);
        let mut p = Package::new(1, 0);
        select(&mut b, &mut p, &mut f, &[0, 1, 2]);
        b.process_all(&mut p, &build::move_to(320, 96, QueuePos::New, 0), &mut f)
            .unwrap();
        assert_eq!(b.stats.orders_installed, 3);
        assert_eq!(b.stats.by_order[OrderIndex::MoveTo.index()], 3);
        for o in [0i16, 1, 2] {
            let l = f.orders(1, o).unwrap();
            assert_eq!(l.len(), 1);
            let ord = l.current().unwrap();
            assert_eq!(ord.kind, OrderIndex::MoveTo);
            assert_eq!((ord.x, ord.y), (320, 96));
        }
        assert!(f.orders(1, 3).unwrap().is_empty());
    }

    /// The `orders` byte selects the order kind, and `move_near` carries a tolerance the
    /// `move_to` leg hard-codes to 0.
    #[test]
    fn the_orders_byte_and_tolerance_reach_the_installed_order() {
        let mut b = Bridge::new();
        let mut f = fleet(4);
        let mut p = Package::new(1, 0);
        select(&mut b, &mut p, &mut f, &[0, 1]);
        b.process_all(
            &mut p,
            &build::move_near(10, 20, 48, QueuePos::New, 4),
            &mut f,
        )
        .unwrap();
        let ord = f.orders(1, 0).unwrap().current().unwrap().clone();
        assert_eq!(ord.kind, OrderIndex::FleeTo);
        assert_eq!(ord.tolerance, 48);
        b.process_all(&mut p, &build::move_to(10, 20, QueuePos::New, 2), &mut f)
            .unwrap();
        let ord = f.orders(1, 0).unwrap().current().unwrap().clone();
        assert_eq!(ord.kind, OrderIndex::AttackTo);
        assert_eq!(ord.tolerance, 0);
    }

    #[test]
    fn queue_last_appends_and_queue_new_replaces() {
        let mut b = Bridge::new();
        let mut f = fleet(4);
        let mut p = Package::new(1, 0);
        select(&mut b, &mut p, &mut f, &[0]);
        b.process_all(&mut p, &build::move_to(1, 1, QueuePos::New, 0), &mut f)
            .unwrap();
        b.process_all(&mut p, &build::move_to(2, 2, QueuePos::Last, 0), &mut f)
            .unwrap();
        assert_eq!(f.orders(1, 0).unwrap().len(), 2);
        assert_eq!(f.orders(1, 0).unwrap().current().unwrap().x, 1);
        b.process_all(&mut p, &build::move_to(3, 3, QueuePos::New, 0), &mut f)
            .unwrap();
        assert_eq!(f.orders(1, 0).unwrap().len(), 1);
        assert_eq!(f.orders(1, 0).unwrap().current().unwrap().x, 3);
    }

    /// `QUEUE_FIRST` is the stash / halt / re-issue-as-NEW / replay dance, so the new
    /// order becomes current and the old queue survives behind it.
    #[test]
    fn queue_first_puts_the_new_order_in_front_and_keeps_the_tail() {
        let mut b = Bridge::new();
        let mut f = fleet(4);
        let mut p = Package::new(1, 0);
        select(&mut b, &mut p, &mut f, &[0]);
        b.process_all(&mut p, &build::move_to(1, 1, QueuePos::New, 0), &mut f)
            .unwrap();
        b.process_all(&mut p, &build::move_to(2, 2, QueuePos::Last, 0), &mut f)
            .unwrap();
        b.process_all(&mut p, &build::move_to(9, 9, QueuePos::First, 0), &mut f)
            .unwrap();
        let l = f.orders(1, 0).unwrap();
        assert_eq!(l.len(), 3);
        let xs: Vec<i32> = l.iter().map(|o| o.x).collect();
        assert_eq!(xs, vec![9, 1, 2]);
    }

    #[test]
    fn patrol_bridge_uses_plane_routing_and_the_two_queue_exceptions() {
        let mut b = Bridge::new();
        let mut f = fleet(3);
        f.put(1, 1, Slot::plane(101, 16, 0));
        let mut p = Package::new(1, 0);

        select(&mut b, &mut p, &mut f, &[0]);
        b.process_all(&mut p, &build::patrol(48, 96, QueuePos::New), &mut f)
            .unwrap();
        b.process_all(&mut p, &build::patrol(111, 222, QueuePos::Last), &mut f)
            .unwrap();
        assert_eq!(f.orders(1, 0).unwrap().len(), 1);
        let PatrolPayload::Group(group) = &f.orders(1, 0).unwrap().front().unwrap().patrol_payload
        else {
            panic!("ground member did not receive GroupPatrolOrder")
        };
        assert_eq!(group.points.len(), 3);
        assert_eq!((group.points.x[2], group.points.y[2]), (111, 222));
        b.process_all(&mut p, &build::patrol(333, 444, QueuePos::First), &mut f)
            .unwrap();
        assert_eq!(f.orders(1, 0).unwrap().len(), 1);
        let PatrolPayload::Group(group) = &f.orders(1, 0).unwrap().front().unwrap().patrol_payload
        else {
            unreachable!()
        };
        assert_eq!(group.points.len(), 2, "ground QUEUE_FIRST replaces");

        select(&mut b, &mut p, &mut f, &[1]);
        b.process_all(&mut p, &build::patrol(10, 20, QueuePos::New), &mut f)
            .unwrap();
        b.process_all(&mut p, &build::patrol(30, 40, QueuePos::Last), &mut f)
            .unwrap();
        let PatrolPayload::Air(air) = &f.orders(1, 1).unwrap().front().unwrap().patrol_payload
        else {
            panic!("true plane did not receive AirPatrolOrder")
        };
        assert_eq!(air.points.len(), 2);
        b.process_all(&mut p, &build::patrol(50, 60, QueuePos::First), &mut f)
            .unwrap();
        let PatrolPayload::Air(air) = &f.orders(1, 1).unwrap().front().unwrap().patrol_payload
        else {
            unreachable!()
        };
        assert_eq!(air.points.len(), 1, "air QUEUE_FIRST replaces");

        select(&mut b, &mut p, &mut f, &[2]);
        b.process_all(&mut p, &build::launch_patrol(70, 80, QueuePos::New), &mut f)
            .unwrap();
        assert!(
            f.orders(1, 2).unwrap().is_empty(),
            "launch-patrol filters a non-plane member"
        );
    }

    #[test]
    fn halt_empties_every_members_order_list() {
        let mut b = Bridge::new();
        let mut f = fleet(4);
        let mut p = Package::new(1, 0);
        select(&mut b, &mut p, &mut f, &[0, 1]);
        b.process_all(&mut p, &build::move_to(5, 5, QueuePos::New, 0), &mut f)
            .unwrap();
        assert_eq!(b.stats.orders_installed, 2);
        b.process_all(&mut p, &build::halt(), &mut f).unwrap();
        for o in [0i16, 1] {
            assert!(f.orders(1, o).unwrap().is_empty());
        }
        assert_eq!(b.stats.orders_cleared, 2);
    }

    /// A building group refuses `halt`: the whole loop is gated on `group.buildings == 0`.
    #[test]
    fn a_building_selection_ignores_halt() {
        let mut b = Bridge::new();
        let mut f = ObjectTable::new(4);
        f.put(1, 0, Slot::building(1, 0, 0));
        let mut p = Package::new(1, 0);
        select(&mut b, &mut p, &mut f, &[0]);
        f.orders_mut(1, 0)
            .unwrap()
            .push_back(OrderRec::from(Order::move_to(1, 2, 0)));
        b.process_all(&mut p, &build::halt(), &mut f).unwrap();
        assert_eq!(f.orders(1, 0).unwrap().len(), 1);
    }

    #[test]
    fn attack_targets_the_addressed_object() {
        let mut b = Bridge::new();
        let mut f = fleet(4);
        let mut p = Package::new(1, 0);
        select(&mut b, &mut p, &mut f, &[0, 1]);
        b.process_all(&mut p, &build::attack(7, 2, QueuePos::New), &mut f)
            .unwrap();
        let ord = f.orders(1, 0).unwrap().current().unwrap().clone();
        assert_eq!(ord.kind, OrderIndex::Attack);
        assert_eq!((ord.target_who, ord.target_o), (2, 7));
        assert_eq!(b.stats.by_order[OrderIndex::Attack.index()], 2);
    }

    #[test]
    fn guard_and_follow_install_their_own_kinds_and_never_self_target() {
        let mut b = Bridge::new();
        let mut f = fleet(4);
        let mut p = Package::new(1, 0);
        select(&mut b, &mut p, &mut f, &[0, 1]);
        // follow object 0 of our own owner: member 0 must skip itself.
        b.process_all(&mut p, &build::target(30, 0, 1, QueuePos::New), &mut f)
            .unwrap();
        assert!(f.orders(1, 0).unwrap().is_empty());
        assert_eq!(
            f.orders(1, 1).unwrap().current().unwrap().kind,
            OrderIndex::Follow
        );
        b.process_all(&mut p, &build::target(31, 3, 2, QueuePos::New), &mut f)
            .unwrap();
        assert_eq!(
            f.orders(1, 0).unwrap().current().unwrap().kind,
            OrderIndex::Guard
        );
    }

    #[test]
    fn stance_writes_stance_and_installs_nothing() {
        let mut b = Bridge::new();
        let mut f = fleet(4);
        let mut p = Package::new(1, 0);
        select(&mut b, &mut p, &mut f, &[0, 1]);
        let before = b.stats.orders_installed;
        b.process_all(&mut p, &build::stance(2), &mut f).unwrap();
        assert_eq!(b.stats.orders_installed, before);
        assert_eq!(f.get(1, 0).unwrap().stance, 2);
        assert_eq!(f.get(1, 1).unwrap().stance, 2);
    }

    #[test]
    fn follow_uses_dedicated_transaction_and_preserves_both_target_identities() {
        let mut b = Bridge::new();
        let mut f = fleet(4);
        let mut p = Package::new(1, 0);
        select(&mut b, &mut p, &mut f, &[0, 1]);
        b.process_all(&mut p, &build::target(30, 1, 1, QueuePos::New), &mut f)
            .unwrap();

        let follower = f.orders(1, 0).unwrap().front().unwrap();
        assert_eq!(follower.kind, OrderIndex::Follow);
        assert_eq!(
            follower.follow,
            Some(FollowOrderPayload {
                ox: 1,
                whom: 1,
                uid: f.uid(1, 1),
                oxx: 1,
                whose: 1,
                uid2: f.uid(1, 1),
            })
        );
        assert!(
            f.orders(1, 1).unwrap().is_empty(),
            "canonical self is skipped"
        );
    }

    #[test]
    fn disband_without_all_kills_exactly_one_member_from_the_back() {
        let mut b = Bridge::new();
        let mut f = fleet(4);
        let mut p = Package::new(1, 0);
        select(&mut b, &mut p, &mut f, &[0, 1, 2]);
        b.process_all(&mut p, &build::disband(0), &mut f).unwrap();
        assert!(!f.alive(1, 2));
        assert!(f.alive(1, 1) && f.alive(1, 0));
        assert_eq!(
            b.groups.get(p.group).unwrap().num,
            3,
            "retail leaves the dead identity until Group::normalize"
        );
        b.process_all(&mut p, &build::disband(1), &mut f).unwrap();
        assert!(!f.alive(1, 0) && !f.alive(1, 1));
    }

    // -- packet walking -----------------------------------------------------

    #[test]
    fn a_whole_packet_walks_and_the_selection_leads() {
        let mut b = Bridge::new();
        let mut f = fleet(8);
        let mut p = Package::new(1, 0);
        let mut payload = build::group(1, &[0, 1, 2, 3]);
        payload.extend_from_slice(&build::move_to(100, 200, QueuePos::New, 0));
        payload.extend_from_slice(&build::move_to(300, 400, QueuePos::Last, 0));
        payload.extend_from_slice(&build::stance(1));
        b.process_all(&mut p, &payload, &mut f).unwrap();
        assert_eq!(b.stats.dispatched, 4);
        assert_eq!(b.stats.selections, 1);
        assert_eq!(b.stats.orders_installed, 8);
        for o in 0..4i16 {
            assert_eq!(f.orders(1, o).unwrap().len(), 2);
            assert_eq!(f.get(1, o).unwrap().stance, 1);
        }
    }

    #[test]
    fn an_unported_action_is_counted_not_faked() {
        let mut b = Bridge::new();
        let mut f = fleet(4);
        let mut p = Package::new(1, 0);
        select(&mut b, &mut p, &mut f, &[0]);
        // opcode 24 QUEUE_UP -> Group::action_queue_up, Port::Todo.
        let mut q = vec![24u8];
        q.extend_from_slice(&0i32.to_le_bytes());
        q.extend_from_slice(&1i32.to_le_bytes());
        b.process_all(&mut p, &q, &mut f).unwrap();
        assert_eq!(b.stats.unported, 1);
        assert_eq!(b.stats.orders_installed, 0);
        assert!(b.stats.ported_fraction() < 1.0);
    }

    #[test]
    fn a_truncated_payload_reports_where_it_ran_out() {
        let mut b = Bridge::new();
        let mut f = fleet(4);
        let mut p = Package::new(1, 0);
        let mut payload = build::group(1, &[0]);
        payload.push(7); // a MoveToCommand opcode with no body
        let e = b.process_all(&mut p, &payload, &mut f).unwrap_err();
        assert_eq!(
            e,
            WireError::Truncated {
                offset: 5,
                need: 22
            }
        );
    }

    #[test]
    fn dead_members_are_skipped_rather_than_addressed() {
        let mut b = Bridge::new();
        let mut f = fleet(4);
        let mut p = Package::new(1, 0);
        select(&mut b, &mut p, &mut f, &[0, 1, 2]);
        f.get_mut(1, 1).unwrap().alive = false;
        b.process_all(&mut p, &build::move_to(1, 1, QueuePos::New, 0), &mut f)
            .unwrap();
        assert_eq!(b.stats.orders_installed, 2);
        assert!(f.orders(1, 1).unwrap().is_empty());
    }
}
