//! Territory and ever-seen: `Wall::check_ever_seen` 0x0063ce70,
//! `GameDaemon::check_borders` 0x00732060 and `World::compute_reg_territory`
//! 0x006b0bb0 — the code that writes the `WData::who`/`who2` owner plane of
//! `World.wdata`, the per-region `LeaderData::reg_terr[64]` counts and the
//! `territory`/`territory_high` totals. Leaf module: free functions for the
//! owning traversals (`build_process` for the Wall body, `game_daemon` for
//! `check_borders`); never edits sibling modules.
//!
//! # What the captures say (oracle, read before trusting any of this)
//!
//! Measured with [`tests::dump_world_diffs`] on every consecutive pair of the
//! stride-1 (`f11..f40`) and stride-15 (`f40..f325`) captures:
//!
//! * `WData::who` (+0xf) and `who2` (+0x10) **never change**. The wdata
//!   bytes that do move are `down`/`down_who` (+0x8..+0xb, object chain
//!   links), `was_seen` (+0x14) and `flags`/`land_sub` (+1/+3). The
//!   "World.? ~1.8 KB/frame" unexplained residue is therefore *not* the
//!   territory plane.
//! * Every `Region.flags` carries `0x80` (territory complete) and every
//!   leader's `territory == Σ reg_terr` already; `territory`,
//!   `territory_high`, `reg_terr`, `my_team_terr`, `other_team_terr`,
//!   `min_other_team_terr` are constant across all 40+ pairs. Borders are
//!   fully settled: `check_borders` is a no-op on every captured frame.
//!
//! So the only gate these captures can enforce is `introduced == 0`
//! ([`tests::territory_introduces_nothing`]); the per-cell algorithm is
//! transcribed from the instruction stream and has **no** oracle yet.
//!
//! # Cadence
//!
//! * `check_ever_seen(0)` runs from `Wall::process` when
//!   `frame != 0 && (frame & 7) == who` (transcribed in `build_process`),
//!   and `check_ever_seen(1)` from the foreign-territory arm there.
//! * `check_borders` runs once per `GameDaemon::process_all` (every frame,
//!   step 12), after the region flag pass. It only does work while some
//!   region has `borders < size`; `compute_reg_territory` consumes at most
//!   `GameDaemon::borders` = 256 cells per frame across all regions
//!   (`0x006b0bb0` returns 1 as soon as the budget is spent).
//! * The totals tail (`territory`, `territory_high`) is recomputed only on
//!   the frame the *last* incomplete region completes.
//!
//! # Unwalked state
//!
//! `Region::borders` (+0x2c, the resume cursor) is outside the walked 44-byte
//! head, so a partially computed region cannot be resumed from a save. The
//! walked proxy is `Region.flags & 0x80`: cleared by `compute_reg_territory`
//! when it starts a region from cursor 0 (`0x006b0c5c`), set when the region
//! completes (`0x006b1a2f`) and on every claimed cell. `Region::fix_borders`
//! 0x00680f60 (city founded / razed / assimilated) zeroes all 64 cursors;
//! the next `check_borders` then recomputes from scratch, which *is*
//! reproducible from a save. A region with `0x80` clear is treated as
//! "cursor 0"; a region with `0x80` set as complete.
//!
//! RNG: none of `check_ever_seen`, `check_borders`, `compute_reg_territory`
//! or their callees (`tile_corner` 0x00643440, `Leader::meet` 0x006e1250,
//! `Borders::*` 0x008835e0/0x008836d0/0x00883780, `Leader::new_rare`
//! 0x006d9e70, `LeaderData::has_preq` 0x006db810, `has_wonder` 0x006ebc10,
//! `has_tribe_bonus` 0x006e1370, `get_handicap` 0x006da740) call
//! `Random::get` 0x00a39d70 — zero `game_random` draws on every path here.

#![allow(dead_code)]

use super::img::{constant, get_i32, get_u16, leader_flags, put_i32, put_u16, type_rec, Img};
use crate::sections::{Build, Obj, Save};
use crate::tick::StepStatus;

/// `check_ever_seen` is transcribed (bits + `Leader::meet` treaties);
/// `check_borders` cadence and totals tail are transcribed; the
/// `compute_reg_territory` per-cell body is transcribed but its leader
/// precompute needs `has_preq`/`has_wonder`/`has_tribe_bonus`/`get_handicap`
/// (TODO below) and is skipped — fields untouched — when they are needed.
pub const STATUS: StepStatus = StepStatus::Partial;

// ---------------------------------------------------------------------------
// Image offsets
// ---------------------------------------------------------------------------

/// `LeaderData` offsets (PDB).
const LD_WHO: usize = 0x08;
const LD_DIPLOS: usize = 0x74; // int[8]
const LD_TREATIES: usize = 0x94; // int[8]
const LD_CITY_MARK: usize = 0x408;
const LD_FORT_MARK: usize = 0x428;
const LD_REG_KNOWN_RARES: usize = 0x4d4; // int[64]
const LD_TERRITORY: usize = 0x9d8;
const LD_TERRITORY_HIGH: usize = 0x9dc;
const LD_REG_TERR: usize = 0x145e; // u16[64]
const LD_ALLY_MASK: usize = 0x6929;
const LD_RARE_A: usize = 0x6da6;
const LD_RARE_B: usize = 0x6dce;
const LD_TEAM_BONUS: usize = 0x6916;

/// `Game` scalars (image − 0x550).
const GAME_EVERYONE_MASK: usize = 0x6dc - 0x550;
const GAME_SEMAPHORE: usize = 0x820 - 0x550; // semaphore.ptr[32]

/// `World` direct block (`World.direct[k]` = World+(8+4k)).
const WORLD_FOG_XS: usize = 1; // +0x0c
const WORLD_TILE_XS: usize = 4; // +0x18
const WORLD_TILE_YS: usize = 5; // +0x1c
const WORLD_PLAYER_LIMIT: usize = 12; // +0x38 player_territory_limit
const WORLD_PLAYER_LIMIT_CIVIC: usize = 13; // +0x3c
const WORLD_PLAYER_LIMIT_CITY: usize = 14; // +0x40
const WORLD_COLONIZED_LIMIT: usize = 15; // +0x44
const WORLD_COLONIZED_LIMIT_CIVIC: usize = 16; // +0x48
const WORLD_COLONIZED_LIMIT_CITY: usize = 17; // +0x4c

/// `Region.head` offsets (44 B head after the tag).
const REG_FLAGS: usize = 0x08;
const REG_SIZE: usize = 0x14;

