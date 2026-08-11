//! `groups` checksum channel (`CheckSums::check_groups`, `0x00937530`).
//!
//! # Why this channel could be closed and its neighbours could not
//!
//! Across the 21 checksum-bearing recordings, `units`, `builds`, `guys`, `leaders`,
//! `cities`, `goods`, `items` and `world` each carry **21 distinct** values on their first
//! checksummed turn: their initial state depends on the game setup. `groups` carries only
//! 15, and one of them — `0x1c78f3f5` — occurs in exactly the **seven recordings with no
//! computer players**, the same seven that survive on `scenario_data` and `script_run_time`.
//! Setup-independence is the signature that made `ScenarioFuncSet::init` derivable, and it
//! is why this module exists.
//!
//! # The traversal, read off the instruction stream
//!
//! `CheckSums::check_groups` `0x00937530` is 109 bytes and does two things:
//!
//! ```text
//! if (groups.list.count > 0)                 ; [0x00e85f14] = ArrayBase<Group>::count
//!     for each group: Group::walk_data(cs)   ; 0x00708400
//! adler = cs->checksum;                      ; CheckSum +0x10
//! for (i = 0; i < 0x20; i += 4)              ; 32 bytes, one dword at a time
//!     adler = adler32(adler, (const int*)groups.const_last_group + i, 4);
//! cs->checksum = adler;                      ; +0x10 only
//! ```
//!
//! Two measured details that are easy to lose:
//!
//! * the tail is hashed through `adler32` **directly** — `0x005089d0`, the second of the
//!   binary's two byte-identical copies of the routine, the other being the `0x00a46830`
//!   one the oracle pins — and it writes back only `CheckSum+0x10`. It never touches
//!   `+0x14`, so **retail's own byte counter under-reports this channel by 32**. See
//!   [`RETAIL_BYTE_COUNTER_SHORTFALL`].
//! * the tail base is the *value* at `groups+0x3c`, i.e. `GroupsData::const_last_group`,
//!   which `Groups::Groups` `0x00713ff0` points at `GroupsData::last_group` (`groups+0x1c`).
//!   `Groups::walk_data` `0x00713e30` — the SaveGame walker, a different function — walks
//!   the same `int[8]` by immediate address plus `proc_group`; the checksum walker does not
//!   walk `proc_group`.
//!
//! `Group::walk_data` `0x00708400` walks `[this+4, this+0x4c)` = **72 bytes**
//! unconditionally, and then, **only when `num != 0`**, six `num`-length member arrays in
//! the order `list`, `off_x`, `off_y`, `curr_x`, `curr_y`, `angles`. The generated table in
//! `walk_gen.rs` resolves the first op and marks the other six `Unresolved`, because their
//! ends are computed from `num` at run time; [`group_ops_agree_with_the_generated_table`]
//! pins this module's first op against the generated one so the hand-derived traversal
//! cannot drift from the extraction.
//!
//! # The initial state, read off the two initializers
//!
//! `Game::init` `0x0058c480` calls `Groups::clear` `0x00713f20` (so do `Game::run`
//! `0x00584590`, `Game::init_rules_and_teams` `0x00589bb0`, `Setup::build_game`
//! `0x005ac190` and `ScenarioEditor::generate_map` `0x009a0950`). `Groups::clear`:
//!
//! 1. forces the `Array<Group>` count to **`0x200` = 512**, growing the array through
//!    `Array<Group>::init` `0x0047e8a0` first if it holds fewer;
//! 2. calls `Group::clear(i)` `0x00713e80` on each slot in index order, whose stores are
//!    `id = i` (only when `i >= 0`), `army = -1`, `form = -1`, `stamp = *(Game+0x550)` and
//!    zero everywhere else in the walked window — all 72 bytes are written;
//! 3. re-zeros `+0x14` (`stamp`) and `+0x30` (`priority`) on each slot straight after, so
//!    the one non-constant store in `Group::clear` is erased. The loop bound is
//!    `0x13a800 = 512 * 0x9d4`, and `0x9d4 = 2516 = sizeof(Group)`;
//! 4. writes `last_group[8] = {0, 0x40, 0x80, 0xc0, 0x100, 0x140, 0x180, 0x1c0}` — 64
//!    group slots per player across eight players — and `proc_group = 0`.
//!
//! A cleared group has `num == 0`, so it walks exactly its 72-byte window.
//! `512 * 72 + 32 = 36,896` bytes, and the adler-32 over them is
//! [`CORPUS_INITIAL_GROUPS_CHANNEL`].
//!
//! # What agreement on this channel does and does not mean
//!
//! Every compare hands the visitor 36,896 real bytes, so an agreement here is **not**
//! empty-state agreement: an absent channel walks zero bytes and reads 1, and this reads
//! `0x1c78f3f5`. But the producer is **frozen at `Game::init`** exactly as
//! `scenario_data` is: nothing in `don-sim` drives `Groups::get_open_slot` `0x006fa460`,
//! `Groups::push_group` `0x0070f9e0` or any `Group::action_*`, so the claim it makes is
//! *"no group slot has been touched since `Game::init`"*. That is true from the start of
//! the game and false from the first group command onward, and the turn it stops matching
//! is the measurement. No value here is fitted: the state is the two initializers' stores
//! and nothing else, and the recorded wire value is never an input to the producer.

