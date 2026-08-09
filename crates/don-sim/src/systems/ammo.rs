//! # Projectiles — the `ammo` checksum channel
//!
//! A port of the `Ammo` subsystem of *Rise of Nations: Extended Edition*, derived from
//! `ron-bin/riseofnations.exe` + `ron-bin/sbl/rise.pdb`. Everything below is `[measured]`
//! against those two artifacts unless a comment says `UNVERIFIED` or `UNDERIVED`.
//!
//! ## Where this sits in the tick
//!
//! Projectiles are **not** advanced by `Objects::process_all`. `Ammo::process`
//! (`0x0067D370`) is a single `ret` — one byte, an empty body. All projectile motion,
//! impact and damage happens in **`Ammo::inc_time` (`0x0067D380`)**, reached from
//! `Objects::inc_time` (`0x0065DB70`), which is **step 15** of `Game::do_frame`, i.e.
//! *after* every unit and building has already processed this frame. That ordering is
//! load-bearing: a projectile fired this frame is stepped in the same frame it was
//! spawned, and every projectile impact lands after all unit logic.
//!
//! ```text
//! Game::do_frame
//!   ...
//!   14  Objects::process_all   -> Unit::process / Build::process -> Object::fire_ammo   (SPAWN)
//!   15  Objects::inc_time      -> Ammo::inc_time                                        (FLY + HIT)
//! ```
//!
//! ## The pool
//!
//! `Objects::ammo_objs` is a `PtrArray<Ammo>` at `Objects + 0x11C`, **preallocated to 200
//! slots** in `Objects::init` (`0x0065EA80`: `malloc(800)` for the pointer table, then 200
//! × `malloc(0x7C)` = 4-byte array cookie + `sizeof(Ammo) == 120`). `increment` is set to
//! `0xFFFF` (= `-1` as `short`), i.e. "grow by the current size". The pool is **200, not
//! 400** — correcting the figure this lane was briefed with.
//!
//! Allocation (`Objects::add_ammo`, `0x00658B10`) is a **linear scan for the first slot
//! with `flags & 3 == 0`**. Slot identity therefore depends on the entire history of
//! spawns and frees, and it directly determines checksum order. Reproduce the scan
//! exactly or the `ammo` channel diverges even when the physics agrees.
//!
//! ## Checksum shape — this is the `ammo` channel
//!
//! `CheckSums::check_ammo` (`0x009374E0`) iterates the pool in **slot order** `0..length`
//! and calls the ammo's `walk_data` **only when `flags & 3 != 0`**. `AmmoData::walk_data`
//! (`0x0067AB50`) then walks, in order:
//!
//! | # | bytes | engine range | contents |
//! |--:|------:|--------------|----------|
//! | 1 | 1  | `[+0x04, +0x05)` | `flags` |
//! | 2 | 99 | `[+0x05, +0x68)` | everything from `rolling` through `start_roll_angle` — **only if `flags & 3`** |
//! | 3 | 1  | stack byte       | `ammo_path != nullptr` |
//! | 4 | —  |                  | `Spline::walk_data` if that byte is set |
//!
//! The hash is zlib adler-32 (`0x00A46830`), seeded to 1 at the start of the channel and
//! carried across every slot ([`AmmoPool::checksum`]).
//!
//! **The ballistics floats are inside the checksum.** `v1z`, `dx`, `bank_dx`, `bank_dy`
//! live at `+0x54..+0x64`, well inside the 99-byte block, so they are hashed bit-for-bit
//! and a 1-ULP difference is a desync. This is the honest answer to "are the AmmoData
//! ballistics sim-critical" — **yes, bit-exactly**, and it makes the ammo channel the one
//! place in the walked state where f32 arithmetic must be reproduced exactly rather than
//! merely closely. `v1z` is produced by one divide and two multiplies from integers, and
//! `dx` by `sqrtf` (IEEE-exact) over an integer square sum, so both are reproducible on
//! any IEEE binary32 host; the danger is x87 double rounding, not the algorithm.
//!
//! ## What is NOT here
//!
//! * `TRAJ_SPLINE` (aircraft crashes, nukes, cruise missiles) — `Spline::calc_from_dir`
//!   /`calc_nuke_spline` are unread. Spline ammo is modelled as an opaque path.
//! * `find_angle` (`0x0092D130`) — the binary-angle helper is not derived; the impact
//!   angle is passed through from the caller so the flanking term in the damage pipeline
//!   still gets a value.
//! * The full body of `Object::do_damage` (`0x0064A480`, 9,214 bytes). This module derives
//!   and implements only the **projectile-specific** head of it — the argument packing and
//!   the `ammo_per_att` / `uber_size` / 1-16th split, which is the part the ammo channel
//!   owns. Everything after that is the `units`/`deaths` lanes.

#![allow(clippy::too_many_arguments)]

// ============================================================================
// Constants
// ============================================================================

/// Slots preallocated by `Objects::init` `0x0065EA80` [measured].
pub const AMMO_POOL_SLOTS: usize = 200;

/// World units per tile. `check_hit` bounds-checks `ex < world_w * 0xC0` [measured,
/// `0x0067?` in `Ammo::check_hit`], and `Objects::init` places objects on the same grid.
pub const TILE: i32 = 0xC0; // 192

/// `ObjectData` position fields are stored XOR-obfuscated with this mask; every read in
/// `Ammo::*` is `*(u32*)(obj+0x10) ^ 0x63637` [measured].
pub const OBJ_FIELD_XOR: u32 = 0x0006_3637;

/// Gravity, world units per frame². `DAT_00CAB378`, written unconditionally as the raw
/// bit pattern `0xC127CCCD` by `GraphicPieces::init` (`0x008FFCC0`) [measured].
///
/// Note the provenance oddity, and do not "fix" it: a **presentation** class initialises
/// the constant the **simulation** ballistic solver depends on. A headless port must still
/// run this assignment.
pub const GRAVITY: f32 = f32::from_bits(0xC127_CCCD); // -10.4875

/// `Constants + 0x04` = `unit_move_speed`, `1/192 tile` granularity, stored value **1**
/// [measured, `docs/derivation/rules-constants.json`]. It multiplies `proj_speed` in the
/// flight-time solve, so with the shipped rules `total_time = dist / proj_speed`.
pub const UNIT_MOVE_SPEED: i32 = 1;

/// `Constants + 0x38` = `target_radius`, `1/2 tile`, stored value **96** [measured].
/// Numerator of the miss-scatter radius.
pub const TARGET_RADIUS: i32 = 96;

/// A "spent" projectile that hit bare ground lingers this many frames before its slot is
/// released — `Ammo::inc_time`: `if (!(flags & FLYING)) { if (cur_time < 200) return; close(); }`
/// [measured]. It stays `flags & 3 != 0` the whole time, so **it keeps contributing to the
/// ammo checksum for 200 frames after it stopped moving.**
pub const SPENT_LINGER_FRAMES: u32 = 200;

/// Vertical muzzle offset added to the shooter's `z`, in `Object::fire_ammo` [measured]:
/// 100 for units, 250 for buildings.
pub const MUZZLE_Z_UNIT: i32 = 100;
/// See [`MUZZLE_Z_UNIT`].
pub const MUZZLE_Z_BUILD: i32 = 250;

/// Extra impact height granted to a projectile that is allowed to overshoot
/// ([`FLAG_OVERSHOOT`]); `Ammo::init` sets `ez = target.z + 0x4B` in that case [measured].
pub const OVERSHOOT_Z_BONUS: i32 = 0x4B; // 75

/// Impact-point jitter applied when a projectile arrives with no target left:
/// `ex += Random::in_range(0,0xFFFF) % 0x29 - 0x14` [measured, `Ammo::do_damage`].
pub const GROUND_JITTER_MOD: i32 = 0x29; // 41
/// See [`GROUND_JITTER_MOD`].
pub const GROUND_JITTER_BIAS: i32 = 0x14; // 20

/// `Ammo::do_damage` searches this many frames of overshoot before giving up:
/// `if (total_time * 3 < cur_time) close()` [measured, `Ammo::inc_time`].
pub const OVERSHOOT_TIME_LIMIT_MULT: u32 = 3;

// ---- `AmmoData::flags` bits, recovered from use ----------------------------------

/// Slot is occupied. `Objects::add_ammo` treats `flags & 3 == 0` as free;
/// `CheckSums::check_ammo` walks a slot iff `flags & 3 != 0`; `Ammo::close` writes
/// `flags = 0` [measured].
pub const FLAG_ALIVE: u8 = 0x01;
/// In flight. `Ammo::init` ends with `flags |= 2`. `Ammo::inc_time` only integrates while
/// this is set; without it the projectile is a spent ground marker [measured].
pub const FLAG_FLYING: u8 = 0x02;
/// This shot is allowed to fly past its target and land. Set by `Ammo::init` when the
/// target is a **ground-domain unit** and the shot is not a spline [measured].
pub const FLAG_OVERSHOOT: u8 = 0x04;
/// The overshoot resolved to "nothing was there" — set on arrival when both `hit_target`
/// and `check_hit` fail [measured].
pub const FLAG_MISSED: u8 = 0x08;
/// Cosmetic round: `Ammo::inc_time` calls `close()` instead of `do_damage()`. Set either
/// from the ammo graphic's flag bit `0x80`, or by a failed anti-air roll [measured].
pub const FLAG_NO_DAMAGE: u8 = 0x10;

/// `enum TrajectoryType` [measured, PDB TPI].
pub const TRAJ_STRAIGHT: i32 = 0;
/// See [`TRAJ_STRAIGHT`]. `Ammo::init` only ever writes 1 or 2.
pub const TRAJ_ARC: i32 = 1;
/// See [`TRAJ_STRAIGHT`].
pub const TRAJ_SPLINE: i32 = 2;

/// `ObjectType::domain` (`+0x218`) values used by the ammo code [measured from use:
/// `check_hit` treats 2 as the flying case, `Ammo::init` gates the overshoot flag on 0].
pub const DOMAIN_LAND: i32 = 0;
/// See [`DOMAIN_LAND`].
pub const DOMAIN_AIR: i32 = 2;

// ============================================================================
// `vector_dist` — `0x0046CFF0`, pure integer [measured]
// ============================================================================

/// `int __fastcall vector_dist(int dx, int dy)` — the engine's distance primitive.
///
/// Despite the PDB rendering it `__cdecl`, the retail code passes `dx` in `ecx` and `dy`
/// in `edx` and pushes nothing. It contains **no floating point**: it is the first-order
/// expansion `hi + lo²/(2·hi)` of `sqrt(hi² + lo²)`, with an overflow fallback.
///
/// ```asm
/// 0x46cff5  cdq / xor / sub        ; a = |dx|
/// 0x46cfff  cdq / xor / sub        ; b = |dy|
/// 0x46d006  cmp edi, esi           ; hi = max(a,b), lo = min(a,b)
/// 0x46d013  cmp esi, 0xea60        ; lo >= 60000 ?
/// 0x46d01b  lea eax,[esi+edi*2] / shr eax,1   ;  -> (2*hi + lo) / 2
/// 0x46d023  imul esi,esi / div ecx / add eax,edi ; -> hi + lo*lo / (2*hi)   [unsigned div]
/// ```
///
/// The `imul`/`div` pair is signed-multiply-low then **unsigned** divide, which is exactly
/// `u32` arithmetic — `lo < 60000` keeps `lo*lo < 2^32`, so no wrap occurs, but the
/// division must be unsigned to match.
#[inline]
pub fn vector_dist(dx: i32, dy: i32) -> i32 {
    let a = dx.wrapping_abs();
    let b = dy.wrapping_abs();
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    if hi == 0 {
        return 0;
    }
    if lo >= 0xEA60 {
        // (hi*2 + lo) >> 1, logical shift on the 32-bit sum.
        return (((lo as u32).wrapping_add((hi as u32).wrapping_mul(2))) >> 1) as i32;
    }
    let num = (lo as u32).wrapping_mul(lo as u32);
    let den = (hi as u32).wrapping_mul(2);
    hi.wrapping_add((num / den) as i32)
}

