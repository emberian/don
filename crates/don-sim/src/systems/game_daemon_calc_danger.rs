// SPDX-License-Identifier: GPL-3.0-or-later
//! `GameDaemon::calc_danger` `0x00732D10` (1,476 B) and `GameDaemon::do_danger`
//! `0x00732390` (195 B) — the AI threat map, the one child of tick step 12 whose body was
//! *absent* rather than host-blocked.
//!
//! # What this is
//!
//! Step 12's shell ([`super::game_daemon_step12`]) already executes and schedules this
//! child on `frame % 200 == 0`. Until now it charged [`crate::tick::Gap::GameDaemonCalcDanger`]
//! and did nothing. This module is the recovered body: three passes that rewrite
//! `World::danger[8]` — the per-player `int` plane at `World +0x13C`, `reg_size` entries,
//! indexed `ry * reg_xs + rx` in `RCoord` (1,536 Coord = 8 tiles) — from every live unit and
//! building in the world.
//!
//! ```text
//! GameDaemon::process_all                   0x00732700   (step 12 shell)
//!  +- [frame % 200 == 0] GameDaemon::calc_danger 0x00732D10   <- this file
//!      pass 1  0x00732D30..0x00732D5D   wipe the plane of every `flags & 2` leader
//!      pass 2  0x00732D90..0x00732F01   the unit band, per owner
//!      pass 3  0x00732F20..0x00733299   the building band, per owner
//!      +- GameDaemon::do_danger          0x00732390   <- [`do_danger`], every write
//! ```
//!
//! Everything below is [measured] from a capstone disassembly of `0x00732D10` and
//! `0x00732390` in `ron-bin/riseofnations.exe` (sha256 `30478a44…625079`), with names and
//! layouts from `ron-bin/sbl/rise.pdb` via `schema/types.json` / `schema/symbols.json`.
//! **Tier C**: structure, constants and arithmetic are read off the instruction stream;
//! nothing here has been executed against retail and no oracle case covers it.
//!
//! # The three passes, transcribed
//!
//! ```text
//! ; pass 1 — wipe
//! for who in 0..8:
//!     if leaders[who].flags & 2 and world.danger[who] != NULL:
//!         memset(world.danger[who], 0, world.reg_size * 4)
//!
//! ; pass 2 — the unit band
//! for who in 0..8:
//!     if leaders[who].flags & 1 == 0: continue
//!     for o in 0 .. objects.unit_mark[who]:                  ; band base 0
//!         u = objects.lists[who][o]
//!         if !u->is_valid_unit(): continue                   ; vtable +0x08
//!         if !u->is_on_map():     continue                   ; vtable +0xBC
//!         if (units.lists[who][o]->ptype->role & 0x10000) == 0: continue
//!         cell = RCoord(u->y_internal) * reg_xs + RCoord(u->x_internal)
//!         for to in 0..8:
//!             if leaders[to].flags & 2 == 0: continue
//!             if to == leaders[who].slot: continue           ; NOTE: `slot`, not `who`
//!             if leaders[who].diplo[to] != 0
//!                and leaders[to].diplo[leaders[who].slot] != 0: continue
//!             do_danger(o, who, to, cell, (u->attack() * 5) / 10)   ; vtable +0x120
//!
//! ; pass 3 — the building band
//! for who in 0..8:
//!     if leaders[who].flags & 1 == 0: continue
//!     for o in obj_base[1] .. objects.obj_mark[1][who]:      ; 2000 .. build_mark[who]
//!         b = objects.lists[who][o]
//!         if !b->is_valid_wall(): continue                   ; vtable +0x0C
//!         if !b->is_active():     continue                   ; vtable +0x4C
//!         if b->get_build()->city < 0                        ; BuildData +0x72
//!            and (b->get_build()->ptype->build_flags & 0x10) == 0: continue
//!         cell = RCoord(b->y_internal) * reg_xs + RCoord(b->x_internal)
//!         amount = <the strength ladder, below>
//!         for to in 0..8:
//!             if leaders[to].flags & 2 == 0: continue        ; no self/diplomacy gate here
//!             for k in 0..8:
//!                 nx = rx + NEIGHBOUR_DX[k] ; ny = ry + NEIGHBOUR_DY[k]
//!                 if 0 <= nx < reg_xs and 0 <= ny < reg_ys:
//!                     do_danger(o, who, to, ny * reg_xs + nx, amount / 2)
//!             do_danger(o, who, to, cell, amount)
//! ```
//!
//! ## The building strength ladder `0x00732FF4`..`0x00733198`
//!
//! Evaluated once per building, before the target loop, and short-circuiting in this exact
//! order:
//!
//! | # | test | site | result |
//! |--:|---|---|---|
//! | 1 | `b->get_wall()->ptype->is_fort()` | `0x00732FFF` | `hits_left() / 2` |
//! | 2 | `b->get_build()->is(TOWER, 0)` | `0x0073303A` | `hits_left() / 2` |
//! | 3 | `b->get_build()->flags & 0x20` | `0x00733058` | `100` |
//! | 4 | `b->get_build()->is(AIRBASE, 0)` | `0x0073308F` | `100` |
//! | 5 | `b->get_build()->is(DOCK, 0)` | `0x007330C7` | `100` |
//! | 6 | otherwise | `0x0073311D` | `50` if `buildtypes[ptype->basic_type()]->build_flags & 0x40000000` else `10` |
//!
//! `is_fort` is `ObjectTypeData::is_fort` (type vtable `+0xFC`), not an `ObjectData` slot —
//! the receiver is `wall->ptype`, and reading the slot number against the wrong class is
//! how this reads as `get_caster_stance` if you stop at the decompiler.
//!
//! Rungs 1 and 2 both land on `hits_left`, which retail reaches devirtualised: it compares
//! the `+0x114` slot against `ObjectData::hits_left` `0x006535C0` and inlines that body
//! (`min(hits(0) - damage, hits(0))`, clamped to zero if either term is negative), else it
//! calls the slot. Rung 6's `basic_type` is inlined the same way against
//! `BuildTypeData::basic_type` `0x00639970` (`from < 0 ? type : buildtypes[from]->basic_type()`).
//! Both inlines are pure reads, so a host may answer them however it likes; only the value
//! is load-bearing.
//!
//! `TOWER`/`AIRBASE`/`DOCK` are `0x1B7`/`0x1BF`/`0x1B0`, resolved from the PDB's own
//! `TypeIndex` enum (869 enumerators) — the raw immediates in `.text` are these three.
//!
//! ## `GameDaemon::do_danger` — every write goes through it
//!
//! ```text
//! do_danger(o, from, to, cell, amount):
//!     if to == from:                       danger[to][cell] -= amount          ; 0x007323B4
//!     d = leaders[from].diplo[to]
//!     if d == 2:                           danger[to][cell] += -(amount / 2)   ; 0x0073244D
//!     if o >= 0 and !objects.lists[from][o]->is_seen(to, 0): amount /= 2       ; vtable +0x48
//!     if d == 1:                                            amount /= 2
//!     danger[to][cell] += amount                                               ; 0x00732428
//! ```
//!
//! Three consequences worth stating, because a "reasonable" danger map has none of them:
//!
//! * **Danger is negative for its owner.** The `to == from` arm *subtracts*, and pass 3 has
//!   no self-skip, so a player's own buildings dig a well in that player's plane.
//! * **Allies subtract too**, at half rate (`d == 2`), and a `d == 1` relationship halves
//!   the deposit. The plane is a signed field, not a heat map.
//! * **Unseen attackers still register**, at half strength, so the map leaks information
//!   about objects the target cannot see. That is the retail behaviour; it is not a bug to
//!   be "fixed" in fidelity mode.
//!
//! # Deliberate boundaries
//!
//! * **The centre cell is not bounds-checked by retail.** Only the eight neighbours are
//!   (`0x007331E0`..`0x007331F0`). Retail computes `ry * reg_xs + rx` from the object
//!   position and stores through it unchecked. This port refuses that store and counts it
//!   in [`CalcDangerTrace::unmapped_centre_cells`] instead of reproducing an out-of-bounds
//!   write. That is a named divergence at the memory-safety floor, not a modelling choice.
//! * **`attack()`, `hits_left()`, `is(...)`, `basic_type()` and `is_seen()` are host
//!   facts.** They are all `const` queries with no RNG and no state mutation, so this
//!   driver may take them as a snapshot; [`CalcDangerTrace`] records the call counts retail
//!   would have made so the difference stays visible.
//! * **`Objects::lists[who][o]` and `Units::lists[who][o]` are the same entity.**
//!   `Objects::lists` is `ObjectsArray[10] = MultiPtrArray<Object>` and pass 2 reads
//!   validity from `objects` (`0x00732DB0`) and `ptype->role` from `units`
//!   (`0x00732DE1`, `units + 0x10 + 0x1C*who` — `Units::lists` is `PtrArray<Unit>[10]`)
//!   at the *same* index. This module models one object; a host that can make the two
//!   arrays disagree must fault in `preflight` rather than pick one.

