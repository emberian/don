//! Objects spatial-query layer: pure read-only functions over `&Save`.
//!
//! Transcribed from `re/decomp-all/` plus Capstone for the register
//! arguments Ghidra drops (`__thiscall` ECX, the `ecx/edx` pairs of
//! `vector_dist` / `find_angle`):
//!
//! - `ObjectsData::find_unit` 0x0065ca80 (`0065ca80.c`) and
//!   `ObjectsData::find_building` 0x0065d260 (`0065d260.c`), the Unit- and
//!   Build-plane spatial searches. Both write the `ObjectsData` scratch
//!   `find_dist` (+0x1fc) and `find_who` (+0x200); neither is walked by
//!   `Objects::walk_data` 0x006541e0 (the scalar block is `+0x1f4..+0x1fc`,
//!   `+0x154..+0x15c`, the marks and `obj_ctr`), so they are returned in
//!   [`Found`] instead of being stored.
//! - `Search::valid_search` 0x0067daa0 (`0067daa0.c`; ECX = the owner slot
//!   of the candidate, resolved from the asm at 0x0065cc02/0x0065cdc1) and
//!   `Search::valid_filter` 0x0067dbb0 (jump table at 0x0067e57c; ECX = the
//!   owner slot, 0x0065cc71 `mov ebx,[ebp-8]`). Only the three
//!   `FilterIndex` arms `find_muster_spot` reaches are transcribed.
//! - `LeaderData::is_enemy` 0x006ebaa0 / `is_ally` 0x006edb50.
//! - `vector_dist` 0x0046cff0 (`ecx`, `edx`), `find_angle` 0x0092d130
//!   (`ecx = dx`, `edx = dy`; transcribed from the asm, which keeps a
//!   swapped-octant flag in `ebx` that the decompiler folds away).
//! - `circle_init` 0x006817f0: the `circle_x`/`circle_y` (0x00cb7e90 /
//!   0x00cbb0e0, `char`) spiral and `circle_radius` (0x00cbe330, ring end
//!   indices) tables are BSS, built at start-up; rebuilt here lazily with
//!   the same loop. `move_x`/`move_y` (0x00adcaf0 / 0x00adc400) are
//!   `.rdata` constants copied verbatim.
//! - `div_3_table` 0x00cae5fc (`init_coord_lookup_array` 0x00681db0):
//!   `table[i] = floor(i / 3)`; a tile is `0x300` coord units so
//!   `table[coord >> 8]` is the tile column/row.
//! - `Army::find_muster_spot` 0x006f5cc0 (`006f5cc0.c`, 3,309 B) with its
//!   helpers `BuildData::is_unassimilated` 0x0062d470, `CityData::get_radius`
//!   0x00738410 → `LeaderData::get_radius` 0x006db790 (ECX = Leader of the
//!   city owner, 0x0073842b) → `has_tribe_bonus` 0x006e1370 (reused from
//!   `leaders_process`), `WorldData::is_ocean` 0x006b4830, `Army::count`
//!   0x006f9120 (sum of `GroupData::count` over attached groups; zero when
//!   `num_groups == 0`, `TODO(0x00711720)` otherwise).
//!
//! RNG: none of the above calls `Random::get` 0x00a39d70 — no `game_random`
//! draws, so these are pure functions of the save image.
//!
//! The only write `find_muster_spot` makes outside the Army is to
//! `CityData.city_flags` (`&= 0xdfff` on success for the own city, `|= 0x2000`
//! on failure when `(flags & 3) == 1`); it is reported in
//! [`MusterSpot::city_flags`] for the owning traversal to apply.
//!
//! Leaf module: never edits sibling modules.

#![allow(dead_code)]

use std::sync::OnceLock;

use crate::sections::{Obj, Save};
use crate::systems::leaders_process::{Ctx, Eval};
use crate::tick::StepStatus;

pub const STATUS: StepStatus = StepStatus::Partial;

/// Coordinate obfuscation applied to every `SubObject` `x/y_internal`.
const COORD_XOR: i32 = 0x63637;

// ---------------------------------------------------------------------------
// Static tables
// ---------------------------------------------------------------------------

/// `move_x` 0x00adcaf0 (first 0x31 entries; `find_muster_spot` indexes
/// `0..0x31` with hurry, `0..9` without).
const MOVE_X: [i32; 49] = [
    0, -1, 0, 1, 1, 1, 0, -1, -1, -1, 0, 1, 2, 2, 2, 1, 0, -1, -2, -2, -2, -2, 2, 2, -2, -3, -2, -1, 0, 1,
    2, 3, 3, 3, 3, 3, 3, 3, 2, 1, 0, -1, -2, -3, -3, -3, -3, -3, -3,
];
/// `move_y` 0x00adc400.
const MOVE_Y: [i32; 49] = [
    0, -1, -1, -1, 0, 1, 1, 1, 0, -2, -2, -2, -1, 0, 1, 2, 2, 2, 1, 0, -1, -2, -2, 2, 2, -3, -3, -3, -3,
    -3, -3, -3, -2, -1, 0, 1, 2, 3, 3, 3, 3, 3, 3, 3, 2, 1, 0, -1, -2,
];

/// `circle_init` 0x006817f0 output.
pub struct Circle {
    /// `circle_x` / `circle_y`: spiral offsets ordered by ring then x then y.
    pub x: Vec<i8>,
    pub y: Vec<i8>,
    /// `circle_radius[r]`: index one past the last entry of ring `r`
    /// (`0..=0x40`).
    pub radius: [i32; 0x41],
}

/// `circle_init` 0x006817f0: for `r in 0..=0x40`, for `x in -r..=r`, for
/// `y in -r..=r`, append `(x, y)` when `vector_dist(x, y) == r`; the table
/// is capped at 0x3249 entries (the remaining `circle_radius` slots are
/// filled with the cap).
pub fn circle() -> &'static Circle {
    static C: OnceLock<Circle> = OnceLock::new();
    C.get_or_init(|| {
        let mut c = Circle { x: Vec::new(), y: Vec::new(), radius: [0; 0x41] };
        let mut n = 0i32;
        let mut r = 0i32;
        'outer: while r < 0x41 {
            let mut x = -r;
            while x <= r {
                let mut y = -r;
                while y <= r {
                    if vector_dist(x, y) == r {
                        c.x.push(x as i8);
                        c.y.push(y as i8);
                        n += 1;
                        if n > 0x3248 {
                            for slot in r as usize..0x41 {
                                c.radius[slot] = n;
                            }
                            break 'outer;
                        }
                    }
                    y += 1;
                }
                x += 1;
            }
            c.radius[r as usize] = n;
            r += 1;
        }
        c
    })
}

/// `div_3_table` 0x00cae5fc: `floor(i / 3)` on both halves.
pub fn div3(i: i32) -> i32 {
    i.div_euclid(3)
}

