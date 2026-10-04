//! City per-frame logic reached from the city-attached block of
//! `Build::process` 0x0061edf0 (0x0061fb6a..0x00620167), plus the two
//! callees it owns: `Build::check_capture` 0x006276a0 and `City::assimilate`
//! 0x00738e90. Leaf module: free functions the Build traversal
//! (`build_process::build_body`) calls for a centre object with
//! `SubObject.flags & 0x20` and `BuildData::city (+0x72) >= 0`; never edits
//! sibling modules.
//!
//! There is no `City::process` in the PDB — every City timer is written
//! from here (or from capture/plunder/caravan code outside this lane).
//!
//! # Retail body (in order; `c` = `Cities.lists[who][city]`, `o` = this)
//!
//! ```text
//! if ((frame + o) % 200 == 0) {
//!   c.city_flags = (c.city_flags & 4) ? c.city_flags & ~4 : c.city_flags & ~2;
//!   if (c.plundered) c.plundered -= 1;                                     // +0x61
//! }
//! if ((c.city_flags & 0x10) && Game.team_style == 2)
//!   for u in 0..8: if ((leader[u].flags & 3) == 3 && leader[u].get_target() == who
//!                      && !(ever_seen & 1<<u)) { ever_seen |= 1<<u; update_local_seen(); break; }
//! if ((frame + o) % 16 == 0 && who == local player && ...)  <advisory, UI + leader +0x6ebc>
//! if ((frame + o) % 64 == 0) {
//!   u = -1;
//!   if (check_capture_eligible()) {
//!     u = ObjectsData::find_unit(x, y, 3, who, 0xc00, 0, 0xd, 0,0,0,0,0, y);     // 0x0065ca80
//!     if (u >= 0 && Search.found_who >= 0 && check_capture(u, found_who)) return; }
//!   if (!(leader[who].flags & 4) && u < 0 && can_capture() && hits()/2 < damage
//!       && !(c.city_flags & 2) && get_diff() > 1)
//!     { b = find_unit(x, y, 1, who, 0x1e00, 0x200, 1, 0x32, 0, 0xc, 0,0, y); if (b >= 0) Unit::add_build_order(o, who, 2, 0); }
//! }
//! if (c.race != who) {
//!   if (has_general(0, 0x164) >= 0) c.assimilation_timer += 1 - Constants.ASSIM_GENERAL;  // +0xc9c
//!   t = frame - c.assimilation_timer;
//!   if (has_tribe_bonus(8))  t = (Constants+0x664 + 100) * t / 100;
//!   if (who == c.founder)    t = (Constants+0xd2c + 100) * t / 100;
//!   if (Constants+0xd30 <= t) c.assimilate();
//! }
//! if (!(c.city_flags & 2)) {
//!   if (damage && c.race == who && (frame + o) % Constants+0xc10 == 0) repair_damage(get_level(), 0, 1);
//!   if (c.city_flags & 0x40) <AI/human caravan muster: period 60 (AI) or 200 (human with option 0x20);
//!                             (frame + 100 + o) % period == 0 → ObjectsData::find / Group alarm>
//! }
//! ```
//!
//! # Oracle
//!
//! On the stride-15 captures (`f40..f325`) every city has `race == who ==
//! founder`, `city_flags ∈ {0x4011, 0x4411, 0x4811}` (bits 2/4 clear),
//! `plundered == 0`, no damage, no enemy unit near any centre. The City
//! bytes retail *does* move there are the gather census (`free` +0x5a,
//! `busy` +0x5b, `gatherers` +0x5c, `peasant_dist` +0x50, `filled` +0x64,
//! `space[2]` +0x6b) — written by `Leader::calc_gather` → `City::calc_gather`
//! 0x00737c60 / `count_gather_slots` 0x00737dc0, not by this block. So the
//! gate here is `introduced == 0` for the fields this module owns
//! ([`tests::city_block_introduces_nothing`]).
//!
//! # RNG
//!
//! Zero `Random::get` 0x00a39d70 draws on any path: not in this block, not
//! in `check_capture_eligible` 0x0062d1d0, `check_capture` 0x006276a0,
//! `Cities::capture_city` 0x00733380, `City::assimilate` 0x00738e90,
//! `find_unit` 0x0065ca80 (→ `Search` 0x0067daa0/0x0067dbb0),
//! `Build::train` 0x0062f9b0 or `Objects::init_unit` 0x0065e0c0 (checked
//! by grepping the decompiled bodies for `FUN_00a39d70`). The earlier
//! `build_process` note attributing draws to `check_capture`/`assimilate`
//! was wrong.

#![allow(dead_code)]

use super::img::{constant, frame, get_i32, leader_flags, type_rec, Img};
use crate::sections::{Build, City, Obj, Save};
use crate::tick::StepStatus;

