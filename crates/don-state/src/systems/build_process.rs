//! `Build::process` 0x0061edf0 and `Wall::process` 0x00640450 — the
//! virtual `process` bodies that `Objects::process_all` 0x0065dce0 reaches
//! through vtable slot +0x9c for every *active* object on the Build plane
//! (slots 2000..build_mark) and Wall plane (slots 3000..wall_mark).
//!
//! The object is a byte image; every read/write below is addressed by the
//! retail image offset (PDB `WallData`/`BuildData`) through [`Img`], which
//! maps it onto the serialized sub-ranges in [`crate::sections::Build`]:
//!
//! ```text
//!   +0x09..+0x1c  SubObject  who,o,z,x,y,ptype_index         base.sub.body
//!   +0x20..+0x42  ObjectData myhits..launch_frames           base.mid
//!   +0x48..+0x66  WallData   job_counter..demolition         wall_body
//!   +0x70..+0x86  BuildData  gather_down..infiltrate2        body
//!   +0x7f,+0x83   founder, max_age (also emitted ahead)      head[0], head[1]
//!   +0x6c         orig_type                                  orig_type
//!   queue rows    QueueItem 18B (job_counter i32, type i16)  queue
//! ```
//!
//! Status is `Partial`: every branch below is either transcribed (writes
//! performed) or marked `TODO(va)` and left with its fields untouched. The
//! largest untranscribed child is `ObjectData::construct_time`
//! `FUN_006508c0` (the total job time the production queue clamps against),
//! so `Build::do_queue` 0x0061e410 transcribes its gates and rate selection
//! but cannot yet advance `QueueItem.job_counter` (see [`do_queue`]).
//!
//! RNG: none of the transcribed paths call `Random::get` 0x00a39d70. The
//! only `game_random` draws under these bodies are inside untranscribed
//! children (`ObjectsData::find_unit` 0x0065ca80 via the inactive-site AI
//! builder dispatch, `Build::check_capture` 0x006276a0, `City::assimilate`
//! 0x00738e90) — none reached by the idle early-game captures.

use crate::sections::{Build, Obj, Save, TypeRec};
use crate::tick::StepStatus;

pub const STATUS: StepStatus = StepStatus::Partial;

/// Retail traversal order for the Build/Wall planes (`Objects::process_all`
/// 0x0065dce0, second loop): owners 0..8 ascending over the eight
/// `LeaderData` slots `0x00e3a390..0x00e71af0` whose `flags & 1`; per owner
/// the Build slots `2000..build_mark` then the Wall slots
/// `3000..wall_mark`; only objects with `SubObject.flags & 1` reach the
/// virtual `process` (inactive ones get the `hold_frames` countdown in the
/// traversal itself, owned by `objects_process.rs`).
pub fn run(save: &mut Save, effects: &mut Vec<String>) {
    for owner in 0..8usize {
        if save.leaders.slots.get(owner).map(|l| l.flags & 1).unwrap_or(0) == 0 {
            continue;
        }
        if owner >= save.objects.lists.len() {
            continue;
        }
        let bmark = build_mark(save, owner).max(2000) as usize;
        let wmark = wall_mark(save, owner).max(3000) as usize;
        for slot in 2000..bmark {
            if is_active_build(save, owner, slot) {
                process_build(save, owner, slot, effects);
            }
        }
        for slot in 3000..wmark {
            if is_active_build(save, owner, slot) {
                process_wall(save, owner, slot, effects);
            }
        }
    }
}

fn is_active_build(save: &Save, owner: usize, slot: usize) -> bool {
    matches!(
        save.objects.lists[owner].elems.get(slot),
        Some(Some(Obj::Build(b))) if b.base.sub.flags & 1 != 0
    )
}

// ---------------------------------------------------------------------------
// Image accessor
// ---------------------------------------------------------------------------

/// Byte-image view of a serialized `Build`, addressed by retail offset.
struct Img<'a>(&'a mut Build);