/// `Constants` offsets (`docs/mechanics/borders-fog.md` §4.1, re-read
/// against the instruction stream).
const C_FORT_UPGRADE_TERR: usize = 0x0b8; // [4]
const C_TEMPLE_UPGRADE_TERR: usize = 0x0c8; // [5] (indexed as 0xc4 + lvl*4)
const C_CIVIC_UPGRADE_TERR: usize = 0x0dc; // [8]
const C_CAPITAL_TERRITORY_BONUS: usize = 0x0fc;
const C_CITY_UPGRADE_TERR: usize = 0x100; // [3]
const C_FORT_TERRITORY_MULTIPLIER: usize = 0x10c;
const C_CITY_TERRITORY_MULTIPLIER: usize = 0x110;
const C_TERRITORY_BASE: usize = 0x114;
const C_TERRITORY_DEN: usize = 0x124;
const C_TERRITORY_NUM: usize = 0x128;
const C_COLOSSUS_TERR: usize = 0x46c; // has_wonder(0x212) border bonus
const C_COLOSSUS_FORT_TERR: usize = 0x478; // has_wonder(0x212) fort bonus
const C_TIKAL_TEMPLE_HP: usize = 0x4a0; // has_wonder(0x214) — retail wiring bug, see borders-fog.md §4.2
const C_WONDER_21C_TERR: usize = 0x53c; // has_wonder(0x21c)
const C_TRIBE6_FORT_TERR: usize = 0x604; // has_tribe_bonus(6)
const C_TRIBE_D_TERR_BASE: usize = 0x760; // has_tribe_bonus(0xd)
const C_TRIBE_D_TERR_PER_GOV: usize = 0x764;
const C_GEMS_TERRITORY_BONUS: usize = 0x930;
const C_TEAM_TEMPLE_TERR: usize = 0xb08;

/// `div_3_table[c >> 6]` = fine-unit coordinate → tile (`/192`);
/// `div_3_table[c >> 8]` → WCoord cell (`/768`).
fn fine_to_tile(c: i32) -> i32 {
    (c >> 6) / 3
}
fn fine_to_wcoord(c: i32) -> i32 {
    (c >> 8) / 3
}

fn leader_i32(save: &Save, who: usize, off: usize) -> Option<i32> {
    super::img::leader_i32(save, who, off)
}

fn leader_u8(save: &Save, who: usize, off: usize) -> Option<u8> {
    super::img::leader_u8(save, who, off)
}

fn leader_live(save: &Save, who: usize) -> bool {
    save.leaders.slots.get(who).map(|l| l.flags & 1 != 0 && l.body.len() == 0x6922).unwrap_or(false)
}

fn with_build<R>(save: &mut Save, owner: usize, slot: usize, f: impl FnOnce(&Save, &mut Build) -> R) -> Option<R> {
    let mut taken = match save.objects.lists.get_mut(owner)?.elems.get_mut(slot)? {
        Some(Obj::Build(b)) => std::mem::take(&mut **b),
        _ => return None,
    };
    let r = f(save, &mut taken);
    if let Some(Obj::Build(b)) = save.objects.lists[owner].elems[slot].as_mut() {
        **b = taken;
    }
    Some(r)
}

// ---------------------------------------------------------------------------
// Wall::check_ever_seen 0x0063ce70
// ---------------------------------------------------------------------------

/// `WallData::tile_corner(&x, &y)` 0x00643440: top-left footprint tile of
/// the object. `tile = div3[fine >> 6]`; re-centred by half a tile when the
/// footprint width/height (`BuildType +0x234`/`+0x238`) is odd; minus half
/// the footprint.
fn tile_corner(save: &Save, b: &Build) -> Option<(i32, i32)> {
    let img = Img(b);
    let t = type_rec(save, img.ptype())?;
    let (xs, ys) = (t.i32(0x234)?, t.i32(0x238)?);
    let cx = fine_to_tile(img.x()) * 0xc0;
    let cy = fine_to_tile(img.y()) * 0xc0;
    let cx = if xs & 1 != 0 { cx + 0x60 } else { cx };
    let cy = if ys & 1 != 0 { cy + 0x60 } else { cy };
    Some((fine_to_tile(cx) - (xs >> 1), fine_to_tile(cy) - (ys >> 1)))
}

