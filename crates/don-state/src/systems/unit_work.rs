//! Gather / construction work bodies reached from `Unit::do_job` 0x00617a10
//! (the order dispatcher at the end of `Unit::work` 0x0060d180):
//!
//! ```text
//!   case 7 (GatherOrder)  -> Unit::do_gather           0x005ef2a0  [do_gather]
//!                              -> Unit::do_non_flat_gather 0x005f0170  [do_non_flat_gather]
//!   case 6 (BuildOrder)   -> Unit::do_build            0x005eebf0  [do_build]
//!                              -> Wall::do_construct       0x006434d0  [do_construct]
//! ```
//!
//! Leaf module: it exposes free functions for the owning traversal
//! (`objects_process::unit_work`) to call and never edits sibling modules.
//! [`run_for_gatherers`] applies them to every unit whose head order is a
//! Gather/Build order, in the retail `Objects::process_all` 0x0065dce0
//! first-loop order, so the bodies can be measured on their own.
//!
//! VAs are from `rise.pdb` (`tools/pdb/lookup.py`); control flow from
//! `re/decomp-all/<EA>.c`; argument order / register use confirmed against a
//! Capstone listing of the mapped image (`riseofnations.exe`). Field offsets
//! from `re/scripts/pdb_layout.py GatherOrder BuildOrder UnitData WallData
//! BuildData`.
//!
//! ## Images
//!
//! ```text
//!   GatherOrder (52 B runtime, 31 B serialized after the i32 OrderIndex + metric):
//!     payload[0]      UnitOrder::flags
//!     +0x08 ox        payload[1..5]      +0x14 tx          payload[11..15]
//!     +0x0c whom      payload[5..9]      +0x18 ty          payload[15..19]
//!     +0x10 uid       payload[9..11]     +0x1c build_type  payload[19..23]
//!                                        +0x20 wait        payload[23..27]
//!     +0x24 goto_build [27]  +0x25 non_flat_gather [28]  +0x26 dist_mod [29]  +0x27 been_there [30]
//!   BuildOrder: flags, ox, whom, uid (11 B).
//!   Unit  : see objects_process.rs (`UImg`); Build/Wall: see build_process.rs (`Img`).
//!   FarmStruct (Farms.farm_data rows, 190 B of the 0xc0 runtime record):
//!     +0 who, +4 o, +8 f32 percent[16], +0x48 f32 height[25], +0xac u8 status[16],
//!     +0xbc valid, +0xbd farm_type; cell index = dx*4 + dy (TCoord offsets from
//!     `WallData::tile_corner` 0x00643440; a TCoord is 0xc0 coord units, i.e.
//!     `div_3_table[(coord ^ 0x63637) >> 6]`).
//! ```
//!
//! ## What executes
//!
//! * `Unit::do_gather`: target/ownership gates; the `been_there == 0`
//!   docking arm's `BuildData::is_gathered_by` 0x0062f520 /
//!   `num_gatherers` 0x00630450 (list walk) gates; the farm (0x1a1) work arm
//!   — `Farms::get_farm_type` 0x008d9160, `WallData::tile_corner`, the status
//!   switch with `Farms::grow` 0x008d91c0 / `Farms::snip` 0x008d9240, the
//!   rice-paddy every-256-frame random re-seat (`GameAccess::rnd` 0x0043cca0,
//!   two `game_random` draws) and the status-0/2 random re-seat (two draws);
//!   the woodcutter/mine (0x1a2/0x1a3) dispatch into `do_non_flat_gather`
//!   with its every-128-frame capacity check; the generic-site `+0x7a += 1 /
//!   build_masks |= 0x800` stamp and the `unit_masks & 0x40000` repair gate
//!   (`LeaderData::get_diff` 0x006ec000).
//! * `Unit::do_non_flat_gather`: the `been_there` site stamp, `group = -1`,
//!   `unit_masks &= 0x87ffffff`, the `been_there`/Leader `0x2000000` stamp,
//!   the chopping/mining clock (`wait -= 1`, `Build::all_gathering`
//!   0x0062f570 list walk, `wait = rnd % 100 + 300` / `rnd % 50 + 100`), the
//!   on-tile arrival (`vector_dist` 0x0046cff0 < 0x140, `find_angle`
//!   0x0092d130 → `Unit::set_angle` 0x00605400 → `Guy::set_angle`
//!   0x005d9010), and the carry-back arm's `wait -= 1` / `been_there` /
//!   angle writes once adjacency is known.
//! * `Unit::do_build`: target gates, anim/angle, the Constants+0x22c amount
//!   (quartered when the site is under attack and the tribe lacks bonus 0x10
//!   — `LeaderData::has_tribe_bonus` 0x006e1370 is read from the walked
//!   tribe-bonus bitmask), then `Wall::do_construct`.
//! * `Wall::do_construct`: `Build::start` 0x006273a0 → `Wall::start`
//!   0x0063e810 (`flags |= 2`, `frame_started = frame`), the per-frame
//!   `amount / (helpers + 1)`, the once-per-frame `+0x7a += 1 /
//!   build_masks |= 0x800` stamp, `helpers += 1`, `job_counter += amount`,
//!   `job_counter_2 += amount`.
//! * `Unit::set_anim` 0x00616f40 → `Guy::set_anim` 0x005da300: only the
//!   early-out is transcribed (same anim class and `cur_time < end_time`
//!   → no-op). Any real transition needs the `AnimationPacket` (`.bha`,
//!   not shipped here) and is reported as a TODO effect.
//!
//! ## What is not (fields untouched, noted in `effects`)
//!
//! `Unit::kill_current_order` 0x005e2cb0, `add_think_order` 0x005e3df0,
//! `add_move_order` 0x00616ed0, `add_repair_order` 0x005e4ff0,
//! `add_gather_order` 0x0061a5c0, `Build::add_gatherer` 0x0062f640,
//! `Build::check_gatherers` 0x0062f710, `UnitType::find_nearby_spot`
//! 0x0061de70, `WorldData::has_gather_access` 0x006b4e50,
//! `WorldData::get_tregion` 0x006b52e0, `Object::adjacent_to` 0x00651f40
//! (→ `ObjectData::attack_dist` 0x006488f0), `Doober::add_hold_doober`
//! 0x00846dd0 / `remove_hold_doobers` 0x00846cd0, `Unit::go_inside`
//! 0x0061a2e0, `Unit::build_done` 0x00603bf0, `Group::*`,
//! `BuildTypeData::blocked_site` 0x00636a50, `BuildData::construct_time`
//! 0x0062d5c0 (completion → `Build::activate` 0x00623e20), the World
//! footprint/seen stamps inside `Wall::start`, `ObjectData::count_inside`
//! 0x0064dda0 (oil platform / university gatherer counts).
//!
//! RNG (`game_random`, seed at `Save.post_world + 0x28`): every draw site is
//! listed at its call: `GameAccess::rnd` ×2 in the two farm re-seat arms,
//! `Random::get(0, 0xffff)` ×1 at each of the three `wait` re-rolls in
//! `do_non_flat_gather` (0x005f06b6, 0x005f0e2e, 0x005f0f18).

use crate::sections::{Build, Obj, Save, TypeRec, Unit};
use crate::tick::StepStatus;

pub const STATUS: StepStatus = StepStatus::Partial;

const FARM: i32 = 0x1a1;
const WOODCUTTER: i32 = 0x1a2;
const MINE: i32 = 0x1a3;
const UNIVERSITY: i32 = 0x1a4;
const OIL_PLATFORM: i32 = 0x1a6;

// ---------------------------------------------------------------------------
// Traversal (measurement entry point)
// ---------------------------------------------------------------------------

/// Apply `do_gather` / `do_build` to every active unit whose head order is a
/// GatherOrder (7) or BuildOrder (6), in the retail first-loop order of
/// `Objects::process_all` 0x0065dce0: owners `(frame + i) % 10` gated on
/// `LeaderData[owner].flags & 1`, slots `0..unit_mark[owner]`, `SubObject.flags
/// & 1`, and (from `Unit::process` 0x00610bc0) `inside_up < 0`.
pub fn run_for_gatherers(save: &mut Save, effects: &mut Vec<String>) {
    let fr = frame(save);
    for i in 0..10i32 {
        let owner = fr.wrapping_add(i).rem_euclid(10) as usize;
        if leader_flags(save, owner) & 1 == 0 {
            continue;
        }
        let Some(list) = save.objects.lists.get(owner) else { continue };
        let n = (unit_mark(save, owner).max(0) as usize).min(list.elems.len());
        for slot in 0..n {
            let ty = match save.objects.lists[owner].elems.get(slot) {
                Some(Some(Obj::Unit(u))) if u.base.sub.flags & 1 != 0 => {
                    if u.body.len() != 111 || i16::from_le_bytes([u.body[0x82 - 0x48], u.body[0x83 - 0x48]]) >= 0 {
                        continue;
                    }
                    u.orders.orders.first().map(|o| o.ty).unwrap_or(0)
                }
                _ => continue,
            };
            match ty {
                7 => do_gather(save, owner, slot, effects),
                6 => do_build(save, owner, slot, effects),
                _ => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Image accessors
// ---------------------------------------------------------------------------

/// Byte-image view of a serialized `Unit`, addressed by retail offset
/// (same layout as `objects_process::UImg`).
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
    fn who(&mut self) -> i32 {
        self.u8(0x09) as i32
    }
    fn o(&mut self) -> i32 {
        self.i16(0x0a) as i32
    }
    fn ptype(&mut self) -> i32 {
        self.i32(0x18)
    }
    fn x(&mut self) -> i32 {
        self.i32(0x10) ^ 0x63637
    }
    fn y(&mut self) -> i32 {
        self.i32(0x14) ^ 0x63637
    }
    fn unit_masks(&mut self) -> u32 {
        self.u32(0x68)
    }
    fn set_unit_masks(&mut self, m: u32, tag: &str, effects: &mut Vec<String>, why: &str) {
        let old = self.unit_masks();
        if old != m {
            self.set_u32(0x68, m);
            effects.push(format!("{tag}.Unit.unit_masks {old:#x} -> {m:#x} {why}"));
        }
    }
    /// `guys.list[0]->cur_anim` (+0x9c), `None` without a complete row.
    fn guy0_anim(&self) -> Option<i8> {
        guy_row_ref(self.0, 0).map(|g| g[0x9c - 8] as i8)
    }
}

fn guy_row_ref(u: &Unit, i: usize) -> Option<&Vec<u8>> {
    u.guys.elems.get(i).and_then(|g| g.as_ref()).map(|g| &g.data).filter(|d| d.len() == 155)
}

fn guy_row(u: &mut Unit, i: usize) -> Option<&mut Vec<u8>> {
    u.guys.elems.get_mut(i).and_then(|g| g.as_mut()).map(|g| &mut g.data).filter(|d| d.len() == 155)
}

/// GuyData row view (+0x08..+0xa3), retail offsets.
struct GImg<'a>(&'a mut Vec<u8>);

impl GImg<'_> {
    fn i32(&self, off: usize) -> i32 {
        i32::from_le_bytes(self.0[off - 8..off - 4].try_into().unwrap())
    }
    fn u32(&self, off: usize) -> u32 {
        self.i32(off) as u32
    }
    fn i8(&self, off: usize) -> i8 {
        self.0[off - 8] as i8
    }
    fn set_i32(&mut self, off: usize, x: i32) {
        self.0[off - 8..off - 4].copy_from_slice(&x.to_le_bytes());
    }
}

/// GatherOrder / BuildOrder payload view (see the module doc for the map).
struct OImg<'a>(&'a mut Vec<u8>);