// ============================================================================
// `Random::get(int, int)` — `0x00A39D70` [measured, Tier B: 1.5M oracle trials]
// ============================================================================

/// The simulation RNG stream (`GameAccess::game_random`, `0x00E37A8C`). A `Random` is a
/// bare `u32`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rng(pub u32);

impl Rng {
    /// `int Random::get(int lo, int hi)` — **half-open `[lo, hi)`**.
    ///
    /// * `lo == hi` returns `lo` **without advancing the state**. Getting this wrong
    ///   desynchronises the whole stream, not one draw.
    /// * `lo > hi` is silently swapped and the state *is* advanced.
    /// * Only the low 16 bits of the *new* state feed the mapping.
    #[inline]
    pub fn in_range(&mut self, lo: i32, hi: i32) -> i32 {
        if lo == hi {
            return lo;
        }
        let (lo, hi) = if lo > hi { (hi, lo) } else { (lo, hi) };
        self.0 = self.0.wrapping_mul(0x0019_660D).wrapping_add(0x3C6E_F35F);
        let range = (hi as u32).wrapping_sub(lo as u32);
        let r = ((self.0 & 0xFFFF).wrapping_mul(range) >> 16) as i32;
        r.wrapping_add(lo)
    }

    /// The exact draw the ammo code uses everywhere it scatters: `Random::get(0, 0xFFFF)`.
    #[inline]
    pub fn draw16(&mut self) -> i32 {
        self.in_range(0, 0xFFFF)
    }
}

// ============================================================================
// adler-32 — `0x00A46830` [measured, Tier B: 500k oracle trials]
// ============================================================================

/// zlib `adler32`, the lockstep checksum primitive — re-exported from
/// [`crate::checksum`], which is the crate's only implementation of it.
pub use crate::checksum::{adler32, ADLER_BASE, ADLER_NMAX};

// ============================================================================
// `AmmoData` — the walked state, laid out to match the engine byte-for-byte
// ============================================================================

/// The checksummed body of an `Ammo`, i.e. engine `AmmoData + 0x04 .. + 0x68`.
///
/// Field order, sizes and padding are the PDB's `AmmoData` record verbatim (`sizeof` 108,
/// vfptr at `+0`, `GameAccessConst` empty base at `+4`), shifted down by 4 so that
/// `offset_of(field) == engine_offset - 4`. `#[repr(C)]` on 32-bit-compatible scalar types
/// reproduces the MSVC layout exactly; [`assert_layout`] proves it at test time.
///
/// `ammo_path: Spline*` (engine `+0x68`) is deliberately **not** a field here: it is a
/// pointer, it is not hashed as a pointer, and the walk instead hashes a single
/// `ammo_path != null` byte. It lives in [`Ammo::has_spline`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AmmoWalk {
    /// `+0x04` — see the `FLAG_*` constants.
    pub flags: u8,
    /// `+0x05` — set to 0 by `Ammo::init`; the rolling-shot phase for multi-barrel FX.
    pub rolling: i8,
    /// `+0x06` — `to_hit` after range attenuation, floored at 5. See [`accuracy`].
    pub accuracy: i16,
    /// `+0x08` — index into `GraphicPieces`' ammo table. Selects the ammo *kind*.
    pub gpiece: i32,
    /// `+0x0C` — muzzle position (world units).
    pub sx: i32,
    /// `+0x10`
    pub sy: i32,
    /// `+0x14`
    pub sz: i32,
    /// `+0x18` — aim point / impact point (world units). Mutated in flight.
    pub ex: i32,
    /// `+0x1C`
    pub ey: i32,
    /// `+0x20`
    pub ez: i32,
    /// `+0x24` — frames since launch. Incremented at the top of `Ammo::inc_time`.
    pub cur_time: u32,
    /// `+0x28` — total flight time in frames. Impact when `cur_time >= total_time`.
    pub total_time: u32,
    /// `+0x2C` — binary angle from muzzle to aim point (`find_angle`, UNDERIVED).
    pub angle: i32,
    /// `+0x30` — copied from the **shooter's** `ObjectType::splash_area` (`+0x200`).
    pub splash_area: i32,
    /// `+0x34` — the **pool slot**. Passed to `Object::do_damage` as the "this came from a
    /// projectile" discriminator; a negative value there means a melee hit.
    pub index: i32,
    /// `+0x38` — `Objects::ammo_index`, a monotonically increasing spawn counter.
    pub graph_index: i32,
    /// `+0x3C` — shooter owner slot.
    pub who: i32,
    /// `+0x40` — shooter object index.
    pub o: i32,
    /// `+0x44` — the shooter unit's live guy count (`UnitData::guy_mark`, `+0xB5`); 0 for
    /// buildings.
    pub num_guys: i32,
    /// `+0x48` — target owner slot, or `-1`.
    pub whom: i32,
    /// `+0x4C` — target object index, or `-1`. (The PDB calls it `ox`; every use is as an
    /// object index paired with `whom`.)
    pub ox: i32,
    /// `+0x50` — `TrajectoryType`.
    pub traj: i32,
    /// `+0x54` — initial vertical velocity, world units/frame. **Checksummed.**
    pub v1z: f32,
    /// `+0x58` — horizontal speed, world units/frame. **Checksummed.**
    pub dx: f32,
    /// `+0x5C` — per-frame drift applied to `ex` when the aim point tracks a mover.
    /// **Checksummed.**
    pub bank_dx: f32,
    /// `+0x60` — as [`AmmoWalk::bank_dx`], for `ey`. **Checksummed.**
    pub bank_dy: f32,
    /// `+0x64`
    pub start_roll_angle: i32,
}

/// A pool slot: the walked body plus the one bit of the `Spline*` that is hashed.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ammo {
    /// Engine `AmmoData + 0x04 .. + 0x68`, hashed verbatim.
    pub w: AmmoWalk,
    /// Stands in for `ammo_path != nullptr` (engine `+0x68`). The walk hashes exactly one
    /// byte for this. Spline geometry itself is not modelled here.
    pub has_spline: bool,
}

impl Ammo {
    /// `flags & 3 != 0` — the predicate `Objects::add_ammo` uses to reject a slot and
    /// `CheckSums::check_ammo` uses to include one.
    #[inline]
    pub fn occupied(&self) -> bool {
        self.w.flags & 3 != 0
    }
    /// `Ammo::close` (`0x006791A0`): a single byte write `flags = 0`, then the spline is
    /// returned to `Recycler<Spline>`. Note it is an **assignment**, not a mask-clear.
    #[inline]
    pub fn close(&mut self) {
        self.w.flags = 0;
        self.has_spline = false;
    }
}

impl AmmoWalk {
    /// The 100 bytes `[+0x04, +0x68)` as the engine has them in memory.
    #[inline]
    pub fn as_bytes(&self) -> &[u8; 100] {
        // SAFETY: `#[repr(C)]` over scalar fields, size asserted == 100 by `assert_layout`.
        // Padding bytes exist only at +0x02..+0x04 in engine terms, which is the `accuracy`
        // field itself, so every byte is initialised.
        unsafe { &*(self as *const AmmoWalk as *const [u8; 100]) }
    }
}

/// Compile-and-run-time proof that [`AmmoWalk`] reproduces the PDB's `AmmoData` layout.
/// Every offset below is `engine_offset - 4` from `schema/pdb-types.json`.
pub fn assert_layout() {
    use core::mem::size_of;
    assert_eq!(
        size_of::<AmmoWalk>(),
        100,
        "AmmoData +0x04..+0x68 is 100 bytes"
    );
    let z = AmmoWalk::default();
    let base = &z as *const AmmoWalk as usize;
    macro_rules! off {
        ($f:ident, $e:expr) => {
            assert_eq!(
                (&z.$f as *const _ as usize) - base,
                $e - 4,
                concat!("AmmoData::", stringify!($f))
            );
        };
    }
    off!(flags, 0x04);
    off!(rolling, 0x05);
    off!(accuracy, 0x06);
    off!(gpiece, 0x08);
    off!(sx, 0x0C);
    off!(sy, 0x10);
    off!(sz, 0x14);
    off!(ex, 0x18);
    off!(ey, 0x1C);
    off!(ez, 0x20);
    off!(cur_time, 0x24);
    off!(total_time, 0x28);
    off!(angle, 0x2C);
    off!(splash_area, 0x30);
    off!(index, 0x34);
    off!(graph_index, 0x38);
    off!(who, 0x3C);
    off!(o, 0x40);
    off!(num_guys, 0x44);
    off!(whom, 0x48);
    off!(ox, 0x4C);
    off!(traj, 0x50);
    off!(v1z, 0x54);
    off!(dx, 0x58);
    off!(bank_dx, 0x5C);
    off!(bank_dy, 0x60);
    off!(start_roll_angle, 0x64);
}

// ============================================================================
// The pool
// ============================================================================

/// `Objects::ammo_objs` (`Objects + 0x11C`) plus `Objects::ammo_index` (`+0x1F8`).
#[derive(Clone, Debug)]
pub struct AmmoPool {
    /// Slot array. `len()` is the engine's `PtrArray::length`; slots are never removed,
    /// only marked free.
    pub slots: Vec<Ammo>,
    /// `Objects::ammo_index` — monotone spawn counter, copied into `graph_index` and then
    /// incremented. Reset to 0 by `Objects::init`.
    pub ammo_index: i32,
}

impl Default for AmmoPool {
    fn default() -> Self {
        Self::new()
    }
}

impl AmmoPool {
    /// `Objects::init` `0x0065EA80`: 200 constructed-but-free slots, `ammo_index = 0`.
    pub fn new() -> Self {
        AmmoPool {
            slots: vec![Ammo::default(); AMMO_POOL_SLOTS],
            ammo_index: 0,
        }
    }

    /// `Objects::add_ammo` (`0x00658B10`) slot selection: scan from 0 for the first slot
    /// with `flags & 3 == 0`; if the scan reaches `length`, append one slot and extend
    /// `length`. Returns the slot index.
    ///
    /// The engine also runs the `ArrayBase` growth dance (`increase_size`, `make_valid`)
    /// when `size < length + 1`; that only affects capacity, never index assignment, so it
    /// is a `Vec::push` here.
    pub fn alloc_slot(&mut self) -> usize {
        let n = self.slots.len();
        let mut i = 0;
        while i < n {
            if !self.slots[i].occupied() {
                break;
            }
            i += 1;
        }
        if i == n {
            self.slots.push(Ammo::default());
        }
        i
    }

    /// The `ammo` checksum channel.
    ///
    /// `CheckSums::check_ammo` `0x009374E0` + `AmmoData::walk_data` `0x0067AB50`, with the
    /// adler seeded to 1 as `check_all` does before every channel.
    ///
    /// Note what is *not* hashed: free slots contribute **nothing at all** (the guard is in
    /// `check_ammo`, before the virtual call), and the pool length itself is not hashed.
    /// Two runs whose pools differ only in trailing free slots agree on this channel.
    pub fn checksum(&self) -> u32 {
        let mut a: u32 = 1;
        for s in &self.slots {
            if !s.occupied() {
                continue;
            }
            let b = s.w.as_bytes();
            a = adler32(a, &b[0..1]); // walk(+0x04, +0x05)  -- flags
            a = adler32(a, &b[1..100]); // walk(+0x05, +0x68)  -- gated on flags & 3
            a = adler32(a, &[s.has_spline as u8]); // walk(stack bool)
                                                   // Spline::walk_data would follow here.
        }
        a
    }

    /// Live projectile count — diagnostics only, not part of any channel.
    pub fn live(&self) -> usize {
        self.slots.iter().filter(|s| s.occupied()).count()
    }
}

// ============================================================================
// Rules and world views
// ============================================================================

