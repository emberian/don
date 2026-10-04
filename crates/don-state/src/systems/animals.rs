//! `Animal::do_idle` 0x005d7460 (the herd-animal wander roll and its
//! `Unit::add_move_order` 0x00616ed0 write), `Herd::process` 0x00741760 and
//! the owner-rotation tail of `Objects::process_all` that calls it
//! (0x0065e04d), and the `Animal::work` 0x005d7330 herd seen-mask tail.
//! Leaf module: exposes free functions for the owning traversal
//! (`objects_process`) to call; never edits sibling modules.
//!
//! Every function here is a literal transcription of the Capstone listing
//! of the mapped image (`re/decomp-all/<EA>.c` for control flow, the
//! disassembly for argument registers / signed-modulo idioms / the `||`
//! in the anim check). Offsets are PDB `UnitData` / `GuyData` / `Herd`
//! (`re/scripts/pdb_layout.py`).
//!
//! # `Animal::do_idle` 0x005d7460 (657 B) — `Unit::do_job` case 0
//!
//! ```text
//! 005d7474  Unit::set_anim_all(0,0,1) 0x00616f40       ; every guy: hold_attack=0 unless
//!                                                     ;   guys[0] is in an attack anim;
//!                                                     ;   Guy::set_anim(0,0,1) 0x005da300
//! 005d747b  collide (+0x88) = 0
//! 005d7485  if ptype->+0x218 (movement plane) != 0: return      ; land animals only
//! 005d7492  if herd (+0x86) < 0: return Animal::do_idle_free 0x005d7700
//! 005d74b6  g0 = guys.list[0]; if g0.cur_time != g0.end_time - 1: return
//! 005d74c5  if !(g0.cur_anim >= 1 || g0.cur_anim <= 3): return   ; always false for a char
//! 005d74de  r = Random::get(0,0xffff)                           ; draw 1
//! 005d74eb  if r % 10 >= 3: return                              ; 70 %: stay put
//! 005d7509  H = Herds.list[herd]; hx = ((H.wx + 2*H.cx)*0x300 + 0x480)/3
//!                                 hy = ((H.wy + 2*H.cy)*0x300 + 0x480)/3
//! 005d7576  d = vector_dist(|x-hx|, |y-hy|) 0x0046cff0
//! 005d7580  if d <= 0x180:                                      ; within half a tile of the herd
//!   005d75ff   dir = Random::get(0,0xffff) % 8                  ; draw 2 (signed %, 0..7 here)
//!   005d762f   n1  = Random::get(0,0xffff) % 4                  ; draw 3
//!   005d7647   nx  = (n1+1) * move_x[dir] * 0x30 + x
//!   005d766d   n2  = Random::get(0,0xffff) % 4                  ; draw 4
//!   005d7682   ny  = (n2+1) * move_y[dir] * 0x30 + y
//!   005d76a3   if !World::valid_coord(&nx,&ny) 0x0043f360: return
//!   005d76bd   if Unit::detect_unit_collision(nx,ny,1,1,0,0,0) 0x00617060 != 0: return
//!   005d76e5   Unit::add_move_order(nx,ny,1,0,2,0,<ecx junk>,-1,-1)
//!            else:
//!   005d75b6   if UnitType::find_nearby_spot(hx,hy,&sx,&sy,0xc0,-1,0,0x55555555,3,o,who,0,0,-1,0,-1)
//!                 0x0061de70 != 0: return                      ; this = ptype
//!   005d75e6   Unit::add_move_order(sx,sy,1,0,2,0,<ecx junk>,-1,-1)
//! ```
//!
//! `move_x` = `DAT_00adcaf4[0..8]` = `[-1,0,1,1,1,0,-1,-1]`, `move_y` =
//! `DAT_00adc404[0..8]` = `[-1,-1,-1,0,1,1,1,0]` (read from `.rdata`; they
//! are `move_x@@3QBHB + 1` / `move_y@@3QBHB + 1`). The anim test at
//! 0x005d74c5 is `cmp al,1; jge ok; cmp al,3; jg skip` — `anim >= 1 ||
//! anim <= 3`, which no signed byte fails; it is dead.
//!
//! So a herd animal draws exactly **1** on the frame its idle animation is
//! about to end (`cur_time == end_time - 1`) and **4** when that roll
//! succeeds (30 %) *and* it is within `0x180` of the herd centre. The same
//! frame, step 15 (`Guy::inc_time`) advances `cur_time` to `end_time` and
//! `Guy::set_anim` re-rolls the idle variant — one more draw per guy when
//! `openlist` (+0x104) is null (`objects_inc_time` reports that site). So
//! an idle herd animal costs 2 or 5 draws on its anim-end frame and 0
//! otherwise.
//!
//! # `Unit::add_move_order` 0x00616ed0 → `Unit::add_move_order_ex` 0x005e55c0
//!
//! ```text
//! angle   = find_angle(x - unit.x, y - unit.y) 0x0092d130      ; UNSNAPPED target
//! tile_x  = T[x >> 4], tile_y = T[y >> 4]                      ; T[i] = floor(i/3) (0x00681db0)
//! → add_move_order_ex(tile_x, tile_y, angle, 1, 0, 2, 0, -1, -1, -1, 0):
//!   unit_masks &= ~0x4000000; path.length = 0                  ; walked: PathStack emptied
//!   Unit::close_orders(0) 0x005e37f0  (no-op: list is empty when do_idle runs)
//!   Unit::clear_partial_path 0x005e3920 (runtime pathfinder trees only)
//!   Unit::update_action 0x0060a870
//!   o = OrdersMemManager::get_obj(1) 0x00730ac0                ; recycled → MoveOrder::clear
//!                                                              ; 0x004889c0, new → ctor 0x00488a10
//!   o.x = tile_x*0x30+0x18; o.y = tile_y*0x30+0x18; o.angle = angle; o.dest = 0
//!   o.dest_x = o.x; o.dest_y = o.y; o.last_x = o.last_y = -1
//!   flags &= ~1; o.off_x = o.x % 0x300; o.off_y = o.y % 0x300
//!   o.orig_x = o.orig_y = -1; o.facing = -1; o.pause = o.retry = o.timer = 0
//!   flags &= ~4; flags &= ~0x20
//!   LinkListBase::add 0x0046d5a0   ; new node becomes head; walk order is head->prev …
//!                                  ; → head, so the new order serializes LAST
//!   Unit::update_action 0x0060a870 ; orders_x/y = o.x/o.y, dest_angle = o.angle
//! ```
//!
//! `MoveOrder::clear` / the ctor zero every field except `facing` (-1) and
//! `orig_x/orig_y` (set by `add_move_order_ex` anyway), so the 77-byte
//! payload is fully determined: see [`move_order_payload`].
//!
//! # Stopping points
//!
//! * `Unit::detect_unit_collision` 0x00617060 and
//!   `UnitType::find_nearby_spot` 0x0061de70 are the `objects_query` lane's.
//!   Until they land, [`do_idle`] performs every write *before* those calls
//!   (hold_attack, collide, the RNG draws) and reports the candidate
//!   destination in `effects` without adding the order. The capture
//!   oracle shows the deer taking the far branch (`d > 0x180`; the
//!   observed order destinations are 5 cells away, outside the near
//!   branch's ±4-cell reach), so its destinations need `find_nearby_spot`.
//! * `Animal::do_idle_free` 0x005d7700 (herd == -1): no such animal in any
//!   capture (all 41 are herd members); untranscribed, reported.
//! * `Guy::set_anim(0,0,1)` inside `set_anim_all`: for an idle-class anim
//!   with `cur_time < end_time` it returns before writing anything — the
//!   only case reachable here, because step 15 has already re-rolled any
//!   guy whose clock ran out. The other arms need the `.bha` animation
//!   packets (`end_time`), as `objects_inc_time` documents.
//!
//! # Owner 9 (the unserialized Nature band)
//!
//! `Objects::walk_data` 0x006541e0 walks nine owner lists and nine
//! `unit_mark`s; wildlife spawned by `Objects::init_unit(9, 0x192, …)`
//! 0x0065e0c0 from the respawn tail lives in the tenth. Nothing of it is in
//! a save: the oracle cannot see those animals, their orders, or their
//! guys' clocks, so (a) their `do_idle` draws are invisible to a per-frame
//! draw count derived from the save, and (b) the respawn tail's own
//! `existing` count (and hence its `2 × (quota − existing)` draws every
//! 32nd frame) is unknowable from the save. Every animal in the captures
//! is owner 8 (the serialized Nature band, `who == 8`); the per-frame
//! animal draw budget below is for that band only.