#![forbid(unsafe_code)]

use crate::checksum::{CheckSum, DataWalk};

/// `CheckSums::check_groups`.
pub const CHECK_GROUPS_VA: u32 = 0x0093_7530;
/// `Group::walk_data`.
pub const GROUP_WALK_DATA_VA: u32 = 0x0070_8400;
/// `Groups::clear` — the initializer this module reproduces.
pub const GROUPS_CLEAR_VA: u32 = 0x0071_3f20;
/// `Group::clear`.
pub const GROUP_CLEAR_VA: u32 = 0x0071_3e80;
/// `Groups::Groups` — points `const_last_group` at `last_group`.
pub const GROUPS_CTOR_VA: u32 = 0x0071_3ff0;
/// `Groups::walk_data`, the **SaveGame** walker. Not this traversal; recorded so the two
/// are not confused.
pub const GROUPS_WALK_DATA_VA: u32 = 0x0071_3e30;
/// The `Groups groups` global.
pub const GROUPS_GLOBAL_VA: u32 = 0x00e8_5f10;
/// The second copy of `adler32` in the binary, which the 32-byte tail calls directly.
pub const ADLER32_TAIL_VA: u32 = 0x0050_89d0;

/// `sizeof(Group)`, and the stride `Groups::clear`'s `0x13a800` bound divides by.
pub const GROUP_STRIDE: usize = 2516;
/// Slots `Groups::clear` forces the array to hold.
pub const GROUP_SLOTS: usize = 0x200;
/// `Group::walk_data`'s unconditional window, `[this+4, this+0x4c)`.
pub const GROUP_WALK_BEGIN: usize = 4;
pub const GROUP_WALK_END: usize = 0x4c;
pub const GROUP_WALK_BYTES: usize = GROUP_WALK_END - GROUP_WALK_BEGIN;
/// Capacity of every one of `GroupData`'s six member arrays.
pub const GROUP_MEMBER_CAPACITY: usize = 128;
/// `GroupsData::last_group` — `int[8]`, hashed through `const_last_group`.
pub const LAST_GROUP_COUNT: usize = 8;
/// Bytes of that tail.
pub const LAST_GROUP_BYTES: usize = LAST_GROUP_COUNT * 4;

/// `Groups::clear`'s eight `last_group` literals: `who * 0x40`.
pub const LAST_GROUP_INIT: [i32; LAST_GROUP_COUNT] =
    [0, 0x40, 0x80, 0xc0, 0x100, 0x140, 0x180, 0x1c0];