use super::leaders::{Leader, NUM_LEADER_SLOTS};
use super::map_terrain::{floor_div, World, COORD_PER_RCELL};

/// `GameDaemon::calc_danger`.
pub const CALC_DANGER_VA: u32 = 0x0073_2D10;
pub const CALC_DANGER_SIZE: usize = 1476;
/// `GameDaemon::do_danger`. `ret 0x14` — five stack arguments, and the emitted body never
/// reads `ECX`, so the PDB's `GameDaemon::` qualification carries no `this`.
pub const DO_DANGER_VA: u32 = 0x0073_2390;
pub const DO_DANGER_SIZE: usize = 195;

/// The eight leader slots both loops walk: `0x00E3A390` to `0x00E71AF0` at stride `0x6EEC`
/// is exactly eight `Leader` records, so slots 8 and 9 never contribute danger.
pub const LEADER_SLOTS: usize = NUM_LEADER_SLOTS;

/// `obj_base[1]` — the first index of the building band inside `Objects::lists[who]`.
/// `Objects::init` `0x0065EA80` sets `obj_base = {0, 2000, 3000}`.
pub const BUILD_BAND_BASE: i32 = 2000;

/// `SubObjectData::x_internal` / `y_internal` are stored XOR `0x00063637`.
pub const COORD_XOR: i32 = 0x0006_3637;

/// The eight-neighbour offsets at `0x00ADCAF4` (x) and `0x00ADC404` (y), read out of the
/// image. They start north-west and run clockwise.
pub const NEIGHBOUR_DX: [i32; 8] = [-1, 0, 1, 1, 1, 0, -1, -1];
/// See [`NEIGHBOUR_DX`].
pub const NEIGHBOUR_DY: [i32; 8] = [-1, -1, -1, 0, 1, 1, 1, 0];

/// `TypeIndex::TOWER`, the immediate at `0x00733022`.
pub const TYPE_TOWER: i32 = 0x1B7;
/// `TypeIndex::AIRBASE`, the immediate at `0x00733077`.
pub const TYPE_AIRBASE: i32 = 0x1BF;
/// `TypeIndex::DOCK`, the immediate at `0x007330AF`.
pub const TYPE_DOCK: i32 = 0x1B0;

/// `UnitTypeData::role` (`ptype +0x2C8`) bit tested at `0x00732DE9`. A unit whose role
/// lacks it contributes no danger at all.
pub const UNIT_ROLE_DANGEROUS: u32 = 0x0001_0000;
/// `BuildTypeData::build_flags` (`ptype +0x2C0`) bit tested at `0x00732FA4`: admits a
/// building that belongs to no city.
pub const BUILD_FLAGS_STANDALONE: u32 = 0x0000_0010;
/// `BuildTypeData::build_flags` bit tested at `0x00733123` on the *basic* type.
pub const BUILD_FLAGS_HIGH_VALUE: u32 = 0x4000_0000;
/// `SubObjectData::flags` (`+0x08`) bit tested at `0x00733058`.
pub const BUILD_FLAG_FIXED_DANGER: u8 = 0x20;

