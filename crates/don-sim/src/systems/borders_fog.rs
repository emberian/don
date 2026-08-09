//! Borders / territory / supply / attrition / fog-of-war.
//!
//! # This module does not own the `world` channel. It writes into it.
//!
//! Channel 12 (`world`) is `World::walk_data` `0x006B5CF0`, and its single Rust owner is
//! [`crate::systems::map_terrain::World`] — the storage *and* the walker. This module used
//! to declare itself the channel owner too, with its own `Grids` / `WDataPlane` /
//! `FogPlanes` copies of the state and a `world_checksum` covering 6 of the walk's 13
//! sections. Two owners meant neither could be validated: whichever one a harness read, the
//! other one's writes were invisible.
//!
//! Reconciled 2026-08-08 in favour of `map_terrain`, on the evidence of the walk itself —
//! `re/decomp-all/006b5cf0.c` emits thirteen guarded sections including the six
//! `SimpleArray<WCoord>` sub-object walks, the `CollBlock` loop, and four `Terrain` arrays
//! reached through `[0x00c06218]`, none of which this module modelled. What this module had
//! and `map_terrain` lacked — the per-section digest, so a mismatch says *which* section —
//! was merged in as [`crate::systems::map_terrain::World::checksum_sections`].
//!
//! So: **`map_terrain::World` is the WData storage and the checksum walker; this module is
//! the territory and fog *behaviour* that writes `WData::who` / `who2` / `was_seen` and the
//! three fog planes through it.** Every function here that touches checksummed state takes
//! a `&World` or `&mut World`, so there is exactly one copy of the bytes and
//! `World::checksum()` sees this module's writes by construction.
//!
//! # Provenance
//!
//! Everything here is `[measured]` against `ron-bin/riseofnations.exe` +
//! `ron-bin/sbl/rise.pdb` unless a comment says otherwise. Structure comes from
//! `re/decomp-all/<EA>.c`; field names and layouts from the PDB TPI stream
//! (`schema/pdb-types.json`); rule constants from `docs/derivation/rules-constants.json`
//! keyed by the byte offset the instruction stream actually loads.
//!
//! Per `docs/CHARTER.md` this is **fidelity tier C** — behaviourally faithful to the
//! decompiled structure, with the exact integer expressions transcribed, but *not*
//! differentially tested against retail. There is no oracle entry for any function here
//! yet. Nothing in this file is "verified".
//!
//! # The functions this ports
//!
//! | VA | symbol | what |
//! |---|---|---|
//! | `0x006B76F0` | `World::init(u16,u16)` | the four grids and every allocation size |
//! | `0x006B0BB0` | `World::compute_reg_territory(int,int)` | **the border computation** |
//! | `0x006B5700` | `World::compute_all_territory()` | full recompute driver |
//! | `0x00732060` | `GameDaemon::check_borders()` | per-frame incremental driver |
//! | `0x006B3C60` | `World::set_seen(FCoord,FCoord,int,int)` | the fog write |
//! | `0x006B41D0` | `World::set_was_seen` | explored write |
//! | `0x006B2250` | `World::clear_seen()` | per-frame visible-plane clear |
//! | `0x00732840` | `GameDaemon::update_all_seen()` | the fog pass |
//! | `0x00651B80` | `Object::update_seen(int)` | per-object LOS stamp |
//! | `0x006817F0` | `circle_init()` | the LOS disc offset table |
//! | `0x006B55C0`/`53F0`/`42C0`/`48C0` | `WorldData::is_seen`/`was_seen`/`is_really_seen`/`is_detected` | the queries |
//! | `0x006B4700`/`2510`/`2490` | `WorldData::get_who`/`get_who2`/`is_enemy_territory` | territory ownership |
//! | `0x005E11A0` | `Unit::process_attrition()` | attrition period selection |
//! | `0x00608FD0` | `UnitData::get_attrition(int)` | attrition rate |
//! | `0x005E1A10` | `Unit::suffer_attrition(int)` | attrition damage |
//! | `0x005E0560` | `Unit::process_supply()` | supply predicate |
//!
//! # Known gaps, stated up front
//!
//! * `Object::update_seen`'s *incremental* path (the `ring_init` `0x00681920` tables) is
//!   not ported — only the full restamp. See [`update_seen`].
//! * The Tikal border multiplier is read from `TIKAL_TEMPLE_HP`, not `TIKAL_TEMPLE_BORDERS`
//!   — `[measured]`, see [`TerritoryRules::tikal_temple_borders_pct`].
//! * `Supplies::find_supply` `0x0073ABA0` is not ported — [`supply_state`] takes the
//!   supplier set as an argument instead.
//! * The out-of-supply reload multipliers are exposed as constants; the call site that
//!   applies them was not located.

#![allow(clippy::too_many_arguments)]

use super::map_terrain::{World, COORD_PER_FCELL, COORD_PER_TILE, COORD_PER_WCELL};
use crate::deviations::{behaviour as deviation_behaviour, ModeConfig};

// ---------------------------------------------------------------------------
// 0. Coordinate systems
// ---------------------------------------------------------------------------

/// Fine world units per `rules.xml` "tile". Alias of
/// [`crate::systems::map_terrain::COORD_PER_TILE`] — the coordinate ladder is derived once,
/// in `map_terrain`, and this module names it in the units its own derivation used.
///
/// `[measured]` two ways that agree: `String::fraction` scale 192 is used for
/// `unit_formation_spacing` (`"1/16 tile"` stores 12 = 192/16) and `unit_move_speed`
/// (`"1/192 tile"` stores 1); and `div_3_table[c >> 6] == c / 192` is what
/// `World::compute_reg_territory` uses to put a city into the tile grid.
pub const FINE_PER_TILE: i32 = COORD_PER_TILE;

/// Fine units per fog cell. `Object::update_seen` computes `(los * 0xC0) / 0x180`
/// = `los * 192 / 384`, and reads its own position through `div_3_table[c >> 7]`
/// = `c / 384`. `[measured]`
pub const FINE_PER_FOG: i32 = COORD_PER_FCELL;

/// Fine units per `WCoord` cell — the `WData` grid, where territory lives.
/// `WallData::in_unfriendly_territory` indexes `WData` with `div_3_table[c >> 8]`
/// = `c / 768`. `[measured]`
pub const FINE_PER_WCOORD: i32 = COORD_PER_WCELL;

/// Object coordinates are stored XOR-obfuscated with this key.
/// `[measured]` — every `Object` position read in the decompiled corpus is
/// `*(u32*)(obj + 0x10) ^ 0x63637` / `+0x14 ^ 0x63637`.
pub const COORD_XOR: u32 = 0x0006_3637;

/// `div_3_table[i]`, the table at `int *div_3_table` `0x00CAE5FC`.
///
/// It exists so the three shifts below land on the three grids: `>>6` then `/3` is
/// `/192` (tile), `>>7` then `/3` is `/384` (fog), `>>8` then `/3` is `/768` (WCoord).
///
/// **Corrected while reconciling the two modules**: this used to be `v / 3`, Rust's
/// truncating division, which disagrees with the table for every negative `v` not
/// divisible by 3 (`-1/3 == 0`, but `div_3_table[-1] == -1`). `init_coord_lookup_array`
/// `0x00681db0` fills `t[j] = (j - 2) / 3` for `j < 0`, i.e. `floor(j/3)` on both sides of
/// zero — which is what [`crate::systems::map_terrain::div_3`] already implemented and what
/// this now delegates to. Off-map and clamped coordinates are the ones that go negative.
#[inline]
pub const fn div3(v: i32) -> i32 {
    super::map_terrain::div_3(v)
}

/// Deobfuscate a stored object coordinate.
#[inline]
pub const fn deobf(stored: u32) -> i32 {
    (stored ^ COORD_XOR) as i32
}

/// Fine coordinate → tile index (`TCoord`).
#[inline]
pub const fn fine_to_tile(fine: i32) -> i32 {
    div3(fine >> 6)
}

/// Fine coordinate → fog cell index (`FCoord`).
#[inline]
pub const fn fine_to_fog(fine: i32) -> i32 {
    div3(fine >> 7)
}

/// Fine coordinate → world cell index (`WCoord`) — the `WData` grid.
#[inline]
pub const fn fine_to_wcoord(fine: i32) -> i32 {
    div3(fine >> 8)
}

/// The engine's integer hypotenuse.
///
/// `[measured]`, appears verbatim in both `circle_init` `0x006817F0` and
/// `World::compute_reg_territory` `0x006B0BB0`:
///
/// ```text
/// mx = max(a,b); mn = min(a,b)
/// mx == 0        -> 0
/// mn <  60000    -> mn*mn / (2*mx) + mx
/// otherwise      -> (mn + 2*mx) >> 1
/// ```
///
/// The first arm is the second-order Taylor form of `sqrt(mx² + mn²)`; the guard exists
/// only so `mn*mn` cannot overflow 32 bits. Both call sites take `abs` of each component
/// first, so the inputs are non-negative.
#[inline]
pub fn hypot_approx(a: u32, b: u32) -> u32 {
    let (mn, mx) = if a < b { (a, b) } else { (b, a) };
    if mx == 0 {
        0
    } else if mn < 60_000 {
        (mn * mn) / (mx * 2) + mx
    } else {
        (mn + mx * 2) >> 1
    }
}

/// `(a + b) & 0x80000007` with the MSVC sign correction — i.e. a signed `% 8` that keeps
/// the sign of the dividend. `World::compute_reg_territory` uses it to rotate the leader
/// scan order by the tile's x coordinate. `[measured]`
#[inline]
pub fn rem8_signed(v: i32) -> i32 {
    let m = v & (0x8000_0007u32 as i32);
    if m < 0 {
        ((m - 1) | !7i32) + 1
    } else {
        m
    }
}

// ---------------------------------------------------------------------------
// 1. Grids and WData storage — both live in `map_terrain`
// ---------------------------------------------------------------------------

// `World::init` `0x006B76F0`'s grid arithmetic, the 28-byte `WData` record and the three
// fog planes were all modelled here as `Grids` / `WDataPlane` / `FogPlanes`. They are gone:
// `map_terrain::World` already carried the same state in the PDB's own field order, and it
// is the state the `world` channel walks. A second copy could only diverge from it.
//
// The mapping, for anyone following an old call site:
//
//   Grids{xs,ys,size,fog_*,tile_*,reg_*}  ->  World's fields of the same names
//   Grids::w_index / f_index              ->  World::w_index / World::f_index
//   Grids::w_in_bounds / f_in_bounds      ->  World::valid_w / World::valid_f
//   WDataPlane::who[i] / who2[i]          ->  World::wdata[i].who / .who2
//   WDataPlane::walk_bytes(i, out)        ->  World::wdata[i].checksum_bytes()
//   FogPlanes{seen,seen2,seen3,wcoord_seen} -> World's fields of the same names
//   world_checksum(..)                    ->  World::checksum_sections()

// ---------------------------------------------------------------------------
// 3. Fog of war
// ---------------------------------------------------------------------------

// The three fog planes plus the coarse `wcoord_seen` plane are `World` fields — they are
// sections 6 and 7 of the checksum, so they live with the rest of the walked state:
//
// | plane | `World` offset | length | meaning |
// |---|---|---|---|
// | `seen`   | `+0x15C` | `fog_size` | currently visible this frame (cleared every tick) |
// | `seen2`  | `+0x160` | `fog_size` | ever explored |
// | `seen3`  | `+0x164` | `fog_size` | **detected** — the stealth-detection plane |
// | `wcoord_seen` | `+0x168` | `size` | explored, at WCoord resolution |
//
// Each byte is a **bitmask over the 8 player slots**; `WorldData::is_seen` and friends test
// it against `LeaderData +0x6929`, a per-leader single-bit player mask.

/// Per-leader flags that short-circuit the fog queries, mirroring the bit tests in
/// `WorldData::is_seen` / `was_seen` / `was_really_seen`.
#[derive(Clone, Copy, Debug, Default)]
pub struct FogLeader {
    /// `LeaderData` flag `0x800` — "sees everything" (observer / defeated / cheat).
    pub see_all: bool,
    /// `LeaderData` flag `0x1000` — "has explored everything" (checked only by `was_seen`).
    pub explored_all: bool,
    /// `LeaderData` flag `0x2000` — "sees inside own territory".
    pub see_own_territory: bool,
    /// `LeaderData +0x59E4` (`(short)puVar[0x1679]`) non-zero — a reveal-map counter.
    pub reveal_counter: i16,
    /// `LeaderData +0x6929` — the single-bit player mask this leader tests planes with.
    pub player_mask: u8,
}

