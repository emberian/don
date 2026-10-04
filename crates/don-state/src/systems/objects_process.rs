//! Step 14: `Objects::process_all` 0x0065dce0 and the Unit/Animal plane it
//! dispatches into (`Unit::process` 0x00610bc0, `Animal::process`
//! 0x005d72c0, `Unit::work` 0x0060d180, `Unit::update_action` 0x0060a870,
//! `Guy::process` 0x005e0230, `Guy::move` 0x005d9240 stationary half,
//! `Guy::turn_towards` 0x005d9720 / `Guy::do_turn` 0x005d97a0 /
//! `Guy::set_angle` 0x005d9010 / `GuyData::turn_speed` 0x005de340, and the
//! owner-9 wildlife respawn tail at 0x0065dec9). The Build/Wall plane bodies
//! are `build_process::process_build` / `process_wall`; this file owns the
//! traversal (owner rotation, `SubObject.flags & 1` gate, `hold_frames`
//! countdown for inactive objects) for all three planes.
//!
//! Every read/write is addressed by the retail image offset (PDB `UnitData`
//! / `GuyData`) through [`UImg`] / [`GImg`], which map onto the serialized
//! sub-ranges of [`crate::sections::Unit`]:
//!
//! ```text
//!   +0x08         SubObject.flags                        base.sub.flags
//!   +0x09..+0x1c  SubObject  who,o,z,x,y,ptype           base.sub.body
//!   +0x20..+0x42  ObjectData myhits..launch_frames       base.mid
//!   +0x48..+0xb7  UnitData   collide_frame..play         body
//!   Guy rows      GuyData +0x08..+0xa3 (155 B)           guys.elems[i].data
//! ```
//!
//! Status is `Partial`: every branch below is either transcribed (writes
//! performed) or marked `TODO(va)` with its fields left untouched. The
//! largest untranscribed children are `Unit::do_job` 0x00617a10 (the order
//! dispatcher: `Unit::do_move` 0x005f7b30 → `Unit::move_step` 0x005faf30
//! for the moving citizens, `Unit::do_idle` 0x0060dcd0 / `Animal::do_idle`
//! 0x005d7460 for the idle ones) and the moving half of `Guy::move`
//! (`GuyData::get_speed` 0x005de410, `find_angle` 0x0092d130,
//! `Guy::set_new_location` 0x005d86f0). See the per-site notes.
//!
//! RNG: the only transcribed `Random::get` site is the wildlife respawn
//! tail. Untranscribed draw sites reachable from an idle frame:
//! `Animal::do_idle` 0x005d7460 (1 or 4 draws when a herd animal's idle
//! animation ends) and `Guy::set_anim` 0x005da300 (1 draw when an idle
//! guy re-rolls its idle variant: `+0x104 const_guys == 0`), both under
//! `Unit::do_job`.

use crate::sections::{Obj, Save, TypeRec, Unit};
use crate::tick::StepStatus;

pub const STATUS: StepStatus = StepStatus::Partial;

// ---------------------------------------------------------------------------
// Objects::process_all 0x0065dce0
// ---------------------------------------------------------------------------

pub fn run(save: &mut Save, effects: &mut Vec<String>) {
    let fr = frame(save);

    // First loop (0x0065dcf6..): ten owner slots in the rotation
    // `(Game::frame + i) % 10`, gated on `LeaderData[owner].flags & 1`;
    // slots `0..unit_mark[owner]`; inactive objects (`SubObject.flags & 1
    // == 0`) get `hold_frames -= 1` when nonzero, active ones reach the
    // virtual `process` (vtable +0x9c).
    for i in 0..10i32 {
        let owner = (fr.wrapping_add(i)).rem_euclid(10) as usize;
        if leader_flags(save, owner) & 1 == 0 {
            continue;
        }
        let mark = unit_mark(save, owner);
        let Some(list) = save.objects.lists.get(owner) else { continue };
        let n = (mark.max(0) as usize).min(list.elems.len());
        for slot in 0..n {
            let kind = match save.objects.lists[owner].elems.get(slot) {
                Some(Some(o)) => (o.obj_flags(), o.ty()),
                _ => continue,
            };
            if kind.0 & 1 == 0 {
                hold_frames_countdown(save, owner, slot, effects);
                continue;
            }
            match kind.1 {
                0 => process_unit(save, owner, slot, effects),
                3 => process_animal(save, owner, slot, effects),
                // A Build in the Unit plane would still dispatch its own
                // virtual process; not observed, kept for fidelity.
                1 => super::build_process::process_build(save, owner, slot, effects),
                _ => {}
            }
        }
    }

    // Second loop (0x0065dd5c..): owners 0..8 ascending (LeaderData slots
    // 0x00e3a390..0x00e71af0), Build slots 2000..build_mark then Wall slots
    // 3000..wall_mark. Inactive Build: `if hold_frames != 0 { if
    // get_wall_data()->build_masks & 0x4000: Build::process_ejection
    // 0x006201e0; hold_frames -= 1 }`; inactive Wall: hold_frames -= 1 when
    // nonzero.
    for owner in 0..8usize {
        if leader_flags(save, owner) & 1 == 0 {
            continue;
        }
        if owner >= save.objects.lists.len() {
            continue;
        }
        let bmark = build_mark(save, owner).max(2000) as usize;
        let wmark = wall_mark(save, owner).max(3000) as usize;
        for slot in 2000..bmark {
            match save.objects.lists[owner].elems.get(slot) {
                Some(Some(Obj::Build(b))) if b.base.sub.flags & 1 != 0 => {
                    super::build_process::process_build(save, owner, slot, effects)
                }
                Some(Some(Obj::Build(b))) => {
                    let hold = b.base.mid.get(0x12..0x14).map(|v| i16::from_le_bytes([v[0], v[1]])).unwrap_or(0);
                    if hold != 0 {
                        let bm = b.wall_body.get(0x18..0x1a).map(|v| u16::from_le_bytes([v[0], v[1]])).unwrap_or(0);
                        if bm & 0x4000 != 0 {
                            // TODO(va 0x006201e0) Build::process_ejection on a held
                            // site with garrison-eject pending; fields untouched.
                        }
                        hold_frames_countdown(save, owner, slot, effects);
                    }
                }
                _ => {}
            }
        }
        for slot in 3000..wmark {
            match save.objects.lists[owner].elems.get(slot) {
                Some(Some(Obj::Build(b))) if b.base.sub.flags & 1 != 0 => {
                    super::build_process::process_wall(save, owner, slot, effects)
                }
                Some(Some(Obj::Build(_))) => hold_frames_countdown(save, owner, slot, effects),
                _ => {}
            }
        }
    }

    wildlife_respawn(save, effects);
}

/// `*(short*)(obj + 0x32) -= 1` when nonzero — ObjectData::hold_frames,
/// serialized in `base.mid[0x12..0x14]` for every object kind.
fn hold_frames_countdown(save: &mut Save, owner: usize, slot: usize, effects: &mut Vec<String>) {
    let mid: &mut Vec<u8> = match save.objects.lists[owner].elems[slot].as_mut() {
        Some(Obj::Unit(u)) => &mut u.base.mid,
        Some(Obj::Animal(a)) => &mut a.unit.base.mid,
        Some(Obj::Build(b)) => &mut b.base.mid,
        None => return,
    };
    if mid.len() < 0x14 {
        return;
    }
    let h = i16::from_le_bytes([mid[0x12], mid[0x13]]);
    if h != 0 {
        mid[0x12..0x14].copy_from_slice(&(h - 1).to_le_bytes());
        effects.push(format!("Objects.lists[{owner}][{slot}].Object.hold_frames {h} -> {}", h - 1));
    }
}

