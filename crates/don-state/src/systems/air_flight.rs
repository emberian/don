//! The air cone: everything `Objects::process_all` 0x0065dce0 reaches for
//! aircraft, their hosts (airbase / carrier / silo containment) and the
//! anti-air dud roll. Leaf module: free functions over [`Save`] for the
//! owning traversals (`objects_process.rs` at the `Unit::process` air-fuel
//! branch and the `Unit::work` launch gate; `build_process.rs` at the
//! `build_masks & 0x4000 / & 0x8` gates) to call; never edits siblings.
//!
//! ```text
//!   0x00611050..0x00611158  Unit::process air-fuel branch       [air_fuel_step]       transcribed
//!   0x00609a50              UnitData::mana                      [mana]                air arm transcribed; land arm stop
//!   0x006e12e0              LeaderData::get_heal_level          [heal_level]          loop transcribed; has_preq stop
//!   0x0067be49..0x0067c16a  Ammo::init anti-air dud gate        [antiair_dud_gate]    transcribed (1 RNG site, 0..2 draws)
//!   0x0060a140 / 0x0060a310 UnitData::is_flying_low / _high     [is_flying_low]       flag arms transcribed; order-target arm stop
//!   0x00646c40              ObjectData::can_carry(DomainIndex)  [can_carry_domain]    transcribed
//!   0x006483c0              ObjectData::can_carry(int, int)     [can_carry_object]    transcribed; home_base/num_aircraft_here stops
//!   0x006454a0              ObjectData::num_aircraft_limit      [num_aircraft_limit]  transcribed
//!   0x00645330              ObjectData::num_aircraft_here       [num_aircraft_here]   count loop transcribed; home_base stop
//!   0x00646d50              ObjectData::num_inside              [num_inside]          transcribed
//!   0x006201e0              Build::process_ejection             [process_ejection]    transcribed; Unit::come_out stop
//!   0x0064f3b0              Object::do_launch                   [do_launch]           head + chain walk transcribed; AI target scans stop
//!   0x005e9be0              Unit::check_fuel                    [check_fuel]          returning latch + host search transcribed; approach vector stop
//!   0x005ea620              Unit::do_air_patrol                 [do_air_patrol]       waypoint/cadence skeleton; physics + finders stop
//!   0x005ea420              Unit::do_air_attack_ground          [do_air_attack_ground] release gate skeleton; physics stop
//!   0x005eab00              Unit::do_strafe                     stop (3,676 B; see notes)
//!   0x005e86d0              Unit::do_air_physics                stop (2 RNG sites; see notes)
//! ```
//!
//! Everything is addressed by retail image offset through [`UImg`] /
//! [`BImg`] (same maps as `objects_process::UImg` / `build_process::Img`,
//! duplicated because leaf modules do not share private items):
//!
//! ```text
//!   Unit  +0x08 flags | +0x09..+0x1c SubObject | +0x20..+0x42 ObjectData | +0x48..+0xb7 UnitData
//!   Build +0x08 flags | +0x09..+0x1c SubObject | +0x20..+0x42 ObjectData | +0x48..+0x66 WallData | +0x70..+0x86 BuildData
//!   ObjectData:  inside_down +0x28 (i16)  up +0x2a  down +0x2c  down_who +0x2e  uid +0x30
//!                hold_frames +0x32  healing +0x38  inside_down_who +0x3e (i8)  up_who +0x3f
//!                launch_frames +0x41 (i8)  launching +0x44 (SimpleArray<int>*, walked as ObjBase.launching)
//!   UnitData:    unit_masks +0x68  inside_up +0x82 (i16)  o_up +0x8e  mana_burn +0x96 (i16)
//!                recharging +0xae (u8)  inside_up_who +0xb4 (i8)
//!   BuildData:   build_masks +0x60 (u16)  recharging +0x7a (i16)
//! ```
//!
//! Containment (PDB names, derived from `num_inside` 0x00646d50 and the
//! `do_launch` chain walk): a host's `inside_down`/`inside_down_who` names
//! the first contained object; each contained object's own
//! `inside_down`/`inside_down_who` names the next one (a singly linked
//! chain terminated by `inside_down < 0`), and its `inside_up`/
//! `inside_up_who` names the host. `inside_up < 0` on an air unit means
//! *airborne*. `up`/`up_who` (+0x2a/+0x3f) are the cargo-on-transport
//! links and are only read here through `get_captain`.
//!
//! Order payloads (serialized `Order.payload`, `[0]` = the inherited
//! `UnitOrder::flags` byte; `AirOrder::walk_data` 0x0047f2d0 walks the six
//! i32 `oxx, whose, cruising_alt, sharp_turn, old, returning`):
//!
//! ```text
//!   0x10 StrafeOrder          0x0047f310  57 B: Target[1..11) Attack[11..24) flags[24] Air[25..49) xx,yy[49..57)
//!   0x11 AirPatrolOrder       0x00483ed0  var:  waypoint[1..5) SimpleArray<Coord> x_pos, y_pos, flags, Air 24 B
//!   0x18 AirAttackGroundOrder 0x004875a0  54 B: att_x,att_y,accuracy,attack_unit[1..17) flags[17] Air[18..42) total_time,sx,sy[42..54)
//! ```
//!
//! `sections::OrderList::walk` does not yet decode these three indices
//! (it errors on them), so no capture with a flying unit can currently be
//! loaded; see the capture recipe at the end of this header.
//!
//! RNG (`Random::get(0, 0xffff)` 0x00a39d70 on `GameAccess::game_random`
//! `[0x00c06184]`, the main LCG at `Save.post_world + 0x28`):
//!
//! * `Ammo::init` 0x0067bbf0 dud gate — transcribed in [`antiair_dud_gate`].
//!   0, 1 or 2 draws per projectile aimed at a fixed-wing aircraft.
//! * `Object::do_launch` 0x00650083 — silo auto-target scan: one draw per
//!   enemy city candidate whose `City+4 & 2 == 0`, `% 10 != 0` rejects it.
//!   Stop (needs the LeaderData AI target lists at +0x408/+0x424/+0x428/
//!   +0x6a4c, only partly serialized).
//! * `Unit::do_air_physics` 0x005e86d0 — `(o + frame) & 7 == 0` on a
//!   non-helicopter, non-`is(0x130)` plane: `cruising_alt = (r % 7 + 13) *
//!   100` (one draw); and when the forward probe `FUN_00607c30` is blocked
//!   with `sharp_turn == 0`: `sharp_turn = (r & 1) ? 1 : -1` (one draw).
//!   Stop.
//! * `Unit::come_out` 0x00617c10 — two draws at 0x00617c10+… (lines 1211/
//!   1219 of the decomp) on the eject path. Stop.
//!
//! Status is `Partial`. No current capture contains an aircraft, an
//! airbase, a carrier or a silo (all Ancient-age), so every function below
//! is transcription-only: unit tests pin the *structure* the disassembly
//! fixes (draw counts per arm, which bytes move, early returns) and a
//! capture-driven control asserts the cone writes nothing on the idle
//! captures. **Capture recipe to verify this module** (a `.svx` pair at
//! stride 1, plus `manifest.json`): a Modern/Information-age game with (a)
//! one airbase holding ≥ 2 fighters, one of them with `mana_burn > 0` so the
//! periodic repair / fuel decay fires; (b) one fighter airborne on
//! `AIR_PATROL` (0x11) with ≥ 2 waypoints, so the waypoint cursor and the
//! 16/32-frame gates are exercised; (c) one bomber on `AIR_ATTACK_GROUND`
//! (0x18); (d) a ground AA unit (`has_objmask(0x80000000)`) and a plain
//! rifleman both firing at the patrolling fighter, so the dud gate's 1-draw
//! and 2-draw arms both appear in the `game_random` delta; (e) a carrier
//! with one plane, so `can_carry`'s `is(0x15f)` arm is exercised; (f) a
//! missile silo with a loaded missile and an enemy city in range, so the
//! `do_launch` silo scan and its `% 10` draw appear. `OrderList::walk`
//! must learn indices 0x10/0x11/0x18 (sizes above) before such a capture
//! loads.

#![allow(dead_code)]

use crate::sections::{Build, Obj, Save, TypeRec, Unit};
use crate::tick::StepStatus;

pub const STATUS: StepStatus = StepStatus::Partial;

/// `FRAMES_BETWEEN_LAUNCHES` 0x00c06248 — .data initialiser 15; no writer
/// in the image (only `Object::do_launch` and `Object::attempt_launch`
/// 0x00643a10 read it).
pub const FRAMES_BETWEEN_LAUNCHES: i8 = 15;

// ---------------------------------------------------------------------------
// Image accessors
// ---------------------------------------------------------------------------

/// Byte-image view of a serialized `Unit`, addressed by retail offset.
pub(crate) struct UImg<'a>(pub(crate) &'a mut Unit);

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
        let (v, i) = self.slot(off);
        v[i] = x;
    }
    fn set_i8(&mut self, off: usize, x: i8) {
        self.set_u8(off, x as u8)
    }
    fn set_i16(&mut self, off: usize, x: i16) {
        let (v, i) = self.slot(off);
        v[i..i + 2].copy_from_slice(&x.to_le_bytes());
    }
    fn set_i32(&mut self, off: usize, x: i32) {
        let (v, i) = self.slot(off);
        v[i..i + 4].copy_from_slice(&x.to_le_bytes());
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
    /// Coords are stored XOR 0x63637.
    fn x(&mut self) -> i32 {
        self.i32(0x10) ^ 0x63637
    }
    fn y(&mut self) -> i32 {
        self.i32(0x14) ^ 0x63637
    }
    fn z(&mut self) -> i32 {
        self.i32(0x0c) ^ 0x63637
    }
    /// `orderlist.length` (+0xd8): the serialized order count.
    fn num_orders(&self) -> usize {
        self.0.orders.orders.len()
    }
    /// `UnitData::order_type` 0x00616e80: the current order's `get_type`,
    /// 0 when the list is empty. The serialized list starts at
    /// `head_node->next` (OrderList::walk_data 0x00730270), so index 0 is
    /// the current order.
    fn order_type(&self) -> i32 {
        self.0.orders.orders.first().map(|o| o.ty).unwrap_or(0)
    }
    fn order_flags(&self) -> u8 {
        self.0.orders.orders.first().and_then(|o| o.payload.first().copied()).unwrap_or(0)
    }
}

/// Byte-image view of a serialized `Build`, addressed by retail offset.
pub(crate) struct BImg<'a>(pub(crate) &'a mut Build);

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
    }
    fn set_i16(&mut self, off: usize, x: i16) {
        let (v, i) = self.slot(off);
        v[i..i + 2].copy_from_slice(&x.to_le_bytes());
    }
    fn set_u16(&mut self, off: usize, x: u16) {
        self.set_i16(off, x as i16)
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
}

/// The ObjectData sub-image (`base.mid`) any object kind shares, for the
/// containment chain walks that hop between Units and Builds.
fn obj_mid(save: &Save, who: i32, o: i32) -> Option<&[u8]> {
    let l = save.objects.lists.get(usize::try_from(who).ok()?)?;
    let mid = match l.elems.get(usize::try_from(o).ok()?)?.as_ref()? {
        Obj::Unit(u) => &u.base.mid,
        Obj::Animal(a) => &a.unit.base.mid,
        Obj::Build(b) => &b.base.mid,
    };
    (mid.len() == 34).then_some(mid.as_slice())
}

fn obj_sub(save: &Save, who: i32, o: i32) -> Option<(&[u8], u8)> {
    let l = save.objects.lists.get(usize::try_from(who).ok()?)?;
    let (body, flags) = match l.elems.get(usize::try_from(o).ok()?)?.as_ref()? {
        Obj::Unit(u) => (&u.base.sub.body, u.base.sub.flags),
        Obj::Animal(a) => (&a.unit.base.sub.body, a.unit.base.sub.flags),
        Obj::Build(b) => (&b.base.sub.body, b.base.sub.flags),
    };
    (body.len() == 19).then_some((body.as_slice(), flags))
}

/// `ObjectData.inside_down` (+0x28) / `inside_down_who` (+0x3e) of any object.
fn inside_down(save: &Save, who: i32, o: i32) -> Option<(i32, i32)> {
    let mid = obj_mid(save, who, o)?;
    Some((i16::from_le_bytes([mid[0x08], mid[0x09]]) as i32, mid[0x1e] as i8 as i32))
}

/// `SubObject.flags & 1` (live) of any object.
fn obj_live(save: &Save, who: i32, o: i32) -> bool {
    obj_sub(save, who, o).map(|(_, f)| f & 1 != 0).unwrap_or(false)
}

fn obj_ptype(save: &Save, who: i32, o: i32) -> Option<i32> {
    obj_sub(save, who, o).map(|(b, _)| i32::from_le_bytes(b[15..19].try_into().unwrap()))
}

fn obj_xy(save: &Save, who: i32, o: i32) -> Option<(i32, i32)> {
    obj_sub(save, who, o).map(|(b, _)| {
        (i32::from_le_bytes(b[7..11].try_into().unwrap()) ^ 0x63637, i32::from_le_bytes(b[11..15].try_into().unwrap()) ^ 0x63637)
    })
}

/// `Object::is_unit` (vtable +0x18: Unit/Animal `return 1`, Build `return 0`).
fn obj_is_unit(save: &Save, who: i32, o: i32) -> Option<bool> {
    let l = save.objects.lists.get(usize::try_from(who).ok()?)?;
    Some(!matches!(l.elems.get(usize::try_from(o).ok()?)?.as_ref()?, Obj::Build(_)))
}