/// `Game +0x30` — the "fog / exploration" game option. `is_seen` returns 1 unconditionally
/// at 3; `was_seen` at >= 2; `was_really_seen` at 3. `[measured]`, values not yet named.
#[derive(Clone, Copy, Debug, Default)]
pub struct FogOption(pub u8);

/// The fog-of-war **policy**: the per-leader facts and the game option that the plane
/// queries consult. The planes themselves are `World` fields.
///
/// This is deliberately not a store. `Fog` holds only what `LeaderData` and `Game` supply;
/// every method takes the `World` whose planes it reads or writes, so a fog stamp is
/// visible to `World::checksum()` the instant it happens.
#[derive(Clone, Debug, Default)]
pub struct Fog {
    pub leaders: [FogLeader; 8],
    pub option: FogOption,
}

impl Fog {
    pub fn new() -> Self {
        Self::default()
    }

    /// `World::set_seen(FCoord fx, FCoord fy, int player, int detect)` `0x006B3C60`.
    ///
    /// Writes the visible plane, optionally the detected plane, the explored plane, the
    /// coarse `WData::was_seen` bitmask and `wcoord_seen`. Returns **true iff the explored
    /// plane changed** — the caller (`Object::update_seen`) uses that as the "newly
    /// explored, run `reveal_fog`" trigger.
    ///
    /// `player < 0` writes mask `0xFF` (all players).
    pub fn set_seen(&self, w: &mut World, fx: i32, fy: i32, player: i32, detect: bool) -> bool {
        let i = w.f_index(fx, fy);
        let mask: u8 = if player < 0 {
            0xFF
        } else {
            1u8 << (player as u32 & 0x1F)
        };
        w.seen[i] |= mask;
        if detect {
            w.seen3[i] |= mask;
        }
        let before = w.seen2[i];
        w.seen2[i] |= mask;
        let after = w.seen2[i];

        let wi = w.w_index(fx >> 1, fy >> 1);
        w.wdata[wi].was_seen |= mask;
        w.wcoord_seen[wi] |= mask;

        after != before
    }

    /// `World::set_was_seen(FCoord, FCoord, int)` `0x006B41D0` — explored only, no
    /// visibility, no return value.
    pub fn set_was_seen(&self, w: &mut World, fx: i32, fy: i32, player: i32) {
        let i = w.f_index(fx, fy);
        let mask: u8 = 1u8 << (player as u32 & 0x1F);
        let wi = w.w_index(fx >> 1, fy >> 1);
        w.wdata[wi].was_seen |= mask;
        w.seen2[i] |= mask;
    }

    /// `GameDaemon::update_all_seen` `0x00732840` also `memset`s `seen3` (detected) to zero
    /// before the object pass, after `World::clear_seen` `0x006B2250` has cleared `seen` and
    /// `wcoord_seen`. `[measured]`
    ///
    /// `World::clear_seen` itself is [`World::clear_seen`]; this is the pair of them, in the
    /// order `update_all_seen` runs them.
    pub fn begin_frame(&self, w: &mut World) {
        w.clear_seen();
        w.seen3.fill(0);
    }

    /// `WorldData::is_really_seen(FCoord, FCoord, int)` `0x006B42C0` — raw current
    /// visibility, with the leader short-circuits but **without** the `Game+0x30` option
    /// override that `is_seen` applies.
    pub fn is_really_seen(&self, w: &World, fx: i32, fy: i32, player: i32) -> bool {
        if player > 7 {
            return true;
        }
        let l = &self.leaders[player as usize];
        if l.see_all || l.reveal_counter != 0 {
            return true;
        }
        if l.see_own_territory {
            let who = w.wdata[w.w_index(fx >> 1, fy >> 1)].who;
            if who >= 0 && self.is_ally(who as i32, player) {
                return true;
            }
        }
        w.seen[w.f_index(fx, fy)] & l.player_mask != 0
    }

    /// `WorldData::is_seen(FCoord, FCoord, int)` `0x006B55C0`.
    pub fn is_seen(&self, w: &World, fx: i32, fy: i32, player: i32) -> bool {
        if player > 7 || self.option.0 == 3 {
            return true;
        }
        self.is_really_seen(w, fx, fy, player)
    }

    /// `WorldData::was_really_seen(FCoord, FCoord, int)` `0x006B54F0` — explored, raw.
    pub fn was_really_seen(&self, w: &World, fx: i32, fy: i32, player: i32) -> bool {
        if player >= 8 || self.option.0 == 3 {
            return true;
        }
        let l = &self.leaders[player as usize];
        if l.see_all || l.reveal_counter != 0 {
            return true;
        }
        w.seen2[w.f_index(fx, fy)] & l.player_mask != 0
    }

    /// `WorldData::is_detected(FCoord, FCoord, int)` `0x006B48C0` — is this cell inside a
    /// *detector's* radius for `player`? This is the stealth counter-plane: a cloaked unit
    /// is drawn/targetable only where `seen3` is set.
    ///
    /// Note it has **no** leader short-circuits at all — `see_all` does not grant detection.
    pub fn is_detected(&self, w: &World, fx: i32, fy: i32, player: i32) -> bool {
        w.seen3[w.f_index(fx, fy)] & self.leaders[player as usize].player_mask != 0
    }

    /// `WorldData::is_detected_by_enemy(FCoord, FCoord, int)` `0x006B50F0` — the same plane
    /// masked with the *complement* of the player's bit: "is anyone but me detecting here".
    pub fn is_detected_by_enemy(&self, w: &World, fx: i32, fy: i32, player: i32) -> bool {
        w.seen3[w.f_index(fx, fy)] & !self.leaders[player as usize].player_mask != 0
    }

    /// Cheap per-player observability plane for the RL environment: `true` where player
    /// `p` currently has vision. One byte-test per cell, no allocation.
    pub fn visible_mask_for<'a>(
        &self,
        w: &'a World,
        player: usize,
    ) -> impl Iterator<Item = bool> + 'a {
        let bit = self.leaders[player].player_mask;
        w.seen.iter().map(move |b| b & bit != 0)
    }

    /// Same, for explored-versus-unexplored.
    pub fn explored_mask_for<'a>(
        &self,
        w: &'a World,
        player: usize,
    ) -> impl Iterator<Item = bool> + 'a {
        let bit = self.leaders[player].player_mask;
        w.seen2.iter().map(move |b| b & bit != 0)
    }

    /// Placeholder for `LeaderData::is_ally` `0x006EDB50`; the diplomacy lane owns the real
    /// one. Overridable so the fog queries stay honest about the dependency.
    fn is_ally(&self, a: i32, b: i32) -> bool {
        a == b
    }
}

// ---------------------------------------------------------------------------
// 4. The LOS disc — circle_init 0x006817F0
// ---------------------------------------------------------------------------

/// The maximum radius `circle_init` tabulates, in fog cells (`0x40`).
pub const CIRCLE_MAX_R: usize = 64;
/// The table's capacity guard: `circle_init` stops as soon as the count exceeds `0x3248`.
pub const CIRCLE_CAP: usize = 0x3248;

/// The precomputed LOS disc: every integer `(dx, dy)` offset with
/// `hypot_approx(|dx|, |dy|) <= r`, grouped into rings and indexed by
/// `radius[r]` = **cumulative** count of offsets out to radius `r`.
///
/// This is `char *circle_x` `0x00CB7E90`, `char *circle_y` `0x00CBB0E0` and
/// `int *circle_radius` `0x00CBE330`, regenerated rather than dumped — `circle_init`
/// `0x006817F0` is small, self-contained and touches nothing but these three arrays, so
/// reproducing it is strictly better than a byte dump we would have to ship.
#[derive(Clone, Debug)]
pub struct CircleTable {
    pub x: Vec<i8>,
    pub y: Vec<i8>,
    /// `radius[r]` = number of entries in `x`/`y` with `hypot_approx <= r`.
    pub radius: [i32; CIRCLE_MAX_R + 1],
}

impl CircleTable {
    /// Port of `circle_init` `0x006817F0`, including its exact iteration order (which sets
    /// the *order* offsets are stamped in, and therefore the order `reveal_fog` side
    /// effects fire) and its `0x3248` early-out.
    pub fn build() -> Self {
        let mut x: Vec<i8> = Vec::with_capacity(13_000);
        let mut y: Vec<i8> = Vec::with_capacity(13_000);
        let mut radius = [0i32; CIRCLE_MAX_R + 1];
        let mut lo: i32 = 0;

        for r in 0..=CIRCLE_MAX_R as i32 {
            let mut o = lo;
            while o <= r {
                let a = o.unsigned_abs();
                let mut i = lo;
                while i <= r {
                    let b = i.unsigned_abs();
                    if hypot_approx(a, b) == r as u32 {
                        x.push(o as i8);
                        y.push(i as i8);
                        if x.len() > CIRCLE_CAP {
                            for slot in radius.iter_mut().skip(r as usize) {
                                *slot = x.len() as i32;
                            }
                            return Self { x, y, radius };
                        }
                    }
                    i += 1;
                }
                o += 1;
            }
            radius[r as usize] = x.len() as i32;
            lo -= 1;
        }
        Self { x, y, radius }
    }
}

/// `Object::update_seen` `0x00651B80`: LOS in tiles → disc radius in fog cells.
///
/// `(los * 0xC0) / 0x180`, clamped to `0x40`. The literals are 192 and 384 — one tile and
/// one fog cell in fine units — so this is "LOS in tiles, halved, because a fog cell is
/// two tiles wide". `[measured]`
#[inline]
pub fn los_to_fog_radius(los_tiles: i32) -> i32 {
    let r = (los_tiles * FINE_PER_TILE) / FINE_PER_FOG;
    if r > CIRCLE_MAX_R as i32 {
        CIRCLE_MAX_R as i32
    } else {
        r
    }
}

/// What `Object::update_seen` needs to know about one object.
#[derive(Clone, Copy, Debug)]
pub struct SeeingObject {
    /// Fine world coordinates, already deobfuscated.
    pub fine_x: i32,
    pub fine_y: i32,
    /// Owning player slot.
    pub owner: u8,
    /// Line of sight in tiles (`Object` vtable `+0x128`).
    pub los_tiles: i32,
    /// `Object` flags bit `0x40` — this object *detects* (its whole disc writes `seen3`).
    pub detector: bool,
    /// `Object +0x3A` — a second player this object also grants explored-state to
    /// (`World::set_seen2`). Zero means none.
    pub grant_seen2_to: i8,
}

/// Port of the stamping loop in `Object::update_seen(int incremental)` `0x00651B80`.
///
/// Full form only (`incremental == 0`): the engine's `incremental != 0` path swaps in the
/// `ring_init` `0x00681920` tables and starts at `circle_radius[r-1]` so only the outermost
/// ring is restamped. That path is **not** ported — it is a pure optimisation over the same
/// planes, but it changes which cells get `set_seen` called on them and therefore which
/// cells fire `reveal_fog`, so it must be ported before any replay comparison is trusted.
///
/// Returns the fog cells whose *explored* bit newly flipped — the ones on which the engine
/// then calls `World::reveal_fog` `0x006B3D30`.
pub fn update_seen(
    fog: &Fog,
    w: &mut World,
    circle: &CircleTable,
    obj: &SeeingObject,
    newly_explored: &mut Vec<(i32, i32)>,
) {
    let r = los_to_fog_radius(obj.los_tiles);
    if r <= 0 && obj.los_tiles <= 0 {
        return;
    }
    let ox = fine_to_fog(obj.fine_x);
    let oy = fine_to_fog(obj.fine_y);

    // `local_14` in the original: the index below which set_seen is called with detect=1.
    let detect_end = if obj.detector {
        circle.radius[r as usize]
    } else {
        0
    };
    let end = circle.radius[r as usize];

    // The original hoists a bounds check when the whole disc is inside the map.
    let fully_inside = w.valid_f(ox + r, oy + r) && w.valid_f(ox - r, oy - r);

    for k in 0..end as usize {
        let fx = circle.x[k] as i32 + ox;
        let fy = circle.y[k] as i32 + oy;
        if !fully_inside && !w.valid_f(fx, fy) {
            continue;
        }
        let detect = (k as i32) < detect_end;
        if fog.set_seen(w, fx, fy, obj.owner as i32, detect) {
            newly_explored.push((fx, fy));
        }
        if obj.grant_seen2_to != 0 {
            fog.set_was_seen(w, fx, fy, obj.grant_seen2_to as i32);
        }
    }
}

// ---------------------------------------------------------------------------
// 5. Territory / borders — World::compute_reg_territory 0x006B0BB0
// ---------------------------------------------------------------------------

