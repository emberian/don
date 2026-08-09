//! Target acquisition and ranking — the half of combat that was missing.
//!
//! Lane `assembly:target-selection`. Channel served: **`units`**, through
//! `ObjectData +0x34 near_o` / `+0x36 near_who` / `+0x3D targeted`, all three of which sit
//! inside `Object::walk_data`'s checksummed `[32, 66)` byte window, so **target selection
//! writes sim-critical state** and a divergent choice desyncs the `units` channel directly.
//!
//! # What this module is
//!
//! `crate::mechanics::damage` (`ObjectData::get_damage` `0x00644130`) is the best-tested
//! thing in the project — 7.99 M oracle trials — and until now nothing chose what to point
//! it at. This is that chooser, and the spatial structure it walks.
//!
//! ```text
//! Unit::find_new_target        0x005FF6A0  unit.cpp   the per-frame entry
//!  └─ Unit::find_melee_target  0x005FF9C0             picks the search radius
//!      └─ Object::find_nearby_target 0x00648DA0       the spiral over World::wdata cells
//!          ├─ Object::valid_target   0x00648BA0       domain / diplomacy gate
//!          ├─ Object::check_target   0x00649E00       distance + region gate
//!          │   ├─ ObjectData::attack_dist 0x006488F0  edge-to-edge distance
//!          │   ├─ ObjectData::is_in_range 0x006486B0
//!          │   └─ Object::poor_target     0x0064A270  (ported: `combat::poor_target`)
//!          └─ Object::compare_target 0x0064E5C0       the priority score
//!              └─ ObjectData::get_damage 0x00644130   (crate::mechanics::damage)
//! Unit::fight                  0x005FD4D0
//!  ├─ find_angle               0x0092D130             -> attack_dir  (see section 8)
//!  └─ Object::do_damage        0x0064A480             (crate::systems::combat)
//! ```
//!
//! # Provenance and fidelity
//!
//! Everything is `[measured]` against `ron-bin/riseofnations.exe` (sha256 `30478a44…625079`)
//! plus `ron-bin/sbl/rise.pdb`. Structure comes from `re/decomp-all/{0064e5c0,00648da0,
//! 00649e00,005ff9c0,005fd4d0}.c`; every constant, shift and divisor below was re-read in
//! capstone output, and every vtable slot was resolved by dumping the real `Unit`
//! (`0x00B417D0`), `Build` (`0x00B42174`) and `Wall` (`0x00B42CF8`) vftables and naming the
//! function each slot points at.
//!
//! **Fidelity is Tier C throughout: never executed against retail.** The oracle cannot reach
//! `compare_target` (3,553 B, walks the object table, the leader array, the world and
//! nineteen virtuals) or `find_nearby_target` (4,042 B, needs a populated `World::wdata`).
//! Nothing here is differentially tested. Do not promote it by proximity to
//! `crate::mechanics::damage`.
//!
//! # The vtable slots, resolved
//!
//! Both `compare_target` and `find_nearby_target` are almost entirely virtual dispatch, and
//! reading them at all required naming the slots. Dumped from the real vftables
//! [measured]; `0x0041BFF0` is `xor eax,eax; ret`, `0x0041E0E0` is `mov eax,1; ret`,
//! `0x0041C000` is `mov eax,ecx; ret`, `0x0046CDA0` is `return this->flags8 & 1`.
//!
//! | slot | Unit | Build | Wall | meaning |
//! |---|---|---|---|---|
//! | `+0x08` | `flags8 & 1` | 0 | 0 | `is_live_unit` |
//! | `+0x0C` | 0 | `flags8 & 1` | `flags8 & 1` | `is_live_build` |
//! | `+0x18` | 1 | 0 | 0 | **`is_unit`** |
//! | `+0x1C` | 0 | 1 | 1 | `is_build` (walls included) |
//! | `+0x20` | 0 | 1 | 0 | **`is_building`** (walls excluded) |
//! | `+0x2C` | 0 | `BuildData::is_wonder` | 0 | `is_wonder` |
//! | `+0x4C` | `flags8 & 1` | `flags8 & 1` | `flags8 & 1` | `is_alive` |
//! | `+0xAC`,`+0xB0` | 0 | `return this` | — | building self-cast |
//! | `+0xC0` | `UnitData::is_plane` | 0 | 0 | exact footprint-bypass gate in `attack_dist` |
//! | `+0xCC` | `is_supply` | 0 | 0 | |
//! | `+0xD8` | `is_moving` | 0 | 0 | |
//! | `+0x114` | `hits_left` | `hits_left` | | |
//! | `+0x120` | `UnitData::attack` | `BuildData::attack` | | |
//! | `+0x148` | `has_objmask` | `BuildData::has_objmask` | | |
//! | `+0x1B8` | 0 | `get_garrison_arrows` | | |
//!
//! **Correction to `crates/don-sim/src/mechanics.rs`** (two sentences, per the standing
//! rule): `DamagePredicates::attacker_vf_0x18` is documented as *"alive-shaped"*. Slot
//! `+0x18` is `is_unit` — `Unit` returns 1, `Build` and `Wall` return 0, and the alive test
//! is slot `+0x4C`; `combat.rs`'s `PoorTargetInput::both_are_units` already reads it
//! correctly. The field is an input either way, so no behaviour changes; only the doc is
//! wrong.

use crate::mechanics::flank_level;
use crate::systems::combat::{circle_table, in_attack_range, vector_dist, CircleTable};
use crate::systems::held_target::{
    attack_distance, AttackDistanceInput, AttackDistanceMode, ObjectFootprint,
};
use crate::trig::find_angle;

// ===========================================================================================
// 1. Coordinate spaces: world units, tiles, and acquisition cells
// ===========================================================================================

/// World units per **tile**. `div_3_table[c >> 6] == c / 192`.
///
/// Same constant as `combat::RANGE_UNITS_PER_TILE`, restated here because every distance in
/// this module is denominated in it and the two modules must not drift.
pub const TILE_UNITS: i32 = 192;

/// World units per **acquisition cell** — one `WData` record. `div_3_table[c >> 8] == c / 768`.
///
/// Four tiles. This is the granularity of the engine's target-search grid, and therefore the
/// granularity any faithful reimplementation must use: a finer or coarser grid changes which
/// candidates are visited in which order, and `find_nearby_target` stops early (section 5),
/// so visit order changes the *answer*, not just the cost.
pub const CELL_UNITS: i32 = 768;

/// `div_3_table` `0x00CAE5FC` — built by `init_coord_lookup_array` `0x00681DB0`.
///
/// A signed-symmetric divide-by-three lookup: `t[k] = k / 3` for `k` in `[0, n·0x18)` filled
/// by the forward loop at `0x00681DF0`, and `t[k] = k / 3` for `k` in `[-n·0x18, 0)` filled
/// by the backward loop at `0x00681E12`. Both use C truncation toward zero, so this is
/// `i32::wrapping_div`, **not** a floor.
///
/// Retail never bounds-checks the index; a coordinate outside `±n·0x18` reads past the
/// allocation. We compute instead of tabulating, which is exact inside the table's domain
/// and defined outside it.
#[inline]
pub fn div_3(k: i32) -> i32 {
    k / 3
}

/// World coordinate (already unmasked — see [`unmask_coord`]) → acquisition cell index.
///
/// `div_3_table[c >> 8]` at `0x00649319`, `0x00649330`. Note the shift is **arithmetic**
/// (`sar`) and the divide truncates, so the two operations do not compose into a single
/// `c / 768` for negative `c`; this reproduces retail exactly.
#[inline]
pub fn cell_of(coord: i32) -> i32 {
    div_3(coord >> 8)
}

/// World coordinate → tile index. `div_3_table[c >> 6]`, e.g. `0x00649E38` in
/// `Object::check_target`.
#[inline]
pub fn tile_of(coord: i32) -> i32 {
    div_3(coord >> 6)
}

/// `ObjectData +0x10 x` / `+0x14 y` are stored XOR `0x00063637`.
///
/// Every read in the acquisition path unmasks first (`xor eax, 0x63637` at `0x005FE878`,
/// `0x0064930F`, …). `GuyData` coordinates are **not** masked; do not reuse this there.
#[inline]
pub fn unmask_coord(raw: i32) -> i32 {
    raw ^ 0x0006_3637
}

// ===========================================================================================
// 2. The acquisition grid — `World::wdata`, an intrusive per-cell object list
// ===========================================================================================

/// The sentinel in every link field. Retail stores `-1` and tests `-1 < value`.
pub const NO_LINK: i16 = -1;

/// One `WData` record's object-list head — `WData +0x08 down`, `+0x0A down_who`
/// [measured, `schema/pdb-types.json`, `WData` is 28 bytes].
///
/// `World::wdata` (`World +0x134`, `WData*`) is a flat `xs × ys` array indexed
/// `xs * cy + cx`. Cell `(cx, cy)` covers world coordinates
/// `[cx·768, (cx+1)·768) × [cy·768, (cy+1)·768)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CellHead {
    /// `+0x08 down` — index of the first object in this cell, or [`NO_LINK`].
    pub down: i16,
    /// `+0x0A down_who` — its owning player.
    pub down_who: i16,
    /// `+0x0F who` — the cell's territory owner, `-1` when unclaimed. Read by
    /// `Object::check_target` at `0x0064A0F5` as one of the arms that admits a candidate.
    pub who: i8,
}

impl CellHead {
    /// A fresh, empty cell: `down = down_who = -1`, unowned territory.
    pub const EMPTY: CellHead = CellHead {
        down: NO_LINK,
        down_who: NO_LINK,
        who: -1,
    };
}

/// `(index, player)` — how the engine names an object everywhere in this path.
///
/// Both halves are needed because `Objects` is per-player: `objects[who].list[o]`
/// (`Objects +0x1C·who + 0x14`, then `+4·o`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjRef {
    /// `ObjectData +0x0A`, the per-player slot index.
    pub o: i16,
    /// `ObjectData +0x09`, the owning player.
    pub who: i16,
}

impl Default for ObjRef {
    /// [`ObjRef::NONE`] — retail's `-1 / -1`, never a valid slot. Defaulting to `(0, 0)`
    /// would make an uninitialised `CompareTargetInput` claim to be player 0's object 0.
    fn default() -> Self {
        ObjRef::NONE
    }
}

impl ObjRef {
    /// The "no object" pair the acquisition path returns and stores.
    pub const NONE: ObjRef = ObjRef {
        o: NO_LINK,
        who: NO_LINK,
    };

    /// Construct a reference.
    #[inline]
    pub fn new(o: i16, who: i16) -> Self {
        ObjRef { o, who }
    }

    /// True when this is a real object slot.
    #[inline]
    pub fn is_some(&self) -> bool {
        self.o >= 0
    }
}

/// The subset of `ObjectData`/`UnitData` the acquisition path reads, at PDB offsets.
///
/// This is a *row*, not a whole object: everything the search and the ranking touch, and
/// nothing else. Coordinates are stored **unmasked** here; unmask on the way in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TargetRow {
    /// `+0x08` object flags. Bit 0 = alive (`vf +0x4C`), bit 5 (`0x20`) = city centre —
    /// the same bit `get_damage` step 30 reads for `RECAPTURE_CITY_MODIFIER`.
    pub flags8: u8,
    /// `+0x10 x`, **unmasked**.
    pub x: i32,
    /// `+0x14 y`, **unmasked**.
    pub y: i32,
    /// `+0x24 damage` — non-zero means "already hurt", worth `×3/2` to a tower.
    pub damage: i32,
    /// `+0x2C down` — next object in this cell.
    pub down: i16,
    /// `+0x2E down_who`.
    pub down_who: i16,
    /// `+0x34 near_o` — the cached nearest valid target. **Checksummed.**
    pub near_o: i16,
    /// `+0x36 near_who`. **Checksummed.**
    pub near_who: i16,
    /// `+0x3D targeted` — how many attackers have picked this object. **Checksummed**, and
    /// the engine's target-spreading term (section 5). Saturates at 100 (`'d'`).
    pub targeted: i8,
    /// `UnitData +0xAC full`. Divides the final score at `0x0064F1D5`.
    pub full: i8,
    /// `vf +0x18` — the object is a `Unit`.
    pub is_unit: bool,
    /// `vf +0x1C` — the object is a `Build` **or** a `Wall`.
    pub is_build: bool,
    /// `vf +0x20` — the object is a `Build` and not a `Wall`.
    pub is_building: bool,
    /// `vf +0x2C` — `BuildData::is_wonder` `0x00472320`.
    pub is_wonder: bool,
}

impl TargetRow {
    /// `vf +0x4C` / `+0x08` / `+0x0C`, all of which reduce to `flags8 & 1`
    /// (`0x0046CDA0`: `movzx eax, byte [ecx+8]; and eax, 1`).
    #[inline]
    pub fn is_alive(&self) -> bool {
        self.flags8 & 1 != 0
    }

    /// `flags8 & 0x20` — the city-centre bit, `0x0064E8A9` and `get_damage` step 30.
    #[inline]
    pub fn is_city_centre(&self) -> bool {
        self.flags8 & 0x20 != 0
    }
}

/// The object table plus its spatial index: `Objects` (`0x00C0AEAC`) and `World::wdata`
/// together, because retail keeps them in lockstep and so must we.
///
/// Ten player slots, matching the `(frame + i) % 10` rotation in `Objects::process_all`
/// `0x0065DCE0`.
#[derive(Clone, Debug)]
pub struct TargetWorld {
    /// Cell-grid width, `World +0x00 xs`.
    pub xs: i32,
    /// Cell-grid height, `World +0x04 ys`.
    pub ys: i32,
    /// `World +0x134 wdata`, `xs × ys` records.
    pub cells: Vec<CellHead>,
    /// `objects[who].list` — one dense slot array per player.
    pub objects: Vec<Vec<TargetRow>>,
}

/// `Objects` player-slot count. The rotation modulus in `Objects::process_all` `0x0065DCE0`.
pub const PLAYER_SLOTS: usize = 10;

impl TargetWorld {
    /// An empty world of `xs × ys` acquisition cells (each 768 world units square).
    pub fn new(xs: i32, ys: i32) -> Self {
        assert!(xs > 0 && ys > 0, "world must have at least one cell");
        TargetWorld {
            xs,
            ys,
            cells: vec![CellHead::EMPTY; (xs * ys) as usize],
            objects: vec![Vec::new(); PLAYER_SLOTS],
        }
    }