/// A Unit's `+0x48..` body byte-range reader for cross-object reads.
fn unit_body(save: &Save, who: i32, o: i32) -> Option<&[u8]> {
    let l = save.objects.lists.get(usize::try_from(who).ok()?)?;
    let u = match l.elems.get(usize::try_from(o).ok()?)?.as_ref()? {
        Obj::Unit(u) => u,
        Obj::Animal(a) => &a.unit,
        Obj::Build(_) => return None,
    };
    (u.body.len() == 111).then_some(u.body.as_slice())
}

fn unit_i16(save: &Save, who: i32, o: i32, off: usize) -> Option<i16> {
    let b = unit_body(save, who, o)?;
    Some(i16::from_le_bytes([b[off - 0x48], b[off - 0x47]]))
}

fn unit_u32(save: &Save, who: i32, o: i32, off: usize) -> Option<u32> {
    let b = unit_body(save, who, o)?;
    Some(u32::from_le_bytes(b[off - 0x48..off - 0x44].try_into().unwrap()))
}

fn unit_ref(save: &Save, who: i32, o: i32) -> Option<&Unit> {
    let l = save.objects.lists.get(usize::try_from(who).ok()?)?;
    match l.elems.get(usize::try_from(o).ok()?)?.as_ref()? {
        Obj::Unit(u) => Some(u),
        Obj::Animal(a) => Some(&a.unit),
        Obj::Build(_) => None,
    }
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

fn leader_flags(save: &Save, who: i32) -> i32 {
    usize::try_from(who).ok().and_then(|w| save.leaders.slots.get(w)).map(|l| l.flags).unwrap_or(0)
}

fn leader_flags2(save: &Save, who: i32) -> i32 {
    usize::try_from(who).ok().and_then(|w| save.leaders.slots.get(w)).map(|l| l.flags2).unwrap_or(0)
}

/// `LeaderData` i32 at image offset `off` (body is +0x08..).
fn leader_i32(save: &Save, who: i32, off: usize) -> Option<i32> {
    let l = save.leaders.slots.get(usize::try_from(who).ok()?)?;
    l.body.get(off - 8..off - 4).map(|b| i32::from_le_bytes(b.try_into().unwrap()))
}

/// `Objects` scalar block: `[0x1f4..0x1fc) [0x154..0x15c) unit_mark[9]
/// build_mark[9] wall_mark[9] obj_ctr[9]` (Objects::walk_data 0x006541e0).
fn unit_mark(save: &Save, owner: i32) -> i32 {
    let Ok(owner) = usize::try_from(owner) else { return 0 };
    if owner >= 9 {
        return 0;
    }
    let o = 16 + owner * 4;
    i32::from_le_bytes(save.objects.scalars[o..o + 4].try_into().unwrap())
}

fn build_mark(save: &Save, owner: i32) -> i32 {
    let Ok(owner) = usize::try_from(owner) else { return 0 };
    if owner >= 9 {
        return 0;
    }
    let o = 16 + 36 + owner * 4;
    i32::from_le_bytes(save.objects.scalars[o..o + 4].try_into().unwrap())
}

/// `Random::get(min, max)` 0x00a39d70 on `GameAccess::game_random`
/// (`[0x00c06184]`, walked at `Save.post_world + 0x28`): `seed = seed *
/// 0x19660d + 0x3c6ef35f; ((seed & 0xffff) * (max - min) >> 16) + min`.
/// Same body as `game_daemon::game_random`, which is private to that
/// module.
pub(crate) fn game_random(save: &mut Save, min: i32, max: i32) -> i32 {
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

/// `vector_dist(dx, dy)` 0x0046cff0: `max(|dx|,|dy|) + min(|dx|,|dy|)/2`
/// — the octagonal distance every range test in this cone uses.
fn vector_dist(dx: i32, dy: i32) -> i32 {
    let (ax, ay) = (dx.wrapping_abs(), dy.wrapping_abs());
    if ax < ay {
        ay + ax / 2
    } else {
        ax + ay / 2
    }
}

// ---------------------------------------------------------------------------
// Type records
// ---------------------------------------------------------------------------

/// Type record image reader (`Rules.types[idx]`): `head` = image[4..94),
/// `obj_mid` = image[0x1e4..0x27c), `ext` = the unit tail
/// `[0x2b4..0x2cc) ++ [0x2d4..0x2dc) ++ [0x2dc..0x2e0) ++ [0x2e0..0x5d4)`,
/// so unit image offsets >= 0x2d4 are shifted by the 8-byte gap at 0x2cc.
/// Building types carry a 49-byte `ext` from 0x2b4 with no gap.
struct TypeImg<'a>(&'a TypeRec, i32);

impl TypeImg<'_> {
    fn is_unit_type(&self) -> bool {
        (0x32..=0x19d).contains(&self.1)
    }
    fn i32(&self, off: usize) -> Option<i32> {
        let (v, i) = match off {
            0x04..=0x5d => (&self.0.head, off - 4),
            0x1e4..=0x27b => (&self.0.obj_mid, off - 0x1e4),
            0x2b4..=0x2cb => (&self.0.ext, off - 0x2b4),
            0x2d4.. if self.is_unit_type() => (&self.0.ext, off - 0x2b4 - 8),
            0x2cc.. => (&self.0.ext, off - 0x2b4),
            _ => return None,
        };
        v.get(i..i + 4).map(|b| i32::from_le_bytes(b.try_into().unwrap()))
    }
    /// `is_list` (+0x280) / `is_strict_list` (+0x29c): the two
    /// `SimpleArray<u16>` that follow the object body.
    fn list(&self, strict: bool) -> impl Iterator<Item = u16> + '_ {
        let a = if strict { &self.0.arr1 } else { &self.0.arr0 };
        a.data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]))
    }
}

fn type_rec(save: &Save, idx: i32) -> Option<TypeImg<'_>> {
    usize::try_from(idx).ok().and_then(|i| save.rules_tail.rules.types.get(i)).map(|t| TypeImg(t, idx))
}

/// `ObjectType.domain` (+0x218): 0 land, 1 sea, 2 air.
fn type_domain(save: &Save, ty: i32) -> i32 {
    type_rec(save, ty).and_then(|t| t.i32(0x218)).unwrap_or(0)
}

/// `ObjectType.obj_masks` (+0x1e4). Bit 0x8000000 = missile.
fn type_obj_masks(save: &Save, ty: i32) -> u32 {
    type_rec(save, ty).and_then(|t| t.i32(0x1e4)).unwrap_or(0) as u32
}

/// `UnitType.unit_flags` (+0x2b4). Bit 0x20 = helicopter.
fn type_unit_flags(save: &Save, ty: i32) -> u32 {
    type_rec(save, ty).and_then(|t| t.i32(0x2b4)).unwrap_or(0) as u32
}

/// `UnitType.unit_flags2` (+0x2b8). 0x02 hero/special class, 0x40 supply.
fn type_unit_flags2(save: &Save, ty: i32) -> u32 {
    type_rec(save, ty).and_then(|t| t.i32(0x2b8)).unwrap_or(0) as u32
}

/// `ObjectTypeData::is_slow` 0x00661ae0 (`strict == 0` arm).
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

/// `ObjectTypeData::is(what, strict)` 0x0065f7d0 — what every
/// `ObjectData::is` 0x00653790 (vtable +0xb8) call below resolves to:
/// exact match, else the serialized `is_list`/`is_strict_list`; an empty
/// list falls back to `is_slow` 0x00661ae0.
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
        if !t.is_unit_type() {
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

/// `ObjectData::is(what, strict)` on the object at `(who, o)`.
fn obj_is(save: &Save, who: i32, o: i32, what: i32, strict: bool) -> bool {
    obj_ptype(save, who, o).map(|t| type_is(save, t, what, strict)).unwrap_or(false)
}

// ---------------------------------------------------------------------------
// LeaderData queries the cone needs
// ---------------------------------------------------------------------------

/// `LeaderData::has_wonder(int)` 0x006ebc10 for the air `mana` arm
/// (`has_wonder(0x21e)`): bit `w - 0x20e` of `conquest_wonders` gated by
/// `Constants+0xad4 <= enc epoch` gives 2; the owned-wonder scan over the
/// leader's wonder list (+0x424 count, `DAT_00c0a390[who]` entries) adds
/// 1. That list is not walked, so a leader with `wonder_mark > 0` is a
/// stop (`None`).
fn has_wonder(save: &Save, who: i32, w: i32) -> Option<i32> {
    if !(0x20e..=0x21e).contains(&w) {
        return Some(0);
    }
    let l = save.leaders.slots.get(usize::try_from(who).ok()?)?;
    let b = (w - 0x20e) as usize;
    let conquest = l.conquest_wonders.data.get(b >> 3).is_some_and(|x| x & (1 << (b & 7)) != 0);
    if conquest {
        // `*(LeaderData+0x6eb8)` is the leader's `Encrypted` block; +0xec ^ 0x63187 = epoch.
        // Not resolved here (the Encrypted image lives in leaders_process);
        // conquest wonders never occur outside Conquer-the-World.
        return None;
    }
    if leader_i32(save, who, 0x424)? > 0 {
        return None;
    }
    Some(0)
}

/// `LeaderData::get_heal_level` 0x006e12e0: `Σ has_preq(t)` for `t ∈
/// {0x2ef, 0x2f0, 0x2f1}` (the three heal bonus cards; the inlined
/// `t == 0x2ad → has_tribe_bonus(4)` arm is dead for this range).
///
/// `LeaderData::has_preq` 0x006db810 for a bonus type walks the two
/// `preq[]` slots (TypeData head +0x2c/+0x30) through `has_tech`, then
/// returns 1 for every non-government type (the `is_gov_type` vtable
/// arm at 0x006dbbf7: `t < 0x26f || t > 0x274 → 1`). The only cases
/// resolvable without the `leaders_process` evaluator are the trivially
/// constant preqs (`-1` always true, `-2` always false, any other negative
/// is `has_tech(-n)` = true); a card with a real preq is a stop (`None`).
fn heal_level(save: &Save, who: i32) -> Option<i32> {
    let _ = save.leaders.slots.get(usize::try_from(who).ok()?)?;
    let mut n = 0;
    for t in 0x2ef..=0x2f1 {
        let rec = type_rec(save, t)?;
        let mut ok = true;
        for i in 0..2 {
            let p = rec.i32(0x2c + i * 4)?;
            match p {
                -1 => {}
                -2 => ok = false,
                p if p < 0 => {}
                _ => return None, // TODO(va 0x006db810) real preq -> has_tech(p): leaders_process::Eval
            }
        }
        if ok {
            n += 1;
        }
    }
    Some(n)
}

// ---------------------------------------------------------------------------
// UnitData::mana 0x00609a50
// ---------------------------------------------------------------------------

/// `UnitData::mana()` — the fuel cap. `UnitType+0x2ec` (`mana`); 0 when
/// the type has none. Air domain: `has_wonder(0x21e)` → `(Constants+0x55c
/// + 100) * mana / 100`. Land/sea domain: `is_supply` (flags2 & 0x40) →
/// `mana *= get_supply_upgrade() + 1`; then `has_tribe_bonus(10) &&
/// is(0x36, 1)` → `(Constants+0x69c + 100) * mana / 100`. The land arm
/// needs `LeaderData::get_supply_upgrade` 0x006e0880 / `has_tribe_bonus`
/// 0x006e1370 and is a stop (`None`) unless the type is not supply and the
/// wonder/tribe gates cannot fire.
pub fn mana(save: &Save, who: i32, ptype: i32) -> Option<i32> {
    let base = type_rec(save, ptype)?.i32(0x2ec)?;
    if base == 0 {
        return Some(0);
    }
    if type_domain(save, ptype) == 2 {
        if has_wonder(save, who, 0x21e)? == 0 {
            return Some(base);
        }
        return Some((constant(save, 0x55c) + 100).wrapping_mul(base) / 100);
    }
    // TODO(va 0x006e0880, 0x006e1370) supply upgrade / tribe bonus 10 for the land arm.
    None
}

// ---------------------------------------------------------------------------
// Unit::process air-fuel branch 0x00611050..0x00611158
// ---------------------------------------------------------------------------

/// What the branch did, for the caller's effect log and the tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AirFuel {
    /// Not an air-domain unit, or the branch was not reached.
    NotAir,
    /// Airborne (`inside_up < 0`): `mana_burn += 1` while `mana() - mana_burn > 0`; `healing = 0`.
    Airborne { burned: bool },
    /// Hosted: `mana_burn` decayed by `Constants+0xa0`; periodic `repair_damage(1,1,1)`
    /// fired (`healing = 1`) or not.
    Hosted { repaired: bool },
    /// Hosted, decay written, but the heal period needs `get_heal_level` (stop).
    HostedNoHealLevel,
}

