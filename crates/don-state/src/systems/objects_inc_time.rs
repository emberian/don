//! Step 15: `Objects::inc_time` 0x0065db70 and its per-object children —
//! `Unit::inc_time` 0x00610b40 → `Guy::inc_time` 0x005d9e10 (the per-guy
//! animation clocks), `Unit::execute_events` 0x0060edc0 → `Guy::execute_events`
//! 0x005d99c0 (animation-event release), and `Wall::inc_time` 0x0063fb60 (the
//! Build plane's `+0xa0` slot; `Build` has no override). All VAs from
//! `rise.pdb` via `tools/pdb/lookup.py`; control flow from
//! `re/decomp-all/<EA>.c`, call arguments and vtable slots confirmed with a
//! Capstone listing of the mapped image and a read of the retail vtables
//! (`Build::vftable` 0x00b42174, `Unit::vftable` 0x00b417d0,
//! `Animal::vftable` 0x00b4145c).
//!
//! ## Shape of the step (`0x0065db70..0x0065dcd8`)
//!
//! ```text
//! 0065db82  call Nuke::do_damage 0x0092bc80            ; head call, unconditional
//! 0065db87  edi = &LeaderData[0] 0x00e3a390            ; TEN records, stride 0x6eec
//!   test byte [edi], 1                                  ; owner live
//!   for o in 0 .. Objects+0x15c+owner*4 (unit_mark):    ; unit band
//!     if obj.flags & 1: call [vt+0xa0] (inc_time); call [vt+0x154] (execute_events)
//!   for o in 2000 .. Objects+0x184+owner*4 (build_mark): ; build band — no wall band
//!     if obj.flags & 1: call [vt+0xa0] (inc_time)
//! 0065dc30  goods band: Good::inc_time is a bare `ret`, skipped outright
//! 0065dc60  ammo pool (Objects+0x120 / +0x12c): flags & 3 -> Ammo::inc_time 0x0067d380
//! 0065dc90  death ring (Objects+0x140 / +0x14c, stride 0xa4): valid -> DeathObj::inc_time
//! 0065dcb2  Farms::inc_time 0x008d8600; Doober::inc_time 0x00846770; Surf::inc_time 0x008a1a00
//! ```
//!
//! Unlike step 14 the owner walk is **not** frame-rotated. Vtable slots on the
//! Unit/Animal planes: `+0xa0` = `Unit::inc_time`, `+0x154` =
//! `Unit::execute_events`, `+0x8` = `ItemData::is_valid_item`, `+0xbc` =
//! `UnitData::is_on_map`. On the Build plane `+0xa0` = `Wall::inc_time`,
//! `+0x154` = `Object::execute_events` (bare `ret`), `+0xac` =
//! `ListBox::get_listbox` (COMDAT-folded `return this`), `+0x20`/`+0xbc`
//! fold to `return 1`, `+0x8` folds to `return 0`.
//!
//! ## What is transcribed (and executes)
//!
//! * the traversal above (unit band + build band, ten owners, `flags & 1`);
//! * `Unit::inc_time`: gate `inside_up < 0 || type == 0x34 || type == 0x35`,
//!   then `Guy::inc_time` for `guys.list[0..guy_mark]` and again for
//!   `guys.list[squad_size..guys.length]`;
//! * `Guy::inc_time` main arm (`guy_num < squad_size || anim_class == 8`):
//!   `last_time = cur_time; cur_time += inc` with `inc` = 1, 2 when
//!   `guy_flags & 4` and the anim is an attack (class 0xc), 0 when the unit's
//!   `unit_masks2 & 0x10`; and the follower arm (`guy_num >= squad_size`,
//!   anim class != 8): `cur_anim = guy0.cur_anim; cur_time = guy0.cur_time`;
//! * `Wall::inc_time` gates: `is_active` (`flags & 4`), `has_objmask
//!   (0x80000000)` (= `ObjectType.obj_masks & 0x80000000`), `!is(0x209,1)`,
//!   `!is(0x20a,1)`, and the full `recharging` (+0x7a) state machine — which
//!   needs `AnimationPacket::get_game_frames(8 / 0xc)` of the building's
//!   render gpiece (see below).
//!
//! ## What is not (fields untouched, noted in `effects`)
//!
//! * `Guy::inc_time` when `cur_time + inc >= end_time` (unsigned): the
//!   anim-end loop calls `Guy::set_anim` 0x005da300 (4,723 B; reads the
//!   gpiece's `AnimationPacket`, draws `game_random` 0–1 times). No shipped
//!   `.bha` data is in the repo, so the guy is left untouched — including the
//!   `last_time`/`cur_time` pre-writes retail makes before the loop (set_anim
//!   rewrites both, so the pre-writes are not observable without it).
//! * `Guy::inc_time` `queued_attack != 0` release (set_anim) and the
//!   captain→extras `Guy::set_new_location` 0x005d86f0 sync.
//! * `Unit::execute_events` → `Guy::execute_events` 0x005d99c0: resolves the
//!   gpiece's animation event list and dispatches
//!   `GraphicEvents::execute_game_events` 0x008e48e0 (missile launch /
//!   `Objects::add_ammo` at event type 1); its only direct walked write is
//!   `Unit.trench_angle` (+0x5c) on `domain == 1 && is(0x15f,0)` units.
//!   Event tables are graphics data — not available.
//! * `Wall::inc_time` → `Wall::update_hits(0)` 0x0063f0d0 on **inactive**
//!   (under-construction) buildings: recomputes `myhits` (+0x20) and
//!   `construct_hits` (+0x54) from `ObjectData::hits` plus tribe/tech/age
//!   modifiers (1,509 B). Left for a follow-up.
//! * `Wall::inc_time` `recharging` state machine: its bounds are
//!   `get_game_frames(8) - 1` and `-get_game_frames(0xc)` of the gpiece's
//!   `AnimationPacket` ([0x00c06214]+0x728[render_gpiece]+0x54); with no
//!   packet the whole block is skipped by retail too. `render_gpiece` (+0x68)
//!   is runtime-only, and the `.bha` files are not shipped here, so
//!   [`AnimFrames`] returns `None` and the block leaves +0x7a untouched.
//! * the tail: `Nuke::do_damage` (empty nuke array in these captures),
//!   `Ammo::inc_time`, `DeathObj::inc_time`, `Farms::inc_time` (0–2
//!   `game_random` per farm record), `Doober`/`Surf` (presentation,
//!   `internal_random` only).
//!
//! ## Attribution correction
//!
//! The Woodcutter-camp `+0x7a += 1` seen ~230 B/frame in the stride-1
//! capture is **not** `Wall::inc_time`. In this body the `near_o < 0 ||
//! build_masks < 0` arm (0x006401fb) *decrements* `recharging`, and every
//! camp in the capture has `near_o == -1`. The incrementers are
//! `Unit::do_gather` 0x005ef2a0 (+0x7a += 1, build_masks |= 0x800),
//! `Unit::do_non_flat_gather` 0x005f0170 and `Wall::do_construct`
//! 0x006434d0 — all under `Unit::process` (step 14), one per site per frame
//! gated by `build_masks & 0x800`, which `Wall::process` clears. That is the
//! `objects_process.rs` lane's write; nothing here derives it.
//!
//! RNG: none of the transcribed paths draw `game_random`. `GuyOut::graph_inc_frame`
//! 0x005dd200 (called at the end of every `Guy::inc_time` for `who < 8`) draws
//! only `internal_random` 0x00eb697c and writes no walked field.