/// `Wall::check_ever_seen(int force)` 0x0063ce70.
///
/// ```text
/// old = ever_seen;
/// mask = is_started() ? Game.everyone_mask : leader[who].ally_mask;
/// if ((ever_seen & mask) != mask || (ever_seen_completed & mask) != mask) {
///   (cx, cy) = tile_corner();
///   for i in 0..type.xs: fx = (cx + i) >> 1
///     for j in 0..type.ys: fy = (cy + j) >> 1
///       s = World.seen[fog_xs * fy + fx];
///       if (!is_started()) ever_seen |= s & leader[who].ally_mask;
///       else { ever_seen |= s; if (is_active()) ever_seen_completed |= s; }
/// }
/// if (old != ever_seen || force) {
///   for u in 0..8: if u != who && leader[u].flags & 1 && (ever_seen & ally_mask[u])
///                     && !(old & ally_mask[u]):
///     changed = true;
///     if (!(leader[who].treaties[u] & 1)) Leader::meet(u, x, y);   // 0x006e1250
///   if (changed) update_local_seen();                              // 0x0063ed50
/// }
/// ```
///
/// `Leader::meet` → `LeaderData::set_treaty(u, 1)` 0x006e1190 sets
/// `treaties[u] |= 1` on *both* leaders, plus local-player UI
/// (`FUN_006d3570`, not state). `Wall::update_local_seen` stamps the fog
/// `was_seen`/`seen2` planes over the footprint+1 ring through
/// `World::set_was_seen` 0x006b4bb0 — owned by the fog lane, left as TODO
/// here (`effects` records that it fired).
///
/// The fog `seen` plane index is `fog_xs * fy + fx` with fog cells of 2
/// tiles (`FCoord`); `fog_xs` is `World.direct[1]`. Out-of-range cells are
/// skipped (retail would read past the plane; the captures never place a
/// footprint on the map edge).
pub fn check_ever_seen(save: &mut Save, owner: usize, slot: usize, force: bool, effects: &mut Vec<String>) {
    let Some(Some((who, changed, old, new))) = with_build(save, owner, slot, |save, b| {
        let img = Img(&*b);
        if !img.complete() {
            return None;
        }
        let who = img.who() as usize;
        let old = img.u8(0x62);
        let started = img.flags() & 2 != 0;
        let active = img.flags() & 4 != 0;
        let ally_mask = leader_u8(save, who, LD_ALLY_MASK)? as u32;
        let mask = if started { get_i32(&save.game.scalars, GAME_EVERYONE_MASK) as u32 } else { ally_mask };
        let mut ever_seen = old as u32;
        let mut completed = img.u8(0x63) as u32;
        if (ever_seen & mask) != mask || (completed & mask) != mask {
            let (cx, cy) = tile_corner(save, b)?;
            let t = type_rec(save, Img(&*b).ptype())?;
            let (xs, ys) = (t.i32(0x234)?, t.i32(0x238)?);
            let fog_xs = save.world.direct[WORLD_FOG_XS];
            for i in 0..xs {
                let fx = (cx + i) >> 1;
                for j in 0..ys {
                    let fy = (cy + j) >> 1;
                    if fx < 0 || fy < 0 {
                        continue;
                    }
                    let Some(&s) = save.world.seen.get((fog_xs * fy + fx) as usize) else { continue };
                    let s = s as u32;
                    if !started {
                        ever_seen |= s & ally_mask;
                    } else {
                        ever_seen |= s;
                        if active {
                            completed |= s;
                        }
                    }
                }
            }
            let mut w = Img(&mut *b);
            w.set_u8(0x62, ever_seen as u8);
            w.set_u8(0x63, completed as u8);
        }
        Some((who, ever_seen as u8 != old || force, old, ever_seen as u8))
    }) else {
        return;
    };
    let tag = format!("Objects.lists[{owner}][{slot}]");
    if new != old {
        effects.push(format!("{tag}.Wall.ever_seen {old:#x} -> {new:#x}"));
    }
    if !changed {
        return;
    }
    let mut any = false;
    for u in 0..8usize {
        if u == who || !leader_live(save, u) {
            continue;
        }
        let Some(am) = leader_u8(save, u, LD_ALLY_MASK) else { continue };
        if new & am == 0 || old & am != 0 {
            continue;
        }
        any = true;
        let treaty = leader_i32(save, who, LD_TREATIES + 4 * u).unwrap_or(0);
        if treaty & 1 == 0 {
            // Leader::meet(u, x, y) 0x006e1250 → set_treaty(u, 1) 0x006e1190.
            for (a, b) in [(who, u), (u, who)] {
                if let Some(l) = save.leaders.slots.get_mut(a) {
                    if l.body.len() == 0x6922 {
                        let off = LD_TREATIES + 4 * b - 8;
                        let v = get_i32(&l.body, off);
                        put_i32(&mut l.body, off, v | 1);
                    }
                }
            }
            effects.push(format!("{tag}: Leader::meet({who} <-> {u}) treaties |= 1"));
        }
    }
    if any {
        // TODO(va 0x0063ed50) Wall::update_local_seen → World::set_was_seen
        // 0x006b4bb0 over the footprint+1 ring (fog lane).
        effects.push(format!("{tag}: Wall::update_local_seen (0x0063ed50) fired — fog planes untouched"));
    }
}

// ---------------------------------------------------------------------------
// GameDaemon::check_borders 0x00732060
// ---------------------------------------------------------------------------

/// `GameDaemon::check_borders()` 0x00732060, the region branch:
///
/// ```text
/// borders = 0;                                    // GameDaemon+0x24 (game_daemon.rs owns this write)
/// r = 0;
/// for reg in 0..64: if (Regions[reg].size != 0 && Regions[reg].borders < size) {
///   r |= compute_reg_territory(reg, 0); if (busy == 0) busy = 1; }
/// if (r & 2) {
///   if (!(r & 1)) { UI flag |= 2; if (busy == 0) busy = 1; }
///   for reg in 0..64: if (size != 0 && borders < size) return;   // still incomplete
///   for who in 0..8: if (leader.flags & 1) {
///     territory = Σ_{reg: size != 0} reg_terr[reg];
///     territory_high = max(territory_high, territory); }
/// }
/// ```
///
/// `borders < size` is evaluated through the walked proxy
/// `Region.flags & 0x80 == 0` (module doc). `busy` (GameDaemon+0x28) is not
/// walked.
pub fn check_borders(save: &mut Save, effects: &mut Vec<String>) {
    let mut r = 0u8;
    let n = save.regions.elems.len().min(64);
    for reg in 0..n {
        if region_incomplete(save, reg) {
            r |= compute_reg_territory(save, reg, false, effects);
        }
    }
    if r & 2 == 0 {
        return;
    }
    if (0..n).any(|reg| region_incomplete(save, reg)) {
        return;
    }
    recompute_territory_totals(save, effects);
}

fn region_size(save: &Save, reg: usize) -> i32 {
    save.regions.elems.get(reg).filter(|r| r.head.len() >= 44).map(|r| get_i32(&r.head, REG_SIZE)).unwrap_or(0)
}

fn region_flags(save: &Save, reg: usize) -> i32 {
    save.regions.elems.get(reg).filter(|r| r.head.len() >= 44).map(|r| get_i32(&r.head, REG_FLAGS)).unwrap_or(0)
}

fn set_region_flags(save: &mut Save, reg: usize, f: i32) {
    if let Some(r) = save.regions.elems.get_mut(reg) {
        if r.head.len() >= 44 {
            put_i32(&mut r.head, REG_FLAGS, f);
        }
    }
}

/// `size != 0 && borders < size`, through the `flags & 0x80` proxy.
fn region_incomplete(save: &Save, reg: usize) -> bool {
    region_size(save, reg) != 0 && region_flags(save, reg) & 0x80 == 0
}

/// Totals tail of `check_borders` (0x007320f6..0x0073215f).
pub fn recompute_territory_totals(save: &mut Save, effects: &mut Vec<String>) {
    let sizes: Vec<i32> = (0..64).map(|reg| region_size(save, reg)).collect();
    for who in 0..8usize {
        if !leader_live(save, who) {
            continue;
        }
        let l = &mut save.leaders.slots[who];
        let mut total = 0i32;
        for (reg, &sz) in sizes.iter().enumerate() {
            if sz != 0 {
                total += get_u16(&l.body, LD_REG_TERR - 8 + 2 * reg) as i32;
            }
        }
        let old = get_i32(&l.body, LD_TERRITORY - 8);
        let high = get_i32(&l.body, LD_TERRITORY_HIGH - 8);
        put_i32(&mut l.body, LD_TERRITORY - 8, total);
        put_i32(&mut l.body, LD_TERRITORY_HIGH - 8, high.max(total));
        if old != total || high.max(total) != high {
            effects.push(format!("Leader[{who}].territory {old} -> {total} (high {high} -> {})", high.max(total)));
        }
    }
}

// ---------------------------------------------------------------------------
// World::compute_reg_territory 0x006b0bb0
// ---------------------------------------------------------------------------

