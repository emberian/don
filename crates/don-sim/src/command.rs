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
//! The order *executors*. This module installs orders; [`crate::order`] describes the
//! 28-arm `Unit::do_job` table that runs them, and `Unit::work` `0x0060D180` — the driver
//! between them — is still uncited by any Rust file. An order installed here will sit in
//! its list until that lands.

use crate::order::{Order, OrderIndex, OrderList};
use crate::systems::groups_guys::{GroupData, GROUP_MAX_MEMBERS};

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
    /// Does this opcode reach a `Group::action_*`? 36 of the 82 do.
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
    /// The order installed on each eligible member, its `OrderIndex`, its target/coords
    /// and its `QueuePos` handling are reproduced. Eligibility predicates that need
    /// unit-type data we do not hold here are delegated to [`Fleet`].
    Orders,
    /// Reproduced, and retail installs no order either — it edits group or unit state
    /// (`stance`, `unitmask`, `set_transport`) or clears orders (`halt`).
    State,
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
    /// `UnitTypeData::role`, OR-ed into `GroupData::role` by `Group::add`.
    fn role(&self, who: u8, o: i16) -> i32 {
        let _ = (who, o);
        0
    }
    /// Can this object be given movement orders at all? Retail asks
    /// `UnitData::get_speed() > 0` plus a stack of "entering/exiting", "is_blown" and
    /// garrison predicates; a port supplies whichever of those it holds.
    fn can_move(&self, who: u8, o: i16) -> bool {
        self.is_unit(who, o)
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
    fn orders(&self, who: u8, o: i16) -> Option<&OrderList>;
    fn orders_mut(&mut self, who: u8, o: i16) -> Option<&mut OrderList>;
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
    pub role: i32,
    pub group: i16,
    pub uid: u16,
    pub x: i32,
    pub y: i32,
    pub stance: i8,
    pub orders: OrderList,
}