#![allow(dead_code)]

use crate::sections::{Obj, Order, Save, Unit};
use crate::tick::StepStatus;

pub const STATUS: StepStatus = StepStatus::Partial;

// ---------------------------------------------------------------------------
// Tables read from the image
// ---------------------------------------------------------------------------

/// `DAT_00adcaf4[0..8]` (`move_x + 1`).
pub const MOVE_X: [i32; 8] = [-1, 0, 1, 1, 1, 0, -1, -1];
/// `DAT_00adc404[0..8]` (`move_y + 1`).
pub const MOVE_Y: [i32; 8] = [-1, -1, -1, 0, 1, 1, 1, 0];

/// `DAT_00af4370`: UnitAnim → anim class (same table `objects_inc_time`
/// reads; class 0 = idle variants, 8 = walk, 0xc = attack).
const ANIM_CLASS: [i32; 38] = [
    0, 0, 0, 0, 0, 0, 0, 8, 8, 8, 10, 12, 12, 12, 12, 15, 15, 15, 15, 15, 15, 21, 22, 23, 24, 25, 8, 27, 8, 29, 8, 31, 8, 33, 34, 35,
    36, 37,
];

fn anim_class(anim: i8) -> i32 {
    ANIM_CLASS.get(anim as usize).copied().unwrap_or(anim as i32)
}

// ---------------------------------------------------------------------------
// Pure arithmetic helpers
// ---------------------------------------------------------------------------

/// `vector_dist(dx, dy)` 0x0046cff0 (fastcall ECX/EDX): octagonal
/// approximation `max + min²/(2·max)`, or `(min + 2·max)/2` when `min >
/// 59999`.
pub fn vector_dist(dx: i32, dy: i32) -> i32 {
    let a = dx.wrapping_abs() as u32;
    let b = dy.wrapping_abs() as u32;
    let (max, min) = if (b as i32) < (a as i32) { (a, b) } else { (b, a) };
    if max == 0 {
        return 0;
    }
    if min > 59999 {
        return (min.wrapping_add(max.wrapping_mul(2)) >> 1) as i32;
    }
    (min.wrapping_mul(min) / max.wrapping_mul(2)).wrapping_add(max) as i32
}

/// `find_angle(dx, dy)` 0x0092d130 (fastcall ECX/EDX), the 32-bit binary
/// angle of the vector `(dx, -dy)`.
pub fn find_angle(dx: i32, dy: i32) -> i32 {
    let ndy = dy.wrapping_neg();
    if dx == 0 {
        return if ndy > 0 { 0 } else { i32::MIN };
    }
    if ndy == 0 {
        return if dx > 0 { 0x4000_0000 } else { 0xc000_0000u32 as i32 };
    }
    let adx = dx.wrapping_abs();
    let ady = ndy.wrapping_abs();
    // ebx = 1 when |dx| > |dy| (the "swapped" octant).
    let (q, swapped) = if adx > ady { ((ady << 14) / adx, true) } else { ((adx << 14) / ady, false) };
    let t = (0x1333 - q).wrapping_abs();
    let a = (0x2800 - ((t.wrapping_mul(0xb00)) >> 14)).wrapping_mul(q) & 0xffff_c000u32 as i32;
    let a = a.wrapping_shl(2);
    if dx > 0 {
        if ndy > 0 {
            if swapped { 0x4000_0000i32.wrapping_sub(a) } else { a }
        } else if swapped {
            a.wrapping_add(0x4000_0000)
        } else {
            i32::MIN.wrapping_sub(a)
        }
    } else if ndy > 0 {
        if swapped { a.wrapping_add(0xc000_0000u32 as i32) } else { a.wrapping_neg() }
    } else if swapped {
        (0xc000_0000u32 as i32).wrapping_sub(a)
    } else {
        a.wrapping_sub(i32::MIN)
    }
}

/// x86 `idiv` remainder (sign follows the dividend), as the `and
/// 0x8000000N / dec / or / inc` idiom computes it.
fn srem(a: i32, m: i32) -> i32 {
    a.wrapping_rem(m)
}

/// `DAT_00cae5fc[i]` = `floor(i / 3)` for negative and positive `i`
/// (0x00681db0 fills both halves), applied to `coord >> 4`: the 48-unit
/// sub-cell index of a coordinate.
fn cell(coord: i32) -> i32 {
    (coord >> 4).div_euclid(3)
}

// ---------------------------------------------------------------------------
// State readers (private copies, as objects_process keeps its own)
// ---------------------------------------------------------------------------

fn frame(save: &Save) -> i32 {
    i32::from_le_bytes(save.game.scalars[0..4].try_into().unwrap())
}

const GAME_RANDOM: usize = 0x28;

