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
//! * `TRAJ_SPLINE` cruise and nuke paths — `calc_from_dir` / both arms of
//!   `calc_nuke_spline` → `calc_spline` → `generate_bspline` → `build_normals`, the six nested
//!   array walks, the slot-aligned pool sidecar/recycler, and indexed flight step are implemented
//!   below. Live nuke and initial cruise launch are installed through the pool, and the dynamic
//!   target-envelope/retarget transaction is pool-owned and driven by step 15. Aircraft wrecks
//!   actually use `TRAJ_ARC`; their [`ammo_init_crash`] constructor is implemented below.
//! * `find_angle` (`0x0092D130`) lives in [`crate::trig`]. The ordinary targeted adapter
//!   still accepts the already-computed angle because attack-ground and spline callers
//!   select different source points.
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
// `Spline` — cruise/nuke path state and its nested checksum walk
// ============================================================================

/// Retail's three-float `Vector<float>` payload.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SplineVec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl SplineVec3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    fn finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }

    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }

    fn scale(self, s: f32) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }

    fn squared_length(self) -> f32 {
        (self.x * self.x + self.y * self.y) + self.z * self.z
    }

    /// `Vector<float>::get_length` / `0x00420830`: three `mulss`, two `addss`, then
    /// double-precision sqrt rounded back to f32.
    fn length(self) -> f32 {
        (self.squared_length() as f64).sqrt() as f32
    }

    fn normalized(self) -> Self {
        let square = self.squared_length();
        if square == 0.0 || square == 1.0 {
            return self;
        }
        let inverse = 1.0 / ((square as f64).sqrt() as f32);
        self.scale(inverse)
    }
}

/// SSE `cvttss2si`: truncate toward zero, returning the architectural indefinite integer
/// for NaN and values outside the signed 32-bit range. Rust's float cast saturates instead.
#[inline]
fn cvttss2si(value: f32) -> i32 {
    if !value.is_finite() || !(-2_147_483_648.0..2_147_483_648.0).contains(&value) {
        i32::MIN
    } else {
        value as i32
    }
}

/// A checksum-complete `SplineData` for the B-spline arm used by cruise projectiles.
///
/// All six arrays use the engine's constructed-empty header (`size=0`, `increment=-1`),
/// hence grow 0→4→8→16 rather than Rust `Vec`'s policy. `SplineData::walk_data`
/// (`0x009132B0`) hashes each non-empty array's length, size, increment, flags and elements.
#[derive(Clone, Debug, PartialEq)]
pub struct RetailSpline {
    pub spline_type: i32,
    pub flags: i32,
    pub max_control_depth_ratio: f32,
    pub total_spline_length: f32,
    pub last_knot: i32,
    pub curr_dist: f32,
    pub next_search_dist: f32,
    pub search_scan: i32,
    pub degree: u16,
    pub depth: u16,
    pub control_verts: crate::container::EngineArray<SplineVec3>,
    pub knots: crate::container::EngineArray<f32>,
    pub weights: crate::container::EngineArray<f32>,
    pub spline_knots: crate::container::EngineArray<f32>,
    pub spline_verts: crate::container::EngineArray<SplineVec3>,
    pub spline_normals: crate::container::EngineArray<SplineVec3>,
}

impl Default for RetailSpline {
    fn default() -> Self {
        Self::new()
    }
}

impl RetailSpline {
    /// `SplineData::SplineData` `0x004AFEA0`, `Spline::Spline` `0x00914530`, then
    /// `Spline::clear` `0x00914410`.
    pub fn new() -> Self {
        Self {
            spline_type: 3,
            flags: 0,
            max_control_depth_ratio: 0.0,
            total_spline_length: 0.0,
            last_knot: 0,
            curr_dist: -1.0,
            next_search_dist: -1.0,
            search_scan: -1,
            degree: 4,
            depth: 16,
            control_verts: crate::container::EngineArray::with_size(0, -1),
            knots: crate::container::EngineArray::with_size(0, -1),
            weights: crate::container::EngineArray::with_size(0, -1),
            spline_knots: crate::container::EngineArray::with_size(0, -1),
            spline_verts: crate::container::EngineArray::with_size(0, -1),
            spline_normals: crate::container::EngineArray::with_size(0, -1),
        }
    }

    /// Retail `clear`: logical lengths and scalar walk state reset; array capacities and
    /// growth hints survive recycler reuse.
    pub fn clear(&mut self) {
        self.degree = 4;
        self.depth = 16;
        self.spline_type = 3;
        self.max_control_depth_ratio = 0.0;
        self.flags = 0;
        self.total_spline_length = 0.0;
        self.last_knot = 0;
        self.curr_dist = -1.0;
        self.next_search_dist = -1.0;
        self.search_scan = -1;
        self.control_verts.clear();
        self.knots.clear();
        self.weights.clear();
        self.spline_knots.clear();
        self.spline_verts.clear();
        self.spline_normals.clear();
    }

    fn set_min_seg_length(&mut self, min_segment_length: f32) {
        let mut total = 0.0_f32;
        for pair in self.control_verts.as_slice().windows(2) {
            total = pair[1].sub(pair[0]).length() + total;
        }
        self.depth = ((total / min_segment_length) as i32) as u16;
    }

    fn generate_knots(&mut self) {
        let control_len = self.control_verts.len() as i32;
        let mut degree = self.degree as i32;
        let mut last = degree + control_len;
        while control_len < degree {
            degree -= 1;
            last -= 1;
        }
        self.degree = degree as u16;
        self.spline_knots.clear();
        let mut value = 0.0_f32;
        let mut weight_index = 0usize;
        for i in 0..=last {
            let knot = if i <= degree {
                value
            } else if i <= control_len {
                if self.knots.is_empty() {
                    value += 1.0;
                } else {
                    for weight in
                        &self.knots.as_slice()[weight_index..weight_index + degree as usize]
                    {
                        value += *weight;
                    }
                    weight_index += 1;
                }
                value
            } else if weight_index == 0 && !self.knots.is_empty() {
                self.knots[0]
            } else {
                value
            };
            self.spline_knots.add(knot);
        }
    }

    /// Cox–de Boor basis, instruction-for-instruction arithmetic order from `0x009116E0`.
    fn basis(&self, t: f32, index: usize, degree: usize) -> f32 {
        if index + degree >= self.spline_knots.len() {
            return 0.0;
        }
        if degree == 0 {
            return if self.spline_knots[index] <= t && t < self.spline_knots[index + 1] {
                1.0
            } else {
                0.0
            };
        }

        let mut out = 0.0_f32;
        let left_num = t - self.spline_knots[index];
        let left_den = self.spline_knots[index + degree] - self.spline_knots[index];
        if left_num > 0.0 && left_den > 0.0 {
            let child = self.basis(t, index, degree - 1);
            out = child * (left_num / left_den);
        }
        let right_num = self.spline_knots[index + degree + 1] - t;
        let right_den = self.spline_knots[index + degree + 1] - self.spline_knots[index + 1];
        if right_num > 0.0 && right_den > 0.0 {
            let child = self.basis(t, index + 1, degree - 1);
            out += child * (right_num / right_den);
        }
        out
    }

    /// `Spline::generate_bspline` `0x00911820`.
    fn generate_bspline(&mut self) {
        if self.flags & 0x80 == 0 {
            self.generate_knots();
        }
        let last_index = self.spline_knots.len() - 1;
        let first_knot = self.spline_knots[0];
        let last_knot = self.spline_knots[last_index];
        let mut delta = (last_knot - first_knot) / self.depth as f32;
        if delta == 0.0 {
            delta = 1.0;
        }
        let mut segment = 0usize;
        let mut t = first_knot + delta;
        self.spline_verts.add(self.control_verts[0]);
        while t < last_knot {
            while segment + 1 < self.spline_knots.len() {
                let next = self.spline_knots[segment + 1];
                if t <= next && self.spline_knots[segment] < next {
                    break;
                }
                segment += 1;
            }
            if last_index < segment {
                break;
            }
            let first_control = segment.saturating_sub(self.degree as usize);
            let mut vertex = SplineVec3::default();
            for control in first_control..=segment {
                let weight = self.basis(t, control, self.degree as usize);
                vertex.x += self.control_verts[control].x * weight;
                vertex.y += self.control_verts[control].y * weight;
                vertex.z += self.control_verts[control].z * weight;
            }
            self.spline_verts.add(vertex);
            t += delta;
        }
        if last_knot - (t - delta) > 0.001 {
            self.spline_verts
                .add(self.control_verts[self.control_verts.len() - 1]);
        }
        if self.spline_verts.is_empty() {
            let controls = self.control_verts.as_slice().to_vec();
            for vertex in controls {
                self.spline_verts.add(vertex);
            }
        }
    }

    /// `Spline::build_normals` `0x00911F60`. Cruise splines use flag `0x10`, making the
    /// per-segment vector the raw tangent; the default-axis arm is retained for nuke paths.
    fn build_normals(&mut self) {
        let mut prior = SplineVec3::default();
        let mut previous = self.spline_verts[0];
        self.total_spline_length = 0.0;
        self.spline_normals.clear();
        for current in &self.spline_verts.as_slice()[1..] {
            let direction = current.sub(previous);
            self.total_spline_length = direction.length() + self.total_spline_length;
            let normal = if self.flags & 0x10 != 0 {
                direction
            } else {
                let axis = if self.flags & 0x20 != 0 {
                    SplineVec3::new(0.0, 0.0, 1.0)
                } else {
                    SplineVec3::new(1.0, 1.0, 1.732_050_8)
                };
                SplineVec3::new(
                    direction.y * axis.z - direction.z * axis.y,
                    direction.z * axis.x - direction.x * axis.z,
                    direction.x * axis.y - direction.y * axis.x,
                )
                .normalized()
            };
            self.spline_normals.add(prior.add(normal).normalized());
            prior = normal;
            previous = *current;
        }

        let mut last = prior;
        if self.flags & 4 != 0 {
            if let Some(first) = self.spline_normals.get_mut(0) {
                *first = first.add(last).normalized();
                last = *first;
            }
        } else if self.flags & 0x10 != 0 && self.control_verts.len() >= 2 {
            let n = self.control_verts.len();
            last = self.control_verts[n - 1]
                .sub(self.control_verts[n - 2])
                .normalized();
        }
        self.spline_normals.add(last);
    }

    fn calc_spline(&mut self) {
        let saved_degree = self.degree;
        let saved_depth = self.depth;
        let len = self.control_verts.len() as i32;
        if len <= self.degree as i32 {
            self.degree = (len - 1) as u16;
        }
        let ratio_depth = (len as f32 * self.max_control_depth_ratio) as i32;
        if ratio_depth != 0 && ratio_depth < self.depth as i32 {
            self.depth = ratio_depth as u16;
        }
        if self.depth < self.degree {
            self.depth = self.degree;
        }
        self.spline_verts.clear();
        self.generate_bspline();
        if !self.spline_verts.is_empty() && self.flags & 0x40 == 0 {
            self.build_normals();
        }
        self.depth = saved_depth;
        self.degree = saved_degree;
    }

    fn calc_from_dir(
        &mut self,
        min_segment_length: f32,
        start: SplineVec3,
        control: SplineVec3,
        end: SplineVec3,
        optional_control: SplineVec3,
    ) {
        self.knots.clear();
        self.control_verts.clear();
        self.control_verts.add(start);
        self.control_verts.add(control);
        if optional_control.length() == 0.0 {
            self.degree = 2;
        } else {
            self.control_verts.add(optional_control);
            self.degree = 3;
        }
        self.control_verts.add(end);
        self.set_min_seg_length(min_segment_length);
        self.max_control_depth_ratio = 4.0;
        self.calc_spline();
    }

    /// `SplineData::walk_data` appended to the ammo checksum. This mutates nested array flags
    /// exactly as the writer does (`flags &= ~0x40`) before hashing them.
    pub fn walk_checksum(&mut self, mut adler: u32) -> u32 {
        macro_rules! scalar {
            ($value:expr) => {
                adler = adler32(adler, &$value.to_le_bytes())
            };
        }
        scalar!(self.spline_type);
        scalar!(self.flags);
        scalar!(self.max_control_depth_ratio.to_bits());
        scalar!(self.total_spline_length.to_bits());
        scalar!(self.last_knot);
        scalar!(self.curr_dist.to_bits());
        scalar!(self.next_search_dist.to_bits());
        scalar!(self.search_scan);
        scalar!(self.degree);
        scalar!(self.depth);
        adler = walk_vec3_array(adler, &mut self.control_verts);
        adler = walk_float_array(adler, &mut self.knots);
        adler = walk_float_array(adler, &mut self.weights);
        adler = walk_float_array(adler, &mut self.spline_knots);
        adler = walk_vec3_array(adler, &mut self.spline_verts);
        walk_vec3_array(adler, &mut self.spline_normals)
    }
}

fn walk_array_header<T: Clone + Default>(
    mut adler: u32,
    array: &mut crate::container::EngineArray<T>,
) -> u32 {
    adler = adler32(adler, &(array.len() as i32).to_le_bytes());
    if array.is_empty() {
        return adler;
    }
    adler = adler32(adler, &array.size().to_le_bytes());
    adler = adler32(adler, &array.increment().to_le_bytes());
    let flags = array.flags() & !0x40;
    array.set_flags(flags);
    adler32(adler, &[flags])
}

fn walk_float_array(mut adler: u32, array: &mut crate::container::EngineArray<f32>) -> u32 {
    adler = walk_array_header(adler, array);
    for value in array.as_slice() {
        adler = adler32(adler, &value.to_bits().to_le_bytes());
    }
    adler
}