    /// Linear cell index, or `None` outside the grid. Retail's bounds test at `0x00649354`
    /// is exactly `0 <= cx < xs && 0 <= cy < ys`.
    #[inline]
    pub fn cell_index(&self, cx: i32, cy: i32) -> Option<usize> {
        if cx < 0 || cy < 0 || cx >= self.xs || cy >= self.ys {
            None
        } else {
            Some((self.xs * cy + cx) as usize)
        }
    }

    /// Borrow a row.
    #[inline]
    pub fn row(&self, r: ObjRef) -> Option<&TargetRow> {
        self.objects
            .get(r.who as usize)
            .and_then(|p| p.get(r.o as usize))
    }

    /// Borrow a row mutably.
    #[inline]
    pub fn row_mut(&mut self, r: ObjRef) -> Option<&mut TargetRow> {
        self.objects
            .get_mut(r.who as usize)
            .and_then(|p| p.get_mut(r.o as usize))
    }

    /// Add an object to player `who`, returning its reference. Does **not** link it into a
    /// cell; call [`Self::link`] for that.
    pub fn push(&mut self, who: i16, mut row: TargetRow) -> ObjRef {
        row.down = NO_LINK;
        row.down_who = NO_LINK;
        row.near_o = NO_LINK;
        row.near_who = NO_LINK;
        let list = &mut self.objects[who as usize];
        list.push(row);
        ObjRef::new((list.len() - 1) as i16, who)
    }

    /// Install an object at its retail `(o, who)` address and link it at the head of its
    /// `WData` cell.
    ///
    /// Arena and replay hosts generally already have object ids; allocating another dense
    /// id with [`Self::push`] would destroy `Objects::list[o]` identity.  Missing lower
    /// slots are filled with dead tombstones, just like holes in retail's per-player object
    /// arrays.  A live slot is never silently replaced because doing so would leave its old
    /// intrusive link in the world.
    pub fn place_at(&mut self, r: ObjRef, mut row: TargetRow) -> bool {
        if r.o < 0 || r.who < 0 || r.who as usize >= self.objects.len() {
            return false;
        }
        let list = &mut self.objects[r.who as usize];
        let slot = r.o as usize;
        if list.len() <= slot {
            list.resize(slot + 1, TargetRow::default());
        }
        if list[slot].is_alive() {
            return false;
        }
        row.down = NO_LINK;
        row.down_who = NO_LINK;
        row.near_o = NO_LINK;
        row.near_who = NO_LINK;
        list[slot] = row;
        self.link(r);
        true
    }

    /// The cell an object currently occupies, from its own coordinates.
    #[inline]
    pub fn cell_of_ref(&self, r: ObjRef) -> Option<usize> {
        let row = self.row(r)?;
        self.cell_index(cell_of(row.x), cell_of(row.y))
    }

    /// Thread an object onto the head of its cell's list.
    ///
    /// `Object::add_to_world` `0x0064D8C0` inserts at the head: the old
    /// `WData +0x08/+0x0A` pair becomes this row's `ObjectData +0x2C/+0x2E`, then the cell
    /// head becomes `r`.  This order is sim-critical because equal target scores retain the
    /// first candidate visited.
    pub fn link(&mut self, r: ObjRef) {
        let Some(cell) = self.cell_of_ref(r) else {
            return;
        };
        let head = self.cells[cell];
        if let Some(row) = self.row_mut(r) {
            row.down = head.down;
            row.down_who = head.down_who;
        }
        self.cells[cell].down = r.o;
        self.cells[cell].down_who = r.who;
    }

    /// Move an already-linked object, reproducing `Object::set_new_location`'s spatial
    /// phase: remove from the old chain before changing coordinates, then add at the head
    /// of the new cell.  Movement within one cell only changes coordinates and preserves
    /// chain order.
    pub fn relocate(&mut self, r: ObjRef, x: i32, y: i32) -> bool {
        let Some(old_row) = self.row(r).copied() else {
            return false;
        };
        if !old_row.is_alive() {
            return false;
        }
        let old = (cell_of(old_row.x), cell_of(old_row.y));
        let new = (cell_of(x), cell_of(y));
        if old == new {
            let row = self.row_mut(r).expect("row was resolved above");
            row.x = x;
            row.y = y;
            return true;
        }
        if !self.unlink_from_cell(r, old.0, old.1) {
            return false;
        }
        {
            let row = self.row_mut(r).expect("row was resolved above");
            row.x = x;
            row.y = y;
        }
        self.link(r);
        true
    }

    /// Remove an object from its spatial chain and mark the slot dead.  The slot remains a
    /// tombstone so later `(o, who)` addresses do not shift.
    pub fn remove(&mut self, r: ObjRef) -> bool {
        let Some(row) = self.row(r).copied() else {
            return false;
        };
        if !row.is_alive() || !self.unlink_from_cell(r, cell_of(row.x), cell_of(row.y)) {
            return false;
        }
        let row = self.row_mut(r).expect("row was resolved above");
        row.flags8 &= !1;
        row.down = NO_LINK;
        row.down_who = NO_LINK;
        true
    }

    fn unlink_from_cell(&mut self, r: ObjRef, cx: i32, cy: i32) -> bool {
        let Some(cell) = self.cell_index(cx, cy) else {
            // Off-map objects are not present in a WData chain.
            if let Some(row) = self.row_mut(r) {
                row.down = NO_LINK;
                row.down_who = NO_LINK;
                return true;
            }
            return false;
        };
        let head = ObjRef::new(self.cells[cell].down, self.cells[cell].down_who);
        if head == r {
            let row = *self.row(r).expect("linked row must exist");
            self.cells[cell].down = row.down;
            self.cells[cell].down_who = row.down_who;
            let row = self.row_mut(r).expect("linked row must exist");
            row.down = NO_LINK;
            row.down_who = NO_LINK;
            return true;
        }

        let mut cur = head;
        let budget: usize = self.objects.iter().map(Vec::len).sum::<usize>() + 1;
        for _ in 0..budget {
            let Some(row) = self.row(cur).copied() else {
                break;
            };
            let next = ObjRef::new(row.down, row.down_who);
            if next == r {
                let victim = *self.row(r).expect("linked row must exist");
                let pred = self.row_mut(cur).expect("predecessor must exist");
                pred.down = victim.down;
                pred.down_who = victim.down_who;
                let victim = self.row_mut(r).expect("linked row must exist");
                victim.down = NO_LINK;
                victim.down_who = NO_LINK;
                return true;
            }
            if !next.is_some() {
                break;
            }
            cur = next;
        }
        false
    }

    /// Walk one cell's chain in retail order, collecting `(o, who)` pairs.
    ///
    /// Bounded by the number of rows in the world so a corrupted link cannot hang the
    /// caller; retail has no such guard.
    pub fn cell_chain(&self, cell: usize) -> Vec<ObjRef> {
        let mut out = Vec::new();
        let mut cur = ObjRef::new(self.cells[cell].down, self.cells[cell].down_who);
        let budget: usize = self.objects.iter().map(|p| p.len()).sum::<usize>() + 1;
        while cur.is_some() && out.len() < budget {
            let Some(row) = self.row(cur) else { break };
            out.push(cur);
            cur = ObjRef::new(row.down, row.down_who);
        }
        out
    }
}

// ===========================================================================================
// 3. Distance
// ===========================================================================================

/// Historical centre-distance/scalar-footprint helper.
///
/// This is **not** `ObjectData::attack_dist` `0x006488F0`: that function snaps anchors and
/// subtracts both objects' x/y extents independently.  Automatic acquisition uses the exact
/// [`attack_distance`] path below.  The generic grid benchmark and legacy [`Engagement`]
/// scaffold retain this private helper until those non-runtime examples are removed.
#[inline]
fn scalar_attack_dist(ax: i32, ay: i32, tx: i32, ty: i32, target_footprint: i32) -> i32 {
    let d = vector_dist(tx.wrapping_sub(ax), ty.wrapping_sub(ay));
    (d - target_footprint).max(0)
}

// ===========================================================================================
// 4. `Object::compare_target` `0x0064E5C0` — the priority score
// ===========================================================================================

/// Everything `compare_target` reads, pre-resolved.
///
/// Same shape as `combat::PoorTargetInput`: retail reaches all of this through nineteen
/// virtual calls and four global tables, which this crate does not model, so each is an
/// input named for the retail read that produces it. Filling them in correctly is the
/// integration problem; getting the arithmetic between them right is this struct's job.
///
/// `a_` = attacker (`this`), `t_` = target (`objects[who][o]`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompareTargetInput {
    // ---- mode ----
    /// `param_3` — ask `ObjectData::is_in_range` and divide by 5 when it says no
    /// (`0x0064F20D`).
    pub check_path: bool,
    /// `param_4` — retail's second scoring mode. Set by `find_nearby_target`'s `local_40`,
    /// which is on when the owner is not AI-flagged (`leaders[who] & 4`), `0x006EC000`
    /// returns 0 and `Game +0x821 & 2` is clear. It multiplies the base by 10 up front and
    /// **divides** by the damage estimate at the end instead of multiplying.
    pub mode: bool,

    // ---- attacker ----
    /// `this->vf[0x18]()` — the attacker is a unit.
    pub a_is_unit: bool,
    /// The attacker's own `(o, who)`, read from `ObjectData +0x0A/+0x09`. The target's
    /// CastSpell activity payload is compared against it at `0x0064ECDE..0x0064ECF0`.
    pub a_ref: ObjRef,
    /// `this->vf[0x20]()` — the attacker is a building (a tower/fort firing on its own).
    pub a_is_building: bool,
    /// `UnitData +0xB1 stance == 3` after the `has_stance_type` devirtualisation at
    /// `0x0064E68A`. Retail calls this the combat stance; tier 3 suppresses the whole
    /// class-multiplier block at `0x0064E7A6`.
    pub a_stance_is_3: bool,
    /// `UnitData +0x68 unit_masks & 0x40000` (`0x0064E717`).
    pub a_unit_mask_0x40000: bool,
    /// `this->type[+0x218] domain == 1` (sea), `0x0064E729`.
    pub a_domain_is_sea: bool,
    /// `this->type[+0x218] domain == 2` (air), `0x0064F13B`.
    pub a_domain_is_air: bool,
    /// `this->has_objmask(0x40000)`, `0x0064E742`.
    pub a_has_objmask_0x40000: bool,
    /// `this->has_objmask(0x80000000)`, `0x0064F1D9`. The anti-air bit.
    pub a_has_objmask_high: bool,
    /// `this->type[+0x1E4] obj_masks & 0x40000`, `0x0064F1A9`.
    pub a_type_mask_0x40000: bool,
    /// `this->type->vf[0x108]()`, `0x0064E76A`.
    pub a_type_vf_0x108: bool,
    /// `leaders[this->who] & 4` — the AI-controlled flag, `0x0064E7B4`.
    pub a_leader_flag_4: bool,
    /// The attacker's own current target (`BuildData +0x7C` / `+0x81`), compared against
    /// `(o, who)` at `0x0064EFA3`. Only read on the `a_is_building` arm.
    pub a_current_target: Option<ObjRef>,
    /// `this->vf[0x1B8]()` — `BuildData::get_garrison_arrows` `0x0062E3D0`.
    pub a_garrison_arrows: i32,

    // ---- target ----
    /// `objects[who][o]`, index and player. Compared against `a_current_target`.
    pub t_ref: ObjRef,
    /// `target->vf[0x4C]()` — alive.
    pub t_alive: bool,
    /// `target->vf[0x20]()` — the target is a building. **The whole first valuation block
    /// is gated on this**, which is why buildings and units score on different scales.
    pub t_is_building: bool,
    /// The spellcaster discriminator at `0x0064EC6E..0x0064EC95`: for a unit this is
    /// `UnitTypeData +0x2B8 unit_flags2 & 2`; for a building it is
    /// `BuildTypeData::is_spellcaster` `0x00639840`.
    pub t_is_spellcaster: bool,
    /// The target's `UnitData::get_action` `0x00608450` currently returns activity
    /// [`crate::order::OrderIndex::CastSpell`] (`0xE`). Only read for a spellcaster.
    pub t_action_is_cast_spell: bool,
    /// The `(o, who)` pair returned by the target CastSpell activity's virtual `+0xF4`.
    /// Retail compares this with the **attacker's** identity at `0x0064ECDE..0x0064ECF0`.
    pub t_action_target: Option<ObjRef>,
    /// `target->is(0x3A)`, consulted when the spellcaster target is not casting. It adds
    /// 6,000,000 before the common score compression (`0x0064ECF8..0x0064ED27`).
    pub t_is_tech_0x3a: bool,
    /// `target->vf[0x2C]()` — a wonder. Divides the base by 25 at `0x0064E70A`.
    pub t_is_wonder: bool,
    /// `target->vf[0x148](0x80000000)`, only evaluated for buildings (`0x0064E60D`).
    pub t_has_objmask_high: bool,
    /// `objecttypes[target->type[+4]]->vf[0xD0]()` — the type's own value. Shifted left 2
    /// at `0x0064E6E4` to form the base score.
    pub t_type_value: i32,
    /// `target->flags8 & 0x20` — a city centre. Worth `base × 5`.
    pub t_is_city_centre: bool,
    /// `WallData::is_defensive` `0x00473650` on the target. `base × 4`.
    pub t_is_defensive_wall: bool,
    /// `BuildTypeData::is_military_trainer` `0x0063BCF0`. `base × 3`.
    pub t_is_military_trainer: bool,
    /// `BuildTypeData::is_training_building` `0x00639DE0`. `base × 2`.
    pub t_is_training_building: bool,
    /// `target->is(0x208)` plus the two captain tests at `0x0064E88E`, which together
    /// promote a military trainer from `×3` to `×15`.
    pub t_trainer_is_active: bool,
    /// `target->type[+0x1E8] != 0` — the type carries hit points, gating the whole
    /// attack/hits-left term at `0x0064EA4E`.
    pub t_type_has_hits: bool,
    /// `target->vf[0x114]()` — `ObjectData::hits_left` `0x006535C0`. A **divisor** at
    /// `0x0064EAF0`; zero skips the term rather than dividing.
    pub t_hits_left: i32,
    /// `target->vf[0x120]()` — attack. Numerator of the same term.
    pub t_attack: i32,
    /// `target[+0x24] damage != 0`, `0x0064EFD9`.
    pub t_is_damaged: bool,
    /// `target->vf[0xD8]()` — `UnitData::is_moving` `0x00610AF0`.
    pub t_is_moving: bool,
    /// `target->vf[0xCC]()` — `UnitData::is_supply` `0x0046CE80`. `× 5000`.
    pub t_is_supply: bool,
    /// `target->type[+0x218] domain == 2` — an air unit. Forces the return to 2 unless the
    /// attacker has the anti-air objmask (`0x0064F1C6`).
    pub t_domain_is_air: bool,
    /// `target[+0x68] unit_masks & 1` combined with `UnitData::is_detected` `0x0060A630`:
    /// an undetected stealth unit is priority 1 (`0x0064F1AE`).
    pub t_stealth_undetected: bool,
    /// `target->type[+0x2C8] & 0x10000`, `0x0064EC30`. `× 20`, and later `+1000000`.
    pub t_type_mask_0x10000: bool,
    /// `target->type[+0x2B4] & 0x200000`, `0x0064E75A` and `0x0064EE18`.
    pub t_type_mask_0x200000: bool,
    /// `target->type[+0x2B4] & 0x10`, `0x0064EE9E`. `+900000`.
    pub t_type_mask_0x10: bool,
    /// `target->is(0x150)`, `0x0064EE41`.
    pub t_is_tech_0x150: bool,
    /// `target->is(0x13D)`, `0x0064EF07`.
    pub t_is_tech_0x13d: bool,
    /// `target->vf[0x114]() == 0` while `flags8 & 0x20` and alive — the "standing but
    /// dead" case that pins the score at 99999 (`0x0064F1F5`).
    pub t_empty_shell: bool,
    /// `UnitData +0xAC full`. The final divide, `score / (full + 1)`, `0x0064F1D5`.
    pub t_full: i32,
    /// The `ObjectData::get_damage` estimate: retail calls it with
    /// `attack_dir = find_angle(0,0) = 0` and all three trailing flags zero
    /// (`0x0064EC10`). Feed `crate::mechanics::damage` with those arguments.
    pub estimated_damage: i32,
    /// `ObjectData::is_in_range(o, who, this->x, this->y, this->y, 0, 0)` `0x006486B0`,
    /// consulted only when [`Self::check_path`] is set.
    pub in_range: bool,
}