use crate::sections::{Build, Obj, Save, Unit};
use crate::tick::StepStatus;

pub const STATUS: StepStatus = StepStatus::Partial;

/// `DAT_00af4370`: UnitAnim → anim class (38 entries read from the mapped
/// image at 0x00af4370). Class 8 = death anims, class 0xc = attack anims.
const ANIM_CLASS: [i32; 38] = [
    0, 0, 0, 0, 0, 0, 0, 8, 8, 8, 10, 12, 12, 12, 12, 15, 15, 15, 15, 15, 15, 21, 22, 23, 24, 25, 8, 27, 8, 29, 8, 31, 8, 33, 34, 35,
    36, 37,
];

fn anim_class(cur_anim: i8) -> Option<i32> {
    usize::try_from(cur_anim).ok().and_then(|i| ANIM_CLASS.get(i).copied())
}

/// `AnimationPacket::get_game_frames(anim)` 0x00918cc0 for the building's
/// render gpiece. The packet lives in the graphics pack (`.bha`), which is
/// not shipped with this repo; `None` means "unknown", and every consumer
/// leaves its fields untouched.
pub struct AnimFrames;

impl AnimFrames {
    fn game_frames(&self, _save: &Save, _b: &Build, _anim: i32) -> Option<i32> {
        None
    }
}

// ---------------------------------------------------------------------------
// Traversal — 0x0065db87..0x0065dc2a
// ---------------------------------------------------------------------------

pub fn run(save: &mut Save, effects: &mut Vec<String>) {
    // TODO(va 0x0092bc80) Nuke::do_damage — head call; walks the nuke array
    // [0x00c0a7fc] (empty in the idle captures) into Object::do_damage.
    for owner in 0..10usize {
        if save.leaders.slots.get(owner).map(|l| l.flags & 1).unwrap_or(0) == 0 {
            continue;
        }
        if owner >= save.objects.lists.len() {
            continue;
        }
        let umark = unit_mark(save, owner).max(0) as usize;
        for slot in 0..umark {
            if is_live(save, owner, slot) {
                unit_inc_time(save, owner, slot, effects);
                unit_execute_events(save, owner, slot, effects);
            }
        }
        let bmark = build_mark(save, owner).max(2000) as usize;
        for slot in 2000..bmark {
            if is_live(save, owner, slot) {
                wall_inc_time(save, owner, slot, effects);
            }
        }
    }
    // TODO(va 0x0067d380) Ammo::inc_time over Objects.ammo (flags & 3).
    // TODO(va 0x008d5240) DeathObj::inc_time over Objects.deaths (valid != 0);
    //   needs get_game_frames(cur_anim) of the corpse gpiece.
    // TODO(va 0x008d8600) Farms::inc_time — 0..2 game_random draws per farm.
    // Doober::inc_time 0x00846770 / Surf::inc_time 0x008a1a00: presentation.
}

fn is_live(save: &Save, owner: usize, slot: usize) -> bool {
    matches!(save.objects.lists[owner].elems.get(slot), Some(Some(o)) if o.obj_flags() & 1 != 0)
}

/// `Objects` scalar block: `[0x1f4..0x1fc) [0x154..0x15c) unit_mark[9]
/// build_mark[9] wall_mark[9] obj_ctr[9]` (Objects::walk_data 0x006541e0:
/// unit_mark = image 0x15c.., build_mark = image 0x184..).
fn unit_mark(save: &Save, owner: usize) -> i32 {
    let o = 16 + owner * 4;
    i32::from_le_bytes(save.objects.scalars[o..o + 4].try_into().unwrap())
}

fn build_mark(save: &Save, owner: usize) -> i32 {
    let o = 16 + 36 + owner * 4;
    i32::from_le_bytes(save.objects.scalars[o..o + 4].try_into().unwrap())
}

// ---------------------------------------------------------------------------
// Unit image + type readers
// ---------------------------------------------------------------------------