impl OImg<'_> {
    fn idx(off: usize) -> usize {
        match off {
            0x08..=0x11 => off - 7,
            0x14..=0x27 => off - 9,
            _ => panic!("order offset {off:#x} not serialized"),
        }
    }
    fn i32(&self, off: usize) -> i32 {
        let i = Self::idx(off);
        i32::from_le_bytes(self.0[i..i + 4].try_into().unwrap())
    }
    fn u8(&self, off: usize) -> u8 {
        self.0[Self::idx(off)]
    }
    fn set_i32(&mut self, off: usize, x: i32) {
        let i = Self::idx(off);
        self.0[i..i + 4].copy_from_slice(&x.to_le_bytes());
    }
    fn set_u8(&mut self, off: usize, x: u8) {
        self.0[Self::idx(off)] = x;
    }
    fn ox(&self) -> i32 {
        self.i32(0x08)
    }
    fn whom(&self) -> i32 {
        self.i32(0x0c)
    }
}

/// Byte-image view of a serialized `Build` (same layout as `build_process::Img`).
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
    fn u32(&mut self, off: usize) -> u32 {
        self.i32(off) as u32
    }
    fn set_u8(&mut self, off: usize, x: u8) {
        let (v, i) = self.slot(off);
        v[i] = x;
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
    fn set_i32(&mut self, off: usize, x: i32) {
        let (v, i) = self.slot(off);
        v[i..i + 4].copy_from_slice(&x.to_le_bytes());
    }
    fn set_u32(&mut self, off: usize, x: u32) {
        self.set_i32(off, x as i32)
    }
    fn flags(&self) -> u8 {
        self.0.base.sub.flags
    }
    fn who(&mut self) -> i32 {
        self.u8(0x09) as i32
    }
    fn ptype(&mut self) -> i32 {
        self.i32(0x18)
    }
    fn x(&mut self) -> i32 {
        self.i32(0x10) ^ 0x63637
    }
    fn y(&mut self) -> i32 {
        self.i32(0x14) ^ 0x63637
    }
    fn build_masks(&mut self) -> u16 {
        self.u16(0x60)
    }
}

/// Read-only snapshot of the fields the work bodies consult on a site.
#[derive(Clone, Copy, Debug, Default)]
struct Site {
    present: bool,
    flags: u8,
    who: i32,
    o: i32,
    x: i32,
    y: i32,
    ptype: i32,
    damage: i32,
    /// +0x60
    build_masks: u16,
    /// +0x64
    helpers: u8,
    /// +0x70 gather_down (head of the gatherer list through `Unit.gather_down` +0x92)
    gather_down: i16,
    /// +0x72
    city: i16,
    /// +0x78 farm index
    farm: i16,
    /// +0x7a
    recharging: i16,
    /// +0x80
    gather_max: i8,
    /// ObjectType +0x234 / +0x238 (TCoord footprint)
    xs: i32,
    ys: i32,
}

fn site(save: &Save, who: i32, o: i32) -> Site {
    let Some(Some(Obj::Build(b))) = usize::try_from(who).ok().and_then(|w| save.objects.lists.get(w)).and_then(|l| usize::try_from(o).ok().map(|o| l.elems.get(o))).flatten() else {
        return Site::default();
    };
    if b.base.sub.body.len() != 19 || b.base.mid.len() != 34 || b.wall_body.len() != 30 || b.body.len() != 22 {
        return Site::default();
    }
    let sb = &b.base.sub.body;
    let i32at = |v: &[u8], i: usize| i32::from_le_bytes(v[i..i + 4].try_into().unwrap());
    let i16at = |v: &[u8], i: usize| i16::from_le_bytes([v[i], v[i + 1]]);
    let ptype = i32at(sb, 0x18 - 9);
    let (xs, ys) = type_footprint(save, ptype);
    Site {
        present: true,
        flags: b.base.sub.flags,
        who: sb[0] as i32,
        o: i16at(sb, 1) as i32,
        x: i32at(sb, 0x10 - 9) ^ 0x63637,
        y: i32at(sb, 0x14 - 9) ^ 0x63637,
        ptype,
        damage: i32at(&b.base.mid, 0x24 - 0x20),
        build_masks: i16at(&b.wall_body, 0x60 - 0x48) as u16,
        helpers: b.wall_body[0x64 - 0x48],
        gather_down: i16at(&b.body, 0),
        city: i16at(&b.body, 2),
        farm: i16at(&b.body, 8),
        recharging: i16at(&b.body, 0xa),
        gather_max: b.body[0x10] as i8,
        xs,
        ys,
    }
}

/// Mutate the site through its byte image.
fn with_site(save: &mut Save, who: i32, o: i32, f: impl FnOnce(&mut BImg)) {
    let (Ok(w), Ok(o)) = (usize::try_from(who), usize::try_from(o)) else { return };
    if let Some(Some(Obj::Build(b))) = save.objects.lists.get_mut(w).and_then(|l| l.elems.get_mut(o)) {
        let mut img = BImg(b);
        if img.complete() {
            f(&mut img);
        }
    }
}

/// `if ((site->build_masks & 0x800) == 0) { site->recharging += 1; site->build_masks |= 0x800; }`
/// — the once-per-frame gatherer/builder stamp shared by `Unit::do_gather`
/// (0x005efb5a), `Unit::do_non_flat_gather` (0x005f01a0) and
/// `Wall::do_construct` (0x006435f8). `Wall::process` 0x00640450 clears 0x800.
fn stamp_site(save: &mut Save, who: i32, o: i32, effects: &mut Vec<String>) {
    with_site(save, who, o, |s| {
        let bm = s.build_masks();
        if bm & 0x800 == 0 {
            let r = s.i16(0x7a);
            s.set_i16(0x7a, r.wrapping_add(1));
            s.set_u16(0x60, bm | 0x800);
            effects.push(format!(
                "Objects.lists[{who}][{o}].Build.recharging {r} -> {} ; Wall.build_masks {bm:#x} -> {:#x} (gather stamp)",
                r.wrapping_add(1),
                bm | 0x800
            ));
        }
    });
}

// ---------------------------------------------------------------------------
// Global-state readers
// ---------------------------------------------------------------------------

/// `Game+0x550` (`Game::frame`).
fn frame(save: &Save) -> i32 {
    i32::from_le_bytes(save.game.scalars[0..4].try_into().unwrap())
}

/// `Constants` i32 at image offset `off` (`[0x00c061f0]`, walked as `Save.constants`).
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

/// `LeaderData` i32 at image offset `off` (`flags` +0, `flags2` +4, body +8..).
fn leader_i32(save: &Save, who: i32, off: usize) -> Option<i32> {
    let l = save.leaders.slots.get(usize::try_from(who).ok()?)?;
    let b = |o: usize| -> Option<u8> {
        Some(match o {
            0..=3 => l.flags.to_le_bytes()[o],
            4..=7 => l.flags2.to_le_bytes()[o - 4],
            _ => *l.body.get(o - 8)?,
        })
    };
    Some(i32::from_le_bytes([b(off)?, b(off + 1)?, b(off + 2)?, b(off + 3)?]))
}

fn leader_u8(save: &Save, who: i32, off: usize) -> Option<u8> {
    let l = save.leaders.slots.get(usize::try_from(who).ok()?)?;
    Some(match off {
        0..=3 => l.flags.to_le_bytes()[off],
        4..=7 => l.flags2.to_le_bytes()[off - 4],
        _ => *l.body.get(off - 8)?,
    })
}

/// `LeaderData.flags |= bit` — the `0x2000000` "has gathered" stamp written
/// by the gather bodies (0x005ef9f7 etc.).
fn leader_flag_or(save: &mut Save, who: i32, bit: i32, effects: &mut Vec<String>) {
    let Some(l) = usize::try_from(who).ok().and_then(|w| save.leaders.slots.get_mut(w)) else { return };
    if l.flags & bit == 0 {
        let old = l.flags;
        l.flags |= bit;
        effects.push(format!("Leaders[{who}].flags {old:#x} -> {:#x}", l.flags));
    }
}

/// `Objects` scalar block: unit_mark at image 0x15c.. = scalars[16 + owner*4].
fn unit_mark(save: &Save, owner: usize) -> i32 {
    if owner >= 9 {
        return 0;
    }
    let o = 16 + owner * 4;
    i32::from_le_bytes(save.objects.scalars[o..o + 4].try_into().unwrap())
}

/// `Random::get(min, max)` 0x00a39d70 on `GameAccess::game_random`
/// `[0x00c06184]` (`Save.post_world + 0x28`).
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

/// `GameAccess::rnd(n)` 0x0043cca0: `n - 1 <= 0 ? 0 : Random::get(0, 0xffff) % n`.
fn game_rnd(save: &mut Save, n: i32) -> i32 {
    if n - 1 <= 0 {
        return 0;
    }
    game_random(save, 0, 0xffff) % n
}

/// `[0x00c061c0]` `GameAccess::ai_speed` — the game-speed multiplier; not
/// walked, assumed 1 (`build_process` makes the same assumption).
const AI_SPEED: i32 = 1;