/// `Object::compare_target(o, who, check_path, mode)` — `0x0064E5C0`, object.cpp, 3,553 B.
///
/// Returns a **priority, higher is better**; `find_nearby_target` takes the maximum. The
/// return is clamped to `>= 15` (`0x0064F1BD`) except for two explicit demotions to `1` and
/// `2`, which is how "never shoot this" is expressed.
///
/// The shape, in retail order:
///
/// ```text
///  1. base   = type_value << 2                          0x0064E6E4
///  2. wonder: base /= 25                                0x0064E70A
///  3. if target is a live BUILDING:
///       sea-attacker vetoes -> return 0                 0x0064E75A / 0x0064E76A
///       mode: base *= 10                                0x0064E793
///       unless attacker stance 3 or AI-flagged:
///         city centre        -> base*5   (mode: 100)    0x0064E9B4
///         defensive wall     -> base*4   (mode: *40)    0x0064E9C7
///         military trainer   -> base*3   (active: *15)  0x0064E8xx
///         training building  -> base*2                  0x0064E9E2
///  4. threat: v = attack*v*100 / hits_left               0x0064EAF0
///             (target a building and attacker stance 3 -> v /= 20 instead)
///  5. tower arm (attacker is a building)                 0x0064EF80..0x0064F002
///  6. v = v * damage      (mode: v = v / damage)         0x0064EC1F
///  7. building tail / unit tail                          0x0064EC30..0x0064F1EC
///  8. clamps: negatives -> 9999999, /(full+1), /5 out of range,
///             (v+99)/100, knee at 100000, floor 15, then 1 / 2 overrides
/// ```
///
/// Steps 3, 5 and 7 are transcribed branch for branch. The spell-target arm in step 7 is
/// supplied as pre-resolved target activity fields, just like the other virtual reads: a
/// non-casting tech `0x3A` spellcaster gets `+6,000,000`; any spellcaster presently casting
/// gets `×20`, plus `+10,000,000` when that CastSpell activity names the attacker.
pub fn compare_target(i: &CompareTargetInput) -> i32 {
    // 1 + 2: the base, from the type's own value.  0x0064E6E4 / 0x0064E70A
    let mut base = i.t_type_value.wrapping_shl(2);
    if i.t_is_wonder {
        base /= 25;
    }
    let mut v = base;

    // 3: the building-target valuation.  0x0064E77E
    if i.t_alive && i.t_is_building {
        if i.a_unit_mask_0x40000 && i.a_domain_is_sea {
            if i.t_type_mask_0x200000 {
                return 0; // 0x0064E75A
            }
            if i.a_type_vf_0x108 && !i.a_has_objmask_0x40000 {
                return 0; // 0x0064E76A
            }
        }
        if i.mode {
            base = base.wrapping_mul(10); // 0x0064E793
        }
        v = base;
        if !i.a_stance_is_3 && !i.a_leader_flag_4 {
            if i.t_is_city_centre {
                if i.mode {
                    return 100; // 0x0064E7C8
                }
                v = base.wrapping_mul(5);
            } else if i.t_is_defensive_wall {
                v = if i.mode {
                    base.wrapping_mul(20).wrapping_mul(2)
                } else {
                    base.wrapping_mul(4)
                };
            } else if i.t_is_military_trainer {
                v = base.wrapping_mul(3);
                if !i.mode && i.t_trainer_is_active {
                    v = base.wrapping_mul(15); // 0x0064E8C6
                }
                if i.a_domain_is_sea && i.a_unit_mask_0x40000 && i.a_has_objmask_0x40000 {
                    v = v.wrapping_mul(5000); // 0x0064E93B
                }
            } else if i.t_is_training_building {
                v = base.wrapping_mul(2); // 0x0064E9E2
            }
        }
    }

    // 4: the threat / fragility term.  0x0064EA4E..0x0064EB0A
    if i.t_type_has_hits && i.t_hits_left != 0 && !i.t_has_objmask_high {
        if !i.t_alive || !i.a_stance_is_3 || !i.t_is_building {
            v = i
                .t_attack
                .wrapping_mul(v)
                .wrapping_mul(100)
                .wrapping_div(i.t_hits_left);
            if i.t_is_building {
                v = v.wrapping_mul(10); // 0x0064EB0A
            }
        } else {
            v /= 20; // 0x0064EAFB
        }
    }

    // 5: the tower arm.  0x0064EF80..0x0064F002
    if i.a_is_building {
        if i.a_current_target == Some(i.t_ref) {
            v = if i.a_garrison_arrows < 2 {
                v.wrapping_mul(2)
            } else {
                v / 2
            };
        }
        if i.t_is_damaged {
            v = v.wrapping_mul(3) / 2;
        }
        if i.t_is_moving {
            v = sar2_round_toward_zero(v); // 0x0064EFF0
        }
        if i.t_is_supply {
            v = v.wrapping_mul(5000);
        }
    }

    // 6: the damage estimate.  0x0064EC10
    if i.mode {
        if i.estimated_damage == 0 {
            return 0;
        }
        v /= i.estimated_damage;
    } else {
        v = i.estimated_damage.wrapping_mul(v);
    }

    if i.t_is_building {
        // 7a: the building tail.  0x0064F0F0..0x0064F1EC
        if !i.a_stance_is_3 {
            let mut has_hits = i.t_type_has_hits;
            if i.t_has_objmask_high && !i.a_domain_is_air {
                has_hits = false; // 0x0064F117
            }
            // 0x0064F14E: an occupied city centre is skipped unless it holds someone.
            let admitted = !i.t_is_city_centre || i.t_trainer_is_active;
            if admitted && has_hits && !i.a_leader_flag_4 {
                if i.a_type_mask_0x40000 {
                    v = v.wrapping_add(1_000_000); // 0x0064F1A2
                } else {
                    v = v.wrapping_mul(5); // 0x0064F195
                }
            }
            if i.a_type_mask_0x40000 && has_hits {
                v = v.wrapping_add(100_000); // 0x0064F1B4
            }
        }
    } else {
        // 7b: the unit tail.  0x0064EC30..0x0064F002
        if i.t_type_mask_0x10000 {
            v = v.wrapping_mul(20); // 0x0064EC44
        }
        // 0x0064EC6E..0x0064ED27. The action belongs to the target: disassembly loads the
        // target from `objects[param_2][param_1]` into ECX immediately before both
        // UnitData::get_action calls. EDI still holds the attacker, whose +0xA/+0x9
        // identity is compared with the CastSpell payload at 0x0064ECDE.
        if i.t_is_spellcaster {
            if i.t_action_is_cast_spell {
                v = v.wrapping_mul(20);
                if i.t_action_target == Some(i.a_ref) {
                    v = v.wrapping_add(10_000_000);
                }
            } else if i.t_is_tech_0x3a {
                v = v.wrapping_add(6_000_000);
            }
        }
        if i.t_stealth_undetected {
            v = sar2_round_toward_zero(v); // 0x0064EFF0 sibling at 0x0064EE0A
        } else if i.a_stance_is_3 {
            if i.a_unit_mask_0x40000 {
                if i.a_domain_is_sea {
                    if i.t_is_tech_0x150 {
                        if i.t_is_tech_0x13d {
                            return 0; // 0x0064EEC5
                        }
                        if i.t_type_mask_0x200000 {
                            v /= 10; // via LAB_0064EE6E
                        }
                    } else if i.t_type_mask_0x200000 && i.t_is_tech_0x150 {
                        v = v.wrapping_add(1_000_000);
                    } else if i.t_type_mask_0x10 {
                        v = v.wrapping_add(900_000); // 0x0064EE97
                    } else if i.t_is_tech_0x13d {
                        v = v.wrapping_add(100_000); // 0x0064EF10
                    } else if i.t_type_mask_0x10000 {
                        v = v.wrapping_add(10_000); // 0x0064EF1F
                    }
                } else {
                    v = v.wrapping_add(900_000); // 0x0064EF60
                }
            } else if i.t_type_mask_0x10000 {
                v = v.wrapping_add(10_000);
            } else {
                v = v.wrapping_add(9_000_000); // 0x0064EE20
            }
        } else if !i.t_type_mask_0x10000 {
            // 0x0064EDA0: 4,000,000 when the target is a supply-flagged object the
            // attacker can actually reach, otherwise a flat 100,000.
            v = v.wrapping_add(if i.t_is_supply && i.a_is_building {
                4_000_000
            } else {
                100_000
            });
        } else {
            v = v.wrapping_add(1_000_000); // 0x0064EDD8
        }
        v /= i.t_full.wrapping_add(1); // 0x0064F1D5
    }

    // 8: the common tail.  0x0064F1ED
    if i.t_empty_shell {
        v = 99_999;
    } else if v < 0 {
        v = 9_999_999; // overflow saturates to "very attractive", not to "reject"
    }
    if i.check_path && !i.a_stance_is_3 && !i.in_range {
        v /= 5; // 0x0064F20D
    }
    let mut out = (v.wrapping_add(99)) / 100; // 0x0064F21A
    if out > 100_000 {
        out = (out - 99_996) / 5 + 100_000; // 0x0064F22C, continuous at the knee
    }
    if out < 15 {
        out = 15; // 0x0064F23A
    }
    if i.a_is_unit && i.t_stealth_undetected {
        out = 1; // 0x0064F26B
    }
    if i.t_domain_is_air && !i.a_has_objmask_high {
        out = 2; // 0x0064F28A
    }
    out
}

/// `(v + ((v >> 31) & 3)) >> 2` — retail's divide-by-4 rounding toward zero, emitted at
/// `0x0064EFF0` and `0x0064EE0A`. Not the same as `v / 4` for negative `v` in Rust? It is,
/// but the transcription is kept explicit because the assembly is explicit.
#[inline]
fn sar2_round_toward_zero(v: i32) -> i32 {
    (v.wrapping_add((v >> 31) & 3)) >> 2
}

// ===========================================================================================
// 5. `Object::find_nearby_target` `0x00648DA0` — the spiral search
// ===========================================================================================

/// Ring cap. `0x00648EF2`: `if (0x20 < rings) rings = 0x20`, and the `max_dist == 0` default
/// at `0x00648EBB` is the same 0x20.
pub const MAX_RINGS: i32 = 0x20;

/// The early-out. `0x0064986C`: after **more than ten** unit candidates have been *scored*,
/// the search stops as soon as it holds any best target.
///
/// This is the engine's own answer to quadratic acquisition cost, and it is not an
/// optimisation we may skip: it changes which target is chosen in a dense fight, so a port
/// that scans everything is both slower *and* wrong.
pub const UNIT_CANDIDATE_LIMIT: i32 = 10;

/// `0x006498D2`: after the scan, a best-distance worse than this clears the cached
/// `near_o`/`near_who` pair.
pub const NEAR_CACHE_MAX_DIST: i32 = 0xF00;

/// `0x00649BA6`: `targeted` saturates at 100 (`cmp cl, 0x64`).
pub const TARGETED_MAX: i8 = 100;

/// How many circle rings a search of `max_dist` world units covers — `0x00648EA8`.
///
/// ```text
/// if (max_dist == 0) return 0x20
/// rings = (max_dist + 0x2FF) / 0x300          ; ceil(max_dist / 768)
/// if (this->vf[0x20]())            rings++    ; attacker is a building
/// if (this->has_objmask(0x80000000)) rings++  ; anti-air reaches further
/// if (this->is_unit() && stance == 3) rings++
/// return min(rings, 0x20)
/// ```
pub fn ring_budget(
    max_dist: i32,
    a_is_building: bool,
    a_has_objmask_high: bool,
    a_stance_is_3: bool,
) -> i32 {
    if max_dist == 0 {
        return MAX_RINGS;
    }
    let mut rings = (max_dist.wrapping_add(0x2FF)) / 0x300;
    if a_is_building {
        rings += 1;
    }
    if a_has_objmask_high {
        rings += 1;
    }
    if a_stance_is_3 {
        rings += 1;
    }
    rings.min(MAX_RINGS)
}