impl Slot {
    /// A live, movable unit at `(x, y)`.
    pub fn unit(uid: u16, x: i32, y: i32) -> Slot {
        Slot {
            alive: true,
            is_unit: true,
            can_move: true,
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
    fn role(&self, who: u8, o: i16) -> i32 {
        self.get(who, o).map_or(0, |s| s.role)
    }
    fn can_move(&self, who: u8, o: i16) -> bool {
        self.get(who, o).is_some_and(|s| s.can_move)
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
    fn orders(&self, who: u8, o: i16) -> Option<&OrderList> {
        self.get(who, o).map(|s| &s.orders)
    }
    fn orders_mut(&mut self, who: u8, o: i16) -> Option<&mut OrderList> {
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
            slots: vec![GroupData::default(); NUM_OWNER_SLOTS * GROUP_SLOTS_STRIDE],
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
            self.slots[dst] = g.clone();
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

/// The command→order bridge.
pub struct Bridge {
    pub groups: Groups,
    /// `CommandPackage::process_group`'s per-player re-selection cache: the `(o, uid)`
    /// pairs of the last non-empty selection. `0x00CBEE88` holds the counts and
    /// `0x00CBEEB0` / `0x00CBF6B0` the two parallel arrays [structure].
    last_selection: [Vec<(i16, u16)>; NUM_OWNER_SLOTS],
    pub stats: BridgeStats,
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
            frame: 0,
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
        let Some(list) = f.orders_mut(who, o) else {
            return;
        };
        if q == QueuePos::New {
            if !list.is_empty() {
                self.stats.orders_cleared += 1;
            }
            list.clear();
        }
        list.push(order);
        self.stats.orders_installed += 1;
        self.stats.by_order[order.kind.index()] += 1;
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
        let saved: Vec<(i16, Vec<Order>)> = list
            .iter()
            .filter_map(|&o| f.orders(who, o).map(|l| (o, l.iter().copied().collect())))
            .collect();
        self.action_halt(0, f);
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
                    l.push(ord);
                    self.stats.orders_installed += 1;
                    self.stats.by_order[ord.kind.index()] += 1;
                }
            }
        }
        true
    }

    fn run(&mut self, name: &str, cmd: &[u8], f: &mut dyn Fleet) {
        match name {
            "move_to" => {
                // MoveToCommand: to_x@1 to_y@5 set_angle@9 angle@13 orders@17 queued@18
                // form@19 width@20 disembark@21. process_move_to passes them to
                // action_move_to(to_x, to_y, queued, set_angle, angle, orders, 1, form,
                // width, disembark) [structure, 0x009497C0].
                let (Some(x), Some(y)) = (i32_at(cmd, 1), i32_at(cmd, 5)) else {
                    return;
                };
                let orders = i8_at(cmd, 17).unwrap_or(0) as i64;
                let q = QueuePos::from_i64(i8_at(cmd, 18).unwrap_or(0) as i64);
                self.action_move_near(x, y, 0, q, orders, f);
            }
            "move_near" => {
                // MoveNearCommand adds tolerance@9 and shifts the tail by four
                // [structure, 0x009495C0].
                let (Some(x), Some(y), Some(tol)) =
                    (i32_at(cmd, 1), i32_at(cmd, 5), i32_at(cmd, 9))
                else {
                    return;
                };
                let orders = i8_at(cmd, 21).unwrap_or(0) as i64;
                let q = QueuePos::from_i64(i8_at(cmd, 22).unwrap_or(0) as i64);
                self.action_move_near(x, y, tol, q, orders, f);
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
            "stance" => {
                let s = i32_at(cmd, 1).unwrap_or(0);
                self.action_stance(s, f);
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
                self.action_ground(OrderIndex::GroupPatrol, x, y, q, f);
            }
            "launch_patrol" => {
                let (Some(x), Some(y)) = (i32_at(cmd, 1), i32_at(cmd, 5)) else {
                    return;
                };
                let q = QueuePos::from_i64(i32_at(cmd, 9).unwrap_or(0) as i64);
                self.action_ground(OrderIndex::AirPatrol, x, y, q, f);
            }
            "scramble" => {
                let (who, list) = self.members();
                for o in list {
                    if f.alive(who, o) && f.can_move(who, o) {
                        let (x, y) = f.pos(who, o);
                        let ord = Order {
                            kind: OrderIndex::AirPatrol,
                            x,
                            y,
                            ..Order::default()
                        };
                        self.install(who, o, ord, QueuePos::New, f);
                    }
                }
            }
            "disband" => {
                let all = i32_at(cmd, 1).unwrap_or(0);
                self.action_disband(all != 0, f);
            }
            _ => {
                self.stats.unported += 1;
            }
        }
    }

    /// `Group::action_move_near` `0x00704990` (9,205 B, 23 call sites), order-installation
    /// spine only.
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
    /// **What this port does not do**: the 9 KB body is overwhelmingly *formation and
    /// destination* work — `Group::compute_form` `0x00707C80`, `Form::categorize`, the
    /// per-member `curr_x`/`curr_y` offsets, the `GROUP_MOVE` promotion when the group is
    /// marching, the garrison-into-transport branch, and the disembark handling. Those
    /// belong to the groups lane's `compute_form` / `update_positions`. Here every member
    /// gets the same destination and the `tolerance` the command carried.
    fn action_move_near(
        &mut self,
        x: i32,
        y: i32,
        tolerance: i32,
        q: QueuePos,
        orders: i64,
        f: &mut dyn Fleet,
    ) {
        let mut body = |a: &mut Action<'_>, q: QueuePos, f: &mut dyn Fleet| {
            let kind = move_order_kind(orders);
            let (who, list) = a.members();
            for o in list {
                if !f.alive(who, o) || !f.can_move(who, o) {
                    continue;
                }
                let ord = Order {
                    kind,
                    x,
                    y,
                    tolerance,
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
    fn the_receiver_split_is_34_group_11_leader_2_unit_7_game() {
        let count = |r: Receiver| OPCODES.iter().filter(|d| d.receiver == r).count();
        assert_eq!(count(Receiver::Group), 34);
        assert_eq!(count(Receiver::Leader), 11);
        assert_eq!(count(Receiver::Unit), 2);
        assert_eq!(count(Receiver::Game), 7);
        assert_eq!(count(Receiver::None), 28);
        assert_eq!(34 + 11 + 2 + 7 + 28, NUM_OPCODES);
        // ALARM / UNITMASK / BUILDMASK are group-scoped, not player-scoped: their
        // handlers call Group::action_*, exactly like MOVE_TO's does.
        for op in [27usize, 32, 33] {
            assert!(OPCODES[op].is_group_action(), "opcode {op}");
        }
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
        let ord = *f.orders(1, 0).unwrap().current().unwrap();
        assert_eq!(ord.kind, OrderIndex::FleeTo);
        assert_eq!(ord.tolerance, 48);
        b.process_all(&mut p, &build::move_to(10, 20, QueuePos::New, 2), &mut f)
            .unwrap();
        let ord = *f.orders(1, 0).unwrap().current().unwrap();
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
        f.orders_mut(1, 0).unwrap().push(Order::move_to(1, 2, 0));
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
        let ord = *f.orders(1, 0).unwrap().current().unwrap();
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