/// Wildlife respawn tail 0x0065dec9..0x0065e0a0 (`frame & 0x1f == 0`):
/// `quota = min(10, xs*ys/100)`; `existing` = active owner-9 units whose
/// `is_animal()` holds and `ptype == 0x192`; per missing animal draw
/// `Random::get(0,0xffff) % xs` and `% ys` (each skipped when the dimension
/// is <= 1), test `WData[y*xs+x].flags & 0x20`, and on a hit call
/// `Objects::init_unit(9, 0x192, x*0x300+0x180, y*0x300+0x180, -1,-1,-1)`
/// 0x0065e0c0 then `Unit::add_air_patrol_order` 0x005e4350.
///
/// Owner 9's list and `unit_mark[9]` are not serialized (Objects::walk_data
/// 0x006541e0 walks nine of each), so `existing` is counted from
/// `lists.get(9)` — absent in every capture, i.e. 0.
fn wildlife_respawn(save: &mut Save, effects: &mut Vec<String>) {
    let fr = frame(save);
    if fr & 0x1f != 0 {
        return;
    }
    let (xs, ys) = (save.world.xs, save.world.ys);
    let mut quota = xs.wrapping_mul(ys) / 100;
    if quota > 10 {
        quota = 10;
    }
    let existing = save
        .objects
        .lists
        .get(9)
        .map(|l| {
            let mark = unit_mark(save, 9).max(0) as usize;
            l.elems
                .iter()
                .take(mark)
                .filter(|e| match e {
                    Some(Obj::Animal(a)) => {
                        a.unit.base.sub.flags & 1 != 0
                            && a.unit.base.sub.body.len() == 19
                            && i32::from_le_bytes(a.unit.base.sub.body[15..19].try_into().unwrap()) == 0x192
                    }
                    _ => false,
                })
                .count() as i32
        })
        .unwrap_or(0);
    let mut left = quota - existing;
    let mut draws = 0;
    while left > 0 {
        let x = if xs == 1 || xs - 1 < 0 {
            0
        } else {
            draws += 1;
            game_random(save, 0, 0xffff) % xs
        };
        let y = if ys == 1 || ys - 1 < 0 {
            0
        } else {
            draws += 1;
            game_random(save, 0, 0xffff) % ys
        };
        let idx = (xs.wrapping_mul(y).wrapping_add(x)) as usize;
        let hit = save.world.wdata.get(idx * 21..idx * 21 + 21).map(|t| t[0] & 0x20 != 0).unwrap_or(false);
        if hit {
            // TODO(va 0x0065e0c0, 0x005e4350) Objects::init_unit(9, 0x192,
            // x*0x300+0x180, y*0x300+0x180, -1, -1, -1) >= 0 ->
            // Unit::add_air_patrol_order. Allocates into the unserialized
            // owner-9 list; nothing in the walked image to write.
            effects.push(format!("wildlife: viable cell ({x},{y}) — init_unit 0x0065e0c0 not transcribed"));
        }
        left -= 1;
    }
    if quota - existing > 0 {
        effects.push(format!("wildlife respawn: quota {quota} existing {existing} -> {draws} Random::get draws"));
    }
    // 0x0065e04d.. (frame & 0x3f == 0): herd scan over Herds (DAT_00c0a250,
    // count DAT_00c0a244) picking herd `(frame/64) % max(count,5)`; when that
    // herd's +0x1a & 1 -> FUN_00741760. No walked field; nothing to do here.
}

// ---------------------------------------------------------------------------
// Image accessors
// ---------------------------------------------------------------------------

/// Byte-image view of a serialized `Unit`, addressed by retail offset.
struct UImg<'a>(&'a mut Unit);

impl UImg<'_> {
    fn complete(&self) -> bool {
        self.0.base.sub.body.len() == 19 && self.0.base.mid.len() == 34 && self.0.body.len() == 111
    }
    fn slot(&mut self, off: usize) -> (&mut Vec<u8>, usize) {
        let u = &mut *self.0;
        match off {
            0x09..=0x1b => (&mut u.base.sub.body, off - 0x09),
            0x20..=0x41 => (&mut u.base.mid, off - 0x20),
            0x48..=0xb6 => (&mut u.body, off - 0x48),
            _ => panic!("Unit image offset {off:#x} is not serialized"),
        }
    }
    fn u8(&mut self, off: usize) -> u8 {
        if off == 0x08 {
            return self.0.base.sub.flags;
        }
        let (v, i) = self.slot(off);
        v[i]
    }
    fn i8(&mut self, off: usize) -> i8 {
        self.u8(off) as i8
    }
    fn i16(&mut self, off: usize) -> i16 {
        let (v, i) = self.slot(off);
        i16::from_le_bytes([v[i], v[i + 1]])
    }
    fn i32(&mut self, off: usize) -> i32 {
        let (v, i) = self.slot(off);
        i32::from_le_bytes(v[i..i + 4].try_into().unwrap())
    }
    fn u32(&mut self, off: usize) -> u32 {
        self.i32(off) as u32
    }
    fn set_u8(&mut self, off: usize, x: u8) {
        if off == 0x08 {
            self.0.base.sub.flags = x;
            return;
        }
        let (v, i) = self.slot(off);
        v[i] = x;
    }
    fn set_i16(&mut self, off: usize, x: i16) {
        let (v, i) = self.slot(off);
        v[i..i + 2].copy_from_slice(&x.to_le_bytes());
    }
    fn set_i32(&mut self, off: usize, x: i32) {
        let (v, i) = self.slot(off);
        v[i..i + 4].copy_from_slice(&x.to_le_bytes());
    }
    fn set_u32(&mut self, off: usize, x: u32) {
        self.set_i32(off, x as i32)
    }
    fn who(&mut self) -> usize {
        self.u8(0x09) as usize
    }
    fn o(&mut self) -> i32 {
        self.i16(0x0a) as i32
    }
    fn ptype(&mut self) -> i32 {
        self.i32(0x18)
    }
    /// Coords are stored XOR 0x63637.
    fn x(&mut self) -> i32 {
        self.i32(0x10) ^ 0x63637
    }
    fn y(&mut self) -> i32 {
        self.i32(0x14) ^ 0x63637
    }
    fn guys_len(&self) -> usize {
        self.0.guys.elems.len()
    }
    /// First order (`orderlist.head_node->next`, see `OrderList::walk_data`
    /// 0x00730270: the serialized order is head.next, …, head).
    fn order(&self, i: usize) -> Option<(i32, u8, &[u8])> {
        self.0.orders.orders.get(i).map(|o| (o.ty, o.payload.first().copied().unwrap_or(0), o.payload.as_slice()))
    }
}

/// Byte-image view of one serialized `GuyData` row (+0x08..+0xa3).
struct GImg<'a>(&'a mut Vec<u8>);

impl GImg<'_> {
    fn i32(&self, off: usize) -> i32 {
        i32::from_le_bytes(self.0[off - 8..off - 4].try_into().unwrap())
    }
    fn u32(&self, off: usize) -> u32 {
        self.i32(off) as u32
    }
    fn i16(&self, off: usize) -> i16 {
        i16::from_le_bytes([self.0[off - 8], self.0[off - 7]])
    }
    fn u16(&self, off: usize) -> u16 {
        self.i16(off) as u16
    }
    fn u8(&self, off: usize) -> u8 {
        self.0[off - 8]
    }
    fn set_i32(&mut self, off: usize, x: i32) {
        self.0[off - 8..off - 4].copy_from_slice(&x.to_le_bytes());
    }
    fn set_u32(&mut self, off: usize, x: u32) {
        self.set_i32(off, x as i32)
    }
    fn set_u16(&mut self, off: usize, x: u16) {
        self.0[off - 8..off - 6].copy_from_slice(&x.to_le_bytes());
    }
    fn set_u8(&mut self, off: usize, x: u8) {
        self.0[off - 8] = x;
    }
}

fn guy_row(u: &mut Unit, i: usize) -> Option<&mut Vec<u8>> {
    u.guys.elems.get_mut(i).and_then(|g| g.as_mut()).map(|g| &mut g.data).filter(|d| d.len() == 155)
}

// ---------------------------------------------------------------------------
// Global-state readers
// ---------------------------------------------------------------------------

/// `Game+0x550` (`Game::frame`).
fn frame(save: &Save) -> i32 {
    i32::from_le_bytes(save.game.scalars[0..4].try_into().unwrap())
}

/// `Constants` i32 at image offset `off` (`[0x00c061f0]` / the const alias
/// `[0x00c061e4]`, walked as `Save.constants`).
fn constant(save: &Save, off: usize) -> i32 {
    let rules = &save.rules_tail.rules.constants;
    if rules.len() >= off + 4 {
        return i32::from_le_bytes(rules[off..off + 4].try_into().unwrap());
    }
    i32::from_le_bytes(save.constants[off..off + 4].try_into().unwrap())
}

fn leader_flags(save: &Save, who: usize) -> i32 {
    save.leaders.slots.get(who).map(|l| l.flags).unwrap_or(0)
}

/// `Objects` scalar block: `[0x1f4..0x1fc) [0x154..0x15c) unit_mark[9]
/// build_mark[9] wall_mark[9] obj_ctr[9]` (Objects::walk_data 0x006541e0).
fn unit_mark(save: &Save, owner: usize) -> i32 {
    if owner >= 9 {
        return 0;
    }
    let o = 16 + owner * 4;
    i32::from_le_bytes(save.objects.scalars[o..o + 4].try_into().unwrap())
}