/// Bytes the derived initial state hands the visitor: `512 * 72 + 32`.
pub const INITIAL_WALKED_BYTES: u64 = (GROUP_SLOTS * GROUP_WALK_BYTES + LAST_GROUP_BYTES) as u64;

/// Bytes retail's own `CheckSum+0x14` counter misses on this channel, because the
/// `last_group` tail bypasses `CheckSum::walk_function` `0x00936ff0` and calls `adler32`
/// directly. The hash is unaffected; only the counter is.
pub const RETAIL_BYTE_COUNTER_SHORTFALL: u64 = LAST_GROUP_BYTES as u64;

/// The value the seven zero-AI recordings carry on their first checksummed turn.
///
/// A replay **target**, never an input: [`InitialGroupsChannel::derive`] computes its
/// value from `Groups::clear` and `Group::clear` and does not consult this constant.
pub const CORPUS_INITIAL_GROUPS_CHANNEL: u32 = 0x1c78_f3f5;

// ---------------------------------------------------------------------------
// GroupData offsets — the walked window
// ---------------------------------------------------------------------------

/// One named field of the 72-byte window, with its `GroupData` offset and width.
///
/// Kept as a table rather than as a struct with hand-placed writes so that
/// [`walked_window_covers_every_declared_field`] can prove the window is exactly the
/// declared fields with no hole and no overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowField {
    pub name: &'static str,
    pub offset: usize,
    pub size: usize,
}

/// `GroupData`'s fields inside `[4, 0x4c)`, in offset order
/// [`schema/types.json`, class `GroupData`].
pub const WINDOW_FIELDS: [WindowField; 21] = [
    WindowField { name: "id", offset: 4, size: 4 },
    WindowField { name: "army", offset: 8, size: 4 },
    WindowField { name: "num", offset: 12, size: 4 },
    WindowField { name: "form", offset: 16, size: 4 },
    WindowField { name: "stamp", offset: 20, size: 4 },
    WindowField { name: "ox", offset: 24, size: 4 },
    WindowField { name: "oy", offset: 28, size: 4 },
    WindowField { name: "o_dist", offset: 32, size: 4 },
    WindowField { name: "o_angle", offset: 36, size: 4 },
    WindowField { name: "disband", offset: 40, size: 4 },
    WindowField { name: "order_num", offset: 44, size: 4 },
    WindowField { name: "priority", offset: 48, size: 4 },
    WindowField { name: "role", offset: 52, size: 4 },
    WindowField { name: "think_frame", offset: 56, size: 4 },
    WindowField { name: "new_speed", offset: 60, size: 4 },
    WindowField { name: "speed", offset: 64, size: 4 },
    WindowField { name: "form_num", offset: 68, size: 4 },
    WindowField { name: "facing", offset: 72, size: 1 },
    WindowField { name: "buildings", offset: 73, size: 1 },
    WindowField { name: "who", offset: 74, size: 1 },
    WindowField { name: "march", offset: 75, size: 1 },
];

/// Offsets of the six `num`-length member arrays [`schema/types.json`, `GroupData`].
pub const OFF_X_OFFSET: usize = 76;
pub const OFF_Y_OFFSET: usize = 588;
pub const CURR_X_OFFSET: usize = 1100;
pub const CURR_Y_OFFSET: usize = 1612;
pub const ANGLES_OFFSET: usize = 2124;
pub const LIST_OFFSET: usize = 2252;

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

/// The checksum-visible scalar state of one `Group`: exactly the 21 fields inside
/// `Group::walk_data`'s unconditional window, no more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GroupWindow {
    pub id: i32,
    pub army: i32,
    pub num: i32,
    pub form: i32,
    pub stamp: i32,
    pub ox: i32,
    pub oy: i32,
    pub o_dist: i32,
    pub o_angle: i32,
    pub disband: i32,
    pub order_num: i32,
    pub priority: i32,
    pub role: i32,
    pub think_frame: i32,
    pub new_speed: i32,
    pub speed: i32,
    pub form_num: i32,
    pub facing: u8,
    pub buildings: u8,
    pub who: u8,
    pub march: u8,
}

