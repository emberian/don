//! Step 12: `GameDaemon::process_all` 0x00732700 (`re/decomp-all/00732700.c`).
//!
//! Transcribed children, in retail call order:
//!
//! 1. `GameDaemon::busy` decrement (+0x28) — NOT walked (the save walks
//!    `GameDaemon+0..+0x28` only, decomp 005a2360.c:678), so no state here.
//! 2. `repaths[i] = repaths[i] / 2, floored to 0 when < 3` for the eight
//!    `GameDaemon::repaths` slots (0x0073271c..0x007327b2; signed division,
//!    `cdq / sub / sar 1 / cmp 3 / cmovl`).
//! 3. `GameDaemon::process_victory` 0x00730ef0 — UNTRANSCRIBED (victory
//!    condition scan; writes Leader timers and UI, no walked Game fields).
//! 4. `frame % 200 == 0` → `GameDaemon::calc_danger` 0x00732d10 —
//!    UNTRANSCRIBED (rebuilds `World.danger`).
//! 5. `frame % 100 == 33` → `GameDaemon::update_all_seen` 0x00732840 —
//!    UNTRANSCRIBED (fog/seen rebuild through object virtuals).
//! 6. `GameDaemon::calc_markets` 0x00732180 + `calc_market(int)`
//!    0x00732270 — fully transcribed below (market prices, flux, RNG).
//! 7. Region flag pass 0x007327eb..0x00732820: for all 64 `Regions.list`
//!    slots, `flags &= ~0x10; if (flags & 0x20) { flags &= ~0x20; flags |= 0x10 }`.
//! 8. `GameDaemon::check_borders` 0x00732060 — `borders = 0` transcribed;
//!    the per-region `size != 0 && borders < size` branch calls
//!    `FUN_006b0bb0` and is UNTRANSCRIBED (`Region::borders` at +0x2c is
//!    outside the walked 44-byte head, so the condition is not evaluable
//!    from save state).
//! 9. `GameDaemon::process_coll_blocks` 0x00731f90 — transcribed: advances
//!    the `empty_colls` cursor `max(5, xs/4)` slots (wrapping at `xs*ys`)
//!    and frees each visited `CollBlock` whose 96 data bytes are all zero.
//! 10. `Groups::process` 0x006fa210 — the `proc_group` advance is ported
//!     inline in `tick.rs` (it runs before this module; there is no data
//!     dependency between it and anything here).
//!
//! State mapping (all byte-image backed):
//! - `Game::scalars` (image offset − 0x550): `market_tick` +0x14,
//!   `market[6]` +0x18, `market_flux[6]` +0x30, `next_flux[6]` +0x48,
//!   `delta_flux[6]` +0x60, `flux_length[6]` +0x78.
//! - `Save.post_world` (44 B, `WalkDataGame` direct block): `GameDaemon`
//!   +0x00..+0x28 = `repaths[8]`, `empty_colls`, `borders`; then the
//!   `GameAccess::game_random` seed at +40.
//! - `Constants` (`[0x00c061f0]`): `market_basement` +0xcd8,
//!   `market_equilibrium` +0xcdc, `market_min_variance` +0xce0,
//!   `market_min_trend` +0xce4, `market_trend_range` +0xce8,
//!   `market_cycle_rate` +0xcec — read from the typed Rules image
//!   (`Save.rules_tail.rules.constants`), falling back to the earlier
//!   `Save.constants` walk of the same object.
//! - `Region.head` (44 B after the tag): `flags` at +8.
//! - `World.blocks[i]` (`CollBlock`): `present` is the `WData::block`
//!   pointer non-null test; `data` the 96 collision bytes.

use crate::tick::StepStatus;
use crate::Save;

/// `calc_markets`, the repaths decay, the region flag pass,
/// `check_borders`'s `borders = 0`, and `process_coll_blocks` are
/// transcribed; `process_victory`, `calc_danger`, `update_all_seen` and the
/// `check_borders` region branch are not.
pub const STATUS: StepStatus = StepStatus::Partial;