fn build_mark(save: &Save, owner: usize) -> i32 {
    let o = 16 + 36 + owner * 4;
    i32::from_le_bytes(save.objects.scalars[o..o + 4].try_into().unwrap())
}

fn wall_mark(save: &Save, owner: usize) -> i32 {
    let o = 16 + 72 + owner * 4;
    i32::from_le_bytes(save.objects.scalars[o..o + 4].try_into().unwrap())
}

/// `Random::get(min, max)` 0x00a39d70 on `GameAccess::game_random`
/// (`Save.post_world` +40): `seed = seed*0x19660d + 0x3c6ef35f;
/// ((seed & 0xffff) * (max-min) >> 16) + min`. Same body as
/// `game_daemon::game_random`, which is private to that module.
fn game_random(save: &mut Save, min: i32, max: i32) -> i32 {
    const GAME_RANDOM: usize = 0x28;
    let (lo, hi) = if max < min { (max, min) } else { (min, max) };
    if lo == hi {
        return min;
    }
    let seed = u32::from_le_bytes(save.post_world[GAME_RANDOM..GAME_RANDOM + 4].try_into().unwrap());
    let seed = crate::tick::rng_step(seed);
    save.post_world[GAME_RANDOM..GAME_RANDOM + 4].copy_from_slice(&seed.to_le_bytes());
    (((seed & 0xffff) as i32).wrapping_mul(hi - lo) as u32 >> 16) as i32 + lo
}

/// Type record image reader (`Rules.types[idx]`): `head` = image[4..94),
/// `obj_mid` = image[0x1e4..0x27c), `ext` = the unit tail
/// `[0x2b4..0x2cc) ++ [0x2d4..0x2dc) ++ [0x2dc..0x2e0) ++ [0x2e0..0x5d4)`
/// (see `TypeRec`), so image offsets >= 0x2d4 are shifted by the 8-byte
/// gap at 0x2cc.
struct TypeImg<'a>(&'a TypeRec);

impl TypeImg<'_> {
    fn i32(&self, off: usize) -> Option<i32> {
        let (v, i) = match off {
            0x04..=0x5d => (&self.0.head, off - 4),
            0x1e4..=0x27b => (&self.0.obj_mid, off - 0x1e4),
            0x2b4..=0x2cb => (&self.0.ext, off - 0x2b4),
            0x2d4.. => (&self.0.ext, off - 0x2b4 - 8),
            _ => return None,
        };
        v.get(i..i + 4).map(|b| i32::from_le_bytes(b.try_into().unwrap()))
    }
}

fn type_rec(save: &Save, idx: i32) -> Option<TypeImg<'_>> {
    usize::try_from(idx).ok().and_then(|i| save.rules_tail.rules.types.get(i)).map(TypeImg)
}

/// Per-type scalars the bodies below read.
#[derive(Clone, Copy, Default)]
struct TypeInfo {
    /// `+0x218` movement plane: 0 land, 1 sea, 2 air.
    kind: i32,
    /// `+0x2b4` unit flags.
    flags: u32,
    /// `+0x2b8` unit flags 2 (0x02 hero/special class, 0x04 formation).
    flags2: u32,
    /// `+0x2c4` turn rate (`>> 8` then × Constants+8).
    turn: u32,
    /// `+0x304` number of primary guys; rows at and beyond are attachments.
    num_guys: i32,
    /// `+0x1fc`.
    f1fc: i32,
}

fn type_info(save: &Save, ptype: i32) -> TypeInfo {
    let Some(t) = type_rec(save, ptype) else { return TypeInfo::default() };
    TypeInfo {
        kind: t.i32(0x218).unwrap_or(0),
        flags: t.i32(0x2b4).unwrap_or(0) as u32,
        flags2: t.i32(0x2b8).unwrap_or(0) as u32,
        turn: t.i32(0x2c4).unwrap_or(0) as u32,
        num_guys: t.i32(0x304).unwrap_or(i32::MAX),
        f1fc: t.i32(0x1fc).unwrap_or(0),
    }
}

/// `uid` (+0x30) of `Objects.lists[who][o]`, any kind.
fn object_uid(save: &Save, who: i32, o: i32) -> Option<i16> {
    let l = save.objects.lists.get(usize::try_from(who).ok()?)?;
    let mid = match l.elems.get(usize::try_from(o).ok()?)? {
        Some(Obj::Unit(u)) => &u.base.mid,
        Some(Obj::Animal(a)) => &a.unit.base.mid,
        Some(Obj::Build(b)) => &b.base.mid,
        None => return None,
    };
    mid.get(0x10..0x12).map(|v| i16::from_le_bytes([v[0], v[1]]))
}

/// Take the object's Unit out of its slot for the duration of a body so the
/// body can read `&Save` while mutating the image; restores it afterwards.
fn with_unit(save: &mut Save, owner: usize, slot: usize, f: impl FnOnce(&Save, &mut Unit)) {
    let taken = save.objects.lists[owner].elems[slot].take();
    match taken {
        Some(Obj::Unit(mut u)) => {
            f(save, &mut u);
            save.objects.lists[owner].elems[slot] = Some(Obj::Unit(u));
        }
        Some(Obj::Animal(mut a)) => {
            f(save, &mut a.unit);
            save.objects.lists[owner].elems[slot] = Some(Obj::Animal(a));
        }
        other => save.objects.lists[owner].elems[slot] = other,
    }
}

// ---------------------------------------------------------------------------
// Unit::process 0x00610bc0
// ---------------------------------------------------------------------------

pub fn process_unit(save: &mut Save, owner: usize, slot: usize, effects: &mut Vec<String>) {
    with_unit(save, owner, slot, |save, u| unit_process_body(save, u, owner, slot, effects));
}