/// The `Constants` fields the border computation reads, each keyed by the byte offset the
/// instruction stream actually loads from `GameAccess::constants` `[0x00C061F0]`.
///
/// Values are the shipped `rules.xml` values as captured in
/// `docs/derivation/rules-constants.json`, cross-checked against `ron-data/rules.xml`,
/// whose declaration order maps contiguously onto this offset block with no gaps —
/// `FORT_UPGRADE_TERR 0xB8 .. TERRITORY_NUM 0x128` — which is a strong independent
/// confirmation of the whole block.
#[derive(Clone, Copy, Debug)]
pub struct TerritoryRules {
    /// Edition policy for named-rule corrections. Defaults to fidelity.
    pub mode: ModeConfig,
    /// `+0x0B8`, 4 entries — fort border upgrade ladder. `{2, 4, 6, 9}`
    pub fort_upgrade_terr: [i32; 4],
    /// `+0x0C8`, 5 entries — temple border upgrade ladder. `{2, 4, 6, 9, 12}`
    /// (the computation indexes `[0..3]` only).
    pub temple_upgrade_terr: [i32; 5],
    /// `+0x0DC`, 8 entries — government-level border bonus. `{0,1,2,4,6,8,11,14}`
    pub civic_upgrade_terr: [i32; 8],
    /// `+0x0FC` — capital bonus. `6`
    pub capital_territory_bonus: i32,
    /// `+0x100`, 3 entries — per-city-level border bonus, indexed by `level - 1`.
    /// `ron-data/rules.xml` calls it `CITY_UPGRADE_TERR`: `{0, 3, 6}`. The loader name in
    /// `docs/derivation/rules-constants.json` is `city_level_territory_bonus` and it has no
    /// captured value there; the XML tag and the loader name simply differ.
    pub city_level_territory_bonus: [i32; 3],
    /// `+0x10C` — multiplier on a fort's summed bonuses. `4`
    pub fort_territory_multiplier: i32,
    /// `+0x110` — multiplier on a city's summed bonuses. `4`
    pub city_territory_multiplier: i32,
    /// `+0x114` — border radius scale, in tiles. `24`
    pub territory_base: i32,
    /// `+0x118` — hard radius cap base, in tiles. `44`
    pub territory_limit_base: i32,
    /// `+0x11C` — hard cap per government level, in tiles. `4`
    pub territory_limit_civic: i32,
    /// `+0x120` — hard cap per city/fort level, in tiles. `4`
    pub territory_limit_city: i32,
    /// `+0x124` — numerator of the distance term. `5`
    pub territory_den: i32,
    /// `+0x128` — constant term of the denominator. `11`
    pub territory_num: i32,
    /// `+0x46C` — Colosseum. `3`
    pub colosseum_territory_bonus: i32,
    /// `+0x478` — Colosseum, fort ladder. `0`
    pub colosseum_fort_borders: i32,
    /// `+0x498` — the named Tikal temple-border multiplier, percent.
    ///
    /// **Retail instead reads `+0x4A0`, which is `TIKAL_TEMPLE_HP`** — `[measured]` at
    /// `0x006B0DC9`, `mov ecx, [constants + 0x4A0]`. `TIKAL_TEMPLE_BORDERS` is `+0x498`
    /// (XML declaration order: `TIKAL_TIMBER_COMMERCE 0x494`, `TIKAL_TEMPLE_BORDERS 0x498`,
    /// `TIKAL_TEMPLE_RANGE 0x49C`, `TIKAL_TEMPLE_HP 0x4A0`). Both ship as `50`, so no
    /// shipped number moves — but the wonder's border effect is driven by the *hit-point*
    /// field, so a mod editing only `TIKAL_TEMPLE_BORDERS` changes nothing.
    pub tikal_temple_borders_pct: i32,
    /// `+0x4A0` — Tikal temple hit points, incorrectly consumed as the retail border
    /// percentage. Both values ship as 50; keeping separate fields makes mods observable.
    pub tikal_temple_hp_pct: i32,
    /// `+0x53C` — Eiffel Tower. `6`
    pub eiffel_tower_territory_bonus: i32,
    /// `+0x604` — Roman tribe bonus, fort ladder. `3`
    pub roman_fort_borders: i32,
    /// `+0x930` — the Gems rare resource. `2`
    pub gems_territory_bonus: i32,
    /// `+0x760` — Russian tribe bonus, flat. `0`
    pub russian_borders: i32,
    /// `+0x764` — Russian tribe bonus, per age. `1`
    pub russian_borders_per_age: i32,
    /// `+0xB08` — Conquer-the-World missionaries, percent on temple bonus. `25`
    pub ctw_missionaries_bonus: i32,
}

impl Default for TerritoryRules {
    /// The shipped `rules.xml` values.
    fn default() -> Self {
        Self {
            mode: ModeConfig::fidelity(),
            fort_upgrade_terr: [2, 4, 6, 9],
            temple_upgrade_terr: [2, 4, 6, 9, 12],
            civic_upgrade_terr: [0, 1, 2, 4, 6, 8, 11, 14],
            capital_territory_bonus: 6,
            city_level_territory_bonus: [0, 3, 6], // rules.xml CITY_UPGRADE_TERR
            fort_territory_multiplier: 4,
            city_territory_multiplier: 4,
            territory_base: 24,
            territory_limit_base: 44,
            territory_limit_civic: 4,
            territory_limit_city: 4,
            territory_den: 5,
            territory_num: 11,
            colosseum_territory_bonus: 3,
            colosseum_fort_borders: 0,
            tikal_temple_borders_pct: 50,
            tikal_temple_hp_pct: 50,
            gems_territory_bonus: 2,
            eiffel_tower_territory_bonus: 6,
            roman_fort_borders: 3,
            russian_borders: 0,
            russian_borders_per_age: 1,
            ctw_missionaries_bonus: 25,
        }
    }
}

/// The per-leader inputs `compute_reg_territory` reads before it touches any tile.
///
/// Field names mirror the calls the original makes: `LeaderData::has_preq(0x2C8/9/A)` for
/// the temple ladder, `has_preq(0x2D1/2/3)` for the fort ladder, `has_wonder(0x212)`
/// Colosseum, `has_wonder(0x214)` Tikal, `has_wonder(0x21C)` Eiffel,
/// `has_tribe_bonus(6)` Roman, `has_tribe_bonus(0xD)` Russian.
#[derive(Clone, Copy, Debug, Default)]
pub struct LeaderBorderInput {
    pub active: bool,
    /// `has_preq(0x2C8)`, `(0x2C9)`, `(0x2CA)` — the three temple border upgrades.
    pub temple_upgrade: [bool; 3],
    /// `has_preq(0x2D1)`, `(0x2D2)`, `(0x2D3)` — the three fort border upgrades.
    pub fort_upgrade: [bool; 3],
    /// Government level, 0..7 (`LeaderData +0x6EB8 -> +0xEC ^ 0x63187`).
    pub gov_level: i32,
    /// Current age (`LeaderData +0x6EB8 -> +0xDC ^ 0x62766`), used by the Russian bonus.
    pub age: i32,
    /// Holds the **Gems** rare resource in either rare slot — `LeaderData +0x6DA6` or
    /// `+0x6DCE`, bit `0x80`. `[measured]` at `0x006B0F3F`; the constant it reaches for is
    /// `+0x930` `GEMS_TERRITORY_BONUS`, whose `rules.xml` neighbours are `DIAMONDS_COMMERCE`
    /// and `ALUMINUM_AIR_COST`, so this is unambiguously the rare-resource block.
    /// `capital_territory_bonus` is **not** used here — it is a per-city term, applied in
    /// [`claim_tile`].
    pub rare_gems: bool,
    pub wonder_colosseum: bool,
    pub wonder_tikal: bool,
    pub wonder_eiffel: bool,
    pub tribe_roman: bool,
    pub tribe_russian: bool,
    /// Conquer-the-World missionaries apply (`Game +0x822 & 2` and `LeaderData +0x6916`).
    pub ctw_missionaries: bool,
    /// `Leader::get_ctw_border_bonus`-ish term (`0x006DA740`), gated on `Game +0x820 & 4`
    /// and `LeaderData & 4`. Contributes `(v + 15) / 25` to the wonder bonus.
    pub ctw_raw_bonus: i32,
}

/// The eight per-leader values `compute_reg_territory` precomputes into stack arrays,
/// named after what they are rather than after the stack slot.
#[derive(Clone, Copy, Debug, Default)]
pub struct LeaderBorderParams {
    /// `local_170` — temple ladder level, 1..4.
    pub temple_level: i32,
    /// `local_f0` — temple border bonus, after the Tikal and CTW percentage scalings.
    pub temple_terr: i32,
    /// `aiStack_130` — fort ladder level, 0..3 (plus the Roman half-step).
    pub fort_level: i32,
    /// `aiStack_110` — fort border bonus.
    pub fort_terr: i32,
    /// `aiStack_150` — government border bonus.
    pub civic_terr: i32,
    /// `local_b0` — capital + wonder + tribe + CTW bonus, applies to cities and forts alike.
    pub wonder_terr: i32,
    /// `local_d0` — hard radius cap for cities, in tiles.
    pub city_cap: i32,
    /// `aiStack_190` — hard radius cap for forts, in tiles.
    pub fort_cap: i32,
}

/// Ladder helper: `has(c) ? 4 : has(b) ? 3 : has(a) ? 2 : 1`, the exact shape the original
/// uses for both the temple and the fort upgrade chains.
#[inline]
fn ladder(u: [bool; 3]) -> i32 {
    if u[2] {
        4
    } else if u[1] {
        3
    } else if u[0] {
        2
    } else {
        1
    }
}

/// Port of the per-leader precompute pass at the head of `compute_reg_territory`.
///
/// `limit_base` / `limit_civic` / `limit_city` are `World +0x38/0x3C/0x40`
/// (`player_territory_limit*`) when the region has flag bit `4`, else `World +0x44/0x48/0x4C`
/// (`colonized_territory_limit*`). `World::init` seeds **both** triples from
/// `Constants +0x118/0x11C/0x120`.
pub fn leader_border_params(
    l: &LeaderBorderInput,
    c: &TerritoryRules,
    limit_base: i32,
    limit_civic: i32,
    limit_city: i32,
) -> LeaderBorderParams {
    let mut p = LeaderBorderParams::default();
    if !l.active {
        return p;
    }

    // --- temple ladder -----------------------------------------------------
    let tl = ladder(l.temple_upgrade);
    p.temple_level = tl;
    p.temple_terr = c.temple_upgrade_terr[(tl - 1) as usize];
    if l.wonder_tikal {
        let tikal_pct = deviation_behaviour::tikal_border_percent(
            &c.mode,
            c.tikal_temple_borders_pct,
            c.tikal_temple_hp_pct,
        );
        p.temple_terr = ((tikal_pct + 100) * p.temple_terr) / 100;
    }
    if l.ctw_missionaries {
        p.temple_terr = ((c.ctw_missionaries_bonus + 100) * p.temple_terr) / 100;
    }

    // --- fort ladder -------------------------------------------------------
    let fl = ladder(l.fort_upgrade) - 1;
    p.fort_level = fl;
    p.fort_terr = c.fort_upgrade_terr[fl as usize];
    if l.wonder_colosseum {
        p.fort_terr += c.colosseum_fort_borders;
    }
    if l.tribe_roman {
        let r = c.roman_fort_borders;
        p.fort_terr += r;
        p.fort_level = fl + (r + 1) / 2;
    }

    // --- government --------------------------------------------------------
    let gov = l.gov_level.clamp(0, 7);
    p.civic_terr = c.civic_upgrade_terr[gov as usize];
    p.city_cap = limit_base + gov * limit_civic;

    // --- rare resource / wonders / tribe ----------------------------------
    let halve = l.tribe_russian;
    if l.rare_gems {
        let v = c.gems_territory_bonus;
        p.wonder_terr += if halve { v / 2 } else { v };
        p.city_cap += limit_civic;
    }
    if l.wonder_colosseum {
        let v = c.colosseum_territory_bonus;
        p.wonder_terr += if halve { v / 2 } else { v };
        let steps = if halve { 1 } else { v };
        p.city_cap += ((steps + 1) / 2) * limit_civic;
    }
    if l.wonder_eiffel {
        let v = c.eiffel_tower_territory_bonus;
        p.wonder_terr += if halve { v / 2 } else { v };
        let steps = if halve { 1 } else { v };
        p.city_cap += ((steps + 1) / 2) * limit_civic;
    }
    if l.tribe_russian {
        p.wonder_terr += c.russian_borders + c.russian_borders_per_age * l.age;
        p.city_cap += (l.age + i32::from(c.russian_borders != 0)) * limit_civic;
    }
    p.wonder_terr += (l.ctw_raw_bonus + 15) / 25;

    p.fort_cap = p.fort_level * limit_city + p.city_cap;
    p
}