/// The `ObjectType` / `UnitType` fields the ammo path reads, with their offsets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShooterRules {
    /// `ObjectType + 0x1EC` — base accuracy percent.
    pub to_hit: i32,
    /// `ObjectType + 0x1F0` — accuracy lost **per tile** of range.
    pub attenuate: i32,
    /// `ObjectType + 0x200` — splash radius in tiles; 0 means single-target.
    pub splash_area: i32,
    /// `ObjectType + 0x204` — splash damage percent (consumed downstream, in
    /// `Object::do_damage`, not here).
    pub splash_percent: i32,
    /// `ObjectType + 0x208` — projectiles per attack, and the damage divisor.
    pub ammo_per_att: i32,
    /// `ObjectType + 0x20C` — world units per frame.
    pub proj_speed: i32,
    /// `ObjectType + 0x218` — `DomainIndex`.
    pub domain: i32,
    /// `ObjectType + 0x234` — footprint half-extent unit, tiles.
    pub x_size: i32,
    /// `ObjectType + 0x238`
    pub y_size: i32,
    /// `UnitType + 0x300` — hit radius for a unit target.
    pub target_size: i32,
    /// `UnitType + 0x308` — the second damage divisor for a unit shooter.
    pub uber_size: i32,
}

/// One object as the ammo code sees it. Positions are already de-obfuscated
/// (`stored ^ 0x63637`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ObjView {
    /// `ObjectData::flags & 1` — the liveness bit every ammo path re-checks.
    pub alive: bool,
    /// `Object::vt[+0x18]()` — true for `Unit`, false for `Build`/`Wall`.
    pub is_unit: bool,
    /// `ObjectData::x_internal ^ 0x63637`.
    pub x: i32,
    /// `ObjectData::y_internal ^ 0x63637`.
    pub y: i32,
    /// `ObjectData::z_internal ^ 0x63637`.
    pub z: i32,
    /// `UnitData::guy_mark` (`+0xB5`), the live squad size. 0 for non-units.
    pub guy_mark: i32,
    /// `z` of `guys[0]` — used verbatim as the impact height for an air target.
    pub guy0_z: i32,
    /// The type record.
    pub rules: ShooterRules,
}

/// One guy's position, from `UnitData::guys` (`PtrArray<Guy>` at `+0xE4`, list at `+0xF4`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GuyPos {
    /// `Guy + 0x0C`
    pub x: i32,
    /// `Guy + 0x10`
    pub y: i32,
    /// `Guy + 0x14`
    pub z: i32,
}

/// The world queries the ammo path makes. Implemented by the caller so this module stays
/// independent of the SoA world (which another lane owns).
pub trait AmmoEnv {
    /// `Objects[who][o]`.
    fn object(&self, who: i32, o: i32) -> Option<ObjView>;
    /// `TerrainOut::find_data_z(x, y, 0)` (`0x00866560`) — ground height at a world point.
    fn terrain_z(&self, x: i32, y: i32) -> i32;
    /// World size in tiles (`World + 0x18`, `+0x1C`).
    fn world_tiles(&self) -> (i32, i32);
    /// `WorldData::is_valid` (`0x0043F360`).
    fn is_valid(&self, x: i32, y: i32) -> bool {
        let (w, h) = self.world_tiles();
        x >= 0 && y >= 0 && x < w * TILE && y < h * TILE
    }
    /// `WorldData::restrict` (`0x006B53A0`) — clamp a point into the map.
    fn restrict(&self, x: &mut i32, y: &mut i32) {
        let (w, h) = self.world_tiles();
        *x = (*x).clamp(0, w * TILE - 1);
        *y = (*y).clamp(0, h * TILE - 1);
    }
    /// `ObjectsData::find_unit(x, y, .., radius 0x180, ..)` (`0x0065CA80`) as `check_hit`
    /// calls it: nearest unit within 2 tiles of the impact point. Returns `(who, o, dist)`.
    fn find_unit_near(&self, x: i32, y: i32, shooter_who: i32) -> Option<(i32, i32, i32)>;
    /// `ObjectsData::find_building_at(tx, ty, ..)` (`0x0065AB40`). Returns `(who, o)`.
    fn find_building_at(&self, tx: i32, ty: i32) -> Option<(i32, i32)>;
    /// `(world.tiles[t].flags & 0x30) == 0x20` — the water test `do_damage` uses to decide
    /// whether a ground miss leaves a decal or just vanishes.
    fn is_water_tile(&self, tx: i32, ty: i32) -> bool;
}

// ============================================================================
// Spawn — `Object::fire_ammo` `0x0064C8B0`
// ============================================================================

/// One projectile's launch point, as `Object::fire_ammo` fills the `GameDataPackage`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpawnPoint {
    /// `pkg + 0x04`
    pub x: i32,
    /// `pkg + 0x08`
    pub y: i32,
    /// `pkg + 0x0C`
    pub z: i32,
}

/// `Object::fire_ammo(int target_o, int target_who)` (`0x0064C8B0`) — how many projectiles
/// a single attack emits, and where each one starts.
///
/// Two completely different shapes [measured]:
///
/// * **Unit shooter** — one projectile **per live guy**, launched from that guy's exact
///   position plus [`MUZZLE_Z_UNIT`]. Draws **no RNG**.
/// * **Building shooter** — `ammo_per_att` projectiles, each launched from a uniformly
///   random point over the building footprint, at `obj.z + `[`MUZZLE_Z_BUILD`]. Draws
///   **exactly two `Random::get(0,0xFFFF)` per projectile**, x then y, each dimension
///   skipped (and the draw *not* taken) when the extent is `<= 1`.
///
/// The building scatter box is `x_size * 0x60` by `y_size * 0x60` world units, centred on
/// the object:
///
/// ```asm
/// 0x64ca4e  ...  w = objtype[+0x234] * 0x60
/// 0x64ca7?  if (w - 1 < 1) r = 0 else { r = Random::get(0,0xFFFF); r %= w; }
///           x = (obj.x ^ 0x63637) - w/2 + r
/// ```
///
/// The gate on firing at all is
/// `(target_o >= 0 && target_who >= 0) || (is_unit && order in {ATTACK_GROUND(23),
/// AIR_ATTACK_GROUND(24)})`.
pub fn fire_ammo_spawns(
    shooter: &ObjView,
    guys: &[GuyPos],
    rng: &mut Rng,
    out: &mut Vec<SpawnPoint>,
) {
    if shooter.is_unit {
        // One per guy, in guy-list order. `for (i = 0; i < unit->guy_mark; i++)`.
        let n = shooter.guy_mark.max(0) as usize;
        for g in guys.iter().take(n) {
            out.push(SpawnPoint {
                x: g.x,
                y: g.y,
                z: g.z + MUZZLE_Z_UNIT,
            });
        }
    } else {
        let w = shooter.rules.x_size * 0x60;
        let h = shooter.rules.y_size * 0x60;
        for _ in 0..shooter.rules.ammo_per_att.max(0) {
            let rx = if w - 1 < 1 { 0 } else { rng.draw16() % w };
            let ry = if h - 1 < 1 { 0 } else { rng.draw16() % h };
            out.push(SpawnPoint {
                x: shooter.x - w / 2 + rx,
                y: shooter.y - h / 2 + ry,
                z: shooter.z + MUZZLE_Z_BUILD,
            });
        }
    }
}

// ============================================================================
// Launch — the derived half of `Ammo::init` `0x0067BBF0`
// ============================================================================

/// `accuracy` (`AmmoData + 0x06`), from `Ammo::init` [measured].
///
/// ```asm
/// 0x67c4??  imul  <dist>, -0x2aaaaaab ; sar edx,5 ; sub sign   ->  dist / -192
///           imul  <that>, objtype[+0x1f0]                      ->  * attenuate
///           add   objtype[+0x1ec]                              ->  + to_hit
///           cmp   5 ; cmovl 5                                  ->  floor of 5
/// ```
///
/// The magic multiply is MSVC's signed divide by **-192**, i.e. exactly one tile, rounding
/// toward zero. So: **`to_hit` minus one `attenuate` per full tile of range, never below
/// 5.** `dist` is `ObjectData::attack_dist` (`0x0064C880`) for a targeted shot, plain
/// [`vector_dist`] for attack-ground.
#[inline]
pub fn accuracy(to_hit: i32, attenuate: i32, dist: i32) -> i16 {
    let tiles = dist / -TILE; // truncates toward zero, negative
    let acc = to_hit.wrapping_add(tiles.wrapping_mul(attenuate));
    (if acc < 5 { 5 } else { acc }) as i16
}

/// The scatter radius `R` applied to the aim point, from `Ammo::init` [measured].
///
/// ```asm
/// 0x67c5c5  R = (constants[+0x38] * 100) / ((100 - acc) / 5 + acc)
///           if (acc > 100)  R = R / 4          ; sar with the standard sign fixup
/// ```
///
/// `constants + 0x38` is [`TARGET_RADIUS`] = 96. `(100 - acc) / 5` truncates toward zero,
/// including for `acc > 100` where it is negative. Special cases from the surrounding
/// branches, in priority order:
///
/// * shooter's `UnitType + 0x2B4 & 0x400000` set → `R = 0` (perfect aim);
/// * target is not a unit, or its `domain != 0` (i.e. a building or a ship/plane)
///   → `R = 192`, one flat tile;
/// * attack-ground with a non-zero order field → `R = 0`;
/// * shooter's `obj_masks & 0x8000000` (the aircraft class) → `R *= 2`, or `R = 0` if a
///   further ability check passes.
///
/// Only the main formula is implemented here; the branch selection is [`MissRadius`].
#[inline]
pub fn miss_radius_formula(acc: i32) -> i32 {
    let den = (100 - acc) / 5 + acc;
    if den == 0 {
        return 0; // engine would divide by zero; unreachable for acc >= 5 (den >= 24).
    }
    let mut r = (TARGET_RADIUS * 100) / den;
    if acc > 100 {
        // `sar eax,2` with the `and edx,3 ; add` sign fixup == truncating divide by 4.
        r /= 4;
    }
    r
}

/// Which arm of `Ammo::init`'s aim-scatter selection applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissRadius {
    /// `UnitType + 0x2B4 & 0x400000`, or a spline shot, or attack-ground with the order
    /// flag set.
    Perfect,
    /// A non-unit target, or a unit whose `domain != 0`.
    FlatTile,
    /// The accuracy-driven formula.
    Formula,
}

/// Resolve [`MissRadius`] to a radius in world units.
#[inline]
pub fn miss_radius(kind: MissRadius, acc: i32) -> i32 {
    match kind {
        MissRadius::Perfect => 0,
        MissRadius::FlatTile => TILE,
        MissRadius::Formula => miss_radius_formula(acc),
    }
}

/// Apply the aim scatter. `Ammo::init`:
///
/// ```asm
/// 0x67c3??  if (R - 1 < 1) rx = 0 else { rx = Random::get(0,0xFFFF); rx %= R; }
///           ex += rx - R/2
///           (same again for ey)
/// ```
///
/// Two draws, x then y, **both skipped when `R <= 1`** — the draw is not taken, so the
/// stream position depends on the radius. `R / 2` truncates toward zero.
#[inline]
pub fn apply_aim_scatter(ex: &mut i32, ey: &mut i32, r: i32, rng: &mut Rng) {
    let half = r / 2;
    let rx = if r - 1 < 1 { 0 } else { rng.draw16() % r };
    *ex += rx - half;
    let ry = if r - 1 < 1 { 0 } else { rng.draw16() % r };
    *ey += ry - half;
}

/// Flight time in frames, for the ordinary ballistic (non-spline, non-bomb) case.
///
/// ```asm
/// 0x67cd??  d = sqrtf((ex-sx)^2 + (ey-sy)^2)               ; float
///           total_time = (int)(d / (float)(proj_speed * constants[+4]))
///           if (total_time == 0) total_time = 1
/// ```
///
/// `constants + 4` is [`UNIT_MOVE_SPEED`] = 1 with the shipped rules, so this is
/// `dist / proj_speed`. The intermediate really is f32 — the truncation happens on the
/// float quotient, not on an integer division, and the two differ whenever the quotient
/// lands just under an integer.
///
/// Sibling forms, not implemented here: a building shooter with `BuildData::get_shot() == 0`
/// divides by `constants[+4] * 0x5A` instead; a bomb (`vt[+0x130]`) uses
/// `(that * 0xC0) / (proj_speed * constants[+4])`; a free-fall drop uses
/// `sqrtf((ez - sz) * 2 / GRAVITY)`.
#[inline]
pub fn arc_total_time(sx: i32, sy: i32, ex: i32, ey: i32, proj_speed: i32) -> u32 {
    let ddx = (ex - sx) as f32;
    let ddy = (ey - sy) as f32;
    let d = (ddx * ddx + ddy * ddy).sqrt();
    let denom = (proj_speed.wrapping_mul(UNIT_MOVE_SPEED)) as f32;
    let t = (d / denom) as i32;
    if t == 0 {
        1
    } else {
        t as u32
    }
}