fn walk_vec3_array(mut adler: u32, array: &mut crate::container::EngineArray<SplineVec3>) -> u32 {
    adler = walk_array_header(adler, array);
    for value in array.as_slice() {
        adler = adler32(adler, &value.x.to_bits().to_le_bytes());
        adler = adler32(adler, &value.y.to_bits().to_le_bytes());
        adler = adler32(adler, &value.z.to_bits().to_le_bytes());
    }
    adler
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplineBuildError {
    NonFiniteInput,
    NonPositiveSegmentLength,
    MissingTerrain { x: i32, y: i32 },
}

/// Terrain facts consumed by the terrain-following arm of `Spline::calc_nuke_spline`
/// (`0x00913AD0`). `z` is the queried terrain height and `flags` is the corresponding
/// `TerrainMapData` cell's `u16` flags word.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NukeTerrainSample {
    pub z: i32,
    pub flags: u16,
}

/// Host facts required by terrain-following nuke construction. A missing sample fails the
/// transaction closed before the caller's [`Ammo`] is mutated.
pub trait NukeSplineEnv {
    fn nuke_terrain(&self, x: i32, y: i32) -> Option<NukeTerrainSample>;
}

/// The two instruction-level gates selecting `Ammo::init`'s spline constructor.
///
/// The graphic-piece table's flag `8` has priority and selects `calc_from_dir` (cruise).
/// Only when that flag is clear does shooter `ObjectType::obj_masks & 0x08000000` select
/// `calc_nuke_spline`; otherwise the launch is the ordinary arc path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetailSplineFamily {
    Arc,
    Cruise,
    Nuke,
}

/// Dynamic `Guy`/`UnitData` pose facts read only by the graphic-piece spline arm of
/// `Ammo::init` (`0x0067CBA5..0x0067CDCF`).  They do not belong to the ammo type record:
/// non-land shooters take angle/pitch from their lead `Guy`, while the nuke-mask override
/// uses `UnitData::speed()` as the spline's minimum segment length.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CruiseLaunchPose {
    /// `GuyData +0x18`, used in place of the target-facing ammo angle for non-land units.
    pub lead_angle: i32,
    /// `GuyData +0x4C`, in degrees, truncated before the half-degree table lookup.
    pub lead_pitch: f32,
    /// `UnitData::speed()` `0x0060AAE0`.
    pub speed: i32,
}

/// The four vectors and scalar passed to `Spline::calc_from_dir` at `0x0067CDCF`, plus
/// the angle already written to `AmmoData +0x2C` by the same constructor arm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CruiseSplineInputs {
    pub min_segment_length: f32,
    pub start: SplineVec3,
    pub control: SplineVec3,
    pub end: SplineVec3,
    pub optional_control: SplineVec3,
    pub ammo_angle: i32,
}

#[inline]
pub fn select_retail_spline_family(
    graphic_piece_spline: bool,
    shooter_obj_masks: u32,
) -> RetailSplineFamily {
    if graphic_piece_spline {
        RetailSplineFamily::Cruise
    } else if shooter_obj_masks & 0x0800_0000 != 0 {
        RetailSplineFamily::Nuke
    } else {
        RetailSplineFamily::Arc
    }
}

#[inline]
fn normalize_half_degree_index(mut degrees: i32) -> i32 {
    // The shipped normalization is not `rem_euclid`: its negative arm is
    // `359 - ((-degrees) % 360)`, making -1 map to 358 rather than 359.
    if degrees < 0 {
        degrees = degrees.wrapping_neg();
        359i32.wrapping_sub(degrees % 360)
    } else if degrees >= 360 {
        degrees % 360
    } else {
        degrees
    }
}

#[inline]
fn half_degree_quaternion_pair(degrees: i32) -> (f32, f32) {
    // `fast_half_degree_to_{cosine,sine}` lazily build 360 entries in precisely this
    // multiply order (`0x00A29080`, `0x00A29180`) before returning the indexed f32.
    let index = normalize_half_degree_index(degrees) as f32;
    let radians = (index * 0.5) * f32::from_bits(0x3C8E_FA35);
    (radians.cos(), radians.sin())
}

#[inline]
fn rotate_retail(vector: &mut SplineVec3, qx: f32, qy: f32, qz: f32, qw: f32) {
    // `Vector<float>::rotate(Quat<float> const*)` `0x00420B10`, retaining its scalar
    // operand grouping because the resulting path is walked as raw f32 bits.
    let xx = vector.x;
    let yy = vector.y;
    let zz = vector.z;
    let yz2 = qz * qz + qy * qy;
    let x_cross = (qz * qx - qw * qy) * zz + (qw * qz + qy * qx) * yy;
    let zx2 = qz * qz + qx * qx;
    let y_cross = (qw * qx + qz * qy) * zz + (qy * qx - qw * qz) * xx;
    vector.x = (1.0 - (yz2 + yz2)) * xx + x_cross + x_cross;
    let yx2 = qy * qy + qx * qx;
    vector.y = (1.0 - (zx2 + zx2)) * yy + y_cross + y_cross;
    let z_cross = (qz * qy - qw * qx) * yy + (qw * qy + qz * qx) * xx;
    vector.z = (1.0 - (yx2 + yx2)) * zz + z_cross + z_cross;
}

/// Recover `Ammo::init`'s four `calc_from_dir` vectors from the initialized ammo and the
/// live shooter pose.  No RNG is consumed.  The optional fourth control is constructed as
/// the all-zero vector; `UnitData::speed()` is called only after those four future call
/// arguments are pushed, which is why the decompiler incorrectly rendered them as speed
/// arguments even though the PDB signature is `int UnitData::speed() const`.
pub fn cruise_spline_inputs(
    ammo: &AmmoWalk,
    shooter: &ObjView,
    attack_dist: i32,
    pose: CruiseLaunchPose,
) -> Result<CruiseSplineInputs, SplineBuildError> {
    if shooter.rules.domain != 0 && !pose.lead_pitch.is_finite() {
        return Err(SplineBuildError::NonFiniteInput);
    }
    let nuke_mask = shooter.rules.obj_masks & 0x0800_0000 != 0;
    let (reach, pitch, ammo_angle) = if shooter.rules.domain == 0 {
        // `0x0067C459..0x0067C469`: the distance is halved in EAX for reach, while
        // XMM1 is loaded independently from retail's 45.0f constant before jumping
        // past the non-land lead-Guy load at `0x0067CBA5`.
        (attack_dist / 2, 45.0, ammo.angle)
    } else {
        (
            if shooter.is_unit && !nuke_mask {
                shooter
                    .rules
                    .proj_speed
                    .wrapping_mul(UNIT_MOVE_SPEED)
                    .wrapping_mul(5)
            } else {
                0x480
            },
            pose.lead_pitch,
            pose.lead_angle,
        )
    };
    let min_segment = if nuke_mask {
        pose.speed
    } else if shooter.is_unit {
        shooter.rules.proj_speed.wrapping_mul(UNIT_MOVE_SPEED)
    } else {
        150i32.wrapping_mul(UNIT_MOVE_SPEED)
    };
    if min_segment <= 0 {
        return Err(SplineBuildError::NonPositiveSegmentLength);
    }

    let mut control = SplineVec3::new(0.0, -(reach as f32), 0.0);
    let pitch = cvttss2si(pitch);
    // Retail's negative normalization executes `neg` followed by signed `idiv`; the
    // architectural indefinite value would overflow that pair. A host adapter must never
    // index a fabricated table entry when a corrupt/non-representable Guy pitch reaches it.
    if pitch == i32::MIN {
        return Err(SplineBuildError::NonFiniteInput);
    }
    let (pitch_cos, pitch_sin) = half_degree_quaternion_pair(pitch);
    rotate_retail(&mut control, pitch_sin, 0.0, 0.0, pitch_cos);

    let yaw_degrees = cvttss2si(super::unit_inctime::fast_angle_to_degrees(ammo_angle));
    let (yaw_cos, yaw_sin) = half_degree_quaternion_pair(yaw_degrees.wrapping_neg());
    rotate_retail(&mut control, 0.0, 0.0, yaw_sin, yaw_cos);

    let start = SplineVec3::new(ammo.sx as f32, ammo.sy as f32, ammo.sz as f32);
    control.x += start.x;
    control.y += start.y;
    control.z += start.z;
    Ok(CruiseSplineInputs {
        min_segment_length: min_segment as f32,
        start,
        control,
        end: SplineVec3::new(ammo.ex as f32, ammo.ey as f32, ammo.ez as f32),
        optional_control: SplineVec3::default(),
        ammo_angle,
    })
}

/// Reproduce the explicit `ArrayBase::make_valid`/length write used by the nuke arms while
/// retaining recycler-surviving capacity. This is distinct from repeated `add`: fresh fixed
/// nuke knots grow directly to 17 rather than following 4→8→16→32.
fn make_array_length<T: Clone + Default>(
    array: &mut crate::container::EngineArray<T>,
    desired: usize,
) {
    let desired = desired as i32;
    let capacity = array.size();
    if capacity < desired {
        let deficit = desired - capacity;
        let increment = array.increment();
        let growth = if increment < 0 {
            if deficit > capacity {
                deficit
            } else {
                capacity
            }
        } else if deficit > increment as i32 {
            deficit
        } else {
            increment as i32
        };
        array.increase_size(growth as i16);
    }
    while array.len() < desired as usize {
        array.add(T::default());
    }
}

/// Complete cruise-path transaction: recycler-clear, retail flag `0x10`,
/// `calc_from_dir`→`calc_spline`→B-spline generation→normals, then the Ammo pointer/trajectory
/// fields and `total_time = spline_verts.length` write from `Ammo::init`.
pub fn ammo_init_cruise_spline(
    ammo: &mut Ammo,
    min_segment_length: f32,
    start: SplineVec3,
    control: SplineVec3,
    end: SplineVec3,
    optional_control: SplineVec3,
) -> Result<RetailSpline, SplineBuildError> {
    let mut spline = RetailSpline::new();
    ammo_init_cruise_spline_into(
        ammo,
        &mut spline,
        min_segment_length,
        start,
        control,
        end,
        optional_control,
    )?;
    Ok(spline)
}

fn ammo_init_cruise_spline_into(
    ammo: &mut Ammo,
    spline: &mut RetailSpline,
    min_segment_length: f32,
    start: SplineVec3,
    control: SplineVec3,
    end: SplineVec3,
    optional_control: SplineVec3,
) -> Result<(), SplineBuildError> {
    if !min_segment_length.is_finite()
        || !start.finite()
        || !control.finite()
        || !end.finite()
        || !optional_control.finite()
    {
        return Err(SplineBuildError::NonFiniteInput);
    }
    if min_segment_length <= 0.0 {
        return Err(SplineBuildError::NonPositiveSegmentLength);
    }
    spline.clear();
    spline.flags = 0x10;
    spline.calc_from_dir(min_segment_length, start, control, end, optional_control);
    ammo.w.traj = TRAJ_SPLINE;
    ammo.w.total_time = spline.spline_verts.len() as u32;
    ammo.has_spline = true;
    Ok(())
}

/// The self-contained (`terrain_path == 0`) arm of `Spline::calc_nuke_spline`
/// (`0x00913AD0`). It installs retail's 12-point high-altitude arc, degree 3/depth 120,
/// and the 17-entry custom knot-width array (`100,30,15,10...`) before entering the same
/// B-spline/normal/checksum chain as cruise missiles.
pub fn ammo_init_nuke_spline_high_arc(
    ammo: &mut Ammo,
    start: SplineVec3,
    end: SplineVec3,
) -> Result<RetailSpline, SplineBuildError> {
    let mut spline = RetailSpline::new();
    ammo_init_nuke_spline_high_arc_into(ammo, &mut spline, start, end)?;
    Ok(spline)
}

fn ammo_init_nuke_spline_high_arc_into(
    ammo: &mut Ammo,
    spline: &mut RetailSpline,
    start: SplineVec3,
    end: SplineVec3,
) -> Result<(), SplineBuildError> {
    if !start.finite() || !end.finite() {
        return Err(SplineBuildError::NonFiniteInput);
    }

    spline.clear();
    spline.flags = 0x10;
    let quarter = SplineVec3::new(
        (start.x * 3.0 + end.x) * 0.25,
        (start.y * 3.0 + end.y) * 0.25,
        (start.z * 3.0 + end.z) * 0.25 + 20_000.0,
    );
    let middle = SplineVec3::new(
        (start.x + end.x) * 0.5,
        (start.y + end.y) * 0.5,
        (start.z + end.z) * 0.5 + 20_000.0,
    );
    let three_quarters = SplineVec3::new(
        (end.x * 3.0 + start.x) * 0.25,
        (end.y * 3.0 + start.y) * 0.25,
        (end.z * 3.0 + start.z) * 0.25 + 10_000.0,
    );
    for point in [
        start,
        SplineVec3::new(start.x, start.y, start.z + 250.0),
        SplineVec3::new(start.x, start.y, start.z + 750.0),
        SplineVec3::new(start.x, start.y, start.z + 2_250.0),
        SplineVec3::new(start.x, start.y, start.z + 6_750.0),
        quarter,
        middle,
        three_quarters,
        SplineVec3::new(end.x, end.y, end.z + 6_000.0),
        SplineVec3::new(end.x, end.y, end.z + 4_000.0),
        SplineVec3::new(end.x, end.y, end.z + 2_000.0),
        end,
    ] {
        spline.control_verts.add(point);
    }
    spline.spline_type = 3;
    spline.max_control_depth_ratio = 0.0;
    spline.degree = 3;
    spline.depth = 120;

    // make_valid(control_len + 4) treats the argument as an index: len becomes 17 and the
    // negative increment asks increase_size for the exact 17-slot deficit, not doubling.
    make_array_length(&mut spline.knots, 17);
    for (i, knot) in spline.knots.as_mut_slice().iter_mut().enumerate() {
        *knot = match i {
            0 => 100.0,
            1 => 30.0,
            2 => 15.0,
            _ => 10.0,
        };
    }
    spline.calc_spline();
    ammo.w.traj = TRAJ_SPLINE;
    ammo.w.total_time = (spline.spline_verts.len() as u32).saturating_sub(1);
    ammo.has_spline = true;
    Ok(())
}