impl Img<'_> {
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
    fn u8(&mut self, off: usize) -> u8 {
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
    fn u16(&mut self, off: usize) -> u16 {
        self.i16(off) as u16
    }
    fn i32(&mut self, off: usize) -> i32 {
        let (v, i) = self.slot(off);
        i32::from_le_bytes(v[i..i + 4].try_into().unwrap())
    }
    fn set_u8(&mut self, off: usize, x: u8) {
        let (v, i) = self.slot(off);
        v[i] = x;
        // founder/max_age are serialized twice (head + body); keep both.
        if off == 0x7f {
            self.0.head[0] = x;
        } else if off == 0x83 {
            self.0.head[1] = x;
        }
    }
    fn set_i16(&mut self, off: usize, x: i16) {
        let (v, i) = self.slot(off);
        v[i..i + 2].copy_from_slice(&x.to_le_bytes());
    }
    fn set_u16(&mut self, off: usize, x: u16) {
        self.set_i16(off, x as i16)
    }
    /// SubObject flags byte (+0x08) — serialized separately as `sub.flags`.
    /// Bit 1 live, bit 2 started (`WallData::is_started` 0x00472360),
    /// bit 4 active (`WallData::is_active` 0x00472350), bit 0x20 city.
    fn flags(&self) -> u8 {
        self.0.base.sub.flags
    }
    fn who(&mut self) -> u8 {
        self.u8(0x09)
    }
    fn o(&mut self) -> i32 {
        self.i16(0x0a) as i32
    }
    fn ptype(&mut self) -> i32 {
        self.i32(0x18)
    }
    /// Coord `x_internal`/`y_internal` are stored XOR 0x63637 (every retail
    /// reader applies `^ 0x63637`).
    fn x(&mut self) -> i32 {
        self.i32(0x10) ^ 0x63637
    }
    fn y(&mut self) -> i32 {
        self.i32(0x14) ^ 0x63637
    }
    fn build_masks(&mut self) -> u16 {
        self.u16(0x60)
    }
    fn set_build_masks(&mut self, m: u16) {
        self.set_u16(0x60, m)
    }
    /// `QueueItem` row `slot` (18 serialized bytes of the 0x14 runtime item).
    fn queue_row(&self, slot: usize) -> Option<&[u8]> {
        self.0.queue.chunks_exact(18).nth(slot)
    }
}

// ---------------------------------------------------------------------------
// Global-state readers (Game, Leaders, World, Rules) used by the bodies
// ---------------------------------------------------------------------------

/// `Game+0x550` (`Game::frame`).
fn frame(save: &Save) -> i32 {
    i32::from_le_bytes(save.game.scalars[0..4].try_into().unwrap())
}

/// `GameInfo` single-byte setting at `Game+0x24+i` (`settings[i]`; GameInfo
/// sits at Game+0xc so `settings` = gi+0x18 = Game+0x24).
fn game_setting(save: &Save, image_off: usize) -> u8 {
    save.game.info.settings[image_off - 0x24]
}

/// `Constants` i32 at image offset `off` (`[0x00c061f0]`, walked as
/// `Save.constants`, 0xd40 bytes).
fn constant(save: &Save, off: usize) -> i32 {
    i32::from_le_bytes(save.constants[off..off + 4].try_into().unwrap())
}

/// `LeaderData` byte at image offset `off` (`flags` +0, `flags2` +4, body
/// +0x08..+0x692a). `None` when the slot has no walked body.
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

fn leader_flags(save: &Save, who: usize) -> i32 {
    save.leaders.slots.get(who).map(|l| l.flags).unwrap_or(0)
}

/// `init_coord_lookup_array` 0x00681db0: `lookup[i] = i / 3` (floor; the
/// negative half is filled with `(i - 2) / 3`). Coord `>> 8` then lookup
/// gives the tile column/row (a tile is 0x300 coord units).
fn coord_lookup(i: i32) -> i32 {
    i.div_euclid(3)
}

/// Serialized `WData` (the first 21 bytes of the 0x1c runtime tile) under
/// coord `(x, y)`: `World+0x134 + (lookup[y>>8] * xs + lookup[x>>8]) * 0x1c`.
fn tile_at(save: &Save, x: i32, y: i32) -> Option<&[u8]> {
    let w = &save.world;
    let tx = coord_lookup(x >> 8);
    let ty = coord_lookup(y >> 8);
    if tx < 0 || ty < 0 || tx >= w.xs || ty >= w.ys {
        return None;
    }
    let idx = (ty * w.xs + tx) as usize;
    w.wdata.get(idx * 21..idx * 21 + 21)
}

/// `Objects` scalar block: `[0x1f4..0x1fc) [0x154..0x15c) unit_mark[9]
/// build_mark[9] wall_mark[9] obj_ctr[9]` (Objects::walk_data 0x006541e0).
fn build_mark(save: &Save, owner: usize) -> i32 {
    let o = 16 + 36 + owner * 4;
    i32::from_le_bytes(save.objects.scalars[o..o + 4].try_into().unwrap())
}

