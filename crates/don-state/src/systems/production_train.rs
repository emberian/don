//! Production completion: the tail of `Build::do_queue` 0x0061e410 once a
//! `QueueItem` has reached `construct_time`, `Build::finished` 0x00628490,
//! `Build::unqueue` 0x006207c0, `Build::train` 0x0062f9b0,
//! `Objects::init_unit` 0x0065e0c0 / `Objects::find_free` 0x0065ad60 and the
//! `Unit::init` 0x00612100 write set (through `Object::init` 0x00647750,
//! `SubObject::init` 0x00662300, `SubObject::set_type` 0x00662450,
//! `Unit::set_type` 0x00612fa0 and `Leader::track_unit_type` 0x006e0dd0).
//!
//! Leaf module: `build_process::do_queue` owns the per-frame rate/clamp and
//! calls [`complete_queue_item`] when `total <= job_counter`; nothing here
//! edits sibling modules.
//!
//! Status `Partial`. Transcribed and executed:
//!
//! - do_queue completion branch (0x0061ebc5..0x0061ed3b): `job_counter =
//!   total`, repeat-latch eligibility (unit type + Leader known-bit),
//!   `finished` dispatch, `unqueue(slot, 0)`, the blocked-item fallbacks are
//!   reported (they recurse into `do_queue`, which is the owner's).
//! - `Build::finished` unit arm: known-bit gate, population gate
//!   (`Leader+0x7e4 < UnitType+0x2f0 + Leader+0x940` → 0), caravan/airbase
//!   limits reported, then `train`.
//! - `Build::unqueue(slot, refund=0)`: `queue[slot].job_counter = 0`,
//!   `Leader+0x5a22+type*2` queued-per-type decrement, the queued-military
//!   counters `Leader+0xa10..+0xa24`, age/epoch queued bytes `+0x67f4/+0x67f5`,
//!   `BuildQueue::un_queue` row shift, `queued -= 1`, `build_masks &= ~0x40`
//!   on empty.
//! - `Build::train`: `init_unit(who, type, x, y, -1, -1, -1)`,
//!   `Leader+0x61ac+type*4 = o`, stance inheritance (`Unit::set_stance
//!   0x00605310` when the type stance classes match), `unit_masks &=
//!   ~0x4000000`.
//! - `Objects::init_unit` / `find_free`: slot reuse scan (`!(flags&1) &&
//!   hold_frames == 0 && o_up < 0`), allocation at `unit_mark` (+1), batch
//!   (`UnitType+0x308`) captain/follower linkage `o_up`/`o_down`, follower
//!   population undo.
//! - `Unit::init` scalar writes on the SubObject/Object/Unit images, uid =
//!   `Objects.obj_ctr[who]++`, `Game+0x6d8 += 1`, Leader `flags |= 0x800000`,
//!   `+0x808 += 1` (pop-cost types), `Unit::set_type(type, 1)`'s
//!   `track_unit_type(type, +1, o)` (`+0x56fe+type*2`, `+0x978/+0x97c/+0x988`,
//!   military `+0x9f8..+0xa0c`), `+0x940 += pop`, `+0x93c += 1`, caravans
//!   `+0x980`.
//!
//! Not transcribed (fields left at their init values, each reported in
//! `effects`): `Unit::go_inside` 0x0061a2e0 → `Unit::come_out` 0x00617c10
//! (placement at the rally/exit — `set_new_location` 0x005f8d20,
//! `Object::add_to_world` 0x0064d8c0 tile list `down/down_who`, z from
//! `World::get_height` 0x008544a0, `orders_x/y`); `Guy::init_real`
//! 0x005db6b0 (the 155-byte Guy rows); the derived stats `Unit::update_hits`
//! 0x0060e930 (`myhits`), `update_los` 0x0060e4d0 (`mylos`), `update_speed`
//! 0x006055c0 (`myspeed`), `update_armor` 0x006054c0; `Build::queue_up`
//! 0x00620f40 (infinite-queue re-enqueue); `Leader::gain_tech` 0x006dcb60
//! (Library research completion and unknown-unit-type upgrades — the
//! `research.rs` lane); AIRCRAFTCARRIER escort spawns.
//!
//! RNG (main LCG `[0x00c06184]`, `Random::get` 0x00a39d70): none of the
//! transcribed writes draw. The untranscribed callees draw: `Guy::init_real`
//! one `get(0, 0xffff) % 100` per Guy (variant 0/1/2/3 at <70/<80/<90/<100),
//! `Unit::come_out` tail one `get(0, 0xffff)` for AI-owned (`unit_masks &
//! 0x40000`) non-caravan units (0x0061a1bb / 0x0061a1d5, add_to_army coin).
//! So a retail Citizen spawn consumes 2 draws this module does not yet
//! perform — `complete_queue_item` reports `rng_draws_unaccounted`.

#![allow(dead_code)]

use crate::sections::{section_tag, Obj, ObjList, Save, Unit};
use crate::tick::StepStatus;

pub const STATUS: StepStatus = StepStatus::Partial;

// ---------------------------------------------------------------------------
// Global readers / writers
// ---------------------------------------------------------------------------

fn frame(save: &Save) -> i32 {
    i32::from_le_bytes(save.game.scalars[0..4].try_into().unwrap())
}

/// `Game+off` inside the walked `Game.scalars` (+0x550..+0x6e4).
fn game_i32(save: &Save, off: usize) -> i32 {
    let i = off - 0x550;
    i32::from_le_bytes(save.game.scalars[i..i + 4].try_into().unwrap())
}

fn set_game_i32(save: &mut Save, off: usize, v: i32) {
    let i = off - 0x550;
    save.game.scalars[i..i + 4].copy_from_slice(&v.to_le_bytes());
}

fn leader_flags(save: &Save, who: usize) -> i32 {
    save.leaders.slots.get(who).map(|l| l.flags).unwrap_or(0)
}

fn leader_u8(save: &Save, who: usize, off: usize) -> Option<u8> {
    let l = save.leaders.slots.get(who)?;
    Some(match off {
        0..=3 => l.flags.to_le_bytes()[off],
        4..=7 => l.flags2.to_le_bytes()[off - 4],
        _ => *l.body.get(off - 8)?,
    })
}

fn leader_i16(save: &Save, who: usize, off: usize) -> Option<i16> {
    Some(i16::from_le_bytes([leader_u8(save, who, off)?, leader_u8(save, who, off + 1)?]))
}

fn leader_i32(save: &Save, who: usize, off: usize) -> Option<i32> {
    Some(i32::from_le_bytes([
        leader_u8(save, who, off)?,
        leader_u8(save, who, off + 1)?,
        leader_u8(save, who, off + 2)?,
        leader_u8(save, who, off + 3)?,
    ]))
}

fn set_leader_bytes(save: &mut Save, who: usize, off: usize, bytes: &[u8]) -> bool {
    let Some(l) = save.leaders.slots.get_mut(who) else { return false };
    if off < 8 || off - 8 + bytes.len() > l.body.len() {
        return false;
    }
    l.body[off - 8..off - 8 + bytes.len()].copy_from_slice(bytes);
    true
}

fn leader_add_i32(save: &mut Save, who: usize, off: usize, d: i32, tag: &str, effects: &mut Vec<String>) {
    if let Some(v) = leader_i32(save, who, off) {
        let nv = v.wrapping_add(d);
        set_leader_bytes(save, who, off, &nv.to_le_bytes());
        effects.push(format!("Leader[{who}]+{off:#x} {v} -> {nv} ({tag})"));
    }
}

fn leader_add_i16(save: &mut Save, who: usize, off: usize, d: i16, tag: &str, effects: &mut Vec<String>) {
    if let Some(v) = leader_i16(save, who, off) {
        let nv = v.wrapping_add(d);
        set_leader_bytes(save, who, off, &nv.to_le_bytes());
        effects.push(format!("Leader[{who}]+{off:#x} {v} -> {nv} ({tag})"));
    }
}

fn leader_add_u8(save: &mut Save, who: usize, off: usize, d: i8, tag: &str, effects: &mut Vec<String>) {
    if let Some(v) = leader_u8(save, who, off) {
        let nv = v.wrapping_add(d as u8);
        set_leader_bytes(save, who, off, &[nv]);
        effects.push(format!("Leader[{who}]+{off:#x} {v} -> {nv} ({tag})"));
    }
}

fn leader_or_flags(save: &mut Save, who: usize, bits: i32, tag: &str, effects: &mut Vec<String>) {
    if let Some(l) = save.leaders.slots.get_mut(who) {
        if l.flags & bits != bits {
            let old = l.flags;
            l.flags |= bits;
            effects.push(format!("Leader[{who}].flags {old:#x} -> {:#x} ({tag})", l.flags));
        }
    }
}

/// `LeaderData+0x6c18` known-type bitmask (`DAT_00e40fa8`).
fn known_bit(save: &Save, who: usize, ty: i32) -> bool {
    ty >= 0 && leader_u8(save, who, 0x6c18 + (ty >> 3) as usize).map(|b| b & (1 << (ty & 7)) != 0).unwrap_or(false)
}

/// `Objects` scalar block: `[0x1f4..0x1fc) [0x154..0x15c) unit_mark[9]
/// build_mark[9] wall_mark[9] obj_ctr[9]u16` (Objects::walk_data 0x006541e0).
fn unit_mark(save: &Save, who: usize) -> i32 {
    let o = 16 + who * 4;
    i32::from_le_bytes(save.objects.scalars[o..o + 4].try_into().unwrap())
}