/// `vector_dist(int, int)` 0x0046cff0 (`ecx`, `edx`): octagonal distance
/// `max + min² / (2·max)`, with the `>= 60000` overflow arm.
pub fn vector_dist(a: i32, b: i32) -> i32 {
    let a = a.wrapping_abs() as u32;
    let b = b.wrapping_abs() as u32;
    let (big, small) = if (b as i32) < (a as i32) { (a, b) } else { (b, a) };
    if big == 0 {
        return 0;
    }
    if small >= 60000 {
        return (small.wrapping_add(big.wrapping_mul(2)) >> 1) as i32;
    }
    (small.wrapping_mul(small) / (big * 2)).wrapping_add(big) as i32
}

/// `find_angle(int, int)` 0x0092d130 (`ecx = dx`, `edx = dy`), from the asm.
/// Returns a full-circle angle in `i32` turns (`0x40000000` = quarter turn).
pub fn find_angle(dx: i32, dy: i32) -> i32 {
    let ndy = dy.wrapping_neg(); // edi
    if dx == 0 {
        return if ndy > 0 { 0 } else { i32::MIN };
    }
    if ndy == 0 {
        return if dx > 0 { 0x4000_0000 } else { 0xc000_0000u32 as i32 };
    }
    let ax = dx.wrapping_abs();
    let ay = ndy.wrapping_abs();
    // ebx = 1 when |dx| > |ndy| (octant swap).
    let (num, den, swapped) = if ax > ay { (ay << 14, ax, true) } else { (ax << 14, ay, false) };
    let q = num / den; // esi
    let t = (0x1333 - q).wrapping_abs();
    let m = 0x2800 - ((t.wrapping_mul(0xb00)) >> 14);
    let v = (m.wrapping_mul(q) & 0xffff_c000u32 as i32) << 2; // edx
    if dx > 0 {
        if ndy > 0 {
            if swapped {
                0x4000_0000i32.wrapping_sub(v)
            } else {
                v
            }
        } else if swapped {
            v.wrapping_add(0x4000_0000)
        } else {
            i32::MIN.wrapping_sub(v)
        }
    } else if ndy > 0 {
        if swapped {
            v.wrapping_add(0xc000_0000u32 as i32)
        } else {
            v.wrapping_neg()
        }
    } else if swapped {
        (0xc000_0000u32 as i32).wrapping_sub(v)
    } else {
        v.wrapping_add(i32::MIN)
    }
}

// ---------------------------------------------------------------------------
// Image readers
// ---------------------------------------------------------------------------

fn i32_at(b: &[u8], o: usize) -> Option<i32> {
    b.get(o..o + 4).map(|s| i32::from_le_bytes(s.try_into().unwrap()))
}
fn i16_at(b: &[u8], o: usize) -> Option<i16> {
    b.get(o..o + 2).map(|s| i16::from_le_bytes(s.try_into().unwrap()))
}
fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    i16_at(b, o).map(|v| v as u16)
}

/// `Game+0x6d8` `total_units` (`Game::scalars` starts at +0x550).
fn game_total_units(save: &Save) -> i32 {
    i32_at(&save.game.scalars, 0x6d8 - 0x550).unwrap_or(0)
}

/// `Constants` i32 (typed Rules image, falling back to the direct walk).
fn constant(save: &Save, off: usize) -> i32 {
    i32_at(&save.rules_tail.rules.constants, off)
        .or_else(|| i32_at(&save.constants, off))
        .unwrap_or(0)
}

/// `LeaderData` reader over `Save.leaders.slots[who]` (`body` = +0x08..).
struct LeaderRef<'a>(&'a crate::sections::Leader);

impl LeaderRef<'_> {
    fn flags(&self) -> i32 {
        self.0.flags
    }
    /// `LeaderData.who` +0x08.
    fn who(&self) -> i32 {
        i32_at(&self.0.body, 0).unwrap_or(-1)
    }
    /// `LeaderData.diplos[j]` +0x74.
    fn diplo(&self, j: i32) -> i32 {
        usize::try_from(j).ok().and_then(|j| i32_at(&self.0.body, 0x74 - 8 + j * 4)).unwrap_or(0)
    }
}

fn leader(save: &Save, who: i32) -> Option<LeaderRef<'_>> {
    save.leaders.slots.get(usize::try_from(who).ok()?).map(LeaderRef)
}

/// `LeaderData::is_enemy(who)` 0x006ebaa0 on `Leaders[this_who]`.
pub fn leader_is_enemy(save: &Save, this_who: i32, who: i32) -> bool {
    let Some(l) = leader(save, this_who) else { return false };
    let me = l.who();
    who != me && (l.diplo(who) == 0 || leader(save, who).map_or(0, |o| o.diplo(me)) == 0)
}

/// `LeaderData::is_ally(who)` 0x006edb50 on `Leaders[this_who]`.
pub fn leader_is_ally(save: &Save, this_who: i32, who: i32) -> bool {
    let Some(l) = leader(save, this_who) else { return false };
    let me = l.who();
    who == me || (l.diplo(who) == 2 && leader(save, who).map_or(0, |o| o.diplo(me)) == 2)
}

/// `WData` (21 serialized bytes of the 0x1c runtime tile) at tile `(tx, ty)`.
#[derive(Clone, Copy)]
pub struct Tile<'a>(&'a [u8]);

impl Tile<'_> {
    pub fn flags(&self) -> u16 {
        u16_at(self.0, 0).unwrap_or(0)
    }
    pub fn land(&self) -> i8 {
        self.0[2] as i8
    }
    pub fn region(&self) -> i16 {
        i16_at(self.0, 4).unwrap_or(-1)
    }
    /// Head of the per-tile object chain (`WData.down` / `down_who`).
    pub fn down(&self) -> i16 {
        i16_at(self.0, 8).unwrap_or(-1)
    }
    pub fn down_who(&self) -> i16 {
        i16_at(self.0, 0xa).unwrap_or(-1)
    }
    /// `WData.who` +0xf (territory owner, -1 none).
    pub fn who(&self) -> i8 {
        self.0[0xf] as i8
    }
}

fn in_world(save: &Save, tx: i32, ty: i32) -> bool {
    tx >= 0 && ty >= 0 && tx < save.world.xs && ty < save.world.ys
}

pub fn tile(save: &Save, tx: i32, ty: i32) -> Option<Tile<'_>> {
    if !in_world(save, tx, ty) {
        return None;
    }
    let idx = (ty * save.world.xs + tx) as usize;
    save.world.wdata.get(idx * 21..idx * 21 + 21).map(Tile)
}

/// Tile under coord `(x, y)`.
pub fn tile_at_coord(save: &Save, x: i32, y: i32) -> Option<Tile<'_>> {
    tile(save, div3(x >> 8), div3(y >> 8))
}

/// `WorldData::is_ocean(WCoord&, WCoord&)` 0x006b4830: not `flags & 0x100`
/// and `land` ∈ {1, 2}.
pub fn is_ocean(save: &Save, tx: i32, ty: i32) -> bool {
    match tile(save, tx, ty) {
        Some(t) => t.flags() & 0x100 == 0 && matches!(t.land(), 1 | 2),
        None => false,
    }
}