/// Per-leader precompute (0x006b0cd0..0x006b0ff8), one row per leader slot.
/// Names follow the decompiler's stack arrays so the body can be audited
/// against `re/decomp-all/006b0bb0.c`.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct LeaderTerr {
    /// `local_170`: temple level 1..4 from `has_preq(0x2ca/0x2c9/0x2c8)`.
    temple_level: i32,
    /// `local_f0`: `TEMPLE_UPGRADE_TERR[temple_level]` with the Tikal and
    /// team-temple percent scalings.
    temple_terr: i32,
    /// `aiStack_130`: fort level − 1 from `has_preq(0x2d3/0x2d2/0x2d1)`,
    /// plus `(TRIBE6 + 1) / 2` under `has_tribe_bonus(6)`.
    fort_level: i32,
    /// `aiStack_110`: `FORT_UPGRADE_TERR[fort_level]` + wonder/tribe adds.
    fort_terr: i32,
    /// `aiStack_150`: `CIVIC_UPGRADE_TERR[gov]`.
    civic_terr: i32,
    /// `local_d0`: the hard radius cap base — `limit + gov * limit_civic`
    /// plus the rare/wonder/tribe extensions.
    city_cap: i32,
    /// `local_b0`: summed border bonuses (gems, wonders, tribe, handicap).
    bonus: i32,
    /// `aiStack_190`: `fort_level * limit_city + city_cap`.
    fort_cap: i32,
}

/// Per-region territory limit triple (`World +0x38..` or `+0x44..` by
/// `Region.flags & 4`).
fn limits(save: &Save, reg: usize) -> (i32, i32, i32) {
    let d = &save.world.direct;
    if region_flags(save, reg) & 4 == 0 {
        (d[WORLD_COLONIZED_LIMIT], d[WORLD_COLONIZED_LIMIT_CIVIC], d[WORLD_COLONIZED_LIMIT_CITY])
    } else {
        (d[WORLD_PLAYER_LIMIT], d[WORLD_PLAYER_LIMIT_CIVIC], d[WORLD_PLAYER_LIMIT_CITY])
    }
}

/// `LeaderData::has_preq(TypeIndex)` 0x006db810 for the temple/fort
/// upgrade bonus types `0x2c8..=0x2ca`, `0x2d1..=0x2d3`.
/// TODO(va 0x006db810): `leaders_process` holds a partial on its `LeaderCtx`;
/// not reachable from here. `None` = unknown.
fn has_preq(_save: &Save, _who: usize, _t: i32) -> Option<bool> {
    None
}

/// `LeaderData::has_wonder(int)` 0x006ebc10 for 0x212 / 0x214 / 0x21c.
/// TODO(va 0x006ebc10): needs the owned-wonder scan; `None` = unknown.
fn has_wonder(_save: &Save, _who: usize, _w: i32) -> Option<bool> {
    None
}

/// `LeaderData::has_tribe_bonus(int)` 0x006e1370 for 6 / 0xd.
/// TODO(va 0x006e1370): `None` = unknown.
fn has_tribe_bonus(_save: &Save, _who: usize, _b: i32) -> Option<bool> {
    None
}

/// `LeaderData::get_handicap()` 0x006da740 (human players under
/// `Game.semaphore & 4`). TODO(va 0x006da740).
fn get_handicap(_save: &Save, _who: usize) -> Option<i32> {
    None
}

/// Government level: `data_encrypted[0xec/4] ^ 0x63187`.
fn gov_level(save: &Save, who: usize) -> Option<i32> {
    let l = save.leaders.slots.get(who)?;
    let v = l.data_encrypted.get(0xec..0xf0)?;
    Some(i32::from_le_bytes(v.try_into().unwrap()) ^ 0x63187)
}

/// 0x006b0cd0..0x006b0ff8 for one live leader. `None` when a helper above
/// is needed and unavailable.
fn leader_terr(save: &Save, who: usize, lim: (i32, i32, i32)) -> Option<LeaderTerr> {
    let (limit, limit_civic, limit_city) = lim;
    let c = |off| constant(save, off);
    let mut t = LeaderTerr::default();
    t.temple_level = if has_preq(save, who, 0x2ca)? {
        4
    } else if has_preq(save, who, 0x2c9)? {
        3
    } else if has_preq(save, who, 0x2c8)? {
        2
    } else {
        1
    };
    t.temple_terr = c(C_TEMPLE_UPGRADE_TERR - 4 + t.temple_level as usize * 4);
    if has_wonder(save, who, 0x214)? {
        t.temple_terr = ((c(C_TIKAL_TEMPLE_HP) + 100) * t.temple_terr) / 100;
    }
    let sem = save.game.scalars.get(GAME_SEMAPHORE + 2).copied().unwrap_or(0);
    if sem & 2 != 0 && leader_u8(save, who, LD_TEAM_BONUS)? != 0 {
        t.temple_terr = ((c(C_TEAM_TEMPLE_TERR) + 100) * t.temple_terr) / 100;
    }
    let fl = if has_preq(save, who, 0x2d3)? {
        4
    } else if has_preq(save, who, 0x2d2)? {
        3
    } else if has_preq(save, who, 0x2d1)? {
        2
    } else {
        1
    } - 1;
    t.fort_level = fl;
    t.fort_terr = c(C_FORT_UPGRADE_TERR + fl as usize * 4);
    if has_wonder(save, who, 0x212)? {
        t.fort_terr += c(C_COLOSSUS_FORT_TERR);
    }
    if has_tribe_bonus(save, who, 6)? {
        let v = c(C_TRIBE6_FORT_TERR);
        t.fort_terr += v;
        t.fort_level = (v + 1) / 2 + fl;
    }
    let gov = gov_level(save, who)?;
    t.civic_terr = c(C_CIVIC_UPGRADE_TERR + gov as usize * 4);
    t.city_cap = limit + gov * limit_civic;
    t.bonus = 0;
    let tribe_d = has_tribe_bonus(save, who, 0xd)?;
    if leader_u8(save, who, LD_RARE_A)? & 0x80 != 0 || leader_u8(save, who, LD_RARE_B)? & 0x80 != 0 {
        let v = c(C_GEMS_TERRITORY_BONUS);
        t.bonus = if tribe_d { v / 2 } else { v };
        t.city_cap += limit_civic;
    }
    if has_wonder(save, who, 0x212)? {
        let v = c(C_COLOSSUS_TERR);
        t.bonus += if tribe_d { v / 2 } else { v };
        t.city_cap += ((if tribe_d { 1 } else { v }) + 1) / 2 * limit_civic;
    }
    if has_wonder(save, who, 0x21c)? {
        let v = c(C_WONDER_21C_TERR);
        t.bonus += if tribe_d { v / 2 } else { v };
        t.city_cap += ((if tribe_d { 1 } else { v }) + 1) / 2 * limit_civic;
    }
    if tribe_d {
        let base = c(C_TRIBE_D_TERR_BASE);
        t.bonus += c(C_TRIBE_D_TERR_PER_GOV) * gov + base;
        t.city_cap += ((base != 0) as i32 + gov) * limit_civic;
    }
    let handicap = if leader_flags(save, who) & 4 != 0 && sem & 4 != 0 { get_handicap(save, who)? } else { 0 };
    t.bonus += (handicap + 0xf) / 0x19;
    t.fort_cap = t.fort_level * limit_city + t.city_cap;
    Some(t)
}