/// A border-emitting object.
#[derive(Clone, Copy, Debug)]
pub enum BorderSourceKind {
    /// A city. `level` is 1/2/3 for small/large/major, decided by the `BuildType` id the
    /// original switches on: `0x19F -> 2`, `0x1A0` or `0x213 -> 3`, anything else `-> 1`.
    City {
        level: i32,
        /// `City` flags bit `0x80` — the city has its temple, so the temple ladder applies.
        has_temple: bool,
        /// `City` flags bit `0x4000`, or the building answers `get_bonus(0x213)` — use
        /// `capital_territory_bonus` instead of `city_level_territory_bonus[level-1]`.
        capital: bool,
    },
    /// A fort. `upgraded` is the building answering `get_bonus(0x216)`, worth a flat `+4`.
    Fort { upgraded: bool },
}

/// One entry of the per-leader city or fort list, as the tile loop sees it.
#[derive(Clone, Copy, Debug)]
pub struct BorderSource {
    pub kind: BorderSourceKind,
    /// The leader slot whose list this entry sits in.
    pub slot: i32,
    /// `City +0x5F` — the leader whose *bonuses* apply. Normally equals `slot`; when it
    /// does not, the claim is recorded as owner `-2`. Forts always use `slot`.
    pub who: i32,
    /// Position in **tiles** (`TCoord`), i.e. `div_3_table[fine >> 6]`.
    pub tile_x: i32,
    pub tile_y: i32,
    /// The entry is alive (`+0x04 & 1` for cities, `+0x06 & 1` for forts).
    pub alive: bool,
}

/// Distance compression applied before the score, verbatim.
///
/// Three nested shrink steps that only bite inside ~3¼ tiles of the source; they make the
/// innermost cells score far lower (= claim far harder) than linear distance would.
#[inline]
pub fn compress_distance(mut d: u32) -> u32 {
    if d < 13 {
        d = (d * 2) / 3;
    }
    if d < 9 {
        d = (d * 2) / 3;
    }
    if d < 5 {
        d /= 2;
    }
    d
}

/// The border score. **Lower wins**; a claim is only admissible while
/// `score <= territory_base << 8`.
///
/// ```text
/// score = (territory_den * d * 256) / (bonuses * multiplier + territory_num)
/// ```
///
/// With the shipped constants that is `(5 * d * 256) / (4*B + 11)`, and the admissibility
/// gate `score <= 24 * 256` rearranges to `d <= 24 * (4*B + 11) / 5` tiles — which is
/// exactly the `rules.xml` comment on `territory_num`,
/// *"(Numerator + (CityorFortMultiplier * BorderBonuses)) / Denominator"*.
#[inline]
pub fn border_score(d: u32, bonuses: i32, multiplier: i32, c: &TerritoryRules) -> i32 {
    let num = c.territory_den * d as i32 * 256;
    let den = bonuses * multiplier + c.territory_num;
    num / den
}

/// The winner and runner-up for one tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileClaim {
    /// Written to `WData::who`. `-1` unclaimed, `-2` claimed by a source whose `who` field
    /// disagreed with its slot.
    pub who: i8,
    /// Written to `WData::who2`.
    pub who2: i8,
    /// Index into the winning leader's city list, or `-1` if a fort won. The original uses
    /// it to mark the contested-border flag on that city.
    pub best_city_index: i32,
}

/// Port of the per-tile body of `World::compute_reg_territory` `0x006B0BB0`.
///
/// `sources` must be grouped by `slot`; the function walks the eight slots in the order
/// `(k + wx) & 7` for `k in 0..8` — **the leader scan order rotates with the tile's x
/// coordinate**, which is what breaks ties deterministically without an RNG draw. That
/// rotation is as load-bearing as `Objects::process_all`'s `(frame + i) % 10`.
///
/// `wx`/`wy` are WCoord indices; the tile-space position used for distances is
/// `wx * 4 + 2`, `wy * 4 + 2` — the centre tile of the 4×4 tile block.
pub fn claim_tile(
    wx: i32,
    wy: i32,
    slots: &[Vec<BorderSource>; 8],
    params: &[LeaderBorderParams; 8],
    active: &[bool; 8],
    c: &TerritoryRules,
) -> TileClaim {
    let tx = wx * 4 + 2;
    let ty = wy * 4 + 2;
    let limit = c.territory_base << 8;

    let mut best_score = 999_999_999i32;
    let mut second_score = 999_999_999i32;
    let mut best_who: i32 = -1;
    let mut second_who: i32 = -1;
    let mut best_city: i32 = -1;

    for k in 0..8i32 {
        let slot = rem8_signed(k + wx);
        let si = slot as usize;
        if !active[si] {
            continue;
        }
        let p = &params[si];

        for (ci, src) in slots[si].iter().enumerate() {
            if !src.alive {
                continue;
            }
            let dx = (tx - src.tile_x).unsigned_abs();
            let dy = (ty - src.tile_y).unsigned_abs();
            let raw = hypot_approx(dx, dy);

            match src.kind {
                BorderSourceKind::City {
                    level,
                    has_temple,
                    capital,
                } => {
                    let cap = p.city_cap
                        + (level - 1) * c.territory_limit_city
                        + if has_temple {
                            p.temple_level * c.territory_limit_city
                        } else {
                            0
                        };
                    if raw as i32 > cap {
                        continue;
                    }
                    let d = compress_distance(raw);

                    // Whose bonus table applies: the city's own `who`, not the slot.
                    let bp = if src.who == slot {
                        p
                    } else {
                        &params[src.who.clamp(0, 7) as usize]
                    };
                    let mut bonuses = bp.civic_terr;
                    if has_temple {
                        bonuses += bp.temple_terr;
                    }
                    let level_bonus = if capital {
                        c.capital_territory_bonus
                    } else {
                        c.city_level_territory_bonus[(level - 1).clamp(0, 2) as usize]
                    };
                    bonuses += level_bonus + bp.wonder_terr;

                    let s = border_score(d, bonuses, c.city_territory_multiplier, c);
                    let claimant: i32 = if src.who == slot { slot } else { -2 };

                    if s > limit || s >= best_score {
                        if s < second_score {
                            second_score = s;
                            second_who = claimant;
                        }
                    } else {
                        if best_who != -1 {
                            second_score = best_score;
                            second_who = best_who;
                        }
                        best_score = s;
                        best_city = ci as i32;
                        best_who = claimant;
                    }
                }

                BorderSourceKind::Fort { upgraded } => {
                    if raw as i32 > p.fort_cap {
                        continue;
                    }
                    let d = compress_distance(raw);
                    let mut bonuses = p.fort_terr + p.civic_terr;
                    if upgraded {
                        bonuses += 4;
                    }
                    let s =
                        border_score(d, bonuses + p.wonder_terr, c.fort_territory_multiplier, c);

                    if s > limit || s >= best_score {
                        if s < second_score {
                            second_score = s;
                            second_who = slot;
                        }
                    } else {
                        second_score = best_score;
                        second_who = best_who;
                        best_score = s;
                        best_city = -1;
                        best_who = slot;
                    }
                }
            }
        }
    }

    TileClaim {
        who: best_who as i8,
        who2: second_who as i8,
        best_city_index: best_city,
    }
}

// ---------------------------------------------------------------------------
// 5b. The incremental driver — GameDaemon::check_borders 0x00732060
// ---------------------------------------------------------------------------

/// Per-frame budget: `GameDaemon +0x24` is reset to 0 at the top of `check_borders` and
/// `compute_reg_territory` bails as soon as it reaches `0x100`. So **at most 256 territory
/// tiles are resolved per sim frame, across all regions**, and a large map's borders take
/// many frames to settle after a city is founded or destroyed. `[measured]`
pub const TERRITORY_TILES_PER_FRAME: i32 = 0x100;

/// One region's border cursor. Mirrors `Region` (`schema/pdb-types.json`, 136 bytes):
/// `+0x08 flags`, `+0x14 size`, `+0x2C borders`, `+0x6C coords : WCoordList`.
#[derive(Clone, Debug, Default)]
pub struct RegionBorderState {
    /// `Region +0x08`. Bit `0x04` selects the `player_*` limit triple over the
    /// `colonized_*` one; bit `0x20`/`0x80` are set when a pass completes.
    pub flags: u32,
    /// `Region +0x14` — number of WCoord cells in the region.
    pub size: i32,
    /// `Region +0x2C` — the resume cursor. `size > borders` means "work outstanding".
    pub borders: i32,
    /// `Region +0x6C` — the region's cell list, `(wx, wy)` pairs.
    pub coords: Vec<(i32, i32)>,
}

impl RegionBorderState {
    /// `region.size > region.borders` — the exact test `check_borders` uses to decide a
    /// region needs work.
    #[inline]
    pub fn dirty(&self) -> bool {
        self.size > self.borders
    }
}

/// Port of `GameDaemon::check_borders` `0x00732060` + the outer loop of
/// `World::compute_reg_territory`.
///
/// Walks regions in index order, resolving cells from each region's cursor until the global
/// budget runs out. Returns the number of tiles resolved.
///
/// **This is the writer of `WData::who` / `who2`, section 5 of the `world` channel.** It
/// takes the `World` rather than a private plane, and the two territory-limit triples come
/// from `World +0x38..0x4c` — the same dwords section 4 of the checksum walks — instead of
/// being passed in beside it. A caller can no longer hand it limits that differ from the
/// ones on the wire.
pub fn check_borders(
    regions: &mut [RegionBorderState],
    world: &mut World,
    slots: &[Vec<BorderSource>; 8],
    inputs: &[LeaderBorderInput; 8],
    c: &TerritoryRules,
) -> i32 {
    let mut active = [false; 8];
    for (i, l) in inputs.iter().enumerate() {
        active[i] = l.active;
    }

    // `World +0x38/0x3C/0x40` and `+0x44/0x48/0x4C`.
    let player_limits = (
        world.player_territory_limit,
        world.player_territory_limit_civic,
        world.player_territory_limit_city,
    );
    let colonized_limits = (
        world.colonized_territory_limit,
        world.colonized_territory_limit_civic,
        world.colonized_territory_limit_city,
    );

    let mut budget = 0i32;
    for r in regions.iter_mut() {
        if !r.dirty() {
            continue;
        }
        let (b, cv, ct) = if r.flags & 4 != 0 {
            player_limits
        } else {
            colonized_limits
        };
        let mut params = [LeaderBorderParams::default(); 8];
        for i in 0..8 {
            params[i] = leader_border_params(&inputs[i], c, b, cv, ct);
        }

        while r.dirty() {
            if budget >= TERRITORY_TILES_PER_FRAME {
                return budget;
            }
            budget += 1;
            let cursor = r.borders as usize;
            r.borders += 1;
            let Some(&(wx, wy)) = r.coords.get(cursor) else {
                continue;
            };
            let claim = claim_tile(wx, wy, slots, &params, &active, c);
            let i = world.w_index(wx, wy);
            world.wdata[i].who = claim.who;
            world.wdata[i].who2 = claim.who2;
        }
        r.flags |= 0x80 | 0x20;
    }
    budget
}

// ---------------------------------------------------------------------------
// 5c. The ownership queries everything else depends on
// ---------------------------------------------------------------------------

/// `WorldData::get_who(WCoord, WCoord)` `0x006B4700` (and `get_whose` `0x006B4D80`, which
/// is byte-identical). Re-exported from the storage owner.
#[inline]
pub fn get_who(w: &World, wx: i32, wy: i32) -> i32 {
    w.get_who(wx, wy)
}

/// `WorldData::get_who2(WCoord, WCoord)` `0x006B2510`.
#[inline]
pub fn get_who2(w: &World, wx: i32, wy: i32) -> i32 {
    w.get_who2(wx, wy)
}

/// The diplomacy facts `is_enemy_territory` needs. `team[p]` is `LeaderData +0x08`;
/// `diplo[a][b]` is `LeaderData +0x74 + b*4` (value `2` = allied).
#[derive(Clone, Copy, Debug, Default)]
pub struct Diplomacy {
    pub team: [i32; 8],
    pub diplo: [[i32; 8]; 8],
}

/// `WorldData::is_enemy_territory(WCoord, WCoord, int player)` `0x006B2490`, and the same
/// predicate as `WallData::in_unfriendly_territory` `0x0063ECA0`.
///
/// Unowned (`who < 0`) is never enemy; own is never enemy; a tile owned by your team is
/// never enemy; and a **mutual** alliance (`diplo[who][me] == 2 && diplo[me][team[who]] == 2`)
/// is never enemy. Everything else is.
pub fn is_enemy_territory(w: &World, d: &Diplomacy, wx: i32, wy: i32, player: i32) -> bool {
    let who = w.get_who(wx, wy);
    if who < 0 || who == player {
        return false;
    }
    if who == d.team[player as usize] {
        return false;
    }
    let mutual = d.diplo[player as usize][who as usize] == 2
        && d.diplo[who as usize][d.team[player as usize] as usize] == 2;
    !mutual
}