/// Byte-image view of one Object, addressed by retail offset.
#[derive(Clone, Copy)]
pub struct ObjView<'a>(pub &'a Obj);

impl<'a> ObjView<'a> {
    fn sub(&self) -> &'a crate::sections::SubObj {
        match self.0 {
            Obj::Unit(u) => &u.base.sub,
            Obj::Build(b) => &b.base.sub,
            Obj::Animal(a) => &a.unit.base.sub,
        }
    }
    fn mid(&self) -> &'a [u8] {
        match self.0 {
            Obj::Unit(u) => &u.base.mid,
            Obj::Build(b) => &b.base.mid,
            Obj::Animal(a) => &a.unit.base.mid,
        }
    }
    /// `SubObject.flags` +0x08.
    pub fn flags(&self) -> u8 {
        self.sub().flags
    }
    /// `SubObject.who` +0x09.
    pub fn who(&self) -> Option<i32> {
        self.sub().body.first().map(|&b| b as i32)
    }
    /// `x_internal ^ 0x63637` (+0x10).
    pub fn x(&self) -> Option<i32> {
        i32_at(&self.sub().body, 0x10 - 9).map(|v| v ^ COORD_XOR)
    }
    /// `y_internal ^ 0x63637` (+0x14).
    pub fn y(&self) -> Option<i32> {
        i32_at(&self.sub().body, 0x14 - 9).map(|v| v ^ COORD_XOR)
    }
    /// `ptype` (+0x18) serialized as the type index.
    pub fn ptype(&self) -> Option<i32> {
        i32_at(&self.sub().body, 0x18 - 9)
    }
    /// `ObjectData.damage` +0x24.
    pub fn damage(&self) -> Option<i32> {
        i32_at(self.mid(), 0x24 - 0x20)
    }
    /// `ObjectData.down` / `down_who` +0x2c / +0x2e: next link of the
    /// per-tile object chain.
    pub fn down(&self) -> Option<i16> {
        i16_at(self.mid(), 0x2c - 0x20)
    }
    pub fn down_who(&self) -> Option<i16> {
        i16_at(self.mid(), 0x2e - 0x20)
    }
    /// vtable +0x08: `ItemData::is_valid_item` (`flags & 1`) on the
    /// Unit/Animal vtables, `return 0` on Build.
    pub fn vt_is_unit(&self) -> bool {
        match self.0 {
            Obj::Build(_) => false,
            _ => self.flags() & 1 != 0,
        }
    }
    /// vtable +0x0c: `flags & 1` on Build, `return 0` on Unit/Animal.
    pub fn vt_is_build(&self) -> bool {
        match self.0 {
            Obj::Build(_) => self.flags() & 1 != 0,
            _ => false,
        }
    }
    /// vtable +0x4c: `WallData::is_active` (`flags & 4`) on Build,
    /// `flags & 1` on Unit/Animal.
    pub fn vt_is_active(&self) -> bool {
        match self.0 {
            Obj::Build(_) => self.flags() & 4 != 0,
            _ => self.flags() & 1 != 0,
        }
    }
    /// vtable +0xbc: `UnitData::is_on_map` (`inside_up` +0x82 bit 15) on
    /// Unit/Animal, `return 1` on Build.
    pub fn vt_is_on_map(&self) -> Option<bool> {
        match self.0 {
            Obj::Build(_) => Some(true),
            Obj::Unit(u) => i16_at(&u.body, 0x82 - 0x48).map(|v| v < 0),
            Obj::Animal(a) => i16_at(&a.unit.body, 0x82 - 0x48).map(|v| v < 0),
        }
    }
    /// vtable +0x1c: `return 1` on Build, `return 0` on Unit/Animal.
    pub fn vt_is_building_kind(&self) -> bool {
        matches!(self.0, Obj::Build(_))
    }
    /// `WallData.build_masks` +0x60 (Build only).
    pub fn build_masks(&self) -> Option<u16> {
        match self.0 {
            Obj::Build(b) => u16_at(&b.wall_body, 0x60 - 0x48),
            _ => None,
        }
    }
    /// `BuildData.city` +0x72.
    pub fn city(&self) -> Option<i16> {
        match self.0 {
            Obj::Build(b) => i16_at(&b.body, 0x72 - 0x70),
            _ => None,
        }
    }
    /// `BuildData.city_down` +0x74 (next building of the same city).
    pub fn city_down(&self) -> Option<i16> {
        match self.0 {
            Obj::Build(b) => i16_at(&b.body, 0x74 - 0x70),
            _ => None,
        }
    }
}

pub fn obj(save: &Save, who: i32, slot: i32) -> Option<ObjView<'_>> {
    let l = save.objects.lists.get(usize::try_from(who).ok()?)?;
    l.elems.get(usize::try_from(slot).ok()?)?.as_ref().map(ObjView)
}

fn obj_or_todo<'a>(save: &'a Save, who: i32, slot: i32, site: &str) -> Result<ObjView<'a>, String> {
    obj(save, who, slot).ok_or_else(|| format!("TODO({site}) Objects.lists[{who}][{slot}] absent"))
}

/// `Objects` scalar block (`Objects::walk_data` 0x006541e0): `[0x1f4..0x1fc)
/// [0x154..0x15c) unit_mark[9] build_mark[9] wall_mark[9] obj_ctr[9]`.
fn unit_mark(save: &Save, owner: usize) -> i32 {
    i32_at(&save.objects.scalars, 16 + owner * 4).unwrap_or(0)
}
fn build_mark(save: &Save, owner: usize) -> i32 {
    i32_at(&save.objects.scalars, 16 + 36 + owner * 4).unwrap_or(0)
}
fn wall_mark(save: &Save, owner: usize) -> i32 {
    i32_at(&save.objects.scalars, 16 + 72 + owner * 4).unwrap_or(0)
}

/// Type record fields: `ObjectType.attack` +0x1e8 / `domain` +0x218 live in
/// `obj_mid` (image 0x1e4..), `UnitType.role` +0x2c8 and `BuildType`
/// +0x2c0 in `ext` (image 0x2b4..).
struct TypeRef<'a>(&'a crate::sections::TypeRec);

impl TypeRef<'_> {
    fn type_index(&self) -> Option<i32> {
        i32_at(&self.0.head, 0)
    }
    fn attack(&self) -> Option<i32> {
        i32_at(&self.0.obj_mid, 0x1e8 - 0x1e4)
    }
    fn domain(&self) -> Option<i32> {
        i32_at(&self.0.obj_mid, 0x218 - 0x1e4)
    }
    fn unit_role(&self) -> Option<i32> {
        i32_at(&self.0.ext, 0x2c8 - 0x2b4)
    }
    fn build_flags(&self) -> Option<i32> {
        i32_at(&self.0.ext, 0x2c0 - 0x2b4)
    }
}

fn type_rec(save: &Save, idx: i32) -> Option<TypeRef<'_>> {
    save.rules_tail.rules.types.get(usize::try_from(idx).ok()?).map(TypeRef)
}