/// The terrain-following (`terrain_path != 0`) arm of `Spline::calc_nuke_spline`
/// (`0x00913AD0`). Retail truncates both endpoints before computing
/// `vector_dist(start,end) / 768`, but interpolates each interior control point from the
/// original f32 endpoints and truncates the result only for the terrain query/stored point.
///
/// Every interior point begins at `terrain_z + 250`. A cell with flag bit 14 and low bits
/// `3` instead receives `+1250`; otherwise a cell whose `0x30` bits are both set receives
/// `+750`. Construction then uses degree 3, `set_min_seg_length(96.0)`, an exact-length
/// all-30 knot-width array, and the common B-spline/normal/checksum chain. No RNG is consumed.
pub fn ammo_init_nuke_spline_terrain<E: NukeSplineEnv>(
    ammo: &mut Ammo,
    env: &E,
    start: SplineVec3,
    end: SplineVec3,
) -> Result<RetailSpline, SplineBuildError> {
    let mut spline = RetailSpline::new();
    ammo_init_nuke_spline_terrain_into(ammo, &mut spline, env, start, end)?;
    Ok(spline)
}

fn ammo_init_nuke_spline_terrain_into<E: NukeSplineEnv + ?Sized>(
    ammo: &mut Ammo,
    spline: &mut RetailSpline,
    env: &E,
    start: SplineVec3,
    end: SplineVec3,
) -> Result<(), SplineBuildError> {
    if !start.finite() || !end.finite() {
        return Err(SplineBuildError::NonFiniteInput);
    }

    // 0x00913B01..0x00913B54: the four endpoint cvttss2si operations happen before
    // vector_dist, followed by MSVC's signed magic divide by 0x300.
    let dx = cvttss2si(start.x)
        .wrapping_sub(cvttss2si(end.x))
        .wrapping_abs();
    let dy = cvttss2si(start.y)
        .wrapping_sub(cvttss2si(end.y))
        .wrapping_abs();
    let divisions = vector_dist(dx, dy) / 0x300;

    spline.clear();
    spline.flags = 0x10;
    spline.control_verts.add(start);
    spline
        .control_verts
        .add(SplineVec3::new(start.x, start.y, start.z + 250.0));

    if divisions > 1 {
        let denominator = divisions as f32;
        for i in 1..divisions {
            // Preserve retail's scalar-SSE order: i*end + (n-i)*start, then divide.
            let fi = i as f32;
            let remaining = (divisions - i) as f32;
            let sample_y = (fi * end.y + remaining * start.y) / denominator;
            let sample_x = (fi * end.x + remaining * start.x) / denominator;
            let x = cvttss2si(sample_x);
            let y = cvttss2si(sample_y);
            let terrain = env
                .nuke_terrain(x, y)
                .ok_or(SplineBuildError::MissingTerrain { x, y })?;
            let lift = if terrain.flags & 0x4000 != 0 && terrain.flags & 3 == 3 {
                1_250
            } else if terrain.flags & 0x30 == 0x30 {
                750
            } else {
                250
            };
            spline.control_verts.add(SplineVec3::new(
                x as f32,
                y as f32,
                terrain.z.wrapping_add(lift) as f32,
            ));
        }
    }

    spline
        .control_verts
        .add(SplineVec3::new(end.x, end.y, end.z + 250.0));
    spline.control_verts.add(end);
    spline.spline_type = 3;
    spline.max_control_depth_ratio = 0.0;
    spline.degree = 3;
    spline.depth = 120;
    spline.set_min_seg_length(96.0);

    // 0x00913E1D..0x00913E78 grows capacity by exactly the deficit when the constructed
    // array is empty, then sets length to max(desired, old length). A fresh path therefore
    // has identical length/capacity `control_len + 2 + degree`.
    let knot_count = spline.control_verts.len() + 2 + spline.degree as usize;
    make_array_length(&mut spline.knots, knot_count);
    for knot in spline.knots.as_mut_slice() {
        *knot = 30.0;
    }
    spline.calc_spline();
    ammo.w.traj = TRAJ_SPLINE;
    ammo.w.total_time = (spline.spline_verts.len() as u32).saturating_sub(1);
    ammo.has_spline = true;
    Ok(())
}

/// The isolated spline arm of `Ammo::inc_time` after `cur_time` has already incremented.
/// Retail reads `spline_verts[cur_time]` only while `length > cur_time + 1` and truncates all
/// three f32 coordinates with `cvttss2si`.
pub fn ammo_step_cruise_spline(ammo: &mut AmmoWalk, spline: &RetailSpline) -> Option<SplineVec3> {
    if spline.spline_verts.len() as u32 <= ammo.cur_time.wrapping_add(1) {
        return None;
    }
    let point = spline.spline_verts[ammo.cur_time as usize];
    ammo.ex = cvttss2si(point.x);
    ammo.ey = cvttss2si(point.y);
    ammo.ez = cvttss2si(point.z);
    Some(point)
}

/// Result of the target-aware spline arm inside `Ammo::inc_time`
/// (`0x0067D3E6..0x0067D8C4`). The caller has already performed the common `cur_time++`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CruiseTargetStep {
    /// Target identity/liveness or the `length > cur_time + 1` sample gate failed.
    Unavailable,
    /// The sampled point remains outside the target envelope and this is not a four-frame
    /// unit retarget. Retail leaves the ammo endpoint and spline completely unchanged.
    Checked { sample: [i32; 3] },
    /// The sampled point entered the target envelope. Retail writes it as the endpoint,
    /// sets `total_time = cur_time - 1`, and jumps back to the top of `Ammo::inc_time` so
    /// the common increment/arrival path runs immediately in the same call.
    SnappedForImmediateLoop { sample: [i32; 3] },
    /// A live unit target was outside the envelope on a non-zero multiple-of-four frame.
    /// `start` is the truncated current sample; `control` mirrors the next raw vertex about
    /// it; `end` is the target's current position/lead-Guy height.
    Rebuilt {
        start: SplineVec3,
        control: SplineVec3,
        end: SplineVec3,
        vertex_count: u32,
    },
}