fn wall_mark(save: &Save, owner: usize) -> i32 {
    let o = 16 + 72 + owner * 4;
    i32::from_le_bytes(save.objects.scalars[o..o + 4].try_into().unwrap())
}

/// Type record image reader (`Rules.types[idx]`): `head` = image[4..94),
/// `obj_mid` = image[0x1e4..0x27c), `ext` = per-kind tail from image 0x2b4.
struct TypeImg<'a>(&'a TypeRec);

impl TypeImg<'_> {
    fn i32(&self, off: usize) -> Option<i32> {
        let (v, i) = match off {
            0x04..=0x5d => (&self.0.head, off - 4),
            0x1e4..=0x27b => (&self.0.obj_mid, off - 0x1e4),
            0x2b4.. => (&self.0.ext, off - 0x2b4),
            _ => return None,
        };
        v.get(i..i + 4).map(|b| i32::from_le_bytes(b.try_into().unwrap()))
    }
    /// `is_list` (+0x280/+0x28c) and `is_strict_list` (+0x29c/+0x2a8) are
    /// the two `SimpleArray<u16>` that follow the object body.
    fn list(&self, strict: bool) -> impl Iterator<Item = u16> + '_ {
        let a = if strict { &self.0.arr1 } else { &self.0.arr0 };
        a.data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]))
    }
}

fn type_rec(save: &Save, idx: i32) -> Option<TypeImg<'_>> {
    usize::try_from(idx).ok().and_then(|i| save.rules_tail.rules.types.get(i)).map(TypeImg)
}

/// `TypeData::is_unit_type` 0x004707e0 default body: `0x32 <= type <= 0x19d`.
fn is_unit_type(idx: i32) -> bool {
    (0x32..=0x19d).contains(&idx)
}

/// `TypeData::is_building_type` 0x004707c0 default body: `0x19e <= type <= 0x21e`.
fn is_building_type(idx: i32) -> bool {
    (0x19e..=0x21e).contains(&idx)
}

/// `BuildData::is_wonder` 0x00472320 (Build vtable arm): `0x20e <= ptype <= 0x21e`.
fn is_wonder(ptype: i32) -> bool {
    (0x20e..=0x21e).contains(&ptype)
}

/// `ObjectTypeData::is_slow` 0x00661ae0 (`strict == 0` arm): the type
/// itself, its `graft` (+0x25c), or recursion through `from` (+0x3c).
fn type_is_slow(save: &Save, this: i32, what: i32, depth: u8) -> bool {
    if this == what {
        return true;
    }
    if what < 0 || depth > 16 {
        return false;
    }
    let Some(t) = type_rec(save, this) else { return false };
    if t.i32(0x25c) == Some(what) {
        return true;
    }
    match t.i32(0x3c) {
        Some(from) if from >= 0 => type_is_slow(save, from, what, depth + 1),
        _ => false,
    }
}

/// `ObjectTypeData::is` 0x0065f7d0 `(what, strict)`: exact match, else the
/// serialized `is_list`/`is_strict_list`; an empty list falls back to
/// `is_slow` (0x00661ae0).
fn type_is(save: &Save, this: i32, what: i32, strict: bool) -> bool {
    if this == what {
        return true;
    }
    let Some(t) = type_rec(save, this) else { return false };
    let mut it = t.list(strict).peekable();
    if it.peek().is_none() {
        if !strict {
            return what >= 0 && type_is_slow(save, this, what, 0);
        }
        // is_slow strict arm: unit types only; graft match unless the
        // target's +0x2b4 carries 0x1000000.
        if !is_unit_type(this) {
            return false;
        }
        if t.i32(0x25c) == Some(what) {
            if let Some(w) = type_rec(save, what) {
                return w.i32(0x2b4).map(|f| f & 0x1000000 == 0).unwrap_or(false);
            }
        }
        return false;
    }
    if !strict && what < 0 {
        return false;
    }
    it.any(|v| v as i32 == what)
}

/// `BuildType.build_flags` (+0x2c0): 0x40 `is_gather_type` 0x00472bb0,
/// 0x10000000 `is_flat` 0x00472b20.
fn build_flags(save: &Save, ptype: i32) -> u32 {
    type_rec(save, ptype).and_then(|t| t.i32(0x2c0)).unwrap_or(0) as u32
}

/// `BuildTypeData::is_fort` 0x00472ba0 = `is(0x1bb, 0)`; `is_dock`
/// 0x00472a70 = `is(0x1b0, 0)`.
fn is_fort(save: &Save, ptype: i32) -> bool {
    type_is(save, ptype, 0x1bb, false)
}
fn is_dock(save: &Save, ptype: i32) -> bool {
    type_is(save, ptype, 0x1b0, false)
}