fn set_unit_mark(save: &mut Save, who: usize, v: i32) {
    let o = 16 + who * 4;
    save.objects.scalars[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

fn obj_ctr(save: &Save, who: usize) -> u16 {
    let o = 16 + 108 + who * 2;
    u16::from_le_bytes([save.objects.scalars[o], save.objects.scalars[o + 1]])
}

fn set_obj_ctr(save: &mut Save, who: usize, v: u16) {
    let o = 16 + 108 + who * 2;
    save.objects.scalars[o..o + 2].copy_from_slice(&v.to_le_bytes());
}

fn constant(save: &Save, off: usize) -> i32 {
    i32::from_le_bytes(save.constants[off..off + 4].try_into().unwrap())
}

// ---------------------------------------------------------------------------
// Type table readers (Rules.types[idx] image: head 0x04..0x5e, obj_mid
// 0x1e4..0x27c, ext 0x2b4..)
// ---------------------------------------------------------------------------

fn type_i32(save: &Save, idx: i32, off: usize) -> Option<i32> {
    let t = save.rules_tail.rules.types.get(usize::try_from(idx).ok()?)?;
    let (v, i) = match off {
        0x04..=0x5d => (&t.head, off - 4),
        0x1e4..=0x27b => (&t.obj_mid, off - 0x1e4),
        0x2b4.. => (&t.ext, off - 0x2b4),
        _ => return None,
    };
    v.get(i..i + 4).map(|b| i32::from_le_bytes(b.try_into().unwrap()))
}

fn is_unit_type(idx: i32) -> bool {
    (0x32..=0x19d).contains(&idx)
}
fn is_building_type(idx: i32) -> bool {
    (0x19e..=0x21e).contains(&idx)
}
/// `TypeData::is_age_type` 0x00470820 / `is_epoch_type` 0x00470870 /
/// `is_spell_type` 0x00470590 default bodies.
fn is_age_type(idx: i32) -> bool {
    (0x220..=0x226).contains(&idx)
}
fn is_epoch_type(idx: i32) -> bool {
    (0x227..=0x242).contains(&idx)
}
fn is_spell_type(idx: i32) -> bool {
    (0x275..=0x2ab).contains(&idx)
}
/// `TypeData::is_peasant` 0x0042db70 / `is_scholar` 0x0042db90.
fn is_peasant(idx: i32) -> bool {
    idx == 0x32 || idx == 0x33
}
fn is_scholar(idx: i32) -> bool {
    idx == 0x34 || idx == 0x35
}

/// `ObjectTypeData::is_slow` 0x00661ae0 (`strict == 0` arm).
fn type_is_slow(save: &Save, this: i32, what: i32, depth: u8) -> bool {
    if this == what {
        return true;
    }
    if what < 0 || depth > 16 {
        return false;
    }
    if type_i32(save, this, 0x25c) == Some(what) {
        return true;
    }
    match type_i32(save, this, 0x3c) {
        Some(from) if from >= 0 => type_is_slow(save, from, what, depth + 1),
        _ => false,
    }
}

/// `ObjectTypeData::is` 0x0065f7d0 `(what, strict)` — same transcription as
/// `build_process::type_is` (kept local: leaf module).
fn type_is(save: &Save, this: i32, what: i32, strict: bool) -> bool {
    if this == what {
        return true;
    }
    let Some(t) = usize::try_from(this).ok().and_then(|i| save.rules_tail.rules.types.get(i)) else { return false };
    let a = if strict { &t.arr1 } else { &t.arr0 };
    let mut it = a.data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]) as i32).peekable();
    if it.peek().is_none() {
        if !strict {
            return what >= 0 && type_is_slow(save, this, what, 0);
        }
        if !is_unit_type(this) {
            return false;
        }
        if type_i32(save, this, 0x25c) == Some(what) {
            return type_i32(save, what, 0x2b4).map(|f| f & 0x1000000 == 0).unwrap_or(false);
        }
        return false;
    }
    if !strict && what < 0 {
        return false;
    }
    it.any(|v| v == what)
}

// ---------------------------------------------------------------------------
// Build image access (retail offsets → serialized sub-ranges)
// ---------------------------------------------------------------------------

struct BuildRef {
    who: usize,
    o: i32,
    x: i32,
    y: i32,
    ptype: i32,
    flags: u8,
    stance: i8,
    queued: u8,
    build_masks: u16,
    queue_rows: usize,
}

fn build_ref(save: &Save, owner: usize, slot: usize) -> Option<BuildRef> {
    let Some(Some(Obj::Build(b))) = save.objects.lists.get(owner).and_then(|l| l.elems.get(slot)) else { return None };
    if b.base.sub.body.len() < 19 || b.wall_body.len() < 30 || b.body.len() < 22 {
        return None;
    }
    let s = &b.base.sub.body;
    Some(BuildRef {
        who: s[0] as usize,
        o: i16::from_le_bytes([s[1], s[2]]) as i32,
        x: i32::from_le_bytes(s[7..11].try_into().unwrap()) ^ 0x63637,
        y: i32::from_le_bytes(s[11..15].try_into().unwrap()) ^ 0x63637,
        ptype: i32::from_le_bytes(s[15..19].try_into().unwrap()),
        flags: b.base.sub.flags,
        stance: b.body[0x0e] as i8,
        queued: b.body[0x12],
        build_masks: u16::from_le_bytes([b.wall_body[0x18], b.wall_body[0x19]]),
        queue_rows: b.queue.len() / 18,
    })
}

fn with_build_mut(save: &mut Save, owner: usize, slot: usize, f: impl FnOnce(&mut crate::sections::Build)) {
    if let Some(Some(Obj::Build(b))) = save.objects.lists.get_mut(owner).and_then(|l| l.elems.get_mut(slot)) {
        f(b);
    }
}

/// `QueueItem[qslot]` (18 serialized bytes: job_counter i32, type i16,
/// good[3], cost[3]).
fn queue_item(save: &Save, owner: usize, slot: usize, qslot: usize) -> Option<(i32, i16)> {
    let Some(Some(Obj::Build(b))) = save.objects.lists.get(owner).and_then(|l| l.elems.get(slot)) else { return None };
    let r = b.queue.chunks_exact(18).nth(qslot)?;
    Some((i32::from_le_bytes(r[0..4].try_into().unwrap()), i16::from_le_bytes([r[4], r[5]])))
}

// ---------------------------------------------------------------------------
// Build::do_queue 0x0061e410 — completion branch
// ---------------------------------------------------------------------------

/// Result of [`complete_queue_item_at`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Completion {
    /// `finished()` return: 1 produced, 0 blocked (population), -1 limit.
    pub finished: i32,
    /// New unit `o` (captain of the batch) when a unit was trained.
    pub unit_o: Option<i32>,
    /// Main-LCG draws retail would have made in callees this module does not
    /// yet execute (`Guy::init_real` per Guy + `Unit::come_out` tail).
    pub rng_draws_unaccounted: u32,
}

/// Queue index 0 of `Objects.lists[owner][slot]` has reached its total
/// (`total <= job_counter`, as `do_queue` tests on the pre-advance value):
/// run the completion branch with `total = job_counter`.
pub fn complete_queue_item(save: &mut Save, owner: usize, slot: usize, effects: &mut Vec<String>) -> Option<Completion> {
    let (jc, _) = queue_item(save, owner, slot, 0)?;
    complete_queue_item_at(save, owner, slot, 0, jc, effects)
}

/// `Build::do_queue(qslot)` from 0x0061eb8f (`ptype->is(LIBRARY)` test) on,
/// for the `total <= job_counter` case. `total` is `construct_time(type)`.
pub fn complete_queue_item_at(
    save: &mut Save,
    owner: usize,
    slot: usize,
    qslot: usize,
    total: i32,
    effects: &mut Vec<String>,
) -> Option<Completion> {
    let b = build_ref(save, owner, slot)?;
    if b.queued == 0 {
        return None;
    }
    let tag = format!("Objects.lists[{owner}][{slot}]");
    let ty = if qslot < b.queue_rows { queue_item(save, owner, slot, qslot).map(|q| q.1 as i32).unwrap_or(-1) } else { -1 };
    let who = b.who;

    // if (ptype->is(0x1b3 LIBRARY, 0)) { Library arm }
    if type_is(save, b.ptype, 0x1b3, false) {
        // n = LeaderData::get_building_cities(...) 0x006e06f0; if (qslot+1 < n &&
        // qslot+1 < queued) do_queue(qslot+1)   -- parallel research slots.
        // if (complete) { if (!finished(type)) return; if (!(flags&1)) return;
        //                 unqueue(qslot, 0); }
        // finished(type) for a tech/age type is Leader::gain_tech 0x006dcb60
        // (research.rs lane); until it lands we leave the row at `total` and
        // report, so the Leader tech bits are never half-written.
        with_build_mut(save, owner, slot, |bb| {
            if let Some(r) = bb.queue.chunks_exact_mut(18).nth(qslot) {
                r[0..4].copy_from_slice(&total.to_le_bytes());
            }
        });
        effects.push(format!(
            "{tag}.Build.queue[{qslot}] type {ty} complete: Library arm — Leader::gain_tech 0x006dcb60 not transcribed (research.rs); row held at {total}"
        ));
        return Some(Completion { finished: 0, unit_o: None, rng_draws_unaccounted: 0 });
    }

    // if (qslot < queue_size) queue[qslot].job_counter = total;
    with_build_mut(save, owner, slot, |bb| {
        if let Some(r) = bb.queue.chunks_exact_mut(18).nth(qslot) {
            let old = i32::from_le_bytes(r[0..4].try_into().unwrap());
            if old != total {
                r[0..4].copy_from_slice(&total.to_le_bytes());
                effects.push(format!("{tag}.Build.queue[{qslot}].job_counter {old} -> {total} (complete)"));
            }
        }
    });
    // repeat = type->is_unit_type() && leader known-bit(type)
    let repeat = is_unit_type(ty) && known_bit(save, who, ty);
    // r = finished(type)                         // vtable +0x1b0 = 0x00628490
    let mut comp = Completion::default();
    let r = finished(save, owner, slot, ty, &mut comp, effects);
    comp.finished = r;
    if r > 0 {
        let masks = b.build_masks;
        // unqueue(qslot, 0)                      // vtable +0x1c8 = 0x006207c0
        unqueue(save, owner, slot, qslot, false, effects);
        if repeat && masks & 0x40 != 0 {
            // build_masks &= ~0x40; if (queue_up(type, 0) != 0) return; build_masks |= 0x40;
            // TODO(va 0x00620f40) Build::queue_up — pays the cost into the
            // Leader resource counters and appends a QueueItem; the latch
            // bit is left untouched until it is transcribed.
            effects.push(format!("{tag}.Build infinite-queue repeat of type {ty}: Build::queue_up 0x00620f40 not transcribed"));
        }
        return Some(comp);
    }
    // Blocked: if (qslot != 0) return;  then the queue-reorder fallbacks
    // (BuildQueueData::get_next_non_unit 0x00630b10, get_next_helicopter
    // 0x00630bb0 / get_next_non_caravan 0x00630c20) pick a later item and
    // recurse into do_queue(idx) — owned by the caller's do_queue.
    if qslot == 0 {
        effects.push(format!(
            "{tag}.Build.queue[0] type {ty} blocked (finished={r}): get_next_* fallback 0x00630b10/0x00630bb0/0x00630c20 → do_queue(idx) left to the owner"
        ));
    }
    Some(comp)
}