// ---------------------------------------------------------------------------
// 6. Supply and attrition
// ---------------------------------------------------------------------------

/// Supply and attrition rule constants, keyed by `Constants` byte offset.
#[derive(Clone, Copy, Debug)]
pub struct AttritionRules {
    /// `+0xCAC` — supply radius, tiles. `14`
    pub supply_radius: i32,
    /// `+0xCB0` — supply radius per upgrade, tiles. `2`
    pub supply_radius_upgrade: i32,
    /// `+0xCB4`, 4 entries — supply hit-point upgrade ladder. `{0, 20, 40, 60}`
    pub supply_hp_upgrade: [i32; 4],
    /// `+0xCC8`, `String::fraction` scale 256 — siege out-of-supply reload. `384` = 3/2.
    pub siege_out_of_supply_reload_256: i32,
    /// `+0xCCC`, scale 256 — artillery out-of-supply reload. `512` = 2/1.
    pub artillery_out_of_supply_reload_256: i32,
    /// `+0xBF8` — supply heal rate, frames. `0` = do not heal.
    pub supply_heal_rate: i32,
    /// `+0xD34` — border-violation attrition period while at peace, frames. `8`
    pub peace_attrition: i32,
    /// `+0xD38` — assassin attrition period, frames. `8`
    pub assassin_attrition: i32,
    /// `+0xD3C` — the baseline attrition period, frames. `48`
    pub attrition: i32,
    /// `+0x1E8` — siege attrition reduction, percent. `50`
    pub siege_attrition: i32,
    /// `+0x1EC` — militia attrition increase, percent. `300`
    pub militia_attrition: i32,
    /// `+0x1F0` — attrition increase per age of advantage, percent. `25`
    pub attrition_aged_up: i32,
    /// `+0x1C8`, 4 entries — attrition-received reduction ladder, percent.
    /// `{25, 50, 75, 100}`
    pub attrition_upgrade: [i32; 4],
    /// `+0x1D8`, 4 entries — attrition-dealt ladder. `{1, 2, 4, 8}`
    pub attrition_improved: [i32; 4],
}

impl Default for AttritionRules {
    fn default() -> Self {
        Self {
            supply_radius: 14,
            supply_radius_upgrade: 2,
            supply_hp_upgrade: [0, 20, 40, 60],
            siege_out_of_supply_reload_256: 384,
            artillery_out_of_supply_reload_256: 512,
            supply_heal_rate: 0,
            peace_attrition: 8,
            assassin_attrition: 8,
            attrition: 48,
            siege_attrition: 50,
            militia_attrition: 300,
            attrition_aged_up: 25,
            attrition_upgrade: [25, 50, 75, 100],
            attrition_improved: [1, 2, 4, 8],
        }
    }
}

/// Inputs to `UnitData::get_attrition(int by_player)` `0x00608FD0`.
#[derive(Clone, Copy, Debug)]
pub struct AttritionInput {
    /// `LeaderData +0x7F0` of the player whose territory the unit stands in — the attrition
    /// *rate*. Zero disables attrition entirely.
    pub attacker_attrition: i32,
    /// `LeaderData +0x7F4` of the unit's owner — `anti_att`. **A float**, and one of the
    /// three documented floats inside walked state.
    ///
    /// It is **256-scaled**: [`calc_anti_attrition`] seeds it with `0x43800000` = `256.0f`,
    /// so `256.0` means "no anti-attrition". `0.0` means total immunity.
    pub victim_anti_att: f32,
    /// The unit type answers the `+0x10C` virtual — the siege-reduction class.
    pub siege_class: bool,
    /// The object answers `get_bonus(0x42)` — the militia class.
    pub militia: bool,
    /// `ObjectType +0x04` — the type id. `0x3D`, `0x3E` and `400` halve the result.
    pub type_id: i32,
    /// `ObjectType +0x218`. `2` halves the result again; `1` suppresses attrition; `0` is
    /// the normal unit path.
    pub type_class: i32,
    /// `attacker.age - victim.age`, clamped at the call site to `>= 0` before use.
    pub age_diff: i32,
}

/// The `anti_att` value meaning "no anti-attrition at all": the immediate
/// `0x43800000` = `256.0f` that `Leader::calc_anti_attrition` `0x006CDCC0` starts from.
pub const ANTI_ATT_BASE: f32 = 256.0;

/// The unit's `anti_att`, `Leader::calc_anti_attrition` `0x006CDCC0`.
///
/// `[measured]` from the instruction stream. The base is the immediate `0x43800000` =
/// **256.0f**, and each source of anti-attrition divides the *received* rate by
/// `(100 - pct) / 100`. A `pct >= 100` anywhere zeroes the field outright, which makes the
/// unit completely immune (`get_attrition` then returns 0 and no period is ever set) —
/// that is how `liberty_attrition` (`Constants +0x524`, "100% reduction of attrition
/// received", the Statue of Liberty) works.
///
/// * `upgrade_level`: `has_preq(0x300) -> 2`, else `(0x2FF) -> 1`, else `(0x2FE) -> 0`,
///   else no upgrade. Indexes `Constants::attrition_upgrade` `+0x1C8` = `{25,50,75,100}`.
/// * `wonder_liberty`: `has_wonder(0x219)`, `Constants +0x524`.
/// * `tribe_mongol`: `has_tribe_bonus(0x11)`, `Constants::mongol_attrition +0x7F0` = 50.
/// * `titanium`: rare-resource flag, `Constants::titanium_attrition +0x95C` = 50.
pub fn calc_anti_attrition(
    upgrade_level: Option<usize>,
    wonder_liberty: bool,
    tribe_mongol: bool,
    titanium: bool,
    disabled: bool,
    attrition_upgrade: [i32; 4],
    liberty_pct: i32,
    mongol_pct: i32,
    titanium_pct: i32,
) -> f32 {
    if disabled {
        return 0.0;
    }
    let mut v = ANTI_ATT_BASE;
    let apply = |pct: i32, v: &mut f32| -> bool {
        if pct > 99 {
            *v = 0.0;
            return false;
        }
        *v = (*v * 100.0) / (100 - pct) as f32;
        true
    };
    if let Some(lv) = upgrade_level {
        if !apply(attrition_upgrade[lv], &mut v) {
            return 0.0;
        }
    }
    if wonder_liberty && !apply(liberty_pct, &mut v) {
        return 0.0;
    }
    if tribe_mongol && !apply(mongol_pct, &mut v) {
        return 0.0;
    }
    if titanium && !apply(titanium_pct, &mut v) {
        return 0.0;
    }
    v
}

/// Port of `UnitData::get_attrition(int)` `0x00608FD0`.
///
/// Returns a **period-ish scalar**, not a damage number: the caller multiplies it by
/// `Constants::attrition` and shifts right 8 to get the frame period between attrition
/// ticks, so a larger return means slower attrition.
///
/// The one floating-point step is `(int)((float)scale * anti_att * K)` with `K` the
/// `binary32` at `0x00B69430` = `0x3B800000` = `1/256` `[measured]`, and the instruction
/// stream is `cvtdq2ps / mulss [leader+0x7F4] / mulss [0xB69430] / cvttss2si`, i.e. exactly
/// that association order. Since `anti_att` is itself 256-scaled, the two 256s cancel and a
/// baseline unit gets `v = scale = 256`. Kept in `f32` deliberately: `anti_att` is float in
/// the engine's own walked state, so rounding it away here would be a silent fidelity loss.
pub fn get_attrition(inp: &AttritionInput, c: &AttritionRules) -> i32 {
    let mut att = inp.attacker_attrition;
    if att == 0 || c.attrition == 0 {
        return 0;
    }
    let mut scale = 0x100i32;
    if inp.siege_class {
        if c.siege_attrition >= 100 {
            return 0;
        }
        scale = 25_600 / (100 - c.siege_attrition);
    }
    let mut v = if !inp.militia {
        (scale as f32 * inp.victim_anti_att * 0.003_906_25f32) as i32
    } else {
        (scale * 100) / (c.militia_attrition + 100)
    };
    if inp.type_id == 0x3D || inp.type_id == 0x3E || inp.type_id == 400 {
        v /= 2;
    }
    if inp.type_class == 2 {
        v /= 2;
    }
    if inp.age_diff >= 0 {
        att = ((c.attrition_aged_up * inp.age_diff + 100) * att + 99) / 100;
    }
    v / att
}

/// The tail of `Unit::process_attrition` `0x005E11A0`: turn `get_attrition`'s scalar into
/// the unit's attrition period in frames (`Unit +0x9E`, an `i16`).
///
/// ```text
/// p = (attrition * v + ((attrition * v >> 31) & 0xFF)) >> 8      // trunc-toward-zero /256
/// period = max(p, 1)
/// ```
///
/// The odd `+ (x >> 31 & 0xFF)` is MSVC's signed division by 256. With the shipped
/// `attrition = 48` and a `v` of 256 (no anti-attrition, rate 1) the period is 48 frames —
/// 3.2 game seconds, since 15 frames is one game second.
pub fn attrition_period(v: i32, c: &AttritionRules) -> Option<i16> {
    if v == 0 {
        return None;
    }
    let x = c.attrition.wrapping_mul(v);
    let p = (x + ((x >> 31) & 0xFF)) >> 8;
    Some(p.max(1) as i16)
}

/// `Unit::process_attrition` also writes a fixed period for the two special sources:
/// `assassin_attrition` (8) and, at peace, `peace_attrition` (8) — halved when
/// `ObjectType +0x218 == 2`.
#[inline]
pub fn special_attrition_period(base_frames: i32, type_class: i32) -> i16 {
    if type_class == 2 {
        (base_frames / 2) as i16
    } else {
        base_frames as i16
    }
}

/// The firing test in `Unit::process` `0x00610BC0`:
/// `period != 0 && (Game::frame + unit_id) % period == 0`.
///
/// The `+ unit_id` phase offset is what stops every unit in a stack taking attrition on the
/// same frame; it is deterministic and must be reproduced exactly.
#[inline]
pub fn attrition_due(frame: i32, unit_id: i16, period: i16) -> bool {
    period != 0 && (frame + unit_id as i32) % period as i32 == 0
}

/// What `Unit::suffer_attrition` `0x005E1A10` passes to `Object::take_damage`.
///
/// `Object::take_damage(int damage, char a, char b, int, int, unsigned long, int, int, int)`.
/// Attrition uses two distinct shapes:
///
/// * `ObjectType +0x308 == 1` → `take_damage(1, 0, …)` — a flat point of damage.
/// * otherwise → `take_damage(0, n, …)` where `n = 6` if `UnitData::curr_uber_size()` is 3,
///   else `16 / curr_uber_size()`.
///
/// The `int damage` slot being 0 in the common case says the `char` argument is a
/// *fractional* damage form, not a flat one. What that fraction is denominated in is
/// `Object::take_damage`'s business and is **not derived here**.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttritionDamage {
    pub flat: i32,
    pub fractional: i8,
}

/// Port of the damage selection in `Unit::suffer_attrition`.
pub fn attrition_damage(type_308: i32, curr_uber_size: i32) -> AttritionDamage {
    if type_308 == 1 {
        AttritionDamage {
            flat: 1,
            fractional: 0,
        }
    } else {
        let n = if curr_uber_size == 3 {
            6
        } else if curr_uber_size != 0 {
            16 / curr_uber_size
        } else {
            0
        };
        AttritionDamage {
            flat: 0,
            fractional: n as i8,
        }
    }
}

/// Inputs to `Unit::process_supply` `0x005E0560`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SupplyInput {
    /// `Unit +0x68 & 0x400000` — already resolved this frame, skip.
    pub already_flagged: bool,
    /// `ObjectType +0x2B8 & 0x40` — this type is never out of supply (air units and such).
    pub always_supplied: bool,
    /// The object answers `get_bonus(0x42)` — militia are exempt.
    pub militia: bool,
    /// `Supplies::find_supply(x, y, owner) >= 0` — inside a supply-wagon / city radius.
    /// **Not ported**; the caller supplies the answer.
    pub near_supply_source: bool,
    /// The owner has at least one of building type `0x16B` and one is in range.
    pub near_building_16b: bool,
    /// … type `0x176`.
    pub near_building_176: bool,
    /// … type `0x16E`.
    pub near_building_16e: bool,
}