// ---------------------------------------------------------------------------
// Search::valid_search / valid_filter
// ---------------------------------------------------------------------------

/// `Search::valid_search(int, SearchIndexBH, int)` 0x0067daa0 with ECX =
/// `owner` (the candidate's list). `who < 0` accepts everything.
pub fn valid_search(save: &Save, owner: i32, search: i32, who: i32) -> bool {
    if who < 0 {
        return true;
    }
    match search {
        1 => who == owner,
        2 => !leader_is_ally(save, owner, who) && !leader_is_enemy(save, owner, who),
        3 => leader_is_enemy(save, owner, who),
        4 => leader_is_ally(save, owner, who),
        5 => !leader_is_ally(save, owner, who),
        6 => who != owner,
        7 => !leader_is_enemy(save, owner, who),
        _ => true,
    }
}

/// `Search::valid_filter(int slot, int a, int b, FilterIndex idx)`
/// 0x0067dbb0 with ECX = `owner`; `idx` is the zero-based jump-table arm
/// (`FilterIndex - 1`). Arms transcribed: 8 (`FilterIndex 9`: military —
/// `type.attack != 0 && type.role & 0x10000` for units; for buildings not a
/// city, active, `attack != 0`, `hits_left() != 0`), 10 (`0xb`: unit whose
/// current action targets `(a, b)` — needs `UnitData::get_action`
/// 0x00608450, TODO), 12 (`0xd`: `type.domain == a`).
pub fn valid_filter(save: &Save, owner: i32, slot: i32, a: i32, b: i32, idx: i32) -> Result<bool, String> {
    let o = obj_or_todo(save, owner, slot, "0x0067dbb0")?;
    let t = o
        .ptype()
        .and_then(|p| type_rec(save, p))
        .ok_or_else(|| format!("TODO(0x0067dbb0) type record for Objects.lists[{owner}][{slot}]"))?;
    match idx {
        8 => {
            if o.vt_is_building_kind() {
                // 0x0067de47: city flag, is_active, attack, hits_left.
                if o.flags() & 0x20 != 0 || !o.vt_is_active() || t.attack() == Some(0) {
                    return Ok(false);
                }
                Err("TODO(0x006535c0) ObjectData::hits_left on an attacking building".to_string())
            } else {
                // 0x0067debf
                if t.attack() == Some(0) {
                    return Ok(false);
                }
                Ok(t.unit_role().map_or(false, |r| (r >> 16) & 1 != 0))
            }
        }
        10 => {
            // 0x0067dfa4: vtable +0x18 (`return 1` on Unit/Animal, 0 on
            // Build), then UnitData::get_action()->vt+0xb4 target (+8, +0xc).
            if o.vt_is_building_kind() {
                return Ok(false);
            }
            let _ = (a, b);
            Err(format!("TODO(0x00608450) UnitData::get_action for Objects.lists[{owner}][{slot}] (filter 0xb)"))
        }
        12 => Ok(t.domain() == Some(a)),
        _ => Err(format!("TODO(0x0067dbb0) FilterIndex {} arm", idx + 1)),
    }
}

fn filter_ok(save: &Save, owner: i32, slot: i32, f: i32, a: i32, b: i32) -> Result<bool, String> {
    // `(param == 0 || 0x16 < param - 1U) || valid_filter(...)`
    if f == 0 || (f - 1) as u32 > 0x16 {
        return Ok(true);
    }
    valid_filter(save, owner, slot, a, b, f - 1)
}

// ---------------------------------------------------------------------------
// ObjectsData::find_unit / find_building
// ---------------------------------------------------------------------------

/// Result of a spatial search: `slot` is `-1` when nothing matched; `who`
/// and `dist` are the `ObjectsData::find_who` / `find_dist` scratch values
/// (initialised to the searching `who` and `99999999`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found {
    pub slot: i32,
    pub who: i32,
    pub dist: i32,
}

/// Follow a per-tile object chain starting at the `WData` head, calling
/// `f(owner, slot)` for every link. Guards against a corrupt cycle.
fn walk_chain(save: &Save, head: i16, head_who: i16, mut f: impl FnMut(i32, i32) -> Result<(), String>) -> Result<(), String> {
    let mut slot = head as i32;
    let mut owner = head_who as i32;
    let mut guard = 0;
    while slot >= 0 {
        guard += 1;
        if guard > 1 << 20 {
            return Err("object chain does not terminate".into());
        }
        let o = obj_or_todo(save, owner, slot, "chain")?;
        let next = o.down().ok_or("object mid missing")? as i32;
        let next_who = o.down_who().ok_or("object mid missing")? as i32;
        f(owner, slot)?;
        slot = next;
        owner = next_who;
    }
    Ok(())
}

/// `ObjectsData::find_unit` 0x0065ca80. `search` is the `SearchIndexBH`
/// relation to `who` (3 = enemy), `max_dist` in coords (`< 0` = unlimited),
/// `flags` bit 0x200 restricts to the region of `(x, y)`, 0x20000 drops the
/// `is_on_map` test, 0x10000 requires `is_captain` (TODO). Two optional
/// `FilterIndex` filters with their int args.
#[allow(clippy::too_many_arguments)]
pub fn find_unit(
    save: &Save,
    x: i32,
    y: i32,
    search: i32,
    who: i32,
    max_dist: i32,
    flags: u32,
    f1: i32,
    f1a: i32,
    f1b: i32,
    f2: i32,
    f2a: i32,
    f2b: i32,
) -> Result<Found, String> {
    let mut found = Found { slot: -1, who, dist: 99_999_999 };
    let region_only = flags & 0x200 != 0;
    let reg = if region_only {
        tile_at_coord(save, x, y).map(|t| t.region()).unwrap_or(-1)
    } else {
        -1
    };
    if flags & 0x10000 != 0 {
        return Err("TODO(0x0065ca80) flags & 0x10000 (is_captain filter)".into());
    }
    let c = circle();
    let r = (max_dist + 0x2ff) / 0x300;
    let ring_end = usize::try_from(r).ok().and_then(|r| c.radius.get(r).copied());

    let consider = |found: &mut Found, owner: i32, slot: i32, unlimited_ok: bool| -> Result<(), String> {
        let o = obj_or_todo(save, owner, slot, "0x0065ca80")?;
        if !o.vt_is_unit() {
            return Ok(());
        }
        if flags & 0x20000 == 0 && !o.vt_is_on_map().ok_or("unit body missing")? {
            return Ok(());
        }
        if !filter_ok(save, owner, slot, f1, f1a, f1b)? || !filter_ok(save, owner, slot, f2, f2a, f2b)? {
            return Ok(());
        }
        let (ox, oy) = (o.x().ok_or("sub body missing")?, o.y().ok_or("sub body missing")?);
        if region_only {
            let oreg = tile_at_coord(save, ox, oy).map(|t| t.region()).unwrap_or(-1);
            if oreg != reg {
                return Ok(());
            }
        }
        let d = vector_dist(ox.wrapping_sub(x), oy.wrapping_sub(y));
        if d <= found.dist && ((unlimited_ok && max_dist < 0) || d <= max_dist) {
            found.dist = d;
            found.who = owner;
            found.slot = slot;
        }
        Ok(())
    };

    match ring_end {
        Some(end) if max_dist >= 0 && end <= game_total_units(save) => {
            // Spiral over rings 0..=r around the tile of (x, y).
            let tx = div3(x >> 8);
            let ty = div3(y >> 8);
            for k in 0..end as usize {
                let cx = tx + c.x[k] as i32;
                let cy = ty + c.y[k] as i32;
                let Some(t) = tile(save, cx, cy) else { continue };
                if t.down() < 0 || (region_only && reg != t.region()) {
                    continue;
                }
                walk_chain(save, t.down(), t.down_who(), |owner, slot| {
                    if owner < 8 && (owner < 0 || valid_search(save, owner, search, who)) {
                        consider(&mut found, owner, slot, false)?;
                    }
                    Ok(())
                })?;
            }
        }
        _ => {
            // Full scan of the eight leader-owned Unit planes.
            for owner in 0..8i32 {
                let Some(l) = leader(save, owner) else { continue };
                if l.flags() & 1 == 0 || !valid_search(save, owner, search, who) {
                    continue;
                }
                for slot in 0..unit_mark(save, owner as usize) {
                    if obj(save, owner, slot).is_none() {
                        continue;
                    }
                    consider(&mut found, owner, slot, true)?;
                }
            }
        }
    }
    Ok(found)
}