/// The three literal strengths: `0x64` at `0x00733134`, and `0x28 + 0x0A` / `0x0A` at
/// `0x0073312C`..`0x0073312F`.
pub const STRENGTH_FIXED: i32 = 0x64;
/// See [`STRENGTH_FIXED`].
pub const STRENGTH_HIGH_VALUE: i32 = 0x28 + 0x0A;
/// See [`STRENGTH_FIXED`].
pub const STRENGTH_PLAIN: i32 = 0x0A;

/// `div_3_table[(c ^ 0x63637) >> 9]` — the `RCoord` of a stored `Coord` field.
///
/// `div_3_table` is `0x00CAE5FC`, filled by `init_coord_lookup_array` `0x00681DB0` with
/// `floor(i/3)` in both halves, so the composed lookup is exactly `floor(c / 1536)` and
/// [`super::map_terrain`] already carries that identity. This helper exists so the XOR and
/// the shift stay next to each other at the one place `calc_danger` performs them
/// (`0x00732E18` and `0x00732FC5`).
#[inline]
pub fn region_of(internal: i32) -> i32 {
    floor_div(internal ^ COORD_XOR, COORD_PER_RCELL)
}

// =======================================================================================
// Facts the host supplies
// =======================================================================================

/// One leader as both loops read it: `LeaderData +0x00`, `+0x08`, `+0x74`.
///
/// This is a projection of [`Leader`], not a second copy of it — [`leader_facts`] builds it
/// from the step-8 record so the two can never drift.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct LeaderDangerFacts {
    /// `LeaderData::leader_flags` `+0x00`.
    pub flags: u32,
    /// `LeaderData::who` `+0x08`. Pass 2 compares the *target loop index* against this, not
    /// against the outer loop index (`0x00732E50`), and indexes the other leader's
    /// `diplos` by it (`0x00732E5A`).
    pub slot: i32,
    /// `LeaderData::diplos` `+0x74`, indexed by the other leader's `slot`.
    pub diplos: [i32; LEADER_SLOTS],
}

/// Project a step-8 [`Leader`] into the three fields `calc_danger` reads.
pub fn leader_facts(leader: &Leader) -> LeaderDangerFacts {
    LeaderDangerFacts {
        flags: leader.flags,
        slot: leader.slot,
        diplos: leader.diplo,
    }
}

/// One unit-band object as pass 2 reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct UnitDangerFacts {
    /// `SubObjectData::is_valid_unit` — vtable `+0x08`, `0x00732DC4`.
    pub is_valid_unit: bool,
    /// `ObjectData::is_on_map` — vtable `+0xBC`, `0x00732DD3`.
    pub is_on_map: bool,
    /// `UnitTypeData::role`, `units.lists[who][o]->ptype +0x2C8`.
    pub role: u32,
    /// `SubObjectData::x_internal` `+0x10`, stored XOR [`COORD_XOR`].
    pub x_internal: i32,
    /// `SubObjectData::y_internal` `+0x14`, stored XOR [`COORD_XOR`].
    pub y_internal: i32,
    /// `ObjectData::attack` — vtable `+0x120`, `0x00732E76`. Retail calls it once per
    /// *reached target*, not once per unit; see [`CalcDangerTrace::attack_calls`].
    pub attack: i32,
}

/// One building-band object as pass 3 reads it.
///
/// Retail short-circuits the strength ladder, so several of these fields are computed for
/// buildings that never reach their rung. Every one of them is a `const` query with no RNG
/// and no state mutation, so evaluating them eagerly cannot move simulation state; it moves
/// only the call count, and [`CalcDangerTrace`] carries the retail count separately.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct BuildDangerFacts {
    /// `SubObjectData::is_valid_wall` — vtable `+0x0C`, `0x00732F5A`.
    pub is_valid_wall: bool,
    /// `SubObjectData::is_active` — vtable `+0x4C`, `0x00732F69`.
    pub is_active: bool,
    /// `BuildData::city` `+0x72`, a `short`; `>= 0` admits the building.
    pub city: i16,
    /// `BuildTypeData::build_flags`, `get_build()->ptype +0x2C0`.
    pub build_flags: u32,
    /// `SubObjectData::x_internal` `+0x10`, stored XOR [`COORD_XOR`].
    pub x_internal: i32,
    /// `SubObjectData::y_internal` `+0x14`, stored XOR [`COORD_XOR`].
    pub y_internal: i32,
    /// `ObjectTypeData::is_fort` on `get_wall()->ptype` — type vtable `+0xFC`.
    pub wall_type_is_fort: bool,
    /// `get_build()->is(TOWER, 0)`.
    pub is_tower: bool,
    /// `SubObjectData::flags` `+0x08` of `get_build()`.
    pub build_object_flags: u8,
    /// `get_build()->is(AIRBASE, 0)`.
    pub is_airbase: bool,
    /// `get_build()->is(DOCK, 0)`.
    pub is_dock: bool,
    /// `buildtypes[get_build()->ptype->basic_type()]->build_flags`.
    pub basic_type_build_flags: u32,
    /// `get_wall()->hits_left()`.
    pub hits_left: i32,
}

/// Which rung of the ladder at `0x00732FF4`..`0x00733198` decided a building's strength.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrengthRung {
    /// Rung 1 — `wall->ptype->is_fort()`.
    Fort,
    /// Rung 2 — `build->is(TOWER, 0)`.
    Tower,
    /// Rung 3 — `build->flags & 0x20`.
    FixedFlag,
    /// Rung 4 — `build->is(AIRBASE, 0)`.
    Airbase,
    /// Rung 5 — `build->is(DOCK, 0)`.
    Dock,
    /// Rung 6 — `buildtypes[basic_type]->build_flags & 0x40000000`.
    BasicType,
}