/// Take the object out of its slot for the duration of a body so the body
/// can read `&Save` while mutating the image; restores it afterwards.
fn with_build(save: &mut Save, owner: usize, slot: usize, f: impl FnOnce(&Save, &mut Build)) {
    let taken = save.objects.lists[owner].elems[slot].take();
    let Some(Obj::Build(mut b)) = taken else {
        save.objects.lists[owner].elems[slot] = taken;
        return;
    };
    f(save, &mut b);
    save.objects.lists[owner].elems[slot] = Some(Obj::Build(b));
}

// ---------------------------------------------------------------------------
// Wall::process 0x00640450
// ---------------------------------------------------------------------------

/// `Wall::process` — the base-class body `Build::process` calls first.
pub fn process_wall(save: &mut Save, owner: usize, slot: usize, effects: &mut Vec<String>) {
    with_build(save, owner, slot, |save, b| wall_body(save, b, owner, slot, effects));
}

fn wall_body(save: &Save, b: &mut Build, owner: usize, slot: usize, effects: &mut Vec<String>) {
    let mut img = Img(b);
    if !img.complete() {
        return;
    }
    let fr = frame(save);
    let who = img.who() as usize;
    let o = img.o();
    let tag = format!("Objects.lists[{owner}][{slot}]");

    // if (frame != 0 && (frame & 7) == who) {
    //   targeted = (targeted + ((targeted >> 31) & 3)) >> 2;   // signed /4
    //   check_ever_seen(0);                                     // 0x0063ce70
    // }
    if fr != 0 && (fr & 7) == who as i32 {
        let t = img.i8(0x3d) as i32;
        let nt = (t + ((t >> 31) & 3)) >> 2;
        if nt != t {
            img.set_u8(0x3d, nt as i8 as u8);
            effects.push(format!("{tag}.Object.targeted {t} -> {nt}"));
        }
        // TODO(va 0x0063ce70) Wall::check_ever_seen(0): ORs the fog `seen`
        // grid (World+0x15c, stride World+0xc) over the footprint from
        // WallData::tile_corner 0x00643440 into ever_seen (+0x62) and, when
        // active, ever_seen_completed (+0x63); newly-met players get
        // Leader::meet 0x006e1250 and Wall::update_local_seen. The fog-grid
        // unit (the loop halves tile_corner's cell index) is unconfirmed
        // against World::walk_data, so ever_seen is left untouched.
    }

    // if ((frame + o) % 32 == 0) {
    if (fr + o) % 32 == 0 {
        let bm = img.build_masks();
        let nbm = if bm & 0x10 == 0 { bm & !0x20 } else { bm & !0x10 };
        if nbm != bm {
            img.set_build_masks(nbm);
            effects.push(format!("{tag}.Wall.build_masks {bm:#x} -> {nbm:#x} (0x10/0x20 phase)"));
        }
        // if (!is_active() && (leader[who].flags & 4) == 0) { ... }   AI-owned unbuilt site
        let active = img.flags() & 4 != 0;
        if !active && leader_flags(save, who) & 4 == 0 {
            let pt = img.ptype();
            if (fr + o) % 128 == 0 && type_is(save, pt, 0x1a6, false) {
                // TODO(va 0x0065ca80) ObjectsData::find_unit(x, y, 1, who, -1, 0, 0xb,
                // o, who, 0, 0, 0, y) < 0  ->  Object::disband(0) 0x006455c0.
            }
            // if (is_wonder() || (ptype->is_fort() && damage == 0)) { ... }
            if is_wonder(pt) || (is_fort(save, pt) && img.i32(0x24) == 0) {
                // TODO(va 0x0065ca80, 0x005e5210) AI builder dispatch: counts
                // helpers against the other leaders' build progress ratios,
                // find_unit(x, y, 1, who, 0xf00, 0x200, 1, 0x32, 0, 0xc, 0, 0, y)
                // then Unit::add_build_order(o, who, 2, 0). No object fields
                // are written on this path; the unit-side order is not ours.
            }
        }
    }

    // if (helpers == 0) build_masks &= ~0x400; else { helpers = 0; build_masks |= 0x400; }
    // build_masks &= ~0x800;
    {
        let bm = img.build_masks();
        let helpers = img.u8(0x64);
        let mut nbm = if helpers == 0 { bm & !0x400 } else { bm | 0x400 };
        if helpers != 0 {
            img.set_u8(0x64, 0);
            effects.push(format!("{tag}.Wall.helpers {helpers} -> 0"));
        }
        nbm &= !0x800;
        if nbm != bm {
            img.set_build_masks(nbm);
            effects.push(format!("{tag}.Wall.build_masks {bm:#x} -> {nbm:#x} (helpers/0x800)"));
        }
    }

    // Territory check, every 16 frames per object.
    let t = fr + o;
    if t % 16 != 0 {
        return;
    }
    // if (leader[who] +0x804 != 0) return;
    if leader_i32(save, who, 0x804).unwrap_or(0) != 0 {
        return;
    }
    // if (Game.rush_rules == 0 || Game::war_allowed()) { if (t % 32 != 0) return; <body> }
    // else { <body> }
    if game_setting(save, 0x32) != 0 {
        // TODO(va 0x00594670) Game::war_allowed under rush rules: compares the
        // current age (FUN_005946d0) with rush_rules and the rush timer
        // [DAT_00e80088 + rush*0x58 + 0x3c] * 900 against Game::frame. Until
        // transcribed the whole territory body is skipped (fields untouched).
        return;
    }
    if t % 32 != 0 {
        return;
    }
    // owner = tile(x, y).who (+0xf); docks re-resolve through tile_corner +
    // BuildTypeData::check_enemy_adjacent 0x00638d70.
    let (x, y) = (img.x(), img.y());
    let Some(tile) = tile_at(save, x, y) else { return };
    let terr = tile[0xf] as i8 as i32;
    if is_dock(save, img.ptype()) {
        // TODO(va 0x00638d70) BuildTypeData::check_enemy_adjacent for docks.
        return;
    }
    if terr < 0 || terr == who as i32 {
        return;
    }
    // if (LeaderData::is_ally(terr)) return;
    // TODO(va 0x006edb50) LeaderData::is_ally and the remainder of the
    // foreign-territory body: ever_seen |= 1 << terr when the owner's flags
    // carry 0x2000 (+ check_ever_seen(1)); !is_started -> Object::disband(0);
    // else Object::take_damage(8, 0, 1, -1, 1, 0x55555555, -1, -1, 0)
    // attrition and the leader +0x1fc "in enemy territory" stamp. Fields
    // untouched.
}