/// `Random::get(min, max)` 0x00a39d70 on `GameAccess::game_random`.
fn game_random(save: &mut Save, min: i32, max: i32) -> i32 {
    let (lo, hi) = if max < min { (max, min) } else { (min, max) };
    if lo == hi {
        return min;
    }
    let seed = u32::from_le_bytes(save.post_world[GAME_RANDOM..GAME_RANDOM + 4].try_into().unwrap());
    let seed = crate::tick::rng_step(seed);
    save.post_world[GAME_RANDOM..GAME_RANDOM + 4].copy_from_slice(&seed.to_le_bytes());
    (((seed & 0xffff) as i32).wrapping_mul(hi - lo) as u32 >> 16) as i32 + lo
}

fn leader_flags(save: &Save, who: usize) -> i32 {
    save.leaders.slots.get(who).map(|l| l.flags).unwrap_or(0)
}

/// `ObjectType +0x218` (movement plane) and `+0x304` (num primary guys)
/// of `Rules.types[ptype]`.
fn type_kind_and_guys(save: &Save, ptype: i32) -> (i32, i32) {
    let Some(t) = usize::try_from(ptype).ok().and_then(|i| save.rules_tail.rules.types.get(i)) else {
        return (0, i32::MAX);
    };
    let rd = |v: &Vec<u8>, i: usize| v.get(i..i + 4).map(|b| i32::from_le_bytes(b.try_into().unwrap()));
    let kind = rd(&t.obj_mid, 0x218 - 0x1e4).unwrap_or(0);
    // ext = [0x2b4..0x2cc) ++ [0x2d4..0x2dc) ++ [0x2dc..0x2e0) ++ [0x2e0..0x5d4)
    let num_guys = rd(&t.ext, 0x304 - 0x2b4 - 8).unwrap_or(i32::MAX);
    (kind, num_guys)
}

/// `Herd` image: `cx,cy,wx,wy,t,good_o: i32; herd: i16; herd_flags: i8`.
#[derive(Clone, Copy, Debug, Default)]
pub struct HerdImg {
    pub cx: i32,
    pub cy: i32,
    pub wx: i32,
    pub wy: i32,
    pub good_o: i32,
    pub flags: i8,
}

pub fn herd(save: &Save, idx: usize) -> Option<HerdImg> {
    let s = &save.herds.list.elems.get(idx)?.as_ref()?.slot;
    if s.len() != 27 {
        return None;
    }
    let rd = |o: usize| i32::from_le_bytes(s[o..o + 4].try_into().unwrap());
    Some(HerdImg { cx: rd(0), cy: rd(4), wx: rd(8), wy: rd(12), good_o: rd(20), flags: s[26] as i8 })
}

/// Herd wander centre, 0x005d7509..0x005d7555: `((w + 2c)*0x300 + 0x480)/3`
/// — exact, since both terms are multiples of 3: `(2c + w)*0x100 + 0x180`.
pub fn herd_centre(h: &HerdImg) -> (i32, i32) {
    let hx = ((h.wx + h.cx * 2).wrapping_mul(0x300).wrapping_add(0x480)) / 3;
    let hy = ((h.wy + h.cy * 2).wrapping_mul(0x300).wrapping_add(0x480)) / 3;
    (hx, hy)
}

// ---------------------------------------------------------------------------
// Unit / Guy image accessors (serialized sub-ranges, retail offsets)
// ---------------------------------------------------------------------------

fn u_i32(u: &Unit, off: usize) -> i32 {
    let (v, i) = match off {
        0x09..=0x1b => (&u.base.sub.body, off - 0x09),
        0x20..=0x41 => (&u.base.mid, off - 0x20),
        0x48..=0xb6 => (&u.body, off - 0x48),
        _ => panic!("Unit image offset {off:#x} is not serialized"),
    };
    i32::from_le_bytes(v[i..i + 4].try_into().unwrap())
}

fn u_i16(u: &Unit, off: usize) -> i16 {
    let (v, i) = match off {
        0x09..=0x1b => (&u.base.sub.body, off - 0x09),
        0x20..=0x41 => (&u.base.mid, off - 0x20),
        0x48..=0xb6 => (&u.body, off - 0x48),
        _ => panic!("Unit image offset {off:#x} is not serialized"),
    };
    i16::from_le_bytes([v[i], v[i + 1]])
}

fn u_set_i16(u: &mut Unit, off: usize, x: i16) {
    u.body[off - 0x48..off - 0x46].copy_from_slice(&x.to_le_bytes());
}

fn u_set_i32(u: &mut Unit, off: usize, x: i32) {
    u.body[off - 0x48..off - 0x44].copy_from_slice(&x.to_le_bytes());
}

fn unit_complete(u: &Unit) -> bool {
    u.base.sub.body.len() == 19 && u.base.mid.len() == 34 && u.body.len() == 111
}

/// Coordinates are stored XOR 0x63637.
fn unit_x(u: &Unit) -> i32 {
    u_i32(u, 0x10) ^ 0x63637
}
fn unit_y(u: &Unit) -> i32 {
    u_i32(u, 0x14) ^ 0x63637
}

fn g_i32(row: &[u8], off: usize) -> i32 {
    i32::from_le_bytes(row[off - 8..off - 4].try_into().unwrap())
}
fn g_i16(row: &[u8], off: usize) -> i16 {
    i16::from_le_bytes([row[off - 8], row[off - 7]])
}

fn guy_row(u: &Unit, i: usize) -> Option<&Vec<u8>> {
    u.guys.elems.get(i).and_then(|g| g.as_ref()).map(|g| &g.data).filter(|d| d.len() == 155)
}

fn guy_row_mut(u: &mut Unit, i: usize) -> Option<&mut Vec<u8>> {
    u.guys.elems.get_mut(i).and_then(|g| g.as_mut()).map(|g| &mut g.data).filter(|d| d.len() == 155)
}

// ---------------------------------------------------------------------------
// Animal::do_idle 0x005d7460
// ---------------------------------------------------------------------------

/// What `do_idle` did on one animal this frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IdleReport {
    /// `Random::get` draws made (0, 1 or 4).
    pub draws: u32,
    /// Where the body stopped / what it decided.
    pub outcome: IdleOutcome,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum IdleOutcome {
    /// Not a complete serialized Animal / no guys.
    #[default]
    Skipped,
    /// `ptype->+0x218 != 0`: sea/air animal, nothing after `collide = 0`.
    NotLand,
    /// `herd < 0`: `Animal::do_idle_free` 0x005d7700 — untranscribed.
    FreeAnimal,
    /// `guys[0].cur_time != end_time - 1`: nothing this frame.
    NotAnimEnd,
    /// Draw 1 failed (`r % 10 >= 3`).
    StayPut,
    /// Near branch (`d <= 0x180`), target off-map: no order.
    NearInvalid { nx: i32, ny: i32 },
    /// Near branch, candidate computed; `detect_unit_collision` pending.
    NearCandidate { nx: i32, ny: i32 },
    /// Far branch (`d > 0x180`); `find_nearby_spot(hx,hy,…)` pending.
    FarCandidate { hx: i32, hy: i32, d: i32 },
}