// --- Game::scalars offsets (image offset − 0x550) ---------------------------
const MARKET_TICK: usize = crate::tick::MARKET_TICK; // Game+0x564
const MARKET: usize = 0x18; // Game+0x568 int[6]
const MARKET_FLUX: usize = 0x30; // Game+0x580 int[6]
const NEXT_FLUX: usize = 0x48; // Game+0x598 int[6]
const DELTA_FLUX: usize = 0x60; // Game+0x5b0 int[6]
const FLUX_LENGTH: usize = 0x78; // Game+0x5c8 int[6]
const NUM_MARKET_GOODS: usize = 6; // 0x568..0x580 step 4

// --- post_world offsets ------------------------------------------------------
const REPATHS: usize = 0x00; // GameDaemon+0x00 int[8]
const EMPTY_COLLS: usize = 0x20; // GameDaemon+0x20
const BORDERS: usize = 0x24; // GameDaemon+0x24
const GAME_RANDOM: usize = 0x28; // Random seed (post-World +40)

// --- Constants offsets ([0x00c061f0]) ---------------------------------------
const MARKET_BASEMENT: usize = 0xcd8;
const MARKET_EQUILIBRIUM: usize = 0xcdc;
const MARKET_MIN_VARIANCE: usize = 0xce0;
const MARKET_MIN_TREND: usize = 0xce4;
const MARKET_TREND_RANGE: usize = 0xce8;
const MARKET_CYCLE_RATE: usize = 0xcec;

fn get_i32(buf: &[u8], off: usize) -> i32 {
    i32::from_le_bytes(buf[off..off + 4].try_into().unwrap())
}

