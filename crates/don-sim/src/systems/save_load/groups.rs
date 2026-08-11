//! The `Groups` save owner: `Groups::walk_data` `0x00713E30` as a DoNSave section.
//!
//! Three functions define this section, and all three were read at instruction level
//! (`ron-bin/riseofnations.exe`, image base `0x00400000`, PDB GUID
//! `51D4F219-61C6-4F84-9D5B-C3361B0D291F`):
//!
//! ```text
//! Groups::walk_data          0x00713E30
//!     Array<Group>::walk_data(this + 0)        ; call 0x0047EA30 at 0x00713E39
//!     walk_test(int_str_array[0xE420/20 == 2920])   ; one tag byte
//!     walk(0x00E85F2C, 0x00E85F4C)             ; last_group[8]  (Groups+0x1C, 32 B)
//!     walk(0x00E85F50, 0x00E85F54)             ; proc_group     (Groups+0x40,  4 B)
//!
//! Array<Group>::walk_data    0x0047EA30       ; specialised on the `groups` global
//!     walk(&tmp, &tmp+4)      tmp = [0x00E85F14]        ; length
//!     if (length == 0) return
//!     walk(&tmp, &tmp+4)      tmp = [0x00E85F18]        ; size (capacity)
//!     walk(0x00E85F1C, 0x00E85F1E)                      ; increment (u16)
//!     walk(&tmp, &tmp+1)      tmp = [0x00E85F24] & 0xBF ; flags, bit 6 cleared
//!     for i in 0..length: Group::walk_data([0x00E85F20] + i*0x9D4)
//!
//! Group::walk_data           0x00708400
//!     walk(this+0x004, this+0x04C)                      ; 72-byte scalar header
//!     if (this->num != 0) {                             ; num is this+0x0C
//!         walk(this+0x8CC, this + (num+0x466)*2)        ; list[0..num]   i16
//!         walk(this+0x04C, this + (num+0x013)*4)        ; off_x[0..num]  i32
//!         walk(this+0x24C, this + (num+0x093)*4)        ; off_y[0..num]  i32
//!         walk(this+0x44C, this + (num+0x113)*4)        ; curr_x[0..num] i32
//!         walk(this+0x64C, this + (num+0x193)*4)        ; curr_y[0..num] i32
//!         walk(this+0x84C, this + 0x84C + num)          ; angles[0..num] i8
//!     }
//! ```
//!
//! `?groups@@3VGroups@@A` is at `0x00E85F10` (`schema/rise-symbols.tsv`), so the two
//! absolute ranges above are `Groups+0x1C` and `Groups+0x40`, which
//! [`crate::systems::groups_guys::Groups`] models as `last_group` and `proc_group`.
//!
//! ## What this section does and does not carry
//!
//! * **The 72-byte scalar header is written as the retail image, verbatim.** The bytes
//!   this section emits per group are exactly `GroupData::header_bytes()`, i.e. exactly
//!   what `Group::walk_data`'s first `walk` hands the visitor. That is deliberate: it makes
//!   the section byte-comparable with the `groups` checksum channel's own input instead of
//!   inventing a second field order.
//! * **Member arrays are written for `[0, num)` only,** because that is the entire extent
//!   retail's own walk covers. Slots at or past `num` are reconstructed as zero on load,
//!   which is sound rather than lossy: `Group::add` `0x00714350` writes
//!   `off_x[num] = off_y[num] = curr_x[num] = curr_y[num] = angles[num] = 0` **before**
//!   `list[num] = o; num++`, so no reader can ever observe a stale tail slot. The one
//!   excursion past `num` in the shipped code is `Group::update_positions`, whose loop
//!   bound is `form_num` and which only *writes* `curr_x`/`curr_y` at those indices; those
//!   writes feed nothing, since every consumer of the parallel arrays is bounded by `num`.
//!   `crates/don-sim/tests/save_load_groups.rs` drives that case explicitly.
//! * **`Array<Group>`'s `size` `[0x00E85F18]`, `increment` `[0x00E85F1C]` and `flags`
//!   `[0x00E85F24]` are not carried,** because `don-sim` does not model them:
//!   `groups_guys::Groups::list` is a fixed 512-slot `Vec`, not a growing `Array<Group>`.
//!   The `length` word is carried and pinned to `NUM_GROUPS`. This is a typed boundary, not
//!   a claim that the three words are inert — they are in the retail save stream and a
//!   future `.svx` writer needs them. They are, however, provably *not* in the checksum:
//!   `CheckSums::check_groups` `0x00937530` addresses `[0x00E85F14]` and `[0x00E85F20]`
//!   only.
//! * **`proc_group` stays in the `CORE` section,** where it already had an owner and a
//!   range check, rather than being written twice. Retail walks it here; DoNSave walks it
//!   there. Named so the divergence is visible.

use crate::systems::groups_guys::{
    GroupData, Groups, GROUPS_PER_PLAYER, GROUP_MAX_MEMBERS, NUM_GROUPS,
};
use crate::tick::NUM_LEADERS;

use super::{Reader, SaveError, Writer};

/// `Group::walk_data`'s first range, `[this+0x04, this+0x4C)`.
const HEADER_BYTES: usize = 72;