/// Unit image offsets inside the serialized `Unit.body` (UnitData
/// +0x48..+0xb7, 111 bytes).
const U_BODY_BASE: usize = 0x48;

fn unit_of(o: &Obj) -> Option<&Unit> {
    match o {
        Obj::Unit(u) => Some(u),
        Obj::Animal(a) => Some(&a.unit),
        Obj::Build(_) => None,
    }
}

fn unit_of_mut(o: &mut Obj) -> Option<&mut Unit> {
    match o {
        Obj::Unit(u) => Some(u),
        Obj::Animal(a) => Some(&mut a.unit),
        Obj::Build(_) => None,
    }
}

/// `SubObject.ptype` (+0x18) — serialized as the type index at sub.body[15..19].
fn unit_ptype(u: &Unit) -> Option<i32> {
    u.base.sub.body.get(15..19).map(|b| i32::from_le_bytes(b.try_into().unwrap()))
}

fn unit_i16(u: &Unit, off: usize) -> Option<i16> {
    u.body.get(off - U_BODY_BASE..off - U_BODY_BASE + 2).map(|b| i16::from_le_bytes([b[0], b[1]]))
}

fn unit_u32(u: &Unit, off: usize) -> Option<u32> {
    u.body.get(off - U_BODY_BASE..off - U_BODY_BASE + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

fn unit_i8(u: &Unit, off: usize) -> Option<i8> {
    u.body.get(off - U_BODY_BASE).map(|&b| b as i8)
}

/// Unit-type tail reader. `Type::walk_rules_data` emits the unit tail as
/// image[0x2b4..0x2cc) + [0x2d4..0x2dc) + [0x2dc..0x2e0) + [0x2e0..0x5d4)
/// into `TypeRec.ext`, so the map is piecewise.
fn unit_type_i32(save: &Save, ty: i32, off: usize) -> Option<i32> {
    let t = save.rules_tail.rules.types.get(usize::try_from(ty).ok()?)?;
    let i = match off {
        0x2b4..=0x2cb => off - 0x2b4,
        0x2d4..=0x2db => 24 + (off - 0x2d4),
        0x2dc..=0x2df => 32 + (off - 0x2dc),
        0x2e0..=0x5d3 => 36 + (off - 0x2e0),
        _ => return None,
    };
    t.ext.get(i..i + 4).map(|b| i32::from_le_bytes(b.try_into().unwrap()))
}

/// `ObjectType.obj_masks` (+0x1e4) = `TypeRec.obj_mid[0..4]`.
fn type_obj_masks(save: &Save, ty: i32) -> Option<u32> {
    let t = save.rules_tail.rules.types.get(usize::try_from(ty).ok()?)?;
    t.obj_mid.get(0..4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

/// `ObjectTypeData::is(what, strict)` 0x0065f7d0 for the strict arm only:
/// exact match, else the serialized `is_strict_list` (`arr1`); an empty list
/// falls to `is_slow` strict (unit types only — never true for a building).
fn type_is_strict(save: &Save, ty: i32, what: i32) -> bool {
    if ty == what {
        return true;
    }
    let Some(t) = usize::try_from(ty).ok().and_then(|i| save.rules_tail.rules.types.get(i)) else { return false };
    let mut it = t.arr1.data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]) as i32).peekable();
    if it.peek().is_none() {
        // ObjectTypeData::is_slow 0x00661ae0 strict arm: unit types only
        // (0x32..=0x19d); graft (+0x25c) match unless the target's +0x2b4
        // carries 0x1000000. Callers here are building types -> false.
        if !(0x32..=0x19d).contains(&ty) {
            return false;
        }
        let graft = t.obj_mid.get(0x25c - 0x1e4..0x260 - 0x1e4).map(|b| i32::from_le_bytes(b.try_into().unwrap()));
        return graft == Some(what) && unit_type_i32(save, what, 0x2b4).map(|f| f & 0x1000000 == 0).unwrap_or(false);
    }
    it.any(|v| v == what)
}

// ---------------------------------------------------------------------------
// Guy row accessors — GuyData::walk_data 0x005e0210 walks [+0x8, +0xa3), so
// row[i] = GuyData + 8 + i.
// ---------------------------------------------------------------------------

const G_BASE: usize = 8;
const G_CUR_TIME: usize = 0x74;
const G_END_TIME: usize = 0x78;
const G_LAST_TIME: usize = 0x7c;
const G_GUY_FLAGS: usize = 0x9a;
const G_CUR_ANIM: usize = 0x9c;
const G_QUEUED_ATTACK: usize = 0xa0;
const G_GUY_NUM: usize = 0xa2;

fn g_u32(row: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(row[off - G_BASE..off - G_BASE + 4].try_into().unwrap())
}
fn g_set_u32(row: &mut [u8], off: usize, v: u32) {
    row[off - G_BASE..off - G_BASE + 4].copy_from_slice(&v.to_le_bytes());
}
fn g_i16(row: &[u8], off: usize) -> i16 {
    i16::from_le_bytes([row[off - G_BASE], row[off - G_BASE + 1]])
}
fn g_i8(row: &[u8], off: usize) -> i8 {
    row[off - G_BASE] as i8
}
fn g_set_i8(row: &mut [u8], off: usize, v: i8) {
    row[off - G_BASE] = v as u8;
}

// ---------------------------------------------------------------------------
// Unit::inc_time 0x00610b40
// ---------------------------------------------------------------------------

/// ```text
/// 00610b43  cmp word [esi+0x82], 0 ; jl body          ; inside_up < 0
/// 00610b4d  eax = ptype->type_index; cmp 0x34 / 0x35  ; Scholar exceptions
/// 00610b60  for edi in 0 .. (i8)guy_mark (+0xb5): ecx = guys.list[edi]; call Guy::inc_time
/// 00610b8a  for edi in ptype->squad_size (+0x304) .. guys.length (+0xe8): same
/// ```
fn unit_inc_time(save: &mut Save, owner: usize, slot: usize, effects: &mut Vec<String>) {
    let Some(Some(obj)) = save.objects.lists[owner].elems.get(slot) else { return };
    let Some(u) = unit_of(obj) else { return };
    if u.body.len() != 111 {
        return;
    }
    let Some(ty) = unit_ptype(u) else { return };
    let inside_up = unit_i16(u, 0x82).unwrap();
    if !(inside_up < 0 || ty == 0x34 || ty == 0x35) {
        return;
    }
    let guy_mark = unit_i8(u, 0xb5).unwrap().max(0) as usize;
    let Some(squad_size) = unit_type_i32(save, ty, 0x304) else {
        effects.push(format!("Objects.lists[{owner}][{slot}]: type {ty} has no unit-type record (squad_size); untouched"));
        return;
    };
    let nguys = u.guys.elems.len();
    let tag = format!("Objects.lists[{owner}][{slot}]");
    let mut order: Vec<usize> = (0..guy_mark).collect();
    if squad_size >= 0 && (squad_size as usize) < nguys {
        order.extend(squad_size as usize..nguys);
    }
    for g in order {
        guy_inc_time(save, owner, slot, g, ty, squad_size, &tag, effects);
    }
}

/// `Guy::inc_time` 0x005d9e10 — see module docs for the arm map.
#[allow(clippy::too_many_arguments)]
fn guy_inc_time(save: &mut Save, owner: usize, slot: usize, g: usize, ty: i32, squad_size: i32, tag: &str, effects: &mut Vec<String>) {
    let Some(Some(obj)) = save.objects.lists[owner].elems.get_mut(slot) else { return };
    let Some(u) = unit_of_mut(obj) else { return };
    let masks2 = unit_u32(u, 0x6c).unwrap();
    let Some(Some(row0)) = u.guys.elems.first() else { return };
    let (anim0, cur0) = (g_i8(&row0.data, G_CUR_ANIM), g_u32(&row0.data, G_CUR_TIME));
    let Some(Some(row)) = u.guys.elems.get_mut(g) else { return };
    let row = &mut row.data;
    if row.len() != 155 {
        return;
    }
    let cur_anim = g_i8(row, G_CUR_ANIM);
    let Some(class) = anim_class(cur_anim) else {
        effects.push(format!("{tag}.guys[{g}]: cur_anim {cur_anim} outside DAT_00af4370; untouched"));
        return;
    };
    // 005d9e2f  ecx = 1; if (guy_flags & 4) && class == 0xc: ecx = 2
    // 005d9e79  eax = 0; if !(unit.unit_masks2 & 0x10): eax = ecx
    let mut inc: u32 = 1;
    if g_i16(row, G_GUY_FLAGS) & 4 != 0 && class == 0xc {
        inc = 2;
    }
    if masks2 & 0x10 != 0 {
        inc = 0;
    }
    let guy_num = g_i8(row, G_GUY_NUM) as i32;
    if guy_num < squad_size || class == 8 {
        // 005d9f71  last_time = cur_time; cur_time += inc; cmp cur_time, end_time; jb skip
        let cur = g_u32(row, G_CUR_TIME);
        let end = g_u32(row, G_END_TIME);
        let ncur = cur.wrapping_add(inc);
        if ncur >= end {
            // TODO(va 0x005da300) anim-end loop -> Guy::set_anim (needs the
            // gpiece AnimationPacket; 0-1 game_random). Untouched.
            effects.push(format!("{tag}.guys[{g}]: cur_time {cur}+{inc} reaches end_time {end}: set_anim 0x005da300 untranscribed; untouched"));
            return;
        }
        if class != 0xc && g_i8(row, G_QUEUED_ATTACK) != 0 && cur_anim != 8 {
            // TODO(va 0x005da300) queued_attack release: queued_attack = 0;
            // set_anim(queued < 2 ? 0xb : queued, 0, queued < 2). Untouched.
            effects.push(format!("{tag}.guys[{g}]: queued_attack release untranscribed; untouched"));
            return;
        }
        g_set_u32(row, G_LAST_TIME, cur);
        g_set_u32(row, G_CUR_TIME, ncur);
        effects.push(format!("{tag}.guys[{g}].cur_time {cur} -> {ncur}, last_time -> {cur}"));
        // 005da08d  if guy_num == 0 && class == 0xc && squad_size < guys.length:
        //   for extras: Guy::set_new_location(des_x, des_y, 1); cur_anim/cur_time copy
        // TODO(va 0x005d86f0) — the copy half is reproduced by the follower
        // arm when Unit::inc_time's second loop reaches them; set_new_location
        // (position writes) is not.
        let _ = ty;
    } else {
        // 005d9eb5  follower arm: if cur_anim != guy0.cur_anim: Guy+0xd0 = 0 (runtime);
        //           cur_anim = guy0.cur_anim; cur_time = guy0.cur_time; Log::say(...)
        if g == 0 {
            return;
        }
        let old_anim = cur_anim;
        let old_cur = g_u32(row, G_CUR_TIME);
        if old_anim != anim0 || old_cur != cur0 {
            g_set_i8(row, G_CUR_ANIM, anim0);
            g_set_u32(row, G_CUR_TIME, cur0);
            effects.push(format!("{tag}.guys[{g}] follower: cur_anim {old_anim} -> {anim0}, cur_time {old_cur} -> {cur0}"));
        }
    }
    // 005da2d2  if who < 8: GuyOut::graph_inc_frame 0x005dd200 — presentation
    // (internal_random only; no walked writes).
}

// ---------------------------------------------------------------------------
// Unit::execute_events 0x0060edc0
// ---------------------------------------------------------------------------

/// ```text
/// 0060edc4  call [vt+0x8]  (ItemData::is_valid_item) ; je verify
/// 0060edd1  call [vt+0xbc] (UnitData::is_on_map)     ; je verify
/// 0060eddb  test byte [esi+0x6c], 0x10               ; jne verify   (unit_masks2)
/// execute: for g in 0..guy_mark: Guy::execute_events 0x005d99c0
/// verify:  for g in 0..guy_mark: GraphicEvents::verify_load(guy.gpiece) — presentation
/// ```
fn unit_execute_events(save: &mut Save, owner: usize, slot: usize, effects: &mut Vec<String>) {
    let Some(Some(obj)) = save.objects.lists[owner].elems.get(slot) else { return };
    let Some(u) = unit_of(obj) else { return };
    if u.body.len() != 111 {
        return;
    }
    let on_map = unit_i16(u, 0x82).unwrap() < 0; // UnitData::is_on_map 0x0046ce30: (u16)inside_up >> 15
    let masks2 = unit_u32(u, 0x6c).unwrap();
    if on_map && masks2 & 0x10 == 0 {
        // TODO(va 0x005d99c0) Guy::execute_events per guy: animation event
        // release through GraphicEvents::execute_game_events 0x008e48e0 (event
        // tables are gpiece graphics data); direct walked write only on
        // domain==1 && is(0x15f,0): Unit.trench_angle +0x5c. Untouched.
        let guy_mark = unit_i8(u, 0xb5).unwrap().max(0);
        if guy_mark > 0 {
            effects.push(format!("Objects.lists[{owner}][{slot}]: execute_events x{guy_mark} untranscribed (gpiece event tables); untouched"));
        }
    }
}

// ---------------------------------------------------------------------------
// Wall::inc_time 0x0063fb60 (Build plane)
// ---------------------------------------------------------------------------

/// Build image reader by retail offset (same sub-range map as
/// `build_process::Img`).
struct BImg<'a>(&'a mut Build);

