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

use crate::order::{Order, OrderIndex};
use crate::systems::groups_guys::{
    formation_order_coord, resolve_form, vector_dist, Formation, FormationMember, GroupData,
    MemberState, GROUP_MAX_MEMBERS,
};
use crate::systems::order_dispatch::{
    install_air_patrol, install_group_patrol, OrderQueue, OrderRec, PatrolInstall, UnitWork,
};

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

/// How much of an inline `CommandPackage::process_*` state mutation the bridge carries.
/// These handlers have [`Receiver::None`] because they write `Game` / `TurnControl` /
/// per-player state directly rather than calling an `action_*` receiver.
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
    /// `Build::mask_me` `0x0063E2A0`: whether this building admits this UI build-mask
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
    pub is_captain: bool,
    pub leaves_groups: bool,
    pub form_category: i32,
    pub formation_member: Option<FormationMember>,
    pub form: i8,
    pub angle: i32,
    pub role: i32,
    pub domain: i32,
    pub unit_masks: u32,
    pub can_ever_transport: bool,
    pub build_masks: u16,
    pub build_mask_capabilities: u16,
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
}

impl ObjectTable {
    pub fn new(per_owner: usize) -> ObjectTable {
        ObjectTable {
            lists: (0..NUM_OWNER_SLOTS)
                .map(|_| vec![Slot::default(); per_owner])
                .collect(),
        }
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
    /// Inline `Game` / `TurnControl` / player-state handlers reproduced by this bridge.
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
pub const HOTKEY_GROUP_SLOTS: usize = 162;

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

/// State written inline by command handlers without an `action_*` receiver.
///
/// `speed` is `TurnControl+0x30`. `network`, `speed_locked`, and `immediate_process`
/// name the exact `Game+0x820/0x20/0x821` gates read by the handlers. The eight player
/// counters are the `u32` fields at `PlayerData+0x48..+0x68`. `ai_speed` and `ai_off`
/// are the `GameAccess` globals at `0x00C061C0/0x00C061C4` [measured].
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
    pub mp_log: bool,
    pub restart_delay: i32,
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
            mp_log: false,
            restart_delay: 0,
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
    /// The non-zero test recovered from `Group::action_set_transport` `0x007024B0`.
    /// Callers populate it from the owner leader's `0x100/0x200/0x400` transport flags.
    transport_level: [u8; NUM_OWNER_SLOTS],
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
            transport_level: [0; NUM_OWNER_SLOTS],
        }
    }

    /// Install the already-decoded transport level for one owner. Only zero/non-zero is
    /// read by opcode 14, exactly as retail's collapsed leader-flag branch does.
    pub fn set_transport_level(&mut self, who: u8, level: u8) {
        if let Some(slot) = self.transport_level.get_mut(who as usize) {
            *slot = level;
        }
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
            self.process_inline(pkg, cmd);
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

    /// Inline `CommandPackage::process_*` handlers which mutate state without an
    /// `action_*` receiver.
    fn process_inline(&mut self, pkg: &Package, cmd: &[u8]) {
        match cmd[0] {
            34 => self.process_hotkey(pkg, cmd),
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
            56 => {}
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
            76 => {
                if let Some(&state) = cmd.get(1) {
                    self.process_pause(pkg.play, state);
                }
            }
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
            81 => {}
            _ => unreachable!("inline command table and dispatcher disagree"),
        }
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

    /// Core state path of `TurnControl::pause` `0x00956990` / `0x00957AB0`.
    ///
    /// Retail compares the raw request byte with its one-bit paused flag and toggles only
    /// when they differ. The common solo path and the network pause allowance/counter are
    /// represented here. The network restart/callback tail remains `StateWired` metadata.
    fn process_pause(&mut self, play: i32, requested: u8) {
        if self.inline.paused as u8 == requested {
            return;
        }
        if !self.inline.paused {
            let play_slot = usize::try_from(play).ok().filter(|&p| p < NUM_OWNER_SLOTS);
            let allowed = !self.inline.network
                || play < 0
                || play_slot.is_some_and(|p| self.inline.pauses[p] < 10)
                || self.inline.pause_override;
            if !allowed {
                return;
            }
            self.inline.paused = true;
            self.inline.pause_delay = 0;
            if self.inline.network {
                if let Some(p) = play_slot {
                    self.inline.pauses[p] = self.inline.pauses[p].wrapping_add(1);
                }
            }
        } else if !self.inline.immediate_process {
            self.inline.paused = false;
            if self.inline.pause_delay == 0 {
                self.inline.pause_delay = 2;
            }
        }
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
            transport_level: self.transport_level,
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
    transport_level: [u8; NUM_OWNER_SLOTS],
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
        self.action_halt(0, f);
        {
            let mut inner = Action {
                groups: self.groups,
                slot: self.slot,
                stats: self.stats,
                frame: self.frame,
                transport_level: self.transport_level,
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
            "halt" => self.action_halt(0, f),
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
                let (Some(ox), Some(whom)) = (i32_at(cmd, 1), i32_at(cmd, 5)) else {
                    return;
                };
                let q = QueuePos::from_i64(i32_at(cmd, 9).unwrap_or(0) as i64);
                self.action_target(OrderIndex::Follow, ox, whom, q, f);
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
                self.action_disband(all != 0, f);
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

    /// `Group::action_begin` `0x00714100`: one store, `GroupData::disband = 0`.
    fn action_begin(&mut self) {
        if let Some(group) = self.groups.get_mut(self.slot) {
            group.disband = 0;
        }
    }

    /// Simulation state of `Group::action_set_transport` `0x007024B0`.
    ///
    /// The handler calls `action_begin`, collapses the owner's three transport flags to
    /// a zero/non-zero level, then sets `UnitData::unit_masks & 0x0080_0000` on members
    /// for which `UnitData::can_ever_transport` succeeds. The command flag is ignored
    /// when the owner has no transport level. Presentation callbacks are omitted.
    fn action_set_transport(&mut self, flag: i32, f: &mut dyn Fleet) {
        self.action_begin();
        if self.groups.get(self.slot).is_none_or(|g| g.buildings != 0) {
            return;
        }
        let (who, list) = self.members();
        let enabled = self
            .transport_level
            .get(who as usize)
            .is_some_and(|level| *level != 0)
            && flag != 0;
        for o in list {
            if !f.alive(who, o) || !f.is_unit(who, o) || !f.can_ever_transport(who, o) {
                continue;
            }
            let current = f.unit_masks(who, o);
            let next = if enabled {
                current | 0x0080_0000
            } else {
                current & !0x0080_0000
            };
            f.set_unit_masks(who, o, next);
        }
    }

    /// `Group::action_unitmask` `0x006FCB90`.
    ///
    /// The second wire dword is unused by retail. Except for mask `0x40000`, the first
    /// eligible member decides whether the whole group sets or clears the bit. Mask
    /// `0x40000` always clears. Mask `0x100` skips true planes and additionally cancels
    /// the current unit action; the bridge's canonical action state is its order queue.
    fn action_unitmask(&mut self, mask: u32, _set: i32, f: &mut dyn Fleet) {
        if self.groups.get(self.slot).is_none_or(|g| g.buildings != 0) {
            return;
        }
        let (who, list) = self.members();
        let mut set = mask != 0x0004_0000;
        for o in list {
            if (!f.alive(who, o) || !f.is_unit(who, o)) || (mask == 0x100 && f.is_plane(who, o)) {
                continue;
            }
            set = f.unit_masks(who, o) & mask == 0 && set;
            let mut next = f.unit_masks(who, o);
            if set {
                next |= mask;
            } else {
                next &= !mask;
            }
            if mask == 0x100 {
                next &= !0x0400_0000;
                if let Some(orders) = f.orders_mut(who, o) {
                    if !orders.is_empty() {
                        self.stats.orders_cleared += 1;
                    }
                    orders.clear();
                }
            }
            f.set_unit_masks(who, o, next);
        }
    }

    /// `Group::action_buildmask` `0x006FC9A0`.
    ///
    /// Like UNIT_MASK, the second dword is unused and the first eligible building
    /// chooses set-vs-clear for the whole selection. `Build::mask_me` is an explicit
    /// Fleet predicate: a missing build-state host therefore skips rather than guesses.
    fn action_buildmask(&mut self, mask: u16, _set: i32, f: &mut dyn Fleet) {
        if self.groups.get(self.slot).is_none_or(|g| g.buildings == 0) {
            return;
        }
        let (who, list) = self.members();
        let mut set = true;
        for o in list {
            if !f.alive(who, o) || !f.is_building(who, o) || !f.can_toggle_build_mask(who, o, mask)
            {
                continue;
            }
            let Some(current) = f.build_masks(who, o) else {
                continue;
            };
            set = current & mask == 0 && set;
            let next = if set { current | mask } else { current & !mask };
            let _ = f.set_build_masks(who, o, next);
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
            if let Some(g) = self.groups.get_mut(self.slot) {
                g.form = -1;
            }
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
            self.action_halt(0, f);
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

    /// The shared shape of `action_follow` / `action_guard` / `action_garrison` /
    /// `action_repair` / `action_gather` / `action_board_ship` / `action_trade`: one
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
                // `action_follow` refuses to make a unit follow itself [structure,
                // 0x006FD510: `if (member != ox || group.who != whom) add_follow_order`].
                if ox as i16 == o && (whom < 0 || whom as u8 == who) {
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
    /// Installs nothing. Per member it runs `Unit::close_orders(0)` +
    /// `Unit::clear_partial_path` + `Unit::update_action`, which empties the order list,
    /// after three guards [structure]: `group.buildings == 0` gates the whole loop, a
    /// plane that is airborne is skipped, and `flags & 1` / `flags & 2` skip units that
    /// answer two virtual predicates. Only the first guard and the emptying are here.
    fn action_halt(&mut self, _flags: i32, f: &mut dyn Fleet) {
        let buildings = self.groups.get(self.slot).map(|g| g.buildings).unwrap_or(0);
        if buildings != 0 {
            return;
        }
        let (who, list) = self.members();
        for o in list {
            if !f.alive(who, o) {
                continue;
            }
            if let Some(l) = f.orders_mut(who, o) {
                if !l.is_empty() {
                    self.stats.orders_cleared += 1;
                }
                l.clear();
            }
        }
        // `in_ECX[4] = -1` — GroupData::form is reset [structure, 0x0070D14C].
        if let Some(g) = self.groups.get_mut(self.slot) {
            g.form = -1;
        }
    }

    /// `Group::action_stance(int stance)` `0x0070D440` (928 B, 8 call sites).
    ///
    /// Installs no order: it writes `UnitData::stance` (`+0xB1`) and `Build::stance`
    /// (`+0x7E`) and sets bit `0x10` in the object flag byte at `+0x08` [structure].
    ///
    /// The negative arguments are cycles, and the modulus depends on the group's stance
    /// *type* — `GroupData::get_stance_type` `0x0070D370` returns 0/1/2/3 and the cycle
    /// length is 6/4/2/2 respectively [measured, the `switch` at `0x0070D47B`].
    /// `-1` steps forward, `-2` steps back. The stance type needs unit-type data this
    /// module does not hold, so a negative argument is resolved against the combat cycle
    /// of 6 and that assumption is stated rather than hidden.
    fn action_stance(&mut self, stance: i32, f: &mut dyn Fleet) {
        let cycle = 6i32;
        let v = if stance >= 0 {
            stance
        } else if stance == -2 {
            let c = self
                .groups
                .get(self.slot)
                .map(|g| g.facing as i32)
                .unwrap_or(0);
            if c - 1 < 0 {
                cycle - 1
            } else {
                c - 1
            }
        } else {
            let c = self
                .groups
                .get(self.slot)
                .map(|g| g.facing as i32)
                .unwrap_or(0);
            (c + 1) % cycle
        };
        let (who, list) = self.members();
        for o in list {
            if f.alive(who, o) {
                f.set_stance(who, o, v as i8);
            }
        }
    }

    /// `Group::action_disband(int all)` `0x0070E260` (693 B, 3 call sites).
    ///
    /// Walks the member list **backwards** and calls `Object::disband` `0x006455C0`;
    /// with `all == 0` it stops after the first success [structure]. Members that are
    /// producing buildings go to `Build::queue_up` instead, which is the production
    /// lane's, not this one's.
    fn action_disband(&mut self, all: bool, f: &mut dyn Fleet) {
        let (who, list) = self.members();
        let mut killed = 0;
        for &o in list.iter().rev() {
            if !f.alive(who, o) {
                continue;
            }
            f.disband(who, o);
            if let Some(g) = self.groups.get_mut(self.slot) {
                g.remove_member(o);
            }
            killed += 1;
            if !all {
                break;
            }
        }
        let _ = killed;
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
    fn disband_without_all_kills_exactly_one_member_from_the_back() {
        let mut b = Bridge::new();
        let mut f = fleet(4);
        let mut p = Package::new(1, 0);
        select(&mut b, &mut p, &mut f, &[0, 1, 2]);
        b.process_all(&mut p, &build::disband(0), &mut f).unwrap();
        assert!(!f.alive(1, 2));
        assert!(f.alive(1, 1) && f.alive(1, 0));
        assert_eq!(b.groups.get(p.group).unwrap().num, 2);
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