/// The ballistic solve, exactly as `Ammo::init` writes it [measured, `0x0067CF1A`ff]:
///
/// ```text
/// T   = (float)total_time
/// v1z = ((ez - sz) - G*0.5*T*T) / T
/// dx  = sqrtf((ex-sx)^2 + (ey-sy)^2) / T
/// bank_dx = bank_dy = 0
/// rolling = 0
/// traj = spline ? TRAJ_SPLINE : TRAJ_ARC
/// ```
///
/// i.e. `v1z` is chosen so that `z(T) == ez` under `z(t) = sz + v1z·t + ½·G·t²`. **These
/// four f32s are hashed by the ammo channel**, so the multiply/divide order above is not
/// cosmetic — keep it.
pub fn arc_ballistics(a: &mut AmmoWalk) {
    let t = a.total_time as f32;
    a.v1z = (((a.ez - a.sz) as f32) - GRAVITY * 0.5 * t * t) / t;
    let ddx = (a.ex - a.sx) as f32;
    let ddy = (a.ey - a.sy) as f32;
    a.dx = (ddy * ddy + ddx * ddx).sqrt() / t;
    a.bank_dx = 0.0;
    a.bank_dy = 0.0;
    a.rolling = 0;
}

/// Height of the projectile at `t` frames after launch:
/// `sz + v1z·t + G·0.5·t²`, evaluated in the engine's operand order.
#[inline]
pub fn z_at(a: &AmmoWalk, t: f32) -> f32 {
    a.v1z * t + (a.sz as f32) + GRAVITY * 0.5 * t * t
}

/// Horizontal position at `t`, `Ammo::inc_time`'s form:
/// `(float)(ex - sx) * (t/T) + (float)sx`, truncated to int.
#[inline]
pub fn xy_at(a: &AmmoWalk, t: f32) -> (i32, i32) {
    let frac = t / (a.total_time as f32);
    let x = ((a.ex - a.sx) as f32 * frac + a.sx as f32) as i32;
    let y = ((a.ey - a.sy) as f32 * frac + a.sy as f32) as i32;
    (x, y)
}

/// Everything `Ammo::init` needs that is not already on the [`AmmoWalk`].
#[derive(Clone, Copy, Debug)]
pub struct LaunchOrder {
    /// `pkg + 0x00` — ammo graphic id, straight into `gpiece`.
    pub gpiece: i32,
    /// Muzzle, from [`fire_ammo_spawns`].
    pub start: SpawnPoint,
    /// Shooter identity.
    pub who: i32,
    /// See [`LaunchOrder::who`].
    pub o: i32,
    /// Target identity, `-1` for attack-ground.
    pub whom: i32,
    /// See [`LaunchOrder::whom`].
    pub ox: i32,
    /// `find_angle(ex - sx, ey - sy)` — **UNDERIVED**, supplied by the caller.
    pub angle: i32,
    /// From the ammo graphic table (`graphic_pieces + 0xF70`, bit `0x80`).
    pub cosmetic: bool,
}

/// `Ammo::init` (`0x0067BBF0`), the ordinary targeted-arc path.
///
/// Order of operations, which is also the RNG consumption order [measured]:
///
/// 1. `flags &= 0xE3` (clears `OVERSHOOT | MISSED | NO_DAMAGE`, keeps `ALIVE | FLYING`);
/// 2. `who`/`o`/`whom`/`ox` from the package;
/// 3. anti-air dud roll — **not implemented**, see the module docs, it draws 1–2 times;
/// 4. `index = slot`, `graph_index = Objects::ammo_index++`;
/// 5. `num_guys = shooter.guy_mark` (units) or 0 (buildings);
/// 6. muzzle `sx/sy/sz` from the package;
/// 7. `accuracy`, then the miss radius `R`;
/// 8. aim point `ex/ey` = target centre, then **2 RNG draws** of scatter;
/// 9. `ez` = target `z`, `+75` if [`FLAG_OVERSHOOT`], or `guys[0].z` for an air target,
///    clamped at 0;
/// 10. `splash_area` from the **shooter's** type;
/// 11. `flags |= FLYING`, `cur_time = 0`, `total_time`, `traj`, `v1z`, `dx`.
///
/// Returns the initialised slot body. `FLAG_ALIVE` is *not* set by `init` — the slot is
/// already occupied because `add_ammo` claimed it and `flags` was left non-zero by the
/// previous tenant, or the `FLYING` bit alone satisfies `flags & 3`.
pub fn ammo_init(
    slot: usize,
    ammo_index: &mut i32,
    ord: &LaunchOrder,
    shooter: &ObjView,
    target: Option<&ObjView>,
    radius_kind: MissRadius,
    dist_for_accuracy: i32,
    rng: &mut Rng,
) -> Ammo {
    let mut a = AmmoWalk {
        flags: 0,
        ..Default::default()
    };

    a.flags &= 0xE3;
    a.who = ord.who;
    a.o = ord.o;
    a.whom = ord.whom;
    a.ox = ord.ox;
    a.gpiece = ord.gpiece;
    a.angle = ord.angle;
    if ord.cosmetic {
        a.flags |= FLAG_NO_DAMAGE;
    }

    a.index = slot as i32;
    a.graph_index = *ammo_index;
    *ammo_index += 1;

    a.num_guys = if shooter.is_unit { shooter.guy_mark } else { 0 };

    a.sx = ord.start.x;
    a.sy = ord.start.y;
    a.sz = ord.start.z;

    let acc = accuracy(
        shooter.rules.to_hit,
        shooter.rules.attenuate,
        dist_for_accuracy,
    );
    a.accuracy = acc;

    let r = miss_radius(radius_kind, acc as i32);

    if let Some(t) = target {
        a.ex = t.x;
        a.ey = t.y;
        // `flags |= 4` is set alongside the formula arm when the target is a ground unit.
        if radius_kind == MissRadius::Formula && t.is_unit && t.rules.domain == DOMAIN_LAND {
            a.flags |= FLAG_OVERSHOOT;
        }
        apply_aim_scatter(&mut a.ex, &mut a.ey, r, rng);
        a.ez = t.z;
        if a.flags & FLAG_OVERSHOOT != 0 {
            a.ez += OVERSHOOT_Z_BONUS;
        }
        if t.is_unit && t.rules.domain == DOMAIN_AIR {
            a.ez = t.guy0_z;
        }
        if a.ez < 0 {
            a.ez = 0;
        }
    }

    a.splash_area = shooter.rules.splash_area;
    a.flags |= FLAG_FLYING;
    a.cur_time = 0;
    a.total_time = arc_total_time(a.sx, a.sy, a.ex, a.ey, shooter.rules.proj_speed);
    a.traj = TRAJ_ARC;
    arc_ballistics(&mut a);

    Ammo {
        w: a,
        has_spline: false,
    }
}

// ============================================================================
// Flight — `Ammo::inc_time` `0x0067D380`
// ============================================================================

/// What one frame of `Ammo::inc_time` decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Still in the air, or still lingering as a spent ground marker.
    Flying,
    /// Reached the aim point / hit the ground: run [`ammo_do_damage`].
    Impact,
    /// `Ammo::close()` — the slot is now free, no damage.
    Closed,
}

/// `Ammo::inc_time` (`0x0067D380`), the `TRAJ_ARC` (non-spline) path.
///
/// Structure [measured]:
///
/// ```text
/// cur_time++
/// if (o >= 0 && who >= 0 && !(shooter.flags & 1)) shooter.hold_frames++   // dead shooter
/// if (!(flags & FLYING)) { if (cur_time < 200) return Flying; return Closed; }
/// if (cur_time >= total_time) goto arrival
/// if (traj != TRAJ_ARC) return Flying                       // straight shots never move
/// if (bank_dx != 0 || bank_dy != 0) {                       // aim point tracks a mover
///     ex += (int)bank_dx; ey += (int)bank_dy; restrict(&ex,&ey)
///     dx = sqrtf((ex-sx)^2 + (ey-sy)^2) / (float)total_time
/// }
/// t = (float)cur_time; frac = t / total_time
/// nx = (ex-sx)*frac + sx ; ny = (ey-sy)*frac + sy
/// if (terrain_z(nx,ny) <= v1z*t + sz + G*0.5*t*t) return Flying   // still above ground
/// ex = nx; ey = ny; ez = terrain_z; total_time = cur_time + 1     // clip into the ground
/// (loop; next iteration takes the arrival branch)
///
/// arrival:
/// if (flags & OVERSHOOT) {
///     if (!(flags & MISSED) && !hit_target() && !check_hit(0)) flags |= MISSED
///     if (flags & MISSED) {
///         if (cur_time > total_time * 3) return Closed
///         ... extrapolate past the aim point, keep flying until the ground is met ...
///     }
/// }
/// restrict(&ex,&ey); graphic_finish()
/// if (flags & NO_DAMAGE) return Closed
/// return Impact
/// ```
///
/// The ground-clip step is the reason a projectile can impact *early*: the parabola is
/// tested against terrain height every frame, and hitting a hillside converts the aim point
/// on the spot. The engine does it by rewriting `total_time` rather than by an early exit,
/// so `total_time` in the checksum is not the launch value once terrain intervenes.
pub fn ammo_inc_time<E: AmmoEnv>(
    a: &mut Ammo,
    env: &E,
    hit_target_now: impl Fn(&AmmoWalk) -> bool,
    check_hit_now: impl Fn(&mut AmmoWalk) -> bool,
) -> Step {
    loop {
        a.w.cur_time = a.w.cur_time.wrapping_add(1);

        if a.w.flags & FLAG_FLYING == 0 {
            return if a.w.cur_time < SPENT_LINGER_FRAMES {
                Step::Flying
            } else {
                a.close();
                Step::Closed
            };
        }

        if a.w.cur_time >= a.w.total_time {
            break;
        }

        if a.w.traj != TRAJ_ARC {
            // Spline flight is driven by `ammo_path`; not modelled.
            return Step::Flying;
        }

        if a.w.bank_dx != 0.0 || a.w.bank_dy != 0.0 {
            a.w.ex += a.w.bank_dx as i32;
            a.w.ey += a.w.bank_dy as i32;
            let (mut x, mut y) = (a.w.ex, a.w.ey);
            env.restrict(&mut x, &mut y);
            a.w.ex = x;
            a.w.ey = y;
            let ddx = (a.w.ex - a.w.sx) as f32;
            let ddy = (a.w.ey - a.w.sy) as f32;
            a.w.dx = (ddx * ddx + ddy * ddy).sqrt() / (a.w.total_time as f32);
        }

        let t = a.w.cur_time as f32;
        let (nx, ny) = xy_at(&a.w, t);
        let ground = env.terrain_z(nx, ny);
        if (ground as f32) <= z_at(&a.w, t) {
            return Step::Flying;
        }
        // Clipped into terrain: land here on the next iteration.
        a.w.ex = nx;
        a.w.ey = ny;
        a.w.ez = ground;
        a.w.total_time = a.w.cur_time + 1;
    }

    // ---- arrival --------------------------------------------------------------
    if a.w.flags & FLAG_OVERSHOOT != 0 {
        if a.w.flags & FLAG_MISSED == 0 && !hit_target_now(&a.w) && !check_hit_now(&mut a.w) {
            a.w.flags |= FLAG_MISSED;
        }
        if a.w.flags & FLAG_MISSED != 0 {
            if a.w.cur_time > a.w.total_time.wrapping_mul(OVERSHOOT_TIME_LIMIT_MULT) {
                a.close();
                return Step::Closed;
            }
            let t = a.w.cur_time as f32;
            let z = z_at(&a.w, t);
            let (nx, ny) = xy_at(&a.w, t);
            if !env.is_valid(nx, ny) {
                a.close();
                return Step::Closed;
            }
            let ground = env.terrain_z(nx, ny);
            if (ground as f32) < z {
                return Step::Flying; // sailing past the target
            }
            a.w.ex = nx;
            a.w.ey = ny;
            a.w.ez = z as i32;
            a.w.total_time = a.w.cur_time;
        }
    }

    let (mut x, mut y) = (a.w.ex, a.w.ey);
    env.restrict(&mut x, &mut y);
    a.w.ex = x;
    a.w.ey = y;

    if a.w.flags & FLAG_NO_DAMAGE != 0 {
        a.close();
        return Step::Closed;
    }
    Step::Impact
}