// ---------------------------------------------------------------------------
// Build::finished 0x00628490
// ---------------------------------------------------------------------------

/// `Build::finished(type)`: 1 when the item produced its effect, 0 when the
/// population cap blocks a unit, -1 when a caravan/aircraft limit blocks it.
pub fn finished(save: &mut Save, owner: usize, slot: usize, ty: i32, comp: &mut Completion, effects: &mut Vec<String>) -> i32 {
    let Some(b) = build_ref(save, owner, slot) else { return 0 };
    let who = b.who;
    let tag = format!("Objects.lists[{owner}][{slot}]");
    if is_unit_type(ty) && known_bit(save, who, ty) {
        // if (leader+0x7e4 < UnitType+0x2f0 + leader+0x940) return 0;
        let cap = leader_i32(save, who, 0x7e4).unwrap_or(0);
        let pop = type_i32(save, ty, 0x2f0).unwrap_or(0);
        let used = leader_i32(save, who, 0x940).unwrap_or(0);
        if cap < pop + used {
            effects.push(format!("{tag}.Build.finished({ty}): population {used}+{pop} > cap {cap} → 0"));
            return 0;
        }
        // if (UnitType+0x2b8 & 8) && get_units(type,0) >= get_caravan_limit(0) return -1;
        if type_i32(save, ty, 0x2b8).unwrap_or(0) & 8 != 0 {
            // TODO(va 0x006e07d0, 0x006dca50) LeaderData::get_units /
            // get_caravan_limit — caravan cap. Not evaluated; proceed as retail
            // does below the cap.
            effects.push(format!("{tag}.Build.finished({ty}): caravan limit 0x006dca50 not evaluated"));
        }
        // if (ptype->is(0x1bf AIRBASE, 0) && !(UnitType+0x2b4 & 0x20)) num_aircraft_here >= limit → -1
        if type_is(save, b.ptype, 0x1bf, false) && type_i32(save, ty, 0x2b4).unwrap_or(0) & 0x20 == 0 {
            // TODO(va 0x00645330, 0x006454a0) ObjectData::num_aircraft_here /
            // num_aircraft_limit.
            effects.push(format!("{tag}.Build.finished({ty}): aircraft limit 0x006454a0 not evaluated"));
        }
        let o = train_unit(save, owner, slot, ty, comp, effects);
        if o >= 0 {
            comp.unit_o = Some(o);
        }
        return 1;
    }
    if is_spell_type(ty) {
        // if (LeaderData::has_spell(type)) { SpellType::cast(o, who, 0, 0); return 1; }
        // TODO(va 0x006e0bc0, 0x00676ce0)
        effects.push(format!("{tag}.Build.finished({ty}): spell arm (SpellType::cast 0x00676ce0) not transcribed"));
        return 0;
    }
    if is_building_type(ty) && type_i32(save, ty, 0x2c0).unwrap_or(0) & 4 == 0 {
        // Building upgrade in place: CityData::get_pop_value, set_type(type, 0)
        // (vtable +0x84), Wall::mask_me(1, 0), leader flags |= 0x8000000, city
        // pop/Game+0x6cc/region counters, LeaderData 0x006dc490, 0x00680f60.
        // TODO(va 0x00738450, 0x00642fc0, 0x006dc490, 0x00680f60)
        effects.push(format!("{tag}.Build.finished({ty}): building-upgrade arm not transcribed"));
        return 0;
    }
    // Leader::gain_tech(type, x, y, 1, 1) 0x006dcb60 — techs, ages, and
    // unit types the Leader does not yet know (the upgrade path). Then
    // SENATE: get_gov_hero → train(hero) / find_unit patriot swap.
    // TODO(va 0x006dcb60) research.rs lane.
    effects.push(format!("{tag}.Build.finished({ty}): Leader::gain_tech 0x006dcb60 not transcribed (research.rs)"));
    0
}

// ---------------------------------------------------------------------------
// Build::unqueue 0x006207c0
// ---------------------------------------------------------------------------

/// `Build::unqueue(slot, refund)`: drop `QueueItem[slot]` and roll back the
/// Leader's queued counters. `refund` (param_2) also calls
/// `Build::unpay_cost` 0x006206e0 and collapses runs of equal types — only
/// the `refund == false` arm (production completion) is transcribed.
pub fn unqueue(save: &mut Save, owner: usize, slot: usize, qslot: usize, refund: bool, effects: &mut Vec<String>) {
    let Some(b) = build_ref(save, owner, slot) else { return };
    let who = b.who;
    let tag = format!("Objects.lists[{owner}][{slot}]");
    let queued = b.queued as usize;
    // Library delegation: a non-first Library forwards to the first library's
    // unqueue(slot - queued, refund) unless the item is DISBAND (0x29a).
    if type_is(save, b.ptype, 0x1b3, false) {
        let is_disband = queued != 0 && qslot < queued && qslot < b.queue_rows && queue_item(save, owner, slot, qslot).map(|q| q.1) == Some(0x29a);
        if !is_disband {
            // TODO(va 0x006db6c0) LeaderData::get_first_library — if it is not
            // this building, retail forwards; we treat this Build as the first.
            effects.push(format!("{tag}.Build.unqueue: get_first_library 0x006db6c0 not evaluated; assuming this Library"));
        }
    }
    if queued == 0 || qslot > queued - 1 {
        return;
    }
    if refund {
        // TODO(va 0x006206e0) Build::unpay_cost + equal-type run collapse.
        effects.push(format!("{tag}.Build.unqueue refund arm (unpay_cost 0x006206e0) not transcribed"));
    }
    // queue[slot].job_counter = 0; type = queue[slot].type
    let mut ty: i32 = -1;
    with_build_mut(save, owner, slot, |bb| {
        if let Some(r) = bb.queue.chunks_exact_mut(18).nth(qslot) {
            r[0..4].copy_from_slice(&0i32.to_le_bytes());
            ty = i16::from_le_bytes([r[4], r[5]]) as i32;
        }
    });
    if ty >= 0 {
        // if (leader.queued_types[type] != 0) leader.queued_types[type] -= 1;   // +0x5a22 + type*2 (i16)
        let off = 0x5a22 + (ty as usize) * 2;
        if leader_i16(save, who, off).unwrap_or(0) != 0 {
            leader_add_i16(save, who, off, -1, "queued per type", effects);
        }
        // unit types with attack: queued military counters by `from` (+0x40)
        if is_unit_type(ty) && type_i32(save, ty, 0x1e8).unwrap_or(0) != 0 {
            let from = type_i32(save, ty, 0x40).unwrap_or(-1);
            let dec = |save: &mut Save, off: usize, effects: &mut Vec<String>| {
                if leader_i32(save, who, off).unwrap_or(0) != 0 {
                    leader_add_i32(save, who, off, -1, "queued military", effects);
                }
            };
            match from {
                0x1ab => {
                    dec(save, 0xa10, effects);
                    dec(save, 0xa1c, effects);
                }
                0x1ac => {
                    dec(save, 0xa14, effects);
                    dec(save, 0xa1c, effects);
                }
                0x1ae => dec(save, 0xa18, effects),
                0x1b0 => dec(save, 0xa20, effects),
                _ => {
                    if type_i32(save, ty, 0x218) == Some(2) {
                        dec(save, 0xa24, effects);
                    }
                }
            }
        }
        if is_age_type(ty) {
            leader_add_u8(save, who, 0x67f4, -1, "queued ages", effects);
        }
        if is_epoch_type(ty) {
            leader_add_u8(save, who, 0x67f5, -1, "queued epochs", effects);
        }
    }
    // if (slot < queued - 1) BuildQueue::un_queue(slot, queued): memmove rows slot+1.. down.
    with_build_mut(save, owner, slot, |bb| {
        if qslot < queued - 1 {
            let rows = bb.queue.len() / 18;
            if qslot < rows {
                let end = queued.min(rows);
                let n = (end - qslot - 1) * 18;
                let src = (qslot + 1) * 18;
                let dst = qslot * 18;
                bb.queue.copy_within(src..src + n, dst);
            }
        }
        // queued -= 1; if (queued == 0) build_masks &= ~0x40;
        bb.body[0x12] -= 1;
        if bb.body[0x12] == 0 {
            let m = u16::from_le_bytes([bb.wall_body[0x18], bb.wall_body[0x19]]) & !0x40;
            bb.wall_body[0x18..0x1a].copy_from_slice(&m.to_le_bytes());
        }
    });
    effects.push(format!("{tag}.Build.unqueue({qslot}) type {ty}: queued {queued} -> {}", queued - 1));
}