/// `Unit::find_melee_target` `0x005FF9C0` — how the search radius is chosen when the caller
/// passes `-1` ("use my own respond range"), `0x005FFCE9..0x005FFDE1`.
///
/// ```text
/// mr = this->max_range()                        ; tiles, 0 for melee
/// if stance == 1 (defensive):
///     d = (mr == 0) ? 0x120 : mr * 0xC0
///     d = max(d, RULES.unit_defensive_respond_range * 0xC0)
/// else:
///     d = (mr == 0) ? 0x120 : (mr + 1) * 0xC0
///     if stance == 0: d += 0x180
///     d = max(d, RULES.unit_respond_range * 0xC0)
///     if (unit_masks & 0x40000): d = max(d, RULES.unit_respond_range * 0x180)
/// ```
///
/// `0x120` is 1.5 tiles, `0xC0` is [`TILE_UNITS`], `0x180` is two tiles. Note the `× 0x180`
/// floor: an object carrying `unit_masks 0x40000` searches to **twice** the rule's range.
pub fn respond_range(
    max_range_tiles: i32,
    stance: i32,
    unit_mask_0x40000: bool,
    unit_respond_range: i32,
    unit_defensive_respond_range: i32,
) -> i32 {
    if stance == 1 {
        let mut d = if max_range_tiles == 0 {
            0x120
        } else {
            max_range_tiles.wrapping_mul(TILE_UNITS)
        };
        d = d.max(unit_defensive_respond_range.wrapping_mul(TILE_UNITS));
        d
    } else {
        let mut d = if max_range_tiles == 0 {
            0x120
        } else {
            max_range_tiles.wrapping_add(1).wrapping_mul(TILE_UNITS)
        };
        if stance == 0 {
            d = d.wrapping_add(0x180);
        }
        d = d.max(unit_respond_range.wrapping_mul(TILE_UNITS));
        if unit_mask_0x40000 {
            d = d.max(unit_respond_range.wrapping_mul(0x180));
        }
        d
    }
}

/// The per-candidate weighting `find_nearby_target` applies **around** `compare_target` —
/// `0x00649690..0x006497E0`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CandidateWeights {
    /// `this->is_unit()`, gating the range reshaping at `0x006496A4`.
    pub a_is_unit: bool,
    /// `this->min_range() × 0xC0`. Non-zero reshapes near targets *upward*, i.e. penalises
    /// them: `d = min_range + (max_range - d)`.
    pub min_range_units: i32,
    /// `this->max_range() × 0xC0`. With no minimum range, a target more than two tiles
    /// inside maximum range gets `d /= 2` — a comfort bonus for staying inside the envelope.
    pub max_range_units: i32,
    /// `target[+0x3D] targeted`. Enters as `(targeted + 8) × 0x30` added to the distance.
    pub targeted: i32,
    /// `(o, who) == (this->last_seen_o, this->last_seen_who)` from `Unit::update_order`;
    /// halves the score at `0x006496FE`.
    pub is_last_order_target: bool,
    /// Filter bit `0x10` was requested but the candidate is not a unit — halve
    /// (`0x006497A7`).
    pub demote_non_unit: bool,
    /// Filter bit `0x20` was requested but the candidate is not a building — halve.
    pub demote_non_building: bool,
    /// `mode` (`param_4`). Enables the facing weighting below.
    pub mode: bool,
    /// `find_angle(target - this) - this->angle`, folded to `[0, 0x80000000]` by
    /// `if (a > 0x80000000) a = ~a` at `0x006497CA`. Only read when `mode`.
    pub facing_delta: u32,
}

/// What the facing weighting did to a candidate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FacingWeight {
    /// `delta < 0x15555555` (within 30°) — `score << 2`.
    Ahead4x,
    /// `delta < 0x2AAAAAAA` (within 60°) — `score × 2`.
    Ahead2x,
    /// `0x2AAAAAAA ..= 0x40000000` (60°–90°) — unchanged.
    Side,
    /// `0x40000001 ..= 0x60000000` (90°–135°) — `score / 10`.
    Behind,
    /// `> 0x60000000` (beyond 135°) — the candidate is **skipped entirely**.
    Rejected,
}

/// The facing classifier at `0x006497D4..0x00649802`, in `mode` only.
///
/// Note this is a *third* angular classifier, distinct from
/// `crate::mechanics::flank_level` and `crate::mechanics::entrench_dir_level`: it splits at
/// 30°/60°/90°/135° rather than 60°/135°, and it is about the **attacker's** facing, not the
/// defender's. Assuming one of the other two serves here would be an easy, invisible error.
#[inline]
pub fn facing_weight(delta: u32) -> FacingWeight {
    let d = if delta > 0x8000_0000 { !delta } else { delta };
    if d < 0x1555_5555 {
        FacingWeight::Ahead4x
    } else if d < 0x2AAA_AAAA {
        FacingWeight::Ahead2x
    } else if d > 0x6000_0000 {
        FacingWeight::Rejected
    } else if d > 0x4000_0000 {
        FacingWeight::Behind
    } else {
        FacingWeight::Side
    }
}

/// Turn a `compare_target` priority plus a raw distance into the value the search maximises
/// — `0x00649690..0x00649802`.
///
/// ```text
/// d = attack_dist
/// if (this is a unit) {
///     if (min_range != 0) { if (d < min_range) d = min_range + (max_range - d); }
///     else if (max_range != 0 && d + 0x180 < max_range) d /= 2;
/// }
/// penalty = d + (targeted + 8) * 0x30
/// score   = compare_target(...) / (penalty / 0xC0 + 1)
/// if (last order target)  score /= 2
/// if (demoted)            score /= 2
/// if (mode)               score = facing_weight(score)     ; or reject
/// if (score == 0 && priority != 0) score = 1
/// ```
///
/// `(targeted + 8) × 0x30` is the **target-spreading** term: each attacker already aimed at
/// this object adds 48 world units (a quarter tile) of virtual distance, so a mob spreads
/// across a line instead of stacking on one victim. It reads and writes a checksummed field.
pub fn rank_candidate(priority: i32, attack_dist: i32, w: &CandidateWeights) -> Option<i32> {
    let mut d = attack_dist;
    if w.a_is_unit {
        if w.min_range_units != 0 {
            if d < w.min_range_units {
                d = w
                    .min_range_units
                    .wrapping_add(w.max_range_units.wrapping_sub(d));
            }
        } else if w.max_range_units != 0 && d.wrapping_add(0x180) < w.max_range_units {
            d /= 2;
        }
    }
    let penalty = d.wrapping_add((w.targeted.wrapping_add(8)).wrapping_mul(0x30));
    let mut score = priority / (penalty / TILE_UNITS + 1);
    if w.is_last_order_target {
        score /= 2;
    }
    if w.demote_non_unit || w.demote_non_building {
        score /= 2;
    }
    if w.mode {
        match facing_weight(w.facing_delta) {
            FacingWeight::Ahead4x => score = score.wrapping_shl(2),
            FacingWeight::Ahead2x => score = score.wrapping_mul(2),
            FacingWeight::Side => {}
            FacingWeight::Behind => score /= 10,
            FacingWeight::Rejected => return None,
        }
    }
    if score == 0 && priority != 0 {
        score = 1;
    }
    Some(score)
}

/// One candidate the caller admitted, with everything the ranker needs.
#[derive(Clone, Copy, Debug)]
pub struct Candidate {
    /// Which object.
    pub who: ObjRef,
    /// `compare_target`'s return.
    pub priority: i32,
    /// `ObjectData::attack_dist`.
    pub attack_dist: i32,
    /// The rest of the weighting.
    pub weights: CandidateWeights,
    /// `target->vf[0x18]()` — counts toward [`UNIT_CANDIDATE_LIMIT`].
    pub is_unit: bool,
}

/// What one search returned.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AcquireResult {
    /// The chosen target, or [`ObjRef::NONE`].
    pub best: Option<ObjRef>,
    /// The winning score, `local_70`.
    pub best_score: i32,
    /// The closest *valid* candidate seen, whatever its score — retail caches this in
    /// `near_o`/`near_who` (`0x00649669`) and clears it past [`NEAR_CACHE_MAX_DIST`].
    pub nearest: Option<ObjRef>,
    /// Its distance, `local_4C`, initialised to `9999999`.
    pub nearest_dist: i32,
    /// How many *unit* candidates were scored. The search aborts once this exceeds
    /// [`UNIT_CANDIDATE_LIMIT`] and a best exists.
    pub units_scored: i32,
    /// Cells visited before the scan ended. Diagnostic; retail keeps no such counter.
    pub cells_visited: i32,
}

/// The whole spiral scan — `0x00649306..0x006498DE`.
///
/// `admit` is the caller's gate: it stands for `Object::valid_target` `0x00648BA0` plus
/// `Object::check_target` `0x00649E00` plus the filter-mask arms at `0x0064949F`, all of
/// which need the diplomacy table, the fog and the region map. Return `None` to reject a
/// candidate, or the fully-weighted [`Candidate`] to score it.
///
/// The iteration order is retail's exactly: the `circle_init` spiral (`combat::circle_table`,
/// `0x006817F0`) over cells, and within each cell the `down`/`down_who` chain from the
/// `WData` head. **This is a bucketed uniform grid, not a scan of all objects** — which is
/// both the faithful structure and the answer to the quadratic acquisition cost the web lane
/// measured, since the work is bounded by `ring_end[rings]` cells and
/// [`UNIT_CANDIDATE_LIMIT`] scored units rather than by the world's population.
pub fn find_nearby_target<F>(
    world: &mut TargetWorld,
    circle: &CircleTable,
    searcher: ObjRef,
    rings: i32,
    mut admit: F,
) -> AcquireResult
where
    F: FnMut(&TargetWorld, ObjRef, i32) -> Option<Candidate>,
{
    let mut r = AcquireResult {
        nearest_dist: 9_999_999,
        ..AcquireResult::default()
    };
    let Some(srow) = world.row(searcher) else {
        return r;
    };
    let (sx, sy) = (srow.x, srow.y);
    let base_cx = cell_of(sx);
    let base_cy = cell_of(sy);
    let rings = rings.clamp(0, MAX_RINGS);
    let limit = circle.ring_end[rings as usize];

    'scan: for slot in 0..limit as usize {
        let cx = base_cx + circle.x[slot] as i32;
        let cy = base_cy + circle.y[slot] as i32;
        let Some(cell) = world.cell_index(cx, cy) else {
            continue;
        };
        r.cells_visited += 1;
        for cand_ref in world.cell_chain(cell) {
            let Some(row) = world.row(cand_ref) else {
                continue;
            };
            // 0x00649408: the alive bit is tested inline before anything virtual.
            if !row.is_alive() {
                continue;
            }
            let dist = {
                let row = world.row(cand_ref).unwrap();
                scalar_attack_dist(sx, sy, row.x, row.y, 0)
            };
            let Some(c) = admit(world, cand_ref, dist) else {
                continue;
            };
            // 0x00649650: the nearest cache is updated for every admitted candidate,
            // regardless of score, and only when either end of the pair is a unit.
            if c.attack_dist < r.nearest_dist {
                r.nearest_dist = c.attack_dist;
                r.nearest = Some(cand_ref);
            }
            let Some(score) = rank_candidate(c.priority, c.attack_dist, &c.weights) else {
                continue;
            };
            if c.is_unit {
                r.units_scored += 1;
            }
            if score > r.best_score {
                r.best_score = score;
                r.best = Some(cand_ref);
            }
            // 0x0064986C: more than ten scored units and a best in hand ends the search.
            if r.units_scored > UNIT_CANDIDATE_LIMIT && r.best.is_some() {
                break 'scan;
            }
        }
    }

    // 0x006498D2 / 0x006498DE: commit or clear the cached nearest pair.
    let (near_o, near_who) = match r.nearest {
        Some(n) if r.nearest_dist <= NEAR_CACHE_MAX_DIST => (n.o, n.who),
        _ => {
            r.nearest = None;
            (NO_LINK, NO_LINK)
        }
    };
    if let Some(row) = world.row_mut(searcher) {
        row.near_o = near_o;
        row.near_who = near_who;
    }

    // 0x00649BA0: the chosen target's `targeted` counter increments, saturating at 100.
    if let Some(best) = r.best {
        if let Some(row) = world.row_mut(best) {
            if row.targeted < TARGETED_MAX {
                row.targeted += 1;
            }
        }
    }
    r
}

// ===========================================================================================
// 6. `Unit::think_attack` -> `find_new_target`: executable host contract
// ===========================================================================================

/// Is an idle unit's ordinary attack-think slice due this frame?
///
/// `Unit::think` `0x005F6E40` gates `think_attack` with
/// `(Game::frame + ObjectData::o) & 0x1F == 0` once the unit is in its normal idle-think
/// state (`0x005F6FC2..0x005F6FE8`).  The object slot, not owner, spawn frame, or an RNG
/// draw, phases the work.  Non-negative retail frames and object slots make this identical
/// to modulo 32; wrapping arithmetic states the machine operation for long-running hosts.
#[inline]
pub fn retarget_due(frame: i32, object_slot: i16) -> bool {
    frame.wrapping_add(object_slot as i32) & 0x1f == 0
}

/// Is the checksummed `ObjectData::targeted` crowding byte due to decay?
///
/// `Unit::process` `0x006114E8` and `Wall::process` `0x0064047A` use the same phased
/// `(frame + o) & 0x0F` gate.  On the due frame retail divides the signed byte by four,
/// truncating toward zero.
#[inline]
pub fn targeted_decay_due(frame: i32, object_slot: i16) -> bool {
    frame.wrapping_add(object_slot as i32) & 0x0f == 0
}

/// Apply the retail target-crowding decay for one on-map object process slice.
pub fn decay_targeted(world: &mut TargetWorld, frame: i32, object: ObjRef) -> bool {
    if !targeted_decay_due(frame, object.o) {
        return false;
    }
    let Some(row) = world.row_mut(object) else {
        return false;
    };
    row.targeted /= 4;
    true
}

/// The fixed and dynamic unit fields read while `find_new_target` chooses its response
/// radius and ranks candidates.
///
/// This represents the ordinary `find_new_target(out_who, 0)` path: it calls
/// `find_melee_target(-1, out_who, 0, 1, 0)`, which means an automatic response range,
/// order creation enabled, no special filter mask, and no facing-cone rejection.  The
/// caller creates its own attack job from the returned target; this module owns selection,
/// not an arena's order representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoTargetQuery {
    pub searcher: ObjRef,
    /// `GameAccess::game + 0x550`.
    pub frame: i32,
    /// `this->max_range()` in tiles.
    pub max_range_tiles: i32,
    /// `this->min_range()` in tiles.
    pub min_range_tiles: i32,
    /// `UnitData +0xB1`, normally 0 aggressive, 1 defensive, 2 default, 3 hold-ground.
    pub stance: i32,
    /// `UnitData +0x68`.
    pub unit_masks: u32,
    /// `this->has_objmask(0x80000000)`, the anti-air reach bonus in the cell budget.
    pub has_objmask_high: bool,
    /// Shipped `Rules::unit_respond_range`.
    pub unit_respond_range: i32,
    /// Shipped `Rules::unit_defensive_respond_range`.
    pub unit_defensive_respond_range: i32,
    /// `find_nearby_target`'s `local_40`: false for AI-flagged leaders, otherwise enabled
    /// only when the two retail global suppressors are clear.  This selects
    /// `compare_target`'s alternate valuation mode; it is distinct from facing mode.
    pub compare_mode: bool,
    /// The current targeted order's `(o, who)` when one exists.  Ordinary idle acquisition
    /// supplies `None`; retaining this input also covers the same retail search reached
    /// while an order is being reconsidered.
    pub last_order_target: Option<ObjRef>,
}