/// `ObjectsData::find_building` 0x0065d260. Distances are the coarse
/// `div3[coord >> 6]` grid (4 per tile) in both arms; the spiral is used
/// when `0 <= max_dist` and `ceil(max_dist / 0x300) <= 9`.
#[allow(clippy::too_many_arguments)]
pub fn find_building(
    save: &Save,
    x: i32,
    y: i32,
    search: i32,
    who: i32,
    max_dist: i32,
    flags: u32,
    f1: i32,
    f1a: i32,
    f1b: i32,
) -> Result<Found, String> {
    let mut found = Found { slot: -1, who, dist: 99_999_999 };
    let region_only = flags & 0x200 != 0;
    let reg = if region_only {
        tile_at_coord(save, x, y).map(|t| t.region()).unwrap_or(-1)
    } else {
        -1
    };
    let c = circle();
    let r = (max_dist + 0x2ff) / 0x300;

    let consider = |found: &mut Found, owner: i32, slot: i32| -> Result<(), String> {
        let o = obj_or_todo(save, owner, slot, "0x0065d260")?;
        if !o.vt_is_build() || !o.vt_is_active() {
            return Ok(());
        }
        if !filter_ok(save, owner, slot, f1, f1a, f1b)? {
            return Ok(());
        }
        let (ox, oy) = (o.x().ok_or("sub body missing")?, o.y().ok_or("sub body missing")?);
        if region_only {
            let oreg = tile_at_coord(save, ox, oy).map(|t| t.region()).unwrap_or(-1);
            if oreg != reg {
                return Ok(());
            }
        }
        let d = vector_dist(div3(ox >> 6) - div3(x >> 6), div3(oy >> 6) - div3(y >> 6));
        if d <= found.dist && (max_dist < 0 || d <= max_dist) {
            found.dist = d;
            found.who = owner;
            found.slot = slot;
        }
        Ok(())
    };

    if max_dist >= 0 && r <= 9 {
        let end = c.radius[r as usize] as usize;
        let tx = div3(x >> 8);
        let ty = div3(y >> 8);
        for k in 0..end {
            let cx = tx + c.x[k] as i32;
            let cy = ty + c.y[k] as i32;
            let Some(t) = tile(save, cx, cy) else { continue };
            if t.down() < 0 || (region_only && reg != t.region()) {
                continue;
            }
            walk_chain(save, t.down(), t.down_who(), |owner, slot| {
                if owner < 8 && (owner < 0 || valid_search(save, owner, search, who)) {
                    consider(&mut found, owner, slot)?;
                }
                Ok(())
            })?;
        }
    } else {
        for owner in 0..8i32 {
            let Some(l) = leader(save, owner) else { continue };
            if l.flags() & 1 == 0 || !valid_search(save, owner, search, who) {
                continue;
            }
            // obj_base[1..3] = {2000, 3000}; obj_mark[1..3] = build/wall marks.
            for (base, mark) in [(2000, build_mark(save, owner as usize)), (3000, wall_mark(save, owner as usize))] {
                for slot in base..mark {
                    if obj(save, owner, slot).is_none() {
                        continue;
                    }
                    consider(&mut found, owner, slot)?;
                }
            }
        }
    }
    Ok(found)
}

// ---------------------------------------------------------------------------
// Army::find_muster_spot
// ---------------------------------------------------------------------------

/// Army body offsets (class-relative; `Army.body` holds +0x02..+0x98).
mod army_off {
    pub const ARMY: usize = 0x02;
    pub const REG: usize = 0x08;
    pub const NAVY: usize = 0x24;
    pub const X: usize = 0x38;
    pub const Y: usize = 0x3c;
    pub const MUSTER_X: usize = 0x48;
    pub const MUSTER_Y: usize = 0x4c;
    pub const WHO: usize = 0x94;
    pub const NUM_GROUPS: usize = 0x96;
}

struct ArmyRef<'a>(&'a crate::sections::Army);

impl ArmyRef<'_> {
    fn i32(&self, o: usize) -> Option<i32> {
        i32_at(&self.0.body, o - 2)
    }
    fn i16(&self, o: usize) -> Option<i16> {
        i16_at(&self.0.body, o - 2)
    }
}

fn army(save: &Save, who: usize, idx: usize) -> Option<ArmyRef<'_>> {
    save.armies.lists.get(who)?.elems.get(idx)?.as_ref().filter(|a| a.body.len() == 150).map(ArmyRef)
}

/// `CityData` reader (`City.flags` = +0x04, `pod` = +0x06..+0x72).
struct CityRef<'a>(&'a crate::sections::City);

impl CityRef<'_> {
    fn flags(&self) -> u16 {
        self.0.flags
    }
    /// `CityData.o` +0x08 (city-centre build slot).
    fn o(&self) -> Option<i16> {
        i16_at(&self.0.pod, 0x08 - 6)
    }
    /// `CityData.x/y` +0x0c/+0x10 (plain coords).
    fn x(&self) -> Option<i32> {
        i32_at(&self.0.pod, 0x0c - 6)
    }
    fn y(&self) -> Option<i32> {
        i32_at(&self.0.pod, 0x10 - 6)
    }
    /// `CityData+0x5e` (`who`).
    fn who(&self) -> Option<i8> {
        self.0.pod.get(0x5e - 6).map(|&b| b as i8)
    }
    /// `CityData+0x5f` — the byte `BuildData::is_unassimilated` compares
    /// against `SubObject.who` (PDB names it `race`).
    fn byte_5f(&self) -> Option<i8> {
        self.0.pod.get(0x5f - 6).map(|&b| b as i8)
    }
}