// ---------------------------------------------------------------------------
// Build::process 0x0061edf0
// ---------------------------------------------------------------------------

pub fn process_build(save: &mut Save, owner: usize, slot: usize, effects: &mut Vec<String>) {
    with_build(save, owner, slot, |save, b| {
        wall_body(save, b, owner, slot, effects);
        build_body(save, b, owner, slot, effects);
    });
}

fn build_body(save: &Save, b: &mut Build, owner: usize, slot: usize, effects: &mut Vec<String>) {
    let mut img = Img(b);
    if !img.complete() {
        return;
    }
    let fr = frame(save);
    let who = img.who() as usize;
    let o = img.o();
    let pt = img.ptype();
    let tag = format!("Objects.lists[{owner}][{slot}]");

    // if (!is_active()) return;            // flags & 4
    if img.flags() & 4 == 0 {
        return;
    }
    // if (healing != 0) healing -= 1;      // +0x38
    let healing = img.i16(0x38);
    if healing != 0 {
        img.set_i16(0x38, healing - 1);
        effects.push(format!("{tag}.Object.healing {healing} -> {}", healing - 1));
    }
    let bm = img.build_masks();
    if bm & 0x4000 != 0 {
        // TODO(va 0x006201e0) Build::process_ejection (garrison ejection).
    }
    if bm & 0x8 != 0 {
        // TODO(va 0x0064f3b0) Object::do_launch (missile/launch state).
    }
    // if (ptype->attack != 0) { ... find_target / do_attack / angle turn }
    if type_rec(save, pt).and_then(|t| t.i32(0x1e8)).unwrap_or(0) != 0 {
        // TODO(va 0x006228f0, 0x00648d70, 0x0092d130) armed building:
        // Build::do_attack, ObjectData::is_in_range, and the +0x48 angle
        // slew toward find_angle(). Fields untouched.
    }
    // Derived-class neutralization path (`*this != Build::vftable`) is not
    // reachable for a plain Build; the un-neutralize branch follows:
    if bm & 0x200 != 0 {
        // TODO(va) build_masks &= ~0x200 plus City flag restores
        // (CityData+4 |= 0x80/0x400/0x200/0x800 by ptype->is(0x1b5/0x1a8/0x1a7/0x1b4)).
        // Writes City records (not this object); left untouched.
    }
    // Garrison repair: if any of leader +0x59c0/+0x59c4/+0x59c8 (shorts) are
    // nonzero and (damage || damage_frac) and (o + frame) % Constants+0x698 == 0
    // then has_general(0, 0x163/0x161/0x165) gates a repair_damage(1, 0, 1)
    // and healing = max(healing, Constants+0x698).
    let repair_live = [0x59c0usize, 0x59c4, 0x59c8].iter().any(|&off| leader_i16(save, who, off).unwrap_or(0) != 0);
    if repair_live && (img.i32(0x24) != 0 || img.i8(0x3b) != 0) {
        let period = constant(save, 0x698);
        if period != 0 && (o + fr) % period == 0 {
            // TODO(va 0x00646b00, 0x00628130) ObjectData::has_general +
            // Build::repair_damage. Fields untouched.
        }
    }
    // do_queue(0)                           // vtable +0x1b4 = 0x0061e410
    do_queue(save, &mut img, 0, &tag, effects);
    // Gather-site maintenance: ptype->is_gather_type() && !ptype->is_flat()
    let bf = build_flags(save, pt);
    if bf & 0x40 != 0 && bf & 0x10000000 == 0 {
        // region = tile(x,y).region (+4); flags = Regions[region].flags (+8)
        // & 0x20 -> Build::verify_gather_tiles 0x00623570
        // & 0x10 -> Build::find_gather_tiles   0x00623350
        // TODO(va 0x00623570, 0x00623350) both rewrite gather_from (mining
        // list) and World tdata bits; left untouched.
    }
    // ptype == 0x21a (538): every 15 frames, leader flag 0x10000000 -> recharging countdown + train(0x3a)
    // leader +0x1a48 == frame + 1 -> gov hero / building-cities train
    // ptype == 0x1b4 (436): has_tribe_bonus(0x17) caravan auto-train every 15 frames
    // ptype == 0x211 (529): recharging countdown -> current_upgrade train
    if pt == 0x21a || pt == 0x1b4 || pt == 0x211 || leader_i32(save, who, 0x1a48) == Some(fr + 1) {
        // TODO(va 0x0062f9b0) Build::train and the LeaderData helpers
        // (has_tribe_bonus 0x006e1370, get_gov_hero 0x006e0600,
        // current_upgrade 0x006e3140, check_population 0x006e1330). These
        // write `recharging` (+0x7a) and spawn units; left untouched.
    }
    // (frame + o) % 256 == 0: human-player advisories for is(0x1b4)/is(0x1a4)
    // — gated on who == local player (UI global [0x00c06210]+0x298) and write
    // only leader +0x6ec0/+0x6ec4 stamps, which are outside the walked body.
    // No sim-state effect; nothing to transcribe here.

    // City-attached block: flags & 0x20 && city (+0x72) >= 0
    if img.flags() & 0x20 != 0 && img.i16(0x72) >= 0 {
        // TODO(va) every 200 frames: CityData+4 bit 2/4 toggle and the
        // +0x61 countdown; team_style == 2 -> ever_seen |= team bit;
        // (frame+o)%16 human advisory; (frame+o)%64 -> check_capture_eligible
        // 0x0062d1d0 / find_unit / Build::check_capture 0x006276a0 (RNG) /
        // add_repair_order; foreign-founder assimilation (City::assimilate
        // 0x00738e90, RNG); CityData get_level/get_radius territory refresh.
        // All of these write City/Leader/other-object state; untouched.
    }

    // if (build_masks & 0x100) && (frame + o) % 16 == 0: build_masks &= ~0x100;
    //   BuildType::place_roads(x, y, o, who, 1, 3)   // 0x0063c580
    // Ordering caveat: inside the city block above, the (frame+o)%64
    // check_capture path (0x006276a0) returns before reaching here when it
    // yields non-zero; that path is untranscribed, so a captured city with
    // 0x100 set on such a frame would be cleared here where retail kept it.
    let bm = img.build_masks();
    if bm & 0x100 != 0 && (fr + o) % 16 == 0 {
        img.set_build_masks(bm & !0x100);
        effects.push(format!("{tag}.Wall.build_masks {bm:#x} -> {:#x} (~0x100 road pass)", bm & !0x100));
        // TODO(va 0x0063c580) BuildType::place_roads — writes TerrainRoads /
        // World tdata around the footprint; not this object's fields.
    }
}