impl GroupWindow {
    /// `Group::clear(id)` `0x00713e80` **as `Groups::clear` calls it**: the two stores
    /// `Groups::clear` repeats immediately afterwards (`+0x14` `stamp`, `+0x30`
    /// `priority`) are folded in, which is what erases `Group::clear`'s single
    /// non-constant store `stamp = *(Game+0x550)`.
    ///
    /// A `Group::clear` reached from anywhere else keeps that store; this constructor is
    /// deliberately named for the `Groups::clear` path and is not a general one.
    pub const fn cleared_by_groups_clear(id: i32) -> GroupWindow {
        GroupWindow {
            id,
            army: -1,
            num: 0,
            form: -1,
            stamp: 0,
            ox: 0,
            oy: 0,
            o_dist: 0,
            o_angle: 0,
            disband: 0,
            order_num: 0,
            priority: 0,
            role: 0,
            think_frame: 0,
            new_speed: 0,
            speed: 0,
            form_num: 0,
            facing: 0,
            buildings: 0,
            who: 0,
            march: 0,
        }
    }

    /// The 72 bytes `Group::walk_data`'s first op hands the visitor.
    pub fn bytes(&self) -> [u8; GROUP_WALK_BYTES] {
        let mut b = [0u8; GROUP_WALK_BYTES];
        let mut put = |off: usize, v: i32| {
            let o = off - GROUP_WALK_BEGIN;
            b[o..o + 4].copy_from_slice(&v.to_le_bytes());
        };
        put(4, self.id);
        put(8, self.army);
        put(12, self.num);
        put(16, self.form);
        put(20, self.stamp);
        put(24, self.ox);
        put(28, self.oy);
        put(32, self.o_dist);
        put(36, self.o_angle);
        put(40, self.disband);
        put(44, self.order_num);
        put(48, self.priority);
        put(52, self.role);
        put(56, self.think_frame);
        put(60, self.new_speed);
        put(64, self.speed);
        put(68, self.form_num);
        b[72 - GROUP_WALK_BEGIN] = self.facing;
        b[73 - GROUP_WALK_BEGIN] = self.buildings;
        b[74 - GROUP_WALK_BEGIN] = self.who;
        b[75 - GROUP_WALK_BEGIN] = self.march;
        b
    }
}

/// The six `num`-length member arrays, walked only when `num != 0`.
///
/// Borrowed rather than owned so that a caller with real group membership can hand its own
/// storage over without a copy, and so that the empty case costs nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GroupMembers<'a> {
    /// `short list[128]` at `+0x8cc` — walked **first**.
    pub list: &'a [i16],
    /// `int off_x[128]` at `+0x4c`.
    pub off_x: &'a [i32],
    /// `int off_y[128]` at `+0x24c`.
    pub off_y: &'a [i32],
    /// `Coord curr_x[128]` at `+0x44c`.
    pub curr_x: &'a [i32],
    /// `Coord curr_y[128]` at `+0x64c`.
    pub curr_y: &'a [i32],
    /// `char angles[128]` at `+0x84c`.
    pub angles: &'a [i8],
}

/// One `Group` as the checksum sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GroupRecord<'a> {
    pub window: GroupWindow,
    pub members: GroupMembers<'a>,
}

impl GroupRecord<'_> {
    /// A slot straight out of `Groups::clear`.
    pub const fn cleared(id: i32) -> GroupRecord<'static> {
        GroupRecord {
            window: GroupWindow::cleared_by_groups_clear(id),
            members: GroupMembers {
                list: &[],
                off_x: &[],
                off_y: &[],
                curr_x: &[],
                curr_y: &[],
                angles: &[],
            },
        }
    }
}