/// Execute retail's dynamic target-envelope and retarget/rebuild spline transaction after
/// the common `cur_time++` boundary. No RNG is consumed.
///
/// A proximity snap deliberately does not apply the subsequent common increment itself:
/// [`CruiseTargetStep::SnappedForImmediateLoop`] represents the shipped backward jump to
/// `0x0067D392`, whose shooter-side bookkeeping belongs to the live `Ammo::inc_time` adapter.
/// The endpoint/`total_time` state at that jump is nevertheless committed exactly here.
pub fn ammo_step_cruise_targeted(
    ammo: &mut AmmoWalk,
    spline: &mut RetailSpline,
    target: Option<&ObjView>,
) -> CruiseTargetStep {
    if ammo.ox < 0 || ammo.whom < 0 {
        return CruiseTargetStep::Unavailable;
    }
    let Some(target) = target.filter(|target| target.alive) else {
        return CruiseTargetStep::Unavailable;
    };
    let Ok(index) = usize::try_from(ammo.cur_time) else {
        return CruiseTargetStep::Unavailable;
    };
    let Some(next_index) = index.checked_add(1) else {
        return CruiseTargetStep::Unavailable;
    };
    if spline.spline_verts.len() <= next_index {
        return CruiseTargetStep::Unavailable;
    }

    let raw = spline.spline_verts[index];
    let sample = [cvttss2si(raw.x), cvttss2si(raw.y), cvttss2si(raw.z)];
    let target_z = if target.is_unit {
        target.guy0_z
    } else {
        target.z
    };
    let target_radius = if target.is_unit {
        target.rules.target_size
    } else {
        0xC0
    };
    let dx = sample[0].wrapping_sub(target.x).wrapping_abs();
    let dy = sample[1].wrapping_sub(target.y).wrapping_abs();
    let dz = sample[2].wrapping_sub(target_z).wrapping_abs();
    if vector_dist(dx, dy) < target_radius && dz < 0xC0 {
        ammo.ex = sample[0];
        ammo.ey = sample[1];
        ammo.ez = sample[2];
        ammo.total_time = ammo.cur_time.wrapping_sub(1);
        return CruiseTargetStep::SnappedForImmediateLoop { sample };
    }

    if ammo.cur_time == 0 || ammo.cur_time & 3 != 0 || !target.is_unit {
        return CruiseTargetStep::Checked { sample };
    }

    // `0x0067D76A..0x0067D8AA`: convert the already-truncated current sample back to
    // floats, mirror the following raw spline vertex around it, then rebuild toward the
    // live target with a zero optional control and `200 * unit_move_speed` minimum.
    let start = SplineVec3::new(sample[0] as f32, sample[1] as f32, sample[2] as f32);
    let next = spline.spline_verts[next_index];
    let control = SplineVec3::new(
        (next.x - start.x) * 2.0 + start.x,
        (next.y - start.y) * 2.0 + start.y,
        (next.z - start.z) * 2.0 + start.z,
    );
    let end = SplineVec3::new(target.x as f32, target.y as f32, target_z as f32);

    // These endpoint writes precede `Spline::calc_from_dir` in the instruction stream.
    ammo.ex = target.x;
    ammo.ey = target.y;
    ammo.ez = target_z;
    spline.calc_from_dir(
        (200i32.wrapping_mul(UNIT_MOVE_SPEED)) as f32,
        start,
        control,
        end,
        SplineVec3::default(),
    );
    let vertex_count = spline.spline_verts.len() as u32;
    ammo.total_time = vertex_count;
    ammo.cur_time = 0;
    CruiseTargetStep::Rebuilt {
        start,
        control,
        end,
        vertex_count,
    }
}

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
    /// byte for this. Geometry lives in the slot-aligned [`RetailSpline`] sidecar consumed by
    /// [`AmmoPool::checksum_with_splines`].
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
    /// Slot-aligned stand-in for `AmmoData::ammo_path`. A live slot with `has_spline` must
    /// have `Some` at the same index; free/arc slots have `None`.
    pub spline_slots: Vec<Option<RetailSpline>>,
    /// Retail `Recycler<Spline>::temp_pool`: closed paths are cleared and pushed here, and
    /// the next spline launch pops from the end. Array capacities survive that LIFO reuse.
    spline_recycler: Vec<RetailSpline>,
    /// Ammo graphic ids whose graphic-table record carries flag `8`, the first spline
    /// selector gate in `Ammo::init`. Configuration, not walked simulation state.
    spline_graphics: Vec<i32>,
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
            spline_slots: vec![None; AMMO_POOL_SLOTS],
            spline_recycler: Vec::new(),
            spline_graphics: Vec::new(),
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
            self.spline_slots.push(None);
        } else {
            // Direct callers may have run Ammo::close before returning through the pool.
            // Retail close returns the pointer to Recycler<Spline> immediately; catch up
            // before this lowest free slot is reused.
            self.recycle_slot_spline(i);
        }
        i
    }

    fn acquire_spline(&mut self) -> RetailSpline {
        let mut spline = self.spline_recycler.pop().unwrap_or_default();
        spline.clear();
        spline
    }

    fn recycle_slot_spline(&mut self, slot: usize) {
        if let Some(mut spline) = self.spline_slots.get_mut(slot).and_then(Option::take) {
            spline.clear();
            self.spline_recycler.push(spline);
        }
        if let Some(ammo) = self.slots.get_mut(slot) {
            ammo.has_spline = false;
        }
    }

    /// Install a cruise path into an already allocated/initialized ammo slot, consuming a
    /// LIFO-recycled `RetailSpline` exactly as `Recycler<Spline>::pop` does.
    pub fn install_cruise_spline(
        &mut self,
        slot: usize,
        min_segment_length: f32,
        start: SplineVec3,
        control: SplineVec3,
        end: SplineVec3,
        optional_control: SplineVec3,
    ) -> Result<(), SplineBuildError> {
        self.recycle_slot_spline(slot);
        let mut spline = self.acquire_spline();
        let result = ammo_init_cruise_spline_into(
            &mut self.slots[slot],
            &mut spline,
            min_segment_length,
            start,
            control,
            end,
            optional_control,
        );
        if let Err(error) = result {
            spline.clear();
            self.spline_recycler.push(spline);
            return Err(error);
        }
        arc_ballistics(&mut self.slots[slot].w);
        self.spline_slots[slot] = Some(spline);
        Ok(())
    }

    /// Compose the recovered `Ammo::init` orientation transaction with the pool-owned
    /// constructor. Input derivation is mutation-free; `ammo_angle` is committed only after
    /// the spline itself succeeds, so missing/non-positive facts cannot half-install a path.
    pub fn install_cruise_launch(
        &mut self,
        slot: usize,
        shooter: &ObjView,
        attack_dist: i32,
        pose: CruiseLaunchPose,
    ) -> Result<CruiseSplineInputs, SplineBuildError> {
        let inputs = cruise_spline_inputs(&self.slots[slot].w, shooter, attack_dist, pose)?;
        self.install_cruise_spline(
            slot,
            inputs.min_segment_length,
            inputs.start,
            inputs.control,
            inputs.end,
            inputs.optional_control,
        )?;
        self.slots[slot].w.angle = inputs.ammo_angle;
        Ok(inputs)
    }

    /// Install the retail nuke constructor selected by shooter property `0x13A`: zero uses
    /// the fixed high arc, non-zero uses the terrain-following path.
    pub fn install_nuke_spline<E: NukeSplineEnv + ?Sized>(
        &mut self,
        slot: usize,
        env: &E,
        terrain_path: bool,
        start: SplineVec3,
        end: SplineVec3,
    ) -> Result<(), SplineBuildError> {
        self.recycle_slot_spline(slot);
        let mut spline = self.acquire_spline();
        let result = if terrain_path {
            ammo_init_nuke_spline_terrain_into(&mut self.slots[slot], &mut spline, env, start, end)
        } else {
            ammo_init_nuke_spline_high_arc_into(&mut self.slots[slot], &mut spline, start, end)
        };
        if let Err(error) = result {
            spline.clear();
            self.spline_recycler.push(spline);
            return Err(error);
        }
        arc_ballistics(&mut self.slots[slot].w);
        self.spline_slots[slot] = Some(spline);
        Ok(())
    }

    /// Apply the indexed spline sample after `ammo_inc_time` performed its common
    /// `cur_time++`. Missing slot-aligned state is a hard error, never an empty path.
    pub fn step_spline_slot(
        &mut self,
        slot: usize,
    ) -> Result<Option<SplineVec3>, AmmoSplineChecksumError> {
        let ammo = self
            .slots
            .get_mut(slot)
            .ok_or(AmmoSplineChecksumError::MissingSpline { slot })?;
        if !ammo.has_spline {
            return Ok(None);
        }
        let spline = self
            .spline_slots
            .get(slot)
            .and_then(Option::as_ref)
            .ok_or(AmmoSplineChecksumError::MissingSpline { slot })?;
        Ok(ammo_step_cruise_spline(&mut ammo.w, spline))
    }

    /// Apply the exact target-aware dynamic cruise arm to one slot-aligned sidecar. This
    /// API owns every checksummed ammo/spline mutation but deliberately does not fabricate
    /// the live target lookup or the common-loop bookkeeping after a proximity snap.
    pub fn step_cruise_targeted_slot(
        &mut self,
        slot: usize,
        target: Option<&ObjView>,
    ) -> Result<CruiseTargetStep, AmmoSplineChecksumError> {
        let ammo = self
            .slots
            .get_mut(slot)
            .ok_or(AmmoSplineChecksumError::MissingSpline { slot })?;
        if !ammo.has_spline {
            return Ok(CruiseTargetStep::Unavailable);
        }
        let spline = self
            .spline_slots
            .get_mut(slot)
            .and_then(Option::as_mut)
            .ok_or(AmmoSplineChecksumError::MissingSpline { slot })?;
        Ok(ammo_step_cruise_targeted(&mut ammo.w, spline, target))
    }

    /// `Ammo::close` plus `Recycler<Spline>::push`, preserving the recycled object's array
    /// capacities for the next LIFO allocation.
    pub fn close_slot(&mut self, slot: usize) {
        if let Some(ammo) = self.slots.get_mut(slot) {
            ammo.close();
        }
        self.recycle_slot_spline(slot);
    }

    /// Complete a close that occurred inside a projectile primitive such as
    /// `ammo_do_damage_single` or `ammo_inc_time`.
    pub fn recycle_if_closed(&mut self, slot: usize) {
        if self.slots.get(slot).is_some_and(|ammo| !ammo.occupied()) {
            self.recycle_slot_spline(slot);
        }
    }

    pub fn spline(&self, slot: usize) -> Option<&RetailSpline> {
        self.spline_slots.get(slot).and_then(Option::as_ref)
    }

    /// Diagnostic exposure for mutation-sensitive lifecycle tests.
    pub fn recycled_spline_count(&self) -> usize {
        self.spline_recycler.len()
    }

    /// Install the recovered graphic-table flag `8` fact for one ammo graphic id.
    pub fn install_spline_graphic(&mut self, gpiece: i32) {
        if !self.spline_graphics.contains(&gpiece) {
            self.spline_graphics.push(gpiece);
        }
    }

    #[inline]
    pub fn graphic_piece_uses_spline(&self, gpiece: i32) -> bool {
        self.spline_graphics.contains(&gpiece)
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
                                                   // Compatibility channel for arc-only callers. Spline-aware owners must use
                                                   // `checksum_with_splines` so the immediately-following nested walk is included.
        }
        a
    }

    /// The complete ammo channel when live spline sidecars are present.
    ///
    /// Retail walks the non-null byte and immediately enters `SplineData::walk_data` for the
    /// same slot.  A missing sidecar is therefore an error, never an empty-path substitution.
    /// Free slots still contribute nothing even if the sidecar slice contains stale recycler
    /// data at that index.
    pub fn checksum_with_splines(
        &self,
        splines: &mut [Option<RetailSpline>],
    ) -> Result<u32, AmmoSplineChecksumError> {
        let mut a = 1;
        for (slot, ammo) in self.slots.iter().enumerate() {
            if !ammo.occupied() {
                continue;
            }
            let bytes = ammo.w.as_bytes();
            a = adler32(a, &bytes[0..1]);
            a = adler32(a, &bytes[1..100]);
            a = adler32(a, &[ammo.has_spline as u8]);
            if ammo.has_spline {
                let spline = splines
                    .get_mut(slot)
                    .and_then(Option::as_mut)
                    .ok_or(AmmoSplineChecksumError::MissingSpline { slot })?;
                a = spline.walk_checksum(a);
            }
        }
        Ok(a)
    }

    /// The live pool's complete checksum channel, including its owned slot-aligned paths.
    /// The nested walker clears transient array flag `0x40`, matching retail mutation order.
    pub fn checksum_complete(&mut self) -> Result<u32, AmmoSplineChecksumError> {
        let mut a = 1;
        for (slot, ammo) in self.slots.iter().enumerate() {
            if !ammo.occupied() {
                continue;
            }
            let bytes = ammo.w.as_bytes();
            a = adler32(a, &bytes[0..1]);
            a = adler32(a, &bytes[1..100]);
            a = adler32(a, &[ammo.has_spline as u8]);
            if ammo.has_spline {
                let spline = self
                    .spline_slots
                    .get_mut(slot)
                    .and_then(Option::as_mut)
                    .ok_or(AmmoSplineChecksumError::MissingSpline { slot })?;
                a = spline.walk_checksum(a);
            }
        }
        Ok(a)
    }

    /// Read-only facade for reporting APIs. It walks cloned path headers so the digest is
    /// complete without changing an `&self` interface; the live mutable checksum boundary
    /// should prefer [`AmmoPool::checksum_complete`].
    pub fn checksum_complete_readonly(&self) -> Result<u32, AmmoSplineChecksumError> {
        let mut sidecars = self.spline_slots.clone();
        self.checksum_with_splines(&mut sidecars)
    }

    /// Live projectile count — diagnostics only, not part of any channel.
    pub fn live(&self) -> usize {
        self.slots.iter().filter(|s| s.occupied()).count()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmmoSplineChecksumError {
    MissingSpline { slot: usize },
}

// ============================================================================
// Rules and world views
// ============================================================================

/// The `ObjectType` / `UnitType` fields the ammo path reads, with their offsets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShooterRules {
    /// `ObjectType + 0x1E4` — the `A..Z,1..6` mask alphabet. The anti-air gate consumes
    /// the `'2'` missile and `'6'` anti-air bits through [`super::air::AirTypeData`].
    pub obj_masks: u32,
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
    /// Negation of shooter virtual property `get(0x13A, 1)`: retail passes that property
    /// directly as `calc_nuke_spline`'s terrain-path selector. `false` therefore preserves
    /// the retail default (`1`, terrain-following); `true` selects the fixed high arc.
    pub nuke_high_arc: bool,
    /// `ObjectType + 0x234` — footprint half-extent unit, tiles.
    pub x_size: i32,
    /// `ObjectType + 0x238`
    pub y_size: i32,
    /// `ObjectType + 0x250` — high-band hit/evasion percentage used by the anti-air gate.
    pub fly_high: i32,
    /// `ObjectType + 0x254` — low-band hit/evasion percentage used by the anti-air gate.
    pub fly_low: i32,
    /// `UnitType + 0x2B4` — the lower-case unit flag alphabet. Bit `f` exempts
    /// helicopters from the anti-air roll.
    pub unit_flags: u32,
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
    /// `find_angle(ex - sx, ey - sy)`, supplied by the caller because ordinary target,
    /// attack-ground, spline, and offset-muzzle arms choose different source points.
    pub angle: i32,
    /// Lead-Guy pose / live `UnitData::speed()` facts consumed only by the graphic-piece
    /// cruise constructor. Ordinary arcs and nukes ignore this record.
    pub cruise_pose: CruiseLaunchPose,
    /// From the ammo graphic table (`graphic_pieces + 0xF70`, bit `0x80`).
    pub cosmetic: bool,
}

/// Dynamic state read by the anti-air portion of `Ammo::init`.
///
/// The type fields live in [`ShooterRules`]. These two values cannot: the order is the
/// shooter's current order and the target's flight band depends on its live order and
/// position. Callers obtain `target_flying_low` from [`super::air::is_flying_low`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AntiAirLaunch {
    /// `UnitData::order_type()` on the shooter. Only read for unit shooters.
    pub shooter_order: i32,
    /// `UnitData::is_flying_low()` on the target.
    pub target_flying_low: bool,
}

/// Outcome of the complete ordinary targeted launch adapter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AmmoInitOutcome {
    /// `None` when retail returns from `Ammo::init` before constructing a projectile
    /// because the target identity is negative or the target slot is inactive.
    pub ammo: Option<Ammo>,
    /// Exact anti-air arm and RNG draw count. This makes stream-position assertions
    /// mutation-sensitive instead of inferring them from the final dud bit.
    pub anti_air: super::air::AntiAirGate,
}

#[inline]
fn air_type(r: &ShooterRules) -> super::air::AirTypeData {
    super::air::AirTypeData {
        obj_masks: r.obj_masks,
        domain: r.domain,
        fly_high: r.fly_high,
        fly_low: r.fly_low,
        unit_flags: r.unit_flags,
        ..Default::default()
    }
}

/// Run the recovered `Ammo::init` anti-air gate on the ammo subsystem's RNG view.
///
/// [`Rng`] and [`crate::rng::Random`] are the same one-word retail `Random` object. The
/// conversion is deliberately state-only: it lets the already-derived air gate own its
/// intricate short-circuit while preserving one authoritative simulation stream.
#[inline]
pub fn apply_antiair_gate(
    flags: &mut u8,
    shot: &super::air::AntiAirShot<'_>,
    rng: &mut Rng,
) -> super::air::AntiAirGate {
    let mut stream = crate::rng::Random::new(rng.0 as i32);
    let gate = super::air::apply_antiair_gate(flags, shot, &mut stream);
    rng.0 = stream.state() as u32;
    gate
}

/// Complete ordinary targeted `Ammo::init` path, including the anti-air dud gate.
///
/// The dud gate runs before aim scatter, exactly at `0x0067BE49..0x0067C16A`; therefore
/// its zero, one, or two draws change which subsequent values scatter `ex` and `ey`.
/// A dud remains a live, checksummed projectile with [`FLAG_NO_DAMAGE`] set. Invalid or
/// inactive targets return before scatter but after `Objects::ammo_index` advances, matching
/// `Objects::add_ammo` `0x00658B10` (the increment is outside `Ammo::init`).
#[allow(clippy::too_many_arguments)]
pub fn ammo_init_targeted(
    slot: usize,
    ammo_index: &mut i32,
    ord: &LaunchOrder,
    shooter: &ObjView,
    target: Option<&ObjView>,
    anti_air: AntiAirLaunch,
    radius_kind: MissRadius,
    dist_for_accuracy: i32,
    rng: &mut Rng,
) -> AmmoInitOutcome {
    // Objects::add_ammo passes the old counter into Ammo::init and increments it after the
    // call unconditionally, including every early return inside init.
    let graph_index = *ammo_index;
    *ammo_index = ammo_index.wrapping_add(1);

    let shooter_type = air_type(&shooter.rules);
    let target_type = target.map(|t| air_type(&t.rules)).unwrap_or_default();
    let shot = super::air::AntiAirShot {
        whom: ord.whom,
        ox: ord.ox,
        target_active: target.is_some_and(|t| t.alive),
        target_is_unit: target.is_some_and(|t| t.is_unit),
        target_type: &target_type,
        target_flying_low: anti_air.target_flying_low,
        shooter_is_unit: shooter.is_unit,
        shooter_order: anti_air.shooter_order,
        shooter_type: &shooter_type,
    };
    let mut gate_flags = 0;
    let gate = apply_antiair_gate(&mut gate_flags, &shot, rng);
    if gate.init_aborted {
        return AmmoInitOutcome {
            ammo: None,
            anti_air: gate,
        };
    }

    let mut ammo = ammo_init_post_gate(
        slot,
        graph_index,
        ord,
        shooter,
        target,
        radius_kind,
        dist_for_accuracy,
        rng,
    );
    ammo.w.flags |= gate_flags;
    AmmoInitOutcome {
        ammo: Some(ammo),
        anti_air: gate,
    }
}