fn put_i32(buf: &mut [u8], off: usize, v: i32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

/// Read-only accessor for the `Constants` image. Prefers the typed Rules
/// section image; falls back to the earlier direct walk of the same object.
fn constant(save: &Save, off: usize) -> i32 {
    let rules = &save.rules_tail.rules.constants;
    if rules.len() >= off + 4 {
        return get_i32(rules, off);
    }
    get_i32(&save.constants, off)
}

/// `Random::get(min, max)` 0x00a39d70 on `GameAccess::game_random`:
/// `seed = seed * 0x19660d + 0x3c6ef35f; return ((seed & 0xffff) * (max - min) >> 16) + min`.
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

/// `GameDaemon::calc_market(int good)` 0x00732270.
/// Returns the number of LCG draws consumed (0, 2 or 3).
fn calc_market(save: &mut Save, good: usize) -> u32 {
    let mut draws = 0;
    let min_variance = constant(save, MARKET_MIN_VARIANCE);
    let price = get_i32(&save.game.scalars, MARKET + good * 4);
    // 0x0073228b..0x00732299: eax = price / 2 (signed); if min_variance > eax: eax = min_variance.
    let mut v = price / 2;
    if min_variance > v {
        v = min_variance;
    }
    // 0x0073229c..0x007322a2: range = (v + 1) / 2 (signed).
    let range = (v + 1) / 2;
    let (r1, r2) = if range > 0 {
        let a = game_random(save, 0, 0xffff) % (range + 1);
        let b = game_random(save, 0, 0xffff) % (range + 1);
        draws += 2;
        (a, b)
    } else {
        (0, 0)
    };
    // 0x00732300..0x00732304: next_flux[good] = (r2 - (range + 1)) + r1.
    let next = (r2 - (range + 1)) + r1;
    put_i32(&mut save.game.scalars, NEXT_FLUX + good * 4, next);

    // 0x0073230b..0x00732337: trend = market_trend_range; r3 = trend > 1 ? get() % trend : 0.
    let trend = constant(save, MARKET_TREND_RANGE);
    let r3 = if trend - 1 > 0 {
        draws += 1;
        game_random(save, 0, 0xffff) % trend
    } else {
        0
    };
    // 0x0073233f..0x0073235e: len = market_min_trend + r3; d = next - market_flux[good];
    // flux_length[good] = len; delta_flux[good] = d (then refined below).
    let len = constant(save, MARKET_MIN_TREND) + r3;
    let d = next - get_i32(&save.game.scalars, MARKET_FLUX + good * 4);
    put_i32(&mut save.game.scalars, FLUX_LENGTH + good * 4, len);
    put_i32(&mut save.game.scalars, DELTA_FLUX + good * 4, d);
    // 0x00732364..0x00732381: sign = d > 0 ? 1 : d >> 31;
    // delta_flux[good] = ((len - 1) * sign + d) / len  (signed idiv).
    let sign = if d > 0 { 1 } else { d >> 31 };
    if len != 0 {
        let delta = ((len - 1).wrapping_mul(sign).wrapping_add(d)) / len;
        put_i32(&mut save.game.scalars, DELTA_FLUX + good * 4, delta);
    }
    // (len == 0 would be an integer divide fault in retail; left untouched.)
    draws
}

/// `GameDaemon::calc_markets` 0x00732180. Returns `(ran, draws)`.
fn calc_markets(save: &mut Save, effects: &mut Vec<String>) -> (bool, u32) {
    let frame = get_i32(&save.game.scalars, crate::tick::FRAME);
    // 0x00732189..0x007321aa: gate — frame == 0 || rate < 2 || frame % rate == 0.
    if frame != 0 {
        let rate = constant(save, MARKET_CYCLE_RATE);
        if rate > 1 && frame % rate != 0 {
            return (false, 0);
        }
    }
    let equilibrium = constant(save, MARKET_EQUILIBRIUM);
    let basement = constant(save, MARKET_BASEMENT);
    // market_tick is read once per iteration but never written inside the loop.
    let mt = get_i32(&save.game.scalars, MARKET_TICK);
    let mut draws = 0;
    for good in 0..NUM_MARKET_GOODS {
        let phase = mt.wrapping_add(good as i32);
        // 0x007321c6..0x007321cf: if (mt != 0 && (phase & 7) != 0) continue;
        if mt != 0 && (phase & 7) != 0 {
            continue;
        }
        // 0x007321d1..0x007321d6: if ((phase & 0xff) == 0) — price drift toward equilibrium.
        if (phase & 0xff) == 0 {
            let off = MARKET + good * 4;
            let price = get_i32(&save.game.scalars, off);
            if price > equilibrium {
                if equilibrium == 0 {
                    put_i32(&mut save.game.scalars, off, price - 1);
                } else {
                    put_i32(&mut save.game.scalars, off, price - price / equilibrium);
                }
            } else if price < equilibrium {
                put_i32(&mut save.game.scalars, off, price + 1);
                if price + 1 < basement {
                    put_i32(&mut save.game.scalars, off, price + 2);
                }
            }
            effects.push(format!(
                "Game.market[{good}] {price} -> {} (equilibrium {equilibrium}, basement {basement})",
                get_i32(&save.game.scalars, off)
            ));
        }
        // 0x00732224..0x00732239: if (mt == 0 || --flux_length[good] <= 0) calc_market(good).
        let recalc = if mt == 0 {
            true
        } else {
            let fl = FLUX_LENGTH + good * 4;
            let n = get_i32(&save.game.scalars, fl) - 1;
            put_i32(&mut save.game.scalars, fl, n);
            n < 1
        };
        if recalc {
            let d = calc_market(save, good);
            draws += d;
            effects.push(format!(
                "Game.calc_market[{good}]: next_flux {} flux_length {} delta_flux {} ({d} draws)",
                get_i32(&save.game.scalars, NEXT_FLUX + good * 4),
                get_i32(&save.game.scalars, FLUX_LENGTH + good * 4),
                get_i32(&save.game.scalars, DELTA_FLUX + good * 4),
            ));
        }
        // 0x00732244..0x00732248: market_flux[good] += delta_flux[good].
        let mf = MARKET_FLUX + good * 4;
        let v = get_i32(&save.game.scalars, mf).wrapping_add(get_i32(&save.game.scalars, DELTA_FLUX + good * 4));
        put_i32(&mut save.game.scalars, mf, v);
    }
    // 0x0073225c: market_tick += 1.
    put_i32(&mut save.game.scalars, MARKET_TICK, mt.wrapping_add(1));
    effects.push(format!("Game.market_tick -> {} ({draws} LCG draws)", mt.wrapping_add(1)));
    (true, draws)
}

/// Repaths decay 0x0073271c..0x007327b2.
fn decay_repaths(save: &mut Save, effects: &mut Vec<String>) {
    if save.post_world.len() < BORDERS + 4 {
        return;
    }
    let mut changed = false;
    for i in 0..8 {
        let off = REPATHS + i * 4;
        let v = get_i32(&save.post_world, off);
        let mut h = v / 2;
        if h < 3 {
            h = 0;
        }
        if h != v {
            changed = true;
        }
        put_i32(&mut save.post_world, off, h);
    }
    if changed {
        effects.push("GameDaemon.repaths[i] = repaths[i]/2 (0 when < 3)".into());
    }
}

/// Region flag pass 0x007327eb..0x00732820 over the 64 `Regions.list` slots.
fn region_flags(save: &mut Save, effects: &mut Vec<String>) {
    let mut touched = 0;
    for r in save.regions.elems.iter_mut().take(64) {
        if r.head.len() < 12 {
            continue;
        }
        let f = get_i32(&r.head, 8);
        let mut n = f & !0x10;
        if n & 0x20 != 0 {
            n = (n & !0x20) | 0x10;
        }
        if n != f {
            touched += 1;
        }
        put_i32(&mut r.head, 8, n);
    }
    if touched > 0 {
        effects.push(format!("Region.flags: 0x20 -> 0x10 handoff / 0x10 clear on {touched} regions"));
    }
}

/// `GameDaemon::check_borders` 0x00732060 — `borders = 0` only.
fn check_borders(save: &mut Save, effects: &mut Vec<String>) {
    if save.post_world.len() < BORDERS + 4 {
        return;
    }
    let old = get_i32(&save.post_world, BORDERS);
    put_i32(&mut save.post_world, BORDERS, 0);
    if old != 0 {
        effects.push(format!("GameDaemon.borders {old} -> 0"));
    }
    // 0x00732077..: per-region `size != 0 && borders < size` → FUN_006b0bb0:
    // UNTRANSCRIBED (Region::borders +0x2c not in the walked head).
}

/// `GameDaemon::process_coll_blocks` 0x00731f90.
fn process_coll_blocks(save: &mut Save, effects: &mut Vec<String>) {
    if save.post_world.len() < EMPTY_COLLS + 4 {
        return;
    }
    let xs = save.world.xs;
    let ys = save.world.ys;
    // 0x00731f9d..0x00731fb5: n = xs / 4 (signed); if (n < 5) n = 5.
    let mut n = xs / 4;
    if n < 5 {
        n = 5;
    }
    let total = xs.wrapping_mul(ys);
    let mut cursor = get_i32(&save.post_world, EMPTY_COLLS);
    let mut freed = 0;
    for _ in 0..n {
        // WData[cursor].block: present + all data bytes zero → block is
        // released (CollBlock::flags 2/1 path), WData::block = 0.
        if cursor >= 0 {
            if let Some(b) = save.world.blocks.get_mut(cursor as usize) {
                if b.present == 1 && b.data.iter().all(|&x| x == 0) {
                    b.present = 0;
                    b.bits = 0;
                    b.size = 0;
                    b.data.clear();
                    freed += 1;
                }
            }
        }
        // 0x00732033..0x00732045: cursor += 1; if (xs*ys <= cursor) cursor = 0.
        cursor = cursor.wrapping_add(1);
        if total <= cursor {
            cursor = 0;
        }
    }
    put_i32(&mut save.post_world, EMPTY_COLLS, cursor);
    effects.push(format!("GameDaemon.empty_colls -> {cursor} ({n} slots scanned, {freed} empty blocks freed)"));
}

pub fn run(save: &mut Save, effects: &mut Vec<String>) {
    if save.game.scalars.len() < FLUX_LENGTH + NUM_MARKET_GOODS * 4 {
        return;
    }
    // 1. busy-- : not walked.
    // 2.
    decay_repaths(save, effects);
    // 3. process_victory 0x00730ef0: UNTRANSCRIBED.
    let frame = get_i32(&save.game.scalars, crate::tick::FRAME);
    // 4.
    if frame % 200 == 0 {
        effects.push("calc_danger 0x00732d10 due (frame % 200 == 0): UNTRANSCRIBED".into());
    }
    // 5.
    if frame % 100 == 0x21 {
        effects.push("update_all_seen 0x00732840 due (frame % 100 == 33): UNTRANSCRIBED".into());
    }
    // 6.
    calc_markets(save, effects);
    // 7.
    region_flags(save, effects);
    // 8.
    check_borders(save, effects);
    // 9.
    process_coll_blocks(save, effects);
    // 10. Groups::process proc_group advance: inline in tick.rs.
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn capture_dirs() -> Vec<PathBuf> {
        let Ok(root) = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(root.join("schema/live/frame-pairs")) {
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

    /// (frame, save_name) per manifest step.
    fn manifest_steps(dir: &Path) -> Vec<(i64, String)> {
        let text = std::fs::read_to_string(dir.join("manifest.json")).unwrap();
        let mut out = Vec::new();
        for seg in text.split("\"frame\":").skip(1) {
            let Some(frame) = seg
                .trim_start()
                .split(|c: char| !c.is_ascii_digit())
                .next()
                .and_then(|t| t.parse::<i64>().ok())
            else {
                continue;
            };
            let save = seg
                .split("\"save_name\":")
                .nth(1)
                .and_then(|s| s.split('"').nth(1))
                .unwrap_or_default()
                .to_string();
            out.push((frame, save));
        }
        out
    }

    #[test]
    fn random_get_matches_retail_formula() {
        let mut s = Save::default();
        s.post_world = vec![0u8; 44];
        s.post_world[GAME_RANDOM..GAME_RANDOM + 4].copy_from_slice(&12345u32.to_le_bytes());
        let r = game_random(&mut s, 0, 0xffff);
        let seed = 12345u32.wrapping_mul(0x19660d).wrapping_add(0x3c6ef35f);
        assert_eq!(
            u32::from_le_bytes(s.post_world[GAME_RANDOM..GAME_RANDOM + 4].try_into().unwrap()),
            seed
        );
        assert_eq!(r, (((seed & 0xffff) * 0xffff) >> 16) as i32);
    }

    /// Oracle: for every stride-1 pair, the market block
    /// (`Game+0x564..+0x5e0`) and the walked `GameDaemon` block after
    /// `do_frame` must equal retail N+1 exactly, and the number of LCG
    /// draws the market consumed must not exceed the retail frame total.
    #[test]
    fn market_block_matches_retail_next_frame() {
        let dirs = capture_dirs();
        if dirs.is_empty() {
            eprintln!("no captures; skipping");
            return;
        }
        let mut pairs = 0;
        let mut market_ticks = 0;
        let mut draws_total = 0;
        for dir in dirs {
            let steps = manifest_steps(&dir);
            for k in 0..steps.len().saturating_sub(1) {
                let (fa, sa) = &steps[k];
                let (fb, sb) = &steps[k + 1];
                if fb - fa != 1 {
                    continue;
                }
                let raw_a = crate::container::load_svx(&dir.join(format!("{sa}.svx"))).unwrap();
                let raw_b = crate::container::load_svx(&dir.join(format!("{sb}.svx"))).unwrap();
                let img_a = crate::load(&raw_a).unwrap();
                let img_b = crate::load(&raw_b).unwrap();
                let mut ours = img_a.state.clone();
                if pairs == 0 {
                    eprintln!(
                        "constants: basement {} equilibrium {} min_variance {} min_trend {} trend_range {} cycle_rate {}",
                        constant(&ours, MARKET_BASEMENT),
                        constant(&ours, MARKET_EQUILIBRIUM),
                        constant(&ours, MARKET_MIN_VARIANCE),
                        constant(&ours, MARKET_MIN_TREND),
                        constant(&ours, MARKET_TREND_RANGE),
                        constant(&ours, MARKET_CYCLE_RATE),
                    );
                    eprintln!(
                        "f{fa}: market_tick {} market {:?} flux {:?} next {:?} delta {:?} len {:?}",
                        get_i32(&ours.game.scalars, MARKET_TICK),
                        (0..6).map(|g| get_i32(&ours.game.scalars, MARKET + g * 4)).collect::<Vec<_>>(),
                        (0..6).map(|g| get_i32(&ours.game.scalars, MARKET_FLUX + g * 4)).collect::<Vec<_>>(),
                        (0..6).map(|g| get_i32(&ours.game.scalars, NEXT_FLUX + g * 4)).collect::<Vec<_>>(),
                        (0..6).map(|g| get_i32(&ours.game.scalars, DELTA_FLUX + g * 4)).collect::<Vec<_>>(),
                        (0..6).map(|g| get_i32(&ours.game.scalars, FLUX_LENGTH + g * 4)).collect::<Vec<_>>(),
                    );
                }

                // Market: run calc_markets alone against retail N+1.
                let mut effects = Vec::new();
                let (ran, draws) = calc_markets(&mut ours, &mut effects);
                draws_total += draws;
                if ran {
                    market_ticks += 1;
                }
                let o = &ours.game.scalars[MARKET_TICK..FLUX_LENGTH + 24];
                let r = &img_b.state.game.scalars[MARKET_TICK..FLUX_LENGTH + 24];
                assert_eq!(
                    o,
                    r,
                    "{}: f{fa}->f{fb} market block mismatch\n ours  {:?}\n retail {:?}\n effects {:#?}",
                    dir.display(),
                    o.chunks(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect::<Vec<_>>(),
                    r.chunks(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect::<Vec<_>>(),
                    effects
                );
                // Retail's total draws for the frame must cover ours.
                let s0 = u32::from_le_bytes(img_a.state.post_world[GAME_RANDOM..GAME_RANDOM + 4].try_into().unwrap());
                let s1 = u32::from_le_bytes(img_b.state.post_world[GAME_RANDOM..GAME_RANDOM + 4].try_into().unwrap());
                let retail_draws = crate::tick::rng_draws(s0, s1).unwrap_or(u32::MAX);
                assert!(draws <= retail_draws, "f{fa}->f{fb}: market drew {draws} > retail frame total {retail_draws}");

                // GameDaemon walked block: full step 12 through do_frame.
                let mut ours2 = img_a.state.clone();
                crate::tick::do_frame(&mut ours2);
                let ours_gd: Vec<i32> =
                    ours2.post_world[..GAME_RANDOM].chunks(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect();
                let retail_gd: Vec<i32> = img_b.state.post_world[..GAME_RANDOM]
                    .chunks(4)
                    .map(|c| i32::from_le_bytes(c.try_into().unwrap()))
                    .collect();
                // empty_colls / borders are owned by this step.
                assert_eq!(
                    &ours_gd[8..10],
                    &retail_gd[8..10],
                    "{}: f{fa}->f{fb} GameDaemon empty_colls/borders mismatch\n ours   {ours_gd:?}\n retail {retail_gd:?}",
                    dir.display(),
                );
                // repaths are decayed here and re-incremented by later
                // (unported) pathing systems within the same frame: ours
                // must never exceed retail.
                for i in 0..8 {
                    assert!(
                        ours_gd[i] <= retail_gd[i],
                        "{}: f{fa}->f{fb} repaths[{i}] ours {} > retail {}",
                        dir.display(),
                        ours_gd[i],
                        retail_gd[i]
                    );
                }
                pairs += 1;
            }
        }
        eprintln!("{pairs} pairs, market ran on {market_ticks}, {draws_total} market LCG draws");
        assert!(pairs > 0);
    }
}