impl BImg<'_> {
    fn complete(&self) -> bool {
        self.0.base.sub.body.len() == 19 && self.0.base.mid.len() == 34 && self.0.wall_body.len() == 30 && self.0.body.len() == 22
    }
    fn slot(&mut self, off: usize) -> (&mut Vec<u8>, usize) {
        let b = &mut *self.0;
        match off {
            0x09..=0x1b => (&mut b.base.sub.body, off - 0x09),
            0x20..=0x41 => (&mut b.base.mid, off - 0x20),
            0x48..=0x65 => (&mut b.wall_body, off - 0x48),
            0x70..=0x85 => (&mut b.body, off - 0x70),
            _ => panic!("Build image offset {off:#x} is not serialized"),
        }
    }
    fn i16(&mut self, off: usize) -> i16 {
        let (v, i) = self.slot(off);
        i16::from_le_bytes([v[i], v[i + 1]])
    }
    fn i32(&mut self, off: usize) -> i32 {
        let (v, i) = self.slot(off);
        i32::from_le_bytes(v[i..i + 4].try_into().unwrap())
    }
    fn set_i16(&mut self, off: usize, x: i16) {
        let (v, i) = self.slot(off);
        v[i..i + 2].copy_from_slice(&x.to_le_bytes());
    }
}