fn unit_process_body(save: &Save, u: &mut Unit, owner: usize, slot: usize, effects: &mut Vec<String>) {
    let mut img = UImg(u);
    if !img.complete() {
        return;
    }
    let fr = frame(save);
    let who = img.who();
    let o = img.o();
    let ti = type_info(save, img.ptype());
    let tag = format!("Objects.lists[{owner}][{slot}]");

    // if (recharging) --; if (full) --; if (waiting) --; if (healing) --;
    for (off, name) in [(0xaeusize, "Unit.recharging"), (0xac, "Unit.full"), (0xad, "Unit.waiting")] {
        let v = img.u8(off);
        if v != 0 {
            img.set_u8(off, v.wrapping_sub(1));
            effects.push(format!("{tag}.{name} {v} -> {}", v.wrapping_sub(1)));
        }
    }
    let healing = img.i16(0x38);
    if healing != 0 {
        img.set_i16(0x38, healing - 1);
        effects.push(format!("{tag}.Object.healing {healing} -> {}", healing - 1));
    }

    let um = img.u32(0x68);
    if um & 1 == 0 {
        if ti.flags2 & 2 == 0 {
            if ti.kind == 2 {
                // TODO(va 0x00611050) air unit fuel: inside_up < 0 -> mana_burn
                // (+0x96) += 1 while UnitData::mana 0x00609a50 exceeds it;
                // else mana_burn -= Constants+0xa0 and the
                // LeaderData::get_heal_level 0x006e12e0 / Constants+0xc00
                // periodic Unit::repair_damage(1,1,1); writes healing (+0x38).
                // Fields untouched (no air units in the captures).
            }
        } else {
            // TODO(va 0x00610d72) hero/special class (flags2 & 2):
            // City::... FUN_00739ad0 per (is_hero || is_special); every 32
            // frames with unit_masks < 0 || unit_masks2 & 8 -> is(0x4b)
            // toggles unit_masks bit 31 / clears unit_masks2 bit 3 and emits
            // a graphic event; supply/gov-hero mana_burn (+0x96) decay.
            // Fields untouched.
        }
    } else {
        // TODO(va 0x00610c50) unit_masks & 1 (packed/embarked state):
        // mana_burn (+0x96) += 1, Unit::close (vtable +0x150) when past
        // Constants+0xcd0 * (LeaderData 0x006e0830 + 2) / 2; else when
        // is_on_map and the fog grid says unseen: suffer_attrition
        // 0x005e1a10 every 7th frame. Fields untouched.
    }

    // LAB_00611158: num_queued != 0 && LeaderData[who].flags & 2
    if img.i16(0xa0) != 0 && leader_flags(save, who) & 2 != 0 {
        // TODO(va 0x00611158) unit-side production: queue_time (+0x64) +=
        // Constants+0x228 against ObjectData::construct_time 0x006508c0,
        // LeaderData pop caps, Objects::init_unit 0x00461310 on completion.
        // Fields untouched.
    }

    // LAB_006114c6
    // TODO(va 0x005e0670) Unit::process_healing — writes damage/damage_frac
    // and healing; fields untouched.
    if img.i16(0x82) < 0 {
        // Not inside anything.
        let um2 = img.u32(0x6c);
        if um2 & 0x10 != 0 {
            img.set_u32(0x6c, um2 & !0x10);
            effects.push(format!("{tag}.Unit.unit_masks2 &= ~0x10"));
        }
        if (fr + o) % 16 == 0 {
            let t = img.i8(0x3d) as i32;
            let nt = (t + ((t >> 31) & 3)) >> 2;
            if nt != t {
                img.set_u8(0x3d, nt as i8 as u8);
                effects.push(format!("{tag}.Object.targeted {t} -> {nt}"));
            }
            // TODO(va 0x005e1c20) Unit::process_cloak; fields untouched.
            if (fr + o) % 32 == 0 {
                let um2 = img.u32(0x6c);
                let mut n = um2 & !0x40000;
                // TODO(va 0x005e11a0) Unit::process_attrition; fields untouched.
                if n & 2 != 0 {
                    n &= !2;
                } else {
                    n &= !1;
                }
                if n != um2 {
                    img.set_u32(0x6c, n);
                    effects.push(format!("{tag}.Unit.unit_masks2 {um2:#x} -> {n:#x} (32-frame attrition pass)"));
                }
            }
            // TODO(va 0x00611648) has_objmask(0x80000000) -> is_jammed -> Items
            // record +0x6c |= 8 (another object). Merchant local-player
            // advisory writes announce_frame (+0x14c, not serialized).
        }
        let attr = img.i16(0x9e) as i32;
        if attr != 0 && (fr + o) % attr == 0 {
            // TODO(va 0x005e0560, 0x005e1a10) Unit::process_supply == 0 ->
            // Unit::suffer_attrition(1) (may `return` before work());
            // else unit_masks2 |= 0x40000. Fields untouched; the
            // supply-covered case continues below as retail does.
        }
        // (vtable +0x188) Unit::work — the guy loops below run whether or
        // not work() returned early; an early return (Unit::repath /
        // Unit::kill_current_order, both untranscribed) may have re-aimed
        // the guys, so `des_known` is forced off in that case.
        let completed = unit_work(save, &mut img, &tag, effects);
        guys_process(save, img.0, ti, fr, !completed, &tag, effects);
    } else {
        // Inside a building/transport.
        let um_old = img.u32(0x68);
        let um2 = img.u32(0x6c);
        let n2 = um2 & 0xffff7f9f;
        if n2 != um2 {
            img.set_u32(0x6c, n2);
            effects.push(format!("{tag}.Unit.unit_masks2 {um2:#x} -> {n2:#x} (inside)"));
        }
        let n = if um_old & 0x2000 == 0 { um_old & 0xfffee7ff } else { um_old & 0xfffeefff };
        if n != um_old {
            img.set_u32(0x68, n);
            effects.push(format!("{tag}.Unit.unit_masks {um_old:#x} -> {n:#x} (inside)"));
        }
        if (who as i32 + fr) % 32 == 0 && n & 0x40000 != 0 {
            // TODO(va 0x00617c10) Unit::come_out when the host city's flags
            // lack 0x42 (FUN_00651a80 host lookup). Fields untouched.
        }
        let len = img.guys_len();
        for gi in 0..len {
            if let Some(row) = guy_row(img.0, gi) {
                let mut g = GImg(row);
                let z = g.i32(0x14);
                if g.i32(0x70) != z {
                    g.set_i32(0x70, z);
                    effects.push(format!("{tag}.guys[{gi}].last_z = z"));
                }
            }
        }
    }
}

/// The two guy loops shared by `Unit::process` and `Animal::process`:
/// rows `0..guy_mark` then `ptype->num_guys..guys.length`.
fn guys_process(save: &Save, u: &mut Unit, ti: TypeInfo, fr: i32, work_bailed: bool, tag: &str, effects: &mut Vec<String>) {
    let guy_mark = u.body[0xb5 - 0x48] as i8 as i32;
    let len = u.guys.elems.len() as i32;
    let mut idx: Vec<usize> = (0..guy_mark.max(0)).map(|i| i as usize).collect();
    idx.extend((ti.num_guys.max(0)..len).map(|i| i as usize));
    let des_known = !work_bailed && des_known(u, ti);
    for gi in idx {
        guy_process(save, u, gi, ti, fr, des_known, tag, effects);
    }
}

/// Whether the untranscribed `Unit::do_job` 0x00617a10 (which runs inside
/// `work()` *before* the guy loops) could have rewritten the guys'
/// `des_x/des_y/des_angle` this frame. `Guy::move` branches on
/// `des == pos`, so its branch bodies are only executed when the
/// destination is known to be the serialized one:
///
/// - a MoveOrder family head (types 1..4, 0x12, 0x13, 0x15) or a pending
///   path means `Unit::do_move` → `Unit::move_step` 0x005faf30 re-aims the
///   guys every frame; a TargetOrder head (6/7/14) reaches `Unit::set_angle`
///   0x00605400 from its (untranscribed) do_* body when it docks;
/// - an idle Animal (`Animal::do_idle` 0x005d7460) issues
///   `Unit::add_move_order` 0x00616ed0 — executed the same frame — exactly
///   when `guys[0].cur_time == end_time - 1` (its herd/kind/RNG gates can
///   only shrink that set);
/// - an idle Unit (`Unit::do_idle` 0x0060dcd0 → `Unit::think` 0x005f6e40)
///   can issue orders; the captures show none, and `think` is not
///   transcribed, so idle non-animals are treated the same way as animals.
fn des_known(u: &Unit, ti: TypeInfo) -> bool {
    let ty = u.orders.orders.first().map(|o| o.ty).unwrap_or(0);
    // Any order body is untranscribed (`Unit::do_gather` for 7 calls
    // `Unit::set_angle` 0x00605400 on docking, which re-aims des_angle), so
    // only order-less units qualify.
    if ty != 0 || !u.path.data.is_empty() {
        return false;
    }
    if ty == 0 {
        if let Some(Some(g0)) = u.guys.elems.first() {
            if g0.data.len() == 155 {
                let g = GImg(&mut g0.data.clone());
                if g.i32(0x74) == g.i32(0x78).wrapping_sub(1) {
                    return false;
                }
            }
        }
    }
    let _ = ti;
    true
}

// ---------------------------------------------------------------------------
// Unit::work 0x0060d180
// ---------------------------------------------------------------------------