// ---------------------------------------------------------------------------
// Build::train 0x0062f9b0
// ---------------------------------------------------------------------------

/// `Build::train(type)`: spawn `type` at this building. Returns the new unit
/// `o` or a negative `init_unit` failure.
pub fn train_unit(save: &mut Save, owner: usize, slot: usize, ty: i32, comp: &mut Completion, effects: &mut Vec<String>) -> i32 {
    let Some(b) = build_ref(save, owner, slot) else { return -1 };
    let who = b.who;
    let tag = format!("Objects.lists[{owner}][{slot}]");
    // o = Objects::init_unit(who, type, x, y, -1, -1, -1)
    let o = init_unit(save, who, ty, b.x, b.y, comp, effects);
    if o < 0 {
        return o;
    }
    // DAT_00cc1930[who] = o   (last trained — not walked)
    // leader.last_made[type] = o                   // +0x61ac + type*4
    let off = 0x61ac + (ty as usize) * 4;
    let old = leader_i32(save, who, off).unwrap_or(-1);
    set_leader_bytes(save, who, off, &o.to_le_bytes());
    effects.push(format!("Leader[{who}]+{off:#x} {old} -> {o} (last trained of type {ty})"));
    // if (unit->get_stance_type() == ptype->get_stance_type()) unit->set_stance(stance, 0)
    if unit_stance_type(save, ty) == build_stance_type(save, b.ptype) {
        set_stance(save, who, o as usize, b.stance, effects);
    }
    // Unit::go_inside(o_build, who, 0) 0x0061a2e0 then (for non-garrisoning
    // buildings) Unit::come_out(0) 0x00617c10 — the exit placement:
    // find_nearby_spot 0x0061de70 / set_new_location 0x005f8d20 /
    // add_to_world 0x0064d8c0, plus the AI add_to_army coin (1 LCG draw).
    // TODO(va 0x0061a2e0, 0x00617c10)
    effects.push(format!(
        "{tag}.Build.train({ty}) → unit [{who}][{o}]: Unit::go_inside 0x0061a2e0 / come_out 0x00617c10 placement not transcribed (unit left at the init location)"
    ));
    if leader_flags(save, who) & 0xc != 4 {
        // come_out tail: AI-owned non-caravan → one Random::get(0, 0xffff).
        if type_i32(save, ty, 0x2b8).unwrap_or(0) & 0x40 == 0 {
            comp.rng_draws_unaccounted += 1;
        }
    }
    // unit_masks &= ~0x4000000
    with_unit_mut(save, who, o as usize, |u| {
        let m = u32::from_le_bytes(u.body[0x20..0x24].try_into().unwrap()) & !0x4000000;
        u.body[0x20..0x24].copy_from_slice(&m.to_le_bytes());
    });
    // if (unit->is(0x15f AIRCRAFTCARRIER, 1)) { action_unqueue(1); n = num_aircraft_limit();
    //   for i in 0..n init_unit(who, current_upgrade(0x134), ux, uy, -1,-1,-1) + go_inside }
    if type_is(save, ty, 0x15f, true) {
        effects.push(format!("{tag}.Build.train({ty}): AIRCRAFTCARRIER escort spawn (0x006e3140, 0x006454a0) not transcribed"));
    }
    o
}

/// `UnitTypeData::get_stance_type` 0x0061d350.
fn unit_stance_type(save: &Save, ty: i32) -> i32 {
    if type_i32(save, ty, 0x2c8).unwrap_or(0) & 0x10000 != 0 {
        return if type_i32(save, ty, 0x2b8).unwrap_or(0) & 4 != 0 { 3 } else { 0 };
    }
    if is_peasant(ty) || is_scholar(ty) {
        return 1;
    }
    if type_i32(save, ty, 0x2b8).unwrap_or(0) & 6 == 2 {
        2
    } else {
        -1
    }
}

/// `BuildTypeData::get_stance_type` 0x006396c0 (through
/// `BuildTypeData::has_stance` 0x00639de0: `BuildType+0x2c0 & 0x80000000`
/// of the type or its `graft`/root).
fn build_stance_type(save: &Save, pt: i32) -> i32 {
    let root = match type_i32(save, pt, 0x3c) {
        Some(f) if f >= 0 => f,
        _ => pt,
    };
    if type_i32(save, root, 0x2c0).unwrap_or(0) as u32 & 0x80000000 == 0 {
        return -1;
    }
    if type_is(save, pt, 0x19e, false) {
        return 1;
    }
    if type_is(save, pt, 0x1bb, false) {
        return 2;
    }
    match pt {
        0x1ae | 0x1af => 3,
        0x1b4 | 0x1a4 | 0x1bf => -1,
        0x208 => -1,
        _ => 0,
    }
}

/// `Unit::set_stance(stance, 0)` 0x00605310 on a captain: `stance` (+0xb1),
/// then propagates down the `o_down` chain while the followers are live.
fn set_stance(save: &mut Save, who: usize, o: usize, stance: i8, effects: &mut Vec<String>) {
    let mut cur = o;
    loop {
        let mut next: i32 = -1;
        let mut changed = false;
        with_unit_mut(save, who, cur, |u| {
            if u.body[0x69] != stance as u8 {
                u.body[0x69] = stance as u8;
                changed = true;
            }
            next = i16::from_le_bytes([u.body[0x48], u.body[0x49]]) as i32;
        });
        if changed {
            effects.push(format!("Objects.lists[{who}][{cur}].Unit.stance -> {stance}"));
        }
        if next < 0 {
            break;
        }
        match save.objects.lists[who].elems.get(next as usize) {
            Some(Some(Obj::Unit(u))) if u.base.sub.flags & 1 != 0 => cur = next as usize,
            _ => break,
        }
    }
}

fn with_unit_mut(save: &mut Save, who: usize, o: usize, f: impl FnOnce(&mut Unit)) {
    if let Some(Some(Obj::Unit(u))) = save.objects.lists.get_mut(who).and_then(|l| l.elems.get_mut(o)) {
        if u.body.len() == 111 && u.base.mid.len() == 34 && u.base.sub.body.len() == 19 {
            f(u);
        }
    }
}

// ---------------------------------------------------------------------------
// Objects::init_unit 0x0065e0c0 / Objects::find_free 0x0065ad60
// ---------------------------------------------------------------------------

/// `Objects::init_unit(who, type, x, y, -1, -1, -1)`: allocate
/// `UnitType+0x308` units (the batch), `Unit::init` each, link
/// captain/followers, return the captain's `o`.
pub fn init_unit(save: &mut Save, who: usize, ty: i32, x: i32, y: i32, comp: &mut Completion, effects: &mut Vec<String>) -> i32 {
    let count = type_i32(save, ty, 0x308).unwrap_or(1).max(0);
    let mut captain: i32 = -1;
    let mut prev: i32 = -1;
    for i in 0..count {
        // o = find_free(who, 0, 2000, &unit_mark[who], -1)
        let o = find_free(save, who, 0, 2000);
        if o < 0 {
            effects.push(format!("Objects::init_unit({who}, {ty}): find_free failed ({o})"));
            return o;
        }
        // Objects.lists[who][o]->init(who, type, o, x, y)   // vtable +0x8c = Unit::init
        unit_init(save, who, ty, o, x, y, comp, effects);
        // unit->o_up = prev (-1 for the captain)
        with_unit_mut(save, who, o as usize, |u| u.body[0x46..0x48].copy_from_slice(&(prev as i16).to_le_bytes()));
        if prev >= 0 {
            // Follower: undo the population that set_type counted unless
            // the type has no pop cost and is neither FIGHTERBOMBER-class
            // nor flagged 0x4000000.
            let pop = type_i32(save, ty, 0x2f0).unwrap_or(0);
            let counts = pop != 0 || type_is(save, ty, 0x134, false) || type_i32(save, ty, 0x2b4).unwrap_or(0) & 0x4000000 != 0;
            if counts {
                track_unit_type(save, who, ty, -1, o, effects);
                let masks = unit_masks(save, who, o as usize);
                let pop_d = if masks & 1 == 0 { pop } else { 0 };
                leader_add_i32(save, who, 0x940, -pop_d, "follower pop undo", effects);
                leader_add_i32(save, who, 0x93c, -1, "follower count undo", effects);
                leader_add_i32(save, who, 0x808, -1, "follower pop-type undo", effects);
            }
            // UnitType::find_nearby_spot around the captain + set_new_location
            // TODO(va 0x0061de70, 0x005f8d20)
            effects.push(format!("Objects::init_unit follower [{who}][{o}] placement (find_nearby_spot 0x0061de70) not transcribed"));
            with_unit_mut(save, who, prev as usize, |u| u.body[0x48..0x4a].copy_from_slice(&(o as i16).to_le_bytes()));
        } else {
            captain = o;
        }
        prev = o;
        let _ = i;
    }
    // return captain->get_captain()   (o_up < 0 → own o)
    captain
}

fn unit_masks(save: &Save, who: usize, o: usize) -> u32 {
    match save.objects.lists[who].elems.get(o) {
        Some(Some(Obj::Unit(u))) if u.body.len() == 111 => u32::from_le_bytes(u.body[0x20..0x24].try_into().unwrap()),
        _ => 0,
    }
}