/// `Ammo::init` (`0x0067BBF0`), the ordinary targeted-arc path.
///
/// Order of operations, which is also the RNG consumption order [measured]:
///
/// 1. `flags &= 0xE3` (clears `OVERSHOOT | MISSED | NO_DAMAGE`, keeps `ALIVE | FLYING`);
/// 2. `who`/`o`/`whom`/`ox` from the package;
/// 3. anti-air dud roll — owned by [`ammo_init_targeted`], which draws 0–2 times before
///    entering this post-gate body;
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
    let graph_index = *ammo_index;
    *ammo_index = ammo_index.wrapping_add(1);
    ammo_init_post_gate(
        slot,
        graph_index,
        ord,
        shooter,
        target,
        radius_kind,
        dist_for_accuracy,
        rng,
    )
}

#[allow(clippy::too_many_arguments)]
fn ammo_init_post_gate(
    slot: usize,
    graph_index: i32,
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
    a.graph_index = graph_index;

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
// Aircraft wreck projectile — `Ammo::init_crash` `0x0067B800`
// ============================================================================

/// PDB `TypeData::cat` value admitted by the aircraft-wreck arm in `Objects::kill_guy`.
pub const CRASH_TYPE_CAT: i32 = 8;
/// `ObjectTypeData::obj_masks` bit that suppresses the wreck even for category 8.
pub const CRASH_SUPPRESS_OBJ_MASK: u32 = 0x0800_0000;

/// Static type-table facts needed to decide and construct one aircraft wreck.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CrashTypeRule {
    /// Global PDB `TypeIndex` used by `GuyData::type` / the owning Unit's `ptype`.
    pub type_index: i32,
    /// PDB `TypeData +0x14 cat`.
    pub cat: i32,
    /// PDB `UnitTypeData +0x2C0 moves`.
    pub moves: i32,
    /// PDB `ObjectTypeData +0x1E4 obj_masks`.
    pub obj_masks: u32,
}

/// Live owning-Unit facts around the dying PDB Guy.
#[derive(Clone, Copy, Debug)]
pub struct CrashOwnerView<'a> {
    pub who: i32,
    pub o: i32,
    pub type_index: i32,
    pub x: i32,
    pub y: i32,
    /// Result of the owning Object's virtual `get_gpiece`; absent means the adapter cannot
    /// reproduce the constructor and must fail closed.
    pub gpiece: Option<i32>,
    /// `UnitData::order_type()`, with `-1` representing an empty order list.
    pub order_type: i32,
    /// `UnitData::guys[0]`; conditionally required by orders 16, 17, and 24.
    pub lead_guy: Option<&'a super::groups_guys::GuyData>,
}

/// Why the live crash adapter could not construct retail input without guessing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrashPlanError {
    MissingDyingGuy,
    MissingDyingTypeRule(i32),
    MissingOwnerTypeRule(i32),
    MissingOwnerGpiece,
    MissingLeadGuy,
    OwnerIdentityMismatch {
        guy_who: i32,
        guy_o: i32,
        owner_who: i32,
        owner_o: i32,
    },
}

/// Pure, fail-closed `Objects::kill_guy` crash gate and live-view flattener.
///
/// `Ok(None)` is a fully known retail rejection (`cat != 8` or the suppression mask).
/// `Err` means a fact required by the admitted arm was unavailable or incoherent. No ammo-pool
/// or RNG reference is accepted here, so every error is transactionally mutation-free.
pub fn plan_ammo_crash(
    dying_guy: Option<&super::groups_guys::GuyData>,
    owner: CrashOwnerView<'_>,
    rules: &[CrashTypeRule],
) -> Result<Option<CrashGuy>, CrashPlanError> {
    let guy = dying_guy.ok_or(CrashPlanError::MissingDyingGuy)?;
    let guy_rule = rules
        .iter()
        .find(|r| r.type_index == guy.ty)
        .ok_or(CrashPlanError::MissingDyingTypeRule(guy.ty))?;
    if guy_rule.cat != CRASH_TYPE_CAT {
        return Ok(None);
    }

    let owner_rule = rules
        .iter()
        .find(|r| r.type_index == owner.type_index)
        .ok_or(CrashPlanError::MissingOwnerTypeRule(owner.type_index))?;
    if owner_rule.obj_masks & CRASH_SUPPRESS_OBJ_MASK != 0 {
        return Ok(None);
    }

    let guy_who = guy.who as i32;
    let guy_o = guy.o as i32;
    if (guy_who, guy_o) != (owner.who, owner.o) {
        return Err(CrashPlanError::OwnerIdentityMismatch {
            guy_who,
            guy_o,
            owner_who: owner.who,
            owner_o: owner.o,
        });
    }
    let gpiece = owner.gpiece.ok_or(CrashPlanError::MissingOwnerGpiece)?;
    let lead_bank = if matches!(owner.order_type, 16 | 17 | 24) {
        owner.lead_guy.ok_or(CrashPlanError::MissingLeadGuy)?.bank
    } else {
        0.0
    };

    Ok(Some(CrashGuy {
        who: guy_who,
        o: guy_o,
        type_index: guy.ty,
        x: guy.x,
        y: guy.y,
        z: guy.z,
        angle: guy.angle,
        shooter_gpiece: gpiece,
        owner_x: owner.x,
        owner_y: owner.y,
        moves: guy_rule.moves,
        order_type: owner.order_type,
        lead_bank,
    }))
}

/// The exact `Guy`/owner/type facts consumed by `Ammo::init_crash`.
///
/// The source pointer is a PDB `Guy*`, not a Unit. Identity and pose come from `GuyData`,
/// while `gpiece`, current order, and the squad leader's bank are reached through the owning
/// `Unit`. They are flattened here so the primitive remains independent of live object storage.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CrashGuy {
    /// `GuyData +0xA1 who`, sign-extended from `char`.
    pub who: i32,
    /// `GuyData +0x8C o`, sign-extended from `short`.
    pub o: i32,
    /// `GuyData +0x08 type`; its `UnitType +0x2C0 moves` is [`CrashGuy::moves`].
    pub type_index: i32,
    /// Plain, unobfuscated `GuyData +0x0C/+0x10/+0x14` coordinates.
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// `GuyData +0x18`, binary angle.
    pub angle: i32,
    /// Owning object's virtual `+0x34` (`get_gpiece`).
    pub shooter_gpiece: i32,
    /// The owning `Unit` object's XOR-decoded `ObjectData +0x10/+0x14` coordinates.
    ///
    /// Retail launches the wreck's extrapolated endpoint from these coordinates, even though
    /// `sx/sy` and the later distance calculation use the individual `Guy` coordinates above.
    pub owner_x: i32,
    pub owner_y: i32,
    /// PDB `UnitType +0x2C0 moves`, before multiplication by `Constants::unit_move_speed`.
    pub moves: i32,
    /// Current `OrderIndex`, or a negative value when the order list is empty.
    pub order_type: i32,
    /// `unit.guys[0].bank` (`GuyData +0x44`). Read only for orders 16, 17, and 24.
    pub lead_bank: f32,
}

/// Terrain/world reads in `Ammo::init_crash`.
pub trait CrashEnv {
    /// `WorldData +0/+4`, measured in four-tile `WCoord` cells.
    fn crash_world_wcells(&self) -> (i32, i32);
    /// `TerrainOut::find_data_z(x, y, 0)` `0x00866560`.
    fn crash_terrain_z(&self, x: i32, y: i32) -> i32;
}

/// SSE `cvttss2si` / `cvttsd2si` invalid conversion result. Rust's float-to-int cast
/// saturates instead, so the exceptional cases need spelling out for instruction fidelity.
#[inline]
fn crash_cvtt_f32_i32(value: f32) -> i32 {
    if value.is_finite() && (-2_147_483_648.0..2_147_483_648.0).contains(&value) {
        value.trunc() as i32
    } else {
        i32::MIN
    }
}

/// `Ammo::init_crash(Guy*, int slot, int graph_index)` `0x0067B800`.
///
/// This mutates an existing pool slot because retail deliberately preserves fields it does not
/// write: notably `accuracy`, flag bits outside `0x1c`, and `ammo_path`. The constructor is not
/// a spline transaction: it writes `traj = TRAJ_ARC`, `v1z = 0`, and never touches
/// `AmmoData::ammo_path`.
///
/// RNG order is asymmetric and checksum-critical:
///
/// 1. `game_random.get(0, 0xffff) % 7 - 3` writes `rolling` (one global draw);
/// 2. a temporary `Random(sx + sy + sz)` supplies `bank_dx` then `bank_dy` from `[-30,30)`.
///
/// The two bank draws therefore do not advance the game stream and are identical for crashes
/// with the same coordinate sum.
pub fn ammo_init_crash<E: CrashEnv + ?Sized>(
    ammo: &mut Ammo,
    guy: &CrashGuy,
    slot: i32,
    graph_index: i32,
    env: &E,
    game_rng: &mut Rng,
) {
    let a = &mut ammo.w;

    a.who = guy.who;
    a.flags &= 0xe3;
    a.index = slot;
    a.graph_index = graph_index;
    a.o = guy.o;
    a.num_guys = 1;
    a.gpiece = guy.shooter_gpiece;
    a.start_roll_angle = if matches!(guy.order_type, 16 | 17 | 24) {
        crash_cvtt_f32_i32(guy.lead_bank).wrapping_neg()
    } else {
        0
    };

    a.sx = guy.x;
    a.sy = guy.y;
    a.sz = guy.z;

    let speed = guy.moves.wrapping_mul(UNIT_MOVE_SPEED);
    let fall_numerator = guy.z.wrapping_neg().wrapping_mul(2);
    let fall_time = ((fall_numerator as f32) / GRAVITY).sqrt();
    let vx = crate::trig::sinx(guy.angle, speed);
    let vy = crate::trig::cosx(guy.angle, speed).wrapping_neg();
    let mut ex = guy
        .owner_x
        .wrapping_add(crash_cvtt_f32_i32((vx as f32) * fall_time));
    let mut ey = guy
        .owner_y
        .wrapping_add(crash_cvtt_f32_i32((vy as f32) * fall_time));

    let (world_xs, world_ys) = env.crash_world_wcells();
    let max_x = world_xs.wrapping_mul(0x300).wrapping_sub(1);
    let max_y = world_ys.wrapping_mul(0x300).wrapping_sub(1);
    ex = ex.max(0).min(max_x);
    ey = ey.max(0).min(max_y);
    a.ex = ex;
    a.ey = ey;
    a.ez = env.crash_terrain_z(ex, ey);

    let ddx = ex.wrapping_sub(guy.x);
    let ddy = ey.wrapping_sub(guy.y);
    let squared = ddx.wrapping_mul(ddx).wrapping_add(ddy.wrapping_mul(ddy));
    let distance = (squared as f32).sqrt();
    let raw_time = crash_cvtt_f32_i32(distance / (speed as f32));
    a.flags |= FLAG_FLYING;
    a.angle = guy.angle;
    a.splash_area = 2;
    a.cur_time = 0;
    a.total_time = if (raw_time as u32) < 1 {
        1
    } else {
        raw_time as u32
    };

    a.rolling = (game_rng.draw16() % 7 - 3) as i8;
    a.traj = TRAJ_ARC;
    a.v1z = 0.0;
    a.dx = distance / (a.total_time as f32);

    let seed = guy.x.wrapping_add(guy.y).wrapping_add(guy.z) as u32;
    let mut local_rng = Rng(seed);
    a.bank_dx = local_rng.in_range(-30, 30) as f32;
    a.bank_dy = local_rng.in_range(-30, 30) as f32;
    a.whom = -1;
    a.ox = -1;
}

/// The ammo-pool transaction surrounding [`ammo_init_crash`] in `Objects::kill_guy`
/// (`0x00659410`).
///
/// Retail scans for the lowest free slot, passes the current `Objects::ammo_index` as the
/// wreck's `graph_index`, runs the constructor against that recycled slot, and only then
/// increments the counter. The PDB `TypeData::cat == 8` and `obj_masks & 0x08000000` gates stay
/// with the death caller; once it elects to spawn a wreck, this is the complete ammo-owned
/// transaction.
pub fn ammo_spawn_crash<E: CrashEnv + ?Sized>(
    pool: &mut AmmoPool,
    guy: &CrashGuy,
    env: &E,
    game_rng: &mut Rng,
) -> usize {
    let slot = pool.alloc_slot();
    let graph_index = pool.ammo_index;
    ammo_init_crash(
        &mut pool.slots[slot],
        guy,
        slot as i32,
        graph_index,
        env,
        game_rng,
    );
    pool.ammo_index = pool.ammo_index.wrapping_add(1);
    slot
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
            // Spline flight is driven by the slot sidecar; live owners call
            // AmmoPool::step_cruise_targeted_slot after this common increment boundary
            // and re-enter here on SnappedForImmediateLoop.
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
/// The engine walks a precomputed disc of four-tile `WCoord` offsets
/// (`DAT_00ADC400` / `DAT_00ADCAF0`,
/// count from `DAT_00ADD1E0[ring]`) where
/// `ring = min(10, splash_area/4 + 1)`, reads each tile's occupant from
/// `World.tiles[t] + 8` (`o`) and `+ 10` (`who`).  [`ammo_do_damage_splash_scan`] now owns
/// that exact extraction.  This older compatibility adapter accepts an already-selected list
/// and reproduces only the rectangular per-victim arithmetic; it deliberately does not replace
/// the exact scan's down-chain, diplomacy, unit geometry, or mutation behaviour.
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

/// One object reached by the splash walk's `WData::down` / `ObjectData::down` chain.
///
/// The three virtual predicates are kept explicit because retail does not reduce them to one
/// generic `alive` test.  Slot `+0x0c` (`is_live_build`) selects the building arm first; the
/// other arm then requires slot `+0x08` (`is_live_unit`) and slot `+0xbc` (`is_on_map`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplashObject {
    pub view: ObjView,
    /// Object virtual `+0x0c`: live Build or Wall.  This arm uses the rectangular footprint.
    pub live_build: bool,
    /// Object virtual `+0x08`: live Unit.  Read only when [`SplashObject::live_build`] is false.
    pub live_unit: bool,
    /// Object virtual `+0xbc`: `ObjectData::is_on_map`.
    pub on_map: bool,
    /// `UnitType + 0x23c`, PDB `UnitData::block_radius`.
    pub block_radius: i32,
    /// PDB `ObjectData::{down,down_who}` at `+0x2c/+0x2e`.
    pub down: Option<(i32, i32)>,
}