// ---------------------------------------------------------------------------
// Build::do_queue 0x0061e410
// ---------------------------------------------------------------------------

/// `Build::do_queue(slot)`: advances `QueueItem[slot].job_counter` by the
/// per-frame rate, clamped to `ObjectData::construct_time(type)`
/// (`FUN_006508c0`), and finishes the item when the counter has reached it.
///
/// Transcribed: the `queued == 0` exit, the Library-capital gate, the
/// is(0x208) gate, the rate selection (Constants +0x22c for building types,
/// +0x230 / +0x228 for units by the leader's known-type bit, scaled by the
/// game-speed multiplier `[0x00c061c0]` when > 1). Not transcribed:
/// `construct_time` itself (hundreds of tech/government/tribe modifiers
/// over LeaderData state) — without it neither the clamp nor the
/// completion test can be evaluated, so `job_counter` is left untouched.
fn do_queue(save: &Save, img: &mut Img, slot: usize, tag: &str, effects: &mut Vec<String>) {
    // if (queued == 0) return;             // +0x82
    let queued = img.u8(0x82);
    if queued == 0 {
        return;
    }
    let who = img.who() as usize;
    let pt = img.ptype();
    // type = slot < queue_size ? queue[slot].type : -1
    let ty = img.queue_row(slot).map(|r| i16::from_le_bytes([r[4], r[5]]) as i32).unwrap_or(-1);
    let progress = img.queue_row(slot).map(|r| i32::from_le_bytes(r[0..4].try_into().unwrap())).unwrap_or(-1);

    // if (ptype->is(0x1b3, 0) && o != LeaderData::get_first_library() && type != 0x29a) return;
    if type_is(save, pt, 0x1b3, false) && ty != 0x29a {
        // TODO(va 0x006db6c0) LeaderData::get_first_library — scans the
        // leader's build list for the first Library; only that one
        // researches. Cannot be evaluated yet; treat as "not this one" and
        // leave the row untouched.
        return;
    }
    // if (ptype->is(0x208, 1) && inside_down >= 0 && leader known-bit(type) && type != 0x29a) return;
    if type_is(save, pt, 0x208, true) && img.i16(0x28) >= 0 && ty != 0x29a {
        let known = ty >= 0
            && leader_u8(save, who, 0x6c18 + (ty >> 3) as usize).map(|b| b & (1 << (ty & 7)) != 0).unwrap_or(false);
        if known {
            return;
        }
    }
    // total = ObjectData::construct_time(type)   // FUN_006508c0
    // TODO(va 0x006508c0) — see fn doc. Rate selection below is transcribed
    // so the lead can see what the write would be once `total` is known.
    let rate_base = if is_building_type(ty) || ty == 0x29a {
        constant(save, 0x22c)
    } else if is_unit_type(ty) {
        let known = ty >= 0
            && leader_u8(save, who, 0x6c18 + (ty >> 3) as usize).map(|b| b & (1 << (ty & 7)) != 0).unwrap_or(false);
        if known {
            constant(save, 0x228) // then LeaderData::check_population gates (TODO 0x006e1330)
        } else {
            constant(save, 0x230)
        }
    } else {
        constant(save, 0x230)
    };
    // if ([0x00c061c0] > 1) rate *= [0x00c061c0]  — the game-speed multiplier;
    // not a walked field, assumed 1 (the captures run at normal speed).
    let _rate = rate_base;
    let _ = progress;
    effects.push(format!(
        "{tag}.Build.queue[{slot}] type {ty} progress {progress}: job_counter NOT advanced (construct_time 0x006508c0 untranscribed)"
    ));
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

    /// (image offset, byte) for every serialized Build byte.
    fn image(b: &Build) -> Vec<(usize, u8)> {
        let mut v = Vec::new();
        v.push((0x7f, b.head[0]));
        v.push((0x83, b.head[1]));
        v.extend(b.base.sub.body.iter().enumerate().map(|(i, &x)| (0x9 + i, x)));
        v.extend(b.base.mid.iter().enumerate().map(|(i, &x)| (0x20 + i, x)));
        v.extend(b.wall_body.iter().enumerate().map(|(i, &x)| (0x48 + i, x)));
        v.extend(b.body.iter().enumerate().map(|(i, &x)| (0x70 + i, x)));
        v.extend(b.queue.iter().enumerate().map(|(i, &x)| (0x1000 + i, x)));
        v.extend(b.mining.iter().enumerate().map(|(i, &x)| (0x2000 + i, x)));
        for (gi, g) in b.gathers.iter().enumerate() {
            v.extend(g.body.iter().enumerate().map(|(i, &x)| (0x3000 + gi * 16 + i, x)));
        }
        v.extend(b.orig_type.to_le_bytes().iter().enumerate().map(|(i, &x)| (0x6c + i, x)));
        v
    }

    /// Lane gate: run `build_process::run` on retail frame N and account
    /// every Build byte against retail N+1 — `introduced` (we changed, retail
    /// kept) must be 0 on every pair; `explained` must be > 0 overall.
    #[test]
    fn build_wall_burndown_introduces_nothing() {
        let Some(dir) = capture_dir() else {
            eprintln!("no capture dir; skipping");
            return;
        };
        let st = steps(&dir);
        let (mut explained, mut unexplained, mut introduced) = (0usize, 0usize, 0usize);
        let mut introduced_rows = Vec::new();
        // Known cross-lane ordering dependency: retail runs the Unit plane
        // first, and a builder's `Wall::do_construct` 0x006434d0 does
        // `helpers += 1` on its site before `Wall::process` resets helpers
        // and sets build_masks 0x400. Run in isolation on a site under
        // construction (flags & 4 == 0) we see helpers == 0 and clear 0x400.
        // Counted apart so the gate stays honest until Unit::process lands.
        let mut deferred_helpers_bit = 0usize;
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
            run(&mut ours, &mut effects);
            for owner in 0..a.objects.lists.len() {
                for slot in 0..a.objects.lists[owner].elems.len().min(b.objects.lists[owner].elems.len()) {
                    let (Some(Obj::Build(xa)), Some(Obj::Build(xb)), Some(Obj::Build(xo))) = (
                        &a.objects.lists[owner].elems[slot],
                        &b.objects.lists[owner].elems[slot],
                        &ours.objects.lists[owner].elems[slot],
                    ) else {
                        continue;
                    };
                    let (ia, ib, io) = (image(xa), image(xb), image(xo));
                    assert_eq!(ia.len(), io.len(), "[{owner}][{slot}] we changed the image length");
                    for i in 0..ia.len().min(ib.len()) {
                        let (ra, rb, ro) = (ia[i].1, ib[i].1, io[i].1);
                        if ra != rb {
                            if ro == rb {
                                explained += 1;
                            } else {
                                unexplained += 1;
                            }
                        } else if ro != ra {
                            let under_construction = xa.base.sub.flags & 4 == 0;
                            if under_construction && ia[i].0 == 0x61 && (ra ^ ro) == 0x04 && ro & 0x04 == 0 {
                                deferred_helpers_bit += 1;
                                continue;
                            }
                            introduced += 1;
                            introduced_rows.push(format!(
                                "f{}->f{} [{owner}][{slot}] +{:#x}: retail {ra:#04x} ours {ro:#04x}",
                                st[k].0,
                                st[k + 1].0,
                                ia[i].0
                            ));
                        }
                    }
                }
            }
        }
        eprintln!(
            "build/wall bytes: explained={explained} unexplained={unexplained} introduced={introduced} \
             (deferred to Unit plane: build_masks 0x400 helpers bit x{deferred_helpers_bit})"
        );
        for r in introduced_rows.iter().take(40) {
            eprintln!("  INTRODUCED {r}");
        }
        assert_eq!(introduced, 0, "introduced bytes");
        assert!(explained > 0, "no Build/Wall byte explained");
    }

    /// Diagnostic: dump every Build byte retail changed between consecutive
    /// captures, by image offset (`cargo test ... -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn dump_build_diffs() {
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
                    let (Some(Obj::Build(ba)), Some(Obj::Build(bb))) = (&la.elems[slot], &lb.elems[slot]) else { continue };
                    let (ia, ib) = (image(ba), image(bb));
                    let mut diffs: Vec<String> =
                        ia.iter().zip(ib.iter()).filter(|(x, y)| x.1 != y.1).map(|(x, y)| format!("+{:#x}:{:#04x}->{:#04x}", x.0, x.1, y.1)).collect();
                    if ia.len() != ib.len() {
                        diffs.push(format!("len {}->{}", ia.len(), ib.len()));
                    }
                    if diffs.is_empty() {
                        continue;
                    }
                    if ba.base.sub.body.len() < 19 {
                        println!("  [{owner}][{slot}] gate-off {}", diffs.join(" "));
                    } else {
                        let ptype = i32::from_le_bytes(ba.base.sub.body[15..19].try_into().unwrap());
                        println!("  [{owner}][{slot}] ptype={ptype} flags={:#x} {}", ba.base.sub.flags, diffs.join(" "));
                    }
                }
            }
        }
    }
}