/// Why a supplied `Groups` state cannot be walked. Every arm is a refusal, never a repair:
/// a channel that guessed a length would produce a number that looked like agreement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupsChannelError {
    /// `num` is negative or larger than the 128-element member arrays.
    MemberCountOutOfRange { slot: usize, num: i32 },
    /// One of the six member arrays does not have exactly `num` elements.
    MemberLengthMismatch {
        slot: usize,
        array: &'static str,
        num: usize,
        supplied: usize,
    },
}

impl std::fmt::Display for GroupsChannelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GroupsChannelError::MemberCountOutOfRange { slot, num } => write!(
                f,
                "group {slot}: num = {num} is outside [0, {GROUP_MEMBER_CAPACITY}]"
            ),
            GroupsChannelError::MemberLengthMismatch {
                slot,
                array,
                num,
                supplied,
            } => write!(
                f,
                "group {slot}: num = {num} but {array} has {supplied} elements"
            ),
        }
    }
}

impl std::error::Error for GroupsChannelError {}

/// The result of one `check_groups`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupsChecksum {
    pub checksum: u32,
    /// Bytes handed to the visitor, including the 32-byte `last_group` tail.
    pub bytes_walked: u64,
    /// What retail's own `CheckSum+0x14` counter would read: `bytes_walked` minus the
    /// tail, which bypasses the counter.
    pub retail_byte_counter: u64,
    pub groups_walked: u32,
}