/// Candidate facts the engine obtains through object/type virtuals and path/region state.
///
/// There are deliberately no defaults.  A host must answer both remaining gate groups and
/// supply every [`CompareTargetInput`] field instead of getting a permissive zero-filled
/// nearest-enemy substitute by accident.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoTargetCandidate {
    /// All `ObjectData::valid_target_const` `0x006472C0` gates *other than* alive,
    /// diplomacy and `is_seen`, which [`AutoTargetAdapter`] and the spatial walker enforce:
    /// on-map/domain compatibility, targetability masks, and land/sea/air restrictions.
    pub valid_target_const: bool,
    /// `Object::check_target` `0x00649E00`: region/path reachability, territory admission,
    /// and `poor_target`.  The distance ceiling is enforced by [`find_auto_target`].
    pub check_target: bool,
    /// The path-check flag that `check_target` returns to `compare_target` after its
    /// in-range/path arms (`local_64` at `0x00649612`).
    pub check_path: bool,
    /// Mandatory resolved distance facts for `ObjectData::attack_dist` `0x006488F0`.
    ///
    /// Both footprints are required even though the attacker is invariant across this
    /// search. A rectangular building must retain both axes. [`AttackDistanceMode`] must be
    /// derived from the attacker vtable/type gate (for `Unit`, use
    /// [`AttackDistanceMode::for_unit_type`]); it has no default. Return `None` from the
    /// adapter when any fact is unavailable.
    pub distance_facts: AutoTargetDistanceFacts,
    /// The virtual/type/dynamic fields used by the exact priority arithmetic.
    pub compare: CompareTargetInput,
}

/// Coordinate-independent facts read by `ObjectData::attack_dist` for one candidate pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoTargetDistanceFacts {
    pub attacker: ObjectFootprint,
    pub target: ObjectFootprint,
    pub mode: AttackDistanceMode,
}

/// Read-only object-side contract for ordinary automatic target acquisition.
///
/// `is_seen` is intentionally a mandatory gate.  Retail dispatches the candidate's
/// `UnitData::is_seen` / `BuildData::is_seen` virtual from
/// `ObjectData::valid_target_const`; a host may reproduce remembered-object bits as well as
/// current fog, but it may not silently expose every live enemy.  The candidate callback is
/// reached only after alive, hostility, and visibility pass.
pub trait AutoTargetAdapter {
    /// `LeaderData::is_enemy` `0x006EBAA0`, after effective-owner resolution if the host
    /// supports shared control.
    fn is_enemy(&self, observer_who: i16, candidate_who: i16) -> bool;

    /// Candidate virtual `is_seen(observer_who, 0)`.
    fn is_seen(&self, observer_who: i16, candidate: ObjRef) -> bool;

    /// Resolve the remaining candidate gates and priority inputs.
    fn candidate(
        &self,
        searcher: ObjRef,
        candidate: ObjRef,
        searcher_row: TargetRow,
        candidate_row: TargetRow,
    ) -> Option<AutoTargetCandidate>;
}

/// One ordinary automatic-acquisition slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoTargetStep {
    /// The unit's phased 32-frame think slice is not due.
    Deferred,
    /// A retail search ran, whether or not it found a target.
    Searched {
        /// Exact response ceiling in world units.
        max_dist: i32,
        /// Exact number of 768-unit spiral rings visited at most.
        rings: i32,
        result: AcquireResult,
    },
}

/// Execute ordinary idle-unit target acquisition without a global or nearest-hostile scan.
///
/// The order is retail's: cadence gate; `find_melee_target` response radius; spiral cells;
/// each cell's intrusive object chain; alive/diplomacy/visibility/target-validity/check
/// gates; edge distance ceiling; `compare_target`; distance, target-crowding and prior-order
/// weighting; first strictly-greater score wins; stop after the eleventh scored unit.
///
/// This function accepts no [`crate::rng::Random`].  Capstone and decompilation show no RNG
/// call anywhere in `Unit::think_attack`, `find_new_target`, `find_melee_target`,
/// `Object::find_nearby_target`, `valid_target`, `check_target`, or `compare_target`; chain
/// order is the deterministic tie-breaker.
pub fn find_auto_target<A: AutoTargetAdapter>(
    world: &mut TargetWorld,
    circle: &CircleTable,
    query: AutoTargetQuery,
    adapter: &A,
) -> AutoTargetStep {
    if !retarget_due(query.frame, query.searcher.o) {
        return AutoTargetStep::Deferred;
    }
    let max_dist = respond_range(
        query.max_range_tiles,
        query.stance,
        query.unit_masks & 0x40000 != 0,
        query.unit_respond_range,
        query.unit_defensive_respond_range,
    );
    let rings = ring_budget(max_dist, false, query.has_objmask_high, query.stance == 3);
    let searcher = query.searcher;
    let observer_who = searcher.who;
    let result = find_nearby_target(world, circle, searcher, rings, |w, target, _| {
        if !adapter.is_enemy(observer_who, target.who) || !adapter.is_seen(observer_who, target) {
            return None;
        }
        let searcher_row = *w.row(searcher)?;
        let target_row = *w.row(target)?;
        let facts = adapter.candidate(searcher, target, searcher_row, target_row)?;
        if !facts.valid_target_const || !facts.check_target {
            return None;
        }
        let dist = attack_distance(AttackDistanceInput {
            attacker_x: searcher_row.x,
            attacker_y: searcher_row.y,
            target_x: target_row.x,
            target_y: target_row.y,
            attacker: facts.distance_facts.attacker,
            target: facts.distance_facts.target,
            mode: facts.distance_facts.mode,
        });
        if dist > max_dist {
            return None;
        }

        let mut compare = facts.compare;
        // These values come from the indexed rows/query and cannot legitimately disagree
        // with an adapter's virtual/type expansion.
        compare.check_path = facts.check_path;
        compare.mode = query.compare_mode;
        compare.a_is_unit = true;
        compare.a_ref = searcher;
        compare.a_stance_is_3 = query.stance == 3;
        compare.a_unit_mask_0x40000 = query.unit_masks & 0x40000 != 0;
        compare.a_has_objmask_high = query.has_objmask_high;
        compare.t_ref = target;
        compare.t_alive = target_row.is_alive();
        compare.t_is_building = target_row.is_building;
        compare.t_is_wonder = target_row.is_wonder;
        compare.t_is_city_centre = target_row.is_city_centre();
        compare.t_is_damaged = target_row.damage != 0;
        compare.t_full = target_row.full as i32;

        Some(Candidate {
            who: target,
            priority: compare_target(&compare),
            attack_dist: dist,
            weights: CandidateWeights {
                a_is_unit: true,
                min_range_units: query.min_range_tiles.wrapping_mul(TILE_UNITS),
                max_range_units: query.max_range_tiles.wrapping_mul(TILE_UNITS),
                targeted: target_row.targeted as i32,
                is_last_order_target: query.last_order_target == Some(target),
                // `find_new_target`'s filter is zero and its find-nearby facing-mode
                // parameter is zero on this path.
                demote_non_unit: false,
                demote_non_building: false,
                mode: false,
                facing_delta: 0,
            },
            is_unit: target_row.is_unit,
        })
    });
    AutoTargetStep::Searched {
        max_dist,
        rings,
        result,
    }
}

// ===========================================================================================
// 7. `attack_dir`, settled
// ===========================================================================================

/// `attack_dir` — the third argument of `Object::do_damage` `0x0064A480`, and the value the
/// flank and entrenchment steps difference against the defender's facing.
///
/// **It is the direction the attack travels, from attacker to target.** Traced in capstone
/// at `0x005FE872..0x005FE89B` inside `Unit::fight` [measured]:
///
/// ```text
/// 005fe875  mov edx, [esi+0x14]; xor edx, 0x63637   ; target.y
/// 005fe875  mov eax, [ebx+0x14]; xor eax, 0x63637   ; attacker.y
/// 005fe889  sub edx, eax                            ; dy = target.y - attacker.y
/// 005fe880  mov ecx, [esi+0x10]; xor ecx, 0x63637   ; target.x
/// 005fe891  mov eax, [ebx+0x10]; xor eax, 0x63637   ; attacker.x
/// 005fe899  sub ecx, eax                            ; dx = target.x - attacker.x
/// 005fe89b  call find_angle                         ; __fastcall(ecx=dx, edx=dy)
/// 005fe8a4  mov [ebp+0x14], eax                     ; -> attack_dir
/// 005fec4a  mov esi, [ebp+0x14]                     ; ... reloaded
/// 005fedd5  push esi                                ; ... as do_damage's 3rd argument
/// ```
///
/// The same value is written to every guy's `+0x64` at `0x005FEC6A`.
///
/// This settles the open question in `combat::flank_delta`'s doc comment and explains the
/// web lane's measurement. With `defender_facing = 0`:
///
/// * `attack_dir = 0` means the attack travels *the way the defender is looking*, i.e. the
///   attacker is **behind** it. `flank_delta` is `0x80000000`, tier **1**.
/// * `attack_dir = half turn` means the attack comes head-on. `flank_delta` is `0`, which
///   fails the caller's `>= 0x2AAAAAAA` pre-guard at `0x00644B1D`, so **no** flank bonus.
///
/// Nothing is inverted; the name is just about the projectile, not about the shooter's
/// bearing. Substituting `bearing = attack_dir - half turn` gives
/// `flank_delta == defender_facing - bearing_to_attacker`, and the arcs become plain:
///
/// | attacker's bearing from the defender's nose | width | tier | infantry bonus |
/// |---|---|---|---|
/// | within ±60° of dead ahead | 120° | none | +0 % |
/// | 60°–135° off, either side | 2 × 75° | **2** | +100 % |
/// | within ±45° of dead astern | 90° | **1** | +50 % |
///
/// So the **sides** are worth twice the **rear**, and the front is worth nothing. That is
/// what "flank" means, and it is not what a reading of `attack_dir` as "bearing to the
/// attacker" would have produced — that reading makes a frontal charge the flanking bonus.
#[inline]
pub fn attack_dir(ax: i32, ay: i32, tx: i32, ty: i32) -> i32 {
    find_angle(tx.wrapping_sub(ax), ty.wrapping_sub(ay))
}

/// The flank tier a defender suffers given where its attacker stands.
///
/// Composes [`attack_dir`] with `combat::flank_delta` and `crate::mechanics::flank_level`,
/// **including the caller's pre-guard** at `0x00644B1D` that `flank_level` alone does not
/// apply: a delta below `0x2AAAAAAA` skips the flank step entirely and is reported here as
/// tier 0. Use this rather than calling `flank_level` directly, or a head-on hit scores
/// tier 2.
#[inline]
pub fn flank_tier(defender_facing: i32, attack_dir: i32) -> u32 {
    let delta = (defender_facing as u32)
        .wrapping_sub(attack_dir as u32)
        .wrapping_sub(0x8000_0000);
    if delta < 0x2AAA_AAAA {
        0
    } else {
        flank_level(delta)
    }
}

// ===========================================================================================
// 7. The engagement loop — acquire, close, hit
// ===========================================================================================

/// What one frame of engagement did. This is the observable the lane exists to produce: a
/// unit that picks a target, walks to it, and lands a hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngageStep {
    /// No target within the respond range this frame.
    NoTarget,
    /// A target is held but is out of weapon range; `remaining` world units to close.
    Closing { target: ObjRef, remaining: i32 },
    /// In range, weapon still reloading.
    Recharging { target: ObjRef, frames_left: u8 },
    /// A hit landed. `damage` is `crate::mechanics::damage`'s return, `flank` the tier the
    /// defender suffered.
    Hit {
        target: ObjRef,
        damage: i32,
        flank: u32,
    },
}

/// A minimal unit-side engagement driver: the connective tissue between acquisition, the
/// approach, and `ObjectData::get_damage`.
///
/// It is **not** `Unit::fight` `0x005FD4D0` (8,157 B) — it is the spine of what `Unit::fight`
/// does with the pieces this repository actually has, so that combat can run. What it
/// reproduces: the range test (`combat::in_attack_range`), the recharge byte
/// (`combat::AttackCycle`, `UnitData +0xAE`, wrapping at 255 exactly as retail's byte store
/// does), `attack_dir` from [`attack_dir`], and the flank tier from [`flank_tier`]. What it
/// does **not** reproduce: order-list interaction, guy-level firing, projectile spawning,
/// `Object::do_damage`'s splash and retaliation, and every one of `Unit::fight`'s other
/// branches.
#[derive(Clone, Debug)]
pub struct Engagement {
    /// Which unit is fighting.
    pub unit: ObjRef,
    /// Its current target, if any.
    pub target: Option<ObjRef>,
    /// `UnitData +0xAE recharging`.
    pub cycle: crate::systems::combat::AttackCycle,
    /// `this->max_range()` in tiles.
    pub max_range_tiles: i32,
    /// Movement per frame in world units, used only to close the gap.
    pub speed: i32,
    /// `UnitData +0x50 angle` of this unit — updated to face the target when closing.
    pub facing: i32,
}

impl Engagement {
    /// A fresh engagement for `unit`.
    pub fn new(unit: ObjRef, max_range_tiles: i32, speed: i32) -> Self {
        Engagement {
            unit,
            target: None,
            cycle: crate::systems::combat::AttackCycle::default(),
            max_range_tiles,
            speed,
            facing: 0,
        }
    }

