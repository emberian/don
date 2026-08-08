//! Native twin of the browser's readouts.
//!
//! Builds the *same* shard the wasm shim builds, through the *same* C-ABI entry points and
//! off the *same* packed game data, so numbers the browser prints can be compared against a
//! host build:
//!
//! * `digest` — the batch digest after exactly N frames. Two independently compiled targets
//!   (aarch64, where LLVM autovectorises freely, and wasm32, where it does not) agreeing on
//!   a 64-bit digest over hundreds of thousands of unit-frames of *real* combat is a check
//!   that could have failed. It also proves the browser is running the same simulation the
//!   host runs, which is the precondition for a spectator being worth looking at.
//!
//! * `bench` — single-threaded `sim_step` + `sim_gather` throughput, which is exactly what
//!   one browser worker does, so the browser's `stepMs`/`gatherMs` are directly comparable.
//!
//! * `damage` — the derived damage chain, dumped for a matchup by name. This is the number
//!   the screen is drawing; printing it here means a claim about it can be checked without
//!   a browser.
//!
//! ```sh
//! cargo run --release --bin digest -- digest <worlds> <owners> <per-owner> <cap> <frames> <seed-hex>
//! cargo run --release --bin digest -- bench  <worlds> <owners> <per-owner> <cap> <frames>
//! cargo run --release --bin digest -- damage <attacker-type-id> <defender-type-id>
//! ```
//!
//! All three read `web/public/data/gamedata.bin` if it exists (override with
//! `DON_GAMEDATA=<path>`) and say so; without it they run the synthetic table, and any
//! comparison against the browser must then use the synthetic table on both sides.

use std::time::Instant;

fn usage() -> ! {
    eprintln!("usage: digest digest <worlds> <owners> <per-owner> <capacity> <frames> <seed-hex>");
    eprintln!("       digest bench  <worlds> <owners> <per-owner> <capacity> <frames>");
    eprintln!("       digest damage <attacker-type-id> <defender-type-id>");
    std::process::exit(2);
}