/// `Group::walk_data` `0x00708400` over one record.
fn walk_group<W: DataWalk>(
    slot: usize,
    rec: &GroupRecord<'_>,
    w: &mut W,
) -> Result<(), GroupsChannelError> {
    w.walk(&rec.window.bytes());
    let num = rec.window.num;
    if num == 0 {
        return Ok(());
    }
    if num < 0 || num as usize > GROUP_MEMBER_CAPACITY {
        return Err(GroupsChannelError::MemberCountOutOfRange { slot, num });
    }
    let num = num as usize;
    let m = &rec.members;
    let check = |array: &'static str, supplied: usize| -> Result<(), GroupsChannelError> {
        if supplied == num {
            Ok(())
        } else {
            Err(GroupsChannelError::MemberLengthMismatch {
                slot,
                array,
                num,
                supplied,
            })
        }
    };
    check("list", m.list.len())?;
    check("off_x", m.off_x.len())?;
    check("off_y", m.off_y.len())?;
    check("curr_x", m.curr_x.len())?;
    check("curr_y", m.curr_y.len())?;
    check("angles", m.angles.len())?;

    // The order is the instruction order at 0x00708438..0x007084ac: `list` first, then
    // the four int planes, then `angles`. Adler-32 is order-sensitive, so this is not a
    // cosmetic detail.
    let mut buf = Vec::with_capacity(num * 4);
    buf.clear();
    for v in m.list {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    w.walk(&buf);
    for plane in [m.off_x, m.off_y, m.curr_x, m.curr_y] {
        buf.clear();
        for v in plane {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        w.walk(&buf);
    }
    buf.clear();
    for v in m.angles {
        buf.push(*v as u8);
    }
    w.walk(&buf);
    Ok(())
}

/// `CheckSums::check_groups` `0x00937530` over a supplied `Groups` state.
///
/// `last_group` is walked through `GroupsData::const_last_group`, which the constructor
/// points at `last_group` itself; the caller supplies the pointee because the pointer is
/// not otherwise observable.
pub fn groups_checksum(
    groups: &[GroupRecord<'_>],
    last_group: &[i32; LAST_GROUP_COUNT],
) -> Result<GroupsChecksum, GroupsChannelError> {
    let mut cs = CheckSum::new();
    for (slot, rec) in groups.iter().enumerate() {
        walk_group(slot, rec, &mut cs)?;
    }
    let walked_by_counter = cs.bytes;
    // The tail: eight separate four-byte `adler32` calls, writing back only `+0x10`.
    for v in last_group {
        cs.checksum = crate::checksum::adler32(cs.checksum, &v.to_le_bytes());
    }
    Ok(GroupsChecksum {
        checksum: cs.checksum,
        bytes_walked: walked_by_counter + LAST_GROUP_BYTES as u64,
        retail_byte_counter: walked_by_counter,
        groups_walked: groups.len() as u32,
    })
}

// ---------------------------------------------------------------------------
// The derived initial state
// ---------------------------------------------------------------------------

/// The `Groups` state a retail `Game::init` leaves behind: 512 slots cleared by
/// `Groups::clear` `0x00713f20` and the eight `last_group` literals it writes.
#[derive(Debug, Clone)]
pub struct RetailInitialGroups {
    slots: Vec<GroupRecord<'static>>,
    last_group: [i32; LAST_GROUP_COUNT],
}

impl Default for RetailInitialGroups {
    fn default() -> Self {
        Self::new()
    }
}

impl RetailInitialGroups {
    pub fn new() -> RetailInitialGroups {
        RetailInitialGroups {
            slots: (0..GROUP_SLOTS)
                .map(|i| GroupRecord::cleared(i as i32))
                .collect(),
            last_group: LAST_GROUP_INIT,
        }
    }

    pub fn slots(&self) -> &[GroupRecord<'static>] {
        &self.slots
    }

    pub fn last_group(&self) -> &[i32; LAST_GROUP_COUNT] {
        &self.last_group
    }
}

/// Channel 5 as `Game::init` leaves it, ready to install on a [`crate::state::SimState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InitialGroupsChannel {
    pub checksum: u32,
    pub bytes_walked: u64,
    pub retail_byte_counter: u64,
    pub slots: u32,
}

impl Default for InitialGroupsChannel {
    fn default() -> Self {
        Self::derive()
    }
}

impl InitialGroupsChannel {
    /// Build the derived state and walk it. Infallible by construction: every slot has
    /// `num == 0` and therefore no member arrays to disagree with.
    pub fn derive() -> InitialGroupsChannel {
        let owner = RetailInitialGroups::new();
        let c = groups_checksum(owner.slots(), owner.last_group())
            .expect("cleared slots have num == 0 and no member arrays");
        InitialGroupsChannel {
            checksum: c.checksum,
            bytes_walked: c.bytes_walked,
            retail_byte_counter: c.retail_byte_counter,
            slots: c.groups_walked,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walk::WalkOp;

    /// The single 32-bit statement this module exists to make. The state is the two
    /// initializers' stores; the target is not consulted while computing it.
    #[test]
    fn the_derived_game_init_groups_state_is_the_value_retail_carries() {
        let ch = InitialGroupsChannel::derive();
        assert_eq!(ch.slots, GROUP_SLOTS as u32);
        assert_eq!(ch.bytes_walked, INITIAL_WALKED_BYTES);
        assert_eq!(ch.bytes_walked, 36_896);
        assert_eq!(
            ch.retail_byte_counter,
            ch.bytes_walked - RETAIL_BYTE_COUNTER_SHORTFALL
        );
        assert_eq!(
            ch.checksum, CORPUS_INITIAL_GROUPS_CHANNEL,
            "derived {:#010x}, recorded {:#010x}",
            ch.checksum, CORPUS_INITIAL_GROUPS_CHANNEL
        );
    }

    /// The window is exactly `GroupData`'s declared fields between 4 and 0x4c: no hole,
    /// no overlap, no padding. If the offsets in [`WINDOW_FIELDS`] ever drift, the sum
    /// stops being 72 and this fails before the checksum does.
    #[test]
    fn walked_window_covers_every_declared_field() {
        let mut cursor = GROUP_WALK_BEGIN;
        for f in WINDOW_FIELDS {
            assert_eq!(f.offset, cursor, "{} is not contiguous", f.name);
            cursor += f.size;
        }
        assert_eq!(cursor, GROUP_WALK_END);
        assert_eq!(
            WINDOW_FIELDS.iter().map(|f| f.size).sum::<usize>(),
            GROUP_WALK_BYTES
        );
    }

    /// This module's traversal and the generated extraction must agree where they
    /// overlap. The generated table resolves op 0 and marks the six `num`-gated array
    /// walks `Unresolved`; that is the exact division of labour, asserted rather than
    /// described.
    #[test]
    fn group_ops_agree_with_the_generated_table() {
        let k = crate::walk_gen::class_index("Group").expect("Group is in the generated table");
        let spec = &crate::walk_gen::SPECS[k];
        assert_eq!(spec.walk_data_va, GROUP_WALK_DATA_VA);
        assert_eq!(spec.sizeof as usize, GROUP_STRIDE);
        assert_eq!(spec.walked_bytes as usize, GROUP_WALK_BYTES);
        assert_eq!(
            spec.ops[0],
            WalkOp::Bytes {
                begin: GROUP_WALK_BEGIN as u32,
                end: GROUP_WALK_END as u32
            }
        );
        assert_eq!(spec.ops.len(), 7);
        assert!(
            spec.ops[1..].iter().all(|o| *o == WalkOp::Unresolved),
            "the six num-gated member walks are the ones this module resolves"
        );
    }

    /// A cleared slot walks its window and nothing else; the member arrays are gated on
    /// `num != 0` and a cleared group has `num == 0`.
    #[test]
    fn a_cleared_slot_walks_seventy_two_bytes() {
        let rec = GroupRecord::cleared(7);
        let mut cs = CheckSum::new();
        walk_group(0, &rec, &mut cs).unwrap();
        assert_eq!(cs.bytes, GROUP_WALK_BYTES as u64);
        assert_eq!(rec.window.id, 7);
        assert_eq!(rec.window.army, -1);
        assert_eq!(rec.window.form, -1);
        assert_eq!(rec.window.stamp, 0, "Groups::clear re-zeros +0x14");
        assert_eq!(rec.window.priority, 0, "Groups::clear re-zeros +0x30");
    }

    /// `id = i` is a store, not a convention: changing it changes the channel. This is
    /// the bite test for the one field `Groups::clear` varies across slots.
    #[test]
    fn the_slot_index_reaches_the_channel() {
        let owner = RetailInitialGroups::new();
        let baseline = groups_checksum(owner.slots(), owner.last_group()).unwrap();
        let all_zero: Vec<GroupRecord<'static>> =
            (0..GROUP_SLOTS).map(|_| GroupRecord::cleared(0)).collect();
        let flat = groups_checksum(&all_zero, owner.last_group()).unwrap();
        assert_eq!(flat.bytes_walked, baseline.bytes_walked);
        assert_ne!(
            flat.checksum, baseline.checksum,
            "Group::clear's `id = i` store must be visible to the checksum"
        );
    }

    /// The `last_group` tail is part of the value, and each of its eight entries is.
    #[test]
    fn the_last_group_tail_reaches_the_channel() {
        let owner = RetailInitialGroups::new();
        let baseline = groups_checksum(owner.slots(), owner.last_group()).unwrap();
        for i in 0..LAST_GROUP_COUNT {
            let mut tail = LAST_GROUP_INIT;
            tail[i] += 1;
            let moved = groups_checksum(owner.slots(), &tail).unwrap();
            assert_ne!(
                moved.checksum, baseline.checksum,
                "last_group[{i}] did not reach the channel"
            );
            assert_eq!(moved.bytes_walked, baseline.bytes_walked);
        }
    }

    /// Slot count is not a free parameter either: 511 or 513 slots is a different
    /// channel, so `Groups::clear`'s `0x200` is doing real work.
    #[test]
    fn the_slot_count_reaches_the_channel() {
        let owner = RetailInitialGroups::new();
        let baseline = groups_checksum(owner.slots(), owner.last_group()).unwrap();
        for n in [GROUP_SLOTS - 1, GROUP_SLOTS + 1] {
            let slots: Vec<GroupRecord<'static>> =
                (0..n).map(|i| GroupRecord::cleared(i as i32)).collect();
            let other = groups_checksum(&slots, owner.last_group()).unwrap();
            assert_ne!(other.checksum, baseline.checksum, "{n} slots");
        }
    }

    /// `Groups::clear`'s loop bound `0x13a800` is `GROUP_SLOTS * sizeof(Group)`. Two
    /// numbers read from two different places in the same function agreeing is the
    /// cheapest available check that neither was transcribed wrong.
    #[test]
    fn the_clear_loop_bound_is_the_slot_count_times_the_stride() {
        assert_eq!(GROUP_SLOTS * GROUP_STRIDE, 0x13_a800);
    }

    /// `last_group[who] = who * 0x40`, and `8 * 0x40` is the slot count.
    #[test]
    fn last_group_partitions_the_slots_evenly_across_eight_players() {
        for (who, v) in LAST_GROUP_INIT.iter().enumerate() {
            assert_eq!(*v, (who as i32) * 0x40);
        }
        assert_eq!(LAST_GROUP_COUNT * 0x40, GROUP_SLOTS);
    }

    /// A populated group walks its members in the instruction order, and a caller that
    /// supplies inconsistent lengths is refused rather than padded.
    #[test]
    fn a_populated_group_walks_its_members_and_fails_closed() {
        let list = [3i16, 5];
        let off = [10i32, 20];
        let ang = [1i8, -1];
        let rec = GroupRecord {
            window: GroupWindow {
                num: 2,
                ..GroupWindow::cleared_by_groups_clear(0)
            },
            members: GroupMembers {
                list: &list,
                off_x: &off,
                off_y: &off,
                curr_x: &off,
                curr_y: &off,
                angles: &ang,
            },
        };
        let mut cs = CheckSum::new();
        walk_group(0, &rec, &mut cs).unwrap();
        // 72 + list 2*2 + four int planes 4*(2*4) + angles 2
        assert_eq!(cs.bytes, (GROUP_WALK_BYTES + 4 + 32 + 2) as u64);

        let short = [10i32];
        let bad = GroupRecord {
            members: GroupMembers {
                off_x: &short,
                ..rec.members
            },
            ..rec
        };
        assert_eq!(
            walk_group(4, &bad, &mut CheckSum::new()),
            Err(GroupsChannelError::MemberLengthMismatch {
                slot: 4,
                array: "off_x",
                num: 2,
                supplied: 1
            })
        );

        let oversize = GroupRecord {
            window: GroupWindow {
                num: (GROUP_MEMBER_CAPACITY + 1) as i32,
                ..rec.window
            },
            ..rec
        };
        assert_eq!(
            walk_group(9, &oversize, &mut CheckSum::new()),
            Err(GroupsChannelError::MemberCountOutOfRange {
                slot: 9,
                num: 129
            })
        );
    }

    /// Eight four-byte `adler32` calls and one 32-byte call are the same function of the
    /// same bytes; the chunking only moves where the modulus is applied. Asserted because
    /// the traversal transcribes the four-at-a-time form.
    #[test]
    fn the_tail_chunking_does_not_change_the_value() {
        let mut chunked = 1u32;
        for v in LAST_GROUP_INIT {
            chunked = crate::checksum::adler32(chunked, &v.to_le_bytes());
        }
        let flat: Vec<u8> = LAST_GROUP_INIT
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        assert_eq!(chunked, crate::checksum::adler32(1, &flat));
    }
}