/// `Objects::find_free(who, start, limit, &mark, -1)`: first slot in
/// `start..mark` whose object is dead (`!(flags & 1)`), has `hold_frames ==
/// 0` and (unit plane) `o_up < 0`; else allocate at `mark` (`mark += 1`),
/// failing with -1 at `limit`. Allocation constructs the Unit in place.
pub fn find_free(save: &mut Save, who: usize, start: usize, limit: i32) -> i32 {
    let mark = unit_mark(save, who);
    let list = &save.objects.lists[who];
    for o in start..(mark.max(0) as usize) {
        match list.elems.get(o) {
            Some(Some(Obj::Unit(u))) => {
                let hold = u.base.mid.get(0x12..0x14).map(|b| u16::from_le_bytes([b[0], b[1]])).unwrap_or(1);
                let o_up = u.body.get(0x46..0x48).map(|b| i16::from_le_bytes([b[0], b[1]])).unwrap_or(0);
                if u.base.sub.flags & 1 == 0 && hold == 0 && o_up < 0 {
                    return o as i32;
                }
            }
            Some(Some(Obj::Animal(a))) => {
                let u = &a.unit;
                let hold = u.base.mid.get(0x12..0x14).map(|b| u16::from_le_bytes([b[0], b[1]])).unwrap_or(1);
                let o_up = u.body.get(0x46..0x48).map(|b| i16::from_le_bytes([b[0], b[1]])).unwrap_or(0);
                if u.base.sub.flags & 1 == 0 && hold == 0 && o_up < 0 {
                    return o as i32;
                }
            }
            Some(Some(Obj::Build(_))) => {}
            // A null pointer in the band: retail allocates into it and still
            // returns mark (the loop only tests live pointers' fields; a
            // null entry fails `*(byte*)(piVar1+2)` — not reachable in saves).
            _ => {}
        }
    }
    if limit <= mark {
        return -1;
    }
    let slot = mark as usize;
    let fresh = new_unit_record(save);
    place(&mut save.objects.lists[who], slot, Obj::Unit(Box::new(fresh)));
    set_unit_mark(save, who, mark + 1);
    mark
}

/// A freshly constructed `Unit` (0x15c bytes, `Unit::Unit` 0x00616e00) as a
/// serialized record: every walked range present and zeroed, tags copied
/// from an existing Unit (same `walk_test` strings) or from `section_tag`.
fn new_unit_record(save: &Save) -> Unit {
    let mut u = Unit::default();
    let template = save.objects.lists.iter().flat_map(|l| l.elems.iter()).find_map(|e| match e {
        Some(Obj::Unit(t)) if t.base.sub.gate == 1 && t.gate == 1 => Some(t.as_ref()),
        _ => None,
    });
    match template {
        Some(t) => {
            u.base.sub.tag = t.base.sub.tag;
            u.base.tag = t.base.tag;
            u.tag = t.tag;
        }
        None => {
            u.base.sub.tag = section_tag("SubObject");
            u.base.tag = section_tag("Object");
            u.tag = section_tag("Unit");
        }
    }
    u.base.sub.gate = 1;
    u.base.sub.body = vec![0; 19];
    u.base.gate = 1;
    u.base.mid = vec![0; 34];
    u.base.launch = 0;
    u.gate = 1;
    u.body = vec![0; 111];
    u
}

/// Install `obj` at `slot` of a `MultiPtrArray<Object>` record, keeping the
/// presence plane and the stream-ordered concrete-type list consistent.
fn place(list: &mut ObjList, slot: usize, obj: Obj) {
    if slot >= list.elems.len() {
        let n = slot + 1;
        list.elems.resize_with(n, || None);
        list.present.resize(n, 0);
        list.len = n as i32;
        if list.cap < list.len {
            list.cap = list.len;
            list.cap2 = list.len;
        }
    }
    let was_present = list.present[slot] != 0;
    let before = list.present[..slot].iter().filter(|&&p| p != 0).count();
    if was_present {
        if before < list.types.len() {
            list.types[before] = obj.ty();
        }
    } else {
        list.types.insert(before.min(list.types.len()), obj.ty());
        list.present[slot] = 1;
    }
    list.elems[slot] = Some(obj);
}

// ---------------------------------------------------------------------------
// Unit::init 0x00612100 (with Object::init 0x00647750, SubObject::init
// 0x00662300, SubObject::set_type 0x00662450, Unit::set_type 0x00612fa0)
// ---------------------------------------------------------------------------

/// `init_coord_lookup_array` 0x00681db0: `lookup[i] = i / 3` (floor).
fn coord_lookup(i: i32) -> i32 {
    i.div_euclid(3)
}