/// Returns `false` when retail `return`s before the trailing
/// `unit_masks &= ~0x10` (the remainder of *work* was skipped; the guy
/// loops in `Unit::process` still run afterwards).
fn unit_work(save: &Save, img: &mut UImg, tag: &str, effects: &mut Vec<String>) -> bool {
    let fr = frame(save);
    let who = img.who();
    let o = img.o();
    let ti = type_info(save, img.ptype());
    let first = img.order(0).map(|(ty, fl, _)| (ty, fl));
    let ty = first.map(|f| f.0).unwrap_or(0);
    let oflags = first.map(|f| f.1).unwrap_or(0);

    // if ((frame + o) % 32 == 0) { if (flags >= 0) visible = 0; unit_masks &= ~4; }
    if (fr + o) % 32 == 0 {
        if (img.u8(0x08) as i8) >= 0 && img.u8(0x40) != 0 {
            img.set_u8(0x40, 0);
            effects.push(format!("{tag}.Object.visible -> 0"));
        }
        let um = img.u32(0x68);
        if um & 4 != 0 {
            img.set_u32(0x68, um & !4);
            effects.push(format!("{tag}.Unit.unit_masks &= ~4"));
        }
    }
    if ty != 10 {
        let fl = img.u8(0x08);
        if fl & 0x80 != 0 {
            img.set_u8(0x08, fl & 0x7f);
            effects.push(format!("{tag}.SubObject.flags &= 0x7f"));
        }
        let um = img.u32(0x68);
        if um & 0x100 != 0 && first.is_some() && oflags & 4 != 0 && ty != 3 {
            img.set_u32(0x68, um & !0x100);
            effects.push(format!("{tag}.Unit.unit_masks &= ~0x100"));
        }
        if img.u8(0xae) != 0 && ti.f1fc == 0 && ti.flags & 0x400 == 0 {
            if first.is_none() || oflags & 4 == 0 {
                return false;
            }
        }
    }
    // is_caravan (ptype vtable +0x130) && Leaders caravan slot flags & 6 && order_type != 0xe
    // TODO(va 0x0060d2b0) caravan re-route (Unit::do_caravan FUN_005e3bd0,
    // City::... FUN_0073db10). No caravans in the captures; fields untouched.

    let move_family = matches!(ty, 1 | 2 | 3 | 4 | 0x12 | 0x13 | 0x15);
    if move_family {
        // LAB_0060d36a / LAB_0060d440
        if ty == 1 && img.u32(0x68) & 0x4000000 != 0 {
            // TODO(va 0x0060d36a) unit_masks & 0x4000000 re-issue:
            // Unit::add_move_order_ex FUN_005e55c0 from the current
            // MoveOrder then Unit::update_order. Fields untouched.
        }
        if ti.flags2 & 4 == 0 || img.u32(0x68) & 0x80000 != 0 {
            if (fr + o) % 16 == 0 {
                // TODO(va 0x0060d44f) every 16 frames: update_action target
                // re-validation (FUN_005e29b0 / FUN_005e22d0 re-path when the
                // action target is targeted and not type 9/0xc). Fields
                // untouched; falls through to LAB_0060d733 as retail does
                // when nothing re-paths.
            }
        } else {
            // Formation unit (flags2 & 4, no 0x80000): LAB_0060d94d path.
            // TODO(va 0x0060d5f0) group-move stand-off: when the MoveOrder's
            // flags & 4 and orderlist.length == 1 and vector_dist < type
            // +0x240, rewrites the MoveOrder x/y/dest/orig/last to the unit
            // position and pops the path (FUN_0046d870/0046d820); otherwise
            // FUN_005e4a60(-1,-1,-1,-1) then update_order, or, when already
            // on the destination tile, FUN_005e2cb0 and `return`.
            return false;
        }
    }
    // LAB_0060d733
    if !move_family && ty != 0 && img.u8(0x08) & 8 != 0 {
        // if (ptype->index in {0x3d, 0x3e, 400} || ptype->is(...)) LeaderData[who].flags |= 0x2000000;
        // TODO(va 0x0060d74b) the LeaderData flag write (Leader image, not ours).
        let fl = img.u8(0x08);
        img.set_u8(0x08, fl & !8);
        effects.push(format!("{tag}.SubObject.flags &= ~8"));
    }
    // if (const_guys != 0 && order && !order->is_move()) FUN_005e3920();
    // const_guys (+0x104) is runtime-only; TODO(va 0x005e3920).

    // piVar5 = update_action()
    let act = update_action(save, img, tag, effects);
    match act {
        None => {
            if ty != 0xe && img.i16(0x98) != 0 {
                img.set_i16(0x98, 0);
                effects.push(format!("{tag}.Unit.collide -> 0"));
            }
        }
        Some(ai) => {
            let (aty, _afl, payload) = img.order(ai).map(|(t, f, p)| (t, f, p.to_vec())).unwrap();
            // Every action order reaching here is TargetOrder-derived
            // (is_targeted = 1 for types 6/7/14).
            if ty != 0xe && aty != 0xe && who < 8 && img.i16(0x98) != 0 {
                img.set_i16(0x98, 0);
                effects.push(format!("{tag}.Unit.collide -> 0"));
            }
            // tgt = act->update_target_order(): +8 o, +0xc who, +0x10 uid
            let (to, tw, tuid) = if payload.len() >= 11 {
                (
                    i32::from_le_bytes(payload[1..5].try_into().unwrap()),
                    i32::from_le_bytes(payload[5..9].try_into().unwrap()),
                    i16::from_le_bytes([payload[9], payload[10]]),
                )
            } else {
                (-1, -1, 0)
            };
            if to >= 0 && tw >= 0 {
                if aty == 10 {
                    // TODO(va 0x005fcfb0) type-10 (board) target refresh.
                }
                let live_uid = object_uid(save, tw, to);
                if live_uid != Some(tuid) {
                    if aty == 0x10 {
                        // TODO(va 0x0060d8a0) type-0x10 target reset (+8/+0xc = -1, +0x3c = 1).
                    } else {
                        // act->vtable+0x18 is 0 for Gather/Target/Cast orders:
                        // LAB_0060d948 FUN_005e29b0(); FUN_005e2cb0(); return;
                        // TODO(va 0x005e29b0, 0x005e2cb0) stale-target cleanup.
                        effects.push(format!("{tag}: action target ({tw},{to}) uid {tuid} stale -> work() returned early"));
                        return false;
                    }
                }
                if aty == 0xf {
                    // TODO(va 0x0060d8d8) type-0xf secondary target check.
                }
            }
        }
    }
    // if (o >= 0 && inside_up < 0 && FUN_00646c40()) FUN_0064f3b0();
    // TODO(va 0x00646c40, 0x0064f3b0) ObjectData launch gate / Object::do_launch.

    let safe = img.u8(0xb2);
    if safe != 0 {
        img.set_u8(0xb2, safe - 1);
        effects.push(format!("{tag}.Unit.safe {safe} -> {}", safe - 1));
    }
    if ty != 0 {
        if img.u8(0xb0) != 0 {
            img.set_u8(0xb0, 0);
            effects.push(format!("{tag}.Unit.idle -> 0"));
        }
        let um = img.u32(0x68);
        if ti.flags & 0x4000 == 0 && ti.flags & 0x40000 != 0 && um & 0x800 == 0 && img.u32(0x6c) & 0x8000 == 0 && um & 0x1000 == 0 {
            img.set_u32(0x68, um | 0x1000);
            effects.push(format!("{tag}.Unit.unit_masks |= 0x1000"));
        }
    }
    if fr - 4 < img.i32(0x48) {
        // TODO(va 0x005fa8b0) recent collision (collide_frame within 4
        // frames) for non-move orders on non-supply/hero types. Fields
        // untouched.
    }
    // do_job(ty, order)
    // TODO(va 0x00617a10) Unit::do_job — the order dispatcher. ty 0 ->
    // do_idle (Unit::do_idle 0x0060dcd0 / Animal::do_idle 0x005d7460),
    // 1..4 -> Unit::do_move 0x005f7b30 -> Unit::move_step 0x005faf30
    // (writes x/y/z, angle +0x50, dest_angle +0x58, collide_*, the Guy
    // des_x/des_y/des_angle, path stack), 7 -> gather. Fields untouched.
    effects.push(format!("{tag}: do_job(ty={ty}) 0x00617a10 not transcribed"));

    if ti.flags & 0x20 != 0 && img.u8(0x08) & 1 != 0 && img.i16(0x82) < 0 {
        // TODO(va 0x0060daf0) ObjectsData::find_unit 0x0065ca80 neighbour
        // nudge for flags & 0x20 types. Fields untouched.
    }
    let um = img.u32(0x68);
    if um & 0x10 != 0 {
        img.set_u32(0x68, um & !0x10);
        effects.push(format!("{tag}.Unit.unit_masks &= ~0x10"));
    }
    true
}

// ---------------------------------------------------------------------------
// Unit::update_action 0x0060a870
// ---------------------------------------------------------------------------

/// Writes `orders_x/orders_y` (+0x70/+0x74) and `dest_angle` (+0x58) from
/// the unit position, then from each leading MoveOrder (types 1..4, whose
/// `is_move` vtable +0x14 returns 1) that is not `flags & 4`, and returns
/// the index of the first non-move order (the "action"), or `None`.
///
/// The orderlist is a ring whose `head_node` is the *last* serialized node
/// (`OrderList::walk_data` 0x00730270 starts at `head->next`), so the
/// `current_node == head_node` tests below are `i == len - 1`.
fn update_action(save: &Save, img: &mut UImg, tag: &str, effects: &mut Vec<String>) -> Option<usize> {
    let _ = save;
    let (x, y, angle) = (img.x(), img.y(), img.i32(0x50));
    let mut set = |img: &mut UImg, ox: i32, oy: i32, da: i32, why: &str| {
        if img.i32(0x70) != ox || img.i32(0x74) != oy {
            img.set_i32(0x70, ox);
            img.set_i32(0x74, oy);
            effects.push(format!("{tag}.Unit.orders_x/y -> ({ox},{oy}) {why}"));
        }
        if img.i32(0x58) != da {
            img.set_i32(0x58, da);
            effects.push(format!("{tag}.Unit.dest_angle -> {da} {why}"));
        }
    };
    set(img, x, y, angle, "(update_action self)");
    let n = img.0.orders.orders.len();
    if n == 0 {
        return None;
    }
    let is_move = |ty: i32| matches!(ty, 1..=4);
    let order_xy = |p: &[u8]| -> (i32, i32, i32) {
        if p.len() >= 13 {
            (
                i32::from_le_bytes(p[1..5].try_into().unwrap()),
                i32::from_le_bytes(p[5..9].try_into().unwrap()),
                i32::from_le_bytes(p[9..13].try_into().unwrap()),
            )
        } else {
            (x, y, angle)
        }
    };
    let mut i = 0usize;
    let (ty0, _, _) = img.order(0).unwrap();
    if !(n == 1 && !is_move(ty0)) {
        loop {
            let (ty, fl, p) = img.order(i).unwrap();
            let cont = ((is_move(ty) && fl & 4 == 0) || ty == 0x12) && i != n - 1;
            if !cont {
                break;
            }
            if is_move(ty) {
                let (ox, oy, da) = order_xy(p);
                set(img, ox, oy, da, "(leading MoveOrder)");
            }
            i += 1;
        }
        let (ty, fl, p) = img.order(i).unwrap();
        if !(!is_move(ty) || (i != n - 1 && fl & 4 == 0)) {
            let (ox, oy, da) = order_xy(p);
            set(img, ox, oy, da, "(final MoveOrder)");
        }
    }
    // LAB_0060aa16
    let (ty, fl, _) = img.order(i).unwrap();
    if (is_move(ty) && fl & 4 == 0) || ty == 0x12 {
        return None;
    }
    Some(i)
}