/// Transcribed: the 200-frame flag phase and `plundered` decay, the
/// team-style ever-seen bit, `check_capture_eligible`, every admission gate
/// of `check_capture` that reads walked state, the assimilation timer
/// arithmetic and `City::assimilate`'s own writes. Not transcribed (fields
/// untouched, `effects` notes): `find_unit`, the capture census / transfer,
/// `has_general`'s spatial search, library/senate side effects of
/// assimilate, `repair_damage`'s `hits()` queue arm, the caravan muster.
pub const STATUS: StepStatus = StepStatus::Partial;

// ---------------------------------------------------------------------------
// Offsets
// ---------------------------------------------------------------------------

/// `CityData` (PDB).
const CD_O: usize = 0x08;
const CD_X: usize = 0x0c;
const CD_Y: usize = 0x10;
const CD_CAPTURE_STAMP: usize = 0x20;
const CD_ASSIMILATION_TIMER: usize = 0x24;
const CD_CAPTURE_STRENGTH: usize = 0x28;
const CD_WHO: usize = 0x5e;
const CD_RACE: usize = 0x5f;
const CD_FOUNDER: usize = 0x60;
const CD_PLUNDERED: usize = 0x61;

/// `BuildData`/`WallData`/`ObjectData` (PDB).
const BD_DAMAGE: usize = 0x24;
const BD_INSIDE_DOWN: usize = 0x28;
const BD_CONSTRUCT_HITS: usize = 0x54;
const BD_BUILD_MASKS: usize = 0x60;
const BD_EVER_SEEN: usize = 0x62;
const BD_CITY: usize = 0x72;
const BD_CITY_DOWN: usize = 0x74;
const BD_QUEUED: usize = 0x82;

/// `UnitData`.
const UD_UNIT_MASKS: usize = 0x68;

/// `LeaderData`.
const LD_WHO: usize = 0x08;
const LD_DIPLOS: usize = 0x74;
const LD_START_LIST: usize = 0x65c - 0x550; // Game.scalars: start_list[8]
const LD_START_INDEX: usize = 0x67c - 0x550; // Game.scalars: start_index[8]
const LD_NUM_UNITS: usize = 0x5762; // u16[352], unit type t at +0x56fe + 2t

/// `Constants`.
const C_UNIT_RESPOND_RANGE: usize = 0x018;
const C_CITY_CAPTURE_RADIUS: usize = 0x134;
const C_ASSIM_TRIBE8_PCT: usize = 0x664;
const C_CITY_REPAIR_PERIOD: usize = 0xc10;
const C_ASSIM_GENERAL: usize = 0xc9c;
const C_ASSIM_FOUNDER_PCT: usize = 0xd2c;
const C_ASSIM_FRAMES: usize = 0xd30;
const C_ASSIM_TRIBE_E_UPGRADE: usize = 0x788;

/// `GameInfo` single-byte settings (`Game+0x24+i`).
const GI_TEAM_STYLE: usize = 0;

fn game_setting(save: &Save, i: usize) -> u8 {
    save.game.info.settings.get(i).copied().unwrap_or(0)
}

fn leader_live(save: &Save, who: usize) -> bool {
    save.leaders.slots.get(who).map(|l| l.flags & 1 != 0 && l.body.len() == 0x6922).unwrap_or(false)
}

fn leader_i32(save: &Save, who: usize, off: usize) -> Option<i32> {
    super::img::leader_i32(save, who, off)
}

fn leader_i16(save: &Save, who: usize, off: usize) -> Option<i16> {
    super::img::leader_i16(save, who, off)
}

/// `LeaderData::is_ally(who)` 0x006edb50: self, or `diplos == 2` both ways.
fn is_ally(save: &Save, me: usize, other: usize) -> bool {
    if me == other {
        return true;
    }
    leader_i32(save, me, LD_DIPLOS + 4 * other) == Some(2) && leader_i32(save, other, LD_DIPLOS + 4 * me) == Some(2)
}

/// `LeaderData::is_enemy(who)` 0x006ebaa0: not self and `diplos == 0`
/// either way.
fn is_enemy(save: &Save, me: usize, other: usize) -> bool {
    if me == other {
        return false;
    }
    leader_i32(save, me, LD_DIPLOS + 4 * other) == Some(0) || leader_i32(save, other, LD_DIPLOS + 4 * me) == Some(0)
}

/// `ObjectTypeData::is(what, 0)` through the exact index or `is_list`
/// (`is_slow` fallback lives in `build_process`).
fn type_is(save: &Save, this: i32, what: i32) -> bool {
    if this == what {
        return true;
    }
    type_rec(save, this).map(|t| t.list(false).any(|v| v as i32 == what)).unwrap_or(false)
}

fn build_ref(save: &Save, who: usize, o: usize) -> Option<&Build> {
    match save.objects.lists.get(who)?.elems.get(o)? {
        Some(Obj::Build(b)) if Img(&**b).complete() => Some(&**b),
        _ => None,
    }
}