/// World/object access needed by retail's splash-victim extractor.
pub trait SplashEnv {
    /// `WorldData +0/+4`, in four-tile `WCoord` cells.
    fn world_wcells(&self) -> (i32, i32);
    /// Head of the object chain stored in one `WData` cell (`+8/+10`).
    fn splash_head(&self, wx: i32, wy: i32) -> Option<(i32, i32)>;
    /// `Objects[who][o]`, including the next link read before any victim gate.
    fn splash_object(&self, who: i32, o: i32) -> Option<SplashObject>;
    /// `LeaderData::is_enemy` (`0x006ebaa0`).  The recorded primary bypasses this predicate.
    fn splash_is_enemy(&self, shooter_who: i32, candidate_who: i32) -> bool;
}

/// The mutable half of retail's splash transaction.
///
/// [`SplashEnv`] deliberately exposes only snapshot reads, which is enough to inspect or
/// replay a scan.  The real `Ammo::do_damage` loop is stronger: after it reads the current
/// node's `down` link, it calls `Object::do_damage` **before** looking up that next node.  A
/// hit may therefore kill/remove that node or change diplomacy before the walk resumes.
/// Implementors must apply the call synchronously; queueing it until the scan ends changes
/// retail ordering.
pub trait SplashDamageEnv: SplashEnv {
    fn splash_do_damage(&mut self, call: DamageCall);
}

/// The byte immediately following each 441-entry ring table in retail `.rdata`.
///
/// `Ammo::do_damage` uses `jle` at `0x00678b50`, so its walk is inclusive: it reads index
/// `RING_COUNT[ring]`, not merely the preceding prefix.  For rings 1..9 that is the first
/// point of the next ring.  At ring 10 it reads the adjacent bytes at index 441.  Those decode
/// as `(0x00610076, 120)` and are necessarily out of bounds on a retail-sized world, but they
/// remain part of the exact extractor and are pinned here rather than silently repaired.
pub const SPLASH_RING_SENTINEL: (i32, i32) = (0x0061_0076, 120);

/// Number of table probes made for `splash_area > 0`, including retail's inclusive endpoint.
#[inline]
pub fn splash_probe_count(splash_area: i32) -> usize {
    if splash_area <= 0 {
        return 0;
    }
    let ring = splash_ring(splash_area) as usize;
    super::collision::RING_COUNT[ring] as usize + 1
}

/// One exact splash-table probe.  The shared literals are the measured retail tables owned by
/// `systems::collision`; index 441 is the adjacent-byte sentinel described above.
#[inline]
pub fn splash_probe_offset(index: usize) -> Option<(i32, i32)> {
    if index < super::collision::RING_X.len() {
        Some((
            super::collision::RING_X[index],
            super::collision::RING_Y[index],
        ))
    } else if index == super::collision::RING_X.len() {
        Some(SPLASH_RING_SENTINEL)
    } else {
        None
    }
}

/// `div_3_table[coord >> 8]`: world coordinate to the four-tile `WCoord` grid used by the
/// splash walk.  `div_3_table[i] == i/3`; the shift is deliberately 8 rather than the tile
/// conversion's 6.  Impact positions are in-bounds/non-negative at this call site.
#[inline]
pub fn splash_wcell(coord: i32) -> i32 {
    (coord >> 8) / 3
}

/// Unit-arm splash falloff (`0x00678a58..0x00678ac3`).
///
/// Unlike buildings, units use centre distance minus `block_radius + 192`, and the call is
/// suppressed at `scale <= 0` (`jle`).
#[inline]
pub fn splash_unit_scale(
    impact_x: i32,
    impact_y: i32,
    victim: &ObjView,
    block_radius: i32,
    splash_area: i32,
) -> Option<i32> {
    let centre = vector_dist(impact_x - victim.x, impact_y - victim.y);
    let distance = (centre - block_radius - TILE).max(0);
    let den = splash_area * TILE;
    if den == 0 {
        return None;
    }
    let scale = 0x100 - ((distance << 8) / den);
    if scale <= 0 {
        None
    } else {
        Some(scale)
    }
}

/// Resumable state for the retail splash-table/down-chain walk.
///
/// This cursor exists to preserve the call boundary at `0x00678b0a`: [`next`] captures the
/// candidate's `down` identity and returns exactly one packed call.  A live adapter can then
/// mutate the world and resume from the captured identity, exactly as the retail loop does.
/// It is intentionally private; callers should use [`ammo_do_damage_splash_scan`] for an
/// inspection snapshot or [`ammo_do_damage_splash_execute`] for the live transaction.
struct SplashCursor {
    primary: (i32, i32),
    target_domain: i32,
    base_x: i32,
    base_y: i32,
    angle: i32,
    world_xs: i32,
    world_ys: i32,
    probe: usize,
    probes: usize,
    next: Option<(i32, i32)>,
}

impl SplashCursor {
    fn new<E: SplashEnv>(a: &AmmoWalk, env: &E, target_domain: i32) -> Self {
        let shooter_midpoint = env
            .splash_object(a.who, a.o)
            .is_some_and(|s| s.view.is_unit && s.view.rules.unit_flags & 0x2000 != 0);
        let (centre_x, centre_y) = if shooter_midpoint {
            (a.sx.wrapping_add(a.ex) / 2, a.sy.wrapping_add(a.ey) / 2)
        } else {
            (a.ex, a.ey)
        };
        let (world_xs, world_ys) = env.world_wcells();
        Self {
            primary: (a.whom, a.ox),
            target_domain,
            base_x: splash_wcell(centre_x),
            base_y: splash_wcell(centre_y),
            angle: crate::trig::find_angle(a.ex.wrapping_sub(a.sx), a.ey.wrapping_sub(a.sy)),
            world_xs,
            world_ys,
            probe: 0,
            probes: splash_probe_count(a.splash_area),
            next: None,
        }
    }

    fn next<E: SplashEnv>(&mut self, a: &mut AmmoWalk, env: &E) -> Option<DamageCall> {
        loop {
            while let Some((who, o)) = self.next.take() {
                if o < 0 {
                    break;
                }
                a.whom = who;
                a.ox = o;

                let Some(victim) = env.splash_object(who, o) else {
                    break;
                };
                // Retail reads +0x2c/+0x2e before every gate and, critically, before the
                // Object::do_damage call returned below.  Keep it in the cursor while that
                // call is allowed to mutate the world.
                self.next = victim.down;

                if (who, o) == (a.who, a.o) || !(0..8).contains(&who) {
                    continue;
                }
                let is_primary = (who, o) == self.primary;
                if !is_primary && !env.splash_is_enemy(a.who, who) {
                    continue;
                }

                let scale = if victim.live_build {
                    if self.target_domain == DOMAIN_AIR {
                        continue;
                    }
                    match splash_scale(a.ex, a.ey, &victim.view, a.splash_area) {
                        // Building arm uses `js`: zero is still dispatched.
                        Some(scale) => scale,
                        None => continue,
                    }
                } else {
                    if !victim.live_unit || !victim.on_map {
                        continue;
                    }
                    let same_air_partition = (self.target_domain != DOMAIN_AIR)
                        == (victim.view.rules.domain != DOMAIN_AIR);
                    if !same_air_partition || victim.view.rules.obj_masks & 0x0800_0000 != 0 {
                        continue;
                    }
                    match splash_unit_scale(
                        a.ex,
                        a.ey,
                        &victim.view,
                        victim.block_radius,
                        a.splash_area,
                    ) {
                        Some(scale) => scale,
                        None => continue,
                    }
                };

                return Some(DamageCall {
                    victim_o: o,
                    victim_who: who,
                    angle: self.angle,
                    num_guys: a.num_guys,
                    ammo_slot: a.index,
                    scale256: scale,
                    secondary: i32::from(!is_primary),
                    shooter_who: a.who,
                    shooter_o: a.o,
                });
            }

            if self.probe >= self.probes {
                return None;
            }
            let (dx, dy) =
                splash_probe_offset(self.probe).expect("probe count is bounded by retail table");
            self.probe += 1;
            let wx = self.base_x.wrapping_add(dx);
            let wy = self.base_y.wrapping_add(dy);
            if wx < 0 || wy < 0 || wx >= self.world_xs || wy >= self.world_ys {
                continue;
            }
            self.next = env.splash_head(wx, wy);
        }
    }
}

/// Retail-exact splash victim extraction and `Object::do_damage` call packing from
/// `Ammo::do_damage` `0x00678629..0x00678b56`.
///
/// This is intentionally mutation-sensitive.  Retail writes each candidate identity into
/// `AmmoData::{whom,ox}` *before* the self/diplomacy/class gates and does not restore the
/// recorded primary afterward.  It also does not deduplicate: an object linked from two world
/// cells receives two calls.  Each cell walks the complete `ObjectData::down` chain.
pub fn ammo_do_damage_splash_scan<E: SplashEnv>(
    a: &mut AmmoWalk,
    env: &E,
    target_domain: i32,
) -> Vec<DamageCall> {
    if a.splash_area <= 0 {
        return Vec::new();
    }
    let mut cursor = SplashCursor::new(a, env, target_domain);
    let mut calls = Vec::new();
    while let Some(call) = cursor.next(a, env) {
        calls.push(call);
    }
    calls
}

/// Execute the ordinary splash walk as one live retail transaction.
///
/// This closes the gap between victim extraction and world mutation.  Calls are applied in
/// table/down-chain order, with each `down` identity captured before its predecessor takes
/// damage.  After the last probe, `Ammo::do_damage` falls through to `Ammo::close`, so the
/// projectile flags are assigned zero and any spline is recycled.  The returned [`Impact`]
/// inside `Some` is an audit record of calls that were actually issued; it is not a deferred
/// command list.  A non-splash input returns `None` without touching ammo or world state.
///
/// The caller must already have run retail's splash preamble (`hit_target`/`check_hit`) and
/// supplied the resulting primary target's domain.  Nuclear terrain effects and the optional
/// post-hit statistics hook precede/follow this primitive and remain separate boundaries.
pub fn ammo_do_damage_splash_execute<E: SplashDamageEnv>(
    a: &mut Ammo,
    env: &mut E,
    target_domain: i32,
) -> Option<Impact> {
    if a.w.splash_area <= 0 {
        return None;
    }
    let mut impact = Impact::default();
    let mut cursor = SplashCursor::new(&a.w, env, target_domain);
    while let Some(call) = cursor.next(&mut a.w, env) {
        impact.calls.push(call);
        env.splash_do_damage(call);
    }
    a.close();
    impact.closed = true;
    Some(impact)
}