// ---------------------------------------------------------------------------
// Animal::process 0x005d72c0 / Animal::work 0x005d7330
// ---------------------------------------------------------------------------

pub fn process_animal(save: &mut Save, owner: usize, slot: usize, effects: &mut Vec<String>) {
    with_unit(save, owner, slot, |save, u| {
        let mut img = UImg(u);
        if !img.complete() {
            return;
        }
        let fr = frame(save);
        let ti = type_info(save, img.ptype());
        let tag = format!("Objects.lists[{owner}][{slot}]");
        // Animal::work (vtable +0x188): do_job(order_type, order) then, when
        // herd (+0x86) != -1 and (o + frame) % 16 == 0, mark the herd's
        // rare-resource record (DAT_00c0a0f0[Herds[herd]+0x14]) +0x20 with
        // every live player whose fog (FUN_006b55c0) sees the tile.
        // TODO(va 0x00617a10) do_job: no orders -> Animal::do_idle 0x005d7460
        // (Random::get draws; add_move_order when the idle animation ends);
        // MoveOrder -> Unit::do_move. Fields untouched.
        // TODO(va 0x005d7390) herd seen-mask (Items image, not ours).
        let ty = img.order(0).map(|(t, _, _)| t).unwrap_or(0);
        effects.push(format!("{tag}: Animal::work do_job(ty={ty}) 0x00617a10 not transcribed"));
        guys_process(save, img.0, ti, fr, false, &tag, effects);
    });
}

// ---------------------------------------------------------------------------
// Guy::process 0x005e0230
// ---------------------------------------------------------------------------

fn guy_process(save: &Save, u: &mut Unit, gi: usize, ti: TypeInfo, fr: i32, des_known: bool, tag: &str, effects: &mut Vec<String>) {
    guy_move(save, u, gi, ti, des_known, tag, effects);
    let Some(row) = guy_row(u, gi) else { return };
    let mut g = GImg(row);
    let gtag = format!("{tag}.guys[{gi}]");
    // guy_flags &= ~2
    let gf = g.u16(0x9a);
    if gf & 2 != 0 {
        g.set_u16(0x9a, gf & !2);
        effects.push(format!("{gtag}.guy_flags &= ~2"));
    }
    // Turret slew when guy_flags & 0x100.
    if g.u16(0x9a) & 0x100 != 0 {
        let mut nf = g.u16(0x96);
        for t in 0..4usize {
            let cur = g.u32(0x20 + t * 4);
            let des = g.u32(0x30 + t * 4);
            if des == cur {
                nf |= 1 << t;
                continue;
            }
            let d = cur.wrapping_sub(des);
            let ad = if d > 0x8000_0000 { !d } else { d };
            if ad < 0x0aaa_aaaa {
                g.set_u32(0x20 + t * 4, des);
                nf |= 1 << t;
            } else if d < 0x8000_0000 {
                g.set_u32(0x20 + t * 4, cur.wrapping_sub(0x0aaa_aaaa));
            } else {
                g.set_u32(0x20 + t * 4, cur.wrapping_add(0x0aaa_aaaa));
            }
        }
        if nf != g.u16(0x96) {
            g.set_u16(0x96, nf);
        }
        effects.push(format!("{gtag}.turret_angles slewed"));
    }
    // Every 64 frames per guy, when avg_speed == 0: trail stamp + clear 0x20.
    let o = g.i16(0x8c) as i32;
    if (fr + o) % 64 == 0 && g.i32(0x84) == 0 {
        if ti.kind != 2 && (g.u8(0xa2) as i8 as i32) < ti.num_guys {
            // TODO(va 0x005e02f0) footprint stamp: for each of the type's
            // +0x248 footprint cells (DAT_00add1e0 count, DAT_00adc400 /
            // DAT_00adcaf0 offsets) sets a bit in the World coll block
            // (+0x18 of the 0x1c tile, FUN_0046d250 allocating) and zeroes
            // its +8. World image, not this object; untouched.
        }
        let gf = g.u16(0x9a);
        if gf & 0x20 != 0 {
            g.set_u16(0x9a, gf & !0x20);
            effects.push(format!("{gtag}.guy_flags &= ~0x20"));
        }
    }
}

// ---------------------------------------------------------------------------
// Guy::move 0x005d9240
// ---------------------------------------------------------------------------

fn guy_move(save: &Save, u: &mut Unit, gi: usize, ti: TypeInfo, des_known: bool, tag: &str, effects: &mut Vec<String>) {
    let kind = ti.kind;
    let um = u32::from_le_bytes(u.body[0x68 - 0x48..0x6c - 0x48].try_into().unwrap());
    let Some(row) = guy_row(u, gi) else { return };
    let mut g = GImg(row);
    let gtag = format!("{tag}.guys[{gi}]");

    // last_x = x; last_y = y; if !(guy_flags & 0x40) last_z = z; last_angle = angle;
    let (x, y, z, angle) = (g.i32(0x0c), g.i32(0x10), g.i32(0x14), g.i32(0x18));
    let mut touched = false;
    if g.i32(0x68) != x || g.i32(0x6c) != y {
        g.set_i32(0x68, x);
        g.set_i32(0x6c, y);
        touched = true;
    }
    if g.u16(0x9a) & 0x40 == 0 && g.i32(0x70) != z {
        g.set_i32(0x70, z);
        touched = true;
    }
    if g.i32(0x1c) != angle {
        g.set_i32(0x1c, angle);
        touched = true;
    }
    if touched {
        effects.push(format!("{gtag}.last_x/last_y/last_z/last_angle <- x/y/z/angle"));
    }

    let (des_x, des_y, des_angle) = (g.i32(0x5c), g.i32(0x60), g.i32(0x64));
    if !des_known {
        // See `des_known`: do_job may have re-aimed this guy before we got
        // here; neither branch below can be chosen from the serialized des.
        effects.push(format!("{gtag}: Guy::move branch skipped (des may be rewritten by do_job 0x00617a10)"));
        return;
    }
    if des_x == x && des_y == y {
        // Stationary.
        if g.i32(0x80) != 0 {
            g.set_i32(0x80, 0);
            effects.push(format!("{gtag}.last_speed -> 0"));
        }
        if des_angle == angle {
            let hold = g.u8(0x9e) as i8;
            if hold == 0 {
                if g.u8(0x9c) == 8 && g.u8(0x9d) != 0 {
                    // TODO(va 0x005da300) Guy::set_anim(0,0,1): walk anim
                    // ended in place -> idle anim (may draw Random::get).
                }
                if g.u8(0x9d) != 1 {
                    g.set_u8(0x9d, 1);
                    effects.push(format!("{gtag}.stopped -> 1"));
                }
            } else {
                // TODO(va 0x005da300, 0x005d8bc0) set_anim(hold < 2 ? 0xb :
                // hold, 0, hold < 2); Guy::set_all_pivots; hold_attack = 0;
                // stopped = 1. Left untouched until set_anim is transcribed.
            }
        } else {
            // Turning in place.
            let order_ty = u.orders.orders.first().map(|o| o.ty).unwrap_or(0);
            let Some(row) = guy_row(u, gi) else { return };
            let mut g = GImg(row);
            if kind == 1 || order_ty == 0x19 {
                if g.u8(0x9d) != 1 {
                    g.set_u8(0x9d, 1);
                    effects.push(format!("{gtag}.stopped -> 1"));
                }
            } else {
                let hold = g.u8(0x9e) as i8;
                let anim = g.u8(0x9c);
                if hold == 0 {
                    if anim != 0x15 && anim != 0x16 && anim != 0x0a {
                        // TODO(va 0x005da300) set_anim(8,0,1) — walk anim.
                    }
                } else {
                    // TODO(va 0x005da300, 0x005d8bc0) attack-hold turn anims.
                }
                if g.u8(0x9d) != 0 {
                    g.set_u8(0x9d, 0);
                    effects.push(format!("{gtag}.stopped -> 0"));
                }
            }
            if g.u16(0x9a) & 2 == 0 {
                turn_towards(save, u, gi, ti, um, des_angle, &gtag, effects);
            }
        }
    } else {
        // Moving toward (des_x, des_y).
        // TODO(va 0x005d93b0) the moving half: set_anim(walk variant by
        // unit_masks 0x78000000 / herd flag), GuyData::get_speed 0x005de410
        // (last_speed = speed*11 >> 3), find_angle 0x0092d130 +
        // turn_towards, snap via Guy::set_new_location 0x005d86f0 when
        // within reach else step by the 0x0092d100/0x0092d0c0 trig
        // components, stopped = 0. x/y/z/angle/last_speed untouched; the
        // avg_speed epilogue below is skipped because last_speed is unknown.
        effects.push(format!("{gtag}: moving toward ({des_x},{des_y}) — Guy::move moving half not transcribed"));
        return;
    }
    // LAB_005d96ec: avg_speed = (avg_speed*3 + last_speed) >> 2 (signed floor)
    let Some(row) = guy_row(u, gi) else { return };
    let mut g = GImg(row);
    let avg = g.i32(0x84);
    let s = avg.wrapping_mul(3).wrapping_add(g.i32(0x80));
    let navg = (s + ((s >> 31) & 3)) >> 2;
    if navg != avg {
        g.set_i32(0x84, navg);
        effects.push(format!("{gtag}.avg_speed {avg} -> {navg}"));
    }
}