fn city_ref(save: &Save, who: usize, city: usize) -> Option<&City> {
    save.cities.lists.get(who)?.elems.get(city)?.as_ref().filter(|c| c.flags & 1 != 0 && c.pod.len() == 108)
}

fn city_mut(save: &mut Save, who: usize, city: usize) -> Option<&mut City> {
    save.cities.lists.get_mut(who)?.elems.get_mut(city)?.as_mut().filter(|c| c.flags & 1 != 0 && c.pod.len() == 108)
}

/// `BuildData::hits(0)` 0x0062e740: `construct_hits` (+0x54); when active
/// with a queued `0x29a`/`0x286` upgrade the value is scaled by the
/// upgrade's progress through `ObjectData::construct_time` 0x006508c0 —
/// TODO(va 0x006508c0): that arm returns `None`.
fn build_hits(save: &Save, b: &Build) -> Option<i32> {
    let img = Img(b);
    let hits = img.i32(BD_CONSTRUCT_HITS);
    if img.flags() & 4 != 0 && img.u8(BD_QUEUED) != 0 {
        if let Some(row) = img.queue_row(0) {
            let ty = i16::from_le_bytes([row[4], row[5]]) as i32;
            if ty == 0x29a || ty == 0x286 {
                let _ = save;
                return None;
            }
        }
    }
    Some(hits)
}

/// `ObjectData::health_level()` 0x006534b0 for a Build: `hits = hits(0)`,
/// `cur = clamp(hits - damage, 0, hits)`; 0 ≥90%, 1 ≥75%, 2 ≥50%, 3 ≥25%,
/// 4 ≥10%, 5 >0, 6 = 0.
fn health_level(save: &Save, b: &Build) -> Option<i32> {
    let hits = build_hits(save, b)?;
    let mut cur = hits - Img(b).i32(BD_DAMAGE);
    if cur < 0 || hits < 0 {
        cur = 0;
    } else if hits < cur {
        cur = hits;
    }
    Some(if (hits * 0x5a) / 100 <= cur {
        0
    } else if (hits * 0x4b) / 100 <= cur {
        1
    } else if cur >= (hits * 0x32) / 100 {
        2
    } else if (hits * 0x19) / 100 <= cur {
        3
    } else if cur >= (hits * 10) / 100 {
        4
    } else if cur < 1 {
        6
    } else {
        5
    })
}

/// `BuildData::check_capture_eligible()` 0x0062d1d0: `ptype->is(0x19e)`
/// (city centre family, `BuildTypeData::is_city` vtable arm), `is_active()`,
/// and `health_level() >= 6` for a city-centre-flagged object
/// (`flags & 0x20`) or `>= 5` otherwise. `None` when `hits()` is not
/// evaluable.
pub fn check_capture_eligible(save: &Save, b: &Build) -> Option<bool> {
    let img = Img(b);
    if !type_is(save, img.ptype(), 0x19e) {
        return Some(false);
    }
    if img.flags() & 4 == 0 {
        return Some(false);
    }
    let hl = health_level(save, b)?;
    Some(if img.flags() & 0x20 != 0 { hl > 5 } else { hl > 4 })
}

// ---------------------------------------------------------------------------
// Build::process city block (0x0061fb6a..0x00620167)
// ---------------------------------------------------------------------------