/// Structural bounds every `GroupData` must satisfy to be written or accepted.
///
/// Deliberately *not* checked: whether a member index names a live object. Retail groups
/// legitimately name dead members — `Groups::process` `0x006FA210` revisits one slot index
/// per player per frame, so a group can carry a dead member for up to 64 frames, and
/// `Group::normalize` `0x00711540` treats a negative `list[i]` as a tombstone rather than
/// as corruption. A liveness check here would refuse states retail produces.
fn check_shape(g: &GroupData, what: &'static str) -> Result<(), SaveError> {
    if !(0..=GROUP_MAX_MEMBERS as i32).contains(&g.num) {
        return Err(SaveError::Invalid(what));
    }
    if !(0..=GROUP_MAX_MEMBERS as i32).contains(&g.form_num) {
        return Err(SaveError::Invalid(what));
    }
    if usize::from(g.who) >= NUM_LEADERS {
        return Err(SaveError::Invalid(what));
    }
    Ok(())
}

/// Whether the live `Groups` can be written at all.
///
/// Called from `reject_unsupported`, so that a `Sim` whose group pool is structurally
/// impossible fails before any byte is produced, exactly like the other sections.
pub(super) fn validate(groups: &Groups) -> Result<(), SaveError> {
    if groups.list.len() != NUM_GROUPS {
        return Err(SaveError::Invalid("group pool cardinality"));
    }
    if !(0..GROUPS_PER_PLAYER as i32).contains(&groups.proc_group) {
        return Err(SaveError::Invalid("groups proc_group"));
    }
    for g in &groups.list {
        check_shape(g, "group slot shape")?;
    }
    Ok(())
}

/// `Groups::walk_data` `0x00713E30` as a DoNSave section.
pub(super) fn write(groups: &Groups) -> Result<Vec<u8>, SaveError> {
    validate(groups)?;
    let mut w = Writer::default();
    // `Array<Group>::walk_data`'s length word. `size`/`increment`/`flags` have no model
    // here; see the module header.
    w.len(groups.list.len(), "group pool cardinality")?;
    for g in &groups.list {
        w.bytes(&g.header_bytes());
        let n = g.num as usize;
        if n == 0 {
            continue;
        }
        // Retail's order, which is not the declaration order: `list` precedes the offsets.
        for &o in &g.list[..n] {
            w.i16(o);
        }
        for arr in [&g.off_x, &g.off_y, &g.curr_x, &g.curr_y] {
            for &v in &arr[..n] {
                w.i32(v);
            }
        }
        for &a in &g.angles[..n] {
            w.i8(a);
        }
    }
    // `walk(0x00E85F2C, 0x00E85F4C)` — `Groups+0x1C`, the eight `last_group` dwords that
    // `check_groups` reaches through the `const_last_group` pointer at `Groups+0x3C`.
    for &v in &groups.last_group {
        w.i32(v);
    }
    Ok(w.0)
}

/// The inverse of [`write`]. `proc_group` is the caller's, from `CORE`.
pub(super) fn read(data: &[u8], proc_group: i32) -> Result<Groups, SaveError> {
    let mut r = Reader::new(data);
    if r.len(NUM_GROUPS, "group pool cardinality")? != NUM_GROUPS {
        return Err(SaveError::Invalid("group pool cardinality"));
    }
    let mut list = Vec::with_capacity(NUM_GROUPS);
    for _ in 0..NUM_GROUPS {
        let mut g = read_header(&mut r)?;
        check_shape(&g, "group slot shape")?;
        let n = g.num as usize;
        if n != 0 {
            for slot in &mut g.list[..n] {
                *slot = r.i16()?;
            }
            for arr in [&mut g.off_x, &mut g.off_y, &mut g.curr_x, &mut g.curr_y] {
                for slot in &mut arr[..n] {
                    *slot = r.i32()?;
                }
            }
            for slot in &mut g.angles[..n] {
                *slot = r.i8()?;
            }
        }
        list.push(g);
    }
    let mut last_group = [0i32; NUM_LEADERS];
    for v in &mut last_group {
        *v = r.i32()?;
    }
    r.finish()?;
    let groups = Groups {
        list,
        last_group,
        proc_group,
    };
    validate(&groups)?;
    Ok(groups)
}

/// Decode the 72 bytes `Group::walk_data` writes from `this+0x04`.
///
/// The inverse of `GroupData::header_bytes`; the round-trip is asserted against that
/// function rather than restated, so the two cannot drift apart silently.
fn read_header(r: &mut Reader<'_>) -> Result<GroupData, SaveError> {
    let raw = r.take(HEADER_BYTES)?;
    let word = |i: usize| i32::from_le_bytes(raw[i * 4..i * 4 + 4].try_into().unwrap());
    Ok(GroupData {
        id: word(0),
        army: word(1),
        num: word(2),
        form: word(3),
        stamp: word(4),
        ox: word(5),
        oy: word(6),
        o_dist: word(7),
        o_angle: word(8),
        disband: word(9),
        order_num: word(10),
        priority: word(11),
        role: word(12),
        think_frame: word(13),
        new_speed: word(14),
        speed: word(15),
        form_num: word(16),
        facing: raw[68],
        buildings: raw[69],
        who: raw[70],
        march: raw[71],
        ..GroupData::default()
    })
}