/// `Animal::do_idle` for `Objects.lists[owner][slot]`. Performs every
/// retail write up to the two untranscribed query calls (see the module
/// doc) and returns what it found. `None` when the slot is not an Animal.
pub fn do_idle(save: &mut Save, owner: usize, slot: usize, effects: &mut Vec<String>) -> Option<IdleReport> {
    let tag = format!("Objects.lists[{owner}][{slot}]");
    let taken = save.objects.lists.get_mut(owner)?.elems.get_mut(slot)?.take();
    let Some(Obj::Animal(mut a)) = taken else {
        save.objects.lists[owner].elems[slot] = taken;
        return None;
    };
    let rep = do_idle_body(save, &mut a.unit, &tag, effects);
    save.objects.lists[owner].elems[slot] = Some(Obj::Animal(a));
    Some(rep)
}

fn do_idle_body(save: &mut Save, u: &mut Unit, tag: &str, effects: &mut Vec<String>) -> IdleReport {
    let mut rep = IdleReport::default();
    if !unit_complete(u) {
        return rep;
    }
    let ptype = u_i32(u, 0x18);
    let (kind, num_guys) = type_kind_and_guys(save, ptype);

    // 005d7474  Unit::set_anim_all(0,0,1)
    set_anim_all_idle(u, num_guys, tag, effects);

    // 005d747b  collide = 0
    if u_i16(u, 0x88) != 0 {
        let old = u_i16(u, 0x88);
        u_set_i16(u, 0x88, 0);
        effects.push(format!("{tag}.Unit.collide {old} -> 0"));
    }

    // 005d7485  land animals only
    if kind != 0 {
        rep.outcome = IdleOutcome::NotLand;
        return rep;
    }
    // 005d7492  herd < 0 -> Animal::do_idle_free
    let herd_idx = u_i16(u, 0x86);
    if herd_idx < 0 {
        // TODO(va 0x005d7700) Animal::do_idle_free: every 128th frame
        // (`(o * (aid+1) + frame) % 128 == 0`) paths back toward its home
        // object (`whom`/`ox` +0x152/+0x150) with one Random::get; the
        // 0x005e55c0 order write is the same as below. No free animal in
        // any capture; fields untouched.
        effects.push(format!("{tag}: Animal::do_idle_free 0x005d7700 (herd == -1) not transcribed"));
        rep.outcome = IdleOutcome::FreeAnimal;
        return rep;
    }
    // 005d74b6  guys.list[0] clock
    let Some(g0) = guy_row(u, 0) else {
        return rep;
    };
    let (cur, end) = (g_i32(g0, 0x74), g_i32(g0, 0x78));
    if cur != end.wrapping_sub(1) {
        rep.outcome = IdleOutcome::NotAnimEnd;
        return rep;
    }
    // 005d74c5  anim >= 1 || anim <= 3 — vacuous for a signed byte.
    let anim = g0[0x9c - 8] as i8;
    if !(anim >= 1 || anim <= 3) {
        unreachable!();
    }
    // 005d74de  draw 1
    let r = game_random(save, 0, 0xffff);
    rep.draws += 1;
    if srem(r, 10) >= 3 {
        rep.outcome = IdleOutcome::StayPut;
        effects.push(format!("{tag}: Animal::do_idle anim-end roll {r} % 10 >= 3 -> stay (1 draw)"));
        return rep;
    }
    let Some(h) = herd(save, herd_idx as usize) else {
        effects.push(format!("{tag}: Animal::do_idle herd {herd_idx} missing from Herds"));
        return rep;
    };
    let (hx, hy) = herd_centre(&h);
    let (x, y) = (unit_x(u), unit_y(u));
    let d = vector_dist(x.wrapping_sub(hx), y.wrapping_sub(hy));
    if d <= 0x180 {
        // 005d75f2  three more draws
        let dir = srem(game_random(save, 0, 0xffff), 8);
        let n1 = srem(game_random(save, 0, 0xffff), 4);
        let nx = (n1 + 1).wrapping_mul(MOVE_X[dir as usize]).wrapping_mul(0x30).wrapping_add(x);
        let n2 = srem(game_random(save, 0, 0xffff), 4);
        let ny = (n2 + 1).wrapping_mul(MOVE_Y[dir as usize]).wrapping_mul(0x30).wrapping_add(y);
        rep.draws += 3;
        // 005d76a3  World::valid_coord: 0 <= c < tile_xs*0xc0 (World+0x18/+0x1c)
        let tile_xs = save.world.direct.get(4).copied().unwrap_or(0);
        let tile_ys = save.world.direct.get(5).copied().unwrap_or(0);
        if !(nx >= 0 && ny >= 0 && nx < tile_xs.wrapping_mul(0xc0) && ny < tile_ys.wrapping_mul(0xc0)) {
            rep.outcome = IdleOutcome::NearInvalid { nx, ny };
            effects.push(format!("{tag}: Animal::do_idle near-herd step to ({nx},{ny}) off-map (4 draws)"));
            return rep;
        }
        // TODO(va 0x00617060) Unit::detect_unit_collision(nx,ny,1,1,0,0,0)
        // == 0 -> add_move_order(nx, ny). objects_query lane.
        rep.outcome = IdleOutcome::NearCandidate { nx, ny };
        effects.push(format!(
            "{tag}: Animal::do_idle near-herd (d={d}) candidate ({nx},{ny}) dir {dir} — \
             detect_unit_collision 0x00617060 not transcribed; order not added (4 draws)"
        ));
    } else {
        // TODO(va 0x0061de70) UnitType::find_nearby_spot(hx,hy,&sx,&sy,0xc0,
        // -1,0,0x55555555,3,o,who,0,0,-1,0,-1) == 0 -> add_move_order(sx,sy).
        // objects_query lane.
        rep.outcome = IdleOutcome::FarCandidate { hx, hy, d };
        effects.push(format!(
            "{tag}: Animal::do_idle far from herd centre ({hx},{hy}) d={d} — \
             find_nearby_spot 0x0061de70 not transcribed; order not added (1 draw)"
        ));
    }
    rep
}

/// `Unit::set_anim_all(0,0,1)` 0x00616f40 as `do_idle` calls it: rows
/// `0..guy_mark` then `num_guys..guys.length`; each row gets `hold_attack
/// = 0` unless `guys.list[0]` is in an attack-class anim, then
/// `Guy::set_anim(0,0,1)`.
fn set_anim_all_idle(u: &mut Unit, num_guys: i32, tag: &str, effects: &mut Vec<String>) {
    let guy_mark = u.body[0xb5 - 0x48] as i8 as i32;
    let len = u.guys.elems.len() as i32;
    let mut idx: Vec<usize> = (0..guy_mark.max(0)).map(|i| i as usize).collect();
    idx.extend((num_guys.max(0)..len).map(|i| i as usize));
    let g0_attack = guy_row(u, 0).map(|g| anim_class(g[0x9c - 8] as i8) == 0xc).unwrap_or(false);
    for gi in idx {
        let Some(row) = guy_row_mut(u, gi) else { continue };
        if !g0_attack && row[0x9e - 8] != 0 {
            row[0x9e - 8] = 0;
            effects.push(format!("{tag}.guys[{gi}].hold_attack -> 0"));
        }
        guy_set_anim_idle(u, gi, tag, effects);
    }
}