/// The city-attached block of `Build::process` for the centre at
/// `Objects.lists[owner][slot]`. Returns `true` when retail returns from
/// `Build::process` early (`check_capture` yielded non-zero), so the caller
/// must skip the trailing `build_masks & 0x100` road pass.
pub fn process_city_block(save: &mut Save, owner: usize, slot: usize, effects: &mut Vec<String>) -> bool {
    let Some(b) = build_ref(save, owner, slot) else { return false };
    let img = Img(b);
    if img.flags() & 0x20 == 0 || img.i16(BD_CITY) < 0 {
        return false;
    }
    let who = img.who() as usize;
    let o = img.o();
    let city = img.i16(BD_CITY) as usize;
    let (x, y, damage) = (img.x(), img.y(), img.i32(BD_DAMAGE));
    let fr = frame(save);
    let tag = format!("Objects.lists[{owner}][{slot}] City[{who}][{city}]");
    if city_ref(save, who, city).is_none() {
        return false;
    }

    // (frame + o) % 200 == 0: flag phase + plundered decay.
    if (fr + o) % 200 == 0 {
        let c = city_mut(save, who, city).unwrap();
        let f = c.flags;
        let nf = if f & 4 == 0 { f & !2 } else { f & !4 };
        if nf != f {
            c.flags = nf;
            effects.push(format!("{tag}.city_flags {f:#x} -> {nf:#x} (200-frame phase)"));
        }
        let p = Img(&*c).u8(CD_PLUNDERED);
        if p != 0 {
            Img(&mut *c).set_u8(CD_PLUNDERED, p - 1);
            effects.push(format!("{tag}.plundered {p} -> {}", p - 1));
        }
    }

    // city_flags & 0x10 && team_style == 2: reveal the centre to the team
    // member whose start target is this player.
    if city_ref(save, who, city).unwrap().flags & 0x10 != 0 && game_setting(save, GI_TEAM_STYLE) == 2 {
        let ever_seen = Img(build_ref(save, owner, slot).unwrap()).u8(BD_EVER_SEEN);
        for u in 0..8usize {
            if leader_flags(save, u) & 3 != 3 {
                continue;
            }
            if get_target(save, u) != Some(who as i32) {
                continue;
            }
            if ever_seen & (1 << u) != 0 {
                continue;
            }
            if let Some(Some(Obj::Build(bb))) = save.objects.lists[owner].elems.get_mut(slot) {
                Img(&mut **bb).set_u8(BD_EVER_SEEN, ever_seen | (1 << u));
            }
            effects.push(format!("{tag}.Wall.ever_seen {ever_seen:#x} -> {:#x} (team_style 2, leader {u})", ever_seen | (1 << u)));
            // TODO(va 0x0063ed50) Wall::update_local_seen — fog planes.
            break;
        }
    }

    // (frame + o) % 16 == 0 human advisory: gated on who == local player
    // ([0x00c06210]+0x298) and writes only leader +0x6ebc (outside the walked
    // body) plus UI. No sim state.

    // (frame + o) % 64 == 0: capture attempt / AI repair order.
    if (fr + o) % 64 == 0 {
        let b = build_ref(save, owner, slot).unwrap();
        let mut found: i32 = -1;
        match check_capture_eligible(save, b) {
            Some(true) => {
                // TODO(va 0x0065ca80) ObjectsData::find_unit(x, y, 3, who, 0xc00,
                // 0, 0xd, 0, 0, 0, 0, 0, y): nearest enemy unit able to capture
                // within 0xc00 fine units; `Search.found_who` ([0x00c0618c]+0x200)
                // gives its owner. Not evaluable here (Search machinery); the
                // capture is only attempted when a caller supplies the unit via
                // [`check_capture`]. `found` stays -1.
                effects.push(format!("{tag}: capture-eligible; find_unit (0x0065ca80) untranscribed, no attempt"));
                found = -1;
            }
            Some(false) => {}
            None => effects.push(format!("{tag}: check_capture_eligible needs hits() upgrade arm (0x006508c0)")),
        }
        if leader_flags(save, who) & 4 == 0 && found < 0 {
            // AI: `WallData::can_capture()` 0x00471360 (= ptype->is(0x19e)),
            // hits()/2 < damage, !(city_flags & 2), LeaderData::get_diff() > 1
            // → find_unit(...) → Unit::add_build_order(o, who, 2, 0) 0x005e4ff0.
            // Writes a unit's order list, not this object or the City; TODO.
            let b = build_ref(save, owner, slot).unwrap();
            if type_is(save, Img(b).ptype(), 0x19e) {
                if let Some(h) = build_hits(save, b) {
                    if h / 2 < damage && city_ref(save, who, city).unwrap().flags & 2 == 0 {
                        effects.push(format!("{tag}: AI repair-order path (get_diff 0x006ec000 / find_unit / add_build_order 0x005e4ff0) untranscribed"));
                    }
                }
            }
        }
    }

    // Foreign-race assimilation.
    let race = Img(city_ref(save, who, city).unwrap()).i8(CD_RACE) as i32;
    if race != who as i32 {
        // has_general(0, 0x164) 0x00646b00: for a Build, -1 when the owner
        // has no units of type 0x164 (num_units[0x164 - 50]); otherwise a
        // spatial search (FUN_0073a1b0) over the footprint — TODO(va
        // 0x0073a1b0): treated as "none" only when the count is 0.
        let n164 = leader_i16(save, who, LD_NUM_UNITS - 100 + 2 * 0x164).unwrap_or(0);
        if n164 != 0 {
            effects.push(format!("{tag}: has_general(0, 0x164) spatial arm (0x0073a1b0) untranscribed; assimilation_timer not advanced"));
        }
        let c = city_ref(save, who, city).unwrap();
        let ci = Img(c);
        let mut t = fr - ci.i32(CD_ASSIMILATION_TIMER);
        // TODO(va 0x006e1370) has_tribe_bonus(8): t = (C+0x664 + 100) * t / 100.
        if who as i32 == ci.i8(CD_FOUNDER) as i32 {
            t = ((constant(save, C_ASSIM_FOUNDER_PCT) + 100) * t) / 100;
        }
        if constant(save, C_ASSIM_FRAMES) <= t {
            assimilate(save, who, city, effects);
        }
    }

    // !(city_flags & 2): repair + caravan muster.
    let c = city_ref(save, who, city).unwrap();
    if c.flags & 2 == 0 {
        let race = Img(c).i8(CD_RACE) as i32;
        let period = constant(save, C_CITY_REPAIR_PERIOD);
        if damage != 0 && race == who as i32 && period != 0 && (fr + o) % period == 0 {
            // Build::repair_damage(get_level(), 0, 1) 0x00628130: damage =
            // min(damage, hits()); damage_frac = 0; damage -= level (floor 0).
            let b = build_ref(save, owner, slot).unwrap();
            let level = city_level(save, who, city).unwrap_or(1);
            match build_hits(save, b) {
                Some(h) => {
                    let mut d = damage.min(h);
                    d = if d <= level { 0 } else { d - level };
                    if let Some(Some(Obj::Build(bb))) = save.objects.lists[owner].elems.get_mut(slot) {
                        let mut w = Img(&mut **bb);
                        w.set_i32(BD_DAMAGE, d);
                        w.set_u8(0x3b, 0);
                    }
                    effects.push(format!("{tag}.Object.damage {damage} -> {d} (repair_damage level {level})"));
                }
                None => effects.push(format!("{tag}: repair_damage needs hits() upgrade arm (0x006508c0)")),
            }
        }
        if city_ref(save, who, city).unwrap().flags & 0x40 != 0 {
            // TODO(va 0x00738410, 0x0065c6b0, 0x00713e80, 0x00714350, 0x0070ec30)
            // caravan muster: period 60 for AI (leader flags & 4 clear), 200
            // for a human with option byte 0x20 at [0x00c061b4]+who*0x20+0x1c;
            // (frame + 100 + o) % period == 0 → ObjectsData::find(x, y, 6, who,
            // get_radius()*0xc0, o, 0x11, who, …) < 0 → Group::clear ×2,
            // Group::add(o, who, 0, 0), Group::action_alarm. Writes Groups.
            let _ = (x, y);
        }
    }
    false
}