/// Port of `Unit::process_supply` `0x005E0560`. `true` = in supply.
///
/// Reproduces the short-circuit order exactly, because the engine's own order decides which
/// of the three building queries even runs.
pub fn supply_state(s: &SupplyInput) -> bool {
    if s.already_flagged || s.always_supplied || s.militia {
        return false;
    }
    s.near_supply_source || s.near_building_16b || s.near_building_176 || s.near_building_16e
}

/// The whole attrition step for one unit, as `Unit::process` sequences it:
/// attrition only lands when the unit is **out of supply**.
///
/// ```text
/// if period != 0 && (frame + id) % period == 0:
///     if !process_supply():  suffer_attrition()
///     else:                  unit.flags2 |= 0x40000     // "resupplied this tick"
/// ```
///
/// Returns `Some(damage)` when attrition fires, `None` otherwise.
pub fn step_attrition(
    frame: i32,
    unit_id: i16,
    period: i16,
    supply: &SupplyInput,
    type_308: i32,
    curr_uber_size: i32,
) -> Option<AttritionDamage> {
    if !attrition_due(frame, unit_id, period) {
        return None;
    }
    if supply_state(supply) {
        return None;
    }
    Some(attrition_damage(type_308, curr_uber_size))
}

/// Out-of-supply reload multiplier, `String::fraction` scale 256.
/// Apply as `delay = (delay * mult) / 256`.
///
/// The constants are `[measured]`; **the call site that applies them is not located**, so
/// nothing here calls this yet.
#[inline]
pub fn out_of_supply_reload(base_delay: i32, mult_256: i32) -> i32 {
    (base_delay * mult_256) / 256
}

// ---------------------------------------------------------------------------
// 7. The `world` checksum channel — owned by `map_terrain`
// ---------------------------------------------------------------------------