/// `Unit::process` 0x00610bc0, the `domain == 2` arm at 0x00611054 (reached
/// when `unit_masks & 1 == 0` and `!(flags2 & 2)`):
///
/// ```text
/// 0061105d  if (inside_up < 0) {                                  // airborne
/// 00611129      left = max(mana() - mana_burn, 0);
/// 00611148      if (left != 0) mana_burn += 1;
/// 00611152      healing = 0;
///           } else {                                               // on a host
/// 0061107b      if (mana_burn > Constants+0xa0) mana_burn -= (i16)Constants+0xa0; else mana_burn = 0;
/// 006110ac      period = Constants[0xc00 + 4*get_heal_level()];
/// 006110c5      if (is(0x130, 0)) period = (period*2 + 2) / 3;
/// 0061110c      if ((o + frame) % period != 0) goto LAB_00611158;  // healing untouched
/// 0061111c      repair_damage(1, 1, 1);                           // vtable +0x168
/// 00611154      healing = 1;
///           }
/// ```
///
/// `Unit::repair_damage` 0x0060de10 writes `damage`/`damage_frac` (+0x24/
/// +0x3b) and is a stop here; the `healing` write is performed.
pub fn air_fuel_step(save: &Save, u: &mut Unit, tag: &str, effects: &mut Vec<String>) -> AirFuel {
    let mut img = UImg(u);
    if !img.complete() {
        return AirFuel::NotAir;
    }
    let who = img.who();
    let ptype = img.ptype();
    if type_domain(save, ptype) != 2 {
        return AirFuel::NotAir;
    }
    let burn = img.i16(0x96);
    if img.i16(0x82) < 0 {
        let cap = mana(save, who, ptype).unwrap_or(0);
        let left = (cap - burn as i32).max(0);
        let burned = left != 0;
        if burned {
            img.set_i16(0x96, burn.wrapping_add(1));
            effects.push(format!("{tag}.Unit.mana_burn {burn} -> {}", burn.wrapping_add(1)));
        }
        let healing = img.i16(0x38);
        img.set_i16(0x38, 0);
        if healing != 0 {
            effects.push(format!("{tag}.Object.healing {healing} -> 0"));
        }
        return AirFuel::Airborne { burned };
    }
    let decay = constant(save, 0xa0);
    let nb = if (burn as i32) > decay { burn.wrapping_sub(decay as i16) } else { 0 };
    if nb != burn {
        img.set_i16(0x96, nb);
        effects.push(format!("{tag}.Unit.mana_burn {burn} -> {nb}"));
    }
    let Some(level) = heal_level(save, who) else {
        effects.push(format!("{tag}: air repair period needs LeaderData::get_heal_level 0x006e12e0 (stop)"));
        return AirFuel::HostedNoHealLevel;
    };
    let mut period = constant(save, 0xc00 + 4 * level.max(0) as usize);
    if type_is(save, ptype, 0x130, false) {
        period = (period * 2 + 2) / 3;
    }
    if period == 0 || (img.o() + frame(save)) % period != 0 {
        return AirFuel::Hosted { repaired: false };
    }
    // TODO(va 0x0060de10) Unit::repair_damage(1, 1, 1): damage/damage_frac untouched.
    effects.push(format!("{tag}: Unit::repair_damage(1,1,1) 0x0060de10 not transcribed"));
    let healing = img.i16(0x38);
    img.set_i16(0x38, 1);
    if healing != 1 {
        effects.push(format!("{tag}.Object.healing {healing} -> 1"));
    }
    AirFuel::Hosted { repaired: true }
}

// ---------------------------------------------------------------------------
// UnitData::is_flying_low 0x0060a140 / is_flying_high 0x0060a310
// ---------------------------------------------------------------------------

/// `UnitData::is_flying_low`: false unless `domain == 2 && !(unit_flags &
/// 0x20) && !(obj_masks & 0x8000000) && is_on_map` (`inside_up < 0`, the
/// sign bit of +0x82). Then by current order type: 0 → false; 0x10
/// (strafe) → true when the target `(ox, whom)` is live and within 0x900;
/// 0x18 (air attack ground) → true within 0x900 of `att_x/att_y`; any
/// other order falls through to the caster-stance target check
/// (`get_caster_stance()->+0x18`, a stance target within 0x900). The
/// order-target arms need the live target coordinates and are returned as
/// `None` when the order is not 0; `Some(false)` is exact for an idle
/// plane and for every non-plane.
pub fn is_flying_low(save: &Save, u: &Unit) -> Option<bool> {
    let ptype = i32::from_le_bytes(u.base.sub.body[15..19].try_into().unwrap());
    if type_domain(save, ptype) != 2 || type_unit_flags(save, ptype) & 0x20 != 0 || type_obj_masks(save, ptype) & 0x8000000 != 0 {
        return Some(false);
    }
    let inside_up = i16::from_le_bytes([u.body[0x82 - 0x48], u.body[0x83 - 0x48]]);
    if inside_up >= 0 {
        return Some(false);
    }
    match u.orders.orders.first().map(|o| o.ty).unwrap_or(0) {
        0 => Some(false),
        // TODO(va 0x0060a1a0..0x0060a2f0) strafe / attack-ground / stance-target range arms.
        _ => None,
    }
}

/// `UnitData::is_flying_high`: a fixed-wing plane on the map that is not
/// flying low.
pub fn is_flying_high(save: &Save, u: &Unit) -> Option<bool> {
    let ptype = i32::from_le_bytes(u.base.sub.body[15..19].try_into().unwrap());
    if type_domain(save, ptype) != 2 || type_unit_flags(save, ptype) & 0x20 != 0 || type_obj_masks(save, ptype) & 0x8000000 != 0 {
        return Some(false);
    }
    let inside_up = i16::from_le_bytes([u.body[0x82 - 0x48], u.body[0x83 - 0x48]]);
    if inside_up >= 0 {
        return Some(false);
    }
    is_flying_low(save, u).map(|l| !l)
}

// ---------------------------------------------------------------------------
// Ammo::init 0x0067bbf0 — anti-air dud gate 0x0067be49..0x0067c16a
// ---------------------------------------------------------------------------

/// Result of the gate: whether `Ammo.flags |= 0x10` (no-damage dud) and
/// how many `Random::get(0, 0xffff)` draws it consumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DudVerdict {
    pub dud: bool,
    pub draws: u8,
}

/// Transcription of the gate (shooter `(s_who, s_o)` = Ammo +0x3c/+0x40,
/// target `(t_who, t_o)` = Ammo +0x48/+0x4c), entered only when
/// `!(shooter.is_unit && order_type ∈ {0x17, 0x18})`:
///
/// ```text
/// 0067bef8  if (t_o < 0 || t_who < 0) return;              // init aborted (0x0067cfdc)
/// 0067bf23  if (!(target.flags & 1)) return;
/// 0067bf32  if (!target.is_unit()) -> no gate
/// 0067bf5a  if (target.domain != 2) -> no gate
/// 0067bf74  if (target.unit_flags & 0x20) -> no gate      // helicopter
/// 0067bf81  if (target.obj_masks & 0x8000000) -> no gate  // missile
/// 0067bfb9  if (shooter.has_objmask(0x80000000)) {         // anti-air shooter
/// 0067bfe1      if (shooter.domain == 2) -> no gate
/// 0067c007      if (target.is_flying_low())  { r = rng; dud = r % 100 >= shooter.type+0x254 }
///               else                         { r = rng; dud = r % 100 >= shooter.type+0x250 }
///           } else {                                        // ordinary shooter
/// 0067c074      if (target.is_flying_low()) {
/// 0067c08a          r = rng; if (r % 100 >= target.type+0x254) dud
/// 0067c0c7          else { r = rng; dud = r % 100 >= shooter.type+0x254 }
///               } else {
/// 0067c0fa          r = rng; if (r % 100 >= target.type+0x250) dud
/// 0067c133          else { r = rng; dud = r % 100 >= shooter.type+0x250 }
///               }
///           }
/// 0067c166  dud: Ammo.flags |= 0x10
/// ```
///
/// `UnitData::is_flying_low` is evaluated on the *target*
/// (`ecx = Objects[t_who][t_o]` at 0x0067c004 / 0x0067c071). The caller
/// passes it in because its order-target arm is not transcribed; `None`
/// when the gate was not entered (`Some(DudVerdict{dud:false,draws:0})`
/// means entered and passed).
pub fn antiair_dud_gate(
    save: &mut Save,
    shooter: (i32, i32),
    target: (i32, i32),
    shooter_is_unit_with_air_order: bool,
    target_flying_low: bool,
) -> Option<DudVerdict> {
    if shooter_is_unit_with_air_order {
        return None;
    }
    let (t_who, t_o) = target;
    if t_o < 0 || t_who < 0 || !obj_live(save, t_who, t_o) {
        return None;
    }
    if !obj_is_unit(save, t_who, t_o)? {
        return Some(DudVerdict { dud: false, draws: 0 });
    }
    let tt = obj_ptype(save, t_who, t_o)?;
    if type_domain(save, tt) != 2 || type_unit_flags(save, tt) & 0x20 != 0 || type_obj_masks(save, tt) & 0x8000000 != 0 {
        return Some(DudVerdict { dud: false, draws: 0 });
    }
    let st = obj_ptype(save, shooter.0, shooter.1)?;
    let pct = |save: &Save, ty: i32, off: usize| type_rec(save, ty).and_then(|t| t.i32(off)).unwrap_or(0);
    // ObjectData::has_objmask 0x0046cf10 / BuildData::has_objmask 0x0062e210: `obj_masks & m`.
    if type_obj_masks(save, st) & 0x80000000 != 0 {
        if type_domain(save, st) == 2 {
            return Some(DudVerdict { dud: false, draws: 0 });
        }
        let off = if target_flying_low { 0x254 } else { 0x250 };
        let r = game_random(save, 0, 0xffff);
        return Some(DudVerdict { dud: r % 100 >= pct(save, st, off), draws: 1 });
    }
    let off = if target_flying_low { 0x254 } else { 0x250 };
    let r = game_random(save, 0, 0xffff);
    if r % 100 >= pct(save, tt, off) {
        return Some(DudVerdict { dud: true, draws: 1 });
    }
    let r = game_random(save, 0, 0xffff);
    Some(DudVerdict { dud: r % 100 >= pct(save, st, off), draws: 2 })
}

// ---------------------------------------------------------------------------
// Host capacity: can_carry / num_aircraft_limit / num_aircraft_here / num_inside
// ---------------------------------------------------------------------------

/// `ObjectData::can_carry(DomainIndex)` 0x00646c40 on the host `(who, o)`:
///
/// ```text
/// domain 2: is(0x1bf,1) || is(0x15f,1) || is(0x208,1)            // airbase, carrier, silo
/// domain 0: is_unit() && type+0x2d4 != 0 && !is(0x15f,0)         // transports (capacity field), not carriers
/// domain 1: false
/// ```
pub fn can_carry_domain(save: &Save, who: i32, o: i32, domain: i32) -> bool {
    let Some(ty) = obj_ptype(save, who, o) else { return false };
    if domain == 2 {
        return type_is(save, ty, 0x1bf, true) || type_is(save, ty, 0x15f, true) || type_is(save, ty, 0x208, true);
    }
    if domain != 0 {
        return false;
    }
    obj_is_unit(save, who, o).unwrap_or(false)
        && type_rec(save, ty).and_then(|t| t.i32(0x2d4)).unwrap_or(0) != 0
        && !type_is(save, ty, 0x15f, false)
}

/// `ObjectData::num_aircraft_limit` 0x006454a0 on the host: `is(0x15f,1)`
/// → `Constants+0xa4`; `is(0x1bf,1)` → `Constants+0xa8`; `is(0x208,1)` →
/// 1; else 0.
pub fn num_aircraft_limit(save: &Save, who: i32, o: i32) -> i32 {
    let Some(ty) = obj_ptype(save, who, o) else { return 0 };
    if type_is(save, ty, 0x15f, true) {
        return constant(save, 0xa4);
    }
    if type_is(save, ty, 0x1bf, true) {
        return constant(save, 0xa8);
    }
    type_is(save, ty, 0x208, true) as i32
}

/// `ObjectData::inside(&who)` 0x00651a80 on a unit: its outermost host —
/// follow `inside_up`/`inside_up_who` (+0x82/+0xb4) while the host is
/// itself a unit with an `inside_up >= 0` (a plane on a carrier that is
/// itself carried). `(-1, -1)` when not a unit or not hosted.
fn inside(save: &Save, who: i32, o: i32) -> (i32, i32) {
    if obj_is_unit(save, who, o) != Some(true) {
        return (-1, -1);
    }
    let Some(b) = unit_body(save, who, o) else { return (-1, -1) };
    let mut up = i16::from_le_bytes([b[0x82 - 0x48], b[0x83 - 0x48]]) as i32;
    let mut up_who = b[0xb4 - 0x48] as i8 as i32;
    if up < 0 {
        return (-1, -1);
    }
    for _ in 0..64 {
        // `Objects[up_who][up].is_unit()` (vtable +0x18): hop again only through units.
        if obj_is_unit(save, up_who, up) != Some(true) {
            break;
        }
        let Some(hb) = unit_body(save, up_who, up) else { break };
        let nup = i16::from_le_bytes([hb[0x82 - 0x48], hb[0x83 - 0x48]]) as i32;
        if nup < 0 {
            break;
        }
        up_who = hb[0xb4 - 0x48] as i8 as i32;
        up = nup;
    }
    (up, up_who)
}

/// `ObjectData::can_carry(TypeIndex t)` 0x00645e00 on the host `(who, o)`:
/// may a *type* `t` be hosted here?
///
/// ```text
/// if (!is_unit_type(t)) return 0;                         // 0x32..=0x19d
/// if (this.is(0x208,0))  return types[t].+0x114() && !this.has_nuke();      // silo: stop (ObjectData::has_nuke 0x00643d40)
/// if (this.is(0x1bf,0))  return types[t].domain == 2 && !(obj_masks & 0x8000000) && !types[t].is(0x134,0);
/// if (this.is(0x15f,0))  return types[t].is(0x134,0);
/// return types[t].domain == 0 && this.is_unit() && this.type.+0x2d4 != 0;  // land transport
/// ```
pub fn can_carry_type(save: &Save, who: i32, o: i32, t: i32) -> Option<bool> {
    if !(0x32..=0x19d).contains(&t) {
        return Some(false);
    }
    let ht = obj_ptype(save, who, o)?;
    if type_is(save, ht, 0x208, false) {
        // TODO(va 0x00643d40) silo arm: ObjectType vtable +0x114 predicate && !has_nuke().
        return None;
    }
    if type_is(save, ht, 0x1bf, false) {
        return Some(type_domain(save, t) == 2 && type_obj_masks(save, t) & 0x8000000 == 0 && !type_is(save, t, 0x134, false));
    }
    if type_is(save, ht, 0x15f, false) {
        return Some(type_is(save, t, 0x134, false));
    }
    Some(type_domain(save, t) == 0 && obj_is_unit(save, who, o)? && type_rec(save, ht).and_then(|r| r.i32(0x2d4)).unwrap_or(0) != 0)
}