fn wall_inc_time(save: &mut Save, owner: usize, slot: usize, effects: &mut Vec<String>) {
    let taken = save.objects.lists[owner].elems[slot].take();
    let Some(Obj::Build(mut b)) = taken else {
        save.objects.lists[owner].elems[slot] = taken;
        return;
    };
    wall_inc_time_body(save, &mut b, owner, slot, effects);
    save.objects.lists[owner].elems[slot] = Some(Obj::Build(b));
}

fn wall_inc_time_body(save: &Save, b: &mut Build, owner: usize, slot: usize, effects: &mut Vec<String>) {
    let flags = b.base.sub.flags;
    let mut img = BImg(b);
    if !img.complete() {
        return;
    }
    let tag = format!("Objects.lists[{owner}][{slot}]");
    let active = flags & 4 != 0; // vt+0x4c == WallData::is_active 0x00472350 -> flags & 4
    if !active {
        // TODO(va 0x0063f0d0) Wall::update_hits(0): myhits (+0x20) and
        // construct_hits (+0x54) recomputation for an under-construction
        // site. Untouched.
        effects.push(format!("{tag}: Wall::update_hits(0) 0x0063f0d0 untranscribed (inactive site); untouched"));
    }
    // 0063fc04..0063fdb0  vt+0x20 (folded `return 1`) -> queue/build particle
    // emission (GraphicPieces::emit_queue_particles 0x00907b20), gated on
    // is_active, BuildTypeData::is_military_trainer, queued, is_now_seen —
    // presentation only, no walked writes.
    // 0063fdb0  health_level() -> emit_build_particles — presentation.
    if !active {
        return; // 0063fddf  je 0x640419
    }
    // 0063fe5a..0063ff7c  more emit_build_particles — presentation.
    // 0063ff80  has_objmask(0x80000000) == 0 -> return
    let pt = img.i32(0x18);
    let masks = type_obj_masks(save, pt).unwrap_or(0);
    if masks & 0x8000_0000 == 0 {
        return;
    }
    // 0063ff99  is(0x209, 1) -> return; 0063ffc5  is(0x20a, 1) -> return
    if type_is_strict(save, pt, 0x209) || type_is_strict(save, pt, 0x20a) {
        return;
    }
    // 0063ffed  gpiece = render_gpiece (+0x68, WallOut::get_gpiece 0x006424d0)
    // 00640002  packet = [0x00c06214+0x728][gpiece]+0x54; if 0 -> 0x6402b1 (skip)
    // 0064001c  frames8 = packet->get_game_frames(8)
    let frames = AnimFrames;
    let Some(frames8) = frames.game_frames(save, img.0, 8) else {
        effects.push(format!("{tag}: recharging state machine needs AnimationPacket::get_game_frames(8) of render_gpiece; untouched"));
        return;
    };
    let near_o = img.i16(0x34);
    let build_masks = img.i16(0x60);
    let mut rech = img.i16(0x7a);
    let before = rech;
    if near_o < 0 || build_masks < 0 {
        // 006401fb  if (recharging < 0) recharging = frames8 - 1;
        //           if (recharging > 0) recharging -= 1;
        if rech < 0 {
            rech = (frames8 - 1) as i16;
        }
        if rech > 0 {
            rech -= 1;
        }
    } else if rech >= 0 && (rech as i32) < frames8 - 1 {
        // 00640065..006400c6  recharging += 1
        rech += 1;
    } else if img.i16(0x7c) < 0 {
        // 006400fa  attack_ox < 0 -> recharging = frames8 - 1
        rech = (frames8 - 1) as i16;
    } else {
        // 00640128  if (recharging >= 0) recharging = 0;
        // 0064015c  frames12 = packet->get_game_frames(0xc)
        // 00640198  if (recharging <= -frames12) recharging = -1 else recharging -= 1
        if rech >= 0 {
            rech = 0;
        }
        let Some(frames12) = frames.game_frames(save, img.0, 0xc) else { return };
        if (rech as i32) <= -frames12 {
            rech = -1;
        } else {
            rech -= 1;
        }
    }
    if rech != before {
        img.set_i16(0x7a, rech);
        effects.push(format!("{tag}.Build.recharging {before} -> {rech}"));
    }
    // 006402b4  if (recharging < 0): GraphicEvents::verify_load + (build_masks >= 0)
    //           execute_game_events for the attack anim — graphics event release.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{container, load};
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    fn capture_dir() -> Option<PathBuf> {
        let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/live/frame-pairs/20261004-075733-stride1");
        d.join("manifest.json").exists().then_some(d)
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

    fn i32_at(v: &[u8], o: usize) -> i32 {
        i32::from_le_bytes(v[o..o + 4].try_into().unwrap())
    }

    /// (image offset, byte) for every serialized Unit byte, guys keyed at
    /// 0x10000 + g*0x100 + row index.
    fn unit_image(u: &Unit) -> Vec<(usize, u8)> {
        let mut v = Vec::new();
        v.extend(u.base.sub.body.iter().enumerate().map(|(i, &x)| (0x9 + i, x)));
        v.extend(u.base.mid.iter().enumerate().map(|(i, &x)| (0x20 + i, x)));
        v.extend(u.body.iter().enumerate().map(|(i, &x)| (0x48 + i, x)));
        v.extend(u.path.data.iter().enumerate().map(|(i, &x)| (0x1000 + i, x)));
        for (oi, o) in u.orders.orders.iter().enumerate() {
            v.extend(o.payload.iter().enumerate().map(|(i, &x)| (0x2000 + oi * 0x100 + i, x)));
        }
        for (gi, g) in u.guys.elems.iter().enumerate() {
            if let Some(g) = g {
                v.extend(g.data.iter().enumerate().map(|(i, &x)| (0x10000 + gi * 0x100 + i, x)));
            }
        }
        v
    }

    fn build_image(b: &Build) -> Vec<(usize, u8)> {
        let mut v = Vec::new();
        v.push((0x7f, b.head[0]));
        v.push((0x83, b.head[1]));
        v.extend(b.base.sub.body.iter().enumerate().map(|(i, &x)| (0x9 + i, x)));
        v.extend(b.base.mid.iter().enumerate().map(|(i, &x)| (0x20 + i, x)));
        v.extend(b.wall_body.iter().enumerate().map(|(i, &x)| (0x48 + i, x)));
        v.extend(b.body.iter().enumerate().map(|(i, &x)| (0x70 + i, x)));
        v.extend(b.queue.iter().enumerate().map(|(i, &x)| (0x1000 + i, x)));
        v
    }

    fn image(o: &Obj) -> Vec<(usize, u8)> {
        match o {
            Obj::Build(b) => build_image(b),
            Obj::Unit(u) => unit_image(u),
            Obj::Animal(a) => unit_image(&a.unit),
        }
    }

    /// Lane gate: run `objects_inc_time::run` on retail frame N and account
    /// every Unit/Animal/Build byte against retail N+1 — `introduced` (we
    /// changed, retail kept) must be 0 on every pair; the guy clocks must
    /// explain bytes.
    #[test]
    fn inc_time_burndown_introduces_nothing() {
        let Some(dir) = capture_dir() else {
            eprintln!("no capture dir; skipping");
            return;
        };
        let st = steps(&dir);
        let (mut explained, mut unexplained, mut introduced) = (0usize, 0usize, 0usize);
        let mut explained_by: BTreeMap<String, usize> = BTreeMap::new();
        let mut unexplained_by: BTreeMap<String, usize> = BTreeMap::new();
        let mut introduced_rows = Vec::new();
        let mut deferred_set_anim = 0usize;
        let mut pairs = 0;
        for k in 0..st.len() - 1 {
            if st[k + 1].0 - st[k].0 != 1 {
                continue;
            }
            pairs += 1;
            let ra = container::load_svx(&dir.join(format!("{}.svx", st[k].1))).unwrap();
            let rb = container::load_svx(&dir.join(format!("{}.svx", st[k + 1].1))).unwrap();
            let a = load(&ra).unwrap().state;
            let b = load(&rb).unwrap().state;
            let mut ours = a.clone();
            let mut effects = Vec::new();
            run(&mut ours, &mut effects);
            for owner in 0..a.objects.lists.len() {
                for slot in 0..a.objects.lists[owner].elems.len().min(b.objects.lists[owner].elems.len()) {
                    let (Some(xa), Some(xb), Some(xo)) =
                        (&a.objects.lists[owner].elems[slot], &b.objects.lists[owner].elems[slot], &ours.objects.lists[owner].elems[slot])
                    else {
                        continue;
                    };
                    if xa.ty() != xb.ty() {
                        continue;
                    }
                    let (ia, ib, io) = (image(xa), image(xb), image(xo));
                    assert_eq!(ia.len(), io.len(), "[{owner}][{slot}] we changed the image length");
                    let mb: BTreeMap<usize, u8> = ib.into_iter().collect();
                    // Guys whose cur_anim (+0x9c) or end_time (+0x78) retail
                    // changed in this pair went through Guy::set_anim
                    // 0x005da300 under Unit::process (step 14) before our
                    // step ran; set_anim resets cur_time/last_time, so their
                    // clock bytes are that lane's.
                    let set_anim_guys: std::collections::BTreeSet<usize> = ia
                        .iter()
                        .filter(|(off, ra)| {
                            *off >= 0x10000
                                && matches!((off & 0xff) + 8, 0x9c | 0x78..=0x7b)
                                && mb.get(off).map(|&rb| rb != *ra).unwrap_or(false)
                        })
                        .map(|(off, _)| (off - 0x10000) >> 8)
                        .collect();
                    for i in 0..ia.len() {
                        let (off, ra) = ia[i];
                        let ro = io[i].1;
                        let Some(&rb) = mb.get(&off) else { continue };
                        let key = if off >= 0x10000 { format!("Guy+{:#x}", (off & 0xff) + 8) } else { format!("{:?}+{:#x}", xa.ty(), off) };
                        if ra != rb {
                            if ro == rb {
                                explained += 1;
                                *explained_by.entry(key).or_default() += 1;
                            } else {
                                unexplained += 1;
                                *unexplained_by.entry(key).or_default() += 1;
                            }
                        } else if ro != ra {
                            let is_clock = off >= 0x10000 && matches!((off & 0xff) + 8, 0x74..=0x77 | 0x7c..=0x7f);
                            if is_clock && set_anim_guys.contains(&((off - 0x10000) >> 8)) {
                                deferred_set_anim += 1;
                                continue;
                            }
                            introduced += 1;
                            introduced_rows.push(format!("f{}->f{} [{owner}][{slot}] {key}: retail {ra:#04x} ours {ro:#04x}", st[k].0, st[k + 1].0));
                        }
                    }
                }
            }
        }
        eprintln!(
            "inc_time over {pairs} pairs: explained={explained} unexplained={unexplained} introduced={introduced} \
             (deferred to step 14 set_anim: clock bytes x{deferred_set_anim})"
        );
        eprintln!("  explained by field: {explained_by:?}");
        let mut un: Vec<_> = unexplained_by.into_iter().collect();
        un.sort_by(|a, b| b.1.cmp(&a.1));
        eprintln!("  unexplained by field (top 12): {:?}", &un[..un.len().min(12)]);
        for r in introduced_rows.iter().take(40) {
            eprintln!("  INTRODUCED {r}");
        }
        assert_eq!(introduced, 0, "introduced bytes");
        assert!(explained_by.get("Guy+0x74").copied().unwrap_or(0) > 0, "no Guy.cur_time byte explained");
        assert!(explained_by.get("Guy+0x7c").copied().unwrap_or(0) > 0, "no Guy.last_time byte explained");
    }

    #[test]
    fn anim_class_table_matches_retail_shape() {
        // Death anims 7,8,9 (+ 26,28,30,32) are class 8; attack anims 11..=14 are 0xc.
        for a in [7, 8, 9, 26, 28, 30, 32] {
            assert_eq!(anim_class(a), Some(8));
        }
        for a in 11..=14 {
            assert_eq!(anim_class(a), Some(12));
        }
        assert_eq!(anim_class(-1), None);
        assert_eq!(anim_class(38), None);
    }

    /// Diagnostic: dump every Guy/Unit byte retail changed between
    /// consecutive captures (`-- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn dump_guy_diffs() {
        let Some(dir) = capture_dir() else { return };
        let st = steps(&dir);
        let mut hist: BTreeMap<String, usize> = BTreeMap::new();
        for k in 0..st.len().min(6) - 1 {
            if st[k + 1].0 - st[k].0 != 1 {
                continue;
            }
            let ra = container::load_svx(&dir.join(format!("{}.svx", st[k].1))).unwrap();
            let rb = container::load_svx(&dir.join(format!("{}.svx", st[k + 1].1))).unwrap();
            let a = load(&ra).unwrap().state;
            let b = load(&rb).unwrap().state;
            println!("== f{}", st[k].0);
            for owner in 0..a.objects.lists.len() {
                let la = &a.objects.lists[owner];
                let lb = &b.objects.lists[owner];
                for slot in 0..la.elems.len().min(lb.elems.len()) {
                    let (Some(oa), Some(ob)) = (&la.elems[slot], &lb.elems[slot]) else { continue };
                    let (Some(ua), Some(ub)) = (unit_of(oa), unit_of(ob)) else { continue };
                    let ptype = if ua.base.sub.body.len() == 19 { i32_at(&ua.base.sub.body, 15) } else { -1 };
                    let inside_up = unit_i16(ua, 0x82).unwrap_or(0);
                    let masks2 = unit_u32(ua, 0x6c).unwrap_or(0);
                    let squad = unit_type_i32(&a, ptype, 0x304);
                    let mut ud: Vec<String> = Vec::new();
                    for (name, va, vb, base) in [
                        ("S", &ua.base.sub.body, &ub.base.sub.body, 0x9usize),
                        ("O", &ua.base.mid, &ub.base.mid, 0x20),
                        ("U", &ua.body, &ub.body, 0x48),
                    ] {
                        for i in 0..va.len().min(vb.len()) {
                            if va[i] != vb[i] {
                                ud.push(format!("{name}+{:#x}:{:#04x}->{:#04x}", base + i, va[i], vb[i]));
                            }
                        }
                    }
                    let mut gd: Vec<String> = Vec::new();
                    for g in 0..ua.guys.elems.len().min(ub.guys.elems.len()) {
                        let (Some(ga), Some(gb)) = (&ua.guys.elems[g], &ub.guys.elems[g]) else { continue };
                        let (ra, rb) = (&ga.data, &gb.data);
                        let mut fields = Vec::new();
                        let mut i = 0;
                        while i < 155 {
                            if ra[i] != rb[i] {
                                let base = (i / 4) * 4;
                                let w = if base + 4 <= 155 { 4 } else { 155 - base };
                                fields.push(format!("+{:#x}:{:02x?}->{:02x?}", base + 8, &ra[base..base + w], &rb[base..base + w]));
                                *hist.entry(format!("Guy+{:#x}", base + 8)).or_default() += 1;
                                i = base + w;
                            } else {
                                i += 1;
                            }
                        }
                        if !fields.is_empty() {
                            gd.push(format!(
                                "    guy[{g}] anim={} gnum={} gflags={:#x} cur={} end={} last={}: {}",
                                g_i8(ra, G_CUR_ANIM),
                                g_i8(ra, G_GUY_NUM),
                                g_i16(ra, G_GUY_FLAGS),
                                g_u32(ra, G_CUR_TIME),
                                g_u32(ra, G_END_TIME),
                                g_u32(ra, G_LAST_TIME) as i32,
                                fields.join(" ")
                            ));
                        }
                    }
                    if !ud.is_empty() || !gd.is_empty() {
                        println!(
                            "  [{owner}][{slot}] ptype={ptype} squad={squad:?} flags={:#x} inside_up={inside_up} masks2={masks2:#x} nguys={} guy_mark={} {}",
                            ua.base.sub.flags,
                            ua.guys.elems.len(),
                            unit_i8(ua, 0xb5).unwrap_or(-1),
                            ud.join(" ")
                        );
                        for l in gd {
                            println!("{l}");
                        }
                    }
                }
            }
        }
        println!("== histogram");
        for (k, v) in hist {
            println!("  {k}: {v}");
        }
    }

    /// Diagnostic: dump every Build byte retail changed, with the
    /// Wall::inc_time-relevant fields alongside.
    #[test]
    #[ignore]
    fn dump_build_diffs() {
        let Some(dir) = capture_dir() else { return };
        let st = steps(&dir);
        let mut hist: BTreeMap<String, usize> = BTreeMap::new();
        for k in 0..st.len() - 1 {
            if st[k + 1].0 - st[k].0 != 1 {
                continue;
            }
            let ra = container::load_svx(&dir.join(format!("{}.svx", st[k].1))).unwrap();
            let rb = container::load_svx(&dir.join(format!("{}.svx", st[k + 1].1))).unwrap();
            let a = load(&ra).unwrap().state;
            let b = load(&rb).unwrap().state;
            println!("== f{}", st[k].0);
            for owner in 0..a.objects.lists.len() {
                let la = &a.objects.lists[owner];
                let lb = &b.objects.lists[owner];
                for slot in 0..la.elems.len().min(lb.elems.len()) {
                    let (Some(Obj::Build(ba)), Some(Obj::Build(bb))) = (&la.elems[slot], &lb.elems[slot]) else { continue };
                    let mut d = Vec::new();
                    for (name, va, vb, base) in [
                        ("S", &ba.base.sub.body, &bb.base.sub.body, 0x9usize),
                        ("O", &ba.base.mid, &bb.base.mid, 0x20),
                        ("W", &ba.wall_body, &bb.wall_body, 0x48),
                        ("B", &ba.body, &bb.body, 0x70),
                    ] {
                        for i in 0..va.len().min(vb.len()) {
                            if va[i] != vb[i] {
                                d.push(format!("{name}+{:#x}:{:#04x}->{:#04x}", base + i, va[i], vb[i]));
                                *hist.entry(format!("{name}+{:#x}", base + i)).or_default() += 1;
                            }
                        }
                    }
                    if d.is_empty() {
                        continue;
                    }
                    let ptype = if ba.base.sub.body.len() == 19 { i32_at(&ba.base.sub.body, 15) } else { -1 };
                    let (near_o, bm, rech, aox) = if ba.base.mid.len() == 34 && ba.wall_body.len() == 30 && ba.body.len() == 22 {
                        (
                            i16::from_le_bytes([ba.base.mid[0x14], ba.base.mid[0x15]]),
                            i16::from_le_bytes([ba.wall_body[0x18], ba.wall_body[0x19]]),
                            i16::from_le_bytes([ba.body[0xa], ba.body[0xb]]),
                            i16::from_le_bytes([ba.body[0xc], ba.body[0xd]]),
                        )
                    } else {
                        (0, 0, 0, 0)
                    };
                    println!(
                        "  [{owner}][{slot}] ptype={ptype} objmask={:#x} flags={:#x} near_o={near_o} build_masks={bm:#x} recharging={rech} attack_ox={aox} {}",
                        type_obj_masks(&a, ptype).unwrap_or(0),
                        ba.base.sub.flags,
                        d.join(" ")
                    );
                }
            }
        }
        println!("== histogram");
        for (k, v) in hist {
            println!("  {k}: {v}");
        }
    }
}