/// Type record image reader (`Rules.types[idx]`), see `objects_process::TypeImg`.
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
    fn list(&self, strict: bool) -> impl Iterator<Item = u16> + '_ {
        let a = if strict { &self.0.arr1 } else { &self.0.arr0 };
        a.data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]))
    }
}

fn type_rec(save: &Save, idx: i32) -> Option<TypeImg<'_>> {
    usize::try_from(idx).ok().and_then(|i| save.rules_tail.rules.types.get(i)).map(TypeImg)
}

/// `ObjectType +0x234 / +0x238` — footprint in TCoords.
fn type_footprint(save: &Save, ptype: i32) -> (i32, i32) {
    match type_rec(save, ptype) {
        Some(t) => (t.i32(0x234).unwrap_or(0), t.i32(0x238).unwrap_or(0)),
        None => (0, 0),
    }
}

/// `ObjectType +0x218` movement plane (0 land, 1 sea, 2 air).
fn type_kind(save: &Save, ptype: i32) -> i32 {
    type_rec(save, ptype).and_then(|t| t.i32(0x218)).unwrap_or(0)
}

/// `ObjectType +0x2b4` unit flags.
fn type_flags(save: &Save, ptype: i32) -> u32 {
    type_rec(save, ptype).and_then(|t| t.i32(0x2b4)).unwrap_or(0) as u32
}

/// `ObjectTypeData::is_slow` 0x00661ae0 (`strict == 0` arm) — same body as
/// `build_process::type_is_slow` (private there).
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