/// The ladder itself. Returns the strength and the rung that produced it.
pub fn build_strength(facts: &BuildDangerFacts) -> (i32, StrengthRung) {
    if facts.wall_type_is_fort {
        return (hits_half(facts.hits_left), StrengthRung::Fort);
    }
    if facts.is_tower {
        return (hits_half(facts.hits_left), StrengthRung::Tower);
    }
    if facts.build_object_flags & BUILD_FLAG_FIXED_DANGER != 0 {
        return (STRENGTH_FIXED, StrengthRung::FixedFlag);
    }
    if facts.is_airbase {
        return (STRENGTH_FIXED, StrengthRung::Airbase);
    }
    if facts.is_dock {
        return (STRENGTH_FIXED, StrengthRung::Dock);
    }
    let strength = if facts.basic_type_build_flags & BUILD_FLAGS_HIGH_VALUE != 0 {
        STRENGTH_HIGH_VALUE
    } else {
        STRENGTH_PLAIN
    };
    (strength, StrengthRung::BasicType)
}

/// `cdq; sub eax,edx; sar eax,1` at `0x00733193` — signed division by two, truncating
/// toward zero, which is what Rust's `/` already does.
#[inline]
fn hits_half(hits_left: i32) -> i32 {
    hits_left / 2
}

/// `lea ecx,[eax+eax*4]` then the `0x66666667` magic divide at `0x00732E7C`..`0x00732E8E`:
/// `(attack * 5) / 10`, with the multiply wrapping exactly as `imul` does.
#[inline]
pub fn unit_strength(attack: i32) -> i32 {
    attack.wrapping_mul(5) / 10
}

// =======================================================================================
// Planes
// =======================================================================================

/// The eight `World::danger` planes, as `calc_danger` addresses them.
///
/// Implemented for [`World`] so a caller passes the authoritative map, not a copy.
pub trait DangerPlanes {
    /// `World +0x24`.
    fn reg_xs(&self) -> i32;
    /// `World +0x28`.
    fn reg_ys(&self) -> i32;
    /// `World +0x2C`, the element count of each plane.
    fn reg_size(&self) -> i32;
    /// Whether `World::danger[who]` is a live allocation. Retail's pass 1 skips a null
    /// plane (`0x00732D38`); a plane that is absent is not a plane full of zeros.
    fn plane_present(&self, who: usize) -> bool;
    /// `memset(danger[who], 0, reg_size * 4)`.
    fn clear_plane(&mut self, who: usize);
    /// `&danger[who][cell]`, or `None` when `cell` is outside the plane.
    fn cell_mut(&mut self, who: usize, cell: i32) -> Option<&mut i32>;
}

impl DangerPlanes for World {
    fn reg_xs(&self) -> i32 {
        self.reg_xs
    }
    fn reg_ys(&self) -> i32 {
        self.reg_ys
    }
    fn reg_size(&self) -> i32 {
        self.reg_size
    }
    fn plane_present(&self, who: usize) -> bool {
        self.danger.get(who).is_some_and(|plane| !plane.is_empty())
    }
    fn clear_plane(&mut self, who: usize) {
        if let Some(plane) = self.danger.get_mut(who) {
            plane.iter_mut().for_each(|cell| *cell = 0);
        }
    }
    fn cell_mut(&mut self, who: usize, cell: i32) -> Option<&mut i32> {
        if cell < 0 {
            return None;
        }
        self.danger.get_mut(who)?.get_mut(cell as usize)
    }
}

// =======================================================================================
// The host
// =======================================================================================

/// Everything `calc_danger` reads that is not a danger plane.
///
/// `preflight` must validate the whole reachable population before any mutation: retail has
/// no rollback path, so a mid-pass "unsupported" would leave a partially rewritten threat
/// map. After it returns `Ok` the accessors are infallible, exactly as
/// [`super::game_daemon_step12::GameDaemonProcessAllHost`] is arranged.
pub trait CalcDangerHost {
    type Fault;

    /// Refuse the whole child unless every leader, band bound and object fact below can be
    /// answered authoritatively.
    fn preflight(&self) -> Result<(), Self::Fault>;

    /// `leaders.list[slot]`, `slot < 8`.
    fn leader(&self, slot: usize) -> LeaderDangerFacts;

    /// `Objects::unit_mark[who]` (`+0x15C`) — the exclusive end of the unit band.
    fn unit_band_end(&self, who: usize) -> i32;

    /// `Objects::obj_mark[1][who]` (`*(Objects +0x1EC)` indexed by owner, i.e.
    /// `build_mark[who]` at `+0x184`) — the exclusive end of the building band.
    fn build_band_end(&self, who: usize) -> i32;

    /// `objects.lists[who][o]` for `o` in the unit band.
    fn unit(&self, who: usize, o: i32) -> UnitDangerFacts;

    /// `objects.lists[who][o]` for `o` in the building band.
    fn build(&self, who: usize, o: i32) -> BuildDangerFacts;

    /// `objects.lists[from][o]->is_seen(to, 0)` — `SubObjectData::is_seen`, vtable `+0x48`,
    /// the single vcall inside `do_danger` (`0x007323EE`).
    fn is_seen(&self, from: usize, o: i32, to: usize) -> bool;
}

/// A fail-closed structural error. Neither variant mutates a danger plane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CalcDangerError<E> {
    /// The world does not carry the retail region lattice (`reg_xs * reg_ys == reg_size`,
    /// both non-negative). Wiping and indexing planes under a broken lattice would write
    /// the wrong cells rather than fail.
    RegionLattice {
        reg_xs: i32,
        reg_ys: i32,
        reg_size: i32,
    },
    /// [`CalcDangerHost::preflight`] refused.
    Host(E),
}

/// Deterministic evidence from one `calc_danger` pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CalcDangerTrace {
    /// Pass 1: planes wiped (`flags & 2` and a live allocation).
    pub planes_cleared: u32,
    /// Pass 2: units that passed all three gates.
    pub units_scanned: u32,
    /// Pass 3: buildings that passed all three gates.
    pub builds_scanned: u32,
    /// `GameDaemon::do_danger` entries — every write attempt, including self and ally arms.
    pub do_danger_calls: u32,
    /// Writes actually applied to a plane.
    pub cells_written: u32,
    /// Centre cells outside the plane. Retail stores through these unchecked; this port
    /// refuses and counts instead. A non-zero value is a fidelity hole, not noise.
    pub unmapped_centre_cells: u32,
    /// `ObjectData::attack` calls retail would have made — once per reached *target*, which
    /// is why this is not `units_scanned`.
    pub attack_calls: u32,
    /// `SubObjectData::is_seen` calls made from `do_danger`.
    pub is_seen_calls: u32,
}