/// `UnitData::home_base(&who)` 0x00609dc0 on the unit `(who, o)`:
///
/// ```text
/// (h, hw) = inside(&who); if (h >= 0) return (h, hw);
/// if (orderlist.length) { cur = head.next;
///     if (cur.get_type() == 0x19) { b = cur->get_board_order(); if (b.+8 == 0) return b.+0x1c; … }   // stop
///     if (cur.is_air()) { air = cur->get_air_order(); *who = air.whose;
///         if (air.oxx >= 0 && Objects[whose][oxx].is_valid() && host.can_carry(this.type)) return air.oxx; } }
/// return -1
/// ```
///
/// `None` when the order arm needs the untranscribed 0x19 body or the silo
/// `can_carry` arm.
pub fn home_base(save: &Save, who: i32, o: i32) -> Option<(i32, i32)> {
    let (h, hw) = inside(save, who, o);
    if h >= 0 {
        return Some((h, hw));
    }
    let u = unit_ref(save, who, o)?;
    let Some(cur) = u.orders.orders.first() else { return Some((-1, -1)) };
    if cur.ty == 0x19 {
        // TODO(va 0x00609e30) board-order arm.
        return None;
    }
    let Some(air) = air_order(cur.ty, &cur.payload) else { return Some((-1, -1)) };
    let ty = i32::from_le_bytes(u.base.sub.body[15..19].try_into().unwrap());
    if air.oxx >= 0 && obj_live(save, air.whose, air.oxx) && can_carry_type(save, air.whose, air.oxx, ty)? {
        return Some((air.oxx, air.whose));
    }
    Some((-1, air.whose))
}

/// `ObjectData::num_aircraft_here(int with_queue)` 0x00645330 on the host:
/// counts live air-domain units of `who` (slots `0..unit_mark[who]`, the
/// `DAT_00c0accc[who]` length) whose `UnitData::home_base(&w)` 0x00609dc0
/// is `(who, o)`; then `is(0x15f, 0)` (carrier) adds `num_queued` (+0xa0),
/// else `is_build() && with_queue` adds `count_queue(2, 0) - count_queue(1,
/// 0x136)` (BuildData::count_queue 0x0062dee0, vtable +0x190) — a stop.
pub fn num_aircraft_here(save: &Save, who: i32, o: i32, with_queue: bool) -> Option<i32> {
    let mut n = 0;
    let mark = unit_mark(save, who).max(0);
    for i in 0..mark {
        if !obj_live(save, who, i) {
            continue;
        }
        let Some(ty) = obj_ptype(save, who, i) else { continue };
        if type_domain(save, ty) != 2 {
            continue;
        }
        if home_base(save, who, i)? == (o, who) {
            n += 1;
        }
    }
    let ty = obj_ptype(save, who, o)?;
    if type_is(save, ty, 0x15f, false) {
        return Some(n + unit_i16(save, who, o, 0xa0)? as i32);
    }
    if !obj_is_unit(save, who, o)? && with_queue {
        // TODO(va 0x0062dee0) BuildData::count_queue(2,0) - count_queue(1,0x136).
        return None;
    }
    Some(n)
}

/// `ObjectData::can_carry(int o, int who)` 0x006483c0 — may the host
/// `this` take the object `(o, who)`? Transcribed:
///
/// ```text
/// if ((o, who) == this || who != this.who) return false;         // own objects only
/// cargo = Units[who][o]; ct = cargo.type
/// if (ct.domain != 2) {
///     if (ct.domain == 1) return false;
///     if (!cargo.is_unit() || !this.is_unit() || this.is(0x15f,0)) return false;
///     return ObjectData::get_cargo_size() + ct.+0x2d8 < this.type.+0x2d4;   // 0x0064dcd0, stop
/// }
/// if (!(ct.obj_masks & 0x8000000)) {                              // plane
///     if (cargo.is(0x136,0)) { if (!this.is(0x1bf,0)) return false; goto cap; }
///     if (this.is(0x208,0)) return false;
/// } else {                                                        // missile
///     if (this.is(0x1bf)) return false;
///     if (this.is(0x15f,0)) return false;
/// }
/// cap:
/// if (cargo.is(0x134,0)) { if (!this.is(0x15f,0)) return false; } else if (this.is(0x15f,0)) return false;
/// if (cargo.home_base() == this) return true;
/// return this.num_aircraft_here(0) < this.num_aircraft_limit();
/// ```
pub fn can_carry_object(save: &Save, host: (i32, i32), cargo: (i32, i32)) -> Option<bool> {
    let (h_who, h_o) = host;
    let (c_who, c_o) = cargo;
    if (c_o == h_o && c_who == h_who) || c_who != h_who {
        return Some(false);
    }
    let ct = obj_ptype(save, c_who, c_o)?;
    let ht = obj_ptype(save, h_who, h_o)?;
    let dom = type_domain(save, ct);
    if dom != 2 {
        if dom == 1 {
            return Some(false);
        }
        if !obj_is_unit(save, c_who, c_o)? || !obj_is_unit(save, h_who, h_o)? || type_is(save, ht, 0x15f, false) {
            return Some(false);
        }
        // TODO(va 0x0064dcd0) ObjectData cargo size + ct+0x2d8 < ht+0x2d4.
        return None;
    }
    if type_obj_masks(save, ct) & 0x8000000 == 0 {
        if type_is(save, ct, 0x136, false) {
            // `goto LAB_006484e6` skips the silo test below.
            if !type_is(save, ht, 0x1bf, false) {
                return Some(false);
            }
        } else if type_is(save, ht, 0x208, false) {
            return Some(false);
        }
    } else if type_is(save, ht, 0x1bf, false) || type_is(save, ht, 0x15f, false) {
        return Some(false);
    }
    let host_is_carrier = type_is(save, ht, 0x15f, false);
    if type_is(save, ct, 0x134, false) {
        if !host_is_carrier {
            return Some(false);
        }
    } else if host_is_carrier {
        return Some(false);
    }
    if home_base(save, c_who, c_o)? == (h_o, h_who) {
        return Some(true);
    }
    Some(num_aircraft_here(save, h_who, h_o, false)? < num_aircraft_limit(save, h_who, h_o))
}

/// `ObjectData::num_inside(int any)` 0x00646d50 on the host: 0 unless
/// `is_on_map`; walks the containment chain from `inside_down`; each
/// `is_captain()` member counts 1 when `any != 0`; otherwise a unit member
/// counts 1 if `is_gov_hero` (flags2 & 0x4000000 via vtable +0xc8), else
/// `unit_masks & 1 ? 0 : type+0x2f0` (its population/cargo size).
pub fn num_inside(save: &Save, who: i32, o: i32, any: bool) -> Option<i32> {
    let body = unit_body(save, who, o);
    // Build hosts: `is_on_map` (vtable +0xbc) is `return 1`; units: inside_up sign.
    let on_map = match body {
        Some(b) => i16::from_le_bytes([b[0x82 - 0x48], b[0x83 - 0x48]]) < 0,
        None => obj_is_unit(save, who, o).map(|u| !u)?,
    };
    if !on_map {
        return Some(0);
    }
    let mut n = 0;
    let (mut c_o, mut c_who) = inside_down(save, who, o)?;
    let mut guard = 0;
    while c_o >= 0 {
        guard += 1;
        if guard > 4096 {
            return None;
        }
        // UnitData::is_captain 0x0046ceb0 (vtable +0xe8): `o_up < 0`.
        let is_captain = match unit_i16(save, c_who, c_o, 0x8e) {
            Some(v) => v < 0,
            None => true, // Build: vtable +0xe8 = return 1
        };
        if is_captain {
            if any {
                n += 1;
            } else if obj_is_unit(save, c_who, c_o)? {
                let ty = obj_ptype(save, c_who, c_o)?;
                if type_unit_flags(save, ty) & 0x4000000 != 0 {
                    n += 1;
                } else if unit_u32(save, c_who, c_o, 0x68)? & 1 == 0 {
                    n += type_rec(save, ty).and_then(|t| t.i32(0x2f0)).unwrap_or(0);
                }
            }
        }
        let next = inside_down(save, c_who, c_o)?;
        c_o = next.0;
        c_who = next.1;
    }
    Some(n)
}

// ---------------------------------------------------------------------------
// Build::process_ejection 0x006201e0
// ---------------------------------------------------------------------------

/// `Build::process_ejection` — run while `build_masks & 0x4000` (eject
/// pending) on an inactive or active site:
///
/// ```text
/// 006201ed  if (inside_down < 0) { build_masks &= ~0x4000; return; }
/// 00620201  u = Objects[inside_down_who][inside_down];
/// 00620213  captain = u.o_up < 0 ? u.o : Objects[u.who][u.o_up].get_captain();   // UnitData::get_captain 0x00610ab0
/// 00620253  Objects[inside_down_who][captain].come_out(1);                        // Unit::come_out 0x00617c10
/// 00620258  if (!(flags & 1)) hold_frames += 1;
/// ```
///
/// `come_out` (9,925 B; two `Random::get` draws on its path) is the stop:
/// the ejected unit's relocation, `inside_up/_down` relinking and order
/// reset are untouched; the `hold_frames` increment and the mask clear are
/// performed.
pub fn process_ejection(save: &Save, b: &mut Build, tag: &str, effects: &mut Vec<String>) {
    let mut img = BImg(b);
    if !img.complete() {
        return;
    }
    let inside_down = img.i16(0x28) as i32;
    if inside_down < 0 {
        let bm = img.u16(0x60);
        img.set_u16(0x60, bm & !0x4000);
        effects.push(format!("{tag}.Build.build_masks {bm:#x} -> {:#x}", bm & !0x4000));
        return;
    }
    let who = img.i8(0x3e) as i32;
    let captain = get_captain(save, who, inside_down);
    effects.push(format!("{tag}: Unit::come_out(1) 0x00617c10 on Objects[{who}][{captain:?}] not transcribed"));
    if img.u8(0x08) & 1 == 0 {
        let h = img.i16(0x32);
        img.set_i16(0x32, h.wrapping_add(1));
        effects.push(format!("{tag}.Object.hold_frames {h} -> {}", h.wrapping_add(1)));
    }
}