/// `ObjectTypeData::is(what, strict)` 0x0065f7d0 — the devirtualised
/// `site->is(0x1a2, 0)` tests in the bodies (`*this == Build::vftable ?
/// ptype->vt[+0x60](what, 0) : this->vt[+0xb8](what, 0)`). Same body as
/// `build_process::type_is` (private there).
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
        if !(0x32..=0x19d).contains(&this) {
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

/// `LeaderData::has_tribe_bonus(bonus)` 0x006e1370 — reads the tribe's
/// bonus bitmask. Not transcribed (needs the Tribes image walk); `None`.
fn has_tribe_bonus(_save: &Save, _who: i32, _bonus: i32) -> Option<bool> {
    None
}

/// `LeaderData::get_diff()` 0x006ec000: `Game+0x820 & 4 ? leader+0x50 :
/// (Game+0x821 & 0x10 && !(Game+0x822 & 2) && leader+0x50 >= 0 ? leader+0x50
/// : Game+0x2b)`. `Game+0x2b` is `GameInfo.settings[7]` (Game+0x24 base).
fn get_diff(save: &Save, who: i32) -> i32 {
    let g = |off: usize| -> u8 {
        // Game image: scalars start at +0x550; the GameInfo settings block
        // sits at +0x24 (see build_process::game_setting). +0x820.. are the
        // Game option bytes walked right after the scalars block.
        if (0x24..0x24 + save.game.info.settings.len()).contains(&off) {
            return save.game.info.settings[off - 0x24];
        }
        0
    };
    // TODO(va 0x006ec000) Game+0x820..0x822 option bytes (difficulty override
    // flags) are not located in the walked image yet; the default arm
    // (Game+0x2b difficulty byte) is taken.
    let leader_diff = leader_i32(save, who, 0x50).unwrap_or(-1);
    let _ = leader_diff;
    g(0x2b) as i32
}

/// `vector_dist(dx, dy)` 0x0046cff0 — the octagonal distance used by every
/// work body: `a = max(|dx|,|dy|), b = min; b*b / (2a) + a` (or `(b + 2a) >> 1`
/// when `b >= 60000`), 0 when `a == 0`.
fn vector_dist(dx: i32, dy: i32) -> i32 {
    let a = dx.wrapping_abs() as u32;
    let b = dy.wrapping_abs() as u32;
    let (hi, lo) = if (b as i32) < (a as i32) { (a, b) } else { (b, a) };
    if hi == 0 {
        return 0;
    }
    if lo >= 60000 {
        return ((lo.wrapping_add(hi.wrapping_mul(2))) >> 1) as i32;
    }
    (lo.wrapping_mul(lo) / hi.wrapping_mul(2)).wrapping_add(hi) as i32
}

/// `find_angle(dx, dy)` 0x0092d130 — integer atan2 to a binary angle
/// (0 = north, 0x40000000 = east), transcribed from the listing.
fn find_angle(dx: i32, dy: i32) -> i32 {
    let ny = dy.wrapping_neg();
    if dx == 0 {
        return if ny > 0 { 0 } else { i32::MIN };
    }
    if ny == 0 {
        return if dx > 0 { 0x4000_0000 } else { 0xc000_0000u32 as i32 };
    }
    let adx = dx.wrapping_abs();
    let any = ny.wrapping_abs();
    let (num, den, horizontal) = if adx > any { (any << 14, adx, true) } else { (adx << 14, any, false) };
    let r = num / den;
    let err = (0x1333i32).wrapping_sub(r).wrapping_abs();
    let c = err.wrapping_mul(0xb00) >> 14;
    let base = ((0x2800i32).wrapping_sub(c).wrapping_mul(r) & 0xffff_c000u32 as i32) << 2;
    if dx > 0 {
        if ny > 0 {
            if horizontal {
                (0x4000_0000i32).wrapping_sub(base)
            } else {
                base
            }
        } else if horizontal {
            base.wrapping_add(0x4000_0000)
        } else {
            i32::MIN.wrapping_sub(base)
        }
    } else if ny > 0 {
        if horizontal {
            base.wrapping_add(0xc000_0000u32 as i32)
        } else {
            base.wrapping_neg()
        }
    } else if horizontal {
        (0xc000_0000u32 as i32).wrapping_sub(base)
    } else {
        base.wrapping_sub(i32::MIN)
    }
}

/// `div_3_table[c >> 6]` — coord to TCoord (0xc0 units; the table is
/// `init_coord_lookup_array` 0x00681db0's floor division by 3).
fn tcoord(c: i32) -> i32 {
    (c >> 6).div_euclid(3)
}

/// `WallData::tile_corner(&cx, &cy)` 0x00643440:
/// `sx = tcoord(x)*0xc0 (+0x60 if xs odd); cx = tcoord(sx) - (xs >> 1)`; same for y.
fn tile_corner(x: i32, y: i32, xs: i32, ys: i32) -> (i32, i32) {
    let mut sx = tcoord(x) * 0xc0;
    if xs & 1 != 0 {
        sx += 0x60;
    }
    let mut sy = tcoord(y) * 0xc0;
    if ys & 1 != 0 {
        sy += 0x60;
    }
    (tcoord(sx) - (xs >> 1), tcoord(sy) - (ys >> 1))
}

/// `WallData::covers_tile(tx, ty)` 0x006439b0.
fn covers_tile(s: &Site, tx: i32, ty: i32) -> bool {
    let (cx, cy) = tile_corner(s.x, s.y, s.xs, s.ys);
    tx >= cx && tx < cx + s.xs && ty >= cy && ty < cy + s.ys
}

/// `DAT_00af4370` UnitAnim → anim class (38 entries, read from the mapped
/// image; same table as `objects_inc_time::ANIM_CLASS`).
const ANIM_CLASS: [i32; 38] = [
    0, 0, 0, 0, 0, 0, 0, 8, 8, 8, 10, 12, 12, 12, 12, 15, 15, 15, 15, 15, 15, 21, 22, 23, 24, 25, 8, 27, 8, 29, 8, 31, 8, 33, 34, 35,
    36, 37,
];

fn anim_class(a: i32) -> Option<i32> {
    usize::try_from(a).ok().and_then(|i| ANIM_CLASS.get(i).copied())
}

// ---------------------------------------------------------------------------
// Gatherer list helpers (BuildData +0x70 gather_down → Unit +0x92 gather_down)
// ---------------------------------------------------------------------------

/// `Unit.gather_down` (+0x92) of `Objects.lists[who][o]`, with the unit under
/// work (taken out of its slot) supplied as `(self_o, self_gd)`.
fn unit_gather_down(save: &Save, who: i32, o: i32, this: (i32, i16)) -> i16 {
    if o == this.0 {
        return this.1;
    }
    let u = usize::try_from(who).ok().and_then(|w| save.objects.lists.get(w)).and_then(|l| usize::try_from(o).ok().and_then(|o| l.elems.get(o)));
    match u {
        Some(Some(Obj::Unit(u))) if u.body.len() == 111 => i16::from_le_bytes([u.body[0x92 - 0x48], u.body[0x93 - 0x48]]),
        _ => -1,
    }
}

/// `BuildData::is_gathered_by(o)` 0x0062f520: walk the site's gatherer list.
fn is_gathered_by(save: &Save, s: &Site, o: i32, this: (i32, i16)) -> bool {
    let mut i = s.gather_down as i32;
    let mut guard = 0;
    while i >= 0 && guard < 4096 {
        if i == o {
            return true;
        }
        i = unit_gather_down(save, s.who, i, this) as i32;
        guard += 1;
    }
    false
}

/// `Unit::is_gathering_at(o, who, p3)` `FUN_00608880`: the unit's current
/// order is a GatherOrder on `(who, o)`; with `p3 != 0` returns its
/// `been_there`. Scholars (0x34/0x35) inside the building also count.
fn is_gathering_at(save: &Save, who: i32, idx: i32, this: &Unit, this_o: i32, site_o: i32, site_who: i32, p3: i32) -> bool {
    let u: &Unit = if idx == this_o {
        this
    } else {
        match usize::try_from(who).ok().and_then(|w| save.objects.lists.get(w)).and_then(|l| usize::try_from(idx).ok().and_then(|o| l.elems.get(o))) {
            Some(Some(Obj::Unit(u))) => u,
            _ => return false,
        }
    };
    if u.body.len() != 111 || u.base.sub.body.len() != 19 {
        return false;
    }
    let ptype = i32::from_le_bytes(u.base.sub.body[15..19].try_into().unwrap());
    let inside_up = i16::from_le_bytes([u.body[0x82 - 0x48], u.body[0x83 - 0x48]]);
    let on_map = inside_up < 0; // UnitData::is_on_map 0x0046ce30: inside_up >> 15
    if (ptype == 0x34 || ptype == 0x35) && !on_map {
        // TODO(va 0x00651a80) host building lookup for scholars inside a
        // university: `(o, who) == host` -> 1; else p3 != 0 -> 0.
        if p3 != 0 {
            return false;
        }
    }
    if on_map && (0x32..=0x35).contains(&ptype) {
        if let Some(o0) = u.orders.orders.first() {
            if o0.ty == 7 && o0.payload.len() == 31 {
                let g = OImg(&mut o0.payload.clone());
                if g.whom() == site_who && g.ox() == site_o {
                    return if p3 == 0 { true } else { g.u8(0x27) != 0 };
                }
            }
        }
    }
    false
}

/// `BuildData::num_gatherers(p1, p2)` 0x00630450 — list walk; the
/// `count_inside` 0x0064dda0 contribution for oil platforms / universities is
/// not transcribed (`None` when it would apply).
fn num_gatherers(save: &Save, s: &Site, p1: i32, p2: i32, this: &Unit, this_o: i32, this_gd: i16) -> Option<i32> {
    if type_is(save, s.ptype, OIL_PLATFORM, false) || type_is(save, s.ptype, UNIVERSITY, false) {
        return None;
    }
    let mut n = 0;
    let mut i = s.gather_down as i32;
    let mut guard = 0;
    while i >= 0 && guard < 4096 {
        let um = if i == this_o {
            u32::from_le_bytes(this.body[0x68 - 0x48..0x6c - 0x48].try_into().unwrap())
        } else {
            match usize::try_from(s.who).ok().and_then(|w| save.objects.lists.get(w)).and_then(|l| usize::try_from(i).ok().and_then(|o| l.elems.get(o))) {
                Some(Some(Obj::Unit(u))) if u.body.len() == 111 => u32::from_le_bytes(u.body[0x68 - 0x48..0x6c - 0x48].try_into().unwrap()),
                _ => 0,
            }
        };
        if (p2 == 0 || um & 1 == 0) && is_gathering_at(save, s.who, i, this, this_o, s.o, s.who, p1) {
            n += 1;
        }
        i = unit_gather_down(save, s.who, i, (this_o, this_gd)) as i32;
        guard += 1;
    }
    Some(n)
}

/// `Build::all_gathering()` 0x0062f570: `check_gatherers()` (TODO, prunes the
/// list) then every gatherer's current order must be a GatherOrder with
/// `goto_build == 0` and `wait >= 0`.
fn all_gathering(save: &Save, s: &Site, this: &Unit, this_o: i32, this_gd: i16, effects: &mut Vec<String>) -> bool {
    effects.push(format!("Objects.lists[{}][{}]: Build::check_gatherers 0x0062f710 not transcribed (list assumed consistent)", s.who, s.o));
    let mut i = s.gather_down as i32;
    let mut guard = 0;
    while i >= 0 && guard < 4096 {
        let u: &Unit = if i == this_o {
            this
        } else {
            match usize::try_from(s.who).ok().and_then(|w| save.objects.lists.get(w)).and_then(|l| usize::try_from(i).ok().and_then(|o| l.elems.get(o))) {
                Some(Some(Obj::Unit(u))) => u,
                _ => return false,
            }
        };
        let Some(o0) = u.orders.orders.first() else { return false };
        if o0.ty != 7 || o0.payload.len() != 31 {
            return false;
        }
        let g = OImg(&mut o0.payload.clone());
        if g.u8(0x24) != 0 {
            return false;
        }
        if g.i32(0x20) < 0 {
            return false;
        }
        i = unit_gather_down(save, s.who, i, (this_o, this_gd)) as i32;
        guard += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// Unit::set_anim 0x00616f40 / Unit::set_angle 0x00605400
// ---------------------------------------------------------------------------

/// `Unit::set_anim(anim, p2, p3)` → `Guy::set_anim` 0x005da300 on
/// `guys[0..guy_mark]` and the attachments. Only the `p2 == 0` early-out is
/// transcribed: `class(cur_anim) == class(anim) && (class != 8 || type ==
/// 0x192) && cur_time < end_time` is a no-op. Everything else needs the
/// `AnimationPacket` and is reported.
fn set_anim(u: &mut Unit, anim: i32, p2: i32, tag: &str, effects: &mut Vec<String>) {
    let ptype = i32::from_le_bytes(u.base.sub.body[15..19].try_into().unwrap());
    let Some(row) = guy_row(u, 0) else { return };
    let g = GImg(row);
    let cur = g.i8(0x9c) as i32;
    let (ct, et) = (g.u32(0x74), g.u32(0x78));
    let noop = p2 == 0
        && anim_class(cur).is_some()
        && anim_class(cur) == anim_class(anim)
        && (anim_class(cur) != Some(8) || ptype == 0x192)
        && ct < et;
    if noop {
        return;
    }
    effects.push(format!("{tag}: Unit::set_anim({anim:#x}) 0x00616f40 — Guy::set_anim 0x005da300 transition {cur:#x} -> {anim:#x} needs the AnimationPacket; guys untouched"));
}

/// `Unit::set_angle(angle, _, snap)` 0x00605400: when the turn is in
/// `[0x40000000, 0xc0000000]` (more than a quarter turn) `unit_masks ^= 2`
/// and, for grouped units, the Group's +0x48 toggle (Groups image — TODO);
/// then `angle = a` and `guys[0]->set_angle(a, snap)` (`Guy::set_angle`
/// 0x005d9010: `des_angle = a`; with `snap` also `angle`/`last_angle`; a
/// primary guy forwards `des_angle` and its own x/y as `des_x/des_y` to the
/// attachment rows `num_guys..`).
fn set_angle(save: &Save, img: &mut UImg, a: i32, snap: bool, tag: &str, effects: &mut Vec<String>) {
    let cur = img.i32(0x50);
    let d = (a as u32).wrapping_sub(cur as u32);
    if (0x4000_0000..=0xc000_0000).contains(&d) {
        let um = img.unit_masks();
        img.set_unit_masks(um ^ 2, tag, effects, "(set_angle big turn)");
        if img.i16(0x80) >= 0 {
            // TODO(va 0x00605433) GroupData::find_leader 0x0070ccb0 == o ->
            // Groups[who][group] +0x48 toggle (Groups image, not ours).
            effects.push(format!("{tag}: set_angle group +0x48 toggle 0x00605433 not transcribed"));
        }
    }
    if cur != a {
        img.set_i32(0x50, a);
        effects.push(format!("{tag}.Unit.angle {cur:#x} -> {a:#x}"));
    }
    let ptype = img.ptype();
    let num_guys = type_rec(save, ptype).and_then(|t| t.i32(0x304)).unwrap_or(i32::MAX);
    let len = img.0.guys.elems.len();
    let Some(row) = guy_row(img.0, 0) else { return };
    let mut g = GImg(row);
    let (gx, gy) = (g.i32(0x0c), g.i32(0x10));
    let is_primary = g.i8(0xa2) == 0;
    if g.i32(0x64) != a {
        g.set_i32(0x64, a);
        effects.push(format!("{tag}.guys[0].des_angle -> {a:#x}"));
    }
    if snap {
        g.set_i32(0x18, a);
        g.set_i32(0x1c, a);
        effects.push(format!("{tag}.guys[0].angle/last_angle -> {a:#x} (snap)"));
    }
    if is_primary {
        for ei in (num_guys.max(0) as usize)..len {
            let Some(erow) = guy_row(img.0, ei) else { continue };
            let mut e = GImg(erow);
            e.set_i32(0x64, a);
            if e.i32(0x54) != 0 || e.i32(0x58) != 0 {
                // TODO(va 0x005d90b0) track-offset rotation (FUN_00a46a00 trig)
                // and the map-edge clamp; des_x/des_y left untouched.
                continue;
            }
            e.set_i32(0x5c, gx);
            e.set_i32(0x60, gy);
            if snap {
                // TODO(va 0x005d91d0) recursive set_angle(…,1) + Guy::set_new_location 0x005d86f0.
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Unit::do_gather 0x005ef2a0
// ---------------------------------------------------------------------------

/// `Unit::do_gather(order)` for `Objects.lists[owner][slot]` whose head order
/// is the GatherOrder.
pub fn do_gather(save: &mut Save, owner: usize, slot: usize, effects: &mut Vec<String>) {
    let taken = save.objects.lists[owner].elems[slot].take();
    let Some(Obj::Unit(mut u)) = taken else {
        save.objects.lists[owner].elems[slot] = taken;
        return;
    };
    do_gather_body(save, &mut u, owner, slot, effects);
    save.objects.lists[owner].elems[slot] = Some(Obj::Unit(u));
}

fn do_gather_body(save: &mut Save, u: &mut Unit, owner: usize, slot: usize, effects: &mut Vec<String>) {
    let tag = format!("Objects.lists[{owner}][{slot}]");
    if !UImg(u).complete() || u.orders.orders.first().map(|o| o.ty != 7 || o.payload.len() != 31).unwrap_or(true) {
        return;
    }
    let fr = frame(save);
    let (who, o, ux, uy, ptype, this_gd) = {
        let mut img = UImg(u);
        (img.who(), img.o(), img.x(), img.y(), img.ptype(), img.i16(0x92))
    };
    let (t_o, t_who, been, btype) = {
        let g = OImg(&mut u.orders.orders[0].payload);
        (g.ox(), g.whom(), g.u8(0x27), g.i32(0x1c))
    };

    // 0x005ef2de: if (order->whom != who) -> LAB_005ef9d6 kill_current_order(0)
    if t_who != who {
        effects.push(format!("{tag}: GatherOrder whom {t_who} != who {who} -> Unit::kill_current_order 0x005e2cb0 not transcribed"));
        return;
    }
    let s = site(save, t_who, t_o);
    // 0x005ef303..: site->is_valid_build() (flags & 1) && site->is_active() (flags & 4)
    if !s.present || s.flags & 1 == 0 || s.flags & 4 == 0 {
        effects.push(format!("{tag}: gather target [{t_who}][{t_o}] not a live active Build -> kill_current_order 0x005e2cb0 not transcribed"));
        return;
    }
    let kind = type_kind(save, ptype);

    // 0x005ef32e: if (!been_there) { ... docking arm ... }
    if been == 0 {
        if !is_gathered_by(save, &s, o, (o, this_gd)) {
            let gmax = s.gather_max as i32;
            let Some(n) = num_gatherers(save, &s, 0, 0, u, o, this_gd) else {
                effects.push(format!("{tag}: num_gatherers 0x00630450 needs count_inside 0x0064dda0 (oil platform / university); stopped"));
                return;
            };
            if n < gmax {
                if !(btype == OIL_PLATFORM || kind != 0) {
                    // Build::add_gatherer(o, ...) 0x0062f640 links this unit into the
                    // site's list (writes site gather_down and Unit.gather_down).
                    effects.push(format!("{tag}: Build::add_gatherer 0x0062f640 onto [{t_who}][{t_o}] not transcribed; stopped"));
                    return;
                }
                // -> LAB_005ef4f5 (falls into the arrived arm below)
            } else {
                // Site full: kill_current_order(0); add_think_order(); then
                // LAB_005ef3b1: if site->ptype->vt[+0x94]() && !covers_tile(unit tile)
                // -> add_move_order(site.xy).
                effects.push(format!("{tag}: site [{t_who}][{t_o}] full ({n} >= {gmax}) -> kill_current_order / add_think_order 0x005e3df0 / add_move_order 0x00616ed0 not transcribed"));
                return;
            }
        }
    }

    // LAB_005ef4f5
    let mut farm_idx = -1i32;
    if btype == FARM {
        if s.city < 0 {
            effects.push(format!("{tag}: farm [{t_who}][{t_o}] has no city -> kill/think/move 0x005ef4dc not transcribed"));
            return;
        }
        farm_idx = s.farm as i32;
    }
    if btype == MINE || btype == WOODCUTTER {
        // 0x005ef5xx: if (((frame + o*4) & 0x7f) != 0) -> do_non_flat_gather
        if (fr.wrapping_add(o.wrapping_mul(4))) & 0x7f != 0 {
            do_non_flat_gather(save, u, owner, slot, effects);
            return;
        }
        let gmax = s.gather_max as i32;
        let Some(n) = num_gatherers(save, &s, 0, 0, u, o, this_gd) else {
            effects.push(format!("{tag}: num_gatherers 0x00630450 blocked (count_inside); stopped"));
            return;
        };
        if n <= gmax {
            do_non_flat_gather(save, u, owner, slot, effects);
            return;
        }
        effects.push(format!("{tag}: camp [{t_who}][{t_o}] over capacity ({n} > {gmax}) at the 128-frame check -> kill_current_order / add_think_order / add_move_order not transcribed"));
        return;
    }

    let arrived: bool;
    if btype != FARM {
        // Generic gather site (oil platform, university, ...): adjacency test.
        // TODO(va 0x00651f40) Object::adjacent_to(o, whom) -> ObjectData::attack_dist 0x006488f0.
        effects.push(format!("{tag}: Object::adjacent_to 0x00651f40 for site [{t_who}][{t_o}] (type {btype:#x}) not transcribed; stopped"));
        return;
    } else {
        // 0x005ef9xx: farm: covers_tile(unit tile)?
        arrived = covers_tile(&s, tcoord(ux), tcoord(uy));
        if !arrived {
            // Off the farm: region test (WorldData::get_tregion 0x006b52e0),
            // capacity test, then add_move_order(site.xy).
            effects.push(format!("{tag}: farmer off farm [{t_who}][{t_o}] -> get_tregion 0x006b52e0 / add_move_order 0x00616ed0 not transcribed; stopped"));
            return;
        }
    }

    // LAB_005ef939
    if been == 0 {
        let gmax = s.gather_max as i32;
        let Some(n) = num_gatherers(save, &s, 0, 0, u, o, this_gd) else { return };
        if gmax < n {
            // LAB_005ef63e: kill_current_order(0); flags |= 0x10
            effects.push(format!("{tag}: farm over capacity on arrival -> kill_current_order (not transcribed); SubObject.flags |= 0x10 withheld"));
            return;
        }
        OImg(&mut u.orders.orders[0].payload).set_u8(0x27, 1);
        effects.push(format!("{tag}.GatherOrder.been_there 0 -> 1"));
        leader_flag_or(save, who, 0x2000000, effects);
        let is_scholar = ptype == 0x34 || ptype == 0x35;
        if !is_scholar {
            if !type_is(save, s.ptype, OIL_PLATFORM, false) {
                effects.push(format!("{tag}: Build::check_gatherers 0x0062f710 on arrival not transcribed"));
                // -> LAB_005efb20
            } else {
                effects.push(format!("{tag}: oil-platform boarding (can_carry 0x00646c40 / go_inside 0x0061a2e0 / die) not transcribed; stopped"));
                return;
            }
        } else {
            effects.push(format!("{tag}: scholar go_inside 0x0061a2e0 / check_gatherers / kill_current_order not transcribed; stopped"));
            return;
        }
    }

    // LAB_005efb20
    let um = UImg(u).unit_masks();
    if um & 0x40000 != 0 {
        // ((o + frame + who) & 0x800000ff) == 0  (signed % 256 == 0)
        let k = o.wrapping_add(fr).wrapping_add(who) & 0xff;
        if k == 0 && get_diff(save, who) > 1 && s.damage != 0 {
            let under_attack = s.build_masks & 0x20 != 0; // WallData::is_under_attack 0x00472420
            if !under_attack && s.city >= 0 {
                // TODO(va 0x005efc54) Cities[who][city] +4 & 2 gate, then
                // Unit::add_repair_order(o, whom, 2) 0x005e4ff0.
                effects.push(format!("{tag}: damaged site repair gate (Cities +4 & 2, add_repair_order 0x005e4ff0) not transcribed; stopped"));
                return;
            }
        }
    }
    if btype != FARM {
        stamp_site(save, t_who, t_o, effects);
        if UImg(u).guy0_anim() == Some(0x25) {
            return;
        }
        set_anim(u, 0x25, 0, &tag, effects);
        let mut img = UImg(u);
        set_angle(save, &mut img, 0x4000_0000, false, &tag, effects);
        // Unit::set_new_location(site.x + 0x120, site.y, 1) 0x005f8d20
        effects.push(format!("{tag}: Unit::set_new_location 0x005f8d20 ({}, {}) not transcribed", s.x + 0x120, s.y));
        return;
    }

    // Farm work: Farms::get_farm_type(farm) 0x008d9160 = rec[+0xbd] & 1
    let Some(rec) = usize::try_from(farm_idx).ok().and_then(|i| save.farms.farm_data.elems.get(i)).map(|r| r.data.clone()) else {
        effects.push(format!("{tag}: farm index {farm_idx} out of range"));
        return;
    };
    let ft = rec.get(0xbd).map(|b| b & 1).unwrap_or(0);
    let (cx, cy) = tile_corner(s.x, s.y, s.xs, s.ys);
    if ft == 1 {
        set_anim(u, 0x23, 0, &tag, effects);
        // ((o*7 + frame + who) & 0x800000ff) != 0 -> return  (signed % 256)
        let k = o.wrapping_mul(7).wrapping_add(fr).wrapping_add(who) & 0xff;
        if k != 0 {
            return;
        }
        let rx = game_rnd(save, s.xs / 2);
        let ry = game_rnd(save, s.ys / 2);
        let (mx, my) = ((cx + 1 + rx) * 0xc0 + 0x60, (cy + 1 + ry) * 0xc0 + 0x60);
        effects.push(format!("{tag}: rice-paddy re-seat: GameAccess::rnd x2 -> add_move_order({mx},{my}) 0x00616ed0 not transcribed"));
        return;
    }
    let (mut dx, mut dy) = (tcoord(ux) - cx, tcoord(uy) - cy);
    if dy < 0 || dx < 0 || dy >= s.ys || dx >= s.xs {
        dx = 1;
        dy = 1;
    }
    let cell = (dx * 4 + dy) as usize;
    let status = rec.get(0xac + cell).copied().unwrap_or(0);
    let anim0 = UImg(u).guy0_anim().unwrap_or(0) as u8;
    let grow = |save: &mut Save, u: &mut Unit, effects: &mut Vec<String>| {
        set_anim(u, 0x23, 0, &tag, effects);
        farms_grow(save, farm_idx, dy, dx, &tag, effects);
    };
    match status {
        0 => {
            if anim0 != 0x24 {
                grow(save, u, effects);
                return;
            }
        }
        1 => {
            grow(save, u, effects);
            return;
        }
        2 => {
            if anim0 != 0x23 {
                set_anim(u, 0x24, 0, &tag, effects);
                farms_snip(save, farm_idx, dy, dx, &tag, effects);
                return;
            }
        }
        3 => {
            set_anim(u, 0x24, 0, &tag, effects);
            return;
        }
        _ => {}
    }
    // status 0 while reaping / status 2 while sowing: pick another cell.
    let rx = game_rnd(save, s.xs);
    let ry = game_rnd(save, s.ys);
    let (mx, my) = ((cx + rx) * 0xc0 + 0x60, (cy + ry) * 0xc0 + 0x60);
    effects.push(format!("{tag}: farm cell ({dx},{dy}) status {status} with anim {anim0:#x}: GameAccess::rnd x2 -> add_move_order({mx},{my}) 0x00616ed0 not transcribed"));
}

/// `Farms::grow(farm, p2, p3)` 0x008d91c0: `status[p3*4+p2] = 1; percent +=
/// 0.005 (double add, float store); if > 1.0 { percent = 1.0; status = 2 }`.
fn farms_grow(save: &mut Save, farm: i32, p2: i32, p3: i32, tag: &str, effects: &mut Vec<String>) {
    if !(p2 < 4 && p3 < 4) {
        return;
    }
    let Some(rec) = usize::try_from(farm).ok().and_then(|i| save.farms.farm_data.elems.get_mut(i)) else { return };
    let cell = (p3 * 4 + p2) as usize;
    let rec = &mut rec.data;
    let st0 = rec[0xac + cell];
    rec[0xac + cell] = 1;
    let po = 8 + cell * 4;
    let f0 = f32::from_le_bytes(rec[po..po + 4].try_into().unwrap());
    let mut f = (f0 as f64 + 0.005) as f32;
    if 1.0f64 < f as f64 {
        f = 1.0;
        rec[0xac + cell] = 2;
    }
    rec[po..po + 4].copy_from_slice(&f.to_le_bytes());
    effects.push(format!("{tag}: Farms[{farm}].percent[{cell}] {f0} -> {f}, status {st0} -> {}", rec[0xac + cell]));
}

/// `Farms::snip(farm, p2, p3)` 0x008d9240: `if status[p3*4+p2] == 2 { status = 3 }`.
fn farms_snip(save: &mut Save, farm: i32, p2: i32, p3: i32, tag: &str, effects: &mut Vec<String>) {
    if !(p2 < 4 && p3 < 4) {
        return;
    }
    let Some(rec) = usize::try_from(farm).ok().and_then(|i| save.farms.farm_data.elems.get_mut(i)) else { return };
    let cell = (p3 * 4 + p2) as usize;
    if rec.data[0xac + cell] == 2 {
        rec.data[0xac + cell] = 3;
        effects.push(format!("{tag}: Farms[{farm}].status[{cell}] 2 -> 3 (snip)"));
    }
}

// ---------------------------------------------------------------------------
// Unit::do_non_flat_gather 0x005f0170
// ---------------------------------------------------------------------------

/// Woodcutter / mine work body. `u` is the unit (taken out of its slot), its
/// head order the GatherOrder.
fn do_non_flat_gather(save: &mut Save, u: &mut Unit, owner: usize, slot: usize, effects: &mut Vec<String>) {
    let tag = format!("Objects.lists[{owner}][{slot}]");
    let (who, o, ux, uy, ptype, this_gd) = {
        let mut img = UImg(u);
        (img.who(), img.o(), img.x(), img.y(), img.ptype(), img.i16(0x92))
    };
    let (t_o, t_who, mut wait, goto_build, tx, ty, been) = {
        let g = OImg(&mut u.orders.orders[0].payload);
        (g.ox(), g.whom(), g.i32(0x20), g.u8(0x24), g.i32(0x14), g.i32(0x18), g.u8(0x27))
    };
    let s = site(save, t_who, t_o);
    let is_camp = type_is(save, s.ptype, WOODCUTTER, false);

    // 0x005f0188: if (been_there) stamp the site
    if been != 0 {
        stamp_site(save, t_who, t_o, effects);
    }
    // 0x005f01d4: group = -1
    {
        let mut img = UImg(u);
        if img.i16(0x80) != -1 {
            let g = img.i16(0x80);
            img.set_i16(0x80, -1);
            effects.push(format!("{tag}.Unit.group {g} -> -1"));
        }
    }
    let set_been = |save: &mut Save, u: &mut Unit, effects: &mut Vec<String>| {
        if OImg(&mut u.orders.orders[0].payload).u8(0x27) == 0 {
            OImg(&mut u.orders.orders[0].payload).set_u8(0x27, 1);
            effects.push(format!("{tag}.GatherOrder.been_there 0 -> 1"));
            leader_flag_or(save, who, 0x2000000, effects);
        }
    };
    let set_wait = |u: &mut Unit, w: i32, effects: &mut Vec<String>, why: &str| {
        let old = OImg(&mut u.orders.orders[0].payload).i32(0x20);
        if old != w {
            OImg(&mut u.orders.orders[0].payload).set_i32(0x20, w);
            effects.push(format!("{tag}.GatherOrder.wait {old} -> {w} {why}"));
        }
    };

    if goto_build == 0 {
        // 0x005f01e4: unit_masks &= 0x87ffffff
        {
            let mut img = UImg(u);
            let um = img.unit_masks();
            img.set_unit_masks(um & 0x87ff_ffff, &tag, effects, "(&= 0x87ffffff gathering)");
        }
        set_been(save, u, effects);
        if wait < 0 {
            // 0x005f0220..: no tile yet — find_nearby_spot around the camp.
            // TODO(va 0x0061de70) UnitType::find_nearby_spot; on failure
            // set_anim(0x19,0,1), wait = 0x14; on success set_anim(0,0,1),
            // add_move_order(spot), goto_build = 1, wait = 0x20, unit_masks |=
            // is(0x1a2) ? 0x8000000 : 0x20000000.
            effects.push(format!("{tag}: do_non_flat_gather wait<0 spot search (find_nearby_spot 0x0061de70) not transcribed; stopped"));
            return;
        }
        let anim0 = UImg(u).guy0_anim().unwrap_or(0) as u8;
        if anim0 == 0x1d {
            return;
        }
        if anim0 == 0x19 {
            wait -= 1;
            set_wait(u, wait, effects, "(chopping)");
            if wait != 0 {
                return;
            }
            if all_gathering(save, &s, u, o, this_gd, effects) {
                set_wait(u, -1, effects, "(all gathering)");
                return;
            }
            // 0x005f06b6: Random::get(0, 0xffff)
            let r = game_random(save, 0, 0xffff);
            set_wait(u, r % 100 + 300, effects, "(rnd % 100 + 300)");
            return;
        }
        // Heading to / working the tile (tx, ty).
        let (gx, gy) = (tx * 0xc0 + 0x60, ty * 0xc0 + 0x60);
        let d = vector_dist(gx - ux, gy - uy);
        if d < 0x140 {
            wait -= 1;
            set_wait(u, wait, effects, "(on tile)");
            if wait == 0 {
                if all_gathering(save, &s, u, o, this_gd, effects) {
                    // LAB_005f0ef1
                    set_wait(u, -1, effects, "(all gathering)");
                    return;
                }
                // 0x005f0f18: Random::get(0, 0xffff)
                let r = game_random(save, 0, 0xffff);
                set_wait(u, r % 50 + 100, effects, "(rnd % 50 + 100)");
            }
            let a = find_angle(gx - ux, gy - uy);
            if a != UImg(u).i32(0x50) {
                let mut img = UImg(u);
                set_angle(save, &mut img, a, false, &tag, effects);
            }
            if UImg(u).i16(0x86) < 0 {
                // TODO(va 0x00846dd0) Doober::add_hold_doober(...) -> Unit.doober (+0x86)
                // (Doober image allocation; the index is not derivable here).
                effects.push(format!("{tag}: Doober::add_hold_doober 0x00846dd0 -> Unit.doober (+0x86) not transcribed"));
            }
            set_anim(u, if is_camp { 0x19 } else { 0x1d }, 0, &tag, effects);
            return;
        }
        // Far from the tile: set_anim(0,0,1); find_nearby_spot(tile, 0xc0, 0x100, 2, ...)
        set_anim(u, 0, 0, &tag, effects);
        effects.push(format!("{tag}: walk to tile ({tx},{ty}) d={d}: find_nearby_spot 0x0061de70 / add_move_order / is_gathered_by reset not transcribed; stopped"));
        return;
    }

    // goto_build != 0
    if wait >= 0 {
        // TODO(va 0x00651f40) Object::adjacent_to(o, whom) -> attack_dist 0x006488f0.
        // Adjacent: wait -= 1; been_there = 1 (+ Leader 0x2000000); if wait < 0
        // return; set_angle(find_angle(site - unit)); set_anim(is(0x1a2) ? 0x1b : 0x1f).
        // Not adjacent: is_gathered_by / get_tregion / find_nearby_spot /
        // add_move_order; wait = -1 fallback.
        effects.push(format!("{tag}: carry-back (goto_build, wait={wait}) needs Object::adjacent_to 0x00651f40; stopped"));
        return;
    }
    // wait < 0: choose a tile
    set_been(save, u, effects);
    set_anim(u, 0, 0, &tag, effects);
    // TODO(va 0x006b4e50) WorldData::has_gather_access(&tx,&ty, who, 1, 0) and the
    // gather_from (+0x98 Array<TCoordData>) scan (0x005f02b2), then
    // Random::get(0,0xffff) % 200 + 400 -> wait, goto_build = 0, unit_masks |=
    // is(0x1a2) ? 0x10000000 : 0x40000000 (wait = 1000000).
    effects.push(format!("{tag}: tile selection (has_gather_access 0x006b4e50, gather_from scan 0x005f02b2) not transcribed; the 0x005f0e2e rnd draw withheld; stopped"));
}

// ---------------------------------------------------------------------------
// Unit::do_build 0x005eebf0
// ---------------------------------------------------------------------------

/// `Unit::do_build(order)` for `Objects.lists[owner][slot]` whose head order
/// is the BuildOrder.
pub fn do_build(save: &mut Save, owner: usize, slot: usize, effects: &mut Vec<String>) {
    let taken = save.objects.lists[owner].elems[slot].take();
    let Some(Obj::Unit(mut u)) = taken else {
        save.objects.lists[owner].elems[slot] = taken;
        return;
    };
    do_build_body(save, &mut u, owner, slot, effects);
    save.objects.lists[owner].elems[slot] = Some(Obj::Unit(u));
}

fn do_build_body(save: &mut Save, u: &mut Unit, owner: usize, slot: usize, effects: &mut Vec<String>) {
    let tag = format!("Objects.lists[{owner}][{slot}]");
    if !UImg(u).complete() || u.orders.orders.first().map(|o| o.ty != 6 || o.payload.len() != 11).unwrap_or(true) {
        return;
    }
    let (who, ux, uy) = {
        let mut img = UImg(u);
        (img.who(), img.x(), img.y())
    };
    let (t_o, t_who) = {
        let g = OImg(&mut u.orders.orders[0].payload);
        (g.ox(), g.whom())
    };
    // 0x005eec0e: o < 0 || who < 0 || !site->is_valid_build() -> kill + build_done
    let s = site(save, t_who, t_o);
    if t_o < 0 || t_who < 0 || !s.present || s.flags & 1 == 0 {
        effects.push(format!("{tag}: BuildOrder target [{t_who}][{t_o}] invalid -> kill_current_order / Unit::build_done 0x00603bf0 not transcribed"));
        return;
    }
    // 0x005eec5b: site->is_active() -> finished: kill_current_order, follow-up
    if s.flags & 4 != 0 {
        effects.push(format!("{tag}: BuildOrder site [{t_who}][{t_o}] already active -> kill_current_order / add_gather_order 0x0061a5c0 / build_done not transcribed"));
        return;
    }
    // 0x005eecxx: if (!adjacent_to(o, who)) -> LAB_005ef165 Group swarm re-issue
    match adjacent_to(save, u, &s) {
        None => {
            effects.push(format!("{tag}: Object::adjacent_to 0x00651f40 (attack_dist 0x006488f0) not transcribed; do_build stopped before Wall::do_construct"));
            return;
        }
        Some(false) => {
            effects.push(format!("{tag}: builder not adjacent to [{t_who}][{t_o}] -> kill_current_order / Group::action_swarm_around 0x0070fbe0 not transcribed"));
            return;
        }
        Some(true) => {}
    }
    // covers_tile(unit tile) && !site->is(0x1a1) -> LAB_005ef165 (standing on the site)
    let is_farm = type_is(save, s.ptype, FARM, false);
    if covers_tile(&s, tcoord(ux), tcoord(uy)) && !is_farm {
        effects.push(format!("{tag}: builder standing on site [{t_who}][{t_o}] -> swarm re-issue not transcribed"));
        return;
    }
    set_anim(u, if is_farm { 0x23 } else { 0x21 }, 0, &tag, effects);
    let a = find_angle(s.x - ux, s.y - uy);
    if UImg(u).i32(0x50) != a {
        let mut img = UImg(u);
        set_angle(save, &mut img, a, false, &tag, effects);
    }
    if UImg(u).unit_masks() & 1 != 0 {
        return;
    }
    // amount = Constants+0x22c; quartered when the site is under attack unless
    // has_tribe_bonus(0x10) && Constants+0x7dc != 0.
    let mut amount = constant(save, 0x22c);
    let under_attack = s.build_masks & 0x20 != 0;
    if under_attack {
        match has_tribe_bonus(save, who, 0x10) {
            Some(true) if constant(save, 0x7dc) != 0 => {}
            Some(_) => amount = (amount + ((amount >> 31) & 3)) >> 2,
            None => {
                effects.push(format!("{tag}: site under attack; LeaderData::has_tribe_bonus(0x10) 0x006e1370 not transcribed; amount {amount} left unquartered"));
            }
        }
    }
    let done = do_construct(save, t_who, t_o, amount, effects);
    if done {
        effects.push(format!("{tag}: site [{t_who}][{t_o}] completed -> kill_current_order / Group::normalize 0x00711540 / add_gather_order 0x0061a5c0 / build_done 0x00603bf0 not transcribed"));
    }
}

/// `Object::adjacent_to(o, who)` 0x00651f40. Only the trivial gates are
/// transcribed (`flags & 1` on both); the distance test
/// (`ObjectData::attack_dist` 0x006488f0 `< 0x60`, or the sea-unit arm
/// against Constants+0x94) is not, so the answer is `None` when it would be
/// needed.
fn adjacent_to(save: &Save, u: &Unit, s: &Site) -> Option<bool> {
    if u.base.sub.flags & 1 == 0 || s.flags & 1 == 0 {
        return Some(false);
    }
    let _ = save;
    None
}

// ---------------------------------------------------------------------------
// Wall::do_construct 0x006434d0
// ---------------------------------------------------------------------------

/// `Wall::do_construct(amount)` on `Objects.lists[who][o]`. Returns `true`
/// when the site completed (retail returns 1 after `Build::activate`).
///
/// ```text
/// if (ai_speed > 1) amount *= ai_speed;
/// if (!is_started()) {                           // flags & 2
///   r = BuildTypeData::blocked_site(x, y, who, o, 0);        // 0x00636a50
///   if (r == 0x2a) { city >= 0 && num_wonders(1) > has_tribe_bonus(7)+1 -> disband } else
///   if (r in {0, 0x27, 0x28, 0x29, 0x2b}) start(1) else disband(1) + local-player UI
/// }
/// if (!is_active()) {                            // flags & 4
///   amount /= helpers + 1;
///   if (!(build_masks & 0x800)) { recharging += 1; build_masks |= 0x800; }
///   helpers += 1;
///   amount = max(amount, 1); job_counter_2 += amount; job_counter += amount;
///   if (job_counter >= construct_time(0)) { activate(0,1,1); UI; return 1; }
///   UI: 84% advisor flag
/// }
/// return 0;
/// ```
pub fn do_construct(save: &mut Save, who: i32, o: i32, amount: i32, effects: &mut Vec<String>) -> bool {
    let tag = format!("Objects.lists[{who}][{o}]");
    let s = site(save, who, o);
    if !s.present {
        return false;
    }
    let fr = frame(save);
    let mut amount = if AI_SPEED > 1 { AI_SPEED * amount } else { amount };
    if s.flags & 2 == 0 {
        // TODO(va 0x00636a50) BuildTypeData::blocked_site — the placement
        // re-check. Its "clear" results lead to Build::start; 0x2a (wonder
        // slot) to the CityData::num_wonders 0x007382b0 gate; anything else to
        // Object::disband 0x006455c0. Not transcribed: the site is treated as
        // clear (the only non-exceptional outcome) and `Build::start(1)`
        // 0x006273a0 -> `Wall::start` 0x0063e810 is applied: `flags |= 2;
        // frame_started = frame`. Its World writes (footprint collision stamp
        // FUN_00850c40, per-player seen bits, check_ever_seen 0x0063ce70,
        // FUN_0063d230) are not ours and are left untouched.
        effects.push(format!("{tag}: blocked_site 0x00636a50 not transcribed — assumed clear; Build::start applied (World footprint/seen stamps untouched)"));
        with_site(save, who, o, |b| {
            let fl = b.flags();
            b.0.base.sub.flags = fl | 2;
            b.set_i32(0x5c, fr);
            effects.push(format!("{tag}.SubObject.flags {fl:#x} -> {:#x}; Wall.frame_started -> {fr} (Wall::start)", fl | 2));
        });
    }
    if s.flags & 4 != 0 {
        return false;
    }
    let mut done = false;
    with_site(save, who, o, |b| {
        let helpers = b.u8(0x64);
        amount /= helpers as i32 + 1;
        let bm = b.build_masks();
        if bm & 0x800 == 0 {
            let r = b.i16(0x7a);
            b.set_i16(0x7a, r.wrapping_add(1));
            b.set_u16(0x60, bm | 0x800);
            effects.push(format!("{tag}.Build.recharging {r} -> {} ; Wall.build_masks {bm:#x} -> {:#x} (construct stamp)", r.wrapping_add(1), bm | 0x800));
        }
        b.set_u8(0x64, helpers.wrapping_add(1));
        effects.push(format!("{tag}.Wall.helpers {helpers} -> {}", helpers.wrapping_add(1)));
        if amount < 1 {
            amount = 1;
        }
        let jc = b.u32(0x48);
        let jc2 = b.u32(0x4c);
        b.set_u32(0x4c, jc2.wrapping_add(amount as u32));
        b.set_u32(0x48, jc.wrapping_add(amount as u32));
        effects.push(format!("{tag}.Wall.job_counter {jc} -> {} ; job_counter_2 {jc2} -> {}", jc.wrapping_add(amount as u32), jc2.wrapping_add(amount as u32)));
        // TODO(va 0x0062d5c0) BuildData::construct_time(0): base constr_time
        // (+0x50) reduced by wonder-race / Senate-city / general 0x163 / tribe
        // 0x12 modifiers. Only the unmodified base is known here; when the
        // counter reaches it the site has certainly completed in retail
        // (Build::activate 0x00623e20, 13 KB, not transcribed).
        let base = b.u32(0x50);
        if jc.wrapping_add(amount as u32) >= base {
            done = true;
            effects.push(format!("{tag}: job_counter >= constr_time {base} -> Build::activate 0x00623e20 not transcribed"));
        }
    });
    done
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{container, load};
    use std::path::{Path, PathBuf};

    fn capture_dir() -> Option<PathBuf> {
        let name = std::env::var("DON_CAPTURE").unwrap_or_else(|_| "20261004-075733-stride1".into());
        let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/live/frame-pairs").join(name);
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

    fn i32at(v: &[u8], i: usize) -> i32 {
        i32::from_le_bytes(v[i..i + 4].try_into().unwrap())
    }

    fn diffs(a: &[u8], b: &[u8], base: usize) -> Vec<String> {
        a.iter().zip(b.iter()).enumerate().filter(|(_, (x, y))| x != y).map(|(i, (x, y))| format!("+{:#x}:{x:#04x}->{y:#04x}", base + i)).collect()
    }

    #[test]
    fn find_angle_cardinals() {
        assert_eq!(find_angle(0, -5), 0);
        assert_eq!(find_angle(0, 5), i32::MIN);
        assert_eq!(find_angle(5, 0), 0x4000_0000);
        assert_eq!(find_angle(-5, 0), 0xc000_0000u32 as i32);
        // diagonal NE ~ 45 degrees
        let a = find_angle(100, -100) as u32;
        assert!((0x1f00_0000..=0x2100_0000).contains(&a), "{a:#x}");
    }

    #[test]
    fn vector_dist_basic() {
        assert_eq!(vector_dist(0, 0), 0);
        assert_eq!(vector_dist(100, 0), 100);
        assert_eq!(vector_dist(100, 100), 150);
        assert_eq!(vector_dist(-3, 4), 4 + 9 / 8);
    }

    /// Byte image of a Unit (same scheme as objects_process's test).
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

    fn build_image(b: &Build) -> Vec<(usize, u8)> {
        let mut v = Vec::new();
        v.push((0x08, b.base.sub.flags));
        v.extend(b.base.sub.body.iter().enumerate().map(|(i, &x)| (0x9 + i, x)));
        v.extend(b.base.mid.iter().enumerate().map(|(i, &x)| (0x20 + i, x)));
        v.extend(b.wall_body.iter().enumerate().map(|(i, &x)| (0x48 + i, x)));
        v.extend(b.body.iter().enumerate().map(|(i, &x)| (0x70 + i, x)));
        v
    }

    fn field_name(off: usize) -> String {
        if off >= 0x30000 {
            return format!("Guy+{:#x}", (off - 0x30000) % 0x100);
        }
        if off >= 0x20000 {
            return format!("Order+{:#x}", (off - 0x20000) % 0x100);
        }
        if off >= 0x10000 {
            return "Path".into();
        }
        format!("+{off:#x}")
    }

    /// Lane gate: run `run_for_gatherers` on retail frame N and account every
    /// Unit, Build and Farm byte against retail N+1 — `introduced` must be 0;
    /// `explained` > 0.
    #[test]
    fn gather_work_burndown_introduces_nothing() {
        let Some(dir) = capture_dir() else {
            eprintln!("no capture dir; skipping");
            return;
        };
        let st = steps(&dir);
        let (mut explained, mut unexplained, mut introduced) = (0usize, 0usize, 0usize);
        let mut introduced_rows = Vec::new();
        let mut explained_by: std::collections::BTreeMap<String, usize> = Default::default();
        let mut unexplained_by: std::collections::BTreeMap<String, usize> = Default::default();
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
            run_for_gatherers(&mut ours, &mut effects);
            let s1 = u32::from_le_bytes(ours.post_world[0x28..0x2c].try_into().unwrap());
            let draws = crate::tick::rng_draws(s0, s1).unwrap_or(0);
            draws_total += draws;
            let (mut pe, mut pu, mut pi) = (0, 0, 0);
            let mut account = |ia: &[(usize, u8)], ib: &[(usize, u8)], io: &[(usize, u8)], what: &str| {
                assert_eq!(ia.len(), io.len(), "{what}: we changed the image length");
                for i in 0..ia.len().min(ib.len()) {
                    if ia[i].0 != ib[i].0 {
                        break;
                    }
                    let (ra, rb, ro) = (ia[i].1, ib[i].1, io[i].1);
                    if ra != rb {
                        if ro == rb {
                            pe += 1;
                            *explained_by.entry(format!("{what} {}", field_name(ia[i].0))).or_default() += 1;
                        } else {
                            pu += 1;
                            *unexplained_by.entry(format!("{what} {}", field_name(ia[i].0))).or_default() += 1;
                        }
                    } else if ro != ra {
                        pi += 1;
                        introduced_rows.push(format!("f{}->f{} {what} {}: retail {ra:#04x} ours {ro:#04x}", st[k].0, st[k + 1].0, field_name(ia[i].0)));
                    }
                }
            };
            for owner in 0..a.objects.lists.len() {
                for slot in 0..a.objects.lists[owner].elems.len().min(b.objects.lists[owner].elems.len()) {
                    let (Some(oa), Some(ob), Some(oo)) = (&a.objects.lists[owner].elems[slot], &b.objects.lists[owner].elems[slot], &ours.objects.lists[owner].elems[slot]) else {
                        continue;
                    };
                    if let (Some(xa), Some(xb), Some(xo)) = (unit_of(oa), unit_of(ob), unit_of(oo)) {
                        // Only units this lane touches: head order Gather/Build.
                        if !matches!(xa.orders.orders.first().map(|o| o.ty), Some(6) | Some(7)) {
                            continue;
                        }
                        account(&image(xa), &image(xb), &image(xo), &format!("unit[{owner}][{slot}]"));
                    }
                    if let (Obj::Build(xa), Obj::Build(xb), Obj::Build(xo)) = (oa, ob, oo) {
                        account(&build_image(xa), &build_image(xb), &build_image(xo), &format!("build[{owner}][{slot}]"));
                    }
                }
            }
            for (fi, ((fa, fb), fo)) in a.farms.farm_data.elems.iter().zip(b.farms.farm_data.elems.iter()).zip(ours.farms.farm_data.elems.iter()).enumerate() {
                let img = |r: &Vec<u8>| r.iter().enumerate().map(|(i, &x)| (i, x)).collect::<Vec<_>>();
                account(&img(&fa.data), &img(&fb.data), &img(&fo.data), &format!("farm[{fi}]"));
            }
            eprintln!("pair f{}->f{}: explained={pe} unexplained={pu} introduced={pi} draws={draws}", st[k].0, st[k + 1].0);
            explained += pe;
            unexplained += pu;
            introduced += pi;
        }
        eprintln!("gather-work bytes: explained={explained} unexplained={unexplained} introduced={introduced} draws_total={draws_total}");
        for (k, v) in &explained_by {
            eprintln!("  explained   {v:6} {k}");
        }
        for (k, v) in &unexplained_by {
            eprintln!("  unexplained {v:6} {k}");
        }
        for r in introduced_rows.iter().take(40) {
            eprintln!("  INTRODUCED {r}");
        }
        assert_eq!(introduced, 0, "introduced bytes");
        assert!(explained > 0, "no byte explained");
    }

    /// Diagnostic: every gather/build unit, its order, guy anim, and the
    /// retail diff on the unit, its site and farm (`-- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn survey_gatherers() {
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
            let fr = frame(&a);
            let s0 = u32::from_le_bytes(a.post_world[0x28..0x2c].try_into().unwrap());
            let s1 = u32::from_le_bytes(b.post_world[0x28..0x2c].try_into().unwrap());
            println!("## f{} frame={fr} draws={:?}", st[k].0, crate::tick::rng_draws(s0, s1));
            for (fi, (fa, fb)) in a.farms.farm_data.elems.iter().zip(b.farms.farm_data.elems.iter()).enumerate() {
                let d = diffs(&fa.data, &fb.data, 0);
                if !d.is_empty() {
                    println!("  farm[{fi}] who={} o={} type={} status={:02x?} : {}", i32at(&fa.data, 0), i32at(&fa.data, 4), fa.data[0xbd], &fa.data[0xac..0xbc], d.join(" "));
                }
            }
            let mut ours = a.clone();
            let mut effects = Vec::new();
            run_for_gatherers(&mut ours, &mut effects);
            for owner in 0..a.objects.lists.len() {
                for slot in 0..a.objects.lists[owner].elems.len().min(b.objects.lists[owner].elems.len()) {
                    let (Some(oa), Some(ob)) = (&a.objects.lists[owner].elems[slot], &b.objects.lists[owner].elems[slot]) else { continue };
                    let (Some(ua), Some(ub)) = (unit_of(oa), unit_of(ob)) else { continue };
                    let Some(o0) = ua.orders.orders.first() else { continue };
                    if o0.ty != 7 && o0.ty != 6 {
                        continue;
                    }
                    let p = &o0.payload;
                    let mut line = format!("  [{owner}][{slot}] ty={} flags={:#x} tgt=({},{}) uid={}", o0.ty, p[0], i32at(p, 5), i32at(p, 1), u16::from_le_bytes([p[9], p[10]]));
                    if o0.ty == 7 {
                        line += &format!(
                            " tx={} ty={} btype={:#x} wait={} goto={} nonflat={} dist={} been={}",
                            i32at(p, 11),
                            i32at(p, 15),
                            i32at(p, 19),
                            i32at(p, 23),
                            p[27],
                            p[28],
                            p[29],
                            p[30]
                        );
                    }
                    let um = u32::from_le_bytes(ua.body[0x68 - 0x48..0x6c - 0x48].try_into().unwrap());
                    let group = i16::from_le_bytes([ua.body[0x80 - 0x48], ua.body[0x81 - 0x48]]);
                    let gd = i16::from_le_bytes([ua.body[0x92 - 0x48], ua.body[0x93 - 0x48]]);
                    let anim = ua.guys.elems.first().and_then(|g| g.as_ref()).map(|g| g.data[0x9c - 8]).unwrap_or(0xff);
                    let (ct, et) = ua.guys.elems.first().and_then(|g| g.as_ref()).map(|g| (i32at(&g.data, 0x74 - 8), i32at(&g.data, 0x78 - 8))).unwrap_or((0, 0));
                    line += &format!(" um={um:#x} group={group} gdown={gd} anim={anim:#x} clk={ct}/{et} orders={} path={} guys={}", ua.orders.orders.len(), ua.path.data.len() / 16, ua.guys.elems.len());
                    let mut d = diffs(&ua.body, &ub.body, 0x48);
                    d.extend(diffs(&ua.base.mid, &ub.base.mid, 0x20));
                    d.extend(diffs(&ua.base.sub.body, &ub.base.sub.body, 0x09));
                    if ua.orders.orders.len() == ub.orders.orders.len() {
                        for (i, (x, y)) in ua.orders.orders.iter().zip(ub.orders.orders.iter()).enumerate() {
                            let dd = diffs(&x.payload, &y.payload, 0);
                            if !dd.is_empty() {
                                d.push(format!("order[{i}]:{}", dd.join(",")));
                            }
                        }
                    } else {
                        d.push(format!("orders {}->{}", ua.orders.orders.len(), ub.orders.orders.len()));
                    }
                    for (gi, (x, y)) in ua.guys.elems.iter().zip(ub.guys.elems.iter()).enumerate() {
                        if let (Some(x), Some(y)) = (x, y) {
                            let dd = diffs(&x.data, &y.data, 8);
                            if !dd.is_empty() {
                                d.push(format!("guy[{gi}]:{}", dd.join(",")));
                            }
                        }
                    }
                    if ua.path.data != ub.path.data {
                        d.push(format!("path {}->{}", ua.path.data.len() / 16, ub.path.data.len() / 16));
                    }
                    println!("{line}\n      retail: {}", d.join(" "));
                    if let Some(Some(uo)) = ours.objects.lists[owner].elems.get(slot) {
                        if let Some(uo) = unit_of(uo) {
                            let mut d = diffs(&ua.body, &uo.body, 0x48);
                            for (i, (x, y)) in ua.orders.orders.iter().zip(uo.orders.orders.iter()).enumerate() {
                                let dd = diffs(&x.payload, &y.payload, 0);
                                if !dd.is_empty() {
                                    d.push(format!("order[{i}]:{}", dd.join(",")));
                                }
                            }
                            for (gi, (x, y)) in ua.guys.elems.iter().zip(uo.guys.elems.iter()).enumerate() {
                                if let (Some(x), Some(y)) = (x, y) {
                                    let dd = diffs(&x.data, &y.data, 8);
                                    if !dd.is_empty() {
                                        d.push(format!("guy[{gi}]:{}", dd.join(",")));
                                    }
                                }
                            }
                            println!("      ours:   {}", d.join(" "));
                        }
                    }
                    let tw = i32at(p, 5) as usize;
                    let to = i32at(p, 1) as usize;
                    if let (Some(Some(Obj::Build(sa))), Some(Some(Obj::Build(sb)))) = (a.objects.lists.get(tw).and_then(|l| l.elems.get(to)), b.objects.lists.get(tw).and_then(|l| l.elems.get(to))) {
                        let pt = i32at(&sa.base.sub.body, 15);
                        let mut d = diffs(&sa.wall_body, &sb.wall_body, 0x48);
                        d.extend(diffs(&sa.body, &sb.body, 0x70));
                        d.extend(diffs(&sa.base.mid, &sb.base.mid, 0x20));
                        println!(
                            "      site [{tw}][{to}] ptype={pt:#x} flags={:#x} bm={:#x} helpers={} jc={} jc2={} constr={} recharging={} gmax={}: {}",
                            sa.base.sub.flags,
                            u16::from_le_bytes([sa.wall_body[0x18], sa.wall_body[0x19]]),
                            sa.wall_body[0x1c],
                            i32at(&sa.wall_body, 0),
                            i32at(&sa.wall_body, 4),
                            i32at(&sa.wall_body, 8),
                            i16::from_le_bytes([sa.body[0xa], sa.body[0xb]]),
                            sa.body[0x10] as i8,
                            d.join(" ")
                        );
                    }
                }
            }
            for e in effects.iter().filter(|e| !e.contains("set_anim") && !e.contains("check_gatherers")).take(60) {
                println!("    fx: {e}");
            }
        }
    }
}