/// `GuyData::turn_speed(1)` 0x005de340.
fn turn_speed(save: &Save, g: &GImg, ti: TypeInfo, um: u32) -> u32 {
    let mut ts: u32 = 0x4000_0000;
    if (g.u8(0xa2) as i8 as i32) < ti.num_guys {
        ts = (ti.turn >> 8).wrapping_mul(constant(save, 8) as u32);
        if um & 0x80000 != 0 {
            ts = ts.wrapping_mul(constant(save, 0xc) as u32);
        }
    } else if g.i32(0x54) != 0 || g.i32(0x58) != 0 {
        return 0x4000_0000;
    }
    if g.i32(0x80) == 0 && g.u16(0x9a) & 0x10 != 0 {
        return 0x8000_0000;
    }
    ts
}

/// `Guy::turn_towards(des, _, 1)` 0x005d9720 → `Guy::do_turn` 0x005d97a0.
fn turn_towards(save: &Save, u: &mut Unit, gi: usize, ti: TypeInfo, um: u32, des: i32, gtag: &str, effects: &mut Vec<String>) {
    let Some(row) = guy_row(u, gi) else { return };
    let g = GImg(row);
    let cur = g.i32(0x18);
    let d = (des as u32).wrapping_sub(cur as u32);
    let ad = if d > 0x8000_0000 { !d } else { d };
    let new = if ad < 0x0222_2220 {
        des
    } else {
        let ts = turn_speed(save, &g, ti, um);
        if ts < ad {
            if d <= 0x8000_0000 {
                (cur as u32).wrapping_add(ts) as i32
            } else {
                (cur as u32).wrapping_sub(ts) as i32
            }
        } else {
            des
        }
    };
    do_turn(u, gi, ti, des, new, gtag, effects);
}