    /// One frame. `hit` is called when a blow lands and must return `(damage, recharge)`;
    /// wire it to `crate::mechanics::damage` and `combat::recharge_frames`.
    pub fn step<F>(
        &mut self,
        world: &mut TargetWorld,
        circle: &CircleTable,
        rings: i32,
        admit: impl FnMut(&TargetWorld, ObjRef, i32) -> Option<Candidate>,
        mut hit: F,
    ) -> EngageStep
    where
        F: FnMut(i32, u32) -> (i32, i32),
    {
        self.cycle.tick();

        // Re-acquire whenever the held target is gone or dead.
        let held_alive = self
            .target
            .and_then(|t| world.row(t))
            .map(|r| r.is_alive())
            .unwrap_or(false);
        if !held_alive {
            self.target = find_nearby_target(world, circle, self.unit, rings, admit).best;
        }
        let Some(target) = self.target else {
            return EngageStep::NoTarget;
        };

        let (ux, uy) = {
            let u = world.row(self.unit).expect("unit row");
            (u.x, u.y)
        };
        let (tx, ty) = {
            let t = world.row(target).expect("target row");
            (t.x, t.y)
        };
        let dist = scalar_attack_dist(ux, uy, tx, ty, 0);
        let dir = attack_dir(ux, uy, tx, ty);
        self.facing = dir;

        if !in_attack_range(dist, self.max_range_tiles) {
            let step = self.speed.min(dist);
            if step > 0 {
                // Close along the straight line; movement fidelity is the movement lane's.
                let dx = tx.wrapping_sub(ux);
                let dy = ty.wrapping_sub(uy);
                let len = vector_dist(dx, dy).max(1);
                let u = world.row_mut(self.unit).expect("unit row");
                u.x = u.x.wrapping_add(dx.wrapping_mul(step) / len);
                u.y = u.y.wrapping_add(dy.wrapping_mul(step) / len);
            }
            let remaining = (dist - self.max_range_tiles.wrapping_mul(TILE_UNITS)).max(0);
            return EngageStep::Closing { target, remaining };
        }

        if !self.cycle.ready() {
            return EngageStep::Recharging {
                target,
                frames_left: self.cycle.recharging,
            };
        }

        let defender_facing = 0; // callers with a real facing should pass it through `hit`
        let flank = flank_tier(defender_facing, dir);
        let (damage, recharge) = hit(dir, flank);
        self.cycle.fire(recharge);
        if let Some(row) = world.row_mut(target) {
            row.damage = row.damage.wrapping_add(damage);
            if row.damage >= 1 {
                // `Object::take_damage`'s life/death decision lives in `combat`; this only
                // records the accumulation so a caller can observe the hit landing.
            }
        }
        EngageStep::Hit {
            target,
            damage,
            flank,
        }
    }
}

/// Build the circle spiral once. Thin alias so callers of this module do not need to reach
/// into `combat` for it.
pub fn spiral() -> CircleTable {
    circle_table()
}

// ===========================================================================================
// 8. The rules bridge: `combat::CombatConstants` -> `mechanics::CombatRules`
// ===========================================================================================

/// Map the combat lane's `Constants` block onto the damage chain's rules struct.
///
/// Both structs are correct and neither could be fed to the other, which is one reason
/// combat could not run: `combat::CombatConstants` holds the shipped values with their
/// `Constants` offsets and PDB names, `mechanics::CombatRules` holds exactly the subset
/// `ObjectData::get_damage` reads. Every field below is matched **by `Constants` offset**,
/// not by name similarity.
///
/// Three of `CombatRules`'s fields are documented there as *"name not established"*. The
/// offsets settle them from `combat::CombatConstants`, which carries the PDB names
/// [measured]:
///
/// | `CombatRules` | offset | PDB name | shipped |
/// |---|---|---|---|
/// | `rule_0x558` | `+0x0558` | `SUPER_IMMUNE` | 0 |
/// | `rule_0x76c` | `+0x076C` | `RUSSIAN_COSSACK_DAMAGE` | 25 |
/// | `rule_0xb98` | `+0x0B98` | `ANTIPATER_ENTRENCH_BONUS` | 204 |
///
/// and likewise for [`unreached_terms`]: `rule_0xbbc` is `+0x0BBC WELLINGTON_SIEGE_ATTACK`
/// (1) and `rule_0x794` is `+0x0794 JAPANESE_DAMAGE` (-5, "negative is per age").
pub fn combat_rules(c: &crate::systems::combat::CombatConstants) -> crate::mechanics::CombatRules {
    crate::mechanics::CombatRules {
        height_increment: c.height_increment,
        height_bonus: c.height_bonus,
        flank_bonus: c.flank_bonus,
        cavalry_flank_bonus: c.cavalry_flank_bonus,
        vehicle_flank_bonus: c.vehicle_flank_bonus,
        rocky_modifier: c.rocky_modifier,
        overkill_frames: c.overkill_frames,
        overkill_damage: c.overkill_damage,
        entrenchment_modifier: c.entrenchment_modifier,
        river_modifier: c.river_modifier,
        recapture_city_modifier: c.recapture_city_modifier,
        red_fort_air_defense: c.red_fort_air_defense,
        rule_0x558: c.super_immune,
        rule_0x76c: c.russian_cossack_damage,
        rule_0xb98: c.antipater_entrench_bonus,
    }
}