/// `ring = min(10, splash_area/4 + 1)` — the `WCoord`-disc selector [measured,
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

    struct CrashWorld {
        xs: i32,
        ys: i32,
    }

    impl CrashEnv for CrashWorld {
        fn crash_world_wcells(&self) -> (i32, i32) {
            (self.xs, self.ys)
        }

        fn crash_terrain_z(&self, x: i32, y: i32) -> i32 {
            x.wrapping_mul(3).wrapping_add(y.wrapping_mul(5)) & 0x3ff
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

    fn crash_fixture() -> CrashGuy {
        CrashGuy {
            who: 2,
            o: 17,
            type_index: 88,
            x: 2_000,
            y: 5_000,
            z: 1_000,
            angle: 0,
            shooter_gpiece: 41,
            owner_x: 2_200,
            owner_y: 5_100,
            moves: 100,
            order_type: 16,
            lead_bank: 12.75,
        }
    }

    fn live_crash_guy() -> super::super::groups_guys::GuyData {
        super::super::groups_guys::GuyData {
            ty: 88,
            x: 2_000,
            y: 5_000,
            z: 1_000,
            angle: crate::trig::QUARTER_TURN,
            bank: 13.75,
            who: 2,
            o: 17,
            ..Default::default()
        }
    }

    #[test]
    fn crash_death_plan_flattens_distinct_guy_owner_lead_and_type_facts() {
        let dying = live_crash_guy();
        let mut lead = dying;
        lead.bank = -9.875;
        let rules = [
            CrashTypeRule {
                type_index: 88,
                cat: CRASH_TYPE_CAT,
                moves: 123,
                ..Default::default()
            },
            CrashTypeRule {
                type_index: 99,
                obj_masks: 0,
                ..Default::default()
            },
        ];
        let owner = CrashOwnerView {
            who: 2,
            o: 17,
            type_index: 99,
            x: 2_200,
            y: 5_100,
            gpiece: Some(41),
            order_type: 16,
            lead_guy: Some(&lead),
        };

        let got = plan_ammo_crash(Some(&dying), owner, &rules)
            .expect("complete live facts")
            .expect("category 8 is eligible");

        assert_eq!(got.type_index, 88);
        assert_eq!((got.who, got.o), (2, 17));
        assert_eq!((got.x, got.y, got.z), (2_000, 5_000, 1_000));
        assert_eq!((got.owner_x, got.owner_y), (2_200, 5_100));
        assert_eq!((got.shooter_gpiece, got.moves), (41, 123));
        assert_eq!((got.order_type, got.lead_bank), (16, -9.875));
    }

    #[test]
    fn crash_death_plan_distinguishes_known_gates_from_missing_admitted_facts() {
        let dying = live_crash_guy();
        let owner = CrashOwnerView {
            who: 2,
            o: 17,
            type_index: 99,
            x: 2_200,
            y: 5_100,
            gpiece: None,
            order_type: 16,
            lead_guy: None,
        };

        let ground = [CrashTypeRule {
            type_index: 88,
            cat: 7,
            ..Default::default()
        }];
        assert_eq!(
            plan_ammo_crash(Some(&dying), owner, &ground),
            Ok(None),
            "known category rejection must not demand owner-only facts"
        );

        let suppressed = [
            CrashTypeRule {
                type_index: 88,
                cat: CRASH_TYPE_CAT,
                ..Default::default()
            },
            CrashTypeRule {
                type_index: 99,
                obj_masks: CRASH_SUPPRESS_OBJ_MASK,
                ..Default::default()
            },
        ];
        assert_eq!(plan_ammo_crash(Some(&dying), owner, &suppressed), Ok(None));

        let admitted = [
            suppressed[0],
            CrashTypeRule {
                obj_masks: 0,
                ..suppressed[1]
            },
        ];
        assert_eq!(
            plan_ammo_crash(Some(&dying), owner, &admitted),
            Err(CrashPlanError::MissingOwnerGpiece)
        );

        let owner_without_lead = CrashOwnerView {
            gpiece: Some(41),
            ..owner
        };
        assert_eq!(
            plan_ammo_crash(Some(&dying), owner_without_lead, &admitted),
            Err(CrashPlanError::MissingLeadGuy),
            "only an admitted bank-inheriting order requires guys[0]"
        );
        let ordinary_order = CrashOwnerView {
            order_type: 10,
            ..owner_without_lead
        };
        assert!(plan_ammo_crash(Some(&dying), ordinary_order, &admitted)
            .expect("ordinary orders do not read the lead Guy")
            .is_some());
    }

    #[test]
    fn crash_init_is_a_mutating_arc_constructor_not_a_spline_constructor() {
        let env = CrashWorld { xs: 10, ys: 10 };
        let guy = crash_fixture();
        let mut ammo = Ammo::default();
        ammo.w.flags = 0xff;
        ammo.w.accuracy = 321;
        ammo.has_spline = true;
        let before = ammo.w.as_bytes().to_vec();
        let mut rng = Rng(0x1234_5678);

        ammo_init_crash(&mut ammo, &guy, 9, 77, &env, &mut rng);

        assert_ne!(ammo.w.as_bytes(), before.as_slice());
        assert_eq!(
            ammo.w.flags, 0xe3,
            "clear 0x1c, preserve other bits, set FLYING"
        );
        assert_eq!(
            ammo.w.accuracy, 321,
            "init_crash never writes the walked accuracy"
        );
        assert!(
            ammo.has_spline,
            "the function never touches AmmoData::ammo_path"
        );
        assert_eq!((ammo.w.who, ammo.w.o), (2, 17));
        assert_eq!((ammo.w.index, ammo.w.graph_index), (9, 77));
        assert_eq!(ammo.w.gpiece, 41);
        assert_eq!(ammo.w.num_guys, 1);
        assert_eq!((ammo.w.sx, ammo.w.sy, ammo.w.sz), (2_000, 5_000, 1_000));
        assert_eq!(ammo.w.angle, 0);
        assert_eq!(ammo.w.splash_area, 2);
        assert_eq!(ammo.w.cur_time, 0);
        assert_eq!(ammo.w.traj, TRAJ_ARC);
        assert_eq!(ammo.w.v1z.to_bits(), 0);
        assert_eq!((ammo.w.whom, ammo.w.ox), (-1, -1));
        assert_eq!(
            ammo.w.start_roll_angle, -12,
            "bank truncates before integer negation"
        );
        assert_eq!(
            ammo.w.ex, guy.owner_x,
            "angle zero has no x velocity and exposes the owner-centre launch base"
        );
        assert!(
            ammo.w.ey < guy.owner_y,
            "angle zero crashes north from the owner-centre y"
        );
        assert_ne!(
            ammo.w.ex, guy.x,
            "endpoint base is not the recorded individual-Guy start"
        );
        assert_eq!(ammo.w.ez, env.crash_terrain_z(ammo.w.ex, ammo.w.ey));
        assert!(ammo.w.total_time > 0 && ammo.w.dx.is_finite());

        let mut walked = ammo;
        walked.has_spline = false;
        let pool = AmmoPool {
            slots: vec![walked],
            spline_slots: vec![None],
            spline_recycler: Vec::new(),
            spline_graphics: Vec::new(),
            ammo_index: 0,
        };
        assert_eq!(
            pool.checksum(),
            0x8ea7_181d,
            "golden walked mutation pins every checksum-visible crash write"
        );
    }

    #[test]
    fn crash_rng_uses_one_global_draw_then_two_coordinate_seeded_local_draws() {
        let env = CrashWorld { xs: 10, ys: 10 };
        let guy = crash_fixture();
        let seed = 0x89ab_cdef;
        let mut game_rng = Rng(seed);
        let mut ammo = Ammo::default();
        ammo_init_crash(&mut ammo, &guy, 0, 0, &env, &mut game_rng);

        let mut expected_game = Rng(seed);
        let expected_roll = (expected_game.draw16() % 7 - 3) as i8;
        assert_eq!(ammo.w.rolling, expected_roll);
        assert_eq!(
            game_rng, expected_game,
            "bank jitter must not consume game_random"
        );

        let mut local = Rng(guy.x.wrapping_add(guy.y).wrapping_add(guy.z) as u32);
        assert_eq!(ammo.w.bank_dx, local.in_range(-30, 30) as f32);
        assert_eq!(ammo.w.bank_dy, local.in_range(-30, 30) as f32);

        let mut same_sum = guy;
        same_sum.x += 100;
        same_sum.y -= 100;
        same_sum.owner_x += 100;
        same_sum.owner_y -= 100;
        let mut other = Ammo::default();
        let mut other_game_rng = Rng(seed ^ 0xffff);
        ammo_init_crash(&mut other, &same_sum, 0, 0, &env, &mut other_game_rng);
        assert_eq!(
            (ammo.w.bank_dx, ammo.w.bank_dy),
            (other.w.bank_dx, other.w.bank_dy),
            "only sx+sy+sz seeds the private jitter stream"
        );
    }

    #[test]
    fn crash_start_roll_angle_is_only_inherited_by_the_three_air_orders() {
        let env = CrashWorld { xs: 10, ys: 10 };
        for order in [16, 17, 24] {
            let mut guy = crash_fixture();
            guy.order_type = order;
            guy.lead_bank = -9.875;
            let mut ammo = Ammo::default();
            ammo_init_crash(&mut ammo, &guy, 0, 0, &env, &mut Rng(1));
            assert_eq!(ammo.w.start_roll_angle, 9, "order {order}");
        }
        for order in [-1, 0, 15, 18, 23, 25] {
            let mut guy = crash_fixture();
            guy.order_type = order;
            let mut ammo = Ammo::default();
            ammo_init_crash(&mut ammo, &guy, 0, 0, &env, &mut Rng(1));
            assert_eq!(ammo.w.start_roll_angle, 0, "order {order}");
        }

        let mut invalid = crash_fixture();
        invalid.order_type = 16;
        invalid.lead_bank = f32::NAN;
        let mut ammo = Ammo::default();
        ammo_init_crash(&mut ammo, &invalid, 0, 0, &env, &mut Rng(1));
        assert_eq!(
            ammo.w.start_roll_angle,
            i32::MIN,
            "cvttss2si produces INT_MIN for NaN, whose wrapping negation is itself"
        );
    }

    #[test]
    fn crash_landing_is_clamped_in_wcoords_before_terrain_and_flight_time() {
        let env = CrashWorld { xs: 2, ys: 2 };
        let mut east = crash_fixture();
        east.x = 100;
        east.y = 100;
        east.z = 5_000;
        east.angle = crate::trig::QUARTER_TURN;
        east.owner_x = 100;
        east.owner_y = 100;
        east.moves = 500;
        let mut ammo = Ammo::default();
        ammo_init_crash(&mut ammo, &east, 0, 0, &env, &mut Rng(2));
        assert_eq!(ammo.w.ex, 2 * 0x300 - 1);
        assert_eq!(ammo.w.ey, 100);
        assert_eq!(ammo.w.ez, env.crash_terrain_z(1_535, 100));

        let mut north = east;
        north.angle = 0;
        let mut other = Ammo::default();
        ammo_init_crash(&mut other, &north, 0, 0, &env, &mut Rng(2));
        assert_eq!((other.w.ex, other.w.ey), (100, 0));
        assert!(other.w.total_time >= 1);
    }

    #[test]
    fn crash_pool_transaction_reuses_lowest_slot_then_advances_graph_index() {
        let env = CrashWorld { xs: 10, ys: 10 };
        let guy = crash_fixture();
        let mut pool = AmmoPool::new();
        pool.slots[0].w.flags = FLAG_ALIVE;
        pool.slots[1].w.flags = FLAG_NO_DAMAGE;
        pool.slots[1].w.accuracy = 444;
        pool.ammo_index = i32::MAX;
        let mut rng = Rng(7);

        let slot = ammo_spawn_crash(&mut pool, &guy, &env, &mut rng);

        assert_eq!(slot, 1, "the first flags&3==0 recycled slot wins");
        assert_eq!(pool.slots[1].w.index, 1);
        assert_eq!(pool.slots[1].w.graph_index, i32::MAX);
        assert_eq!(
            pool.slots[1].w.accuracy, 444,
            "the recycled body is mutated"
        );
        assert_eq!(
            pool.slots[1].w.flags, FLAG_FLYING,
            "the stale NO_DAMAGE bit is cleared before FLYING is set"
        );
        assert_eq!(
            pool.ammo_index,
            i32::MIN,
            "counter increment wraps after init"
        );
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

    fn targeted_launch_fixture(
        shooter_masks: u32,
        shooter_fly_high: i32,
        target_fly_high: i32,
    ) -> (ObjView, ObjView, LaunchOrder) {
        let shooter = ObjView {
            alive: true,
            is_unit: true,
            x: 100,
            y: 200,
            z: 10,
            guy_mark: 1,
            rules: ShooterRules {
                obj_masks: shooter_masks,
                to_hit: 50,
                attenuate: 0,
                ammo_per_att: 1,
                proj_speed: 20,
                domain: DOMAIN_LAND,
                fly_high: shooter_fly_high,
                fly_low: shooter_fly_high,
                ..Default::default()
            },
            ..Default::default()
        };
        let target = ObjView {
            alive: true,
            is_unit: true,
            x: 2100,
            y: 1300,
            z: 700,
            guy0_z: 760,
            rules: ShooterRules {
                domain: DOMAIN_AIR,
                fly_high: target_fly_high,
                fly_low: target_fly_high,
                target_size: 50,
                ..Default::default()
            },
            ..Default::default()
        };
        let ord = LaunchOrder {
            gpiece: 7,
            start: SpawnPoint {
                x: shooter.x,
                y: shooter.y,
                z: shooter.z + MUZZLE_Z_UNIT,
            },
            who: 0,
            o: 4,
            whom: 1,
            ox: 9,
            angle: 0x1234_5678,
            cruise_pose: CruiseLaunchPose::default(),
            cosmetic: false,
        };
        (shooter, target, ord)
    }

    #[test]
    fn targeted_anti_air_roll_precedes_both_scatter_draws() {
        let (shooter, target, ord) =
            targeted_launch_fixture(super::super::air::OBJ_ANTI_AIR, 100, 100);
        let seed = 0xCAFE_BABE;
        let mut rng = Rng(seed);
        let mut ammo_index = 12;
        let out = ammo_init_targeted(
            3,
            &mut ammo_index,
            &ord,
            &shooter,
            Some(&target),
            AntiAirLaunch::default(),
            MissRadius::Formula,
            2_000,
            &mut rng,
        );
        let ammo = out.ammo.expect("active target creates a projectile");
        assert_eq!(out.anti_air.draws, 1, "ground anti-air rolls once");
        assert!(!out.anti_air.dud, "100 percent accepts every 0..99 roll");
        assert_eq!(ammo.w.flags & FLAG_NO_DAMAGE, 0);
        assert_eq!(ammo_index, 13);
        assert_eq!(ammo.w.graph_index, 12);

        // Mutation guard: if the dud draw is moved after scatter, ex/ey consume draw 1/2
        // instead of 2/3 and both the walked projectile bytes and RNG stream diverge.
        let mut expected = Rng(seed);
        expected.draw16(); // anti-air gate
        let mut ex = target.x;
        let mut ey = target.y;
        let r = miss_radius(MissRadius::Formula, ammo.w.accuracy as i32);
        apply_aim_scatter(&mut ex, &mut ey, r, &mut expected);
        assert_eq!((ammo.w.ex, ammo.w.ey), (ex, ey));
        assert_eq!(rng, expected, "one gate draw followed by x/y scatter");
    }

    #[test]
    fn plain_air_target_gate_short_circuits_or_draws_twice_before_scatter() {
        // Target percentage zero: the first roll always fails and the second is skipped.
        let (shooter, target, ord) = targeted_launch_fixture(0, 100, 0);
        let mut one_draw_rng = Rng(0x1020_3040);
        let mut one_index = 0;
        let one = ammo_init_targeted(
            0,
            &mut one_index,
            &ord,
            &shooter,
            Some(&target),
            AntiAirLaunch::default(),
            MissRadius::Formula,
            2_000,
            &mut one_draw_rng,
        );
        let one_ammo = one.ammo.unwrap();
        assert_eq!(one.anti_air.draws, 1);
        assert!(one.anti_air.dud);
        assert_ne!(one_ammo.w.flags & FLAG_NO_DAMAGE, 0);
        let mut expected_one = Rng(0x1020_3040);
        for _ in 0..3 {
            expected_one.draw16();
        }
        assert_eq!(one_draw_rng, expected_one, "one gate + two scatter draws");

        // Both percentages 100: first passes, second happens and passes.
        let (shooter, target, ord) = targeted_launch_fixture(0, 100, 100);
        let mut two_draw_rng = Rng(0x1020_3040);
        let mut two_index = 0;
        let two = ammo_init_targeted(
            0,
            &mut two_index,
            &ord,
            &shooter,
            Some(&target),
            AntiAirLaunch::default(),
            MissRadius::Formula,
            2_000,
            &mut two_draw_rng,
        );
        let two_ammo = two.ammo.unwrap();
        assert_eq!(two.anti_air.draws, 2);
        assert!(!two.anti_air.dud);
        assert_eq!(two_ammo.w.flags & FLAG_NO_DAMAGE, 0);
        let mut expected_two = Rng(0x1020_3040);
        for _ in 0..4 {
            expected_two.draw16();
        }
        assert_eq!(two_draw_rng, expected_two, "two gate + two scatter draws");
        assert_ne!(
            (one_ammo.w.ex, one_ammo.w.ey),
            (two_ammo.w.ex, two_ammo.w.ey),
            "the short-circuit changes which stream values aim the projectile"
        );
    }

    #[test]
    fn invalid_target_aborts_after_counter_increment_without_rng_or_projectile() {
        let (shooter, mut target, ord) =
            targeted_launch_fixture(super::super::air::OBJ_ANTI_AIR, 100, 100);
        target.alive = false;
        let mut rng = Rng(77);
        let before = rng;
        let mut ammo_index = i32::MAX;
        let out = ammo_init_targeted(
            8,
            &mut ammo_index,
            &ord,
            &shooter,
            Some(&target),
            AntiAirLaunch::default(),
            MissRadius::Formula,
            2_000,
            &mut rng,
        );
        assert!(out.anti_air.init_aborted);
        assert!(out.ammo.is_none());
        assert_eq!(out.anti_air.draws, 0);
        assert_eq!(rng, before);
        assert_eq!(
            ammo_index,
            i32::MIN,
            "Objects::add_ammo increments outside init, with 32-bit wrapping"
        );
    }

    #[test]
    fn dud_bit_is_a_non_vacuous_ammo_checksum_mutation() {
        let (hit_shooter, target, ord) =
            targeted_launch_fixture(super::super::air::OBJ_ANTI_AIR, 100, 100);
        let mut dud_shooter = hit_shooter;
        dud_shooter.rules.fly_high = 0;
        dud_shooter.rules.fly_low = 0;

        let launch = |shooter: &ObjView| {
            let mut rng = Rng(5);
            let mut index = 0;
            ammo_init_targeted(
                0,
                &mut index,
                &ord,
                shooter,
                Some(&target),
                AntiAirLaunch::default(),
                MissRadius::Formula,
                2_000,
                &mut rng,
            )
            .ammo
            .unwrap()
        };
        let hit = launch(&hit_shooter);
        let mut dud = launch(&dud_shooter);
        assert_eq!(hit.w.flags ^ dud.w.flags, FLAG_NO_DAMAGE);
        let mut hit_pool = AmmoPool::new();
        hit_pool.slots[0] = hit;
        let mut dud_pool = AmmoPool::new();
        dud_pool.slots[0] = dud;
        assert_ne!(hit_pool.checksum(), dud_pool.checksum());

        let env = FlatWorld { w: 64, h: 64, z: 0 };
        dud.w.cur_time = dud.w.total_time - 1;
        assert_eq!(
            ammo_inc_time(&mut dud, &env, |_| true, |_| true),
            Step::Closed,
            "arrival closes a dud before the Object::do_damage handoff"
        );
        assert!(!dud.occupied());
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
            spline_slots: vec![None],
            spline_recycler: Vec::new(),
            spline_graphics: Vec::new(),
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

    #[derive(Default)]
    struct SplashWorld {
        xs: i32,
        ys: i32,
        heads: Vec<(i32, i32, i32, i32)>,
        objects: Vec<(i32, i32, SplashObject)>,
        enemies: Vec<(i32, i32)>,
    }

    impl SplashEnv for SplashWorld {
        fn world_wcells(&self) -> (i32, i32) {
            (self.xs, self.ys)
        }

        fn splash_head(&self, wx: i32, wy: i32) -> Option<(i32, i32)> {
            self.heads
                .iter()
                .find(|&&(x, y, _, _)| x == wx && y == wy)
                .map(|&(_, _, who, o)| (who, o))
        }

        fn splash_object(&self, who: i32, o: i32) -> Option<SplashObject> {
            self.objects
                .iter()
                .find(|&&(w, i, _)| w == who && i == o)
                .map(|&(_, _, object)| object)
        }

        fn splash_is_enemy(&self, shooter_who: i32, candidate_who: i32) -> bool {
            self.enemies.contains(&(shooter_who, candidate_who))
        }
    }

    fn splash_unit(x: i32, y: i32, domain: i32, down: Option<(i32, i32)>) -> SplashObject {
        SplashObject {
            view: ObjView {
                alive: true,
                is_unit: true,
                x,
                y,
                rules: ShooterRules {
                    domain,
                    ..Default::default()
                },
                ..Default::default()
            },
            live_build: false,
            live_unit: true,
            on_map: true,
            block_radius: 0,
            down,
        }
    }

    fn splash_build(x: i32, y: i32, down: Option<(i32, i32)>) -> SplashObject {
        SplashObject {
            view: ObjView {
                alive: true,
                x,
                y,
                ..Default::default()
            },
            live_build: true,
            live_unit: false,
            on_map: false,
            block_radius: 0,
            down,
        }
    }

    #[test]
    fn splash_scan_uses_the_shipped_inclusive_ring_endpoint() {
        assert_eq!(splash_probe_count(0), 0);
        assert_eq!(
            splash_probe_count(1),
            10,
            "ring 1 count 9 is an inclusive endpoint"
        );
        assert_eq!(splash_probe_offset(9), Some((-1, -2)));
        assert_eq!(splash_probe_count(40), 442);
        assert_eq!(splash_probe_offset(440), Some((-10, 0)));
        assert_eq!(splash_probe_offset(441), Some(SPLASH_RING_SENTINEL));
        assert_eq!(splash_probe_offset(442), None);
    }

    #[test]
    fn splash_scan_walks_down_links_in_order_and_leaves_the_last_identity_in_ammo() {
        let centre = 4 * 768;
        let mut world = SplashWorld {
            xs: 10,
            ys: 10,
            heads: vec![(4, 4, 1, 10)],
            objects: vec![
                (0, 7, splash_unit(0, 0, DOMAIN_LAND, None)),
                (
                    1,
                    10,
                    splash_unit(centre, centre, DOMAIN_LAND, Some((2, 20))),
                ),
                (2, 20, splash_build(centre, centre, None)),
            ],
            enemies: vec![(0, 1), (0, 2)],
        };
        let mut a = AmmoWalk {
            sx: centre - 100,
            sy: centre,
            ex: centre,
            ey: centre,
            splash_area: 1,
            who: 0,
            o: 7,
            whom: 1,
            ox: 10,
            num_guys: 3,
            index: 12,
            ..Default::default()
        };
        let calls = ammo_do_damage_splash_scan(&mut a, &world, DOMAIN_LAND);
        assert_eq!(
            calls
                .iter()
                .map(|c| (c.victim_who, c.victim_o))
                .collect::<Vec<_>>(),
            [(1, 10), (2, 20)]
        );
        assert_eq!((calls[0].secondary, calls[1].secondary), (0, 1));
        assert_eq!(calls[0].angle, crate::trig::find_angle(100, 0));
        assert_eq!(
            (a.whom, a.ox),
            (2, 20),
            "the scan does not restore the primary"
        );

        // The engine does no identity deduplication: put the same chain head in the next
        // probe cell and both linked objects are dispatched again.
        world.heads.push((3, 3, 1, 10)); // table index 1 = (-1,-1)
        a.whom = 1;
        a.ox = 10;
        let duplicated = ammo_do_damage_splash_scan(&mut a, &world, DOMAIN_LAND);
        assert_eq!(duplicated.len(), 4);
    }

    #[test]
    fn splash_primary_bypasses_diplomacy_but_secondary_allies_and_self_do_not() {
        let centre = 3 * 768;
        let world = SplashWorld {
            xs: 8,
            ys: 8,
            heads: vec![(3, 3, 1, 10)],
            objects: vec![
                (0, 7, splash_unit(centre, centre, DOMAIN_LAND, None)),
                (
                    1,
                    10,
                    splash_unit(centre, centre, DOMAIN_LAND, Some((1, 11))),
                ),
                (
                    1,
                    11,
                    splash_unit(centre, centre, DOMAIN_LAND, Some((8, 80))),
                ),
                (
                    8,
                    80,
                    splash_unit(centre, centre, DOMAIN_LAND, Some((0, 7))),
                ),
            ],
            enemies: vec![],
        };
        let mut a = AmmoWalk {
            ex: centre,
            ey: centre,
            splash_area: 1,
            who: 0,
            o: 7,
            whom: 1,
            ox: 10,
            ..Default::default()
        };
        let calls = ammo_do_damage_splash_scan(&mut a, &world, DOMAIN_LAND);
        assert_eq!(calls.len(), 1);
        assert_eq!(
            (calls[0].victim_who, calls[0].victim_o, calls[0].secondary),
            (1, 10, 0)
        );
        assert_eq!(
            (a.whom, a.ox),
            (0, 7),
            "owner 8 and the rejected self node are followed and mutate the fields"
        );
    }

    #[test]
    fn splash_unit_gates_and_building_zero_scale_asymmetry_are_exact() {
        let mut dead = splash_unit(0, 0, DOMAIN_LAND, Some((2, 22)));
        dead.live_unit = false;
        let mut off_map = splash_unit(0, 0, DOMAIN_LAND, Some((2, 23)));
        off_map.on_map = false;
        let air = splash_unit(0, 0, DOMAIN_AIR, Some((2, 24)));
        let mut missile = splash_unit(0, 0, DOMAIN_LAND, Some((2, 25)));
        missile.view.rules.obj_masks = 0x0800_0000;
        // At 192 from a zero-size build the rectangular arm computes scale == 0 and still
        // dispatches (`js`).  A unit at 384 has centre distance - (block 0 + 192) == 192,
        // also scale zero, but its arm uses `jle` and suppresses the call.
        let build_zero = splash_build(192, 0, Some((2, 26)));
        let unit_zero = splash_unit(384, 0, DOMAIN_LAND, None);
        let world = SplashWorld {
            xs: 3,
            ys: 3,
            heads: vec![(0, 0, 2, 20)],
            objects: vec![
                (0, 7, splash_unit(0, 0, DOMAIN_LAND, None)),
                (2, 20, dead),
                (2, 22, off_map),
                (2, 23, air),
                (2, 24, missile),
                (2, 25, build_zero),
                (2, 26, unit_zero),
            ],
            enemies: vec![(0, 2)],
        };
        let mut a = AmmoWalk {
            ex: 0,
            ey: 0,
            splash_area: 1,
            who: 0,
            o: 7,
            whom: 9,
            ox: 9,
            ..Default::default()
        };
        let calls = ammo_do_damage_splash_scan(&mut a, &world, DOMAIN_LAND);
        assert_eq!(calls.len(), 1);
        assert_eq!(
            (calls[0].victim_who, calls[0].victim_o, calls[0].scale256),
            (2, 25, 0)
        );

        a.whom = 9;
        a.ox = 9;
        let air_calls = ammo_do_damage_splash_scan(&mut a, &world, DOMAIN_AIR);
        assert_eq!(
            air_calls
                .iter()
                .map(|c| (c.victim_who, c.victim_o))
                .collect::<Vec<_>>(),
            [(2, 23)],
            "air-target splash rejects builds/non-air units and admits the air unit"
        );
    }

    #[test]
    fn splash_midpoint_flag_moves_the_wcoord_scan_centre_with_truncation_to_zero() {
        let mut shooter = splash_unit(0, 0, DOMAIN_LAND, None);
        shooter.view.rules.unit_flags = 0x2000;
        let world = SplashWorld {
            xs: 4,
            ys: 4,
            heads: vec![(0, 0, 1, 10)],
            objects: vec![(0, 7, shooter), (1, 10, splash_build(0, 0, None))],
            enemies: vec![(0, 1)],
        };
        let mut a = AmmoWalk {
            // (-3 + 2) / 2 is 0 in MSVC's signed divide-by-two sequence, not -1.
            sx: -3,
            sy: 0,
            ex: 2,
            ey: 0,
            splash_area: 1,
            who: 0,
            o: 7,
            whom: 1,
            ox: 10,
            ..Default::default()
        };
        let calls = ammo_do_damage_splash_scan(&mut a, &world, DOMAIN_LAND);
        assert_eq!(calls.len(), 1);
        assert_eq!(splash_wcell(a.sx.wrapping_add(a.ex) / 2), 0);
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
                cruise_pose: CruiseLaunchPose::default(),
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