/// `Guy::do_turn(target, new_angle, _, 1)` 0x005d97a0.
fn do_turn(u: &mut Unit, gi: usize, ti: TypeInfo, target: i32, new: i32, gtag: &str, effects: &mut Vec<String>) {
    let Some(row) = guy_row(u, gi) else { return };
    let mut g = GImg(row);
    let cur = g.i32(0x18);
    if new != cur {
        let gf = g.u16(0x9a);
        g.set_u16(0x9a, gf | 2);
        if gf & 8 != 0 {
            // TODO(va 0x005da300) set_anim((target - angle) <= 0x80000000 ?
            // 0x16 : 0x15, 0, 1) — turn-in-place anims for guy_flags & 8.
            let _ = target;
        }
    }
    let saved_des = g.i32(0x64);
    g.set_i32(0x18, new);
    effects.push(format!("{gtag}.angle {cur} -> {new}"));
    let (gx, gy) = (g.i32(0x0c), g.i32(0x10));
    // Retail recurses on `guy_num == 0` alone; attachments carry their row
    // index there, so the extra `gi < num_guys` only guards malformed rows.
    let is_primary = g.u8(0xa2) == 0 && (gi as i32) < ti.num_guys;
    // set_angle(new, 0): des_angle = new (restored below); attachments get
    // des_angle/des_x/des_y from this guy.
    let len = u.guys.elems.len();
    if is_primary {
        for ei in (ti.num_guys.max(0) as usize)..len {
            let Some(erow) = guy_row(u, ei) else { continue };
            let mut e = GImg(erow);
            e.set_i32(0x64, new);
            if e.i32(0x54) != 0 || e.i32(0x58) != 0 {
                // TODO(va 0x005d90b0) track offset rotation (FUN_00a46a00
                // trig) and the map-edge clamp; des_x/des_y left untouched.
                continue;
            }
            e.set_i32(0x5c, gx);
            e.set_i32(0x60, gy);
        }
    }
    let Some(row) = guy_row(u, gi) else { return };
    let mut g = GImg(row);
    g.set_i32(0x64, saved_des);
    if is_primary {
        for ei in (ti.num_guys.max(0) as usize)..len {
            let skip = match guy_row(u, ei) {
                Some(erow) => {
                    let e = GImg(erow);
                    e.i32(0x54) != 0 || e.i32(0x58) != 0
                }
                None => true,
            };
            if !skip && ei != gi {
                do_turn(u, ei, ti, target, new, &format!("{gtag}~att{ei}"), effects);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{container, load};
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

    fn unit_of(o: &Obj) -> Option<&Unit> {
        match o {
            Obj::Unit(u) => Some(u),
            Obj::Animal(a) => Some(&a.unit),
            _ => None,
        }
    }

    /// (image offset, byte) for every serialized Unit byte; Guy rows at
    /// `0x30000 + gi*0x100 + GuyData offset`.
    fn image(u: &Unit) -> Vec<(usize, u8)> {
        let mut v = Vec::new();
        v.push((0x08, u.base.sub.flags));
        v.extend(u.base.sub.body.iter().enumerate().map(|(i, &x)| (0x9 + i, x)));
        v.extend(u.base.mid.iter().enumerate().map(|(i, &x)| (0x20 + i, x)));
        v.extend(u.body.iter().enumerate().map(|(i, &x)| (0x48 + i, x)));
        v.extend(u.path.data.iter().enumerate().map(|(i, &x)| (0x10000 + i, x)));
        for (oi, o) in u.orders.orders.iter().enumerate() {
            v.extend(o.payload.iter().enumerate().map(|(i, &x)| (0x20000 + oi * 0x100 + i, x)));
        }
        for (gi, g) in u.guys.elems.iter().enumerate() {
            if let Some(g) = g {
                v.extend(g.data.iter().enumerate().map(|(i, &x)| (0x30000 + gi * 0x100 + 8 + i, x)));
            }
        }
        v
    }

    fn field_name(off: usize) -> String {
        if off >= 0x30000 {
            let g = (off - 0x30000) % 0x100;
            let n = match g {
                0x0c..=0x0f => "x",
                0x10..=0x13 => "y",
                0x14..=0x17 => "z",
                0x18..=0x1b => "angle",
                0x1c..=0x1f => "last_angle",
                0x20..=0x2f => "turret_angles",
                0x30..=0x3f => "des_turret_angles",
                0x5c..=0x5f => "des_x",
                0x60..=0x63 => "des_y",
                0x64..=0x67 => "des_angle",
                0x68..=0x6b => "last_x",
                0x6c..=0x6f => "last_y",
                0x70..=0x73 => "last_z",
                0x74..=0x77 => "cur_time",
                0x78..=0x7b => "end_time",
                0x7c..=0x7f => "last_time",
                0x80..=0x83 => "last_speed",
                0x84..=0x87 => "avg_speed",
                0x96..=0x97 => "node_flags",
                0x9a..=0x9b => "guy_flags",
                0x9c => "cur_anim",
                0x9d => "stopped",
                0x9e => "hold_attack",
                _ => "?",
            };
            return format!("Guy.{n}");
        }
        if off >= 0x20000 {
            return "Order".into();
        }
        if off >= 0x10000 {
            return "Path".into();
        }
        let n = match off {
            0x08 => "SubObject.flags",
            0x0c..=0x0f => "SubObject.z",
            0x10..=0x13 => "SubObject.x",
            0x14..=0x17 => "SubObject.y",
            0x24..=0x27 => "Object.damage",
            0x2a..=0x2f => "Object.up/down/down_who",
            0x32..=0x33 => "Object.hold_frames",
            0x38..=0x39 => "Object.healing",
            0x3d => "Object.targeted",
            0x40 => "Object.visible",
            0x48..=0x4b => "Unit.collide_frame",
            0x50..=0x53 => "Unit.angle",
            0x58..=0x5b => "Unit.dest_angle",
            0x68..=0x6b => "Unit.unit_masks",
            0x6c..=0x6f => "Unit.unit_masks2",
            0x70..=0x77 => "Unit.orders_x/y",
            0x88..=0x8f => "Unit.collide*",
            0x98..=0x9d => "Unit.collide*",
            0xac => "Unit.full",
            0xad => "Unit.waiting",
            0xae => "Unit.recharging",
            0xaf => "Unit.path_recursion",
            0xb0 => "Unit.idle",
            0xb2 => "Unit.safe",
            _ => "?",
        };
        format!("+{off:#x} {n}")
    }

    /// Lane gate: run the full step on retail frame N and account every
    /// Unit/Animal byte against retail N+1 — `introduced` must be 0 on
    /// every pair; `explained` must be > 0 overall. The Build/Wall plane is
    /// gated by `build_process`'s own test.
    #[test]
    fn unit_plane_burndown_introduces_nothing() {
        let Some(dir) = capture_dir() else {
            eprintln!("no capture dir; skipping");
            return;
        };
        let st = steps(&dir);
        let (mut explained, mut unexplained, mut introduced) = (0usize, 0usize, 0usize);
        let mut introduced_rows = Vec::new();
        let mut unexplained_by: std::collections::BTreeMap<String, usize> = Default::default();
        let mut explained_by: std::collections::BTreeMap<String, usize> = Default::default();
        let mut draws_total = 0u32;
        for k in 0..st.len() - 1 {
            if st[k + 1].0 - st[k].0 != 1 {
                continue;
            }
            let ra = container::load_svx(&dir.join(format!("{}.svx", st[k].1))).unwrap();
            let rb = container::load_svx(&dir.join(format!("{}.svx", st[k + 1].1))).unwrap();
            let a = load(&ra).unwrap().state;
            let b = load(&rb).unwrap().state;
            let mut ours = a.clone();
            let mut effects = Vec::new();
            let s0 = u32::from_le_bytes(ours.post_world[0x28..0x2c].try_into().unwrap());
            run(&mut ours, &mut effects);
            let s1 = u32::from_le_bytes(ours.post_world[0x28..0x2c].try_into().unwrap());
            let draws = crate::tick::rng_draws(s0, s1).unwrap_or(0);
            draws_total += draws;
            let (mut pe, mut pu, mut pi) = (0, 0, 0);
            for owner in 0..a.objects.lists.len() {
                for slot in 0..a.objects.lists[owner].elems.len().min(b.objects.lists[owner].elems.len()) {
                    let (Some(oa), Some(ob), Some(oo)) = (
                        &a.objects.lists[owner].elems[slot],
                        &b.objects.lists[owner].elems[slot],
                        &ours.objects.lists[owner].elems[slot],
                    ) else {
                        continue;
                    };
                    let (Some(xa), Some(xb), Some(xo)) = (unit_of(oa), unit_of(ob), unit_of(oo)) else { continue };
                    let (ia, ib, io) = (image(xa), image(xb), image(xo));
                    assert_eq!(ia.len(), io.len(), "[{owner}][{slot}] we changed the image length");
                    for i in 0..ia.len().min(ib.len()) {
                        if ia[i].0 != ib[i].0 {
                            break;
                        }
                        let (ra, rb, ro) = (ia[i].1, ib[i].1, io[i].1);
                        if ra != rb {
                            if ro == rb {
                                pe += 1;
                                *explained_by.entry(field_name(ia[i].0)).or_default() += 1;
                            } else {
                                pu += 1;
                                *unexplained_by.entry(field_name(ia[i].0)).or_default() += 1;
                            }
                        } else if ro != ra {
                            pi += 1;
                            introduced_rows.push(format!(
                                "f{}->f{} [{owner}][{slot}] {}: retail {ra:#04x} ours {ro:#04x}",
                                st[k].0,
                                st[k + 1].0,
                                field_name(ia[i].0)
                            ));
                        }
                    }
                }
            }
            eprintln!("pair f{}->f{}: explained={pe} unexplained={pu} introduced={pi} draws={draws}", st[k].0, st[k + 1].0);
            explained += pe;
            unexplained += pu;
            introduced += pi;
        }
        eprintln!("unit plane bytes: explained={explained} unexplained={unexplained} introduced={introduced} draws_total={draws_total}");
        let mut ub: Vec<_> = unexplained_by.into_iter().collect();
        ub.sort_by(|a, b| b.1.cmp(&a.1));
        for (k, v) in ub.iter().take(30) {
            eprintln!("  unexplained {v:>7}  {k}");
        }
        let mut eb: Vec<_> = explained_by.into_iter().collect();
        eb.sort_by(|a, b| b.1.cmp(&a.1));
        for (k, v) in eb.iter().take(30) {
            eprintln!("  explained   {v:>7}  {k}");
        }
        for r in introduced_rows.iter().take(60) {
            eprintln!("  INTRODUCED {r}");
        }
        assert_eq!(introduced, 0, "introduced bytes");
        assert!(explained > 0, "no Unit byte explained");
    }

    /// Diagnostic: dump every Unit/Animal byte retail changed between
    /// consecutive captures (`-- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn dump_unit_diffs() {
        let Some(dir) = capture_dir() else { return };
        let st = steps(&dir);
        for k in 0..st.len() - 1 {
            if st[k + 1].0 - st[k].0 != 1 {
                continue;
            }
            let ra = container::load_svx(&dir.join(format!("{}.svx", st[k].1))).unwrap();
            let rb = container::load_svx(&dir.join(format!("{}.svx", st[k + 1].1))).unwrap();
            let a = load(&ra).unwrap().state;
            let b = load(&rb).unwrap().state;
            println!("== f{} (frame field {})", st[k].0, frame(&a));
            for owner in 0..a.objects.lists.len() {
                let la = &a.objects.lists[owner];
                let lb = &b.objects.lists[owner];
                for slot in 0..la.elems.len().min(lb.elems.len()) {
                    let (Some(oa), Some(ob)) = (&la.elems[slot], &lb.elems[slot]) else { continue };
                    let (Some(ua), Some(ub)) = (unit_of(oa), unit_of(ob)) else { continue };
                    let (ia, ib) = (image(ua), image(ub));
                    let mut diffs: Vec<String> = ia
                        .iter()
                        .zip(ib.iter())
                        .filter(|(x, y)| x.1 != y.1)
                        .map(|(x, y)| format!("+{:#x}:{:#04x}->{:#04x}", x.0, x.1, y.1))
                        .collect();
                    if ia.len() != ib.len() {
                        diffs.push(format!("len {}->{}", ia.len(), ib.len()));
                    }
                    if diffs.is_empty() {
                        continue;
                    }
                    let ptype = if ua.base.sub.body.len() == 19 { i32::from_le_bytes(ua.base.sub.body[15..19].try_into().unwrap()) } else { -1 };
                    let ords: Vec<(i32, u8)> = ua.orders.orders.iter().map(|o| (o.ty, o.payload.first().copied().unwrap_or(0))).collect();
                    println!(
                        "  [{owner}][{slot}] ty={} who={} ptype={ptype} flags={:#x} guys={} path={} orders={:?} {}",
                        oa.ty(),
                        ua.base.sub.body.first().copied().unwrap_or(0),
                        ua.base.sub.flags,
                        ua.guys.elems.len(),
                        ua.path.data.len() / 16,
                        ords,
                        diffs.join(" ")
                    );
                }
            }
        }
    }
}