/// The two rule values `mechanics::UnreachedTerms` carries, from the same block. See
/// [`combat_rules`] for the offset table. `step11_player_level` is per-player state, not a
/// rule, so the caller supplies it.
pub fn unreached_terms(
    c: &crate::systems::combat::CombatConstants,
    step11_player_level: i32,
) -> crate::mechanics::UnreachedTerms {
    crate::mechanics::UnreachedTerms {
        rule_0xbbc: c.wellington_siege_attack,
        rule_0x794: c.japanese_damage,
        step11_player_level,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::combat::flank_delta;

    // -------- coordinate spaces --------

    #[test]
    fn cells_are_four_tiles() {
        assert_eq!(CELL_UNITS, 4 * TILE_UNITS);
        assert_eq!(cell_of(0), 0);
        assert_eq!(cell_of(767), 0);
        assert_eq!(cell_of(768), 1);
        assert_eq!(cell_of(1535), 1);
        assert_eq!(tile_of(191), 0);
        assert_eq!(tile_of(192), 1);
    }

    #[test]
    fn div3_matches_the_retail_table_over_its_domain() {
        // init_coord_lookup_array 0x00681DB0 fills t[k] = k/3 with C truncation, both
        // directions from zero.
        for k in -3000..3000 {
            assert_eq!(div_3(k), k / 3);
        }
        assert_eq!(div_3(-1), 0, "truncation toward zero, not floor");
    }

    #[test]
    fn coord_mask_is_its_own_inverse() {
        for c in [0, 1, 0x1234, -5, 0x0006_3637] {
            assert_eq!(unmask_coord(unmask_coord(c)), c);
        }
    }

    // -------- the grid --------

    fn unit_row(x: i32, y: i32) -> TargetRow {
        TargetRow {
            flags8: 1,
            x,
            y,
            is_unit: true,
            ..TargetRow::default()
        }
    }

    #[test]
    fn grid_links_and_walks_a_cell_chain() {
        let mut w = TargetWorld::new(8, 8);
        let a = w.push(0, unit_row(100, 100));
        let b = w.push(0, unit_row(200, 200));
        let far = w.push(1, unit_row(100 + CELL_UNITS, 100));
        w.link(a);
        w.link(b);
        w.link(far);

        let c0 = w.cell_index(0, 0).unwrap();
        let chain = w.cell_chain(c0);
        assert_eq!(chain.len(), 2, "both near objects share cell (0,0)");
        assert!(chain.contains(&a) && chain.contains(&b));
        assert!(!chain.contains(&far));

        let c1 = w.cell_index(1, 0).unwrap();
        assert_eq!(w.cell_chain(c1), vec![far]);
    }

    #[test]
    fn out_of_bounds_cells_are_refused() {
        let w = TargetWorld::new(4, 4);
        assert!(w.cell_index(-1, 0).is_none());
        assert!(w.cell_index(0, -1).is_none());
        assert!(w.cell_index(4, 0).is_none());
        assert!(w.cell_index(0, 4).is_none());
        assert_eq!(w.cell_index(3, 3), Some(15));
    }

    #[test]
    fn place_at_preserves_retail_addresses_and_head_order() {
        let mut w = TargetWorld::new(4, 4);
        let older = ObjRef::new(7, 1);
        let newer = ObjRef::new(3, 2);
        assert!(w.place_at(older, unit_row(100, 100)));
        assert!(w.place_at(newer, unit_row(200, 200)));
        assert_eq!(
            w.cell_chain(w.cell_index(0, 0).unwrap()),
            vec![newer, older],
            "Object::add_to_world inserts the newest object at the WData head"
        );
        assert!(
            w.row(ObjRef::new(6, 1)).is_some(),
            "sparse slots are tombstones"
        );
        assert!(!w.row(ObjRef::new(6, 1)).unwrap().is_alive());
        assert!(
            !w.place_at(older, unit_row(300, 300)),
            "live slots cannot be replaced"
        );
    }

    #[test]
    fn relocate_preserves_order_inside_a_cell_and_heads_a_new_cell() {
        let mut w = TargetWorld::new(4, 4);
        let a = ObjRef::new(0, 0);
        let b = ObjRef::new(1, 0);
        assert!(w.place_at(a, unit_row(100, 100)));
        assert!(w.place_at(b, unit_row(200, 200)));
        let c0 = w.cell_index(0, 0).unwrap();
        assert_eq!(w.cell_chain(c0), vec![b, a]);
        assert!(w.relocate(a, 300, 300));
        assert_eq!(
            w.cell_chain(c0),
            vec![b, a],
            "same-WData moves do not relink"
        );
        assert!(w.relocate(a, CELL_UNITS + 20, 100));
        assert_eq!(w.cell_chain(c0), vec![b]);
        assert_eq!(w.cell_chain(w.cell_index(1, 0).unwrap()), vec![a]);
        assert!(w.remove(b));
        assert!(w.cell_chain(c0).is_empty());
        assert!(!w.row(b).unwrap().is_alive());
    }

    // -------- ring budget and respond range --------

    #[test]
    fn ring_budget_matches_0x00648ea8() {
        assert_eq!(ring_budget(0, false, false, false), MAX_RINGS);
        assert_eq!(ring_budget(1, false, false, false), 1);
        assert_eq!(ring_budget(768, false, false, false), 1);
        assert_eq!(ring_budget(769, false, false, false), 2);
        assert_eq!(ring_budget(768, true, true, true), 4, "three bonus rings");
        assert_eq!(ring_budget(1 << 20, false, false, false), MAX_RINGS);
    }

    #[test]
    fn respond_range_matches_0x005ffce9() {
        // Melee, defensive: 1.5 tiles, floored by the defensive rule (shipped 4 tiles).
        assert_eq!(respond_range(0, 1, false, 6, 4), 4 * 192);
        // Melee, aggressive stance 0: 1.5 tiles + 2 tiles, floored by unit_respond_range.
        assert_eq!(respond_range(0, 0, false, 6, 4), 6 * 192);
        // Ranged, stance 2: (mr + 1) tiles.
        assert_eq!(respond_range(10, 2, false, 6, 4), 11 * 192);
        // The 0x40000 mask doubles the rule floor.
        assert_eq!(respond_range(0, 2, true, 6, 4), 6 * 0x180);
    }

    #[test]
    fn target_work_is_phased_by_object_slot_without_rng() {
        assert!(retarget_due(25, 7));
        assert!(!retarget_due(24, 7));
        assert!(retarget_due(57, 7));
        assert!(targeted_decay_due(9, 7));
        assert!(!targeted_decay_due(10, 7));

        let mut w = TargetWorld::new(2, 2);
        let target = ObjRef::new(7, 1);
        let mut row = unit_row(100, 100);
        row.targeted = 15;
        assert!(w.place_at(target, row));
        assert!(decay_targeted(&mut w, 9, target));
        assert_eq!(
            w.row(target).unwrap().targeted,
            3,
            "signed byte division truncates"
        );
        assert!(!decay_targeted(&mut w, 10, target));
        assert_eq!(w.row(target).unwrap().targeted, 3);
    }

    // -------- rank_candidate --------

    fn plain_weights() -> CandidateWeights {
        CandidateWeights {
            a_is_unit: true,
            ..CandidateWeights::default()
        }
    }

    #[test]
    fn distance_divides_the_priority_in_tiles() {
        let w = plain_weights();
        // penalty = 0 + (0+8)*0x30 = 384 -> 384/192 = 2 -> divisor 3.
        assert_eq!(rank_candidate(300, 0, &w), Some(100));
        // One tile further: penalty 192+384 = 576 -> /192 = 3 -> divisor 4.
        assert_eq!(rank_candidate(300, 192, &w), Some(75));
    }

    #[test]
    fn targeted_spreads_attackers() {
        let mut w = plain_weights();
        let clean = rank_candidate(10_000, 0, &w).unwrap();
        w.targeted = 8;
        let crowded = rank_candidate(10_000, 0, &w).unwrap();
        assert!(
            crowded < clean,
            "each existing attacker must make a target less attractive: {crowded} !< {clean}"
        );
        // (8+8)*0x30 = 768 -> 4 tiles -> divisor 5, versus divisor 3.
        assert_eq!(clean, 10_000 / 3);
        assert_eq!(crowded, 10_000 / 5);
    }

    #[test]
    fn a_nonzero_priority_never_ranks_zero() {
        let w = plain_weights();
        assert_eq!(rank_candidate(1, 100_000, &w), Some(1));
        assert_eq!(rank_candidate(0, 100_000, &w), Some(0));
    }

    #[test]
    fn facing_weight_boundaries_match_0x006497d4() {
        assert_eq!(facing_weight(0), FacingWeight::Ahead4x);
        assert_eq!(facing_weight(0x1555_5554), FacingWeight::Ahead4x);
        assert_eq!(facing_weight(0x1555_5555), FacingWeight::Ahead2x);
        assert_eq!(facing_weight(0x2AAA_AAA9), FacingWeight::Ahead2x);
        assert_eq!(facing_weight(0x2AAA_AAAA), FacingWeight::Side);
        assert_eq!(facing_weight(0x4000_0000), FacingWeight::Side);
        assert_eq!(facing_weight(0x4000_0001), FacingWeight::Behind);
        assert_eq!(facing_weight(0x6000_0000), FacingWeight::Behind);
        assert_eq!(facing_weight(0x6000_0001), FacingWeight::Rejected);
        // The fold: 0xFFFFFFFF is one step the other way, so it must read as dead ahead.
        assert_eq!(facing_weight(0xFFFF_FFFF), FacingWeight::Ahead4x);
    }

    #[test]
    fn a_target_behind_the_attacker_is_rejected_in_mode() {
        let w = CandidateWeights {
            a_is_unit: true,
            mode: true,
            facing_delta: 0x7000_0000,
            ..CandidateWeights::default()
        };
        assert_eq!(rank_candidate(1000, 0, &w), None);
    }

    // -------- compare_target --------

    fn base_cmp() -> CompareTargetInput {
        CompareTargetInput {
            t_type_value: 100,
            estimated_damage: 1,
            t_ref: ObjRef::new(0, 0),
            ..CompareTargetInput::default()
        }
    }

    /// The quiet path: a building target with the attacker in stance 3, which switches off
    /// every tail addition, so the only thing left is `base = type_value << 2` and the
    /// clamps. Used to exercise the tail arithmetic on its own.
    fn quiet(type_value: i32) -> CompareTargetInput {
        CompareTargetInput {
            t_type_value: type_value,
            estimated_damage: 1,
            t_is_building: true,
            a_stance_is_3: true,
            ..CompareTargetInput::default()
        }
    }

    #[test]
    fn compare_target_floors_at_fifteen() {
        assert_eq!(compare_target(&quiet(0)), 15);
        assert_eq!(
            compare_target(&quiet(100)),
            15,
            "400/100 = 4, floored to 15"
        );
    }

    #[test]
    fn compare_target_knee_is_continuous_at_100000() {
        // out = (type_value * 4 + 99) / 100, then the knee.
        let below = compare_target(&quiet(2_500_000));
        assert_eq!(below, 100_000, "just under the knee, untouched");
        let above = compare_target(&quiet(2_500_025));
        assert_eq!(above, 100_001, "(v - 99996)/5 + 100000 is continuous here");
        // Above the knee the score grows at one fifth the rate.
        let further = compare_target(&quiet(2_500_025 + 12_500));
        assert_eq!(
            further - above,
            100,
            "a pre-knee delta of 500 becomes 100 after the knee"
        );
    }

    #[test]
    fn the_unit_tail_adds_a_flat_hundred_thousand() {
        // Every non-building target that is not `type[+0x2C8] & 0x10000` picks up +100000
        // at 0x0064EDA0 before the clamps. This is what makes units and buildings score on
        // different scales, and it is why the floor test above uses the building path.
        let i = base_cmp();
        assert_eq!(compare_target(&i), (100 * 4 + 100_000 + 99) / 100);
    }

    #[test]
    fn idle_tech_0x3a_spellcaster_gets_the_retail_six_million_bonus() {
        let plain = base_cmp();
        let mut spellcaster = plain;
        spellcaster.t_is_spellcaster = true;
        spellcaster.t_is_tech_0x3a = true;

        assert_eq!(compare_target(&plain), 1_004);
        assert_eq!(compare_target(&spellcaster), 61_004);
    }

    #[test]
    fn casting_spellcaster_multiplies_and_prioritises_the_attacker_it_targets() {
        let attacker = ObjRef::new(12, 3);
        let mut casting = base_cmp();
        casting.a_ref = attacker;
        casting.t_is_spellcaster = true;
        casting.t_action_is_cast_spell = true;
        casting.t_action_target = Some(ObjRef::new(99, 3));

        // 400 * 20 + the ordinary unit-tail 100,000, then /100.
        assert_eq!(compare_target(&casting), 1_080);

        casting.t_action_target = Some(attacker);
        // The +10,000,000 pushes the compressed result through the 100,000 knee.
        assert_eq!(compare_target(&casting), 100_216);
    }

    #[test]
    fn spell_fields_do_nothing_without_the_retail_spellcaster_discriminator() {
        let mut i = base_cmp();
        let plain = compare_target(&i);
        i.t_is_tech_0x3a = true;
        i.t_action_is_cast_spell = true;
        i.t_action_target = Some(i.a_ref);
        assert_eq!(compare_target(&i), plain);
    }

    #[test]
    fn an_air_target_is_priority_two_without_anti_air() {
        let mut i = base_cmp();
        i.t_domain_is_air = true;
        assert_eq!(compare_target(&i), 2);
        i.a_has_objmask_high = true;
        assert!(compare_target(&i) > 2, "anti-air lifts the demotion");
    }

    #[test]
    fn an_undetected_stealth_unit_is_priority_one() {
        let mut i = base_cmp();
        i.a_is_unit = true;
        i.t_stealth_undetected = true;
        assert_eq!(compare_target(&i), 1);
    }

    #[test]
    fn wonders_are_worth_a_twenty_fifth() {
        let mut plain = base_cmp();
        plain.t_type_value = 100_000; // clear of the floor at both ends
        plain.t_alive = true;
        plain.t_is_building = true;
        let mut wonder = plain;
        wonder.t_is_wonder = true;
        let (w, p) = (compare_target(&wonder), compare_target(&plain));
        assert!(w < p, "wonder {w} should score below plain building {p}");
        assert_eq!(p / w, 25, "the divisor at 0x0064E70A is 25");
    }

    #[test]
    fn building_class_multipliers_are_ordered_as_retail_writes_them() {
        let mut b = base_cmp();
        b.t_alive = true;
        b.t_is_building = true;
        b.t_type_value = 1000;
        let score = |f: fn(&mut CompareTargetInput)| {
            let mut c = b;
            f(&mut c);
            compare_target(&c)
        };
        let plain = score(|_| {});
        let training = score(|c| c.t_is_training_building = true);
        let trainer = score(|c| c.t_is_military_trainer = true);
        let wall = score(|c| c.t_is_defensive_wall = true);
        let city = score(|c| c.t_is_city_centre = true);
        assert!(plain < training, "training building is x2");
        assert!(training < trainer, "military trainer is x3");
        assert!(trainer < wall, "defensive wall is x4");
        assert!(wall < city, "city centre is x5");
    }

    #[test]
    fn a_wounded_target_outranks_a_healthy_one() {
        let mut i = base_cmp();
        i.t_type_has_hits = true;
        i.t_attack = 10;
        i.t_hits_left = 100;
        let healthy = compare_target(&i);
        i.t_hits_left = 10;
        let wounded = compare_target(&i);
        assert!(
            wounded > healthy,
            "hits_left is a divisor: {wounded} !> {healthy}"
        );
    }

    #[test]
    fn zero_hits_left_skips_the_term_instead_of_dividing() {
        let mut i = base_cmp();
        i.t_type_has_hits = true;
        i.t_attack = 10;
        i.t_hits_left = 0;
        // Must not panic on the divide.
        assert!(compare_target(&i) >= 15);
    }

    #[test]
    fn mode_divides_by_damage_and_vetoes_a_zero() {
        let mut i = base_cmp();
        i.mode = true;
        i.estimated_damage = 0;
        i.t_alive = true;
        i.t_is_building = true;
        assert_eq!(compare_target(&i), 0);
    }

    #[test]
    fn out_of_range_costs_four_fifths_when_check_path_is_set() {
        let mut near = base_cmp();
        near.t_type_value = 100_000;
        near.check_path = true;
        near.in_range = true;
        let mut far = near;
        far.in_range = false;
        assert!(compare_target(&far) < compare_target(&near));
    }

    // -------- attack_dir --------

    #[test]
    fn attack_dir_is_the_direction_of_travel() {
        // Target due east of the attacker: the attack travels east.
        assert_eq!(attack_dir(0, 0, 100, 0), crate::trig::QUARTER_TURN);
        // Target due north: find_angle's zero.
        assert_eq!(attack_dir(0, 0, 0, -100), 0);
    }

    #[test]
    fn attack_dir_zero_against_facing_zero_is_the_rear_arc() {
        // This is the web lane's measurement, now explained: attack_dir 0 with the defender
        // facing 0 means the attack travels the way the defender looks, i.e. from behind.
        assert_eq!(flank_delta(0, 0), 0x8000_0000);
        assert_eq!(flank_tier(0, 0), 1, "dead astern is tier 1");
        assert_eq!(
            flank_tier(0, i32::MIN),
            0,
            "head-on is tier 0 — the caller's pre-guard, not flank_level"
        );
    }

    #[test]
    fn the_sides_are_worth_more_than_the_rear() {
        // Defender faces north (0). Attacker due east of it => attack travels west.
        let west = crate::trig::QUARTER_TURN.wrapping_mul(-1);
        assert_eq!(flank_tier(0, west), 2, "broadside is the maximum tier");
        let east = crate::trig::QUARTER_TURN;
        assert_eq!(flank_tier(0, east), 2);
        assert_eq!(flank_tier(0, 0), 1, "rear is only tier 1");
    }

    #[test]
    fn flank_tier_applies_the_pre_guard_that_flank_level_lacks() {
        // flank_level alone reports 2 for delta 0; the damage chain never asks it, because
        // 0x00644B1D rejects anything below 0x2AAAAAAA first.
        assert_eq!(flank_level(0), 2);
        assert_eq!(flank_tier(0, i32::MIN), 0);
    }

    // -------- the search --------

    #[derive(Clone, Copy)]
    struct TestAutoAdapter {
        hidden: ObjRef,
        invalid: ObjRef,
    }

    impl AutoTargetAdapter for TestAutoAdapter {
        fn is_enemy(&self, observer_who: i16, candidate_who: i16) -> bool {
            observer_who != candidate_who
        }

        fn is_seen(&self, _observer_who: i16, candidate: ObjRef) -> bool {
            candidate != self.hidden
        }

        fn candidate(
            &self,
            _searcher: ObjRef,
            candidate: ObjRef,
            _searcher_row: TargetRow,
            _candidate_row: TargetRow,
        ) -> Option<AutoTargetCandidate> {
            Some(AutoTargetCandidate {
                valid_target_const: candidate != self.invalid,
                check_target: true,
                check_path: false,
                distance_facts: AutoTargetDistanceFacts {
                    attacker: ObjectFootprint::Unit { block_radius: 0 },
                    target: ObjectFootprint::Unit { block_radius: 0 },
                    mode: AttackDistanceMode::for_unit_type(0, 0, 0),
                },
                compare: CompareTargetInput {
                    // Enough separation that the high-value rows also exercise priority,
                    // not merely chain order or distance.
                    t_type_value: if candidate.o >= 8 { 1_000_000 } else { 1_000 },
                    estimated_damage: 1,
                    ..CompareTargetInput::default()
                },
            })
        }
    }

    fn auto_query(searcher: ObjRef, frame: i32) -> AutoTargetQuery {
        AutoTargetQuery {
            searcher,
            frame,
            max_range_tiles: 1,
            min_range_tiles: 0,
            stance: 2,
            unit_masks: 0,
            has_objmask_high: false,
            unit_respond_range: 6,
            unit_defensive_respond_range: 4,
            compare_mode: false,
            last_order_target: None,
        }
    }

    #[test]
    fn auto_target_enforces_cadence_hostility_visibility_and_virtual_gates() {
        let mut w = TargetWorld::new(8, 8);
        let me = ObjRef::new(0, 0);
        let ally = ObjRef::new(1, 0);
        let hidden = ObjRef::new(8, 1);
        let invalid = ObjRef::new(9, 1);
        let admitted = ObjRef::new(3, 1);
        let far = ObjRef::new(10, 1);
        assert!(w.place_at(me, unit_row(400, 400)));
        assert!(w.place_at(ally, unit_row(450, 400)));
        assert!(w.place_at(hidden, unit_row(500, 400)));
        assert!(w.place_at(invalid, unit_row(550, 400)));
        assert!(w.place_at(admitted, unit_row(700, 400)));
        assert!(w.place_at(far, unit_row(4000, 400)));
        let adapter = TestAutoAdapter { hidden, invalid };
        let circle = spiral();

        assert_eq!(
            find_auto_target(&mut w, &circle, auto_query(me, 1), &adapter),
            AutoTargetStep::Deferred
        );
        let AutoTargetStep::Searched {
            max_dist,
            rings,
            result,
        } = find_auto_target(&mut w, &circle, auto_query(me, 0), &adapter)
        else {
            panic!("frame zero/object zero must be due");
        };
        assert_eq!(max_dist, 6 * TILE_UNITS);
        assert_eq!(rings, 2);
        assert_eq!(result.best, Some(admitted));
        assert_eq!(w.row(admitted).unwrap().targeted, 1);
        assert_eq!(w.row(hidden).unwrap().targeted, 0);
        assert_eq!(w.row(invalid).unwrap().targeted, 0);
        assert_eq!(w.row(far).unwrap().targeted, 0);
    }

    #[test]
    fn auto_target_ties_follow_wdata_head_order() {
        let mut w = TargetWorld::new(4, 4);
        let me = ObjRef::new(0, 0);
        let older = ObjRef::new(1, 1);
        let newer = ObjRef::new(2, 1);
        assert!(w.place_at(me, unit_row(100, 100)));
        assert!(w.place_at(older, unit_row(500, 100)));
        assert!(w.place_at(newer, unit_row(500, 100)));
        let adapter = TestAutoAdapter {
            hidden: ObjRef::NONE,
            invalid: ObjRef::NONE,
        };
        let AutoTargetStep::Searched { result, .. } =
            find_auto_target(&mut w, &spiral(), auto_query(me, 0), &adapter)
        else {
            unreachable!()
        };
        assert_eq!(
            result.best,
            Some(newer),
            "strictly-greater replacement keeps the first equal score"
        );
    }

    #[test]
    fn auto_target_ranking_uses_both_footprints_and_rectangular_axes() {
        #[derive(Clone, Copy)]
        struct RectAdapter;

        impl AutoTargetAdapter for RectAdapter {
            fn is_enemy(&self, observer_who: i16, candidate_who: i16) -> bool {
                observer_who != candidate_who
            }

            fn is_seen(&self, _observer_who: i16, _candidate: ObjRef) -> bool {
                true
            }

            fn candidate(
                &self,
                _searcher: ObjRef,
                _candidate: ObjRef,
                _searcher_row: TargetRow,
                _candidate_row: TargetRow,
            ) -> Option<AutoTargetCandidate> {
                Some(AutoTargetCandidate {
                    valid_target_const: true,
                    check_target: true,
                    check_path: false,
                    distance_facts: AutoTargetDistanceFacts {
                        attacker: ObjectFootprint::Unit { block_radius: 48 },
                        target: ObjectFootprint::Building {
                            x_size: 8,
                            y_size: 1,
                        },
                        mode: AttackDistanceMode::for_unit_type(0, 0, 0),
                    },
                    compare: CompareTargetInput {
                        t_type_value: 1_000_000,
                        estimated_damage: 1,
                        ..CompareTargetInput::default()
                    },
                })
            }
        }

        let mut w = TargetWorld::new(4, 4);
        let me = ObjRef::new(0, 0);
        let along_long_axis = ObjRef::new(1, 1);
        let along_short_axis = ObjRef::new(2, 1);
        assert!(w.place_at(me, unit_row(24, 24)));

        let mut horizontal = unit_row(984, 24);
        horizontal.is_unit = false;
        horizontal.is_building = true;
        let mut vertical = unit_row(24, 984);
        vertical.is_unit = false;
        vertical.is_building = true;
        assert!(w.place_at(along_long_axis, horizontal));
        assert!(w.place_at(along_short_axis, vertical));

        let AutoTargetStep::Searched { result, .. } =
            find_auto_target(&mut w, &spiral(), auto_query(me, 0), &RectAdapter)
        else {
            unreachable!()
        };
        assert_eq!(
            result.best,
            Some(along_long_axis),
            "the 8x1 building removes 768 units on x but only 96 on y"
        );
        // The former scalar approximation used max(8,1)*96 after vector_dist and made the
        // two centre-distance-equal candidates tie. Retail first leaves residual legs
        // 960-768-72=120 and 960-96-72=792, so priority/ranking must distinguish them.
        assert_eq!(result.nearest_dist, 120);
    }

    /// Admit every object of a *different* player, priority from the caller.
    ///
    /// The self-exclusion stands for `Object::valid_target` `0x00648BA0`, which is where
    /// retail rejects the searcher and its allies: `find_nearby_target` itself does not
    /// special-case `this`, it walks the cell chain and asks the gate. Reproducing that
    /// split matters — a port that hard-codes "skip self" inside the spiral would also skip
    /// the diplomacy rules that live beside it.
    fn admit_enemies<'a>(
        me: ObjRef,
        prio: &'a dyn Fn(ObjRef) -> i32,
    ) -> impl FnMut(&TargetWorld, ObjRef, i32) -> Option<Candidate> + 'a {
        move |w: &TargetWorld, r: ObjRef, d: i32| {
            if r.who == me.who {
                return None;
            }
            let row = w.row(r)?;
            Some(Candidate {
                who: r,
                priority: prio(r),
                attack_dist: d,
                weights: CandidateWeights {
                    a_is_unit: true,
                    ..CandidateWeights::default()
                },
                is_unit: row.is_unit,
            })
        }
    }

    #[test]
    fn the_search_finds_the_only_target() {
        let mut w = TargetWorld::new(16, 16);
        let me = w.push(0, unit_row(400, 400));
        let foe = w.push(1, unit_row(1000, 400));
        w.link(me);
        w.link(foe);
        let circle = spiral();
        let prio = |_: ObjRef| 1000;
        let r = find_nearby_target(&mut w, &circle, me, 4, admit_enemies(me, &prio));
        assert_eq!(r.best, Some(foe));
        assert_eq!(w.row(me).unwrap().near_o, foe.o);
        assert_eq!(w.row(me).unwrap().near_who, foe.who);
        assert_eq!(w.row(foe).unwrap().targeted, 1, "targeted was stamped");
    }

    #[test]
    fn the_closer_of_two_equal_targets_wins() {
        let mut w = TargetWorld::new(16, 16);
        let me = w.push(0, unit_row(400, 400));
        let near = w.push(1, unit_row(600, 400));
        let far = w.push(1, unit_row(3000, 400));
        w.link(me);
        w.link(near);
        w.link(far);
        let circle = spiral();
        let prio = |_: ObjRef| 100_000;
        let r = find_nearby_target(&mut w, &circle, me, 8, admit_enemies(me, &prio));
        assert_eq!(r.best, Some(near));
    }

    #[test]
    fn a_high_value_target_beats_a_closer_worthless_one() {
        let mut w = TargetWorld::new(16, 16);
        let me = w.push(0, unit_row(400, 400));
        let junk = w.push(1, unit_row(500, 400));
        let prize = w.push(1, unit_row(2000, 400));
        w.link(me);
        w.link(junk);
        w.link(prize);
        let circle = spiral();
        let prio = move |r: ObjRef| if r == prize { 1_000_000 } else { 100 };
        let r = find_nearby_target(&mut w, &circle, me, 8, admit_enemies(me, &prio));
        assert_eq!(r.best, Some(prize));
    }

    #[test]
    fn the_ring_budget_bounds_the_reach() {
        let mut w = TargetWorld::new(32, 32);
        let me = w.push(0, unit_row(400, 400));
        // Six cells east: outside a two-ring search, inside an eight-ring one.
        let foe = w.push(1, unit_row(400 + 6 * CELL_UNITS, 400));
        w.link(me);
        w.link(foe);
        let circle = spiral();
        let prio = |_: ObjRef| 1000;
        assert_eq!(
            find_nearby_target(&mut w, &circle, me, 2, admit_enemies(me, &prio)).best,
            None
        );
        assert_eq!(
            find_nearby_target(&mut w, &circle, me, 8, admit_enemies(me, &prio)).best,
            Some(foe)
        );
    }

    #[test]
    fn the_ten_unit_early_out_bounds_the_work() {
        // 400 enemies in a dense blob. Retail scores at most eleven of them.
        let mut w = TargetWorld::new(32, 32);
        let me = w.push(0, unit_row(4000, 4000));
        w.link(me);
        for i in 0..400 {
            let f = w.push(1, unit_row(4000 + (i % 20) * 30, 4000 + (i / 20) * 30));
            w.link(f);
        }
        let circle = spiral();
        let prio = |_: ObjRef| 1000;
        let r = find_nearby_target(&mut w, &circle, me, MAX_RINGS, admit_enemies(me, &prio));
        assert!(r.best.is_some());
        assert!(
            r.units_scored <= UNIT_CANDIDATE_LIMIT + 1,
            "scored {} units; retail stops after {}",
            r.units_scored,
            UNIT_CANDIDATE_LIMIT + 1
        );
        assert!(
            r.cells_visited < 64,
            "the early-out must stop the spiral, not just the chain: {} cells",
            r.cells_visited
        );
    }

    #[test]
    fn the_near_cache_clears_past_0xf00() {
        let mut w = TargetWorld::new(64, 64);
        let me = w.push(0, unit_row(400, 400));
        let foe = w.push(1, unit_row(400 + 0x2000, 400));
        w.link(me);
        w.link(foe);
        let circle = spiral();
        let prio = |_: ObjRef| 1000;
        let r = find_nearby_target(&mut w, &circle, me, MAX_RINGS, admit_enemies(me, &prio));
        assert_eq!(r.best, Some(foe), "still a legal target");
        assert!(r.nearest_dist > NEAR_CACHE_MAX_DIST);
        assert_eq!(
            w.row(me).unwrap().near_o,
            NO_LINK,
            "the cached nearest pair is cleared beyond 0xF00"
        );
    }

    // -------- the engagement loop --------

    #[test]
    fn a_unit_acquires_closes_and_hits() {
        let mut w = TargetWorld::new(32, 32);
        let me = w.push(0, unit_row(400, 400));
        let foe = w.push(1, unit_row(400 + 5 * TILE_UNITS, 400));
        w.link(me);
        w.link(foe);
        let circle = spiral();
        // One-tile reach, 40 world units per frame. Reach 0 would be legal but degenerate:
        // the attacker ends exactly on the target and `find_angle(0, 0)` returns HALF_TURN
        // (`dx == 0`, `ny == 0` is not `> 0`), which reads as a head-on hit.
        let mut e = Engagement::new(me, 1, 40);

        let mut closed = 0;
        let mut hits = 0;
        let mut first_hit_flank = None;
        for _ in 0..200 {
            let prio = |_: ObjRef| 10_000;
            let step = e.step(
                &mut w,
                &circle,
                MAX_RINGS,
                admit_enemies(me, &prio),
                |_dir, flank| (7 + flank as i32, 30),
            );
            match step {
                EngageStep::NoTarget => panic!("target must be acquired on frame 1"),
                EngageStep::Closing { .. } => closed += 1,
                EngageStep::Recharging { .. } => {}
                EngageStep::Hit { flank, .. } => {
                    hits += 1;
                    first_hit_flank.get_or_insert(flank);
                }
            }
        }
        assert!(closed > 0, "the unit had to walk in");
        assert!(hits > 0, "the unit landed at least one blow");
        assert!(
            w.row(foe).unwrap().damage > 0,
            "damage accumulated on the defender"
        );
        // Attacker due west of a defender facing north => attack travels east => broadside.
        assert_eq!(first_hit_flank, Some(2));
    }

    #[test]
    fn the_whole_chain_runs_selection_into_get_damage() {
        // The point of the lane: an acquisition feeds the real 31-step damage chain.
        use crate::mechanics::{damage, DamageInput, DamagePredicates};
        use crate::systems::combat::CombatConstants;

        let rules_const = CombatConstants::shipped();
        let rules = combat_rules(&rules_const);
        let unreached = unreached_terms(&rules_const, 0);

        let mut w = TargetWorld::new(32, 32);
        let me = w.push(0, unit_row(4000, 4000));
        // Defender due west, facing north, so the attack arrives on its right flank.
        let foe = w.push(1, unit_row(4000 - 4 * TILE_UNITS, 4000));
        w.link(me);
        w.link(foe);
        let circle = spiral();
        let mut e = Engagement::new(me, 2, 48);

        let mut total = 0;
        let mut shots = 0;
        for _ in 0..120 {
            let prio = |_: ObjRef| 50_000;
            let step = e.step(
                &mut w,
                &circle,
                MAX_RINGS,
                admit_enemies(me, &prio),
                |dir, flank| {
                    let d = damage(
                        &DamageInput {
                            balance_pct: 100,
                            attack: 80, // stored x10 -> 8 display attack
                            armor: 2,
                            attack_dir: dir,
                            attacker_type_id: 60,
                            defender_type_id: 61,
                            current_frame: 1,
                            ..DamageInput::default()
                        },
                        &DamagePredicates::default(),
                        &rules,
                        &unreached,
                    );
                    // Bookkeeping only: the flank tier the selection computed.
                    let _ = flank;
                    (d, 25)
                },
            );
            if let EngageStep::Hit { damage: d, .. } = step {
                total += d;
                shots += 1;
            }
        }
        assert!(shots > 0, "no shot ever resolved");
        assert!(
            total > 0,
            "get_damage returned nothing across {shots} shots — the chain did not run"
        );
        assert_eq!(
            e.target,
            Some(foe),
            "the engagement held the target it acquired"
        );
    }

    #[test]
    fn rules_bridge_carries_the_shipped_block() {
        let c = crate::systems::combat::CombatConstants::shipped();
        let r = combat_rules(&c);
        assert_eq!(r.flank_bonus, 50);
        assert_eq!(r.recapture_city_modifier, 512);
        assert_eq!(r.rule_0x558, 0, "SUPER_IMMUNE, +0x0558");
        assert_eq!(r.rule_0x76c, 25, "RUSSIAN_COSSACK_DAMAGE, +0x076C");
        assert_eq!(r.rule_0xb98, 204, "ANTIPATER_ENTRENCH_BONUS, +0x0B98");
        let u = unreached_terms(&c, 3);
        assert_eq!(u.rule_0xbbc, 1, "WELLINGTON_SIEGE_ATTACK, +0x0BBC");
        assert_eq!(u.rule_0x794, -5, "JAPANESE_DAMAGE, +0x0794");
        assert_eq!(u.step11_player_level, 3);
    }

    /// Scaling of the retail acquisition structure against a naive all-objects scan.
    ///
    /// `cargo test -p don-sim --lib systems::target::tests::acquisition_scaling -- --ignored
    /// --nocapture`. Ignored by default because it is a measurement, not an assertion about
    /// retail.
    #[test]
    #[ignore]
    fn acquisition_scaling() {
        use std::time::Instant;
        for &n in &[128usize, 512, 1024, 2048, 4096] {
            let side = (n as f64).sqrt().ceil() as i32;
            let mut w = TargetWorld::new(64, 64);
            let me = w.push(0, unit_row(20_000, 20_000));
            w.link(me);
            for i in 0..n as i32 {
                let x = 20_000 + (i % side) * 96 - side * 48;
                let y = 20_000 + (i / side) * 96 - side * 48;
                let f = w.push(1, unit_row(x, y));
                w.link(f);
            }
            let circle = spiral();
            let reps = 200;

            let t0 = Instant::now();
            let mut sink = 0i64;
            for _ in 0..reps {
                let prio = |_: ObjRef| 10_000;
                let r =
                    find_nearby_target(&mut w, &circle, me, MAX_RINGS, admit_enemies(me, &prio));
                sink += r.best_score as i64;
            }
            let grid = t0.elapsed();

            // The naive shape a from-scratch port reaches for: score every object.
            let t1 = Instant::now();
            let mut sink2 = 0i64;
            for _ in 0..reps {
                let (sx, sy) = {
                    let m = w.row(me).unwrap();
                    (m.x, m.y)
                };
                let mut best = 0;
                for who in 0..PLAYER_SLOTS as i16 {
                    if who == me.who {
                        continue;
                    }
                    for o in 0..w.objects[who as usize].len() as i16 {
                        let r = ObjRef::new(o, who);
                        let row = w.row(r).unwrap();
                        if !row.is_alive() {
                            continue;
                        }
                        let d = scalar_attack_dist(sx, sy, row.x, row.y, 0);
                        let s = rank_candidate(10_000, d, &plain_weights()).unwrap_or(0);
                        if s > best {
                            best = s;
                        }
                    }
                }
                sink2 += best as i64;
            }
            let naive = t1.elapsed();

            println!(
                "n={n:5}  grid {:>9.1?}/acq  naive {:>9.1?}/acq  speedup {:>6.1}x  (sinks {sink} {sink2})",
                grid / reps,
                naive / reps,
                naive.as_secs_f64() / grid.as_secs_f64()
            );
        }
    }

    #[test]
    fn recharge_gates_the_rate_of_fire() {
        let mut w = TargetWorld::new(32, 32);
        let me = w.push(0, unit_row(400, 400));
        let foe = w.push(1, unit_row(500, 400));
        w.link(me);
        w.link(foe);
        let circle = spiral();
        let mut e = Engagement::new(me, 1, 0);
        let mut hits = 0;
        for _ in 0..100 {
            let prio = |_: ObjRef| 10_000;
            if let EngageStep::Hit { .. } = e.step(
                &mut w,
                &circle,
                MAX_RINGS,
                admit_enemies(me, &prio),
                |_d, _f| (5, 20),
            ) {
                hits += 1;
            }
        }
        // 100 frames, 20-frame reload: five shots, not a hundred.
        assert_eq!(hits, 5, "recharging must gate the loop");
    }
}