/// `Unit::init(who, type, o, x, y)` — the scalar write set on the walked
/// SubObject/Object/Unit images, the Leader/Objects/Game counters, and the
/// reports for the untranscribed callees.
#[allow(clippy::too_many_arguments)]
fn unit_init(save: &mut Save, who: usize, ty: i32, o: i32, x: i32, y: i32, comp: &mut Completion, effects: &mut Vec<String>) {
    let tag = format!("Objects.lists[{who}][{o}]");
    // Unit::close(0, -1, 0) 0x0060ee50 — on a dead/fresh record the live
    // body (flags & 1) is skipped; the tail only touches orders/path.
    // snapped = lookup[x >> 4] * 0x30 + 0x18   (48-unit grid centre)
    let sx = coord_lookup(x >> 4) * 0x30 + 0x18;
    let sy = coord_lookup(y >> 4) * 0x30 + 0x18;
    let ai = leader_flags(save, who) & 4 == 0;
    let uid = obj_ctr(save, who);
    let pop = type_i32(save, ty, 0x2f0).unwrap_or(0);
    let guys = type_i32(save, ty, 0x304).unwrap_or(1);
    let flags_from_type = type_i32(save, ty, 0x2b4).unwrap_or(0);
    let flags2_from_type = type_i32(save, ty, 0x2b8).unwrap_or(0);
    let is_caravan = flags2_from_type & 0x40 != 0;
    let is_siege_like = flags2_from_type & 0x20 != 0;

    // --- Object::init → SubObject::init(who, type, o, x, y) ---
    with_unit_mut(save, who, o as usize, |u| {
        let s = &mut u.base.sub;
        s.body[0] = who as u8; // who
        s.flags = 1; // flags = 1 (BuildTypeData::is_city via Type vtable +0x64 is 0 for unit types)
        s.body[1..3].copy_from_slice(&(o as i16).to_le_bytes()); // o
        // on_screen (+0x1c) = 0 — not walked
        // set_type(type, 1): ptype = Rules.types[type]
        s.body[15..19].copy_from_slice(&ty.to_le_bytes());
        s.body[7..11].copy_from_slice(&(x ^ 0x63637).to_le_bytes());
        s.body[11..15].copy_from_slice(&(y ^ 0x63637).to_le_bytes());
        // z = World::get_height(lookup[x>>6], lookup[y>>6], 1) 0x008544a0 — TODO
    });
    effects.push(format!("{tag}.SubObject z: World::get_height 0x008544a0 not transcribed"));
    // Unit::set_type(type, 1) counters (captain: o_up < 0 on a fresh record)
    let o_up_fresh = match save.objects.lists[who].elems.get(o as usize) {
        Some(Some(Obj::Unit(u))) => i16::from_le_bytes([u.body[0x46], u.body[0x47]]),
        _ => -1,
    };
    // A fresh Unit (constructor) carries o_up = 0 in the zeroed image; retail's
    // constructor value is what get_captain() tests here. The follower undo in
    // init_unit mirrors exactly this gate, so both sides stay consistent.
    let captain = o_up_fresh >= 0 || true;
    if captain {
        let counts = pop != 0 || type_is(save, ty, 0x134, false) || flags_from_type & 0x4000000 != 0;
        if counts {
            track_unit_type(save, who, ty, 1, o, effects);
            leader_add_i32(save, who, 0x940, pop, "population used", effects);
            leader_add_i32(save, who, 0x93c, 1, "units alive", effects);
        }
    }
    if type_i32(save, ty, 0x130).unwrap_or(0) != 0 || is_caravan {
        // UnitTypeData::is_caravan 0x00470420 → leader +0x980 += 1
        if is_caravan {
            leader_add_i32(save, who, 0x980, 1, "caravans", effects);
        }
    }
    // --- Object::init scalar writes ---
    with_unit_mut(save, who, o as usize, |u| {
        let m = &mut u.base.mid; // +0x20..
        m[0x18..0x1a].copy_from_slice(&0i16.to_le_bytes()); // healing (+0x38) = 0
        m[0x08..0x0a].copy_from_slice(&(-1i16).to_le_bytes()); // inside_down
        m[0x0a..0x0c].copy_from_slice(&(-1i16).to_le_bytes()); // up
        m[0x0c..0x0e].copy_from_slice(&(-1i16).to_le_bytes()); // down
        m[0x14..0x16].copy_from_slice(&(-1i16).to_le_bytes()); // near_o
        m[0x16..0x18].copy_from_slice(&(-1i16).to_le_bytes()); // near_who
        m[0x04..0x08].copy_from_slice(&0i32.to_le_bytes()); // damage
        m[0x1e] = who as u8; // inside_down_who
        m[0x1d] = 0; // targeted
        m[0x20] = 0; // visible
        m[0x21] = 0; // launch_frames
        m[0x10..0x12].copy_from_slice(&uid.to_le_bytes()); // uid = obj_ctr[who]
        m[0x12..0x14].copy_from_slice(&0u16.to_le_bytes()); // hold_frames
        u.base.launch = 0; // launching freed
        u.base.launching = Default::default();
        // if (has_objmask(0x2000000)) flags |= 0x40
        if flags_from_type & 0x2000000 != 0 {
            u.base.sub.flags |= 0x40;
        }
    });
    set_obj_ctr(save, who, uid.wrapping_add(1));
    effects.push(format!("Objects.obj_ctr[{who}] {uid} -> {} ({tag}.uid)", uid.wrapping_add(1)));
    // Object::add_to_world 0x0064d8c0: tile list link (down/down_who, WData
    // +8/+0xa head) — TODO; `up` stays -1.
    effects.push(format!("{tag}.Object down/down_who: Object::add_to_world 0x0064d8c0 not transcribed"));

    // --- Unit::init body ---
    // if (ptype->pop != 0) leader+0x808 += 1
    if pop != 0 {
        leader_add_i32(save, who, 0x808, 1, "pop-cost units", effects);
    }
    with_unit_mut(save, who, o as usize, |u| {
        let b = &mut u.body; // +0x48..
        let w32 = |b: &mut Vec<u8>, off: usize, v: i32| b[off - 0x48..off - 0x44].copy_from_slice(&v.to_le_bytes());
        let w16 = |b: &mut Vec<u8>, off: usize, v: i16| b[off - 0x48..off - 0x46].copy_from_slice(&v.to_le_bytes());
        let w8 = |b: &mut Vec<u8>, off: usize, v: u8| b[off - 0x48] = v;
        w32(b, 0x50, 0x55555555); // angle
        w8(b, 0xaa, if is_peasant(ty) || is_scholar(ty) { 9 } else { 0 }); // form
        w8(b, 0xab, 0xff); // form_mod
        w8(b, 0xac, 0); // full
        w32(b, 0x96, 0); // mana_burn, spell_time
        w16(b, 0x9e, 0); // attrition
        w32(b, 0x54, 0); // rare
        w8(b, 0xb0, 0); // idle
        w32(b, 0x4c, 0); // damage_frame
        w16(b, 0xa4, -1); // damage_o
        w8(b, 0xa9, 0); // damage_who
        w8(b, 0xad, 0); // waiting
        w32(b, 0x58, 0x55555555); // dest_angle
        w32(b, 0x60, 0); // tolerance
        w32(b, 0x70, 0); // orders_x
        w32(b, 0x74, 0); // orders_y
        w32(b, 0x78, sx); // los_x
        w32(b, 0x7c, sy); // los_y
        // unit_masks &= ~0x4000000; close_orders(0); clear_partial_path(); update_action();
        // unit_masks = 0; unit_masks2 = 0;
        w32(b, 0x68, 0);
        w32(b, 0x6c, 0);
        // orderlist/path cleared by close_orders/clear_partial_path
        u.orders.orders.clear();
        u.orders.count = 0;
        u.path.data.clear();
        u.path.len = 0;
    });
    // Human-player STAT_* training stats (Game+0xcb/0x77 gates) — UI stats, not walked.
    with_unit_mut(save, who, o as usize, |u| {
        let b = &mut u.body;
        let w32 = |b: &mut Vec<u8>, off: usize, v: i32| b[off - 0x48..off - 0x44].copy_from_slice(&v.to_le_bytes());
        let w16 = |b: &mut Vec<u8>, off: usize, v: i16| b[off - 0x48..off - 0x46].copy_from_slice(&v.to_le_bytes());
        let w8 = |b: &mut Vec<u8>, off: usize, v: u8| b[off - 0x48] = v;
        let mut masks2: u32 = 0;
        // is(0x165 THECEO, 1) → masks2 0x10000 (+ set_new_location sweep)
        if type_is(save, ty, 0x165, true) {
            masks2 |= 0x10000;
        }
        w8(b, 0xae, 0); // recharging
        w16(b, 0xa0, 0); // num_queued
        w32(b, 0x64, 0); // queue_time
        w8(b, 0xb2, 0); // safe
        w16(b, 0x84, -1); // supply
        w16(b, 0x86, -1); // hero
        w16(b, 0x82, -1); // inside_up
        w8(b, 0xb4, who as u8); // inside_up_who
        w16(b, 0x80, -1); // group
        w16(b, 0x92, -1); // gather_down
        w16(b, 0x94, -1); // good_obj
        // stance by get_stance_type()
        let stance: u8 = match unit_stance_type(save, ty) {
            0 => 0, // Options[who].default_stance (+0xc) — [0x00c061b4] per-player 0x20 block, not walked; 0 observed
            1 => {
                if ai {
                    (game_setting(save, 0x2d) == 8) as u8 + 1
                } else {
                    0 // Options[who]+4 — not walked
                }
            }
            2 => (!(options_byte(save, who, 0x1c) >> 4)) & 1,
            3 => (!(options_byte(save, who, 0x1c) >> 3)) & 1,
            _ => 0,
        };
        w8(b, 0xb1, stance);
        let mut masks: u32 = 0;
        // transport_type() <= leader age class && can_ever_transport() && (!is(0x45 SCOUT) || Options bit) → masks 0x800000
        // TODO(va 0x0046f790, 0x0046f290) UnitData::transport_type / can_ever_transport — zero for land units.
        // has_tribe_bonus(0x12): masks2 0x80 / 0x4080 ; is(0x77 MARINES) → masks2 0x800
        if has_tribe_bonus(save, who, 0x12) {
            if type_i32(save, ty, 0x2b4 + 0x86 * 4 - 0x2b4 + 0x2b4).is_some() {
                // ptype[0x86] (+0x218) == 0 && !has_objmask(4) → 0x80 ; is(0x45) → 0x4080
                if type_i32(save, ty, 0x218) == Some(0) && flags_from_type & 4 == 0 {
                    masks2 |= 0x80;
                }
                if type_is(save, ty, 0x45, false) {
                    masks2 |= 0x4080;
                }
            }
        }
        if type_is(save, ty, 0x77, false) {
            masks2 |= 0x800;
        }
        if flags2_from_type & 4 != 0 {
            masks |= 0x80000;
        }
        // Stack<PathData>::init(10) when path.size < 10
        if u.path.cap < 10 {
            u.path.cap = 10;
            u.path.inc = 10;
        }
        // supply / hero / caravan slot allocation
        if is_caravan {
            // supply = Supplies::alloc(who, o) 0x0073ad40 — TODO (Items section)
        }
        if is_siege_like {
            // hero = Heroes::alloc 0x0073a330 — TODO
        } else if type_i32(save, ty, 0x130).unwrap_or(0) == 0 || type_i32(save, ty, 0x218) != Some(0) {
            if is_peasant(ty) || is_scholar(ty) {
                w16(b, 0x86, -1);
            } else if flags2_from_type & 0x10 != 0 {
                // hero = Specials::alloc 0x007401c0 — TODO
            }
        } else {
            // caravan = Caravans::alloc 0x0073e1f0 — TODO
        }
        // Guys: guy_mark = ptype->guys (+0x304); Recycler<Guy>::pop + Guy::clear + Guy::init_real(0) each
        w8(b, 0xb5, guys as u8);
        // set_new_location(sx, sy, 1, 1) 0x005f8d20 — TODO (x/y stay at SubObject::init's raw coords)
        w16(b, 0x8a, -1); // collide_o
        w8(b, 0xb3, 0xff); // collide_who
        w16(b, 0x8c, -1); // collide_guy
        w32(b, 0x48, -1); // collide_frame
        w16(b, 0x8e, -1); // o_up
        w16(b, 0x90, -1); // o_down
        w16(b, 0xa2, -1); // cavarch_o
        w8(b, 0xb6, 0xff); // play
        w16(b, 0x88, 0); // collide
        w8(b, 0xa8, 0); // cavarch_who
        w16(b, 0xa6, 0); // cavarch_uid
        // can_carry(2) → masks 0x200000  TODO(va 0x00646c40) — zero for non-transports
        // (leader.flags & 0xc) != 4 → masks 0x40000
        if ai {
            masks |= 0x40000;
        }
        // is(0x3a SPY) → mana_burn = mana()/2 ; is(0x143 BARK) → masks2 |= 4 (+ set_stance(3,0) for AI)
        if type_is(save, ty, 0x143, false) {
            masks2 |= 4;
        }
        w32(b, 0x68, masks as i32);
        w32(b, 0x6c, masks2 as i32);
    });
    if type_is(save, ty, 0x143, false) && ai {
        set_stance(save, who, o as usize, 3, effects);
    }
    if type_is(save, ty, 0x3a, false) {
        effects.push(format!("{tag}.Unit.mana_burn: UnitData::mana 0x00609a50 not transcribed"));
    }
    // Game.units_created (+0x6d8) += 1 when who < 9
    if who < 9 {
        let v = game_i32(save, 0x6d8);
        set_game_i32(save, 0x6d8, v + 1);
        effects.push(format!("Game+0x6d8 {v} -> {} (units created)", v + 1));
    }
    // leader.flags |= 0x800000 when who < 8
    if who < 8 {
        leader_or_flags(save, who, 0x800000, "unit list dirty", effects);
    }
    // has_tribe_bonus(0x13)/(0x14) + Constants+0x848/+0x888 → leader.flags |= 0x2000000 (reported, not applied: needs the tribe table)
    // update_hits (vtable +0x15c) / update_los (+0x160) / update_speed / update_armor / update_seen (+0x174)
    effects.push(format!(
        "{tag}.Unit myhits/mylos/myspeed/myarmor: Unit::update_hits 0x0060e930, update_los 0x0060e4d0, update_speed 0x006055c0, update_armor 0x006054c0 not transcribed"
    ));
    effects.push(format!("{tag}.Unit.guys ({guys}): Guy::init_real 0x005db6b0 not transcribed (1 LCG draw each)"));
    comp.rng_draws_unaccounted += guys.max(0) as u32;
}

/// `GameInfo` single-byte setting at `Game+off` (GameInfo at Game+0xc,
/// `settings` = gi+0x18 = Game+0x24).
fn game_setting(save: &Save, off: usize) -> u8 {
    save.game.info.settings.get(off - 0x24).copied().unwrap_or(0)
}