fn city(save: &Save, who: i32, idx: i32) -> Option<CityRef<'_>> {
    save.cities
        .lists
        .get(usize::try_from(who).ok()?)?
        .elems
        .get(usize::try_from(idx).ok()?)?
        .as_ref()
        .map(CityRef)
}

/// `BuildData::is_unassimilated` 0x0062d470 on `Objects.lists[who][slot]`.
pub fn build_is_unassimilated(save: &Save, who: i32, slot: i32) -> Result<bool, String> {
    let b = obj_or_todo(save, who, slot, "0x0062d470")?;
    let bw = b.who().ok_or("sub body missing")?;
    let Some(c) = b.city().filter(|&c| c >= 0) else { return Ok(false) };
    let Some(cd) = city(save, bw, c as i32) else { return Ok(false) };
    if cd.byte_5f().map(|v| v as i32) == Some(bw) {
        return Ok(false);
    }
    if b.flags() & 0x20 != 0 {
        return Ok(true);
    }
    let bf = b
        .ptype()
        .and_then(|p| type_rec(save, p))
        .and_then(|t| t.build_flags())
        .ok_or("TODO(0x0062d470) build type record")?;
    Ok((bf >> 4) & 1 == 0)
}

/// `LeaderData::get_radius(TypeIndex)` 0x006db790 on `Leaders[who]`:
/// `city_center_radius + (tier-1) * city_center_pop_radius
/// [+ indians_city_radius if has_tribe_bonus(0x15)]`, capped at 0x40, where
/// tier is 2 for type 0x19f, 3 for 0x1a0/0x213, else 1.
pub fn leader_get_radius(save: &Save, who: i32, type_idx: i32) -> Result<i32, String> {
    let t = type_rec(save, type_idx)
        .and_then(|t| t.type_index())
        .ok_or_else(|| format!("TODO(0x006db790) type record {type_idx}"))?;
    let tier = match t {
        0x19f => 2,
        0x1a0 | 0x213 => 3,
        _ => 1,
    };
    let mut r = (tier - 1) * constant(save, 0x130);
    let l = save
        .leaders
        .slots
        .get(usize::try_from(who).map_err(|_| "leader index")?)
        .ok_or("leader slot")?;
    let ctx = Ctx {
        frame: i32_at(&save.game.scalars, crate::tick::FRAME).unwrap_or(0),
        game: &save.game,
        rules: &save.rules_tail.rules,
        constants_fallback: &save.constants,
    };
    let ev = Eval { ctx: &ctx, l };
    if ev.has_tribe_bonus(0x15).ok_or("TODO(0x006e1370) has_tribe_bonus")? {
        r += constant(save, 0x88c);
    }
    r += constant(save, 0x12c);
    Ok(r.min(0x40))
}

/// `CityData::get_radius` 0x00738410: `Leaders[city.who].get_radius(type of
/// Objects.lists[city.who][city.o])`, capped at 0x40.
pub fn city_get_radius(save: &Save, who: i32, idx: i32) -> Result<i32, String> {
    let c = city(save, who, idx).ok_or_else(|| format!("TODO(0x00738410) Cities.lists[{who}][{idx}] absent"))?;
    let cw = c.who().ok_or("city pod")? as i32;
    let co = c.o().ok_or("city pod")? as i32;
    let b = obj_or_todo(save, cw, co, "0x00738410")?;
    let pt = b.ptype().ok_or("sub body missing")?;
    Ok(leader_get_radius(save, cw, pt)?.min(0x40))
}

/// `Army::count(CountIndex, int)` 0x006f9120: sums `GroupData::count`
/// 0x00711720 over the attached groups. Zero with no groups; TODO otherwise.
fn army_count(a: &ArmyRef, what: i32) -> Result<i32, String> {
    if a.i16(army_off::NUM_GROUPS).unwrap_or(0) <= 0 {
        return Ok(0);
    }
    Err(format!("TODO(0x00711720) Army::count({what}) with groups attached"))
}

/// What `Army::find_muster_spot` would write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusterSpot {
    /// The function's return value (`local_c`).
    pub result: i32,
    /// `Army.hurry` (+0x2c), always written.
    pub hurry: i32,
    /// `(muster_x, muster_y, muster_angle)` when written.
    pub muster: Option<(i32, i32, i32)>,
    /// `(who, city, new city_flags)` when the own city's flags are touched.
    pub city_flags: Option<(i32, i32, u16)>,
}