// ============================================================================
// Hit resolution — `Ammo::hit_target` `0x00678F90`, `Ammo::check_hit` `0x00678D90`
// ============================================================================

/// `Ammo::hit_target()` (`0x00678F90`) — "is my recorded target still where I aimed?"
///
/// ```text
/// if (ox < 0 || whom < 0) return false
/// t = objects[whom][ox]
/// if (!(t.flags & 1) || !t.vt[+0xBC]()) return false
/// d = vector_dist(<impact - target>)
/// if (accuracy > 100) d /= 2                       // high accuracy doubles the hit radius
/// if (t.is_unit)  return d <= t.unittype.target_size
/// else            return |ex - t.x| <= t.x_size*0x60 && |ey - t.y| <= t.y_size*0x60
/// // on failure: whom = ox = -1     <-- the target is FORGOTTEN, in place
/// ```
///
/// Two things carry into the port. First, the building test is a **rectangle** at half-tile
/// granularity (`0x60`), not a radius. Second, a failed test **mutates the projectile**,
/// clearing `whom`/`ox` — and both fields are in the checksum, so a divergent hit test
/// shows up on the `ammo` channel one frame before it shows up on `units`.
pub fn hit_target(a: &mut AmmoWalk, target: Option<&ObjView>, targetable: bool) -> bool {
    if a.ox < 0 || a.whom < 0 {
        return false;
    }
    let t = match target {
        Some(t) if t.alive && targetable => t,
        _ => return false,
    };
    let mut d = vector_dist(a.ex - t.x, a.ey - t.y);
    if a.accuracy > 100 {
        d /= 2;
    }
    let hit = if t.is_unit {
        d <= t.rules.target_size
    } else {
        (a.ex - t.x).abs() <= t.rules.x_size * 0x60 && (a.ey - t.y).abs() <= t.rules.y_size * 0x60
    };
    if !hit {
        a.ox = -1;
        a.whom = -1;
    }
    hit
}

/// `Ammo::check_hit(DomainIndex)` (`0x00678D90`) — "did I land on *something*?"
///
/// ```text
/// found = ObjectsData::find_unit(ex, ey, .., who, radius 0x180, ..)
/// whom = objects.find_who
/// if (found >= 0 && found.unittype.target_size < objects.find_dist) { ox = whom = -1 }
/// if (ox < 0) {
///     if (ex,ey in bounds) {
///         ox = ObjectsData::find_building_at(tile(ex), tile(ey), ..)
///         whom = objects.find_who
///         if (ox >= 0) return true
///         <impact sound on ground or water>
///         ox = -1
///     }
///     whom = -1
///     return false
/// }
/// return true
/// ```
///
/// The unit search radius is `0x180` = **two tiles**, and the candidate is rejected unless
/// the measured distance is within its own `target_size`. The tile index is taken through a
/// LUT at `DAT_00CAE5FC` indexed by `coord >> 6` — a division by 192 expressed as a table
/// over 64-unit cells; `coord / TILE` is the same map.
pub fn check_hit<E: AmmoEnv>(a: &mut AmmoWalk, env: &E) -> bool {
    if let Some((who, o, dist)) = env.find_unit_near(a.ex, a.ey, a.who) {
        a.ox = o;
        a.whom = who;
        if let Some(v) = env.object(who, o) {
            if v.rules.target_size < dist {
                a.ox = -1;
                a.whom = -1;
            }
        }
    } else {
        a.ox = -1;
        a.whom = -1;
    }
    if a.ox < 0 {
        if a.ex >= 0 && a.ey >= 0 && env.is_valid(a.ex, a.ey) {
            let (tx, ty) = (a.ex / TILE, a.ey / TILE);
            if let Some((who, o)) = env.find_building_at(tx, ty) {
                a.ox = o;
                a.whom = who;
                return true;
            }
            a.ox = -1;
        }
        a.whom = -1;
        return false;
    }
    true
}

// ============================================================================
// Impact — `Ammo::do_damage` `0x00678060`
// ============================================================================

/// One call the engine makes into `Object::do_damage` (`0x0064A480`).
///
/// Argument order recovered from the three call sites in `Ammo::do_damage`
/// (`0x00678982`, `0x00678B0A`, `0x00678C6C`) [measured]:
///
/// ```asm
/// push 0                  ; a8  (always 0 from the ammo path)
/// push <secondary>        ; a7  0 if this victim IS the recorded target, else 1
/// push <scale>            ; a6  0x100 for a direct hit; splash falloff otherwise
/// push [ammo+0x34]        ; a5  ammo.index -- the pool slot; < 0 means "melee, no split"
/// push [ammo+0x44]        ; a4  ammo.num_guys
/// push <angle>            ; a3  find_angle(ex-sx, ey-sy)
/// push [ammo+0x48]        ; a2  victim owner
/// push [ammo+0x4c]        ; a1  victim index
/// ecx = objects[ammo.who][ammo.o]   ; `this` is the SHOOTER
/// call Object::do_damage
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DamageCall {
    /// `a1` — victim object index.
    pub victim_o: i32,
    /// `a2` — victim owner slot.
    pub victim_who: i32,
    /// `a3` — impact angle.
    pub angle: i32,
    /// `a4` — the shooter unit's guy count.
    pub num_guys: i32,
    /// `a5` — the firing ammo's pool slot; `-1` for a melee hit.
    pub ammo_slot: i32,
    /// `a6` — 256 = full. `Object::do_damage` returns immediately when this is `<= 0`.
    pub scale256: i32,
    /// `a7` — 1 when this victim is collateral rather than the aimed-at object.
    pub secondary: i32,
    /// Shooter identity, for the `this` pointer.
    pub shooter_who: i32,
    /// See [`DamageCall::shooter_who`].
    pub shooter_o: i32,
}

/// What `Ammo::do_damage` decided to do at the impact point.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Impact {
    /// Zero, one or many `Object::do_damage` calls, in the order the engine issues them.
    pub calls: Vec<DamageCall>,
    /// True when the projectile hit bare ground: the slot is **kept** with `flags = 1` and
    /// `cur_time = 0`, lingering for [`SPENT_LINGER_FRAMES`].
    pub ground_marker: bool,
    /// True when the slot was released (`Ammo::close`).
    pub closed: bool,
}

/// Splash falloff, exactly as `Ammo::do_damage` computes it per victim [measured,
/// `0x006788E0`–`0x0067892D`]:
///
/// ```asm
/// eax = |impact.x - victim.x| ; sub  victim.x_size*192 ; cmovns   -> max(0, ..)
/// edx = |impact.y - victim.y| ; sub  victim.y_size*192 ; cmovns   -> max(0, ..)
/// call vector_dist                                                ; ecx=dx, edx=dy
/// shl eax, 8 ; cdq ; idiv (splash_area * 192)
/// ecx = 0x100 - eax
/// js  <skip this victim>
/// ```
///
/// So `scale = 256 − (dist·256)/(splash_area·192)`, a **linear** falloff measured from the
/// victim's bounding box rather than its centre, and a **negative** result skips the victim
/// entirely rather than clamping to zero. Note the box inset here is `size * 192` (a full
/// tile per size unit) whereas [`hit_target`]'s rectangle uses `size * 96` — the two tests
/// genuinely use different extents.
///
/// The two splash call sites disagree by one instruction and it is worth recording rather
/// than smoothing over: `0x00678937` is `js` (skip only when **negative**, so a victim at
/// exactly `scale == 0` still gets a call), while `0x006788C1`/`0x00678AC3` is
/// `test ecx,ecx ; jle` (skip at zero too). The observable behaviour is identical because
/// `Object::do_damage` itself opens with `cmp [ebp+0x1c], 0 ; jle <return>`, so a
/// zero-scale call is a no-op — but the *call count* differs, which matters for anything
/// that counts damage events. This function follows the `js` form and returns `Some(0)`.
#[inline]
pub fn splash_scale(
    impact_x: i32,
    impact_y: i32,
    victim: &ObjView,
    splash_area: i32,
) -> Option<i32> {
    let dx = ((impact_x - victim.x).abs() - victim.rules.x_size * TILE).max(0);
    let dy = ((impact_y - victim.y).abs() - victim.rules.y_size * TILE).max(0);
    let d = vector_dist(dx, dy);
    let den = splash_area * TILE;
    if den == 0 {
        return None;
    }
    let scale = 0x100 - ((d << 8) / den);
    if scale < 0 {
        None
    } else {
        Some(scale)
    }
}

/// `Ammo::do_damage` (`0x00678060`) — the single-target (`splash_area == 0`) path.
///
/// ```text
/// if (ox >= 0 && whom >= 0 && !(target.flags & 1)) { whom = ox = -1 }   // died in flight
/// if (o < 0 || who < 0 || gpiece_type == 9) { close(); return }
/// if (splash_area == 0) {
///     if (!hit_target()) check_hit()
///     if (ox < 0 || whom < 0 || (ox == o && whom == who)) {
///         // MISS: nothing there, or we somehow found ourselves
///         ix = ex + Random::get(0,0xFFFF) % 0x29 - 0x14
///         iy = ey + Random::get(0,0xFFFF) % 0x29 - 0x14
///         restrict(&ix,&iy); puncture_ground()
///         if (valid(ix,iy) && !water_tile) { flags = 1; cur_time = 0; return }   // linger
///     } else if (target.flags & 1) {
///         Object::do_damage(ox, whom, find_angle(ex-sx, ey-sy), num_guys, index, 0x100, 0, 0)
///     }
/// } else { <splash ring walk> }
/// close()
/// ```
///
/// **Two RNG draws on a miss, none on a hit.** That asymmetry is a stream-position hazard:
/// getting the hit test wrong desynchronises `game_random` for every later consumer in the
/// frame, not just the projectile.
///
/// `flags = 1` on the ground-miss path is an assignment, not an OR — it clears
/// `FLYING | OVERSHOOT | MISSED | NO_DAMAGE` in one store and leaves the slot occupied.
pub fn ammo_do_damage_single<E: AmmoEnv>(
    a: &mut Ammo,
    env: &E,
    target: Option<&ObjView>,
    targetable: bool,
    rng: &mut Rng,
) -> Impact {
    let mut imp = Impact::default();

    // Target died mid-flight.
    if a.w.ox >= 0 && a.w.whom >= 0 && !target.map(|t| t.alive).unwrap_or(false) {
        a.w.whom = -1;
        a.w.ox = -1;
    }
    if a.w.o < 0 || a.w.who < 0 {
        a.close();
        imp.closed = true;
        return imp;
    }

    if !hit_target(&mut a.w, target, targetable) {
        check_hit(&mut a.w, env);
    }

    let self_hit = a.w.ox == a.w.o && a.w.whom == a.w.who;
    if a.w.ox < 0 || a.w.whom < 0 || self_hit {
        let mut ix = a.w.ex + rng.draw16() % GROUND_JITTER_MOD - GROUND_JITTER_BIAS;
        let mut iy = a.w.ey + rng.draw16() % GROUND_JITTER_MOD - GROUND_JITTER_BIAS;
        env.restrict(&mut ix, &mut iy);
        if env.is_valid(ix, iy) && !env.is_water_tile(ix / TILE, iy / TILE) {
            a.w.flags = 1; // assignment, not |=
            a.w.cur_time = 0;
            imp.ground_marker = true;
            return imp;
        }
    } else if target.map(|t| t.alive).unwrap_or(false) {
        imp.calls.push(DamageCall {
            victim_o: a.w.ox,
            victim_who: a.w.whom,
            angle: a.w.angle,
            num_guys: a.w.num_guys,
            ammo_slot: a.w.index,
            scale256: 0x100,
            secondary: 0,
            shooter_who: a.w.who,
            shooter_o: a.w.o,
        });
    }

    a.close();
    imp.closed = true;
    imp
}