/// `[0x00c061b4] + who*0x20 + off` — the per-player Options block (default
/// stances, formation bits). Not walked in the save; 0 is what the
/// captures' AI players carry.
fn options_byte(_save: &Save, _who: usize, _off: usize) -> u8 {
    0
}

/// `LeaderData::has_tribe_bonus(bonus)` 0x006e1370.
/// TODO(va 0x006e1370): the tribe bonus table lives in `Tribes` + the
/// Leader's tribe index; until transcribed this reports `false` (the
/// captures' nations do not carry bonuses 0x12..0x14 on Citizens).
fn has_tribe_bonus(_save: &Save, _who: usize, _bonus: i32) -> bool {
    false
}

/// `Leader::track_unit_type(type, d, o)` 0x006e0dd0.
fn track_unit_type(save: &mut Save, who: usize, ty: i32, d: i32, o: i32, effects: &mut Vec<String>) {
    // unit_counts[type] += d                              // +0x56fe + type*2 (i16)
    leader_add_i16(save, who, 0x56fe + (ty as usize) * 2, d as i16, "units of type", effects);
    // military by `from` when the type attacks
    if type_i32(save, ty, 0x1e8).unwrap_or(0) != 0 {
        match type_i32(save, ty, 0x40).unwrap_or(-1) {
            0x1ab => {
                leader_add_i32(save, who, 0x9f8, d, "barracks units", effects);
                leader_add_i32(save, who, 0xa04, d, "land military", effects);
            }
            0x1ac => {
                leader_add_i32(save, who, 0x9fc, d, "stable units", effects);
                leader_add_i32(save, who, 0xa04, d, "land military", effects);
            }
            0x1ae => leader_add_i32(save, who, 0xa00, d, "siege units", effects),
            0x1b0 => leader_add_i32(save, who, 0xa08, d, "naval units", effects),
            _ => {
                if type_i32(save, ty, 0x218) == Some(2) {
                    leader_add_i32(save, who, 0xa0c, d, "air units", effects);
                }
            }
        }
    }
    if is_peasant(ty) {
        leader_add_i32(save, who, 0x978, d, "peasants", effects);
    } else if is_scholar(ty) {
        leader_add_i32(save, who, 0x97c, d, "scholars", effects);
    } else if type_i32(save, ty, 0x2c8).unwrap_or(0) & 0x10 != 0 {
        leader_add_i32(save, who, 0x988, d, "merchant-class", effects);
    }
    // if (o < 0 || !type->is(0x42 MILITIA, 0)) { if (type->is(0x4b SPECIALFORCES, 0)) flags bit 0x20000 by count }
    // else if (unit[o].rare >= 0 && type(rare).is(0x34)) leader+0x9f0 += d
    if o < 0 || !type_is(save, ty, 0x42, false) {
        if type_is(save, ty, 0x4b, false) {
            let n = leader_i16(save, who, 0x56fe + (ty as usize) * 2).unwrap_or(0);
            if let Some(l) = save.leaders.slots.get_mut(who) {
                if n == 0 {
                    l.flags &= !0x20000;
                } else {
                    l.flags |= 0x20000;
                }
            }
        }
    } else {
        let rare = match save.objects.lists[who].elems.get(o as usize) {
            Some(Some(Obj::Unit(u))) if u.body.len() == 111 => i32::from_le_bytes(u.body[0x0c..0x10].try_into().unwrap()),
            _ => -1,
        };
        if rare >= 0 && type_is(save, rare, 0x34, false) {
            leader_add_i32(save, who, 0x9f0, d, "militia scholars", effects);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sections::Obj;
    use crate::{container, load};
    use std::path::{Path, PathBuf};

    fn capture_dirs() -> Vec<PathBuf> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/live/frame-pairs");
        let Ok(rd) = std::fs::read_dir(&root) else { return Vec::new() };
        let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.join("manifest.json").exists()).collect();
        v.sort();
        v
    }

    fn steps(dir: &Path) -> Vec<(i64, String)> {
        let text = std::fs::read_to_string(dir.join("manifest.json")).unwrap();
        let mut out = Vec::new();
        for seg in text.split("\"frame\":").skip(1) {
            let frame = seg.trim_start().split(|c: char| !c.is_ascii_digit()).next().unwrap().parse::<i64>().unwrap();
            let save = seg.split("\"save_name\":").nth(1).and_then(|s| s.split('"').nth(1)).unwrap().to_string();
            out.push((frame, save));
        }
        out
    }

    fn load_frame(dir: &Path, st: &[(i64, String)], f: i64) -> Save {
        let name = st.iter().find(|s| s.0 == f).map(|s| s.1.clone()).unwrap();
        let raw = container::load_svx(&dir.join(format!("{name}.svx"))).unwrap();
        load(&raw).unwrap().state
    }

    /// Named view of the three walked scalar ranges of a Unit.
    fn unit_fields(u: &Unit) -> Vec<(&'static str, usize, Vec<u8>)> {
        let s = &u.base.sub.body;
        let m = &u.base.mid;
        let b = &u.body;
        let mut v = Vec::new();
        v.push(("flags", 0x08, vec![u.base.sub.flags]));
        v.push(("who", 0x09, s[0..1].to_vec()));
        v.push(("o", 0x0a, s[1..3].to_vec()));
        v.push(("z", 0x0c, s[3..7].to_vec()));
        v.push(("x", 0x10, s[7..11].to_vec()));
        v.push(("y", 0x14, s[11..15].to_vec()));
        v.push(("ptype", 0x18, s[15..19].to_vec()));
        let mids: [(&str, usize, usize); 20] = [
            ("myhits", 0x20, 4),
            ("damage", 0x24, 4),
            ("inside_down", 0x28, 2),
            ("up", 0x2a, 2),
            ("down", 0x2c, 2),
            ("down_who", 0x2e, 2),
            ("uid", 0x30, 2),
            ("hold_frames", 0x32, 2),
            ("near_o", 0x34, 2),
            ("near_who", 0x36, 2),
            ("healing", 0x38, 2),
            ("infiltrated", 0x3a, 1),
            ("damage_frac", 0x3b, 1),
            ("mylos", 0x3c, 1),
            ("targeted", 0x3d, 1),
            ("inside_down_who", 0x3e, 1),
            ("up_who", 0x3f, 1),
            ("visible", 0x40, 1),
            ("launch_frames", 0x41, 1),
            ("launch", 0x42, 0),
        ];
        for (n, off, sz) in mids {
            if sz > 0 {
                v.push((n, off, m[off - 0x20..off - 0x20 + sz].to_vec()));
            }
        }
        let bodies: [(&str, usize, usize); 50] = [
            ("collide_frame", 0x48, 4),
            ("damage_frame", 0x4c, 4),
            ("angle", 0x50, 4),
            ("rare", 0x54, 4),
            ("dest_angle", 0x58, 4),
            ("trench_angle", 0x5c, 4),
            ("tolerance", 0x60, 4),
            ("queue_time", 0x64, 4),
            ("unit_masks", 0x68, 4),
            ("unit_masks2", 0x6c, 4),
            ("orders_x", 0x70, 4),
            ("orders_y", 0x74, 4),
            ("los_x", 0x78, 4),
            ("los_y", 0x7c, 4),
            ("group", 0x80, 2),
            ("inside_up", 0x82, 2),
            ("supply", 0x84, 2),
            ("hero", 0x86, 2),
            ("collide", 0x88, 2),
            ("collide_o", 0x8a, 2),
            ("collide_guy", 0x8c, 2),
            ("o_up", 0x8e, 2),
            ("o_down", 0x90, 2),
            ("gather_down", 0x92, 2),
            ("good_obj", 0x94, 2),
            ("mana_burn", 0x96, 2),
            ("spell_time", 0x98, 2),
            ("myspeed", 0x9a, 2),
            ("myarmor", 0x9c, 2),
            ("attrition", 0x9e, 2),
            ("num_queued", 0xa0, 2),
            ("cavarch_o", 0xa2, 2),
            ("damage_o", 0xa4, 2),
            ("cavarch_uid", 0xa6, 2),
            ("cavarch_who", 0xa8, 1),
            ("damage_who", 0xa9, 1),
            ("form", 0xaa, 1),
            ("form_mod", 0xab, 1),
            ("full", 0xac, 1),
            ("waiting", 0xad, 1),
            ("recharging", 0xae, 1),
            ("path_recursion", 0xaf, 1),
            ("idle", 0xb0, 1),
            ("stance", 0xb1, 1),
            ("safe", 0xb2, 1),
            ("collide_who", 0xb3, 1),
            ("inside_up_who", 0xb4, 1),
            ("guy_mark", 0xb5, 1),
            ("play", 0xb6, 1),
            ("_end", 0xb7, 0),
        ];
        for (n, off, sz) in bodies {
            if sz > 0 {
                v.push((n, off, b[off - 0x48..off - 0x48 + sz].to_vec()));
            }
        }
        v
    }

    /// Fields written by callees this module does not transcribe (see the
    /// module doc). Everything else must match retail byte-for-byte.
    const UNTRANSCRIBED: &[&str] = &[
        "z",        // World::get_height 0x008544a0
        "x", "y",   // come_out placement (set_new_location 0x005f8d20)
        "orders_x", "orders_y", // set_new_location
        "down", "down_who",     // Object::add_to_world 0x0064d8c0
        "myhits",   // Unit::update_hits 0x0060e930
        "mylos",    // Unit::update_los 0x0060e4d0
        "myspeed",  // Unit::update_speed 0x006055c0
        "myarmor",  // Unit::update_armor 0x006054c0
    ];

    /// Oracle: stride-15 capture, f85 → f100. At f85 the four AI cities
    /// [2|4|5|6][2000] carry `QueueItem[0] = (8400, PEASANTS)`; retail's
    /// counter reaches 9800 at f99 and the item completes at f100 (the new
    /// unit `[who][6]` is captured in the same frame it was created, before
    /// the Unit plane processes it). Drive the completion from the f85 state
    /// and compare every walked field of the new Unit, the Objects marks and
    /// the Build queue with retail f100; the Leader counters are compared as
    /// deltas (the other 14 frames move unrelated Leader state).
    #[test]
    fn citizen_completion_matches_stride15_f100() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/live/frame-pairs/20261004-081342-stride15");
        if !dir.join("manifest.json").exists() {
            eprintln!("no capture dir; skipping");
            return;
        }
        let st = steps(&dir);
        let a = load_frame(&dir, &st, 85);
        let b = load_frame(&dir, &st, 100);
        let mut mismatches = Vec::new();
        let mut matched = 0usize;
        let mut untranscribed_diff = 0usize;
        for owner in [2usize, 4, 5, 6] {
            let mut ours = a.clone();
            let mut effects = Vec::new();
            let (jc, ty) = queue_item(&ours, owner, 2000, 0).unwrap();
            assert_eq!((jc, ty), (8400, 0x32), "owner {owner} queue head");
            // total = 9800: 8400 + 14 frames of Constants+0x228 (100) reaches it at f99.
            let total = 9800;
            let comp = complete_queue_item_at(&mut ours, owner, 2000, 0, total, &mut effects).expect("completion ran");
            assert_eq!(comp.finished, 1, "owner {owner}: finished() must produce");
            assert_eq!(comp.unit_o, Some(6), "owner {owner}: new unit slot");
            assert_eq!(comp.rng_draws_unaccounted, 2, "owner {owner}: Guy::init_real + come_out coin");
            // Objects marks / ctr
            assert_eq!(unit_mark(&ours, owner), unit_mark(&b, owner), "owner {owner} unit_mark");
            assert_eq!(obj_ctr(&ours, owner), obj_ctr(&b, owner), "owner {owner} obj_ctr");
            // Build queue + queued + build_masks
            let (Some(Obj::Build(co)), Some(Obj::Build(cb))) = (&ours.objects.lists[owner].elems[2000], &b.objects.lists[owner].elems[2000]) else { panic!() };
            assert_eq!(co.queue, cb.queue, "owner {owner} queue rows");
            assert_eq!(co.body[0x12], cb.body[0x12], "owner {owner} queued");
            assert_eq!(co.wall_body[0x18..0x1a], cb.wall_body[0x18..0x1a], "owner {owner} build_masks");
            // New unit
            let (Some(Obj::Unit(uo)), Some(Obj::Unit(ub))) = (&ours.objects.lists[owner].elems[6], &b.objects.lists[owner].elems[6]) else {
                panic!("owner {owner}: new unit missing")
            };
            assert_eq!(ours.objects.lists[owner].present, b.objects.lists[owner].present, "owner {owner} presence plane");
            assert_eq!(ours.objects.lists[owner].types, b.objects.lists[owner].types, "owner {owner} type plane");
            assert_eq!((uo.base.sub.tag, uo.base.tag, uo.tag), (ub.base.sub.tag, ub.base.tag, ub.tag), "owner {owner} tags");
            assert_eq!((uo.path.cap, uo.path.len, uo.path.inc), (ub.path.cap, ub.path.len, ub.path.inc), "owner {owner} path header");
            assert_eq!(uo.orders.orders.len(), ub.orders.orders.len(), "owner {owner} orders");
            for ((n, off, vo), (_, _, vb)) in unit_fields(uo).into_iter().zip(unit_fields(ub)) {
                if vo == vb {
                    matched += 1;
                } else if UNTRANSCRIBED.contains(&n) {
                    untranscribed_diff += 1;
                } else {
                    mismatches.push(format!("owner {owner} [{owner}][6] +{off:#x} {n}: ours {vo:02x?} retail {vb:02x?}"));
                }
            }
            // Guys are the untranscribed Guy::init_real rows.
            assert_eq!(ub.guys.elems.iter().flatten().count(), 1, "owner {owner}: retail has one Guy");
            // Leader deltas: counters this path owns must move exactly as retail did.
            let (la, lb, lo) = (&a.leaders.slots[owner], &b.leaders.slots[owner], &ours.leaders.slots[owner]);
            for off in [0x808usize, 0x93c, 0x940, 0x978, 0x6274] {
                let i = off - 8;
                let (ra, rb, ro) = (
                    i32::from_le_bytes(la.body[i..i + 4].try_into().unwrap()),
                    i32::from_le_bytes(lb.body[i..i + 4].try_into().unwrap()),
                    i32::from_le_bytes(lo.body[i..i + 4].try_into().unwrap()),
                );
                assert_eq!(ro - ra, rb - ra, "owner {owner} Leader+{off:#x}: retail {ra}->{rb}, ours {ro}");
            }
            for off in [0x5762usize, 0x5a86] {
                let i = off - 8;
                let (ra, rb, ro) = (
                    i16::from_le_bytes([la.body[i], la.body[i + 1]]),
                    i16::from_le_bytes([lb.body[i], lb.body[i + 1]]),
                    i16::from_le_bytes([lo.body[i], lo.body[i + 1]]),
                );
                assert_eq!(ro - ra, rb - ra, "owner {owner} Leader+{off:#x}: retail {ra}->{rb}, ours {ro}");
            }
            assert_eq!(lo.flags & 0x800000, lb.flags & 0x800000, "owner {owner} Leader.flags 0x800000");
            // Every other Leader body byte we touched must be one retail also moved.
            for i in 0..la.body.len() {
                if lo.body[i] != la.body[i] && lb.body[i] == la.body[i] {
                    mismatches.push(format!("owner {owner} Leader+{:#x}: ours {:02x} retail kept {:02x}", i + 8, lo.body[i], la.body[i]));
                }
            }
            // Game+0x6d8 moves by the number of units created in the 15 frames (4 cities).
            let g = |s: &Save| game_i32(s, 0x6d8);
            assert_eq!(g(&ours) - g(&a), 1, "owner {owner} Game+0x6d8 (+1 per unit)");
            assert_eq!(g(&b) - g(&a), 4, "retail Game+0x6d8 (+4 units in f85..f100)");
            eprintln!("owner {owner}: {} effects", effects.len());
        }
        eprintln!("unit fields: matched={matched} untranscribed_diff={untranscribed_diff} mismatches={}", mismatches.len());
        for m in &mismatches {
            eprintln!("  MISMATCH {m}");
        }
        assert!(mismatches.is_empty(), "walked fields the transcription got wrong");
        assert!(matched >= 4 * 50, "expected most fields to match, got {matched}");
    }

    /// Control: a completion that `finished()` blocks on population must
    /// leave the queue, marks and Leader untouched except `job_counter`.
    #[test]
    fn population_block_leaves_state() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/live/frame-pairs/20261004-081342-stride15");
        if !dir.join("manifest.json").exists() {
            return;
        }
        let st = steps(&dir);
        let a = load_frame(&dir, &st, 85);
        let mut ours = a.clone();
        // Force the cap below used + cost.
        set_leader_bytes(&mut ours, 2, 0x7e4, &0i32.to_le_bytes());
        let before = ours.clone();
        let mut effects = Vec::new();
        let comp = complete_queue_item_at(&mut ours, 2, 2000, 0, 9800, &mut effects).unwrap();
        assert_eq!(comp.finished, 0);
        assert_eq!(unit_mark(&ours, 2), unit_mark(&before, 2));
        assert_eq!(ours.leaders.slots[2].body, before.leaders.slots[2].body);
        let (jc, _) = queue_item(&ours, 2, 2000, 0).unwrap();
        assert_eq!(jc, 9800);
        let (Some(Obj::Build(co)), Some(Obj::Build(cb))) = (&ours.objects.lists[2].elems[2000], &before.objects.lists[2].elems[2000]) else { panic!() };
        assert_eq!(co.body[0x12], cb.body[0x12]);
    }

    /// Diagnostic: every pair where a unit band or a Build queue changed.
    #[test]
    #[ignore]
    fn dump_unit_spawns() {
        for dir in capture_dirs() {
            let st = steps(&dir);
            println!("== {}", dir.display());
            for k in 0..st.len().saturating_sub(1) {
                let a = load_frame(&dir, &st, st[k].0);
                let b = load_frame(&dir, &st, st[k + 1].0);
                for owner in 0..9 {
                    let (ma, mb) = (unit_mark(&a, owner), unit_mark(&b, owner));
                    if ma != mb {
                        println!("  f{}->f{} owner {owner}: unit_mark {ma}->{mb}", st[k].0, st[k + 1].0);
                    }
                    for slot in 0..a.objects.lists[owner].elems.len().min(b.objects.lists[owner].elems.len()) {
                        let (Some(Obj::Build(xa)), Some(Obj::Build(xb))) = (&a.objects.lists[owner].elems[slot], &b.objects.lists[owner].elems[slot]) else {
                            continue;
                        };
                        if xa.queue != xb.queue || xa.body.get(0x12) != xb.body.get(0x12) {
                            let row = |q: &[u8]| {
                                q.chunks_exact(18)
                                    .map(|r| format!("({},{})", i32::from_le_bytes(r[0..4].try_into().unwrap()), i16::from_le_bytes([r[4], r[5]])))
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            };
                            println!(
                                "  f{}->f{} [{owner}][{slot}] queued {:?}->{:?} {} -> {}",
                                st[k].0,
                                st[k + 1].0,
                                xa.body.get(0x12),
                                xb.body.get(0x12),
                                row(&xa.queue),
                                row(&xb.queue)
                            );
                        }
                    }
                }
            }
        }
    }
}