// =======================================================================================
// GameDaemon::do_danger 0x00732390
// =======================================================================================

/// `GameDaemon::do_danger(o, from, to, cell, amount)`.
///
/// `o < 0` is retail's "no owning object" marker (`test edx,edx; js` at `0x007323D0`): it
/// skips the visibility halving. Nothing in `calc_danger` passes a negative `o`, but the
/// arm is real and other callers may.
pub fn do_danger<P: DangerPlanes, H: CalcDangerHost>(
    planes: &mut P,
    host: &H,
    leaders: &[LeaderDangerFacts; LEADER_SLOTS],
    trace: &mut CalcDangerTrace,
    o: i32,
    from: usize,
    to: usize,
    cell: i32,
    amount: i32,
) {
    trace.do_danger_calls = trace.do_danger_calls.saturating_add(1);

    // 0x0073239C — the identity arm subtracts the full amount and returns.
    if to == from {
        apply(planes, trace, to, cell, |slot| slot.wrapping_sub(amount));
        return;
    }

    let diplo = leaders[from].diplos[to];

    // 0x007323C3 — `2` subtracts half and returns; it never reaches the visibility test.
    if diplo == 2 {
        apply(planes, trace, to, cell, |slot| {
            slot.wrapping_add((amount / 2).wrapping_neg())
        });
        return;
    }

    let mut value = amount;
    if o >= 0 {
        trace.is_seen_calls = trace.is_seen_calls.saturating_add(1);
        if !host.is_seen(from, o, to) {
            value /= 2;
        }
    }
    // 0x00732404 — `1` halves again, and composes with the visibility halving.
    if diplo == 1 {
        value /= 2;
    }
    apply(planes, trace, to, cell, |slot| slot.wrapping_add(value));
}

fn apply<P: DangerPlanes>(
    planes: &mut P,
    trace: &mut CalcDangerTrace,
    to: usize,
    cell: i32,
    f: impl FnOnce(i32) -> i32,
) {
    match planes.cell_mut(to, cell) {
        Some(slot) => {
            *slot = f(*slot);
            trace.cells_written = trace.cells_written.saturating_add(1);
        }
        None => {
            trace.unmapped_centre_cells = trace.unmapped_centre_cells.saturating_add(1);
        }
    }
}

// =======================================================================================
// GameDaemon::calc_danger 0x00732D10
// =======================================================================================