/// `LeaderData::get_target()` 0x006da000: the first live-and-started
/// leader (`flags & 3 == 3`) in `Game.start_list` scanning from
/// `start_index[who] + 1`, wrapping mod 8; falls back to `who` after 8
/// misses.
fn get_target(save: &Save, who: usize) -> Option<i32> {
    let sc = &save.game.scalars;
    if sc.len() < LD_START_INDEX + 32 {
        return None;
    }
    let start = get_i32(sc, LD_START_INDEX + 4 * who);
    let mut k = 0;
    loop {
        let i = ((start + 1 + k) & 7) as usize;
        let cand = get_i32(sc, LD_START_LIST + 4 * i);
        if (0..8).contains(&cand) && leader_flags(save, cand as usize) & 3 == 3 {
            return Some(cand);
        }
        k += 1;
        if k >= 8 {
            return Some(who as i32);
        }
    }
}

/// `CityData::get_level()` 0x00739340 from the centre type: `0x19f` → 2,
/// `0x1a0`/`0x213` → 3, else 1.
fn city_level(save: &Save, who: usize, city: usize) -> Option<i32> {
    let c = city_ref(save, who, city)?;
    let o = Img(c).i16(CD_O);
    let b = build_ref(save, who, usize::try_from(o).ok()?)?;
    Some(match Img(b).ptype() {
        0x19f => 2,
        0x1a0 | 0x213 => 3,
        _ => 1,
    })
}

// ---------------------------------------------------------------------------
// City::assimilate 0x00738e90
// ---------------------------------------------------------------------------