/// `Guy::set_anim(0, 0, 1)` 0x005da300, the `param_1 == 0 && param_2 ==
/// 0` entry (0x005da3ab..): only the early-return arms are transcribed.
///
/// ```text
/// class(cur_anim) == 0:  if cur_time < end_time: return          ; still idling
/// class(cur_anim) == 8:  if des != pos - off:  if cur_time < end_time: return
///                                              cur_time = 0; (+0xd0 runtime) return
/// cur_anim in {0x15,0x16}: if des_angle != angle: same as the class-8 arm
/// otherwise: fall into the re-roll body (anim packet + 1 Random::get)
/// ```
fn guy_set_anim_idle(u: &mut Unit, gi: usize, tag: &str, effects: &mut Vec<String>) {
    let Some(row) = guy_row_mut(u, gi) else { return };
    let cur_anim = row[0x9c - 8] as i8;
    let (cur, end) = (g_i32(row, 0x74) as u32, g_i32(row, 0x78) as u32);
    let class = anim_class(cur_anim);
    let still_moving = if class == 8 {
        let (x, y) = (g_i32(row, 0x0c), g_i32(row, 0x10));
        let (ox, oy) = (g_i16(row, 0x92) as i32, g_i16(row, 0x94) as i32);
        g_i32(row, 0x5c) != x.wrapping_sub(ox) || g_i32(row, 0x60) != y.wrapping_sub(oy)
    } else if cur_anim == 0x15 || cur_anim == 0x16 {
        g_i32(row, 0x64) != g_i32(row, 0x18)
    } else {
        false
    };
    if class == 0 {
        if cur < end {
            return;
        }
    } else if still_moving {
        if cur < end {
            return;
        }
        row[0x74 - 8..0x78 - 8].copy_from_slice(&0i32.to_le_bytes());
        effects.push(format!("{tag}.guys[{gi}].cur_time {cur} -> 0 (set_anim(0,0,1) walk/turn arm)"));
        return;
    }
    // TODO(va 0x005da3f0) Guy::set_anim re-roll body: needs the gpiece's
    // AnimationPacket (end_time) — not in the repo; draws Random::get once
    // when openlist (+0x104) is null. Fields untouched.
    effects.push(format!(
        "{tag}.guys[{gi}]: set_anim(0,0,1) re-roll (cur_anim {cur_anim} class {class}, cur {cur} end {end}) not transcribed"
    ));
}

// ---------------------------------------------------------------------------
// Unit::add_move_order 0x00616ed0 / add_move_order_ex 0x005e55c0
// ---------------------------------------------------------------------------

/// The 77-byte MoveOrder payload `add_move_order_ex` leaves for
/// `do_idle`'s argument set `(1, 0, 2, 0, -1, -1, -1, 0)`:
/// `flags` then 18 i32 `x y angle dest tolerance pause retry attempts timer
/// facing dest_x dest_y last_x last_y coll_x coll_y orig_x orig_y` then
/// i16 `off_x off_y`.
pub fn move_order_payload(x: i32, y: i32, angle: i32) -> Vec<u8> {
    let words: [i32; 18] = [x, y, angle, 0, 0, 0, 0, 0, 0, -1, x, y, -1, -1, 0, 0, -1, -1];
    let mut p = Vec::with_capacity(77);
    p.push(0u8);
    for w in words {
        p.extend_from_slice(&w.to_le_bytes());
    }
    p.extend_from_slice(&((x % 0x300) as i16).to_le_bytes());
    p.extend_from_slice(&((y % 0x300) as i16).to_le_bytes());
    p
}

/// `Unit::add_move_order(tx, ty, 1, 0, 2, 0, _, -1, -1)` on a unit whose
/// order list is empty (the only way `do_idle` is reached). Writes the
/// MoveOrder node (appended — it is the new head, serialized last), empties
/// the path stack, clears `unit_masks & 0x4000000`, and runs the
/// `update_action` tail (`orders_x/y`, `dest_angle`).
pub fn add_move_order(u: &mut Unit, tx: i32, ty: i32, tag: &str, effects: &mut Vec<String>) {
    let (ux, uy) = (unit_x(u), unit_y(u));
    let angle = find_angle(tx.wrapping_sub(ux), ty.wrapping_sub(uy));
    let x = cell(tx).wrapping_mul(0x30).wrapping_add(0x18);
    let y = cell(ty).wrapping_mul(0x30).wrapping_add(0x18);

    // add_move_order_ex: unit_masks &= ~0x4000000; path.length = 0
    let um = u_i32(u, 0x68) as u32;
    if um & 0x400_0000 != 0 {
        u_set_i32(u, 0x68, (um & !0x400_0000) as i32);
        effects.push(format!("{tag}.Unit.unit_masks &= ~0x4000000"));
    }
    if !u.path.data.is_empty() {
        u.path.data.clear();
        u.path.len = 0;
        effects.push(format!("{tag}.Unit.path.length -> 0"));
    }
    // close_orders(0): list empty -> no-op. clear_partial_path: runtime only.
    // update_action (first call): orders_x/y = pos, dest_angle = angle —
    // overwritten by the second call below.
    // get_obj(1) + the field writes:
    u.orders.orders.push(Order { ty: 1, metric: 0, payload: move_order_payload(x, y, angle) });
    u.orders.count = u.orders.orders.len() as i32;
    effects.push(format!("{tag}.Unit.orders += MoveOrder(x={x}, y={y}, angle={angle:#x}) (target ({tx},{ty}))"));
    // update_action (second call) with a single flags-0 MoveOrder:
    if u_i32(u, 0x70) != x || u_i32(u, 0x74) != y {
        u_set_i32(u, 0x70, x);
        u_set_i32(u, 0x74, y);
        effects.push(format!("{tag}.Unit.orders_x/y -> ({x},{y})"));
    }
    if u_i32(u, 0x58) != angle {
        u_set_i32(u, 0x58, angle);
        effects.push(format!("{tag}.Unit.dest_angle -> {angle:#x}"));
    }
}

// ---------------------------------------------------------------------------
// Herd::process 0x00741760 and the Objects::process_all herd tail 0x0065e04d
// ---------------------------------------------------------------------------