/// `mn²/(2·mx) + mx` second-order hypot used by both `circle_init` and the
/// territory loop (0x006b124d..), with the `mn >= 60000` overflow fallback
/// `(mn + 2·mx) >> 1`.
pub fn hypot_approx(a: i32, b: i32) -> i32 {
    let (a, b) = (a.abs(), b.abs());
    let (mn, mx) = if b < a { (b, a) } else { (a, b) };
    if mx == 0 {
        return 0;
    }
    if mn == 0 {
        return mx;
    }
    if (mn as u32) < 60000 {
        ((mn as u32).wrapping_mul(mn as u32) / (2 * mx as u32)) as i32 + mx
    } else {
        ((mn as u32).wrapping_add(2 * mx as u32) >> 1) as i32
    }
}

/// Distance compression (0x006b13a3..): `if d<13: d=2d/3; if d<9: d=2d/3;
/// if d<5: d/=2` (each test on the running value).
pub fn compress(mut d: i32) -> i32 {
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

/// `(TERRITORY_DEN * d * 256) / (border_bonuses * multiplier + TERRITORY_NUM)`.
pub fn claim_score(save: &Save, d: i32, border_bonuses: i32, multiplier_off: usize) -> i32 {
    let den = constant(save, C_TERRITORY_DEN);
    let num = constant(save, C_TERRITORY_NUM);
    let mult = constant(save, multiplier_off);
    let divisor = border_bonuses * mult + num;
    if divisor == 0 {
        return i32::MAX;
    }
    (den * d * 0x100) / divisor
}

/// `CityData::get_level` 0x00739340 from the centre's type: `0x19f` → 2,
/// `0x1a0`/`0x213` → 3, else 1.
fn city_level_from_type(pt: i32) -> i32 {
    match pt {
        0x19f => 2,
        0x1a0 | 0x213 => 3,
        _ => 1,
    }
}

/// Type `is(what, 0)` through the exact index or the `is_list`
/// (`ObjectTypeData::is` 0x0065f7d0; non-strict, list-only — the
/// `is_slow` fallback is `build_process::type_is_slow`).
fn type_is(save: &Save, this: i32, what: i32) -> bool {
    if this == what {
        return true;
    }
    type_rec(save, this).map(|t| t.list(false).any(|v| v as i32 == what)).unwrap_or(false)
}

/// Build object of `(who, o)` if present.
fn build_at(save: &Save, who: usize, o: i32) -> Option<&Build> {
    match save.objects.lists.get(who)?.elems.get(usize::try_from(o).ok()?)? {
        Some(Obj::Build(b)) => Some(&**b),
        _ => None,
    }
}

/// Result of the per-cell contest (0x006b1100..0x006b16e4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CellClaim {
    /// `local_14`: winner (`-1` none, `-2` a city whose `race` differs from
    /// the slot it is filed under).
    who: i32,
    /// `local_24`: runner-up, same encoding.
    who2: i32,
    /// `local_1c`: index of the winning *city* in `who`'s list (`-1` when
    /// the winner is a fort or none).
    city: i32,
}

/// One WCoord cell `(wx, wy)` of region `reg`: the leader scan rotates with
/// `wx` (`slot = (k + wx) & 7`), cities then forts per slot, lower score
/// wins; ties keep the earlier claimant.
fn cell_claim(save: &Save, reg: usize, wx: i32, wy: i32, lt: &[Option<LeaderTerr>; 8]) -> CellClaim {
    let (_, _, limit_city) = limits(save, reg);
    let tx = wx * 4 + 2;
    let ty = wy * 4 + 2;
    let base_gate = constant(save, C_TERRITORY_BASE) << 8;
    let mut best = 999_999_999i32;
    let mut second = 999_999_999i32;
    let mut claim = CellClaim { who: -1, who2: -1, city: -1 };
    for k in 0..8 {
        let who = ((k + wx) & 7) as usize;
        if !leader_live(save, who) {
            continue;
        }
        let Some(me) = lt[who] else { continue };
        let city_mark = leader_i32(save, who, LD_CITY_MARK).unwrap_or(0).max(0) as usize;
        for ci in 0..city_mark {
            let Some(Some(c)) = save.cities.lists.get(who).and_then(|l| l.elems.get(ci)) else { continue };
            if c.flags & 1 == 0 || c.pod.len() != 108 {
                continue;
            }
            let cimg = Img(c);
            let d = hypot_approx(tx - fine_to_tile(cimg.i32(0x0c)), ty - fine_to_tile(cimg.i32(0x10)));
            let centre = build_at(save, who, cimg.i16(0x08) as i32);
            let level = centre.map(|b| city_level_from_type(Img(b).ptype())).unwrap_or(1);
            let has_temple = c.flags & 0x80 != 0;
            let temple_term = if has_temple { me.temple_terr * limit_city } else { 0 };
            if d > (level - 1) * limit_city + temple_term + me.city_cap {
                continue;
            }
            let d = compress(d);
            let race = cimg.i8(0x5f) as i32;
            let row = if race == who as i32 { Some(me) } else { lt.get(race as usize).copied().flatten() };
            let Some(row) = row else { continue };
            let mut b = row.civic_terr;
            if has_temple {
                b += row.temple_terr;
            }
            let city_term = if c.flags & 0x4000 != 0 || centre.map(|b| type_is(save, Img(b).ptype(), 0x213)).unwrap_or(false) {
                constant(save, C_CAPITAL_TERRITORY_BONUS)
            } else {
                constant(save, C_CITY_UPGRADE_TERR + (level - 1) as usize * 4)
            };
            let score = claim_score(save, d, b + city_term + row.bonus, C_CITY_TERRITORY_MULTIPLIER);
            let tagged = if race == who as i32 { who as i32 } else { -2 };
            if score > base_gate || score >= best {
                if score < second {
                    second = score;
                    claim.who2 = tagged;
                }
            } else {
                if claim.who != -1 {
                    second = best;
                    claim.who2 = claim.who;
                }
                best = score;
                claim.city = ci as i32;
                claim.who = tagged;
            }
        }
        let fort_mark = leader_i32(save, who, LD_FORT_MARK).unwrap_or(0).max(0) as usize;
        for fi in 0..fort_mark {
            let Some(Some(f)) = save.forts.lists.get(who).and_then(|l| l.elems.get(fi)) else { continue };
            if f.data.len() < 8 || f.data[6] & 1 == 0 {
                continue;
            }
            let o = i16::from_le_bytes([f.data[2], f.data[3]]) as i32;
            let Some(b) = build_at(save, who, o) else { continue };
            let bimg = Img(b);
            if !bimg.complete() {
                continue;
            }
            let d = hypot_approx(tx - fine_to_tile(bimg.x()), ty - fine_to_tile(bimg.y()));
            if d > me.fort_cap {
                continue;
            }
            let d = compress(d);
            let mut bb = me.fort_terr + me.civic_terr;
            if type_is(save, bimg.ptype(), 0x216) {
                bb += 4;
            }
            let score = claim_score(save, d, bb + me.bonus, C_FORT_TERRITORY_MULTIPLIER);
            if score > base_gate || score >= best {
                if score < second {
                    second = score;
                    claim.who2 = who as i32;
                }
            } else {
                second = best;
                best = score;
                claim.who2 = claim.who;
                claim.city = -1;
                claim.who = who as i32;
            }
        }
    }
    claim
}