/// `UnitData::get_captain` 0x00610ab0: `o_up < 0 ? o : Objects[who][o_up].get_captain()`.
/// Builds (`ObjectData::get_captain` 0x00472400) return their own `o`.
fn get_captain(save: &Save, who: i32, o: i32) -> Option<i32> {
    let mut cur = o;
    for _ in 0..64 {
        match unit_i16(save, who, cur, 0x8e) {
            Some(up) if up >= 0 => cur = up as i32,
            Some(_) => return Some(cur),
            None => return obj_sub(save, who, cur).map(|_| cur),
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Object::do_launch 0x0064f3b0
// ---------------------------------------------------------------------------

/// `Object::do_launch` — the host side of the launch handshake, reached
/// from `Unit::work` (`o >= 0 && inside_up < 0 && can_carry(2)`) and
/// `Build::process` (`build_masks & 8`). Head and containment-chain walk
/// transcribed:
///
/// ```text
/// 0064f3f8  if (inside_down < 0 || inside_down_who < 0) return;
/// 0064f40c  if (is_build() && Build.recharging != 0) { Build::do_missile_launch(); return; }   // 0x00622670, stop
/// 0064f43e  lf = launch_frames; launch_frames = lf + 1;
/// 0064f44a  if (lf < FRAMES_BETWEEN_LAUNCHES) return;              // 15
/// 0064f452  launch_frames = FRAMES_BETWEEN_LAUNCHES;
///           launched = false;
///           for (u in chain(inside_down_who, inside_down)) {
/// 0064f47c    if (u.orderlist.length == 0 || u.mana_burn != 0) continue;   // not ready
/// 0064f49a    if (!(this.has_repeat_air() || u.get_order()->flags & 4)) {
///                 u.kill_current_order(0); launching.remove(u.o); continue;  // 0x005e2cb0, stop
///             }
/// 0064f4c0    if (u.order_type() == 0x10) {                          // STRAFE
///                 s = strafe order; if (s.ox < 0) {                   // no unit target: re-home a full base
///                     if ((s.oxx, s.whose) != this && both >= 0 && Objects[whose][oxx].num_aircraft_here(0) >= .num_aircraft_limit())
///                         u.add_air_patrol_order(host.x, host.y, this.o, this.who, 1, …);   // 0x005e4350, stop
///                 } else if (launching.find(u.o) < 0 && !valid_target(s.ox, s.whom, 0) && !World.valid(s.xx, s.yy)) {
///                     u.kill_current_order(0); continue;              // stop
///                 }
///             }
/// 0064f6b1    if (u.order_type() == 0x11) patrol.waypoint = 0;        // AIR_PATROL
/// 0064f6c4    launching ??= new SimpleArray<int>; if (launching.find(u.o) == -1) launching.add(u.o);
/// 0064f73e    if (!launched) {
///                 if (!(u.type.obj_masks & 0x8000000)) { if (!this.is_unit()) u.come_out(0); }   // 0x00617c10, stop
///                 else Build.recharging = this.type+0x1f4   (Unit host: recharge(0), vtable +0x134)
///                 launch_frames = 0;
///             }
///             launched = true;
///           }
/// 0064f81c  AI auto-target (LeaderData[who].flags & 4 == 0 && (frame + o) & 0x1f == 0 && !(flags2 & 8)): stop
/// ```
///
/// Performed writes: `launch_frames`, `launching` (add/remove), the
/// AIR_PATROL `waypoint` reset on the contained unit's order payload, and
/// `Build.recharging` for a missile host. The AI scans (airbase: enemy
/// cities without AA tech 0x132 / AA building 0x20b within 0x900, forts,
/// wonders; silo at `(frame + o) & 0x7f == 0`: the same lists with the
/// `% 10` `Random::get` draw per non-capital city and the `vector_dist <=
/// mana * speed` range gate) are the stop; they read the LeaderData AI
/// lists at +0x408/+0x424/+0x428/+0x6a4c whose pointers are not walked.
pub fn do_launch(save: &mut Save, owner: usize, slot: usize, effects: &mut Vec<String>) {
    let tag = format!("Objects.lists[{owner}][{slot}]");
    let Some(Some(host)) = save.objects.lists.get(owner).and_then(|l| l.elems.get(slot)) else { return };
    let (mid, is_build, h_who, h_o) = match host {
        Obj::Build(b) => (&b.base.mid, true, b.base.sub.body[0] as i32, i16::from_le_bytes([b.base.sub.body[1], b.base.sub.body[2]]) as i32),
        Obj::Unit(u) => (&u.base.mid, false, u.base.sub.body[0] as i32, i16::from_le_bytes([u.base.sub.body[1], u.base.sub.body[2]]) as i32),
        Obj::Animal(_) => return,
    };
    if mid.len() != 34 {
        return;
    }
    let inside_down = i16::from_le_bytes([mid[0x08], mid[0x09]]) as i32;
    let inside_down_who = mid[0x1e] as i8 as i32;
    if inside_down < 0 || inside_down_who < 0 {
        return;
    }
    if is_build {
        if let Some(Some(Obj::Build(b))) = save.objects.lists[owner].elems.get(slot) {
            if b.body.len() == 22 && i16::from_le_bytes([b.body[0x7a - 0x70], b.body[0x7b - 0x70]]) != 0 {
                // TODO(va 0x00622670) Build::do_missile_launch: recharging countdown,
                // launching.remove, come_out(0) + process() on the missile.
                effects.push(format!("{tag}: Build::do_missile_launch 0x00622670 not transcribed"));
                return;
            }
        }
    }
    let lf = mid[0x21] as i8;
    set_launch_frames(save, owner, slot, lf.wrapping_add(1));
    if lf < FRAMES_BETWEEN_LAUNCHES {
        effects.push(format!("{tag}.Object.launch_frames {lf} -> {}", lf.wrapping_add(1)));
        return;
    }
    set_launch_frames(save, owner, slot, FRAMES_BETWEEN_LAUNCHES);
    if lf != FRAMES_BETWEEN_LAUNCHES {
        effects.push(format!("{tag}.Object.launch_frames {lf} -> {FRAMES_BETWEEN_LAUNCHES}"));
    }
    // has_repeat_air: Unit `unit_masks & 0x200000` (0x0046cec0); Build `build_masks & 0x80` (0x00472410).
    let repeat = match &save.objects.lists[owner].elems[slot] {
        Some(Obj::Unit(u)) => u.body.len() == 111 && u32::from_le_bytes(u.body[0x68 - 0x48..0x6c - 0x48].try_into().unwrap()) & 0x200000 != 0,
        Some(Obj::Build(b)) => b.wall_body.len() == 30 && u16::from_le_bytes([b.wall_body[0x60 - 0x48], b.wall_body[0x61 - 0x48]]) & 0x80 != 0,
        _ => false,
    };
    let host_ptype = obj_ptype(save, h_who, h_o);
    let mut launched = false;
    let (mut c_o, mut c_who) = (inside_down, inside_down_who);
    let mut guard = 0;
    while c_o >= 0 && c_who >= 0 {
        guard += 1;
        if guard > 4096 {
            break;
        }
        let Some(next) = inside_down(save, c_who, c_o) else { break };
        let utag = format!("Objects.lists[{c_who}][{c_o}]");
        let ready = match unit_ref(save, c_who, c_o) {
            Some(u) if u.body.len() == 111 => !u.orders.orders.is_empty() && i16::from_le_bytes([u.body[0x96 - 0x48], u.body[0x97 - 0x48]]) == 0,
            _ => false,
        };
        if ready {
            let (oty, oflags) = unit_ref(save, c_who, c_o).map(|u| (u.orders.orders[0].ty, u.orders.orders[0].payload.first().copied().unwrap_or(0))).unwrap();
            if repeat || oflags & 4 != 0 {
                let mut skip = false;
                if oty == 0x10 {
                    let p = &unit_ref(save, c_who, c_o).unwrap().orders.orders[0].payload;
                    if p.len() >= 57 {
                        let ox = i32::from_le_bytes(p[1..5].try_into().unwrap());
                        if ox < 0 {
                            let oxx = i32::from_le_bytes(p[25..29].try_into().unwrap());
                            let whose = i32::from_le_bytes(p[29..33].try_into().unwrap());
                            if (oxx != h_o || whose != h_who) && oxx >= 0 && whose >= 0 {
                                match num_aircraft_here(save, whose, oxx, false) {
                                    Some(n) if n >= num_aircraft_limit(save, whose, oxx) => {
                                        // TODO(va 0x005e4350) Unit::add_air_patrol_order(host.x, host.y, h_o, h_who, 1, …)
                                        effects.push(format!("{utag}: re-home to {tag} — add_air_patrol_order 0x005e4350 not transcribed"));
                                    }
                                    Some(_) => {}
                                    None => effects.push(format!("{utag}: num_aircraft_here on Objects[{whose}][{oxx}] needs home_base (stop)")),
                                }
                            }
                        } else if !launching_contains(save, owner, slot, c_o) {
                            // TODO(va 0x00648ba0, 0x0043f360) Object::valid_target(ox, whom, 0) || World.valid(xx, yy)
                            // else kill_current_order(0) and skip. Target liveness not resolved here.
                            effects.push(format!("{utag}: strafe target validity (valid_target 0x00648ba0) not transcribed"));
                            skip = true;
                        }
                    }
                }
                if !skip {
                    if oty == 0x11 {
                        reset_patrol_waypoint(save, c_who, c_o, &utag, effects);
                    }
                    if !launching_contains(save, owner, slot, c_o) {
                        launching_add(save, owner, slot, c_o);
                        effects.push(format!("{tag}.Object.launching += {c_o}"));
                    }
                    if !launched {
                        let cty = obj_ptype(save, c_who, c_o).unwrap_or(0);
                        if type_obj_masks(save, cty) & 0x8000000 == 0 {
                            if is_build {
                                // TODO(va 0x00617c10) Unit::come_out(0): the plane leaves the host.
                                effects.push(format!("{utag}: Unit::come_out(0) 0x00617c10 not transcribed"));
                            }
                        } else if is_build {
                            let rc = host_ptype.and_then(|t| type_rec(save, t)).and_then(|t| t.i32(0x1f4)).unwrap_or(0) as i16;
                            if let Some(Some(Obj::Build(b))) = save.objects.lists[owner].elems.get_mut(slot) {
                                let old = i16::from_le_bytes([b.body[0x7a - 0x70], b.body[0x7b - 0x70]]);
                                b.body[0x7a - 0x70..0x7c - 0x70].copy_from_slice(&rc.to_le_bytes());
                                effects.push(format!("{tag}.Build.recharging {old} -> {rc}"));
                            }
                        } else {
                            // TODO(va 0x0060fdf0) UnitData::recharge(0) -> Build.recharging on a unit host (not a serialized Unit field).
                        }
                        set_launch_frames(save, owner, slot, 0);
                        effects.push(format!("{tag}.Object.launch_frames -> 0"));
                    }
                    launched = true;
                }
            } else {
                // TODO(va 0x005e2cb0) Unit::kill_current_order(0).
                effects.push(format!("{utag}: Unit::kill_current_order(0) 0x005e2cb0 not transcribed"));
                if launching_remove(save, owner, slot, c_o) {
                    effects.push(format!("{tag}.Object.launching -= {c_o}"));
                }
            }
        }
        c_o = next.0;
        c_who = next.1;
    }
    // 0x0064f81c.. AI auto-target scans.
    if leader_flags(save, h_who) & 4 == 0 {
        let fr = frame(save);
        if (fr.wrapping_add(h_o)).rem_euclid(32) == 0 && leader_flags2(save, h_who) & 8 == 0 {
            let silo = host_ptype.map(|t| type_is(save, t, 0x208, false)).unwrap_or(false);
            if silo {
                if (fr.wrapping_add(h_o)).rem_euclid(128) == 0 {
                    // TODO(va 0x0064fa10..0x00650830) silo target scan: num_inside(1), is(0x13b)
                    // armageddon gate, one Random::get per non-capital enemy city (% 10), range
                    // gate vector_dist <= mana * speed, add_air_attack_ground_order 0x005e41c0.
                    effects.push(format!("{tag}: do_launch silo auto-target scan (Random::get consumer) not transcribed"));
                }
            } else {
                // TODO(va 0x0064f8b0..0x0064f9f0) airbase/carrier target scan over enemy
                // cities/forts/wonders, then clear_orders + add_air_patrol_order per plane.
                effects.push(format!("{tag}: do_launch airbase auto-target scan not transcribed"));
            }
        }
    }
}

fn set_launch_frames(save: &mut Save, owner: usize, slot: usize, v: i8) {
    let mid = match save.objects.lists[owner].elems.get_mut(slot) {
        Some(Some(Obj::Build(b))) => &mut b.base.mid,
        Some(Some(Obj::Unit(u))) => &mut u.base.mid,
        _ => return,
    };
    mid[0x21] = v as u8;
}

fn launching_vec(save: &Save, owner: usize, slot: usize) -> Option<Vec<i32>> {
    let base = match save.objects.lists.get(owner)?.elems.get(slot)?.as_ref()? {
        Obj::Build(b) => &b.base,
        Obj::Unit(u) => &u.base,
        Obj::Animal(a) => &a.unit.base,
    };
    if base.launch == 0 {
        return None;
    }
    Some(base.launching.data.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect())
}

fn launching_contains(save: &Save, owner: usize, slot: usize, o: i32) -> bool {
    launching_vec(save, owner, slot).map(|v| v.contains(&o)).unwrap_or(false)
}

/// `SimpleArray<int>::add` on `launching`, allocating it (`launch = 1`,
/// size 1, increment 1 — `malloc(0x20)` + `SimpleArray<int>(1)` at
/// 0x0064f6d2) when null. `ArrayBase::add` 0x0042daf0 grows by
/// `increment` when full.
fn launching_add(save: &mut Save, owner: usize, slot: usize, o: i32) {
    let base = match save.objects.lists[owner].elems.get_mut(slot) {
        Some(Some(Obj::Build(b))) => &mut b.base,
        Some(Some(Obj::Unit(u))) => &mut u.base,
        _ => return,
    };
    if base.launch == 0 {
        base.launch = 1;
        base.launching.len = 0;
        base.launching.cap = 1;
        base.launching.inc = 1;
        base.launching.data.clear();
    }
    base.launching.data.extend_from_slice(&o.to_le_bytes());
    base.launching.len += 1;
    if base.launching.len > base.launching.cap {
        base.launching.cap += base.launching.inc.max(1) as i32;
    }
}

/// `SimpleArray<int>::remove(value)` 0x00462e70: removes the first equal
/// element, shifting the tail down.
fn launching_remove(save: &mut Save, owner: usize, slot: usize, o: i32) -> bool {
    let base = match save.objects.lists[owner].elems.get_mut(slot) {
        Some(Some(Obj::Build(b))) => &mut b.base,
        Some(Some(Obj::Unit(u))) => &mut u.base,
        _ => return false,
    };
    if base.launch == 0 {
        return false;
    }
    let pos = base.launching.data.chunks_exact(4).position(|c| i32::from_le_bytes(c.try_into().unwrap()) == o);
    let Some(p) = pos else { return false };
    base.launching.data.drain(p * 4..p * 4 + 4);
    base.launching.len -= 1;
    true
}

/// `AirPatrolOrder.waypoint` (payload `[1..5)`) `= 0` on the unit's current order.
fn reset_patrol_waypoint(save: &mut Save, who: i32, o: i32, utag: &str, effects: &mut Vec<String>) {
    let Some(l) = save.objects.lists.get_mut(usize::try_from(who).unwrap_or(usize::MAX)) else { return };
    let Some(Some(Obj::Unit(u))) = l.elems.get_mut(usize::try_from(o).unwrap_or(usize::MAX)) else { return };
    let Some(ord) = u.orders.orders.first_mut() else { return };
    if ord.ty != 0x11 || ord.payload.len() < 5 {
        return;
    }
    let old = i32::from_le_bytes(ord.payload[1..5].try_into().unwrap());
    if old != 0 {
        ord.payload[1..5].copy_from_slice(&0i32.to_le_bytes());
        effects.push(format!("{utag}.AirPatrolOrder.waypoint {old} -> 0"));
    }
}

// ---------------------------------------------------------------------------
// Air order payload views
// ---------------------------------------------------------------------------

/// Byte offset of the `AirOrder` block inside a serialized air order
/// payload (see the module header): strafe 25, attack-ground 18, patrol
/// after `waypoint` and the two `SimpleArray<Coord>` walks.
pub fn air_order_offset(ty: i32, payload: &[u8]) -> Option<usize> {
    match ty {
        0x10 => (payload.len() == 57).then_some(25),
        0x18 => (payload.len() == 54).then_some(18),
        0x11 => {
            let mut p = 5usize;
            for _ in 0..2 {
                let len = i32::from_le_bytes(payload.get(p..p + 4)?.try_into().unwrap());
                p += 4;
                if len != 0 {
                    // size i32, increment i16, flags u8, then len Coords.
                    p += 4 + 2 + 1 + usize::try_from(len).ok()? * 4;
                }
            }
            p += 1; // the virtual-base flags byte walked again
            (payload.len() == p + 24).then_some(p)
        }
        _ => None,
    }
}

/// The six walked `AirOrder` fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AirOrderView {
    pub oxx: i32,
    pub whose: i32,
    pub cruising_alt: i32,
    pub sharp_turn: i32,
    pub old: i32,
    pub returning: i32,
}

pub fn air_order(ty: i32, payload: &[u8]) -> Option<AirOrderView> {
    let p = air_order_offset(ty, payload)?;
    let f = |i: usize| i32::from_le_bytes(payload[p + i * 4..p + i * 4 + 4].try_into().unwrap());
    Some(AirOrderView { oxx: f(0), whose: f(1), cruising_alt: f(2), sharp_turn: f(3), old: f(4), returning: f(5) })
}

fn set_air_order_field(payload: &mut [u8], base: usize, field: usize, v: i32) {
    payload[base + field * 4..base + field * 4 + 4].copy_from_slice(&v.to_le_bytes());
}

/// `AirPatrolOrder` waypoint lists (`x_pos`, `y_pos`) and cursor.
pub struct PatrolView {
    pub waypoint: i32,
    pub xs: Vec<i32>,
    pub ys: Vec<i32>,
}

pub fn patrol_order(payload: &[u8]) -> Option<PatrolView> {
    let waypoint = i32::from_le_bytes(payload.get(1..5)?.try_into().unwrap());
    let mut p = 5usize;
    let mut lists = Vec::new();
    for _ in 0..2 {
        let len = i32::from_le_bytes(payload.get(p..p + 4)?.try_into().unwrap());
        p += 4;
        let mut v = Vec::new();
        if len != 0 {
            p += 7;
            for _ in 0..len {
                v.push(i32::from_le_bytes(payload.get(p..p + 4)?.try_into().unwrap()));
                p += 4;
            }
        }
        lists.push(v);
    }
    let ys = lists.pop()?;
    let xs = lists.pop()?;
    Some(PatrolView { waypoint, xs, ys })
}

// ---------------------------------------------------------------------------
// Unit::check_fuel 0x005e9be0
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FuelCheck {
    /// Not returning (fuel left, or the type has no fuel): the caller keeps its destination.
    Continue,
    /// Returning to `(whose, oxx)`; the approach vector is the stop (`Unit::check_fuel` 0x005ea0c0..).
    Returning { oxx: i32, whose: i32 },
    /// No host anywhere and not a helicopter: `Unit::die(2, -1, 0)` — the caller returns.
    Dies,
    /// Helicopter with no host: hover in place (`*x, *y = own position`).
    Hover,
    /// The host search needs `can_carry_object` arms that are not transcribed.
    Stop,
}

/// `Unit::check_fuel(order, &x, &y, &z)` — transcribed:
///
/// ```text
/// 005e9bf4  air = order->update_air_order();
///           if (!air.returning && type.mana == 0) return 0;
/// 005e9c1a  if (!air.returning) { left = max(mana() - mana_burn, 0);
///               if (left == 0 && !(obj_masks & 0x8000000)) air.returning = 1; }
///           if (!air.returning) return 0;
/// 005e9c5c  if (air.oxx < 0 || !Objects[whose][oxx].is_valid() || !host.can_carry(o, who)) {
///               best = nearest (vector_dist) over Build slots 2000..build_mark[who] with is_valid && can_carry(o, who),
///                      then live Unit slots 0..unit_mark[who] with can_carry(o, who);   // carriers
///               if (none) { if (!helicopter) { die(2, -1, 0); return 1; }  *x,*y = own; return 0; }
///               air.oxx = best.o; air.whose = who;
///           }
/// 005e9ff0  approach vector toward the host (x - 0xc0, find_angle 0x0092d130, FUN_0092cf40): stop
/// ```
///
/// Writes `returning` and `(oxx, whose)` on the order payload.
pub fn check_fuel(save: &Save, u: &mut Unit, tag: &str, effects: &mut Vec<String>) -> FuelCheck {
    let mut img = UImg(u);
    if !img.complete() {
        return FuelCheck::Continue;
    }
    let who = img.who();
    let o = img.o();
    let ptype = img.ptype();
    let burn = img.i16(0x96) as i32;
    let Some(ord) = img.0.orders.orders.first_mut() else { return FuelCheck::Continue };
    let Some(base) = air_order_offset(ord.ty, &ord.payload) else { return FuelCheck::Continue };
    let mut air = air_order(ord.ty, &ord.payload).unwrap();
    let type_mana = type_rec(save, ptype).and_then(|t| t.i32(0x2ec)).unwrap_or(0);
    if air.returning == 0 && type_mana == 0 {
        return FuelCheck::Continue;
    }
    if air.returning == 0 {
        let left = (mana(save, who, ptype).unwrap_or(0) - burn).max(0);
        if left == 0 && type_obj_masks(save, ptype) & 0x8000000 == 0 {
            set_air_order_field(&mut ord.payload, base, 5, 1);
            air.returning = 1;
            effects.push(format!("{tag}.AirOrder.returning 0 -> 1"));
        }
    }
    if air.returning == 0 {
        return FuelCheck::Continue;
    }
    // `is_valid` (vtable +0x4c) is 0x0046cda0 = `flags & 1` for Unit and Build alike.
    let host_ok = air.oxx >= 0 && obj_live(save, air.whose, air.oxx) && can_carry_object(save, (air.whose, air.oxx), (who, o)) == Some(true);
    if !host_ok {
        let (mut best, mut best_d) = (-1i32, 0i32);
        let (ux, uy) = (img.x(), img.y());
        for slot in 2000..build_mark(save, who).max(2000) {
            if !obj_live(save, who, slot) {
                continue;
            }
            match can_carry_object(save, (who, slot), (who, o)) {
                Some(true) => {}
                Some(false) => continue,
                None => return FuelCheck::Stop,
            }
            let (hx, hy) = obj_xy(save, who, slot).unwrap();
            let d = vector_dist(hx - ux, hy - uy);
            if best < 0 || d < best_d {
                best = slot;
                best_d = d;
            }
        }
        for slot in 0..unit_mark(save, who).max(0) {
            if !obj_live(save, who, slot) {
                continue;
            }
            match can_carry_object(save, (who, slot), (who, o)) {
                Some(true) => {}
                Some(false) => continue,
                None => return FuelCheck::Stop,
            }
            let (hx, hy) = obj_xy(save, who, slot).unwrap();
            let d = vector_dist(hx - ux, hy - uy);
            if best < 0 || d < best_d {
                best = slot;
                best_d = d;
            }
        }
        if best < 0 {
            if type_unit_flags(save, ptype) & 0x20 == 0 {
                // TODO(va 0x0060eda0) Unit::die(2, -1, 0) (+ human-player message 0x11a).
                effects.push(format!("{tag}: out of fuel, no host — Unit::die(2,-1,0) 0x0060eda0 not transcribed"));
                return FuelCheck::Dies;
            }
            return FuelCheck::Hover;
        }
        let ord = img.0.orders.orders.first_mut().unwrap();
        set_air_order_field(&mut ord.payload, base, 0, best);
        set_air_order_field(&mut ord.payload, base, 1, who);
        effects.push(format!("{tag}.AirOrder.(oxx,whose) ({},{}) -> ({best},{who})", air.oxx, air.whose));
        air.oxx = best;
        air.whose = who;
    }
    // TODO(va 0x005e9ff0..0x005ea30f) approach vector (x - 0xc0 / angle-dependent offset via find_angle).
    FuelCheck::Returning { oxx: air.oxx, whose: air.whose }
}

// ---------------------------------------------------------------------------
// Unit::do_air_patrol 0x005ea620 / do_air_attack_ground 0x005ea420
// ---------------------------------------------------------------------------

/// `Unit::do_air_patrol(order)` — skeleton transcribed, physics and target
/// finders stopped:
///
/// ```text
/// 005ea63a  think_bird(order, 0);                                   // vtable +0x180: no-op for Unit
/// 005ea645  if (x_pos.length <= waypoint) waypoint = 0;
/// 005ea655  if (!returning) { dest = (x_pos[waypoint], y_pos[waypoint]);
///               if (host (oxx, whose) >= 0 && host.is_unit()) dest += host.xy (carrier-relative), clamp FUN_006b53a0 }
/// 005ea6ee  if (!do_air_physics(order, dest)) return;               // 0x005e86d0, stop
/// 005ea700  if (returning) { if (collide == 0) collide = 1; return; }  // +0x88? no: in_ECX[0x26] = +0x98 spell_time
/// 005ea71e  if (vector_dist(dest - xy) < 0x240) { if (waypoint < length - 1) waypoint++;
///               else if (orderlist.length > 1) { kill_current_order(0); return; } }
/// 005ea763  if ((o + frame) & 0xf == 0): last waypoint (+ host offset when is(0x134)); !is(0x130) ?
///               find_new_air_target 0x005ebc70 [fallback find_new_bomber_target 0x005eb960 under Game+0x2084 & 2]
///               : the reverse; valid_target && (waypoint == length-1 || target.domain == 2) → add_strafe_order(…, 0,0,0) 0x005e48c0
/// 005ea8e0  if ((o + frame) & 0x1f == 0): find_building_at 0x0065ab40 around the current waypoint;
///               ever_seen bit for who → add_strafe_order(…, 1,0,0)
/// ```
///
/// Performed writes: the waypoint wrap and advance. Stops: physics (which
/// also owns the two RNG sites), `kill_current_order`, the finders and
/// `add_strafe_order`. Because the advance depends on the post-physics
/// position, only the wrap is applied when physics is not transcribed; the
/// function reports what it would have evaluated.
pub fn do_air_patrol(save: &Save, u: &mut Unit, tag: &str, effects: &mut Vec<String>) {
    let mut img = UImg(u);
    if !img.complete() {
        return;
    }
    let Some(ord) = img.0.orders.orders.first_mut() else { return };
    if ord.ty != 0x11 {
        return;
    }
    let Some(pv) = patrol_order(&ord.payload) else { return };
    if pv.xs.len() as i32 <= pv.waypoint {
        ord.payload[1..5].copy_from_slice(&0i32.to_le_bytes());
        effects.push(format!("{tag}.AirPatrolOrder.waypoint {} -> 0 (wrap)", pv.waypoint));
    }
    let _ = save;
    // TODO(va 0x005e86d0) Unit::do_air_physics — x/y/z, angle, path stack, collide,
    // cruising_alt (Random::get at (o+frame)&7==0), sharp_turn (Random::get when blocked).
    effects.push(format!("{tag}: Unit::do_air_physics 0x005e86d0 not transcribed; waypoint advance / 16- and 32-frame target scans skipped"));
}

/// `Unit::do_air_attack_ground(order)` — skeleton:
///
/// ```text
/// 005ea42e  aag = order->update_air_attack_ground_order(); dest = (att_x, att_y)
/// 005ea43e  if (!do_air_physics(order, dest)) return;                            // stop
/// 005ea44e  if (recharging != 0) return;
/// 005ea45c  da = |angle - find_angle(dest - xy)|
/// 005ea475  if (aag.returning) return;
/// 005ea489  if (!Object::is_over(dest, xy) 0x0064e4a0) return;
/// 005ea4a0  if (!missile && da > 0x0aaaaaa9) { if (!is(0x127,0) || da > 0x2aaaaaaa) return; }
/// 005ea4d6  Unit::face_target(-1,-1) 0x005fce70;
///           if (!(unit_flags2 & 0x400000) && !is(0x130,0) && type+0x2cc != 0) Object::drop_bomb(-1,-1) 0x0064c8b0
///           else Unit::fire(0xc, 0, 1) 0x00616f40;
/// 005ea56f  if (!missile) { recharging = recharge(0) + 1; if (is(0x130,0)) mana_burn += (i16)Constants+0xc14; }
///           else die(0, -1, 0);
/// ```
///
/// Nothing is written here: every write sits behind the physics stop.
pub fn do_air_attack_ground(save: &Save, u: &mut Unit, tag: &str, effects: &mut Vec<String>) {
    let img = UImg(u);
    if !img.complete() {
        return;
    }
    let Some(ord) = img.0.orders.orders.first() else { return };
    if ord.ty != 0x18 {
        return;
    }
    let _ = save;
    effects.push(format!("{tag}: Unit::do_air_attack_ground 0x005ea420 — do_air_physics 0x005e86d0 stop; release gate not evaluated"));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prim::SimpleVec;
    use crate::sections::{ObjBase, Order, OrderList, SubObj};

    /// A `Save` with the minimum the cone reads: one owner list, 10 type
    /// records, a Constants image and a seeded LCG.
    fn save() -> Save {
        let mut s = Save::default();
        s.constants = vec![0u8; 0xd40];
        s.post_world = vec![0u8; 0x40];
        s.game.scalars = vec![0u8; 0x40];
        s.objects.scalars = vec![0u8; 16 + 36 * 4];
        s.objects.lists.push(Default::default());
        s.objects.lists.push(Default::default());
        for i in 0..0x300 {
            let mut t = TypeRec::default();
            t.head = vec![0u8; 90];
            t.head[0..4].copy_from_slice(&(i as i32).to_le_bytes());
            t.obj_mid = vec![0u8; 152];
            t.ext = vec![0u8; 792];
            s.rules_tail.rules.types.push(t);
        }
        s.leaders.slots.push(Default::default());
        s.leaders.slots.push(Default::default());
        s.leaders.slots[0].body = vec![0u8; 0x6930];
        s.leaders.slots[1].body = vec![0u8; 0x6930];
        s
    }

    fn set_type(s: &mut Save, ty: i32, domain: i32, unit_flags: u32, obj_masks: u32, mana: i32, pct_lo: i32, pct_hi: i32) {
        let t = &mut s.rules_tail.rules.types[ty as usize];
        t.obj_mid[0x218 - 0x1e4..0x21c - 0x1e4].copy_from_slice(&domain.to_le_bytes());
        t.obj_mid[0..4].copy_from_slice(&obj_masks.to_le_bytes());
        t.obj_mid[0x250 - 0x1e4..0x254 - 0x1e4].copy_from_slice(&pct_lo.to_le_bytes());
        t.obj_mid[0x254 - 0x1e4..0x258 - 0x1e4].copy_from_slice(&pct_hi.to_le_bytes());
        t.ext[0..4].copy_from_slice(&unit_flags.to_le_bytes());
        // +0x2ec -> ext index 0x2ec - 0x2b4 - 8
        t.ext[0x2ec - 0x2b4 - 8..0x2f0 - 0x2b4 - 8].copy_from_slice(&mana.to_le_bytes());
    }

    fn unit(who: u8, o: i16, ptype: i32) -> Unit {
        let mut u = Unit::default();
        u.base = ObjBase {
            sub: SubObj { tag: 0, flags: 1, gate: 1, body: vec![0u8; 19] },
            tag: 0,
            gate: 1,
            mid: vec![0u8; 34],
            launch: 0,
            launching: SimpleVec::default(),
        };
        u.base.sub.body[0] = who;
        u.base.sub.body[1..3].copy_from_slice(&o.to_le_bytes());
        u.base.sub.body[15..19].copy_from_slice(&ptype.to_le_bytes());
        u.body = vec![0u8; 111];
        // inside_up = -1 (airborne) by default; o_up = -1.
        u.body[0x82 - 0x48..0x84 - 0x48].copy_from_slice(&(-1i16).to_le_bytes());
        u.body[0x8e - 0x48..0x90 - 0x48].copy_from_slice(&(-1i16).to_le_bytes());
        u.body[0xb4 - 0x48] = 0xff;
        u.base.mid[0x08..0x0a].copy_from_slice(&(-1i16).to_le_bytes());
        u.base.mid[0x1e] = 0xff;
        u
    }

    fn put(s: &mut Save, who: usize, o: usize, obj: Obj) {
        let l = &mut s.objects.lists[who];
        if l.elems.len() <= o {
            l.elems.resize_with(o + 1, || None);
        }
        l.elems[o] = Some(obj);
    }

    fn seed(s: &Save) -> u32 {
        u32::from_le_bytes(s.post_world[0x28..0x2c].try_into().unwrap())
    }

    fn draws(s0: u32, s1: u32) -> u32 {
        crate::tick::rng_draws(s0, s1).unwrap_or(u32::MAX)
    }

    const PLANE: i32 = 0x140;
    const RIFLE: i32 = 0x60;
    const AA: i32 = 0x61;
    const HELI: i32 = 0x141;

    fn air_world() -> Save {
        let mut s = save();
        set_type(&mut s, PLANE, 2, 0, 0, 1500, 50, 50);
        set_type(&mut s, HELI, 2, 0x20, 0, 1500, 50, 50);
        set_type(&mut s, RIFLE, 0, 0, 0, 0, 50, 50);
        set_type(&mut s, AA, 0, 0, 0x80000000, 0, 50, 50);
        put(&mut s, 0, 0, Obj::Unit(Box::new(unit(0, 0, PLANE))));
        put(&mut s, 0, 1, Obj::Unit(Box::new(unit(0, 1, HELI))));
        put(&mut s, 1, 0, Obj::Unit(Box::new(unit(1, 0, RIFLE))));
        put(&mut s, 1, 1, Obj::Unit(Box::new(unit(1, 1, AA))));
        s.post_world[0x28..0x2c].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        s
    }

    // --- Ammo::init dud gate: draw structure per arm (0x0067bf91..0x0067c166) ---

    #[test]
    fn dud_gate_not_entered_for_non_plane_targets() {
        let mut s = air_world();
        let s0 = seed(&s);
        // target rifleman (domain 0): entered, no draws.
        assert_eq!(antiair_dud_gate(&mut s, (1, 1), (1, 0), false, false), Some(DudVerdict { dud: false, draws: 0 }));
        // helicopter target (unit_flags & 0x20): no gate.
        assert_eq!(antiair_dud_gate(&mut s, (1, 1), (0, 1), false, false), Some(DudVerdict { dud: false, draws: 0 }));
        // shooter is a unit with an 0x17/0x18 order: whole branch skipped.
        assert_eq!(antiair_dud_gate(&mut s, (1, 0), (0, 0), true, false), None);
        // dead / absent target: init aborted.
        assert_eq!(antiair_dud_gate(&mut s, (1, 0), (0, 7), false, false), None);
        assert_eq!(seed(&s), s0);
    }

    #[test]
    fn dud_gate_aa_shooter_draws_once() {
        let mut s = air_world();
        for low in [false, true] {
            let s0 = seed(&s);
            let v = antiair_dud_gate(&mut s, (1, 1), (0, 0), false, low).unwrap();
            assert_eq!(v.draws, 1);
            assert_eq!(draws(s0, seed(&s)), 1);
        }
        // Air-domain AA shooter: no gate (0x0067bfe1).
        set_type(&mut s, AA, 2, 0, 0x80000000, 0, 50, 50);
        let s0 = seed(&s);
        assert_eq!(antiair_dud_gate(&mut s, (1, 1), (0, 0), false, false), Some(DudVerdict { dud: false, draws: 0 }));
        assert_eq!(seed(&s), s0);
    }

    #[test]
    fn dud_gate_ordinary_shooter_short_circuits_on_the_target_percentage() {
        // target pct 0 -> every first draw fails -> 1 draw, dud.
        let mut s = air_world();
        set_type(&mut s, PLANE, 2, 0, 0, 1500, 0, 0);
        for _ in 0..8 {
            let s0 = seed(&s);
            let v = antiair_dud_gate(&mut s, (1, 0), (0, 0), false, false).unwrap();
            assert_eq!((v.dud, v.draws), (true, 1));
            assert_eq!(draws(s0, seed(&s)), 1);
        }
        // target pct 100 -> first draw always passes -> 2 draws; shooter pct 100 -> never dud.
        set_type(&mut s, PLANE, 2, 0, 0, 1500, 100, 100);
        set_type(&mut s, RIFLE, 0, 0, 0, 0, 100, 100);
        for low in [false, true] {
            let s0 = seed(&s);
            let v = antiair_dud_gate(&mut s, (1, 0), (0, 0), false, low).unwrap();
            assert_eq!((v.dud, v.draws), (false, 2));
            assert_eq!(draws(s0, seed(&s)), 2);
        }
        // shooter pct 0 -> second draw always fails -> dud with 2 draws.
        set_type(&mut s, RIFLE, 0, 0, 0, 0, 0, 0);
        let v = antiair_dud_gate(&mut s, (1, 0), (0, 0), false, false).unwrap();
        assert_eq!((v.dud, v.draws), (true, 2));
    }

    #[test]
    fn dud_gate_picks_the_0x254_column_when_flying_low() {
        let mut s = air_world();
        // low column 100 / high column 0 on both types.
        set_type(&mut s, PLANE, 2, 0, 0, 1500, 0, 100);
        set_type(&mut s, RIFLE, 0, 0, 0, 0, 0, 100);
        assert_eq!(antiair_dud_gate(&mut s, (1, 0), (0, 0), false, true).unwrap(), DudVerdict { dud: false, draws: 2 });
        assert_eq!(antiair_dud_gate(&mut s, (1, 0), (0, 0), false, false).unwrap(), DudVerdict { dud: true, draws: 1 });
    }

    // --- Unit::process air-fuel branch ---

    #[test]
    fn air_fuel_airborne_burns_until_cap_and_zeroes_healing() {
        let s = air_world();
        let mut u = unit(0, 0, PLANE);
        u.body[0x96 - 0x48..0x98 - 0x48].copy_from_slice(&1499i16.to_le_bytes());
        u.base.mid[0x18..0x1a].copy_from_slice(&7i16.to_le_bytes()); // healing
        let mut fx = Vec::new();
        assert_eq!(air_fuel_step(&s, &mut u, "t", &mut fx), AirFuel::Airborne { burned: true });
        assert_eq!(i16::from_le_bytes([u.body[0x96 - 0x48], u.body[0x97 - 0x48]]), 1500);
        assert_eq!(i16::from_le_bytes([u.base.mid[0x18], u.base.mid[0x19]]), 0);
        // At the cap: `cmovs` clamps mana - burn to 0 -> no increment.
        assert_eq!(air_fuel_step(&s, &mut u, "t", &mut fx), AirFuel::Airborne { burned: false });
        assert_eq!(i16::from_le_bytes([u.body[0x96 - 0x48], u.body[0x97 - 0x48]]), 1500);
    }

    #[test]
    fn air_fuel_hosted_decays_by_constant_a0_and_repairs_on_period() {
        let mut s = air_world();
        s.constants[0xa0..0xa4].copy_from_slice(&10i32.to_le_bytes());
        s.constants[0xc00..0xc04].copy_from_slice(&4i32.to_le_bytes()); // heal level 0 period
        // bonus cards 0x2ef..0x2f1 with preq -2 -> has_preq false -> level 0
        for t in 0x2ef..=0x2f1 {
            s.rules_tail.rules.types[t].head[0x2c - 4..0x30 - 4].copy_from_slice(&(-2i32).to_le_bytes());
            s.rules_tail.rules.types[t].head[0x30 - 4..0x34 - 4].copy_from_slice(&(-1i32).to_le_bytes());
        }
        let mut u = unit(0, 0, PLANE);
        u.body[0x82 - 0x48..0x84 - 0x48].copy_from_slice(&2000i16.to_le_bytes()); // hosted
        u.body[0x96 - 0x48..0x98 - 0x48].copy_from_slice(&25i16.to_le_bytes());
        // frame 3, o 0 -> (0 + 3) % 4 != 0
        s.game.scalars[0..4].copy_from_slice(&3i32.to_le_bytes());
        let mut fx = Vec::new();
        assert_eq!(air_fuel_step(&s, &mut u, "t", &mut fx), AirFuel::Hosted { repaired: false });
        assert_eq!(i16::from_le_bytes([u.body[0x96 - 0x48], u.body[0x97 - 0x48]]), 15);
        assert_eq!(i16::from_le_bytes([u.base.mid[0x18], u.base.mid[0x19]]), 0);
        // frame 4 -> period hit: healing = 1; burn 15 > 10 -> 5
        s.game.scalars[0..4].copy_from_slice(&4i32.to_le_bytes());
        assert_eq!(air_fuel_step(&s, &mut u, "t", &mut fx), AirFuel::Hosted { repaired: true });
        assert_eq!(i16::from_le_bytes([u.body[0x96 - 0x48], u.body[0x97 - 0x48]]), 5);
        assert_eq!(i16::from_le_bytes([u.base.mid[0x18], u.base.mid[0x19]]), 1);
        // burn 5 <= 10 -> `jle` arm: mana_burn = 0
        s.game.scalars[0..4].copy_from_slice(&5i32.to_le_bytes());
        air_fuel_step(&s, &mut u, "t", &mut fx);
        assert_eq!(i16::from_le_bytes([u.body[0x96 - 0x48], u.body[0x97 - 0x48]]), 0);
    }

    #[test]
    fn air_fuel_ignores_land_units_and_real_preqs_are_a_stop() {
        let mut s = air_world();
        let mut r = unit(1, 0, RIFLE);
        let before = r.clone();
        let mut fx = Vec::new();
        assert_eq!(air_fuel_step(&s, &mut r, "t", &mut fx), AirFuel::NotAir);
        assert_eq!(r.body, before.body);
        s.rules_tail.rules.types[0x2ef].head[0x2c - 4..0x30 - 4].copy_from_slice(&0x250i32.to_le_bytes());
        let mut u = unit(0, 0, PLANE);
        u.body[0x82 - 0x48..0x84 - 0x48].copy_from_slice(&2000i16.to_le_bytes());
        assert_eq!(air_fuel_step(&s, &mut u, "t", &mut fx), AirFuel::HostedNoHealLevel);
    }

    // --- containment ---

    fn build(who: u8, o: i16, ptype: i32) -> Build {
        let mut b = Build::default();
        b.head = vec![0u8; 2];
        b.base = ObjBase {
            sub: SubObj { tag: 0, flags: 1 | 4, gate: 1, body: vec![0u8; 19] },
            tag: 0,
            gate: 1,
            mid: vec![0u8; 34],
            launch: 0,
            launching: SimpleVec::default(),
        };
        b.base.sub.body[0] = who;
        b.base.sub.body[1..3].copy_from_slice(&o.to_le_bytes());
        b.base.sub.body[15..19].copy_from_slice(&ptype.to_le_bytes());
        b.base.mid[0x08..0x0a].copy_from_slice(&(-1i16).to_le_bytes());
        b.base.mid[0x1e] = 0xff;
        b.wall_body = vec![0u8; 30];
        b.body = vec![0u8; 22];
        b
    }

    const AIRBASE: i32 = 0x1bf;

    /// Airbase at slot 2000 holding planes 0 -> 2 (chain through inside_down).
    fn base_world() -> Save {
        let mut s = air_world();
        set_type(&mut s, AIRBASE, 0, 0, 0, 0, 0, 0);
        s.rules_tail.rules.types[AIRBASE as usize].ext = vec![0u8; 49];
        let mut b = build(0, 2000, AIRBASE);
        b.base.mid[0x08..0x0a].copy_from_slice(&0i16.to_le_bytes());
        b.base.mid[0x1e] = 0;
        put(&mut s, 0, 2000, Obj::Build(Box::new(b)));
        s.objects.scalars[16 + 36..16 + 40].copy_from_slice(&2001i32.to_le_bytes());
        s.objects.scalars[16..20].copy_from_slice(&3i32.to_le_bytes());
        let mut p0 = unit(0, 0, PLANE);
        p0.body[0x82 - 0x48..0x84 - 0x48].copy_from_slice(&2000i16.to_le_bytes());
        p0.body[0xb4 - 0x48] = 0;
        p0.base.mid[0x08..0x0a].copy_from_slice(&2i16.to_le_bytes());
        p0.base.mid[0x1e] = 0;
        let mut p2 = unit(0, 2, PLANE);
        p2.body[0x82 - 0x48..0x84 - 0x48].copy_from_slice(&2000i16.to_le_bytes());
        p2.body[0xb4 - 0x48] = 0;
        put(&mut s, 0, 0, Obj::Unit(Box::new(p0)));
        put(&mut s, 0, 2, Obj::Unit(Box::new(p2)));
        s
    }

    #[test]
    fn num_inside_walks_the_inside_down_chain() {
        let s = base_world();
        assert_eq!(num_inside(&s, 0, 2000, true), Some(2));
        // Hosted plane (inside_up >= 0) is not on the map -> 0.
        assert_eq!(num_inside(&s, 0, 0, true), Some(0));
        assert_eq!(num_aircraft_here(&s, 0, 2000, false), Some(2));
        assert!(can_carry_domain(&s, 0, 2000, 2));
        assert!(!can_carry_domain(&s, 0, 2000, 1));
        assert!(!can_carry_domain(&s, 0, 0, 2));
    }

    #[test]
    fn num_aircraft_limit_by_host_kind() {
        let mut s = base_world();
        s.constants[0xa8..0xac].copy_from_slice(&7i32.to_le_bytes());
        s.constants[0xa4..0xa8].copy_from_slice(&5i32.to_le_bytes());
        assert_eq!(num_aircraft_limit(&s, 0, 2000), 7);
        assert_eq!(num_aircraft_limit(&s, 0, 0), 0);
        // can_carry(int,int): hosted plane is accepted by its own base through home_base.
        assert_eq!(can_carry_object(&s, (0, 2000), (0, 0)), Some(true));
        // foreign owner / self never.
        assert_eq!(can_carry_object(&s, (0, 2000), (1, 0)), Some(false));
        assert_eq!(can_carry_object(&s, (0, 2000), (0, 2000)), Some(false));
    }

    #[test]
    fn process_ejection_clears_mask_when_empty_and_counts_hold_frames_when_inactive() {
        let s = base_world();
        let mut b = build(0, 2000, AIRBASE);
        b.wall_body[0x60 - 0x48..0x62 - 0x48].copy_from_slice(&0x4001u16.to_le_bytes());
        let mut fx = Vec::new();
        process_ejection(&s, &mut b, "t", &mut fx);
        assert_eq!(u16::from_le_bytes([b.wall_body[0x60 - 0x48], b.wall_body[0x61 - 0x48]]), 1);
        // Loaded, inactive site: hold_frames += 1; mask untouched.
        let mut b = build(0, 2000, AIRBASE);
        b.base.sub.flags = 0;
        b.wall_body[0x60 - 0x48..0x62 - 0x48].copy_from_slice(&0x4000u16.to_le_bytes());
        b.base.mid[0x08..0x0a].copy_from_slice(&0i16.to_le_bytes());
        b.base.mid[0x1e] = 0;
        process_ejection(&s, &mut b, "t", &mut fx);
        assert_eq!(i16::from_le_bytes([b.base.mid[0x12], b.base.mid[0x13]]), 1);
        assert_eq!(u16::from_le_bytes([b.wall_body[0x60 - 0x48], b.wall_body[0x61 - 0x48]]), 0x4000);
        assert!(fx.iter().any(|e| e.contains("come_out(1)")));
    }

    #[test]
    fn do_launch_counts_launch_frames_then_walks_the_chain() {
        let mut s = base_world();
        let mut fx = Vec::new();
        // 15 frames of counting before anything launches.
        for i in 0..15 {
            do_launch(&mut s, 0, 2000, &mut fx);
            let Some(Some(Obj::Build(b))) = s.objects.lists[0].elems.get(2000) else { panic!() };
            assert_eq!(b.base.mid[0x21] as i8, i + 1);
            assert_eq!(b.base.launch, 0);
        }
        // No orders on the planes -> not ready -> nothing launched, counter clamps.
        do_launch(&mut s, 0, 2000, &mut fx);
        let Some(Some(Obj::Build(b))) = s.objects.lists[0].elems.get(2000) else { panic!() };
        assert_eq!(b.base.mid[0x21] as i8, FRAMES_BETWEEN_LAUNCHES);
        assert_eq!(b.base.launch, 0);
        // Give plane 2 an AIR_PATROL order with waypoint 3 and flags & 4: it is
        // recorded in `launching`, its waypoint resets, launch_frames -> 0.
        let mut payload = vec![0u8; 5 + 4 + 4 + 1 + 24];
        payload[0] = 4;
        payload[1..5].copy_from_slice(&3i32.to_le_bytes());
        if let Some(Some(Obj::Unit(u))) = s.objects.lists[0].elems.get_mut(2) {
            u.orders = OrderList { count: 1, orders: vec![Order { ty: 0x11, metric: 0, payload }] };
        }
        do_launch(&mut s, 0, 2000, &mut fx);
        let Some(Some(Obj::Build(b))) = s.objects.lists[0].elems.get(2000) else { panic!() };
        assert_eq!(b.base.mid[0x21], 0);
        assert_eq!(b.base.launch, 1);
        assert_eq!(launching_vec(&s, 0, 2000), Some(vec![2]));
        let Some(Some(Obj::Unit(u))) = s.objects.lists[0].elems.get(2) else { panic!() };
        assert_eq!(i32::from_le_bytes(u.orders.orders[0].payload[1..5].try_into().unwrap()), 0);
        assert!(fx.iter().any(|e| e.contains("come_out(0)")));
        // Next frame: repeat-air off and flags & 4 cleared -> kill + remove.
        if let Some(Some(Obj::Unit(u))) = s.objects.lists[0].elems.get_mut(2) {
            u.orders.orders[0].payload[0] = 0;
        }
        for _ in 0..16 {
            do_launch(&mut s, 0, 2000, &mut fx);
        }
        assert_eq!(launching_vec(&s, 0, 2000), Some(vec![]));
    }

    #[test]
    fn air_order_offsets_match_the_walk_data_layouts() {
        assert_eq!(air_order_offset(0x10, &[0u8; 57]), Some(25));
        assert_eq!(air_order_offset(0x18, &[0u8; 54]), Some(18));
        assert_eq!(air_order_offset(0x10, &[0u8; 56]), None);
        // patrol with two 2-point lists: 1 + 4 + (4+7+8) + (4+7+8) + 1 + 24 = 68
        let mut p = vec![0u8; 68];
        p[5..9].copy_from_slice(&2i32.to_le_bytes());
        p[24..28].copy_from_slice(&2i32.to_le_bytes());
        assert_eq!(air_order_offset(0x11, &p), Some(44));
        let pv = patrol_order(&p).unwrap();
        assert_eq!((pv.xs.len(), pv.ys.len()), (2, 2));
        // empty lists: 1 + 4 + 4 + 4 + 1 + 24 = 38
        assert_eq!(air_order_offset(0x11, &[0u8; 38]), Some(14));
    }

    #[test]
    fn check_fuel_latches_returning_and_finds_the_nearest_live_host() {
        let mut s = base_world();
        s.constants[0xa8..0xac].copy_from_slice(&7i32.to_le_bytes());
        // airborne plane 5 with a strafe order, fuel spent.
        let mut p = unit(0, 5, PLANE);
        p.body[0x96 - 0x48..0x98 - 0x48].copy_from_slice(&1500i16.to_le_bytes());
        p.base.sub.body[7..11].copy_from_slice(&(0x3000 ^ 0x63637).to_le_bytes());
        p.base.sub.body[11..15].copy_from_slice(&(0x3000 ^ 0x63637).to_le_bytes());
        let mut payload = vec![0u8; 57];
        payload[1..5].copy_from_slice(&(-1i32).to_le_bytes());
        payload[25..29].copy_from_slice(&(-1i32).to_le_bytes());
        payload[29..33].copy_from_slice(&(-1i32).to_le_bytes());
        p.orders = OrderList { count: 1, orders: vec![Order { ty: 0x10, metric: 0, payload }] };
        put(&mut s, 0, 5, Obj::Unit(Box::new(p)));
        s.objects.scalars[16..20].copy_from_slice(&6i32.to_le_bytes());
        let mut u = match s.objects.lists[0].elems[5].take() {
            Some(Obj::Unit(u)) => u,
            _ => unreachable!(),
        };
        let mut fx = Vec::new();
        let r = check_fuel(&s, &mut u, "t", &mut fx);
        assert_eq!(r, FuelCheck::Returning { oxx: 2000, whose: 0 });
        let air = air_order(0x10, &u.orders.orders[0].payload).unwrap();
        assert_eq!((air.returning, air.oxx, air.whose), (1, 2000, 0));
        // With fuel left nothing is touched.
        let mut p = unit(0, 6, PLANE);
        let mut payload = vec![0u8; 57];
        payload[1..5].copy_from_slice(&(-1i32).to_le_bytes());
        p.orders = OrderList { count: 1, orders: vec![Order { ty: 0x10, metric: 0, payload: payload.clone() }] };
        assert_eq!(check_fuel(&s, &mut p, "t", &mut fx), FuelCheck::Continue);
        assert_eq!(p.orders.orders[0].payload, payload);
    }

    // --- capture control: the cone writes nothing on the Ancient-age captures ---

    #[test]
    fn idle_captures_have_no_air_cone_activity() {
        use crate::{container, load};
        use std::path::Path;
        let name = std::env::var("DON_CAPTURE").unwrap_or_else(|_| "20261004-075733-stride1".into());
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/live/frame-pairs").join(name);
        if !dir.join("manifest.json").exists() {
            eprintln!("skipping: no capture at {}", dir.display());
            return;
        }
        let text = std::fs::read_to_string(dir.join("manifest.json")).unwrap();
        let first = text.split("\"save_name\":").nth(1).and_then(|s| s.split('"').nth(1)).unwrap().to_string();
        let raw = container::load_svx(&dir.join(format!("{first}.svx"))).unwrap();
        let mut s = load(&raw).unwrap().state;
        let s0 = seed(&s);
        let mut fx = Vec::new();
        let mut air_units = 0;
        let mut hosts = 0;
        for owner in 0..s.objects.lists.len() {
            for slot in 0..s.objects.lists[owner].elems.len() {
                let Some(obj) = s.objects.lists[owner].elems[slot].take() else { continue };
                match obj {
                    Obj::Unit(mut u) => {
                        if u.body.len() == 111 {
                            let ty = i32::from_le_bytes(u.base.sub.body[15..19].try_into().unwrap());
                            if type_domain(&s, ty) == 2 {
                                air_units += 1;
                            }
                            let before = u.clone();
                            let r = air_fuel_step(&s, &mut u, "x", &mut fx);
                            assert_eq!(r, AirFuel::NotAir);
                            assert_eq!(u.body, before.body);
                            assert_eq!(check_fuel(&s, &mut u, "x", &mut fx), FuelCheck::Continue);
                        }
                        s.objects.lists[owner].elems[slot] = Some(Obj::Unit(u));
                    }
                    Obj::Build(mut b) => {
                        if b.base.sub.body.len() != 19 || b.wall_body.len() != 30 {
                            s.objects.lists[owner].elems[slot] = Some(Obj::Build(b));
                            continue;
                        }
                        let who = b.base.sub.body[0] as i32;
                        let o = i16::from_le_bytes([b.base.sub.body[1], b.base.sub.body[2]]) as i32;
                        s.objects.lists[owner].elems[slot] = Some(Obj::Build(b.clone()));
                        if can_carry_domain(&s, who, o, 2) {
                            hosts += 1;
                        }
                        let bm = u16::from_le_bytes([b.wall_body[0x18], b.wall_body[0x19]]);
                        // Retail gates (Build::process 0x0061edf0): & 0x4000 -> process_ejection, & 8 -> do_launch.
                        assert_eq!(bm & 8, 0, "Objects[{owner}][{slot}] has build_masks & 8 (launch pending) in an Ancient-age capture");
                        let before = b.clone();
                        if bm & 0x4000 != 0 {
                            process_ejection(&s, &mut b, "x", &mut fx);
                        }
                        assert_eq!(b.base.mid, before.base.mid);
                        if bm & 8 != 0 {
                            do_launch(&mut s, owner, slot, &mut fx);
                        }
                        let Some(Some(Obj::Build(after))) = s.objects.lists[owner].elems.get(slot) else { panic!() };
                        assert_eq!(after.base.mid, before.base.mid, "do_launch moved an Ancient-age Build");
                    }
                    other => s.objects.lists[owner].elems[slot] = Some(other),
                }
            }
        }
        assert_eq!(air_units, 0, "capture unexpectedly has aircraft — promote this test to a verification");
        assert_eq!(hosts, 0);
        assert_eq!(seed(&s), s0, "the air cone must not draw on an Ancient-age frame");
        eprintln!("capture {first}: {air_units} air units, {hosts} hosts, {} effects", fx.len());
    }
}