// `adler32`, `WorldWalkSection`, `WorldScalars`, `WorldChecksum` and `world_checksum` were
// here. They are gone, and nothing in this module hashes anything any more.
//
//   * the primitive is `crate::checksum::adler32`, the crate's only implementation;
//   * the walker is `map_terrain::World::walk_section`, all thirteen sections;
//   * the per-section digest this module invented, which was its one thing `map_terrain`
//     lacked, is `map_terrain::World::checksum_sections` -> `WorldChecksum`.
//
// The scalar block this module called `WorldScalars` is not a separate struct: those
// twenty dwords are `World +0x30 .. +0x7c`, walked as one contiguous 120-byte range with
// the ten derived sizes ahead of them. `check_borders` above now reads the six territory
// limits straight out of the same fields the checksum walks.

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::map_terrain;

    /// `World::init` grid arithmetic, now read off the one `World` both lanes share.
    #[test]
    fn grids_match_world_init() {
        let g = World::init_default_rules(64, 64);
        assert_eq!((g.xs, g.ys, g.size), (64, 64, 4096));
        assert_eq!((g.fog_xs, g.fog_ys, g.fog_size), (128, 128, 16384));
        assert_eq!((g.tile_xs, g.tile_ys, g.tile_size), (256, 256, 65536));
        assert_eq!((g.reg_xs, g.reg_ys, g.reg_size), (32, 32, 1024));
        // one WCoord cell is 4 tiles, one fog cell is 2 tiles, one region is 8 tiles
        assert_eq!(g.tile_xs / g.xs, 4);
        assert_eq!(g.tile_xs / g.fog_xs, 2);
        assert_eq!(g.tile_xs / g.reg_xs, 8);
    }

    #[test]
    fn coordinate_ladder_is_consistent() {
        // 1 tile = 192 fine, 1 fog = 384, 1 wcoord = 768.
        for tile in 0..64i32 {
            let fine = tile * FINE_PER_TILE;
            assert_eq!(fine_to_tile(fine), tile);
            assert_eq!(fine_to_fog(fine), tile / 2);
            assert_eq!(fine_to_wcoord(fine), tile / 4);
        }
    }

    #[test]
    fn deobfuscation_roundtrips() {
        for v in [0i32, 1, 192, 768, 100_000] {
            let stored = (v as u32) ^ COORD_XOR;
            assert_eq!(deobf(stored), v);
        }
    }

    #[test]
    fn hypot_approx_is_symmetric_and_close_to_euclid() {
        for a in 0..80u32 {
            for b in 0..80u32 {
                assert_eq!(hypot_approx(a, b), hypot_approx(b, a));
            }
        }
        // The Taylor form is exact on the axes and never below the true value.
        for a in 0..200u32 {
            assert_eq!(hypot_approx(a, 0), a);
            let d = hypot_approx(a, a) as f64;
            let real = (2.0 * (a as f64) * (a as f64)).sqrt();
            assert!(d >= real - 1.0, "a={a} approx={d} real={real}");
            assert!(d <= real * 1.07 + 1.0, "a={a} approx={d} real={real}");
        }
    }

    #[test]
    fn rem8_matches_mask_for_non_negative() {
        for v in 0..100i32 {
            assert_eq!(rem8_signed(v), v & 7);
        }
    }

    #[test]
    fn circle_table_is_a_disc() {
        let c = CircleTable::build();
        assert_eq!(c.x.len(), c.y.len());
        assert_eq!(c.radius[0], 1, "radius 0 is the single centre cell");
        assert_eq!((c.x[0], c.y[0]), (0, 0));
        // monotone cumulative counts
        for r in 1..=CIRCLE_MAX_R {
            assert!(c.radius[r] >= c.radius[r - 1], "radius {r} not monotone");
        }
        // every entry in ring r has hypot_approx == r exactly
        for r in 0..=CIRCLE_MAX_R {
            let lo = if r == 0 { 0 } else { c.radius[r - 1] } as usize;
            let hi = c.radius[r] as usize;
            for k in lo..hi {
                let d = hypot_approx(c.x[k].unsigned_abs() as u32, c.y[k].unsigned_abs() as u32);
                assert_eq!(d, r as u32, "entry {k} in ring {r}");
            }
        }
        // the full disc is close to pi*r^2
        let area = c.radius[CIRCLE_MAX_R] as f64;
        let expect = std::f64::consts::PI * 64.0 * 64.0;
        assert!(
            (area - expect).abs() / expect < 0.05,
            "disc area {area} vs pi r^2 {expect}"
        );
    }

    #[test]
    fn circle_table_has_no_duplicates() {
        let c = CircleTable::build();
        let mut seen = std::collections::HashSet::new();
        for k in 0..c.x.len() {
            assert!(seen.insert((c.x[k], c.y[k])), "duplicate offset at {k}");
        }
    }

    #[test]
    fn los_radius_conversion() {
        assert_eq!(los_to_fog_radius(0), 0);
        assert_eq!(los_to_fog_radius(4), 2); // 4-tile LOS -> 2 fog cells -> 4 tiles across
        assert_eq!(los_to_fog_radius(20), 10);
        assert_eq!(los_to_fog_radius(1000), CIRCLE_MAX_R as i32);
    }

    /// A 16x16-WCoord world plus the fog policy. The planes are the `World`'s — that is
    /// the whole point of the reconciliation, so the tests below assert against the same
    /// bytes `World::checksum()` walks.
    fn tiny_fog() -> (Fog, World, CircleTable) {
        let mut fog = Fog::new();
        for (i, l) in fog.leaders.iter_mut().enumerate() {
            l.player_mask = 1u8 << i;
        }
        (fog, World::init_default_rules(16, 16), CircleTable::build())
    }

    #[test]
    fn set_seen_writes_all_four_planes_and_reports_new_exploration() {
        let (fog, mut w, _) = tiny_fog();
        let (fx, fy) = (10, 10);
        assert!(fog.set_seen(&mut w, fx, fy, 2, true), "first touch is new");
        assert!(
            !fog.set_seen(&mut w, fx, fy, 2, true),
            "second touch is not"
        );

        let fi = w.f_index(fx, fy);
        let wi = w.w_index(fx >> 1, fy >> 1);
        assert_eq!(w.seen[fi], 0b100);
        assert_eq!(w.seen2[fi], 0b100);
        assert_eq!(w.seen3[fi], 0b100);
        assert_eq!(w.wcoord_seen[wi], 0b100);
        assert_eq!(w.wdata[wi].was_seen, 0b100);
    }

    #[test]
    fn clear_seen_drops_visibility_but_keeps_exploration() {
        let (fog, mut w, _) = tiny_fog();
        fog.set_seen(&mut w, 4, 4, 0, false);
        fog.begin_frame(&mut w);
        assert!(!fog.is_really_seen(&w, 4, 4, 0));
        assert!(fog.was_really_seen(&w, 4, 4, 0));
        // and the coarse plane went with the visible one
        let wi = w.w_index(2, 2);
        assert_eq!(w.wcoord_seen[wi], 0);
        assert_ne!(w.wdata[wi].was_seen, 0);
    }

    #[test]
    fn detection_plane_is_separate_from_visibility() {
        let (fog, mut w, _) = tiny_fog();
        fog.set_seen(&mut w, 6, 6, 1, false);
        assert!(fog.is_really_seen(&w, 6, 6, 1));
        assert!(!fog.is_detected(&w, 6, 6, 1));
        fog.set_seen(&mut w, 6, 6, 1, true);
        assert!(fog.is_detected(&w, 6, 6, 1));
        assert!(fog.is_detected_by_enemy(&w, 6, 6, 0));
        assert!(!fog.is_detected_by_enemy(&w, 6, 6, 1));
    }

    #[test]
    fn see_all_short_circuits_but_detection_does_not() {
        let (mut fog, w, _) = tiny_fog();
        fog.leaders[3].see_all = true;
        assert!(fog.is_really_seen(&w, 0, 0, 3));
        assert!(fog.was_really_seen(&w, 0, 0, 3));
        assert!(!fog.is_detected(&w, 0, 0, 3), "see_all grants no detection");
    }

    #[test]
    fn update_seen_stamps_the_disc() {
        let (fog, mut w, circle) = tiny_fog();
        let obj = SeeingObject {
            fine_x: 16 * FINE_PER_FOG,
            fine_y: 16 * FINE_PER_FOG,
            owner: 0,
            los_tiles: 8, // -> radius 4 fog cells
            detector: false,
            grant_seen2_to: 0,
        };
        let mut newly = Vec::new();
        update_seen(&fog, &mut w, &circle, &obj, &mut newly);

        assert_eq!(newly.len(), circle.radius[4] as usize);
        assert!(fog.is_really_seen(&w, 16, 16, 0));
        assert!(fog.is_really_seen(&w, 20, 16, 0), "on the rim");
        assert!(!fog.is_really_seen(&w, 22, 16, 0), "outside the rim");
        assert!(!fog.is_detected(&w, 16, 16, 0), "not a detector");

        // idempotent: a second pass reports nothing newly explored
        let mut again = Vec::new();
        update_seen(&fog, &mut w, &circle, &obj, &mut again);
        assert!(again.is_empty());
    }

    #[test]
    fn detector_stamps_the_whole_disc_into_seen3() {
        let (fog, mut w, circle) = tiny_fog();
        let obj = SeeingObject {
            fine_x: 16 * FINE_PER_FOG,
            fine_y: 16 * FINE_PER_FOG,
            owner: 1,
            los_tiles: 6,
            detector: true,
            grant_seen2_to: 0,
        };
        update_seen(&fog, &mut w, &circle, &obj, &mut Vec::new());
        assert!(fog.is_detected(&w, 16, 16, 1));
        assert!(fog.is_detected(&w, 19, 16, 1));
        assert!(!fog.is_detected(&w, 24, 16, 1));
    }

    #[test]
    fn circle_init_truncates_its_own_outermost_ring() {
        // The engine's guard is `if (count > 0x3248) { fill radius[r..=64]; return; }`, and
        // at r = 64 it fires part-way through the ring: 392 offsets qualify, 300 fit. So a
        // maximum-radius LOS disc is *clipped* in the retail engine, and reproducing that
        // clip is part of matching it.
        let c = CircleTable::build();
        assert_eq!(c.x.len(), CIRCLE_CAP + 1);
        let mut natural = 0;
        for oy in -64i32..=64 {
            for ox in -64i32..=64 {
                if hypot_approx(ox.unsigned_abs(), oy.unsigned_abs()) == 64 {
                    natural += 1;
                }
            }
        }
        assert_eq!(natural, 392);
        assert_eq!(c.radius[64] - c.radius[63], 300);
    }

    #[test]
    fn update_seen_clips_at_the_map_edge() {
        let (fog, mut w, circle) = tiny_fog();
        let obj = SeeingObject {
            fine_x: 0,
            fine_y: 0,
            owner: 0,
            los_tiles: 8,
            detector: false,
            grant_seen2_to: 0,
        };
        // must not panic and must not wrap onto the far edge
        update_seen(&fog, &mut w, &circle, &obj, &mut Vec::new());
        assert!(fog.is_really_seen(&w, 0, 0, 0));
        let far = w.fog_xs - 1;
        assert!(!fog.is_really_seen(&w, far, 0, 0));
    }

    // --- territory ---------------------------------------------------------

    fn plain_leader() -> LeaderBorderInput {
        LeaderBorderInput {
            active: true,
            ..Default::default()
        }
    }

    #[test]
    fn leader_params_baseline() {
        let c = TerritoryRules::default();
        let p = leader_border_params(&plain_leader(), &c, 44, 4, 4);
        assert_eq!(p.temple_level, 1);
        assert_eq!(p.temple_terr, c.temple_upgrade_terr[0]); // 2
        assert_eq!(p.fort_level, 0);
        assert_eq!(p.fort_terr, c.fort_upgrade_terr[0]); // 2
        assert_eq!(p.civic_terr, 0);
        assert_eq!(p.wonder_terr, 0);
        assert_eq!(p.city_cap, 44);
        assert_eq!(p.fort_cap, 44);
    }

    #[test]
    fn leader_params_full_ladder() {
        let c = TerritoryRules::default();
        let l = LeaderBorderInput {
            active: true,
            temple_upgrade: [true, true, true],
            fort_upgrade: [true, true, true],
            gov_level: 7,
            rare_gems: true,
            ..Default::default()
        };
        let p = leader_border_params(&l, &c, 44, 4, 4);
        assert_eq!(p.temple_level, 4);
        assert_eq!(p.temple_terr, 9);
        assert_eq!(p.fort_level, 3);
        assert_eq!(p.fort_terr, 9);
        assert_eq!(p.civic_terr, 14);
        assert_eq!(p.wonder_terr, 2); // gems
        assert_eq!(p.city_cap, 44 + 7 * 4 + 4);
        assert_eq!(p.fort_cap, 3 * 4 + p.city_cap);
    }

    #[test]
    fn tikal_scales_the_temple_bonus_by_half() {
        let c = TerritoryRules::default();
        let mut l = plain_leader();
        l.temple_upgrade = [true, true, false]; // level 3 -> 6
        let base = leader_border_params(&l, &c, 44, 4, 4).temple_terr;
        l.wonder_tikal = true;
        let with = leader_border_params(&l, &c, 44, 4, 4).temple_terr;
        assert_eq!(base, 6);
        assert_eq!(with, (150 * 6) / 100);
    }

    #[test]
    fn tikal_rule_selection_is_mode_gated_on_the_real_border_path() {
        let mut l = plain_leader();
        l.temple_upgrade = [true, true, false]; // level 3 -> 6
        l.wonder_tikal = true;

        let mut c = TerritoryRules {
            tikal_temple_borders_pct: 90,
            tikal_temple_hp_pct: 50,
            ..Default::default()
        };
        assert_eq!(
            leader_border_params(&l, &c, 44, 4, 4).temple_terr,
            9,
            "fidelity reads TIKAL_TEMPLE_HP: 6 * 150%"
        );

        c.mode = ModeConfig::improved();
        assert_eq!(
            leader_border_params(&l, &c, 44, 4, 4).temple_terr,
            11,
            "DoN reads TIKAL_TEMPLE_BORDERS: 6 * 190% truncates to 11"
        );
    }

    #[test]
    fn border_score_matches_the_rules_xml_comment() {
        let c = TerritoryRules::default();
        // score <= territory_base<<8  <=>  d <= territory_base*(mult*B + num)/den
        for b in 0..12 {
            let limit = c.territory_base << 8;
            let mut last_ok = 0u32;
            for d in 0..4000u32 {
                if border_score(d, b, c.city_territory_multiplier, &c) <= limit {
                    last_ok = d;
                }
            }
            let closed_form =
                (c.territory_base * (c.city_territory_multiplier * b + c.territory_num)) as f64
                    / c.territory_den as f64;
            assert!(
                (last_ok as f64 - closed_form).abs() <= 1.0,
                "b={b} numeric={last_ok} closed={closed_form}"
            );
        }
    }

    /// The headline structural fact, pinned: for an **uncontested** source the outer edge
    /// of the border is set by the hard `territory_limit_*` cap, never by the score gate.
    /// The score function decides only *where two borders meet*.
    #[test]
    fn the_hard_cap_binds_not_the_score_gate() {
        let c = TerritoryRules::default();
        let limit = c.territory_base << 8;
        let furthest_scoring = |b: i32, mult: i32| -> u32 {
            (0..5000u32)
                .filter(|&d| border_score(compress_distance(d), b, mult, &c) <= limit)
                .next_back()
                .unwrap()
        };
        for (name, l) in [
            (
                "baseline",
                LeaderBorderInput {
                    active: true,
                    ..Default::default()
                },
            ),
            (
                "fully teched",
                LeaderBorderInput {
                    active: true,
                    temple_upgrade: [true; 3],
                    fort_upgrade: [true; 3],
                    gov_level: 7,
                    ..Default::default()
                },
            ),
        ] {
            let p = leader_border_params(&l, &c, 44, 4, 4);
            let city_b = p.civic_terr + p.temple_terr + c.city_level_territory_bonus[2];
            let city_cap = p.city_cap + 2 * 4 + p.temple_level * 4;
            assert!(
                furthest_scoring(city_b, c.city_territory_multiplier) > city_cap as u32,
                "{name}: score gate should be looser than the cap"
            );
            let fort_b = p.fort_terr + p.civic_terr;
            assert!(
                furthest_scoring(fort_b, c.fort_territory_multiplier) > p.fort_cap as u32,
                "{name}: fort score gate should be looser than the cap"
            );
        }

        // and concretely: a lone plain city reaches exactly territory_limit_base tiles
        let p = leader_border_params(
            &LeaderBorderInput {
                active: true,
                ..Default::default()
            },
            &c,
            44,
            4,
            4,
        );
        assert_eq!(p.city_cap, 44);
    }

    #[test]
    fn compress_distance_only_bites_up_close() {
        assert_eq!(compress_distance(0), 0);
        assert_eq!(compress_distance(1), 0);
        assert_eq!(compress_distance(4), 0);
        assert_eq!(compress_distance(12), 5);
        assert_eq!(compress_distance(13), 13);
        assert_eq!(compress_distance(100), 100);
        // monotone non-decreasing
        let mut prev = 0;
        for d in 0..200u32 {
            let v = compress_distance(d);
            assert!(v >= prev || d == 13, "d={d}");
            prev = v.max(prev);
        }
    }

    fn city_at(slot: i32, tx: i32, ty: i32) -> BorderSource {
        BorderSource {
            kind: BorderSourceKind::City {
                level: 1,
                has_temple: false,
                capital: false,
            },
            slot,
            who: slot,
            tile_x: tx,
            tile_y: ty,
            alive: true,
        }
    }

    #[test]
    fn a_lone_city_owns_a_disc_and_nothing_beyond_it() {
        let c = TerritoryRules::default();
        let mut slots: [Vec<BorderSource>; 8] = Default::default();
        slots[0].push(city_at(0, 2, 2));
        let mut params = [LeaderBorderParams::default(); 8];
        params[0] = leader_border_params(&plain_leader(), &c, 44, 4, 4);
        let mut active = [false; 8];
        active[0] = true;

        // right on top of the city
        assert_eq!(claim_tile(0, 0, &slots, &params, &active, &c).who, 0);
        // far outside every cap
        assert_eq!(claim_tile(60, 60, &slots, &params, &active, &c).who, -1);
    }

    #[test]
    fn the_closer_city_wins_and_the_other_becomes_who2() {
        let c = TerritoryRules::default();
        let mut slots: [Vec<BorderSource>; 8] = Default::default();
        slots[0].push(city_at(0, 0, 0));
        slots[1].push(city_at(1, 40, 0));
        let mut params = [LeaderBorderParams::default(); 8];
        let l = plain_leader();
        params[0] = leader_border_params(&l, &c, 44, 4, 4);
        params[1] = params[0];
        let active = [true, true, false, false, false, false, false, false];

        let near_p0 = claim_tile(1, 0, &slots, &params, &active, &c); // tile x = 6
        assert_eq!(near_p0.who, 0);
        assert_eq!(near_p0.who2, 1);

        let near_p1 = claim_tile(9, 0, &slots, &params, &active, &c); // tile x = 38
        assert_eq!(near_p1.who, 1);
        assert_eq!(near_p1.who2, 0);
    }

    #[test]
    fn more_border_bonus_pushes_the_frontier() {
        let c = TerritoryRules::default();
        let mut slots: [Vec<BorderSource>; 8] = Default::default();
        slots[0].push(city_at(0, 0, 0));
        slots[1].push(city_at(1, 40, 0));

        let plain = leader_border_params(&plain_leader(), &c, 44, 4, 4);
        let mut buffed_in = plain_leader();
        buffed_in.gov_level = 7; // civic_upgrade_terr[7] = 14
        let buffed = leader_border_params(&buffed_in, &c, 44, 4, 4);
        let active = [true, true, false, false, false, false, false, false];

        // find the frontier tile with both plain
        let frontier = |p0: LeaderBorderParams, p1: LeaderBorderParams| -> i32 {
            let mut params = [LeaderBorderParams::default(); 8];
            params[0] = p0;
            params[1] = p1;
            let mut last = -1;
            for wx in 0..10 {
                if claim_tile(wx, 0, &slots, &params, &active, &c).who == 0 {
                    last = wx;
                }
            }
            last
        };
        let base = frontier(plain, plain);
        let pushed = frontier(buffed, plain);
        assert!(
            pushed > base,
            "buffed frontier {pushed} should exceed base {base}"
        );
    }

    #[test]
    fn leader_scan_order_rotates_with_tile_x() {
        // Two identical cities equidistant from the tile: the winner is decided purely by
        // the (k + wx) & 7 rotation, so it must flip as wx moves.
        let c = TerritoryRules::default();
        let mut slots: [Vec<BorderSource>; 8] = Default::default();
        let mut params = [LeaderBorderParams::default(); 8];
        let p = leader_border_params(&plain_leader(), &c, 44, 4, 4);
        params[0] = p;
        params[1] = p;
        let active = [true, true, false, false, false, false, false, false];

        let mut winners = std::collections::HashSet::new();
        for wx in 0..8i32 {
            // place both cities symmetrically about this tile's centre
            let tx = wx * 4 + 2;
            slots[0].clear();
            slots[1].clear();
            slots[0].push(city_at(0, tx - 8, 0));
            slots[1].push(city_at(1, tx + 8, 0));
            winners.insert(claim_tile(wx, 0, &slots, &params, &active, &c).who);
        }
        assert!(
            winners.len() > 1,
            "the rotation must let both slots win somewhere: {winners:?}"
        );
    }

    /// The 256-tile-per-frame budget, and — the point of the reconciliation — the claims
    /// land in the `World` the checksum walks, not in a private plane.
    #[test]
    fn check_borders_respects_the_256_tile_budget() {
        let c = TerritoryRules::default();
        let mut world = World::init_default_rules(32, 32);
        let mut slots: [Vec<BorderSource>; 8] = Default::default();
        slots[0].push(city_at(0, 60, 60));
        let mut inputs = [LeaderBorderInput::default(); 8];
        inputs[0] = plain_leader();

        let coords: Vec<(i32, i32)> = (0..world.ys)
            .flat_map(|y| (0..world.xs).map(move |x| (x, y)))
            .collect();
        let mut regions = vec![RegionBorderState {
            flags: 4,
            size: coords.len() as i32,
            borders: 0,
            coords,
        }];

        let size = world.size;
        let mut frames = 0;
        while regions[0].dirty() {
            let done = check_borders(&mut regions, &mut world, &slots, &inputs, &c);
            assert!(done <= TERRITORY_TILES_PER_FRAME);
            frames += 1;
            assert!(frames < 100, "budget loop did not terminate");
        }
        assert_eq!(frames, (size + 255) / 256);
        assert!(
            world.wdata.iter().any(|d| d.who == 0),
            "someone should own something"
        );
    }

    /// A territory claim must move the `world` channel, and must move **section 5** of it
    /// and nothing else. This is the assertion neither module could make while there were
    /// two stores: `borders_fog` wrote one copy and `map_terrain` hashed the other.
    #[test]
    fn a_border_pass_moves_section_5_of_the_world_channel() {
        let c = TerritoryRules::default();
        let mut world = World::init_default_rules(16, 16);
        let before = world.checksum_sections();

        let mut slots: [Vec<BorderSource>; 8] = Default::default();
        slots[0].push(city_at(0, 30, 30));
        let mut inputs = [LeaderBorderInput::default(); 8];
        inputs[0] = plain_leader();
        let coords: Vec<(i32, i32)> = (0..world.ys)
            .flat_map(|y| (0..world.xs).map(move |x| (x, y)))
            .collect();
        let mut regions = vec![RegionBorderState {
            flags: 4,
            size: coords.len() as i32,
            borders: 0,
            coords,
        }];
        while regions[0].dirty() {
            check_borders(&mut regions, &mut world, &slots, &inputs, &c);
        }

        let after = world.checksum_sections();
        assert_ne!(before.full, after.full, "the channel must notice a border");
        assert_eq!(
            after.differing_sections(&before),
            vec![map_terrain::WorldSection::WData],
            "territory is WData, section 5, and nothing else moved"
        );
    }

    /// The same, for fog: `set_seen` touches `WData::was_seen` (section 5), the three fog
    /// planes (section 6) and `wcoord_seen` (section 7), and no other section.
    #[test]
    fn a_fog_stamp_moves_sections_5_6_and_7() {
        let (fog, mut world, circle) = tiny_fog();
        let before = world.checksum_sections();
        let obj = SeeingObject {
            fine_x: 16 * FINE_PER_FOG,
            fine_y: 16 * FINE_PER_FOG,
            owner: 0,
            los_tiles: 8,
            detector: true,
            grant_seen2_to: 0,
        };
        update_seen(&fog, &mut world, &circle, &obj, &mut Vec::new());
        let after = world.checksum_sections();

        use map_terrain::WorldSection::*;
        assert_eq!(
            after.differing_sections(&before),
            vec![WData, TDataAndFog, WCoordSeen]
        );
        // and the byte counts are unchanged — a fog stamp rewrites bytes, never resizes.
        for s in map_terrain::WorldSection::all() {
            assert_eq!(before.section(s).bytes, after.section(s).bytes, "{s:?}");
        }
    }

    #[test]
    fn ownership_queries() {
        let mut w = World::init_default_rules(8, 8);
        let i = w.w_index(3, 3);
        w.wdata[i].who = 1;
        w.wdata[i].who2 = 2;
        assert_eq!(get_who(&w, 3, 3), 1);
        assert_eq!(get_who2(&w, 3, 3), 2);
        assert_eq!(get_who(&w, 0, 0), -1);

        let mut d = Diplomacy::default();
        for (i, t) in d.team.iter_mut().enumerate() {
            *t = i as i32;
        }
        assert!(is_enemy_territory(&w, &d, 3, 3, 0));
        assert!(!is_enemy_territory(&w, &d, 3, 3, 1));
        assert!(!is_enemy_territory(&w, &d, 0, 0, 0), "unowned is neutral");

        // mutual alliance
        d.diplo[0][1] = 2;
        d.diplo[1][0] = 2;
        assert!(!is_enemy_territory(&w, &d, 3, 3, 0));
    }

    // --- attrition / supply ------------------------------------------------

    #[test]
    fn anti_attrition_ladder_and_total_immunity() {
        let c = AttritionRules::default();
        let mk = |lv, liberty, mongol, titanium| {
            calc_anti_attrition(
                lv,
                liberty,
                mongol,
                titanium,
                false,
                c.attrition_upgrade,
                100, // liberty_attrition
                50,  // mongol_attrition
                50,  // titanium_attrition
            )
        };
        assert_eq!(mk(None, false, false, false), ANTI_ATT_BASE);
        // Forage 25% -> 256 * 100/75
        assert!((mk(Some(0), false, false, false) - 256.0 * 100.0 / 75.0).abs() < 0.01);
        // Supply 50% -> exactly double, i.e. half the attrition rate
        assert_eq!(mk(Some(1), false, false, false), 512.0);
        // Mongols and titanium each halve the rate again
        assert_eq!(mk(None, false, true, false), 512.0);
        assert_eq!(mk(None, false, true, true), 1024.0);
        // Statue of Liberty is 100% -> zeroed, which get_attrition turns into "never"
        assert_eq!(mk(None, true, false, false), 0.0);
        let v = get_attrition(
            &AttritionInput {
                attacker_attrition: 8,
                victim_anti_att: 0.0,
                siege_class: false,
                militia: false,
                type_id: 0,
                type_class: 0,
                age_diff: -1,
            },
            &c,
        );
        assert_eq!(v, 0);
        assert_eq!(attrition_period(v, &c), None);
    }

    #[test]
    fn supply_upgrade_doubles_the_attrition_period() {
        let c = AttritionRules::default();
        let mk = |anti: f32| AttritionInput {
            attacker_attrition: 4,
            victim_anti_att: anti,
            siege_class: false,
            militia: false,
            type_id: 0,
            type_class: 0,
            age_diff: -1,
        };
        let plain = attrition_period(get_attrition(&mk(ANTI_ATT_BASE), &c), &c).unwrap();
        let supplied = attrition_period(get_attrition(&mk(512.0), &c), &c).unwrap();
        assert_eq!(plain, 12);
        assert_eq!(supplied, 24);
    }

    #[test]
    fn attrition_period_baseline_is_48_frames() {
        let c = AttritionRules::default();
        // anti_att 1.0, attacker attrition 1, plain unit -> get_attrition returns 256
        let v = get_attrition(
            &AttritionInput {
                attacker_attrition: 1,
                victim_anti_att: ANTI_ATT_BASE,
                siege_class: false,
                militia: false,
                type_id: 0,
                type_class: 0,
                age_diff: -1,
            },
            &c,
        );
        assert_eq!(v, 256);
        assert_eq!(attrition_period(v, &c), Some(48));
    }

    #[test]
    fn higher_attrition_rate_shortens_the_period() {
        let c = AttritionRules::default();
        let mk = |att: i32| AttritionInput {
            attacker_attrition: att,
            victim_anti_att: ANTI_ATT_BASE,
            siege_class: false,
            militia: false,
            type_id: 0,
            type_class: 0,
            age_diff: -1,
        };
        let p1 = attrition_period(get_attrition(&mk(1), &c), &c).unwrap();
        let p2 = attrition_period(get_attrition(&mk(2), &c), &c).unwrap();
        let p4 = attrition_period(get_attrition(&mk(4), &c), &c).unwrap();
        assert_eq!((p1, p2, p4), (48, 24, 12));
    }

    #[test]
    fn anti_attrition_lengthens_the_period() {
        let c = AttritionRules::default();
        let mk = |anti: f32| AttritionInput {
            attacker_attrition: 2,
            victim_anti_att: anti * ANTI_ATT_BASE,
            siege_class: false,
            militia: false,
            type_id: 0,
            type_class: 0,
            age_diff: -1,
        };
        let p = |a| attrition_period(get_attrition(&mk(a), &c), &c).unwrap();
        assert_eq!(p(1.0), 24);
        assert_eq!(p(2.0), 48);
        assert!(p(0.5) < p(1.0));
    }

    #[test]
    fn zero_attrition_rate_disables_it() {
        let c = AttritionRules::default();
        let v = get_attrition(
            &AttritionInput {
                attacker_attrition: 0,
                victim_anti_att: ANTI_ATT_BASE,
                siege_class: false,
                militia: false,
                type_id: 0,
                type_class: 0,
                age_diff: 0,
            },
            &c,
        );
        assert_eq!(v, 0);
        assert_eq!(attrition_period(v, &c), None);
    }

    #[test]
    fn aging_up_increases_the_attackers_rate() {
        let c = AttritionRules::default();
        let mk = |diff: i32| AttritionInput {
            attacker_attrition: 4,
            victim_anti_att: ANTI_ATT_BASE,
            siege_class: false,
            militia: false,
            type_id: 0,
            type_class: 0,
            age_diff: diff,
        };
        // age_diff < 0 leaves att alone; >= 0 scales it by (25*diff + 100)%
        assert_eq!(get_attrition(&mk(-1), &c), 256 / 4);
        assert_eq!(get_attrition(&mk(0), &c), 256 / 4);
        assert_eq!(get_attrition(&mk(4), &c), 256 / 8); // (25*4+100)*4+99 /100 = 8
    }

    #[test]
    fn siege_class_halves_the_effective_rate() {
        let c = AttritionRules::default();
        let mk = |siege| AttritionInput {
            attacker_attrition: 1,
            victim_anti_att: ANTI_ATT_BASE,
            siege_class: siege,
            militia: false,
            type_id: 0,
            type_class: 0,
            age_diff: -1,
        };
        // scale becomes 25600/(100-50) = 512, i.e. the period doubles
        assert_eq!(get_attrition(&mk(false), &c), 256);
        assert_eq!(get_attrition(&mk(true), &c), 512);
    }

    #[test]
    fn militia_take_far_more_attrition() {
        let c = AttritionRules::default();
        let mk = |m| AttritionInput {
            attacker_attrition: 1,
            victim_anti_att: ANTI_ATT_BASE,
            siege_class: false,
            militia: m,
            type_id: 0,
            type_class: 0,
            age_diff: -1,
        };
        // (256*100)/(300+100) = 64  -> period 12 instead of 48
        assert_eq!(get_attrition(&mk(true), &c), 64);
        assert_eq!(attrition_period(get_attrition(&mk(true), &c), &c), Some(12));
    }

    #[test]
    fn attrition_fires_on_the_id_phase_offset() {
        // period 48, two units one frame apart in id fire on different frames
        assert!(attrition_due(48, 0, 48));
        assert!(!attrition_due(48, 1, 48));
        assert!(attrition_due(47, 1, 48));
        assert!(!attrition_due(10, 0, 0), "period 0 never fires");
    }

    #[test]
    fn supply_blocks_attrition() {
        let mut s = SupplyInput {
            near_supply_source: true,
            ..Default::default()
        };
        assert!(supply_state(&s));
        assert_eq!(step_attrition(48, 0, 48, &s, 0, 2), None);

        s.near_supply_source = false;
        assert!(!supply_state(&s));
        assert_eq!(
            step_attrition(48, 0, 48, &s, 0, 2),
            Some(AttritionDamage {
                flat: 0,
                fractional: 8
            })
        );
    }

    #[test]
    fn supply_exemptions() {
        let base = SupplyInput {
            near_supply_source: true,
            ..Default::default()
        };
        // "always supplied" types short-circuit to false — the engine returns 0 there and
        // relies on process_attrition never setting a period for them.
        assert!(!supply_state(&SupplyInput {
            always_supplied: true,
            ..base
        }));
        assert!(!supply_state(&SupplyInput {
            militia: true,
            ..base
        }));
        assert!(!supply_state(&SupplyInput {
            already_flagged: true,
            ..base
        }));
    }

    #[test]
    fn attrition_damage_shapes() {
        assert_eq!(
            attrition_damage(1, 4),
            AttritionDamage {
                flat: 1,
                fractional: 0
            }
        );
        assert_eq!(
            attrition_damage(0, 3),
            AttritionDamage {
                flat: 0,
                fractional: 6
            }
        );
        assert_eq!(
            attrition_damage(0, 1),
            AttritionDamage {
                flat: 0,
                fractional: 16
            }
        );
    }

    #[test]
    fn out_of_supply_reload_multipliers() {
        let c = AttritionRules::default();
        assert_eq!(
            out_of_supply_reload(100, c.siege_out_of_supply_reload_256),
            150
        );
        assert_eq!(
            out_of_supply_reload(100, c.artillery_out_of_supply_reload_256),
            200
        );
    }

    // --- checksum ----------------------------------------------------------

    /// The channel's own bytes, from the one owner. `WData +0x0f/+0x10` is `who`/`who2`
    /// and `+0x14` is `was_seen`, which is what puts territory and exploration on the wire.
    #[test]
    fn wdata_walk_record_is_21_bytes_in_field_order() {
        let mut d = map_terrain::WData::default();
        d.flags = 0x1234;
        d.region = 0x0506;
        d.who = 3;
        d.who2 = -2;
        d.was_seen = 0xA5;
        let out = d.checksum_bytes();
        assert_eq!(out.len(), 0x15);
        assert_eq!(&out[0..2], &[0x34, 0x12]);
        assert_eq!(&out[4..6], &[0x06, 0x05]);
        assert_eq!(out[0x0f], 3);
        assert_eq!(out[0x10], 0xFE);
        assert_eq!(out[0x14], 0xA5);
    }

    /// The point of the whole lane: territory is inside the `world` sync channel, so a
    /// border that resolves one tile differently is a desync, not a cosmetic difference.
    #[test]
    fn a_territory_flip_changes_the_world_channel() {
        let mut a = World::init_default_rules(16, 16);
        let mut b = World::init_default_rules(16, 16);
        assert_eq!(a.checksum(), b.checksum());
        a.wdata[100].who = 1;
        b.wdata[100].who = 2;
        assert_ne!(a.checksum(), b.checksum());
        assert_eq!(
            a.checksum_sections()
                .differing_sections(&b.checksum_sections()),
            vec![map_terrain::WorldSection::WData]
        );
    }
}