/// `City::assimilate()` 0x00738e90:
///
/// ```text
/// city_flags &= ~0x100;
/// if (race != who) {
///   race = who;
///   if (WData(tile of x,y).region < 0x40) Region::fix_borders();        // 0x00680f60: all 64 Region.borders = 0
///   if (has_tribe_bonus(0xe) && Constants+0x788) centre->set_type(0x19f, 0); // 0x00640da0
///   leader[who].flags |= 0x2000000;
///   for each building b in the city chain (centre, then city_down +0x74 links):
///     if (b.is_active() && b.is(0x1b3)) { b.flags &= ~4; Build::new_library(); b.flags |= 4; }   // 0x00627fe0
///     if (b.is_active() && b.is(0x1b6) && leader +0xa48 >= 0 && find_unit(...) < 0
///         && leader +0xa48 <= frame && get_gov_hero(..) >= 0) b.train(hero);   // 0x0062f9b0
///   if (who == local player) <UI message>
/// }
/// ```
///
/// Transcribed writes: `city_flags`, `race`, `leader.flags`, and the
/// `Region.flags & ~0x80` proxy for `fix_borders` (the cursors themselves
/// are not walked — see `territory.rs`). The centre re-type, library reset
/// and senate hero train write other objects and are left as TODO.
pub fn assimilate(save: &mut Save, who: usize, city: usize, effects: &mut Vec<String>) {
    let Some(c) = city_mut(save, who, city) else { return };
    let tag = format!("City[{who}][{city}]");
    let f = c.flags;
    c.flags &= !0x100;
    if f & 0x100 != 0 {
        effects.push(format!("{tag}.city_flags {f:#x} -> {:#x} (assimilate)", f & !0x100));
    }
    let race = Img(&*c).i8(CD_RACE) as i32;
    let cwho = Img(&*c).i8(CD_WHO) as i32;
    if race == cwho {
        return;
    }
    Img(&mut *c).set_i8(CD_RACE, cwho as i8);
    effects.push(format!("{tag}.race {race} -> {cwho}"));
    let (cx, cy) = (Img(&*c).i32(CD_X), Img(&*c).i32(CD_Y));
    // WData.region (+4) of the centre's WCoord cell < 0x40 → fix_borders.
    let xs = save.world.xs;
    let (wx, wy) = ((cx >> 8) / 3, (cy >> 8) / 3);
    let region = save.world.wdata.get(((wy * xs + wx) as usize) * 21 + 4..).map(|t| i16::from_le_bytes([t[0], t[1]])).unwrap_or(-1);
    if region < 0x40 {
        // Region::fix_borders 0x00680f60: all 64 cursors = 0 (unwalked);
        // proxy = clear the "complete" bit so check_borders recomputes.
        let mut n = 0;
        for r in save.regions.elems.iter_mut().take(64) {
            if r.head.len() >= 44 {
                let fl = get_i32(&r.head, 8);
                if fl & 0x80 != 0 {
                    n += 1;
                }
                r.head[8..12].copy_from_slice(&(fl & !0x80).to_le_bytes());
            }
        }
        effects.push(format!("{tag}: Region::fix_borders — {n} regions marked for territory recompute"));
    }
    // TODO(va 0x006e1370, 0x00640da0) has_tribe_bonus(0xe) && Constants+0x788:
    // centre set_type(0x19f, 0).
    let _ = C_ASSIM_TRIBE_E_UPGRADE;
    if let Some(l) = save.leaders.slots.get_mut(cwho as usize) {
        if l.flags & 0x2000000 == 0 {
            l.flags |= 0x2000000;
            effects.push(format!("Leader[{cwho}].flags |= 0x2000000 (assimilate)"));
        }
    }
    // TODO(va 0x00627fe0, 0x0062f9b0) library reset / senate hero train over
    // the city_down (+0x74) chain. Local-player UI message skipped.
    let _ = BD_CITY_DOWN;
}

// ---------------------------------------------------------------------------
// Build::check_capture 0x006276a0
// ---------------------------------------------------------------------------