/// The splash path of `Ammo::do_damage`, given the victims the ring walk found.
///
/// The engine walks a precomputed disc of tile offsets (`DAT_00ADC400` / `DAT_00ADCAF0`,
/// count from `DAT_00ADD1E0[ring]`) where
/// `ring = min(10, splash_area/4 + 1)`, reads each tile's occupant from
/// `World.tiles[t] + 8` (`o`) and `+ 10` (`who`), and issues one `Object::do_damage` per
/// distinct occupant. The disc tables and the diplomacy/self filters are **not** derived;
/// pass the victim list in and this function reproduces the per-victim arithmetic.
///
/// `secondary` is `0` exactly when the victim is the projectile's recorded target.
pub fn ammo_do_damage_splash(
    a: &AmmoWalk,
    victims: &[(i32, i32, ObjView)],
    primary: Option<(i32, i32)>,
) -> Vec<DamageCall> {
    let mut out = Vec::new();
    for (who, o, v) in victims {
        let scale = match splash_scale(a.ex, a.ey, v, a.splash_area) {
            Some(s) => s,
            None => continue,
        };
        let secondary = match primary {
            Some((pw, po)) if pw == *who && po == *o => 0,
            _ => 1,
        };
        out.push(DamageCall {
            victim_o: *o,
            victim_who: *who,
            angle: a.angle,
            num_guys: a.num_guys,
            ammo_slot: a.index,
            scale256: scale,
            secondary,
            shooter_who: a.who,
            shooter_o: a.o,
        });
    }
    out
}

/// `ring = min(10, splash_area/4 + 1)` — the tile-disc selector [measured,
/// `0x006786??`: `(splash + (splash>>31 & 3)) >> 2` then `+ 1`, capped at 10].
#[inline]
pub fn splash_ring(splash_area: i32) -> i32 {
    let r = ((splash_area + ((splash_area >> 31) & 3)) >> 2) + 1;
    if r > 10 {
        10
    } else {
        r
    }
}

// ============================================================================
// The projectile → damage handoff: `AMMO_PER_ATT` splitting
// ============================================================================

/// The whole-plus-sixteenths damage a single projectile deposits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SplitDamage {
    /// Whole hit points.
    pub hits: i32,
    /// The leftover, in sixteenths, stored to `ObjectData::damage_frac` (`+0x3B`, `char`).
    pub frac16: i8,
}

/// The projectile-specific head of `Object::do_damage` (`0x0064A480`) — how one round's
/// raw `ObjectData::get_damage` result becomes applied damage.
///
/// Recovered from `0x0064A49E`–`0x0064A7F1` [measured]:
///
/// ```asm
/// 0x64a49e  cmp [ebp+0x1c], 0 ; jle <return>     ; scale <= 0 -> NOTHING HAPPENS
/// 0x64a4f7  call ObjectData::get_damage(o, who, angle, secondary, 1, &out)
///
/// ; --- shooter is a Unit (vt[+0x18]) ---
/// 0x64a701  imul eax, [ebp+0x1c]                 ; D *= scale
/// 0x64a705  cmp eax, 0x100 ; cmovle eax, 0x100   ; floor of 256, i.e. 1 hit point
/// 0x64a70a  cmp [ebp+0x18], 0 ; jl <skip>        ; ammo_slot < 0  -> melee, no split
/// 0x64a735  idiv [shooter_objtype + 0x208]       ; D /= ammo_per_att
/// 0x64a766  idiv [shooter_unittype + 0x308]      ; D /= uber_size
/// 0x64a76e  sar eax, 4                           ; D /= 16
/// 0x64a784  mov [damage_frac], cl                ; low 4 bits kept as sixteenths
/// 0x64a78a  sar ecx, 4                           ; D /= 16  -> whole hit points
///
/// ; --- shooter is a Build (vt[+0x20]) ---
/// 0x64a7bd  ecx = [shooter_objtype + 0x208]
/// 0x64a7c6  imul eax, [ebp+0x1c] ; idiv ecx      ; D = D*scale / ammo_per_att
/// 0x64a7d3  sar eax,4 ; <frac> ; sar eax,4       ; no uber_size, NO floor of 256
/// ```
///
/// So, reading it as one expression with `scale = 256` for a direct hit:
///
/// | shooter | applied |
/// |---|---|
/// | unit  | `max(D·scale, 256) / ammo_per_att / uber_size / 256` |
/// | build | `D·scale / ammo_per_att / 256` |
///
/// **This is the answer to "how is `AMMO_PER_ATT` damage split, and where does armor go".**
/// Armor is *not* applied here at all — it is subtracted inside `get_damage`, at step 22 of
/// 31, on the **raw per-projectile** number. Because every projectile makes its own
/// `get_damage` call, **armor is subtracted once per projectile, then the result is divided
/// by `ammo_per_att`.** A 4-round volley against 3 armor therefore loses 3 armor four times
/// and keeps a quarter of each — algebraically the same as losing 3 armor once *only*
/// because the divide comes after, but numerically different from `(D − armor)/4` the moment
/// the conditional floor of 1 inside `get_damage` bites, which it does exactly when armor
/// eats the round. That is where high-armor targets shrug off many-projectile attackers.
///
/// The `/16` twice is not `/256` in disguise: the intermediate is truncated, and the low 4
/// bits of the intermediate are **kept** as `damage_frac`, so sub-hit-point damage
/// accumulates across shots instead of vanishing.
///
/// A unit fires one projectile per **guy** ([`fire_ammo_spawns`]) and divides by
/// `uber_size`, so a full-strength squad delivers `D/ammo_per_att` in aggregate and a
/// half-dead squad delivers half that — casualties reduce damage, mechanically, through the
/// projectile count.
pub fn split_damage(
    raw: i32,
    scale256: i32,
    shooter_is_unit: bool,
    ammo_per_att: i32,
    uber_size: i32,
    from_projectile: bool,
) -> Option<SplitDamage> {
    if scale256 <= 0 {
        return None; // `Object::do_damage` returns before touching anything.
    }
    let mut d = raw.wrapping_mul(scale256);
    if shooter_is_unit {
        if d <= 0x100 {
            d = 0x100;
        }
        if from_projectile && ammo_per_att != 0 {
            d /= ammo_per_att;
        }
        if uber_size != 0 {
            d /= uber_size;
        }
    } else {
        if ammo_per_att == 0 {
            return None;
        }
        d /= ammo_per_att;
    }
    let sixteenths = sar4(d);
    let frac = rem16(sixteenths);
    let hits = sar4(sixteenths);
    Some(SplitDamage {
        hits,
        frac16: frac as i8,
    })
}

/// `cdq ; and edx,0xf ; add eax,edx ; sar eax,4` — a truncating divide by 16.
#[inline]
fn sar4(x: i32) -> i32 {
    let fix = if x < 0 { 0xF } else { 0 };
    x.wrapping_add(fix) >> 4
}