/// Stage the packed game data through the same entry point the browser uses.
fn stage_gamedata() -> bool {
    let path = std::env::var("DON_GAMEDATA").unwrap_or_else(|_| {
        // Relative to the crate root, which is where cargo runs the binary from.
        "../public/data/gamedata.bin".to_string()
    });
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("no game data at {path} — running the SYNTHETIC table");
        return false;
    };
    // SAFETY: single-threaded; the buffer is filled before any `sim_create`.
    unsafe {
        let p = don_web::sim_data_alloc(bytes.len() as u32);
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
    }
    let ok = don_web::sim_data_check() == 1;
    println!(
        "game data: {path} ({} bytes) — {}",
        bytes.len(),
        if ok { "REAL" } else { "REJECTED, falling back to synthetic" }
    );
    ok
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.is_empty() {
        usage();
    }
    let n = |i: usize| -> u32 { a.get(i).and_then(|s| s.parse().ok()).unwrap_or_else(|| usage()) };
    stage_gamedata();

    match a[0].as_str() {
        "digest" => {
            if a.len() != 7 {
                usage();
            }
            let (worlds, owners, per, capacity, frames) = (n(1), n(2), n(3), n(4), n(5));
            let seed =
                u64::from_str_radix(a[6].trim_start_matches("0x"), 16).unwrap_or_else(|_| usage());
            let s = don_web::sim_create(worlds, owners, per, capacity, seed as u32, (seed >> 32) as u32);
            assert!(!s.is_null(), "sim_create returned null");
            // SAFETY: `s` is the handle just returned by `sim_create`, destroyed below.
            unsafe {
                don_web::sim_step(s, frames);
                println!(
                    "worlds={worlds} owners={owners} per_owner={per} capacity={capacity} \
                     frames={frames} seed=0x{seed:016x} real={}",
                    don_web::sim_is_real(s)
                );
                println!(
                    "live_total={} kills={} damage={}",
                    don_web::sim_live_total(s),
                    don_web::sim_kills(s),
                    don_web::sim_damage_lo(s)
                );
                println!(
                    "digest=0x{:08x}{:08x}",
                    don_web::sim_digest_hi(s),
                    don_web::sim_digest_lo(s)
                );
                don_web::sim_destroy(s);
            }
        }
        "bench" => {
            if a.len() != 6 {
                usage();
            }
            let (worlds, owners, per, capacity, frames) = (n(1), n(2), n(3), n(4), n(5));
            let s = don_web::sim_create(worlds, owners, per, capacity, 0xC0FFEE, 0);
            assert!(!s.is_null(), "sim_create returned null");
            // SAFETY: as above.
            unsafe {
                // Warm up: first touch of every column page is not steady state, and at
                // 4096 worlds it is most of a short run.
                don_web::sim_step(s, 30);
                don_web::sim_gather(s);

                let t0 = Instant::now();
                don_web::sim_step(s, frames);
                let step = t0.elapsed().as_secs_f64();

                let g0 = Instant::now();
                for _ in 0..frames {
                    don_web::sim_gather(s);
                }
                let gather = g0.elapsed().as_secs_f64();

                let live = don_web::sim_live_total(s) as f64;
                println!(
                    "native single-thread, {worlds} worlds x {owners} owners x {per} units \
                     (capacity {capacity})"
                );
                println!(
                    "  step   {:8.4} ms/frame   {:9.1} M unit-steps/s   (live {live})",
                    step / frames as f64 * 1e3,
                    live * frames as f64 / step / 1e6
                );
                println!(
                    "  gather {:8.4} ms/call    {:9.1} M cells/s   ({} cells)",
                    gather / frames as f64 * 1e3,
                    (worlds as f64 * don_web::sim_stride(s) as f64) * frames as f64 / gather / 1e6,
                    worlds as u64 * don_web::sim_stride(s) as u64
                );
                println!(
                    "  kills {}  damage {}",
                    don_web::sim_kills(s),
                    don_web::sim_damage_lo(s)
                );
                don_web::sim_destroy(s);
            }
        }
        "damage" => {
            if a.len() != 3 {
                usage();
            }
            let (at, dt) = (n(1) as i32, n(2) as i32);
            let gd = don_web::gamedata::GameData::parse(&std::fs::read(
                std::env::var("DON_GAMEDATA")
                    .unwrap_or_else(|_| "../public/data/gamedata.bin".into()),
            )
            .unwrap_or_default())
            .unwrap_or_else(don_web::gamedata::GameData::synthetic);
            let (Some(ai), Some(di)) = (gd.index_of_type(at), gd.index_of_type(dt)) else {
                eprintln!("unknown type id");
                std::process::exit(2);
            };
            let (au, du) = (gd.units[ai], gd.units[di]);
            let bal = gd.balance_pct(at, dt);
            // Two attack directions relative to a defender facing 0. Deliberately *not*
            // labelled "front"/"back": what `attack_dir` means geometrically is not
            // established, and the chain's own bias (`facing - dir - 0x80000000`) makes the
            // equal-angle case the one that scores a flank tier. Reporting the raw angle and
            // the tier is the honest form.
            for (label, dir) in [("dir == facing", 0u32), ("dir = facing + 1/2 turn", 0x8000_0000u32)] {
                let i = don_sim::DamageInput {
                    balance_pct: bal,
                    attack: don_sim::get_attack(au.attack, false, au.military_level, gd.rules_0x8b8),
                    armor: don_sim::get_armor(du.armor, false, du.military_level, gd.rules_0x8b8),
                    attacker_masks: au.obj_masks as u32,
                    defender_masks: du.obj_masks as u32,
                    attack_dir: dir as i32,
                    attacker_player: 0,
                    attacker_type_id: au.type_id,
                    attacker_domain: au.domain,
                    attacker_splash_percent: au.splash_percent,
                    defender_type_id: du.type_id,
                    defender_domain: du.domain,
                    defender_splash_divisor: 1,
                    defender_facing: 0,
                    defender_facing_entrench: 0,
                    tile_owner: 0,
                    ..Default::default()
                };
                let (d, trace) = don_sim::damage_traced(
                    &i,
                    &don_web::real::predicates(),
                    &gd.rules,
                    &don_sim::UnreachedTerms::default(),
                );
                let steps: Vec<&str> = don_sim::STEP_NAMES
                    .iter()
                    .enumerate()
                    .filter(|(k, _)| trace >> k & 1 == 1)
                    .map(|(_, s)| *s)
                    .collect();
                let delta = (i.defender_facing as u32)
                    .wrapping_sub(i.attack_dir as u32)
                    .wrapping_sub(0x8000_0000);
                let tier = if delta >= 0x2AAA_AAAA { don_sim::flank_level(delta) } else { 0 };
                println!(
                    "{:>24}: attack_x10={} armor={} balance={}% -> damage {}  flank_tier={tier}  steps[{}]",
                    label,
                    i.attack,
                    i.armor,
                    bal,
                    d,
                    steps.join(",")
                );
            }
        }
        _ => usage(),
    }
}