/// `Build::check_capture(int o, int who)` 0x006276a0 — admission gates.
/// `o`/`attacker` is the triggering unit (`Objects.lists[attacker][o]`).
/// Returns `Some(0)` when a gate rejects (retail returns 0 with the one
/// side effect transcribed: `inside_down >= 0` sets `build_masks |= 0x4000`),
/// `None` when the attempt passes every gate and reaches the untranscribed
/// census (`docs/mechanics/build-check-capture.md` — circle ring over
/// `WData::down`/`down_who` chains, `get_capture_value`, `Search::valid_filter`,
/// `Cities::capture_city` 0x00733380 / `Build::swap_team`). Nothing is
/// written in that case.
///
/// Gate order (0x006276b7..0x00627829):
/// 1. `check_capture_eligible()`;
/// 2. `inside_down >= 0` → `build_masks |= 0x4000`, return 0;
/// 3. reject when `attacker == leader[victim].who` or mutual `diplos == 2`,
///    while `leader[victim].flags & 2`; require `leader[attacker].flags & 2`;
/// 4. attacker type `+0x218 == 0` (land), `is_unit()`, `!(unit_masks & 1)`,
///    `attack() != 0`, `!(type.obj_masks(+0x1e4) & 0x08000000)`;
/// 5. `frame - city.capture_stamp > 0x4a`.
pub fn check_capture(save: &mut Save, owner: usize, slot: usize, o: usize, attacker: usize, effects: &mut Vec<String>) -> Option<i32> {
    let b = build_ref(save, owner, slot)?;
    let img = Img(b);
    let who = img.who() as usize;
    let tag = format!("Objects.lists[{owner}][{slot}]");
    if check_capture_eligible(save, b)? == false {
        return Some(0);
    }
    if img.i16(BD_INSIDE_DOWN) >= 0 {
        let bm = img.build_masks();
        if let Some(Some(Obj::Build(bb))) = save.objects.lists[owner].elems.get_mut(slot) {
            Img(&mut **bb).set_build_masks(bm | 0x4000);
        }
        effects.push(format!("{tag}.Wall.build_masks {bm:#x} -> {:#x} (check_capture: inside_down)", bm | 0x4000));
        return Some(0);
    }
    let victim_who = leader_i32(save, who, LD_WHO).unwrap_or(who as i32) as usize;
    let same_side = attacker == victim_who || is_ally(save, who, attacker);
    if same_side && leader_flags(save, who) & 2 != 0 {
        return Some(0);
    }
    if leader_flags(save, attacker) & 2 == 0 {
        return Some(0);
    }
    let Some(Some(Obj::Unit(u))) = save.objects.lists.get(attacker).and_then(|l| l.elems.get(o)) else { return Some(0) };
    let ui = Img(&**u);
    if !ui.complete() {
        return Some(0);
    }
    let ut = type_rec(save, ui.ptype())?;
    if ut.i32(0x218)? != 0 {
        return Some(0);
    }
    if ui.u32(UD_UNIT_MASKS) & 1 != 0 {
        return Some(0);
    }
    // UnitData::attack() 0x006103c0 is `ObjectData::base_attack` 0x006469f0
    // (type +0x1e8 with tech/tribe adds) plus additive bonuses; it is zero
    // iff the type's base attack is zero.
    if ut.i32(0x1e8)? == 0 {
        return Some(0);
    }
    if ut.u32(0x1e4)? & 0x0800_0000 != 0 {
        return Some(0);
    }
    let city = img.i16(BD_CITY);
    let c = city_ref(save, who, usize::try_from(city).ok()?)?;
    let since = frame(save) - Img(c).i32(CD_CAPTURE_STAMP);
    if since <= 0x4a {
        return Some(0);
    }
    let radius_rule = if img.flags() & 0x20 != 0 { constant(save, C_CITY_CAPTURE_RADIUS) } else { constant(save, C_UNIT_RESPOND_RANGE) };
    let defender_seed = if since < 900 { Img(c).i32(CD_CAPTURE_STRENGTH) } else { 2 };
    // TODO(va 0x006278a3..0x00627fd5) census over circle ring
    // (radius_rule*0xc0 + 0x2ff)/0x300 and the capture transfer.
    effects.push(format!(
        "{tag}: check_capture({o}, {attacker}) passed admission (radius {radius_rule} tiles, defender seed {defender_seed}); census/transfer untranscribed"
    ));
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container;
    use crate::walk::Loader;
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
    fn health_level_thresholds() {
        // hits 100 via construct_hits, damage d → cur = 100 - d.
        let mut b = Build::default();
        b.base.sub.body = vec![0; 19];
        b.base.mid = vec![0; 34];
        b.wall_body = vec![0; 30];
        b.body = vec![0; 22];
        b.head = vec![0; 2];
        Img(&mut b).set_i32(BD_CONSTRUCT_HITS, 100);
        let s = Save::default();
        for (d, want) in [(0, 0), (10, 0), (11, 1), (25, 1), (26, 2), (50, 2), (51, 3), (75, 3), (76, 4), (90, 4), (91, 5), (99, 5), (100, 6), (150, 6)] {
            Img(&mut b).set_i32(BD_DAMAGE, d);
            assert_eq!(health_level(&s, &b), Some(want), "damage {d}");
        }
    }

    /// Lane gate: run `process_city_block` on every live centre of retail
    /// frame N and account every City byte, the Build `ever_seen`/`damage`
    /// bytes and Leader flags against retail N+1 / N+15 — `introduced`
    /// (we changed, retail kept) must be 0.
    #[test]
    fn city_block_introduces_nothing() {
        let mut introduced = Vec::new();
        let mut pairs_n = 0;
        let mut ran = 0;
        for name in ["20261004-075733-stride1", "20261004-081342-stride15"] {
            for (fa, fb, a, b) in pairs(name) {
                pairs_n += 1;
                let mut ours = a.clone();
                let mut effects = Vec::new();
                for owner in 0..ours.objects.lists.len() {
                    for slot in 0..ours.objects.lists[owner].elems.len() {
                        let Some(Obj::Build(bd)) = &ours.objects.lists[owner].elems[slot] else { continue };
                        let bd = &**bd;
                        if bd.base.sub.flags & 1 == 0 || bd.base.sub.flags & 4 == 0 || !Img(bd).complete() {
                            continue;
                        }
                        if bd.base.sub.flags & 0x20 == 0 || Img(bd).i16(BD_CITY) < 0 {
                            continue;
                        }
                        process_city_block(&mut ours, owner, slot, &mut effects);
                        ran += 1;
                    }
                }
                for owner in 0..a.cities.lists.len() {
                    for slot in 0..a.cities.lists[owner].elems.len().min(b.cities.lists[owner].elems.len()) {
                        let (Some(ca), Some(cb), Some(co)) =
                            (&a.cities.lists[owner].elems[slot], &b.cities.lists[owner].elems[slot], &ours.cities.lists[owner].elems[slot])
                        else {
                            continue;
                        };
                        if co.flags != ca.flags && co.flags != cb.flags {
                            introduced.push(format!("{name} f{fa}->f{fb} City[{owner}][{slot}].flags retail {:#x}->{:#x} ours {:#x}", ca.flags, cb.flags, co.flags));
                        }
                        for i in 0..ca.pod.len().min(cb.pod.len()).min(co.pod.len()) {
                            if co.pod[i] != ca.pod[i] && co.pod[i] != cb.pod[i] {
                                introduced.push(format!("{name} f{fa}->f{fb} City[{owner}][{slot}] +{:#x} retail {:#x}->{:#x} ours {:#x}", i + 6, ca.pod[i], cb.pod[i], co.pod[i]));
                            }
                        }
                    }
                }
                for owner in 0..a.objects.lists.len() {
                    for slot in 0..a.objects.lists[owner].elems.len().min(b.objects.lists[owner].elems.len()) {
                        let (Some(Obj::Build(xa)), Some(Obj::Build(xb)), Some(Obj::Build(xo))) =
                            (&a.objects.lists[owner].elems[slot], &b.objects.lists[owner].elems[slot], &ours.objects.lists[owner].elems[slot])
                        else {
                            continue;
                        };
                        if xa.wall_body != xo.wall_body && xb.wall_body != xo.wall_body {
                            introduced.push(format!("{name} f{fa}->f{fb} [{owner}][{slot}] wall_body"));
                        }
                        if xa.base.mid != xo.base.mid && xb.base.mid != xo.base.mid {
                            introduced.push(format!("{name} f{fa}->f{fb} [{owner}][{slot}] ObjectData"));
                        }
                    }
                }
                for who in 0..9 {
                    let (la, lb, lo) = (&a.leaders.slots[who], &b.leaders.slots[who], &ours.leaders.slots[who]);
                    if lo.flags != la.flags && lo.flags != lb.flags {
                        introduced.push(format!("{name} f{fa}->f{fb} Leader[{who}].flags"));
                    }
                }
                for r in 0..a.regions.elems.len().min(b.regions.elems.len()) {
                    if ours.regions.elems[r].head != a.regions.elems[r].head && ours.regions.elems[r].head != b.regions.elems[r].head {
                        introduced.push(format!("{name} f{fa}->f{fb} Region[{r}].head"));
                    }
                }
                let _ = effects;
            }
        }
        if pairs_n == 0 {
            eprintln!("no capture dir; skipping");
            return;
        }
        eprintln!("city block ran on {ran} centres over {pairs_n} pairs");
        for r in introduced.iter().take(40) {
            eprintln!("  INTRODUCED {r}");
        }
        assert!(introduced.is_empty(), "{} introduced rows", introduced.len());
        assert!(ran > 0);
    }

    /// Diagnostic: every City byte retail changed between consecutive
    /// captures, by CityData image offset (`-- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn dump_city_diffs() {
        for name in ["20261004-075733-stride1", "20261004-081342-stride15", "20261004-044959"] {
            println!("== {name}");
            for (fa, fb, a, b) in pairs(name) {
                let mut rows = Vec::new();
                for owner in 0..a.cities.lists.len().min(b.cities.lists.len()) {
                    let (la, lb) = (&a.cities.lists[owner], &b.cities.lists[owner]);
                    for slot in 0..la.elems.len().max(lb.elems.len()) {
                        match (la.elems.get(slot).and_then(|c| c.as_ref()), lb.elems.get(slot).and_then(|c| c.as_ref())) {
                            (Some(ca), Some(cb)) => {
                                let mut d = Vec::new();
                                if ca.flags != cb.flags {
                                    d.push(format!("flags:{:#x}->{:#x}", ca.flags, cb.flags));
                                }
                                for (i, (x, y)) in ca.pod.iter().zip(cb.pod.iter()).enumerate() {
                                    if x != y {
                                        d.push(format!("+{:#x}:{:#04x}->{:#04x}", i + 6, x, y));
                                    }
                                }
                                if ca.vans.elems.len() != cb.vans.elems.len() {
                                    d.push(format!("vans {}->{}", ca.vans.elems.len(), cb.vans.elems.len()));
                                }
                                if !d.is_empty() {
                                    rows.push(format!("[{owner}][{slot}] {}", d.join(" ")));
                                }
                            }
                            (None, None) => {}
                            (x, _) => rows.push(format!("[{owner}][{slot}] presence {}", x.is_some())),
                        }
                    }
                }
                if !rows.is_empty() {
                    println!("f{fa}->f{fb}:");
                    for r in rows {
                        println!("  {r}");
                    }
                }
            }
        }
    }
}