/// `World::compute_reg_territory(int reg, int restart)` 0x006b0bb0.
/// Returns the retail result bits: `0` region already complete, `1` frame
/// budget exhausted (or incomplete), `2` region completed this call.
///
/// ```text
/// if (restart) Regions[reg].borders = 0;
/// if (Regions[reg].borders == 0) {                 // starting the region over
///   for who: if (leader.flags & 1) { reg_terr[who][reg] = 0; reg_known_rares[reg] = 0;
///                                    for each live city: city.bordering = 0; }
///   Regions[reg].flags &= ~0x80;
/// }
/// if (size <= borders) return 0;
/// if (GameDaemon.borders > 0xff) return 1;
/// <leader precompute>
/// while (borders < size) {
///   if (GameDaemon.borders >= 0x100) break;  GameDaemon.borders++;
///   (wx, wy) = Regions[reg].coords[borders++];
///   Borders::save_last_who(...)                     // presentation (0x008835e0)
///   <cell contest>  → WData.who / who2
///   if (who >= 0) {
///     if (city >= 0 && who2 >= 0 && who2 != who && !(diplos both ways == 2..))
///       city.bordering |= 1<<who | 1<<who2;  if (|dwx|+|dwy| < 5) city_flags |= 0x1000;
///     reg_terr[who][reg]++;  Regions[reg].flags |= 0x80;
///     <rare at this cell → Leader::new_rare 0x006d9e70 / rare.who |= bit>
///   }
/// }
/// if (size <= borders) { all leaders flags |= 0x2000000; Regions[reg].flags |= 0x80 | 0x20;
///                        Borders::add/remove_border_region (presentation) }
/// return (size <= borders) + 1;
/// ```
///
/// The cursor is not walked (module doc): a region with `flags & 0x80`
/// clear is started from cell 0 and, once started, run to completion
/// within the 256-cell budget held in `GameDaemon.borders`
/// (`post_world[0x24]`). When the leader precompute needs an unavailable
/// helper the body is skipped and nothing is written (returns `1`).
pub fn compute_reg_territory(save: &mut Save, reg: usize, restart: bool, effects: &mut Vec<String>) -> u8 {
    let size = region_size(save, reg);
    let flags = region_flags(save, reg);
    if !restart && flags & 0x80 != 0 {
        return 0;
    }
    if size <= 0 {
        return 0;
    }
    let budget = get_i32(&save.post_world, 0x24);
    if budget > 0xff {
        return 1;
    }
    let lim = limits(save, reg);
    let mut lt: [Option<LeaderTerr>; 8] = [None; 8];
    for who in 0..8 {
        if leader_live(save, who) {
            match leader_terr(save, who, lim) {
                Some(t) => lt[who] = Some(t),
                None => {
                    effects.push(format!(
                        "Region[{reg}]: compute_reg_territory skipped — leader[{who}] precompute needs has_preq/has_wonder/has_tribe_bonus (TODO 0x006db810/0x006ebc10/0x006e1370)"
                    ));
                    return 1;
                }
            }
        }
    }
    // Region restart: clear this region's counts and every city's `bordering`.
    for who in 0..8usize {
        if !leader_live(save, who) {
            continue;
        }
        let l = &mut save.leaders.slots[who];
        put_u16(&mut l.body, LD_REG_TERR - 8 + 2 * reg, 0);
        put_i32(&mut l.body, LD_REG_KNOWN_RARES - 8 + 4 * reg, 0);
        if let Some(list) = save.cities.lists.get_mut(who) {
            for c in list.elems.iter_mut().flatten() {
                if c.flags & 1 != 0 && c.pod.len() == 108 {
                    Img(&mut *c).set_u8(0x65, 0);
                }
            }
        }
    }
    set_region_flags(save, reg, flags & !0x80);
    let coords: Vec<(i32, i32)> =
        save.regions.elems[reg].coords.elems.iter().map(|r| (get_i32(&r.data, 0), get_i32(&r.data, 4))).collect();
    let xs = save.world.xs;
    let mut claimed = 0usize;
    let mut cursor = 0usize;
    let mut budget = budget;
    while cursor < coords.len() && (cursor as i32) < size {
        if budget >= 0x100 {
            break;
        }
        budget += 1;
        let (wx, wy) = coords[cursor];
        cursor += 1;
        let claim = cell_claim(save, reg, wx, wy, &lt);
        // TODO(va 0x008835e0) Borders::save_last_who — presentation.
        let idx = (wy * xs + wx) as usize * 21;
        if let Some(t) = save.world.wdata.get_mut(idx..idx + 21) {
            t[0xf] = claim.who as i8 as u8;
            t[0x10] = claim.who2 as i8 as u8;
        }
        if claim.who < 0 {
            continue;
        }
        let who = claim.who as usize;
        if claim.city >= 0 && claim.who2 >= 0 && claim.who2 != claim.who {
            let w2 = claim.who2 as usize;
            let d1 = leader_i32(save, who, LD_DIPLOS + 4 * w2).unwrap_or(0);
            let d2 = leader_i32(save, w2, LD_DIPLOS + 4 * who).unwrap_or(0);
            if d1 == 0 || d2 == 0 {
                if let Some(Some(c)) = save.cities.lists.get_mut(who).and_then(|l| l.elems.get_mut(claim.city as usize)) {
                    let mut ci = Img(&mut *c);
                    let b = ci.u8(0x65) | (1 << who) | (1 << w2);
                    ci.set_u8(0x65, b);
                    let dx = (wx - fine_to_wcoord(ci.i32(0x0c))).abs();
                    let dy = (wy - fine_to_wcoord(ci.i32(0x10))).abs();
                    if dx + dy < 5 {
                        c.flags |= 0x1000;
                    }
                }
            }
        }
        let l = &mut save.leaders.slots[who];
        let off = LD_REG_TERR - 8 + 2 * reg;
        let v = get_u16(&l.body, off);
        put_u16(&mut l.body, off, v.wrapping_add(1));
        let f = region_flags(save, reg);
        set_region_flags(save, reg, f | 0x80);
        claimed += 1;
        // TODO(va 0x006d9e70) rare-resource discovery: a live Rares entry at
        // this cell → Leader::new_rare(idx) / rare.who |= 1 << who (gated on
        // reveal_map == 3, leader flag 0x800, +0x59e4, or was_seen).
    }
    put_i32(&mut save.post_world, 0x24, budget);
    let done = cursor as i32 >= size;
    if done {
        for who in 0..8usize {
            if leader_live(save, who) {
                save.leaders.slots[who].flags |= 0x2000000;
            }
        }
        let f = region_flags(save, reg);
        set_region_flags(save, reg, f | 0x80 | 0x20);
        // TODO(va 0x008836d0/0x00883780) Borders::remove/add_border_region —
        // presentation (border_id +0x30 is not walked).
    }
    effects.push(format!("Region[{reg}]: territory cells {cursor}/{size} computed, {claimed} claimed, done={done}"));
    if done {
        2
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container;
    use crate::systems::img::frame;
    use crate::walk::Loader;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    fn capture_dir(name: &str) -> Option<PathBuf> {
        let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/live/frame-pairs").join(name);
        d.join("manifest.json").exists().then_some(d)
    }

    /// Load tolerating a failure in the trailing `verify_save` words (a
    /// sibling lane is mid-transcription there); everything before the
    /// trailer is fully walked.
    fn load_state(bytes: &[u8]) -> Save {
        let mut l = Loader::new(bytes);
        let mut s = Save::default();
        if let Err(e) = s.walk_data(&mut l) {
            assert_eq!(e.class, "SaveGame::verify_save", "{e:?}");
        }
        s
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

    fn pairs(name: &str) -> Vec<(i64, i64, Save, Save)> {
        let Some(dir) = capture_dir(name) else { return Vec::new() };
        let st = steps(&dir);
        let mut out = Vec::new();
        for k in 0..st.len() - 1 {
            let ra = container::load_svx(&dir.join(format!("{}.svx", st[k].1))).unwrap();
            let rb = container::load_svx(&dir.join(format!("{}.svx", st[k + 1].1))).unwrap();
            out.push((st[k].0, st[k + 1].0, load_state(&ra), load_state(&rb)));
        }
        out
    }

    #[test]
    fn hypot_matches_closed_form() {
        assert_eq!(hypot_approx(0, 0), 0);
        assert_eq!(hypot_approx(3, 0), 3);
        assert_eq!(hypot_approx(0, -7), 7);
        // mn²/(2mx) + mx: 3,4 → 9/8 + 4 = 5
        assert_eq!(hypot_approx(3, 4), 5);
        assert_eq!(hypot_approx(4, 3), 5);
        // 10,10 → 100/20 + 10 = 15
        assert_eq!(hypot_approx(10, 10), 15);
        // overflow guard: mn >= 60000 → (mn + 2mx) >> 1
        assert_eq!(hypot_approx(60000, 70000), (60000 + 140000) >> 1);
    }

    #[test]
    fn compress_ladder() {
        // each test is on the running value
        assert_eq!(compress(13), 13);
        assert_eq!(compress(12), compress_ref(12));
        for d in 0..20 {
            assert_eq!(compress(d), compress_ref(d), "d={d}");
        }
    }

    fn compress_ref(mut d: i32) -> i32 {
        if d < 13 {
            d = d * 2 / 3;
        }
        if d < 9 {
            d = d * 2 / 3;
        }
        if d < 5 {
            d /= 2;
        }
        d
    }

    /// Oracle for the totals tail: on every captured frame
    /// `territory == Σ_{size != 0} reg_terr` and `territory_high >= territory`
    /// for every live leader — exactly what `recompute_territory_totals`
    /// writes. Running it must change nothing.
    #[test]
    fn totals_tail_is_a_fixed_point_on_captures() {
        let mut checked = 0;
        for name in ["20261004-075733-stride1", "20261004-081342-stride15"] {
            for (fa, _, a, _) in pairs(name) {
                let mut ours = a.clone();
                let mut effects = Vec::new();
                recompute_territory_totals(&mut ours, &mut effects);
                for who in 0..8 {
                    assert_eq!(ours.leaders.slots[who].body, a.leaders.slots[who].body, "{name} f{fa} leader[{who}]");
                }
                assert!(effects.is_empty(), "{name} f{fa}: {effects:?}");
                checked += 1;
            }
        }
        if checked == 0 {
            eprintln!("no capture dir; skipping");
        }
    }

    /// Lane gate: run `check_ever_seen(0)` on the retail cadence and
    /// `check_borders` on frame N; every owned byte (Build ever_seen /
    /// ever_seen_completed, Leader treaties/territory/reg_terr, wdata
    /// who/who2, City bordering, Region flags) must equal retail N+1 —
    /// `introduced == 0`.
    #[test]
    fn territory_introduces_nothing() {
        let mut introduced = Vec::new();
        let mut pairs_n = 0;
        for name in ["20261004-075733-stride1", "20261004-081342-stride15"] {
            for (fa, fb, a, b) in pairs(name) {
                pairs_n += 1;
                let mut ours = a.clone();
                let mut effects = Vec::new();
                let fr = frame(&a);
                for owner in 0..ours.objects.lists.len() {
                    for slot in 0..ours.objects.lists[owner].elems.len() {
                        let Some(Obj::Build(bd)) = &ours.objects.lists[owner].elems[slot] else { continue };
                        let bd = &**bd;
                        if bd.base.sub.flags & 1 == 0 || !Img(bd).complete() {
                            continue;
                        }
                        let who = Img(bd).who() as i32;
                        if fr != 0 && (fr & 7) == who {
                            check_ever_seen(&mut ours, owner, slot, false, &mut effects);
                        }
                    }
                }
                check_borders(&mut ours, &mut effects);
                // Build ever_seen bytes.
                for owner in 0..a.objects.lists.len() {
                    for slot in 0..a.objects.lists[owner].elems.len().min(b.objects.lists[owner].elems.len()) {
                        let (Some(Obj::Build(xa)), Some(Obj::Build(xb)), Some(Obj::Build(xo))) =
                            (&a.objects.lists[owner].elems[slot], &b.objects.lists[owner].elems[slot], &ours.objects.lists[owner].elems[slot])
                        else {
                            continue;
                        };
                        if xa.wall_body.len() != 30 || xb.wall_body.len() != 30 {
                            continue;
                        }
                        for off in [0x62usize, 0x63] {
                            let (ra, rb, ro) = (xa.wall_body[off - 0x48], xb.wall_body[off - 0x48], xo.wall_body[off - 0x48]);
                            if ro != ra && ro != rb {
                                introduced.push(format!("{name} f{fa}->f{fb} [{owner}][{slot}] +{off:#x} retail {ra:#x}->{rb:#x} ours {ro:#x}"));
                            }
                        }
                    }
                }
                for who in 0..8 {
                    let (la, lb, lo) = (&a.leaders.slots[who].body, &b.leaders.slots[who].body, &ours.leaders.slots[who].body);
                    if la.len() != 0x6922 {
                        continue;
                    }
                    for (lo_off, len) in [(LD_TREATIES, 32), (LD_TERRITORY, 8), (LD_REG_TERR, 128), (LD_REG_KNOWN_RARES, 256)] {
                        let r = lo_off - 8..lo_off - 8 + len;
                        if lo[r.clone()] != la[r.clone()] && lo[r.clone()] != lb[r.clone()] {
                            introduced.push(format!("{name} f{fa}->f{fb} leader[{who}] +{lo_off:#x}"));
                        }
                    }
                }
                for (i, ((xa, xb), xo)) in a.world.wdata.iter().zip(b.world.wdata.iter()).zip(ours.world.wdata.iter()).enumerate() {
                    if (i % 21 == 0xf || i % 21 == 0x10) && xo != xa && xo != xb {
                        introduced.push(format!("{name} f{fa}->f{fb} wdata t{} +{:#x}", i / 21, i % 21));
                    }
                }
                let _ = effects;
            }
        }
        if pairs_n == 0 {
            eprintln!("no capture dir; skipping");
            return;
        }
        for r in introduced.iter().take(40) {
            eprintln!("  INTRODUCED {r}");
        }
        assert!(introduced.is_empty(), "{} introduced rows", introduced.len());
    }

    /// Diagnostic: per-field histogram of `World` byte changes between
    /// consecutive captures plus the territory words (`-- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn dump_world_diffs() {
        for name in ["20261004-075733-stride1", "20261004-081342-stride15"] {
            println!("== {name}");
            for (k, (fa, fb, a, b)) in pairs(name).into_iter().enumerate() {
                let mut hist: BTreeMap<usize, usize> = BTreeMap::new();
                let mut who_rows = Vec::new();
                for (i, (x, y)) in a.world.wdata.iter().zip(b.world.wdata.iter()).enumerate() {
                    if x != y {
                        *hist.entry(i % 21).or_default() += 1;
                        if i % 21 == 0xf || i % 21 == 0x10 {
                            who_rows.push(format!("t{}+{:#x}:{}->{}", i / 21, i % 21, *x as i8, *y as i8));
                        }
                    }
                }
                let planes = [
                    ("tdata", &a.world.tdata, &b.world.tdata),
                    ("seen", &a.world.seen, &b.world.seen),
                    ("seen2", &a.world.seen2, &b.world.seen2),
                    ("seen3", &a.world.seen3, &b.world.seen3),
                    ("wcoord_seen", &a.world.wcoord_seen, &b.world.wcoord_seen),
                    ("danger", &a.world.danger, &b.world.danger),
                ];
                let mut other = Vec::new();
                for (n, pa, pb) in planes {
                    let c = pa.iter().zip(pb.iter()).filter(|(x, y)| x != y).count();
                    if c > 0 {
                        other.push(format!("{n}:{c}"));
                    }
                }
                if k == 0 {
                    for (i, l) in a.leaders.slots.iter().enumerate() {
                        if l.flags & 1 == 0 || l.body.len() != 0x6922 {
                            continue;
                        }
                        let rd = |off: usize| get_i32(&l.body, off - 8);
                        let reg_terr: Vec<(usize, u16)> =
                            (0..64).map(|r| (r, get_u16(&l.body, LD_REG_TERR - 8 + 2 * r))).filter(|(_, v)| *v != 0).collect();
                        println!(
                            "  leader[{i}] flags={:#x} territory={} high={} other={} min_other={} my_team={} ally_mask={:#x} reg_terr={:?}",
                            l.flags,
                            rd(0x9d8),
                            rd(0x9dc),
                            rd(0x9e4),
                            rd(0x9e8),
                            rd(0x9ec),
                            l.body[0x6929 - 8],
                            reg_terr
                        );
                    }
                    for (r, reg) in a.regions.elems.iter().enumerate().take(64) {
                        if get_i32(&reg.head, REG_SIZE) != 0 {
                            println!("  region[{r}] flags={:#x} size={} coords={}", get_i32(&reg.head, REG_FLAGS), get_i32(&reg.head, REG_SIZE), reg.coords.elems.len());
                        }
                    }
                }
                for (i, (la, lb)) in a.leaders.slots.iter().zip(b.leaders.slots.iter()).enumerate() {
                    if la.body.len() == 0x6922 && lb.body.len() == 0x6922 {
                        let r = 0x9d8 - 8..0x9f0 - 8;
                        if la.body[r.clone()] != lb.body[r.clone()] {
                            println!("  leader[{i}] +0x9d8.. {:?} -> {:?}", &la.body[r.clone()], &lb.body[r]);
                        }
                        let r = LD_REG_TERR - 8..LD_REG_TERR - 8 + 128;
                        if la.body[r.clone()] != lb.body[r] {
                            println!("  leader[{i}] reg_terr changed");
                        }
                    }
                }
                println!(
                    "f{fa}->f{fb} wdata {:?} {} | who rows: {}",
                    hist,
                    other.join(" "),
                    who_rows.iter().take(12).cloned().collect::<Vec<_>>().join(" ")
                );
            }
        }
    }
}