/// `Army::find_muster_spot(int o, int who, int hurry)` 0x006f5cc0 for
/// `Armies.lists[army_who][army_idx]`; `o` is a Build slot of `who`'s plane
/// (the city centre in `do_mustering`). Pure: returns the writes.
pub fn find_muster_spot(save: &Save, army_who: usize, army_idx: usize, o: i32, who: i32, hurry: i32) -> Result<MusterSpot, String> {
    let a = army(save, army_who, army_idx).ok_or("army absent")?;
    let a_who = a.i16(army_off::WHO).ok_or("army body")? as i32;
    let a_army = a.i16(army_off::ARMY).ok_or("army body")? as i32;
    let navy = a.i32(army_off::NAVY).ok_or("army body")?;
    let a_reg = a.i32(army_off::REG).ok_or("army body")?;
    let ax = a.i32(army_off::X).ok_or("army body")?;
    let ay = a.i32(army_off::Y).ok_or("army body")?;

    let mut out = MusterSpot { result: 0, hurry: 0, muster: None, city_flags: None };
    let mut result = 0;

    let my_leader = leader(save, a_who).ok_or("leader absent")?;
    let my_who = my_leader.who();
    // who == Leaders[a_who].who || mutual diplos == 2  (inline is_ally)
    let ally = who == my_who || (my_leader.diplo(who) == 2 && leader(save, who).map_or(0, |l| l.diplo(my_who)) == 2);

    let world_ys = save.world.ys;
    let tile_of = |o: ObjView| -> Result<(i32, i32), String> {
        Ok((div3(o.x().ok_or("sub body")? >> 8), div3(o.y().ok_or("sub body")? >> 8)))
    };

    if ally && navy == 0 {
        // muster_x = muster_y = -1 (+0x48/+0x4c), refined below.
        let mut muster_x = -1;
        let mut muster_y = -1;
        if o >= 0 {
            let mut slot = o;
            loop {
                let b = obj_or_todo(save, who, slot, "0x006f5cc0")?;
                if b.damage().ok_or("mid")? != 0 && b.build_masks().ok_or("wall body")? & 0x20 != 0 {
                    let (bx, by) = (b.x().ok_or("sub body")?, b.y().ok_or("sub body")?);
                    let mut f = find_unit(save, bx, by, 3, a_who, 0x900, 0x200, 9, 0, 0, 0xd, 0, 0)?;
                    if f.slot < 0 {
                        f = find_unit(save, bx, by, 3, a_who, 0x1200, 0x200, 0xb, slot, who, 0xd, 0, 0)?;
                        if f.slot < 0 && army_count(&a, 4)? != 0 {
                            f = find_building(save, bx, by, 3, a_who, 0x1200, 0x200, 9, 0, 0)?;
                        }
                    }
                    if f.slot >= 0 && f.who >= 0 {
                        let t = obj_or_todo(save, f.who, f.slot, "0x006f5cc0")?;
                        let (mx, my) = tile_of(t)?;
                        // 0x006f5ffa: the angle is taken from the city centre
                        // `Objects.lists[who][o]` ([ebp+8] = o) to the muster tile.
                        let (bx, by) = tile_of(obj_or_todo(save, who, o, "0x006f5cc0")?)?;
                        out.muster = Some((mx, my, find_angle(mx - bx, my - by)));
                        out.hurry = 1;
                        out.result = 1;
                        return Ok(out);
                    }
                    if muster_x < 0 || muster_y < 0 {
                        (muster_x, muster_y) = tile_of(b)?;
                    }
                }
                let next = b.city_down().ok_or("build body")? as i32;
                if next < 0 {
                    break;
                }
                slot = next;
            }
        }
        if muster_x < 0 || muster_y < 0 {
            (muster_x, muster_y) = tile_of(obj_or_todo(save, who, o, "0x006f5cc0")?)?;
        }
        muster_y = if muster_y < world_ys - 1 { muster_y + 1 } else { muster_y - 1 };
        // 0x006f604f: find_angle from the tile of Objects.lists[who][o].
        let (bx, by) = tile_of(obj_or_todo(save, who, o, "0x006f5cc0")?)?;
        out.muster = Some((muster_x, muster_y, find_angle(muster_x - bx, muster_y - by)));
    }

    // --- Spiral search around the army position -------------------------
    let tx = div3(ax >> 8);
    let ty = div3(ay >> 8);
    let center = obj_or_todo(save, who, o, "0x006f5cc0")?;
    let center_is_city = center.flags() & 0x20 != 0;
    let r0 = if !center_is_city || navy != 0 {
        5
    } else {
        let cidx = center.city().ok_or("build body")? as i32;
        let cd = city(save, who, cidx).ok_or_else(|| format!("TODO(0x006f5cc0) Cities.lists[{who}][{cidx}] absent"))?;
        let cw = cd.who().ok_or("city pod")? as i32;
        let co = cd.o().ok_or("city pod")? as i32;
        if build_is_unassimilated(save, cw, co)? {
            4
        } else {
            let rad = city_get_radius(save, who, cidx)?;
            let mut r = rad / 4 + 1; // (x + (x>>31 & 3)) >> 2 : signed div toward zero
            if army_count(&a, 4)? == 0 && a_who != who {
                r /= 2;
            }
            r
        }
    };
    let kmax = if hurry == 0 { 9 } else { 0x31 };
    let r1 = (r0 + (navy * 3 + 1) * 2).min(0x40);
    let c = circle();
    let i_start = c.radius[r0.clamp(0, 0x40) as usize];
    let i_end = c.radius[r1.clamp(0, 0x40) as usize];
    if r0 < 0 || r0 > 0x40 {
        return Err(format!("TODO(0x006f5cc0) ring index {r0} outside circle_radius"));
    }

    let mut have = false;
    let mut best_score = 0;
    let mut best_ring = 999_999;
    let mut best = (0, 0);
    let leader_flags = my_leader.flags();

    let mut i = i_start;
    while i < i_end {
        let cx = tx + c.x[i as usize] as i32;
        let cy = ty + c.y[i as usize] as i32;
        if in_world(save, cx, cy) {
            let mut score = 0;
            let mut rejected = false;
            // No other valid army of ours mustering within 4 (ally of `who`) / 2 tiles.
            let n = save.armies.lists.get(a_who as usize).map_or(0, |l| l.len.max(0));
            let mut j = 0;
            let mut aborted = false;
            while j < n {
                if j != a_army {
                    if let Some(other) = army(save, a_who as usize, j as usize) {
                        if other.0.valid != 0 {
                            let who_leader = leader(save, who).ok_or("leader absent")?;
                            let who_who = who_leader.who();
                            let ally_of_who = a_who == who_who
                                || (who_leader.diplo(a_who) == 2 && my_leader.diplo(who_who) == 2);
                            let omx = other.i32(army_off::MUSTER_X).ok_or("army body")?;
                            let omy = other.i32(army_off::MUSTER_Y).ok_or("army body")?;
                            let d = vector_dist(cx.wrapping_sub(omx), cy.wrapping_sub(omy));
                            if d <= if ally_of_who { 4 } else { 2 } {
                                aborted = true;
                                break;
                            }
                        }
                    }
                }
                j += 1;
            }
            if !aborted {
                let mut k = 0;
                while k < kmax {
                    let px = cx + MOVE_X[k];
                    let py = cy + MOVE_Y[k];
                    if !in_world(save, px, py) {
                        result = 0;
                        rejected = true;
                        break;
                    }
                    let eval = if navy == 0 {
                        let qx = px + MOVE_X[k];
                        let qy = py + MOVE_Y[k];
                        if let Some(q) = tile(save, qx, qy) {
                            let w = q.who() as i32;
                            if w != -1 && w != my_who && !(my_leader.diplo(w) == 2 && leader(save, w).map_or(0, |l| l.diplo(my_who)) == 2) && w != who {
                                result = 0;
                                rejected = true;
                                break;
                            }
                        }
                        k < 9
                    } else {
                        k < 0x19
                    };
                    if eval {
                        let t = tile(save, px, py).ok_or("tile")?;
                        let region = t.region() as i32;
                        if a_reg == region || (navy == 0 && leader_flags & 0x700 != 0 && region < 0x41) {
                            let fl = t.flags();
                            let land = t.land() as i32;
                            let ltype = if fl & 4 != 0 {
                                3
                            } else if fl & 0x20 != 0 {
                                4
                            } else if fl & 0x50 != 0 {
                                5
                            } else if fl & 8 != 0 {
                                ((fl & 0x800) | 0x3000) as i32 >> 11
                            } else if is_ocean(save, px, py) && fl & 0x800 != 0 {
                                7
                            } else {
                                land
                            };
                            let water = fl & 0x100 == 0 && matches!(land, 1 | 2);
                            let bad = if navy == 0 {
                                if water || ltype == 5 || ltype == 4 {
                                    result = 0;
                                    rejected = true;
                                    break;
                                }
                                ltype == 3
                            } else {
                                !water
                            };
                            if bad {
                                result = 0;
                                rejected = true;
                                break;
                            }
                            if fl & 0x4000 == 0 {
                                let mr = save
                                    .lands
                                    .list
                                    .elems
                                    .get(usize::try_from(ltype).map_err(|_| "land type")?)
                                    .and_then(|l| i32_at(&l.pod, 0x100))
                                    .ok_or_else(|| format!("TODO(0x006f5cc0) Lands[{ltype}].move_rate"))?;
                                score += mr;
                            } else if navy != 0 && k > 8 {
                                score += 10;
                            }
                            result = 1;
                        } else {
                            result = 0;
                            rejected = true;
                            break;
                        }
                    }
                    k += 1;
                }
            }
            if !rejected && result != 0 && best_score < score {
                best_score = score;
                have = true;
                best = (cx, cy);
                best_ring = i;
            }
            if i - best_ring > 0x28 {
                result = 1;
                break;
            }
        }
        i += 1;
    }

    if have {
        let (bx, by) = best;
        out.muster = Some((bx, by, find_angle(bx - tx, by - ty)));
        out.result = result;
        if center_is_city && a_who == who {
            if let Some(cidx) = center.city().filter(|&c| c >= 0) {
                if let Some(cd) = city(save, a_who, cidx as i32) {
                    let fl = cd.flags();
                    if fl & 1 != 0 {
                        out.city_flags = Some((a_who, cidx as i32, fl & 0xdfff));
                    }
                }
            }
        }
        return Ok(out);
    }

    if navy == 0 && center_is_city {
        if !ally || center.damage().ok_or("mid")? != 0 {
            let cidx = center.city().ok_or("build body")? as i32;
            let cd = city(save, who, cidx).ok_or_else(|| format!("TODO(0x006f5cc0) Cities.lists[{who}][{cidx}] absent"))?;
            let mx = div3(cd.x().ok_or("city pod")? >> 8);
            let my = div3(cd.y().ok_or("city pod")? >> 8);
            let my = if my < world_ys - 1 { my + 1 } else { my - 1 };
            out.muster = Some((mx, my, find_angle(mx - tx, my - ty)));
            out.result = 1;
            return Ok(out);
        }
    }
    if center_is_city && a_who == who {
        if let Some(cidx) = center.city().filter(|&c| c >= 0) {
            if let Some(cd) = city(save, who, cidx as i32) {
                let fl = cd.flags();
                if fl & 3 == 1 {
                    out.city_flags = Some((who, cidx as i32, fl | 0x2000));
                }
            }
        }
    }
    out.result = result;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn capture_dirs() -> Vec<PathBuf> {
        let Ok(root) = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize() else {
            return Vec::new();
        };
        let pairs = root.join("schema/live/frame-pairs");
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(pairs) {
            for e in rd.flatten() {
                let d = e.path();
                if d.is_dir() && d.join("manifest.json").is_file() {
                    out.push(d);
                }
            }
        }
        out.sort();
        out
    }

    fn game_frame(save: &Save) -> i32 {
        i32_at(&save.game.scalars, crate::tick::FRAME).unwrap()
    }

    fn frames(dir: &Path) -> Vec<(i32, Save)> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.extension().map_or(false, |x| x == "svx") {
                let raw = crate::container::load_svx(&p).unwrap();
                let img = crate::load(&raw).unwrap();
                out.push((game_frame(&img.state), img.state));
            }
        }
        out.sort_by_key(|(f, _)| *f);
        out
    }

    #[test]
    fn circle_tables_match_circle_init_shape() {
        let c = circle();
        // Ring 0 is the origin alone; ring 1 is the 8-neighbourhood.
        assert_eq!(c.radius[0], 1);
        assert_eq!((c.x[0], c.y[0]), (0, 0));
        assert_eq!(c.radius[1], 9);
        for k in 1..9 {
            assert_eq!(vector_dist(c.x[k] as i32, c.y[k] as i32), 1);
        }
        // Monotone, every entry's distance equals its ring.
        for r in 1..=0x40usize {
            assert!(c.radius[r] >= c.radius[r - 1]);
            for k in c.radius[r - 1]..c.radius[r] {
                assert_eq!(vector_dist(c.x[k as usize] as i32, c.y[k as usize] as i32), r as i32);
            }
        }
        assert_eq!(c.x.len() as i32, c.radius[0x40]);
    }

    #[test]
    fn find_angle_axes() {
        assert_eq!(find_angle(0, 0), i32::MIN);
        assert_eq!(find_angle(0, -1), 0);
        assert_eq!(find_angle(0, 1), i32::MIN);
        assert_eq!(find_angle(1, 0), 0x4000_0000);
        assert_eq!(find_angle(-1, 0), 0xc000_0000u32 as i32);
        // Diagonals land on the odd eighths.
        assert_eq!(find_angle(1, -1), 0x2000_0000);
        assert_eq!(find_angle(1, 1), 0x6000_0000);
    }

    /// Oracle: on every stride-15 pair, each Army whose bytes retail moved
    /// has to be reproduced by `find_muster_spot(city.o, who, 1)` on the
    /// frame-N state (the `do_mustering` call site, 0x006f4260): retail
    /// N+15 `muster_x/y/angle` must equal ours and `status` must have gained
    /// 0x10 exactly when our `result != 0`.
    #[test]
    fn stride15_muster_writes_reproduced() {
        let mut checked = 0;
        for dir in capture_dirs() {
            let fr = frames(&dir);
            for w in fr.windows(2) {
                let ((fa, a), (fb, b)) = (&w[0], &w[1]);
                if fb - fa != 15 {
                    continue;
                }
                for who in 0..8usize {
                    let n = a.armies.lists.get(who).map_or(0, |l| l.elems.len());
                    for idx in 0..n {
                        let (Some(Some(ra)), Some(Some(rb))) =
                            (a.armies.lists[who].elems.get(idx), b.armies.lists[who].elems.get(idx))
                        else {
                            continue;
                        };
                        if ra.valid == 0 || ra.body == rb.body {
                            continue;
                        }
                        let g = |body: &[u8], o: usize| i32::from_le_bytes(body[o - 2..o + 2].try_into().unwrap());
                        let city_idx = g(&ra.body, 0x20);
                        let army_who = i16::from_le_bytes(ra.body[0x94 - 2..0x94].try_into().unwrap()) as i32;
                        let c = city(a, army_who, city_idx).expect("army city");
                        let co = c.o().unwrap() as i32;
                        let got = find_muster_spot(a, who, idx, co, army_who, 1)
                            .unwrap_or_else(|e| panic!("f{fa}->f{fb} Army[{who}][{idx}]: {e}"));
                        let want = (g(&rb.body, 0x48), g(&rb.body, 0x4c), g(&rb.body, 0x50));
                        let st_a = g(&ra.body, 0x04) as u32;
                        let st_b = g(&rb.body, 0x04) as u32;
                        eprintln!(
                            "f{fa}->f{fb} Army[{who}][{idx}] city {city_idx} o {co}: retail muster {want:?} status {st_a:#x}->{st_b:#x}; ours {got:?}"
                        );
                        assert_eq!(got.muster, Some(want), "f{fa}->f{fb} Army[{who}][{idx}] muster_x/y/angle");
                        assert_eq!(st_b & 0x10 != 0 && st_a & 0x10 == 0, got.result != 0, "status bit 0x10 vs result");
                        checked += 1;
                    }
                }
            }
        }
        if capture_dirs().is_empty() {
            eprintln!("SKIP: proprietary live captures absent (schema/live/frame-pairs/*)");
            return;
        }
        assert!(checked >= 7, "expected the f220..f265 muster armies, checked {checked}");
    }
}