/// `Herd::process`: two draws; `(wx', wy') = (cx - 1 + r1 % 3, cy - 1 + r2 %
/// 3)`; accepted into `wx/wy` when on the map, `WData.flags & 0x70 == 0`
/// and `WData.blocked (+0x11) < 8`. Returns the draw count (always 2).
pub fn herd_process(save: &mut Save, idx: usize, effects: &mut Vec<String>) -> u32 {
    let Some(h) = herd(save, idx) else { return 0 };
    let r1 = game_random(save, 0, 0xffff);
    let nx = h.cx.wrapping_sub(1).wrapping_add(srem(r1, 3));
    let r2 = game_random(save, 0, 0xffff);
    let ny = h.cy.wrapping_sub(1).wrapping_add(srem(r2, 3));
    let (xs, ys) = (save.world.xs, save.world.ys);
    if nx >= 0 && ny >= 0 && nx < xs && ny < ys {
        let ti = (xs.wrapping_mul(ny).wrapping_add(nx)) as usize * 21;
        if let Some(t) = save.world.wdata.get(ti..ti + 21) {
            let flags = u16::from_le_bytes([t[0], t[1]]);
            if flags & 0x70 == 0 && t[0x11] < 8 {
                if let Some(Some(hl)) = save.herds.list.elems.get_mut(idx) {
                    hl.slot[8..12].copy_from_slice(&nx.to_le_bytes());
                    hl.slot[12..16].copy_from_slice(&ny.to_le_bytes());
                    effects.push(format!("Herds[{idx}].wx/wy ({},{}) -> ({nx},{ny})", h.wx, h.wy));
                }
            }
        }
    }
    2
}