/// `GameDaemon::calc_danger` `0x00732D10`, all three passes in retail order.
pub fn calc_danger<P: DangerPlanes, H: CalcDangerHost>(
    planes: &mut P,
    host: &H,
) -> Result<CalcDangerTrace, CalcDangerError<H::Fault>> {
    let (reg_xs, reg_ys, reg_size) = (planes.reg_xs(), planes.reg_ys(), planes.reg_size());
    if reg_xs < 0 || reg_ys < 0 || reg_size != reg_xs.saturating_mul(reg_ys) {
        return Err(CalcDangerError::RegionLattice {
            reg_xs,
            reg_ys,
            reg_size,
        });
    }
    host.preflight().map_err(CalcDangerError::Host)?;

    let leaders: [LeaderDangerFacts; LEADER_SLOTS] = std::array::from_fn(|slot| host.leader(slot));
    let mut trace = CalcDangerTrace::default();

    // ---- pass 1, 0x00732D30 -----------------------------------------------------------
    for (who, leader) in leaders.iter().enumerate() {
        if leader.flags & crate::systems::leaders::flag::PROCESS == 0 {
            continue;
        }
        if !planes.plane_present(who) {
            continue;
        }
        planes.clear_plane(who);
        trace.planes_cleared += 1;
    }

    // ---- pass 2, 0x00732D90 -----------------------------------------------------------
    for who in 0..LEADER_SLOTS {
        if leaders[who].flags & crate::systems::leaders::flag::IN_GAME == 0 {
            continue;
        }
        let end = host.unit_band_end(who);
        for o in 0..end {
            let unit = host.unit(who, o);
            if !unit.is_valid_unit || !unit.is_on_map {
                continue;
            }
            if unit.role & UNIT_ROLE_DANGEROUS == 0 {
                continue;
            }
            trace.units_scanned += 1;

            let rx = region_of(unit.x_internal);
            let ry = region_of(unit.y_internal);
            let cell = ry.wrapping_mul(reg_xs).wrapping_add(rx);

            for to in 0..LEADER_SLOTS {
                if leaders[to].flags & crate::systems::leaders::flag::PROCESS == 0 {
                    continue;
                }
                // 0x00732E50 — against `leaders[who].slot`, not `who`.
                if to as i32 == leaders[who].slot {
                    continue;
                }
                // 0x00732E54 — either direction being 0 admits the pair.
                let self_slot = leaders[who].slot;
                let mine = leaders[who].diplos[to];
                let theirs = leaders[to]
                    .diplos
                    .get(usize::try_from(self_slot).unwrap_or(usize::MAX))
                    .copied();
                let reciprocal_zero = theirs == Some(0);
                if mine != 0 && !reciprocal_zero {
                    continue;
                }

                trace.attack_calls = trace.attack_calls.saturating_add(1);
                do_danger(
                    planes,
                    host,
                    &leaders,
                    &mut trace,
                    o,
                    who,
                    to,
                    cell,
                    unit_strength(unit.attack),
                );
            }
        }
    }

    // ---- pass 3, 0x00732F20 -----------------------------------------------------------
    for who in 0..LEADER_SLOTS {
        if leaders[who].flags & crate::systems::leaders::flag::IN_GAME == 0 {
            continue;
        }
        let end = host.build_band_end(who);
        for o in BUILD_BAND_BASE..end {
            let build = host.build(who, o);
            if !build.is_valid_wall || !build.is_active {
                continue;
            }
            // 0x00732F87 — `city >= 0` admits outright; otherwise the type flag must.
            if build.city < 0 && build.build_flags & BUILD_FLAGS_STANDALONE == 0 {
                continue;
            }
            trace.builds_scanned += 1;

            let rx = region_of(build.x_internal);
            let ry = region_of(build.y_internal);
            let cell = ry.wrapping_mul(reg_xs).wrapping_add(rx);
            let (amount, _rung) = build_strength(&build);

            for to in 0..LEADER_SLOTS {
                if leaders[to].flags & crate::systems::leaders::flag::PROCESS == 0 {
                    continue;
                }
                for k in 0..8 {
                    let nx = rx.wrapping_add(NEIGHBOUR_DX[k]);
                    let ny = ry.wrapping_add(NEIGHBOUR_DY[k]);
                    if nx < 0 || ny < 0 || nx >= reg_xs || ny >= reg_ys {
                        continue;
                    }
                    let neighbour = ny.wrapping_mul(reg_xs).wrapping_add(nx);
                    do_danger(
                        planes,
                        host,
                        &leaders,
                        &mut trace,
                        o,
                        who,
                        to,
                        neighbour,
                        amount / 2,
                    );
                }
                do_danger(planes, host, &leaders, &mut trace, o, who, to, cell, amount);
            }
        }
    }

    Ok(trace)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::leaders::flag;

    /// A plane set the size of a retail region lattice, with no `World` behind it.
    #[derive(Clone, Debug)]
    struct Planes {
        reg_xs: i32,
        reg_ys: i32,
        present: [bool; LEADER_SLOTS],
        planes: [Vec<i32>; LEADER_SLOTS],
    }

    impl Planes {
        fn new(reg_xs: i32, reg_ys: i32) -> Self {
            let n = (reg_xs * reg_ys) as usize;
            Self {
                reg_xs,
                reg_ys,
                present: [true; LEADER_SLOTS],
                planes: std::array::from_fn(|_| vec![0i32; n]),
            }
        }
    }

    impl DangerPlanes for Planes {
        fn reg_xs(&self) -> i32 {
            self.reg_xs
        }
        fn reg_ys(&self) -> i32 {
            self.reg_ys
        }
        fn reg_size(&self) -> i32 {
            self.reg_xs * self.reg_ys
        }
        fn plane_present(&self, who: usize) -> bool {
            self.present[who]
        }
        fn clear_plane(&mut self, who: usize) {
            self.planes[who].iter_mut().for_each(|c| *c = 0);
        }
        fn cell_mut(&mut self, who: usize, cell: i32) -> Option<&mut i32> {
            if cell < 0 {
                return None;
            }
            self.planes[who].get_mut(cell as usize)
        }
    }

    #[derive(Clone, Default)]
    struct Host {
        reject: bool,
        leaders: [LeaderDangerFacts; LEADER_SLOTS],
        unit_end: [i32; LEADER_SLOTS],
        build_end: [i32; LEADER_SLOTS],
        units: Vec<((usize, i32), UnitDangerFacts)>,
        builds: Vec<((usize, i32), BuildDangerFacts)>,
        unseen: Vec<(usize, i32, usize)>,
    }

    impl CalcDangerHost for Host {
        type Fault = &'static str;

        fn preflight(&self) -> Result<(), Self::Fault> {
            if self.reject {
                Err("no authoritative object band")
            } else {
                Ok(())
            }
        }
        fn leader(&self, slot: usize) -> LeaderDangerFacts {
            self.leaders[slot]
        }
        fn unit_band_end(&self, who: usize) -> i32 {
            self.unit_end[who]
        }
        fn build_band_end(&self, who: usize) -> i32 {
            self.build_end[who]
        }
        fn unit(&self, who: usize, o: i32) -> UnitDangerFacts {
            self.units
                .iter()
                .find(|(key, _)| *key == (who, o))
                .map(|(_, facts)| *facts)
                .unwrap_or_default()
        }
        fn build(&self, who: usize, o: i32) -> BuildDangerFacts {
            self.builds
                .iter()
                .find(|(key, _)| *key == (who, o))
                .map(|(_, facts)| *facts)
                .unwrap_or_default()
        }
        fn is_seen(&self, from: usize, o: i32, to: usize) -> bool {
            !self.unseen.contains(&(from, o, to))
        }
    }

    fn two_players() -> Host {
        let mut host = Host::default();
        for (slot, leader) in host.leaders.iter_mut().enumerate() {
            leader.slot = slot as i32;
        }
        host.leaders[0].flags = flag::IN_GAME | flag::PROCESS;
        host.leaders[1].flags = flag::IN_GAME | flag::PROCESS;
        host
    }

    fn armed_unit(rx: i32, ry: i32, attack: i32) -> UnitDangerFacts {
        UnitDangerFacts {
            is_valid_unit: true,
            is_on_map: true,
            role: UNIT_ROLE_DANGEROUS,
            x_internal: (rx * COORD_PER_RCELL) ^ COORD_XOR,
            y_internal: (ry * COORD_PER_RCELL) ^ COORD_XOR,
            attack,
        }
    }

    fn plain_build(rx: i32, ry: i32) -> BuildDangerFacts {
        BuildDangerFacts {
            is_valid_wall: true,
            is_active: true,
            city: 0,
            build_flags: 0,
            x_internal: (rx * COORD_PER_RCELL) ^ COORD_XOR,
            y_internal: (ry * COORD_PER_RCELL) ^ COORD_XOR,
            ..Default::default()
        }
    }

    #[test]
    fn region_of_matches_the_div_3_table_composition_on_both_signs() {
        // `div_3_table[(c ^ k) >> 9]` where the table is `floor(i/3)`.
        for raw in [
            -4000i32, -1537, -1536, -1535, -1, 0, 1, 1535, 1536, 3071, 3072,
        ] {
            let stored = raw ^ COORD_XOR;
            let expected = floor_div(raw >> 9, 3);
            assert_eq!(region_of(stored), expected, "raw {raw}");
            assert_eq!(region_of(stored), floor_div(raw, COORD_PER_RCELL));
        }
    }

    #[test]
    fn neighbour_offsets_are_the_eight_surrounding_cells_once_each() {
        let mut seen: Vec<(i32, i32)> =
            (0..8).map(|k| (NEIGHBOUR_DX[k], NEIGHBOUR_DY[k])).collect();
        seen.sort_unstable();
        let mut expected: Vec<(i32, i32)> = (-1..=1)
            .flat_map(|y| (-1..=1).map(move |x| (x, y)))
            .filter(|&(x, y)| (x, y) != (0, 0))
            .collect();
        expected.sort_unstable();
        assert_eq!(seen, expected);
    }

    #[test]
    fn pass_one_skips_absent_planes_and_leaders_without_the_process_bit() {
        let mut planes = Planes::new(4, 4);
        planes.planes[0][0] = 99;
        planes.planes[1][0] = 99;
        planes.planes[2][0] = 99;
        planes.present[1] = false;
        let mut host = two_players();
        host.leaders[2].flags = flag::IN_GAME; // in game, but no PROCESS bit

        let trace = calc_danger(&mut planes, &host).unwrap();

        assert_eq!(trace.planes_cleared, 1);
        assert_eq!(planes.planes[0][0], 0, "slot 0 is wiped");
        assert_eq!(planes.planes[1][0], 99, "a null plane is not a zero plane");
        assert_eq!(planes.planes[2][0], 99, "flags & 2 is the wipe gate");
    }

    #[test]
    fn a_unit_deposits_on_the_enemy_and_digs_a_well_in_its_own_plane() {
        let mut planes = Planes::new(4, 4);
        let mut host = two_players();
        host.unit_end[0] = 1;
        host.units.push(((0, 0), armed_unit(1, 1, 40)));

        let trace = calc_danger(&mut planes, &host).unwrap();

        let cell = (1 * 4 + 1) as usize;
        assert_eq!(trace.units_scanned, 1);
        assert_eq!(planes.planes[1][cell], 20, "(40 * 5) / 10");
        assert_eq!(
            planes.planes[0][cell], 0,
            "pass 2 skips the owner's own slot outright"
        );
        assert_eq!(
            trace.attack_calls, 1,
            "one reached target, one attack() call"
        );
    }

    #[test]
    fn the_unit_diplomacy_gate_needs_only_one_side_at_zero() {
        let mut planes = Planes::new(4, 4);
        let mut host = two_players();
        host.unit_end[0] = 1;
        host.units.push(((0, 0), armed_unit(0, 0, 100)));
        host.leaders[0].diplos[1] = 1;
        host.leaders[1].diplos[0] = 1;

        let blocked = calc_danger(&mut planes, &host).unwrap();
        assert_eq!(blocked.units_scanned, 1);
        assert_eq!(blocked.do_danger_calls, 0, "both sides non-zero: skipped");

        host.leaders[1].diplos[0] = 0;
        let mut planes = Planes::new(4, 4);
        let admitted = calc_danger(&mut planes, &host).unwrap();
        assert_eq!(admitted.do_danger_calls, 1);
        // 50 halved once by `diplo == 1` on the *source* leader's row.
        assert_eq!(planes.planes[1][0], 25);
    }

    #[test]
    fn the_self_skip_and_the_reciprocal_lookup_use_leader_slot_not_the_loop_index() {
        // `0x00732E50` compares the target loop index against `leaders[who].slot`, and
        // `0x00732E5A` indexes the *other* leader's `diplos` by that same `slot`. A port that
        // uses the outer loop index instead is right only while the two agree.
        let mut planes = Planes::new(4, 4);
        let mut host = two_players();
        // Leader record 0 speaks for player slot 1.
        host.leaders[0].slot = 1;
        // Non-zero on the source row, so only the reciprocal lookup can admit target 0 —
        // and the index it uses is `slot` (1), not the outer loop index (0).
        host.leaders[0].diplos[0] = 3;
        host.leaders[0].diplos[1] = 0;
        host.unit_end[0] = 1;
        host.units.push(((0, 0), armed_unit(0, 0, 100)));

        let trace = calc_danger(&mut planes, &host).unwrap();

        assert_eq!(
            trace.do_danger_calls, 1,
            "target 1 is skipped as self by `slot`; target 0 is admitted by the reciprocal"
        );
        assert_eq!(
            planes.planes[1][0], 0,
            "the slot-skipped target gets nothing"
        );
        // `do_danger`'s identity arm compares the *loop index*, not `slot`, so the admitted
        // target 0 lands on the subtracting arm (0x0073239C).
        assert_eq!(planes.planes[0][0], -50);
    }

    #[test]
    fn an_unseen_attacker_registers_at_half_and_composes_with_the_treaty_halving() {
        let mut planes = Planes::new(4, 4);
        let mut host = two_players();
        host.unit_end[0] = 1;
        host.units.push(((0, 0), armed_unit(0, 0, 100)));
        host.unseen.push((0, 0, 1));

        let trace = calc_danger(&mut planes, &host).unwrap();
        assert_eq!(trace.is_seen_calls, 1);
        assert_eq!(planes.planes[1][0], 25, "50 halved once for invisibility");

        host.leaders[0].diplos[1] = 1;
        let mut planes = Planes::new(4, 4);
        calc_danger(&mut planes, &host).unwrap();
        assert_eq!(
            planes.planes[1][0], 12,
            "halved twice, truncating toward zero"
        );
    }

    #[test]
    fn an_ally_subtracts_half_and_never_consults_visibility() {
        let mut planes = Planes::new(4, 4);
        let mut host = two_players();
        host.unit_end[0] = 1;
        host.units.push(((0, 0), armed_unit(0, 0, 100)));
        host.leaders[0].diplos[1] = 2;
        host.unseen.push((0, 0, 1));

        let trace = calc_danger(&mut planes, &host).unwrap();

        assert_eq!(planes.planes[1][0], -25);
        assert_eq!(trace.is_seen_calls, 0, "the diplo==2 arm returns first");
    }

    #[test]
    fn a_building_paints_its_cell_and_its_eight_neighbours_at_half() {
        let mut planes = Planes::new(4, 4);
        let mut host = two_players();
        host.build_end[0] = BUILD_BAND_BASE + 1;
        host.builds.push(((0, BUILD_BAND_BASE), plain_build(1, 1)));

        let trace = calc_danger(&mut planes, &host).unwrap();

        assert_eq!(trace.builds_scanned, 1);
        assert_eq!(planes.planes[1][1 * 4 + 1], STRENGTH_PLAIN);
        assert_eq!(planes.planes[1][0], STRENGTH_PLAIN / 2, "north-west corner");
        assert_eq!(planes.planes[1][2 * 4 + 2], STRENGTH_PLAIN / 2);
        // Pass 3 has no self-skip: the owner's own plane gets the negative image.
        assert_eq!(planes.planes[0][1 * 4 + 1], -STRENGTH_PLAIN);
        assert_eq!(planes.planes[0][0], -(STRENGTH_PLAIN / 2));
    }

    #[test]
    fn building_neighbours_are_clipped_but_the_centre_is_not_recomputed() {
        let mut planes = Planes::new(4, 4);
        let mut host = two_players();
        host.build_end[0] = BUILD_BAND_BASE + 1;
        host.builds.push(((0, BUILD_BAND_BASE), plain_build(0, 0)));

        calc_danger(&mut planes, &host).unwrap();

        // Only the three in-range neighbours of (0,0) are painted: (1,0), (1,1), (0,1).
        let painted: Vec<usize> = (0..16)
            .filter(|&cell| planes.planes[1][cell] != 0)
            .collect();
        assert_eq!(painted, vec![0, 1, 4, 5]);
    }

    #[test]
    fn the_strength_ladder_short_circuits_in_retail_order() {
        let fort = BuildDangerFacts {
            wall_type_is_fort: true,
            is_tower: true,
            build_object_flags: BUILD_FLAG_FIXED_DANGER,
            hits_left: 401,
            ..Default::default()
        };
        assert_eq!(build_strength(&fort), (200, StrengthRung::Fort));

        let tower = BuildDangerFacts {
            wall_type_is_fort: false,
            build_object_flags: BUILD_FLAG_FIXED_DANGER,
            ..fort
        };
        assert_eq!(build_strength(&tower), (200, StrengthRung::Tower));

        let flagged = BuildDangerFacts {
            is_tower: false,
            is_airbase: true,
            ..tower
        };
        assert_eq!(build_strength(&flagged), (100, StrengthRung::FixedFlag));

        let airbase = BuildDangerFacts {
            build_object_flags: 0,
            ..flagged
        };
        assert_eq!(build_strength(&airbase), (100, StrengthRung::Airbase));

        let dock = BuildDangerFacts {
            is_airbase: false,
            is_dock: true,
            ..airbase
        };
        assert_eq!(build_strength(&dock), (100, StrengthRung::Dock));

        let high = BuildDangerFacts {
            is_dock: false,
            basic_type_build_flags: BUILD_FLAGS_HIGH_VALUE,
            ..dock
        };
        assert_eq!(build_strength(&high), (50, StrengthRung::BasicType));

        let plain = BuildDangerFacts {
            basic_type_build_flags: 0,
            ..high
        };
        assert_eq!(build_strength(&plain), (10, StrengthRung::BasicType));
    }

    #[test]
    fn a_cityless_building_needs_the_standalone_type_flag() {
        let mut host = two_players();
        host.build_end[0] = BUILD_BAND_BASE + 1;
        let mut facts = plain_build(1, 1);
        facts.city = -1;
        host.builds.push(((0, BUILD_BAND_BASE), facts));

        let mut planes = Planes::new(4, 4);
        assert_eq!(calc_danger(&mut planes, &host).unwrap().builds_scanned, 0);

        host.builds[0].1.build_flags = BUILD_FLAGS_STANDALONE;
        let mut planes = Planes::new(4, 4);
        assert_eq!(calc_danger(&mut planes, &host).unwrap().builds_scanned, 1);
    }

    #[test]
    fn an_off_plane_centre_is_refused_and_counted_rather_than_stored() {
        let mut planes = Planes::new(4, 4);
        let mut host = two_players();
        host.unit_end[0] = 1;
        host.units.push(((0, 0), armed_unit(99, 99, 40)));

        let trace = calc_danger(&mut planes, &host).unwrap();

        assert_eq!(trace.units_scanned, 1);
        assert_eq!(trace.do_danger_calls, 1);
        assert_eq!(trace.cells_written, 0);
        assert_eq!(trace.unmapped_centre_cells, 1);
        assert!(planes.planes[1].iter().all(|&c| c == 0));
    }

    #[test]
    fn preflight_failure_leaves_every_plane_untouched() {
        let mut planes = Planes::new(4, 4);
        planes.planes[0][3] = 7;
        let mut host = two_players();
        host.reject = true;
        host.unit_end[0] = 1;
        host.units.push(((0, 0), armed_unit(0, 0, 40)));

        assert_eq!(
            calc_danger(&mut planes, &host),
            Err(CalcDangerError::Host("no authoritative object band"))
        );
        assert_eq!(planes.planes[0][3], 7);
    }

    #[test]
    fn a_broken_region_lattice_is_refused_before_the_host_is_consulted() {
        struct Broken;
        impl DangerPlanes for Broken {
            fn reg_xs(&self) -> i32 {
                4
            }
            fn reg_ys(&self) -> i32 {
                4
            }
            fn reg_size(&self) -> i32 {
                15
            }
            fn plane_present(&self, _who: usize) -> bool {
                true
            }
            fn clear_plane(&mut self, _who: usize) {
                unreachable!("must not mutate")
            }
            fn cell_mut(&mut self, _who: usize, _cell: i32) -> Option<&mut i32> {
                unreachable!("must not mutate")
            }
        }
        let mut host = two_players();
        host.reject = true;
        assert_eq!(
            calc_danger(&mut Broken, &host),
            Err(CalcDangerError::RegionLattice {
                reg_xs: 4,
                reg_ys: 4,
                reg_size: 15,
            })
        );
    }

    #[test]
    fn unit_strength_is_the_retail_five_tenths_not_a_shift() {
        assert_eq!(unit_strength(0), 0);
        assert_eq!(unit_strength(3), 1);
        assert_eq!(unit_strength(-3), -1, "truncates toward zero, like idiv");
        assert_eq!(unit_strength(7), 3);
    }
}