/// `and ecx, 0x8000000f ; jns ; dec ; or 0xfffffff0 ; inc` — C's `%16`, sign-preserving.
#[inline]
fn rem16(x: i32) -> i32 {
    x % 16
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    struct FlatWorld {
        w: i32,
        h: i32,
        z: i32,
    }
    impl AmmoEnv for FlatWorld {
        fn object(&self, _who: i32, _o: i32) -> Option<ObjView> {
            None
        }
        fn terrain_z(&self, _x: i32, _y: i32) -> i32 {
            self.z
        }
        fn world_tiles(&self) -> (i32, i32) {
            (self.w, self.h)
        }
        fn find_unit_near(&self, _x: i32, _y: i32, _who: i32) -> Option<(i32, i32, i32)> {
            None
        }
        fn find_building_at(&self, _tx: i32, _ty: i32) -> Option<(i32, i32)> {
            None
        }
        fn is_water_tile(&self, _tx: i32, _ty: i32) -> bool {
            false
        }
    }

    #[test]
    fn layout_matches_pdb() {
        assert_layout();
    }

    #[test]
    fn gravity_bit_pattern() {
        assert_eq!(GRAVITY.to_bits(), 0xC127_CCCD);
        assert!((GRAVITY - -10.4875).abs() < 1e-4);
    }

    #[test]
    fn vector_dist_matches_the_disassembly() {
        // hi + lo^2/(2*hi)
        assert_eq!(vector_dist(0, 0), 0);
        assert_eq!(vector_dist(10, 0), 10);
        assert_eq!(vector_dist(0, -10), 10);
        assert_eq!(vector_dist(3, 4), 4 + 9 / 8); // 5
        assert_eq!(vector_dist(-4, 3), 4 + 9 / 8);
        assert_eq!(vector_dist(192, 192), 192 + (192 * 192) / 384); // 288
                                                                    // the >= 60000 fallback: (2*hi + lo)/2
        assert_eq!(vector_dist(70000, 60000), (60000 + 140000) >> 1);
        // it is an approximation, and deliberately so: never assert it equals sqrt.
        let approx = vector_dist(1000, 1000) as f64;
        let exact = (2_000_000f64).sqrt();
        assert!(approx > exact, "the expansion over-estimates for lo == hi");
    }

    #[test]
    fn rng_lo_eq_hi_does_not_advance() {
        let mut r = Rng(12345);
        let before = r.0;
        assert_eq!(r.in_range(7, 7), 7);
        assert_eq!(r.0, before, "lo == hi must not consume a draw");
        r.in_range(0, 10);
        assert_ne!(r.0, before);
    }

    #[test]
    fn rng_is_half_open() {
        let mut r = Rng(1);
        for _ in 0..5000 {
            let v = r.in_range(0, 10);
            assert!((0..10).contains(&v));
        }
    }

    #[test]
    fn pool_reuses_the_lowest_free_slot() {
        let mut p = AmmoPool::new();
        assert_eq!(p.slots.len(), AMMO_POOL_SLOTS);
        for i in 0..5 {
            let s = p.alloc_slot();
            assert_eq!(s, i);
            p.slots[s].w.flags = FLAG_ALIVE | FLAG_FLYING;
        }
        p.slots[2].close();
        assert_eq!(
            p.alloc_slot(),
            2,
            "add_ammo scans from 0 for flags & 3 == 0"
        );
    }

    #[test]
    fn pool_grows_past_the_preallocated_200() {
        let mut p = AmmoPool::new();
        for _ in 0..AMMO_POOL_SLOTS {
            let s = p.alloc_slot();
            p.slots[s].w.flags = FLAG_FLYING;
        }
        assert_eq!(p.alloc_slot(), AMMO_POOL_SLOTS);
        assert_eq!(p.slots.len(), AMMO_POOL_SLOTS + 1);
    }

    #[test]
    fn checksum_ignores_free_slots_entirely() {
        let mut p = AmmoPool::new();
        let empty = p.checksum();
        assert_eq!(empty, 1, "adler-32 of nothing, seeded to 1");
        p.slots.push(Ammo::default());
        assert_eq!(p.checksum(), empty, "a free slot contributes zero bytes");
    }

    #[test]
    fn checksum_is_order_sensitive_and_float_sensitive() {
        let mut p = AmmoPool::new();
        p.slots[0].w.flags = FLAG_ALIVE | FLAG_FLYING;
        p.slots[0].w.ex = 1000;
        p.slots[1].w.flags = FLAG_ALIVE | FLAG_FLYING;
        p.slots[1].w.ex = 2000;
        let a = p.checksum();

        // Same two projectiles, swapped slots -> different channel value.
        let mut q = AmmoPool::new();
        q.slots[0] = p.slots[1];
        q.slots[1] = p.slots[0];
        assert_ne!(a, q.checksum(), "slot order is part of the ammo channel");

        // One ULP on a ballistics float must move the checksum.
        let mut r = p.clone();
        r.slots[0].w.v1z = f32::from_bits(r.slots[0].w.v1z.to_bits() + 1);
        assert_ne!(a, r.checksum(), "v1z is hashed bit-for-bit");
    }

    #[test]
    fn checksum_walk_length_is_101_bytes_per_live_slot() {
        // flags(1) + body(99) + has_spline(1)
        let mut p = AmmoPool {
            slots: vec![Ammo::default()],
            ammo_index: 0,
        };
        p.slots[0].w.flags = FLAG_FLYING;
        let via_pool = p.checksum();
        let b = p.slots[0].w.as_bytes();
        let mut a = 1u32;
        a = adler32(a, &b[0..1]);
        a = adler32(a, &b[1..100]);
        a = adler32(a, &[0u8]);
        assert_eq!(via_pool, a);
    }

    #[test]
    fn adler32_known_vectors() {
        assert_eq!(adler32(1, b""), 1);
        assert_eq!(adler32(1, b"a"), 0x0062_0062);
        assert_eq!(adler32(1, b"abc"), 0x024D_0127);
        assert_eq!(adler32(1, b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn accuracy_loses_attenuate_per_tile() {
        // to_hit 100, attenuate 3
        assert_eq!(accuracy(100, 3, 0), 100);
        assert_eq!(
            accuracy(100, 3, TILE - 1),
            100,
            "sub-tile range costs nothing"
        );
        assert_eq!(accuracy(100, 3, TILE), 97);
        assert_eq!(accuracy(100, 3, 10 * TILE), 70);
        assert_eq!(accuracy(100, 3, 100 * TILE), 5, "floored at 5, not at 0");
    }

    #[test]
    fn miss_radius_shrinks_as_accuracy_rises() {
        assert_eq!(miss_radius_formula(100), 96);
        assert_eq!(miss_radius_formula(50), 160);
        assert_eq!(miss_radius_formula(5), 400);
        // acc > 100 takes the extra /4, and (100-acc)/5 truncates toward zero.
        assert_eq!(miss_radius_formula(200), (9600 / (-20 + 200)) / 4);
        assert!(miss_radius_formula(150) < miss_radius_formula(100));
    }

    #[test]
    fn aim_scatter_skips_both_draws_when_the_radius_is_tiny() {
        let mut rng = Rng(99);
        let before = rng.0;
        let (mut x, mut y) = (500, 500);
        apply_aim_scatter(&mut x, &mut y, 1, &mut rng);
        assert_eq!((x, y), (500, 500));
        assert_eq!(
            rng.0, before,
            "R <= 1 consumes no RNG -- a stream-position hazard"
        );

        let (mut x, mut y) = (500, 500);
        apply_aim_scatter(&mut x, &mut y, 96, &mut rng);
        assert_ne!(rng.0, before);
        assert!((x - 500).abs() <= 48 && (y - 500).abs() <= 48);
    }

    #[test]
    fn ballistics_land_where_they_were_aimed() {
        let mut a = AmmoWalk {
            sx: 0,
            sy: 0,
            sz: 100,
            ex: 3000,
            ey: 0,
            ez: 40,
            ..Default::default()
        };
        a.total_time = arc_total_time(a.sx, a.sy, a.ex, a.ey, 100);
        assert_eq!(a.total_time, 30);
        arc_ballistics(&mut a);
        let land = z_at(&a, a.total_time as f32);
        assert!(
            (land - a.ez as f32).abs() < 0.05,
            "z(T) must equal ez, got {land}"
        );
        let (x, y) = xy_at(&a, a.total_time as f32);
        assert_eq!((x, y), (a.ex, a.ey));
        // and the apex is above both ends
        let mid = z_at(&a, (a.total_time / 2) as f32);
        assert!(mid > a.sz as f32 && mid > a.ez as f32, "it arcs");
    }

    #[test]
    fn total_time_is_never_zero() {
        assert_eq!(arc_total_time(0, 0, 0, 0, 100), 1);
        assert_eq!(
            arc_total_time(0, 0, 5, 0, 100),
            1,
            "float truncation would give 0"
        );
    }

    #[test]
    fn flight_reaches_impact_after_total_time_frames() {
        let env = FlatWorld { w: 64, h: 64, z: 0 };
        let mut a = Ammo::default();
        a.w.flags = FLAG_ALIVE | FLAG_FLYING;
        a.w.sx = 0;
        a.w.sy = 0;
        a.w.sz = 200;
        a.w.ex = 2000;
        a.w.ey = 0;
        a.w.ez = 0;
        a.w.traj = TRAJ_ARC;
        a.w.total_time = arc_total_time(0, 0, 2000, 0, 100);
        arc_ballistics(&mut a.w);
        let t = a.w.total_time;
        let mut frames = 0;
        loop {
            match ammo_inc_time(&mut a, &env, |_| true, |_| true) {
                Step::Flying => frames += 1,
                Step::Impact => break,
                Step::Closed => panic!("closed instead of impacting"),
            }
            assert!(frames < 1000, "did not converge");
        }
        assert_eq!(
            a.w.cur_time, t,
            "impact on the frame cur_time reaches total_time"
        );
    }

    #[test]
    fn flat_ground_can_never_clip_a_ballistic_shot() {
        // Worth pinning: `arc_ballistics` solves v1z so that z(T) == ez under negative
        // gravity, which makes the parabola strictly ABOVE the chord between its endpoints.
        // On level terrain below both ends the clip branch is therefore unreachable -- it
        // exists for rising ground, not for "the shot is low".
        let env = FlatWorld {
            w: 64,
            h: 64,
            z: 400,
        };
        let mut a = Ammo::default();
        a.w.flags = FLAG_ALIVE | FLAG_FLYING;
        a.w.sz = 500;
        a.w.ex = 6000;
        a.w.ez = 500;
        a.w.traj = TRAJ_ARC;
        a.w.total_time = arc_total_time(0, 0, 6000, 0, 100);
        arc_ballistics(&mut a.w);
        let launched_total = a.w.total_time;
        while ammo_inc_time(&mut a, &env, |_| true, |_| true) == Step::Flying {}
        assert_eq!(
            a.w.total_time, launched_total,
            "no clip over flat low ground"
        );
        assert_eq!(a.w.ex, 6000, "it reached the aim point");
    }

    #[test]
    fn terrain_clips_a_shot_that_would_pass_through_a_hill() {
        // Ground rises as `z = x`, steeper than the arc's descent: the shell buries itself
        // in the hillside well before it reaches the aim point.
        struct Ramp;
        impl AmmoEnv for Ramp {
            fn object(&self, _w: i32, _o: i32) -> Option<ObjView> {
                None
            }
            fn terrain_z(&self, x: i32, _y: i32) -> i32 {
                x
            }
            fn world_tiles(&self) -> (i32, i32) {
                (64, 64)
            }
            fn find_unit_near(&self, _x: i32, _y: i32, _w: i32) -> Option<(i32, i32, i32)> {
                None
            }
            fn find_building_at(&self, _tx: i32, _ty: i32) -> Option<(i32, i32)> {
                None
            }
            fn is_water_tile(&self, _tx: i32, _ty: i32) -> bool {
                false
            }
        }
        let env = Ramp;
        let mut a = Ammo::default();
        a.w.flags = FLAG_ALIVE | FLAG_FLYING;
        a.w.sz = 500;
        a.w.ex = 6000;
        a.w.ez = 500;
        a.w.traj = TRAJ_ARC;
        a.w.total_time = arc_total_time(0, 0, 6000, 0, 100);
        arc_ballistics(&mut a.w);
        let launched_total = a.w.total_time;
        let mut steps = 0;
        loop {
            match ammo_inc_time(&mut a, &env, |_| true, |_| true) {
                Step::Flying => steps += 1,
                Step::Impact => break,
                Step::Closed => panic!("clipped shots impact, they do not vanish"),
            }
            assert!(steps < 1000, "did not converge");
        }
        assert!(
            a.w.total_time < launched_total,
            "total_time is REWRITTEN by the ground clip: {} -> {}",
            launched_total,
            a.w.total_time
        );
        assert!(
            a.w.ex < 6000,
            "it landed short of the aim point, at x={}",
            a.w.ex
        );
        assert_eq!(
            a.w.ez, a.w.ex,
            "impact height becomes the terrain height there"
        );
    }

    #[test]
    fn spent_ground_marker_lingers_200_frames_and_stays_checksummed() {
        let env = FlatWorld { w: 64, h: 64, z: 0 };
        let mut a = Ammo::default();
        a.w.flags = 1; // exactly what the miss path writes
        a.w.cur_time = 0;
        for i in 1..SPENT_LINGER_FRAMES {
            assert_eq!(
                ammo_inc_time(&mut a, &env, |_| true, |_| true),
                Step::Flying
            );
            assert!(a.occupied(), "frame {i}: still in the ammo channel");
        }
        assert_eq!(
            ammo_inc_time(&mut a, &env, |_| true, |_| true),
            Step::Closed
        );
        assert!(!a.occupied());
    }

    #[test]
    fn a_unit_fires_one_round_per_live_guy_and_draws_no_rng() {
        let shooter = ObjView {
            alive: true,
            is_unit: true,
            x: 1000,
            y: 1000,
            z: 0,
            guy_mark: 3,
            rules: ShooterRules {
                ammo_per_att: 5,
                ..Default::default()
            },
            ..Default::default()
        };
        let guys = [
            GuyPos { x: 10, y: 20, z: 5 },
            GuyPos { x: 30, y: 40, z: 6 },
            GuyPos { x: 50, y: 60, z: 7 },
            GuyPos { x: 70, y: 80, z: 8 },
        ];
        let mut rng = Rng(4242);
        let before = rng.0;
        let mut out = Vec::new();
        fire_ammo_spawns(&shooter, &guys, &mut rng, &mut out);
        assert_eq!(out.len(), 3, "guy_mark rounds, not ammo_per_att");
        assert_eq!(
            out[0],
            SpawnPoint {
                x: 10,
                y: 20,
                z: 5 + MUZZLE_Z_UNIT
            }
        );
        assert_eq!(rng.0, before, "the unit path is RNG-free");
    }

    #[test]
    fn a_building_fires_ammo_per_att_rounds_scattered_over_its_footprint() {
        let shooter = ObjView {
            alive: true,
            is_unit: false,
            x: 5000,
            y: 6000,
            z: 10,
            rules: ShooterRules {
                ammo_per_att: 4,
                x_size: 2,
                y_size: 3,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut rng = Rng(7);
        let mut out = Vec::new();
        fire_ammo_spawns(&shooter, &[], &mut rng, &mut out);
        assert_eq!(out.len(), 4);
        let (w, h) = (2 * 0x60, 3 * 0x60);
        for s in &out {
            assert!(s.x >= 5000 - w / 2 && s.x < 5000 - w / 2 + w);
            assert!(s.y >= 6000 - h / 2 && s.y < 6000 - h / 2 + h);
            assert_eq!(s.z, 10 + MUZZLE_Z_BUILD);
        }
        assert!(out.iter().any(|s| s.x != out[0].x), "genuinely scattered");
    }

    #[test]
    fn building_scatter_takes_exactly_two_draws_per_round() {
        let shooter = ObjView {
            is_unit: false,
            rules: ShooterRules {
                ammo_per_att: 3,
                x_size: 2,
                y_size: 2,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut a = Rng(1);
        let mut out = Vec::new();
        fire_ammo_spawns(&shooter, &[], &mut a, &mut out);
        let mut b = Rng(1);
        for _ in 0..6 {
            b.draw16();
        }
        assert_eq!(a.0, b.0, "2 draws x 3 rounds");
    }

    #[test]
    fn hit_target_uses_a_rectangle_for_buildings_and_a_radius_for_units() {
        let build = ObjView {
            alive: true,
            is_unit: false,
            x: 1000,
            y: 1000,
            rules: ShooterRules {
                x_size: 2,
                y_size: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut a = AmmoWalk {
            ox: 1,
            whom: 0,
            ex: 1000 + 2 * 0x60,
            ey: 1000,
            ..Default::default()
        };
        assert!(hit_target(&mut a, Some(&build), true), "on the x edge");
        let mut a = AmmoWalk {
            ox: 1,
            whom: 0,
            ex: 1000,
            ey: 1000 + 1 * 0x60 + 1,
            ..Default::default()
        };
        assert!(
            !hit_target(&mut a, Some(&build), true),
            "one unit past the y edge"
        );
        assert_eq!(
            (a.ox, a.whom),
            (-1, -1),
            "a miss forgets the target in place"
        );

        let unit = ObjView {
            alive: true,
            is_unit: true,
            x: 0,
            y: 0,
            rules: ShooterRules {
                target_size: 50,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut a = AmmoWalk {
            ox: 1,
            whom: 0,
            ex: 50,
            ey: 0,
            accuracy: 90,
            ..Default::default()
        };
        assert!(hit_target(&mut a, Some(&unit), true));
        let mut a = AmmoWalk {
            ox: 1,
            whom: 0,
            ex: 60,
            ey: 0,
            accuracy: 90,
            ..Default::default()
        };
        assert!(!hit_target(&mut a, Some(&unit), true));
        // accuracy > 100 halves the measured distance, i.e. doubles the effective radius
        let mut a = AmmoWalk {
            ox: 1,
            whom: 0,
            ex: 60,
            ey: 0,
            accuracy: 101,
            ..Default::default()
        };
        assert!(hit_target(&mut a, Some(&unit), true));
    }

    #[test]
    fn a_dead_target_is_never_hit() {
        let dead = ObjView {
            alive: false,
            is_unit: true,
            ..Default::default()
        };
        let mut a = AmmoWalk {
            ox: 1,
            whom: 0,
            ..Default::default()
        };
        assert!(!hit_target(&mut a, Some(&dead), true));
    }

    #[test]
    fn splash_falls_off_linearly_from_the_bounding_box() {
        let v = ObjView {
            x: 0,
            y: 0,
            rules: ShooterRules {
                x_size: 0,
                y_size: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        // splash_area 2 tiles -> denominator 384
        assert_eq!(splash_scale(0, 0, &v, 2), Some(256));
        assert_eq!(splash_scale(192, 0, &v, 2), Some(256 - (192 * 256) / 384));
        assert_eq!(
            splash_scale(384, 0, &v, 2),
            Some(0),
            "exactly at the edge still fires"
        );
        // The guard is `js`, not `jle`: the quotient truncates, so the first distance that
        // actually goes negative is 386, not 385. Verified against the divide, not assumed.
        assert_eq!(splash_scale(385, 0, &v, 2), Some(0));
        assert_eq!(
            splash_scale(386, 0, &v, 2),
            None,
            "past the edge is skipped, not clamped"
        );

        // a big target is measured from its box, so it takes full damage further out
        let big = ObjView {
            x: 0,
            y: 0,
            rules: ShooterRules {
                x_size: 1,
                y_size: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(splash_scale(192, 0, &big, 2), Some(256));
    }

    #[test]
    fn splash_ring_selector() {
        assert_eq!(splash_ring(0), 1);
        assert_eq!(splash_ring(4), 2);
        assert_eq!(splash_ring(7), 2);
        assert_eq!(splash_ring(8), 3);
        assert_eq!(splash_ring(100), 10);
    }

    #[test]
    fn scale_zero_means_no_damage_call_at_all() {
        assert_eq!(split_damage(1000, 0, true, 1, 1, true), None);
        assert_eq!(split_damage(1000, -5, false, 1, 1, true), None);
    }

    #[test]
    fn ammo_per_att_divides_a_building_volley() {
        // One round of a 4-round building volley: D*256/4/256 == D/4.
        let one = split_damage(400, 0x100, false, 4, 0, true).unwrap();
        assert_eq!(one.hits, 100);
        // Four rounds add back up to the raw number.
        assert_eq!(one.hits * 4, 400);
    }

    #[test]
    fn a_unit_squad_divides_by_ammo_per_att_and_uber_size() {
        // 5 guys, ammo_per_att 2: one round is D/(2*5); a full squad delivers D/2.
        let one = split_damage(1000, 0x100, true, 2, 5, true).unwrap();
        assert_eq!(one.hits, 100);
        assert_eq!(one.hits * 5, 500);
    }

    #[test]
    fn a_melee_hit_skips_the_ammo_per_att_divide() {
        let melee = split_damage(1000, 0x100, true, 4, 1, false).unwrap();
        let shot = split_damage(1000, 0x100, true, 4, 1, true).unwrap();
        assert_eq!(melee.hits, 1000);
        assert_eq!(shot.hits, 250);
    }

    #[test]
    fn sub_hit_point_damage_survives_as_sixteenths() {
        // D*256 / 3 / 256 has a remainder; it must land in damage_frac, not vanish.
        let s = split_damage(10, 0x100, false, 3, 0, true).unwrap();
        assert_eq!(s.hits, 3);
        assert!(s.frac16 > 0, "the 1/3 remainder is carried as sixteenths");
        // 10*256/3 = 853 sixteenths-of-sixteenths -> 53 sixteenths -> 3 hits + 5/16
        assert_eq!(s.frac16, 5);
    }

    #[test]
    fn the_unit_floor_of_one_hit_point_bites_when_armor_ate_the_round() {
        // get_damage already floored the raw value at 1; a unit shooter then floors D*scale
        // at 256, so a single-guy single-round attacker always lands one hit point.
        let s = split_damage(1, 0x100, true, 1, 1, true).unwrap();
        assert_eq!(s.hits, 1);
        // A splash victim at 1/256 scale would otherwise round to zero:
        let s = split_damage(1, 1, true, 1, 1, true).unwrap();
        assert_eq!(s.hits, 1, "the 0x100 floor is a UNIT-only rule");
        // buildings have no such floor
        let s = split_damage(1, 1, false, 1, 0, true).unwrap();
        assert_eq!(s.hits, 0);
    }

    #[test]
    fn miss_at_the_impact_point_costs_two_rng_draws_and_leaves_a_marker() {
        let env = FlatWorld { w: 64, h: 64, z: 0 };
        let mut a = Ammo::default();
        a.w.flags = FLAG_ALIVE | FLAG_FLYING;
        a.w.ex = 3000;
        a.w.ey = 3000;
        a.w.who = 0;
        a.w.o = 1;
        a.w.ox = -1;
        a.w.whom = -1;
        let mut rng = Rng(555);
        let before = rng.0;
        let imp = ammo_do_damage_single(&mut a, &env, None, false, &mut rng);
        assert!(imp.calls.is_empty());
        assert!(imp.ground_marker);
        assert_eq!(a.w.flags, 1, "flags is ASSIGNED 1, not OR-ed");
        assert_eq!(a.w.cur_time, 0);
        assert!(a.occupied(), "the slot stays in the ammo channel");
        let mut check = Rng(before);
        check.draw16();
        check.draw16();
        assert_eq!(rng.0, check.0, "exactly two draws on a ground miss");
    }

    #[test]
    fn a_direct_hit_costs_no_rng_and_emits_one_full_scale_call() {
        let env = FlatWorld { w: 64, h: 64, z: 0 };
        let target = ObjView {
            alive: true,
            is_unit: true,
            x: 3000,
            y: 3000,
            rules: ShooterRules {
                target_size: 100,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut a = Ammo::default();
        a.w.flags = FLAG_ALIVE | FLAG_FLYING;
        a.w.ex = 3010;
        a.w.ey = 3000;
        a.w.who = 0;
        a.w.o = 1;
        a.w.ox = 4;
        a.w.whom = 2;
        a.w.index = 17;
        a.w.num_guys = 6;
        a.w.angle = 0x1234;
        let mut rng = Rng(555);
        let before = rng.0;
        let imp = ammo_do_damage_single(&mut a, &env, Some(&target), true, &mut rng);
        assert_eq!(
            rng.0, before,
            "a hit draws nothing -- the miss/hit RNG asymmetry"
        );
        assert_eq!(imp.calls.len(), 1);
        let c = imp.calls[0];
        assert_eq!(c.victim_o, 4);
        assert_eq!(c.victim_who, 2);
        assert_eq!(c.scale256, 0x100);
        assert_eq!(c.secondary, 0);
        assert_eq!(c.ammo_slot, 17);
        assert_eq!(c.num_guys, 6);
        assert!(imp.closed && !a.occupied(), "the slot is freed on a hit");
    }

    #[test]
    fn splash_marks_the_primary_target_as_non_secondary() {
        let a = AmmoWalk {
            ex: 0,
            ey: 0,
            splash_area: 3,
            ox: 7,
            whom: 1,
            ..Default::default()
        };
        let v = ObjView {
            x: 100,
            y: 0,
            ..Default::default()
        };
        let calls = ammo_do_damage_splash(&a, &[(1, 7, v), (2, 9, v)], Some((1, 7)));
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].secondary, 0);
        assert_eq!(calls[1].secondary, 1);
        assert_eq!(calls[0].scale256, calls[1].scale256);
    }

    #[test]
    fn end_to_end_one_shot_moves_the_channel_then_frees_the_slot() {
        let env = FlatWorld { w: 64, h: 64, z: 0 };
        let mut pool = AmmoPool::new();
        let idle = pool.checksum();

        let shooter = ObjView {
            alive: true,
            is_unit: true,
            x: 0,
            y: 0,
            z: 0,
            guy_mark: 2,
            rules: ShooterRules {
                to_hit: 100,
                attenuate: 2,
                ammo_per_att: 1,
                proj_speed: 60,
                uber_size: 2,
                splash_area: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        let target = ObjView {
            alive: true,
            is_unit: true,
            x: 1920,
            y: 0,
            z: 0,
            rules: ShooterRules {
                target_size: 100,
                domain: DOMAIN_LAND,
                ..Default::default()
            },
            ..Default::default()
        };
        let guys = [GuyPos { x: 0, y: 0, z: 0 }, GuyPos { x: 20, y: 0, z: 0 }];
        let mut rng = Rng(0xDEAD_BEEF);

        let mut spawns = Vec::new();
        fire_ammo_spawns(&shooter, &guys, &mut rng, &mut spawns);
        assert_eq!(spawns.len(), 2, "one round per guy");

        let mut slots = Vec::new();
        for sp in &spawns {
            let slot = pool.alloc_slot();
            let ord = LaunchOrder {
                gpiece: 3,
                start: *sp,
                who: 0,
                o: 1,
                whom: 2,
                ox: 4,
                angle: 0,
                cosmetic: false,
            };
            let dist = vector_dist(target.x - sp.x, target.y - sp.y);
            pool.slots[slot] = ammo_init(
                slot,
                &mut pool.ammo_index,
                &ord,
                &shooter,
                Some(&target),
                MissRadius::Formula,
                dist,
                &mut rng,
            );
            slots.push(slot);
        }
        assert_eq!(slots, vec![0, 1]);
        assert_eq!(pool.ammo_index, 2);
        assert_ne!(
            pool.checksum(),
            idle,
            "live projectiles move the ammo channel"
        );
        assert!(
            pool.slots[0].w.flags & FLAG_OVERSHOOT != 0,
            "ground unit target -> overshoot"
        );

        // Fly them out.
        let mut impacts = 0;
        for _ in 0..500 {
            for &s in &slots {
                if !pool.slots[s].occupied() {
                    continue;
                }
                match ammo_inc_time(&mut pool.slots[s], &env, |_| true, |_| true) {
                    Step::Impact => {
                        let mut rng2 = rng;
                        let imp = ammo_do_damage_single(
                            &mut pool.slots[s],
                            &env,
                            Some(&target),
                            true,
                            &mut rng2,
                        );
                        rng = rng2;
                        impacts += imp.calls.len();
                    }
                    Step::Flying | Step::Closed => {}
                }
            }
            if pool.live() == 0 {
                break;
            }
        }
        assert_eq!(impacts, 2, "both rounds landed on the target");
        assert_eq!(
            pool.checksum(),
            idle,
            "the channel returns to its idle value"
        );
        assert_eq!(pool.ammo_index, 2, "ammo_index does NOT rewind");
    }
}