/// The herd tail of `Objects::process_all` (0x0065e04d, inside the
/// `frame & 0x1f == 0` respawn block): on `frame & 0x3f == 0`, herd
/// `(frame / 64) % max(Herds.length, 5)` is processed when it exists and
/// its `herd_flags & 1`. Returns the draw count.
pub fn herds_tail(save: &mut Save, effects: &mut Vec<String>) -> u32 {
    let fr = frame(save);
    if fr & 0x3f != 0 {
        return 0;
    }
    let count = save.herds.list.elems.len() as i32;
    let n = count.max(5);
    let idx = (fr >> 6).wrapping_rem(n);
    if idx < 0 || idx >= count {
        return 0;
    }
    match herd(save, idx as usize) {
        Some(h) if h.flags & 1 != 0 => herd_process(save, idx as usize, effects),
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Animal::work 0x005d7330 herd seen-mask tail
// ---------------------------------------------------------------------------

/// `World::is_seen(&fx, &fy, who)` 0x006b55c0 for the fog cell
/// `(fx, fy)`: `1` when `who > 7`, `GameInfo.reveal_map == 3`, the leader
/// has `flags & 0x800` or a nonzero i16 at +0x59e4; otherwise the allied
/// `who2` arm (TODO) or `seen[fy*fog_xs + fx] & ally_mask`.
fn fog_seen(save: &Save, fx: i32, fy: i32, who: usize) -> Option<bool> {
    if who > 7 || save.game.info.settings.get(12).copied() == Some(3) {
        return Some(true);
    }
    let l = save.leaders.slots.get(who)?;
    if l.body.len() < 0x6922 {
        return None;
    }
    let f = l.flags as u32;
    let v59e4 = i16::from_le_bytes([l.body[0x59e4 - 8], l.body[0x59e5 - 8]]);
    if f & 0x800 != 0 || v59e4 != 0 {
        return Some(true);
    }
    if f & 0x2000 != 0 {
        let xs = save.world.xs;
        let ti = (xs.wrapping_mul(fy >> 1).wrapping_add(fx >> 1)) as usize * 21;
        let who2 = save.world.wdata.get(ti + 0x10).copied().map(|b| b as i8).unwrap_or(-1);
        if who2 >= 0 {
            // TODO(va 0x006edb50) Leader::is_ally(who2) -> return 1.
            return None;
        }
    }
    let fog_xs = *save.world.direct.get(1)?;
    let i = usize::try_from(fog_xs.wrapping_mul(fy).wrapping_add(fx)).ok()?;
    let seen = *save.world.seen.get(i)?;
    Some(seen & l.body[0x6929 - 8] != 0)
}

/// The tail of `Animal::work` after `do_job`: when `herd != -1` and
/// `(o + frame) % 16 == 0`, for every live leader whose fog sees the
/// animal's fog cell `(T[x >> 7], T[y >> 7])`, set bit `who` in
/// `Goods.list[Herds[herd].good_o].ever_seen` (+0x20, walked).
pub fn herd_seen_mask(save: &mut Save, owner: usize, slot: usize, effects: &mut Vec<String>) {
    let fr = frame(save);
    let Some(Some(Obj::Animal(a))) = save.objects.lists.get(owner).and_then(|l| l.elems.get(slot)) else { return };
    let u = &a.unit;
    if !unit_complete(u) {
        return;
    }
    let herd_idx = u_i16(u, 0x86);
    if herd_idx == -1 {
        return;
    }
    let o = u_i16(u, 0x0a) as i32;
    if srem(o.wrapping_add(fr), 16) != 0 {
        return;
    }
    let Some(h) = herd(save, herd_idx as usize) else { return };
    if h.good_o == -1 {
        return;
    }
    let (x, y) = (unit_x(u), unit_y(u));
    let (fx, fy) = ((x >> 7).div_euclid(3), (y >> 7).div_euclid(3));
    let mut mask = 0u8;
    let mut unknown = Vec::new();
    for who in 0..8usize {
        if leader_flags(save, who) & 1 == 0 {
            continue;
        }
        match fog_seen(save, fx, fy, who) {
            Some(true) => mask |= 1 << who,
            Some(false) => {}
            None => unknown.push(who),
        }
    }
    let tag = format!("Objects.lists[{owner}][{slot}]");
    if !unknown.is_empty() {
        effects.push(format!("{tag}: herd seen-mask: fog visibility undecidable for leaders {unknown:?} (is_ally 0x006edb50)"));
    }
    if mask != 0 {
        if let Some(Some(g)) = usize::try_from(h.good_o).ok().and_then(|i| save.goods.elems.get_mut(i)) {
            let old = g.ever_seen;
            if old | mask != old {
                g.ever_seen = old | mask;
                effects.push(format!("Goods[{}].ever_seen {old:#x} -> {:#x} (herd {herd_idx} animal {tag})", h.good_o, old | mask));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{container, load};
    use std::collections::BTreeMap;
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

    fn load_pair(dir: &Path, a: &str, b: &str) -> (Save, Save) {
        let ra = container::load_svx(&dir.join(format!("{a}.svx"))).unwrap();
        let rb = container::load_svx(&dir.join(format!("{b}.svx"))).unwrap();
        (load(&ra).unwrap().state, load(&rb).unwrap().state)
    }

    fn animals(save: &Save) -> Vec<(usize, usize)> {
        let mut v = Vec::new();
        for (owner, l) in save.objects.lists.iter().enumerate() {
            for (slot, e) in l.elems.iter().enumerate() {
                if let Some(Obj::Animal(a)) = e {
                    if a.unit.base.sub.flags & 1 != 0 && unit_complete(&a.unit) {
                        v.push((owner, slot));
                    }
                }
            }
        }
        v
    }

    fn animal(save: &Save, owner: usize, slot: usize) -> &Unit {
        match &save.objects.lists[owner].elems[slot] {
            Some(Obj::Animal(a)) => &a.unit,
            _ => panic!(),
        }
    }

    /// Serialized bytes of a Unit (image offsets as objects_process::tests).
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

    fn mo_xy(o: &Order) -> (i32, i32, i32) {
        let p = &o.payload;
        (
            i32::from_le_bytes(p[1..5].try_into().unwrap()),
            i32::from_le_bytes(p[5..9].try_into().unwrap()),
            i32::from_le_bytes(p[9..13].try_into().unwrap()),
        )
    }

    #[test]
    fn find_angle_axes() {
        assert_eq!(find_angle(0, -5), 0);
        assert_eq!(find_angle(0, 5), i32::MIN);
        assert_eq!(find_angle(5, 0), 0x4000_0000);
        assert_eq!(find_angle(-5, 0), 0xc000_0000u32 as i32);
        // 45° diagonals land on the octant boundaries' shared value.
        let a = find_angle(100, -100);
        assert!(a > 0 && a < 0x4000_0000, "{a:#x}");
    }

    #[test]
    fn vector_dist_shape() {
        assert_eq!(vector_dist(0, 0), 0);
        assert_eq!(vector_dist(100, 0), 100);
        assert_eq!(vector_dist(0, -100), 100);
        assert_eq!(vector_dist(100, 100), 150);
        assert_eq!(vector_dist(240, -192), 240 + 192 * 192 / 480);
    }

    #[test]
    fn cell_floor_div() {
        assert_eq!(cell(20376), 424);
        assert_eq!(cell(-1), -1);
        assert_eq!(cell(47), 0);
        assert_eq!(cell(48), 1);
        assert_eq!(cell(20616), 429);
    }

    /// The payload shape of a freshly added MoveOrder against the f29→f30
    /// capture: deer [8][9] got `x=0x5088 y=0x15d8 angle=0x2d1a0000`,
    /// `dest_x/y = x/y`, `last_x/y = -1`, `facing = -1`, `orig_x/y = -1`,
    /// everything else zero, `off = x % 0x300`.
    #[test]
    fn move_order_payload_shape() {
        let p = move_order_payload(0x5088, 0x15d8, 0x2d1a0000);
        assert_eq!(p.len(), 77);
        assert_eq!(p[0], 0);
        let w = |i: usize| i32::from_le_bytes(p[1 + i * 4..5 + i * 4].try_into().unwrap());
        assert_eq!((w(0), w(1), w(2)), (0x5088, 0x15d8, 0x2d1a0000));
        assert_eq!(w(9), -1, "facing");
        assert_eq!((w(10), w(11)), (0x5088, 0x15d8), "dest");
        assert_eq!((w(12), w(13)), (-1, -1), "last");
        assert_eq!((w(16), w(17)), (-1, -1), "orig");
        for i in [3, 4, 5, 6, 7, 8, 14, 15] {
            assert_eq!(w(i), 0, "word {i}");
        }
        assert_eq!(i16::from_le_bytes([p[73], p[74]]), (0x5088 % 0x300) as i16);
        assert_eq!(i16::from_le_bytes([p[75], p[76]]), (0x15d8 % 0x300) as i16);
    }

    /// Synthetic `add_move_order` on a captured deer: the order appends,
    /// `orders_x/y` and `dest_angle` follow, the path stack empties.
    #[test]
    fn add_move_order_writes() {
        let Some(dir) = capture_dir() else { return };
        let st = steps(&dir);
        let (a, _) = load_pair(&dir, &st[0].1, &st[0].1);
        let Some(&(owner, slot)) = animals(&a).iter().find(|&&(o, s)| animal(&a, o, s).orders.orders.is_empty()) else {
            return;
        };
        let mut u = animal(&a, owner, slot).clone();
        let (ux, uy) = (unit_x(&u), unit_y(&u));
        let mut eff = Vec::new();
        add_move_order(&mut u, ux + 5 * 48, uy - 4 * 48, "t", &mut eff);
        assert_eq!(u.orders.orders.len(), 1);
        let (x, y, ang) = mo_xy(&u.orders.orders[0]);
        assert_eq!(x, cell(ux + 240) * 48 + 24);
        assert_eq!(y, cell(uy - 192) * 48 + 24);
        assert_eq!(ang, find_angle(240, -192));
        assert_eq!(u_i32(&u, 0x70), x);
        assert_eq!(u_i32(&u, 0x74), y);
        assert_eq!(u_i32(&u, 0x58), ang);
        assert!(u.path.data.is_empty());
    }

    /// Capture oracle. For every stride-1 pair: run `do_idle` on every
    /// serialized animal, `herds_tail`, and `herd_seen_mask`, then:
    /// * `introduced == 0` on every animal byte, every Herd byte and every
    ///   Good `ever_seen`;
    /// * every animal whose order list grew in retail was one our `do_idle`
    ///   found at an anim-end frame with a successful roll (a Near/Far
    ///   candidate) — and no animal we flagged as a candidate stayed
    ///   orderless while another in the same herd moved (the far branch's
    ///   `find_nearby_spot` can still reject; that is reported, not
    ///   asserted);
    /// * the per-frame animal draw budget is printed next to the retail
    ///   total (`rng_draws` on the game_random seed).
    #[test]
    fn animal_plane_oracle() {
        let Some(dir) = capture_dir() else {
            eprintln!("no capture dir; skipping");
            return;
        };
        let st = steps(&dir);
        let mut introduced = 0usize;
        let mut explained = 0usize;
        let mut rows = Vec::new();
        let mut cand_hits = 0usize;
        let mut cand_misses = Vec::new();
        let mut orders_grew_unflagged = Vec::new();
        for k in 0..st.len() - 1 {
            if st[k + 1].0 - st[k].0 != 1 {
                continue;
            }
            let (a, b) = load_pair(&dir, &st[k].1, &st[k + 1].1);
            let mut ours = a.clone();
            let s0 = u32::from_le_bytes(a.post_world[0x28..0x2c].try_into().unwrap());
            let s1 = u32::from_le_bytes(b.post_world[0x28..0x2c].try_into().unwrap());
            let retail_draws = crate::tick::rng_draws(s0, s1);
            let mut effects = Vec::new();
            let mut draws = 0u32;
            let mut per_outcome: BTreeMap<String, usize> = Default::default();
            let fr = frame(&a);
            for (owner, slot) in animals(&a) {
                // Animal::work: do_job(0) -> do_idle only when the list is empty.
                if !animal(&a, owner, slot).orders.orders.is_empty() {
                    continue;
                }
                let rep = do_idle(&mut ours, owner, slot, &mut effects).unwrap();
                draws += rep.draws;
                let key = format!("{:?}", rep.outcome).split(|c| c == ' ' || c == '{').next().unwrap().to_string();
                *per_outcome.entry(key).or_default() += 1;
                herd_seen_mask(&mut ours, owner, slot, &mut effects);
                let grew = animal(&b, owner, slot).orders.orders.len() > animal(&a, owner, slot).orders.orders.len();
                let flagged = matches!(rep.outcome, IdleOutcome::NearCandidate { .. } | IdleOutcome::FarCandidate { .. });
                if grew && !flagged {
                    orders_grew_unflagged.push(format!("f{} [{owner}][{slot}] {:?}", st[k].0, rep.outcome));
                }
                if flagged {
                    if grew {
                        cand_hits += 1;
                        let o = animal(&b, owner, slot).orders.orders.last().unwrap();
                        let (x, y, ang) = mo_xy(o);
                        rows.push(format!(
                            "f{} [{owner}][{slot}] {:?} -> retail MoveOrder x={x:#x} y={y:#x} angle={ang:#x} (unit at {},{})",
                            st[k].0,
                            rep.outcome,
                            unit_x(animal(&a, owner, slot)),
                            unit_y(animal(&a, owner, slot))
                        ));
                    } else {
                        cand_misses.push(format!("f{} [{owner}][{slot}] {:?}", st[k].0, rep.outcome));
                    }
                }
            }
            draws += herds_tail(&mut ours, &mut effects);
            // Account animal / herd / goods bytes.
            for (owner, slot) in animals(&a) {
                let (ia, ib, io) = (image(animal(&a, owner, slot)), image(animal(&b, owner, slot)), image(animal(&ours, owner, slot)));
                for i in 0..ia.len().min(ib.len()).min(io.len()) {
                    if ia[i].0 != ib[i].0 || ia[i].0 != io[i].0 {
                        break;
                    }
                    let (ra, rb, ro) = (ia[i].1, ib[i].1, io[i].1);
                    if ra != rb && ro == rb {
                        explained += 1;
                    } else if ra == rb && ro != ra {
                        introduced += 1;
                        rows.push(format!("INTRODUCED f{} [{owner}][{slot}] +{:#x} retail {ra:#04x} ours {ro:#04x}", st[k].0, ia[i].0));
                    }
                }
            }
            for (i, (ha, hb)) in a.herds.list.elems.iter().zip(b.herds.list.elems.iter()).enumerate() {
                let (Some(ha), Some(hb), Some(Some(ho))) = (ha, hb, ours.herds.list.elems.get(i)) else { continue };
                for j in 0..27 {
                    if ha.slot[j] != hb.slot[j] && ho.slot[j] == hb.slot[j] {
                        explained += 1;
                    } else if ha.slot[j] == hb.slot[j] && ho.slot[j] != ha.slot[j] {
                        introduced += 1;
                        rows.push(format!("INTRODUCED f{} Herds[{i}] +{j}", st[k].0));
                    } else if ha.slot[j] != hb.slot[j] {
                        rows.push(format!("f{} Herds[{i}] +{j}: retail {:#x}->{:#x} ours {:#x}", st[k].0, ha.slot[j], hb.slot[j], ho.slot[j]));
                    }
                }
            }
            for (i, (ga, gb)) in a.goods.elems.iter().zip(b.goods.elems.iter()).enumerate() {
                let (Some(ga), Some(gb), Some(Some(go))) = (ga, gb, ours.goods.elems.get(i)) else { continue };
                if ga.ever_seen != gb.ever_seen && go.ever_seen == gb.ever_seen {
                    explained += 1;
                } else if ga.ever_seen == gb.ever_seen && go.ever_seen != ga.ever_seen {
                    introduced += 1;
                    rows.push(format!("INTRODUCED f{} Goods[{i}].ever_seen {:#x} -> {:#x}", st[k].0, ga.ever_seen, go.ever_seen));
                } else if ga.ever_seen != gb.ever_seen {
                    rows.push(format!("f{} Goods[{i}].ever_seen retail {:#x}->{:#x} ours {:#x}", st[k].0, ga.ever_seen, gb.ever_seen, go.ever_seen));
                }
            }
            eprintln!(
                "pair f{}->f{} (frame {fr}): animal draws {draws} / retail {retail_draws:?}; outcomes {per_outcome:?}",
                st[k].0,
                st[k + 1].0
            );
        }
        for r in &rows {
            eprintln!("  {r}");
        }
        eprintln!("candidates matched by a retail order: {cand_hits}; candidates retail rejected: {}", cand_misses.len());
        for m in cand_misses.iter().take(20) {
            eprintln!("  rejected {m}");
        }
        for m in &orders_grew_unflagged {
            eprintln!("  GREW-UNFLAGGED {m}");
        }
        eprintln!("animal plane: explained={explained} introduced={introduced}");
        assert_eq!(introduced, 0, "introduced bytes");
        assert!(orders_grew_unflagged.is_empty(), "retail added an order where do_idle found no roll: {orders_grew_unflagged:?}");
    }

    /// Diagnostic: per-frame table of every animal's clock, herd distance
    /// and the first capture's herds (`-- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn dump_animals() {
        let Some(dir) = capture_dir() else { return };
        let st = steps(&dir);
        let (a, _) = load_pair(&dir, &st[0].1, &st[0].1);
        println!("frame {} herds {}", frame(&a), a.herds.list.elems.len());
        for (i, h) in a.herds.list.elems.iter().enumerate() {
            if let Some(h) = h {
                let hi = herd(&a, i).unwrap();
                println!("  Herds[{i}] {:?} centre {:?} raw {:?}", hi, herd_centre(&hi), h.slot);
            }
        }
        for (owner, slot) in animals(&a) {
            let u = animal(&a, owner, slot);
            let g0 = guy_row(u, 0).unwrap();
            let herd_idx = u_i16(u, 0x86);
            let (hx, hy) = herd(&a, herd_idx.max(0) as usize).map(|h| herd_centre(&h)).unwrap_or((0, 0));
            println!(
                "  [{owner}][{slot}] ptype {} herd {herd_idx} pos ({},{}) d_centre {} cur/end {}/{} anim {} orders {} guys {}",
                u_i32(u, 0x18),
                unit_x(u),
                unit_y(u),
                vector_dist(unit_x(u) - hx, unit_y(u) - hy),
                g_i32(g0, 0x74),
                g_i32(g0, 0x78),
                g0[0x9c - 8] as i8,
                u.orders.orders.len(),
                u.guys.elems.len()
            );
        }
    }
}
