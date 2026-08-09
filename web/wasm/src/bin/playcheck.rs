//! Native twin of the playable client: runs a scripted game against the **real** packed
//! tables and prints what happened, so every number the browser shows can be checked
//! without one.
//!
//! ```sh
//! cd web/wasm
//! cargo run --release --bin playcheck -- ../public/data/gamedata.bin ../public/data/playdata.bin
//! ```
//!
//! Exit code 1 if the scripted game fails to do something it claims to do — this is a
//! check that can fail, not a demo.

use don_web::game::{gap, GameWorld, PlayData, MAP_TILES};
use don_web::gamedata::GameData;
use don_sim::systems::economy as econ;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // `playcheck digest <gamedata> <playdata> <seed-hex> <frames>` — the cross-target
    // check. The browser prints the same three numbers from `window.don.freshDigest`.
    if args.get(1).map(|s| s.as_str()) == Some("digest") {
        let gd = std::fs::read(args.get(2).map(|s| s.as_str()).unwrap_or(""))
            .ok()
            .and_then(|b| GameData::parse(&b))
            .unwrap_or_else(GameData::synthetic);
        let pd = std::fs::read(args.get(3).map(|s| s.as_str()).unwrap_or(""))
            .ok()
            .and_then(|b| PlayData::parse(&b))
            .unwrap_or_else(PlayData::empty);
        let seed = u64::from_str_radix(
            args.get(4).map(|s| s.trim_start_matches("0x")).unwrap_or("c0ffee"), 16)
            .unwrap_or(0xC0FFEE);
        let frames: u32 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(600);
        let mut w = GameWorld::new(&gd, &pd, seed);
        for _ in 0..frames {
            w.step(&gd, &pd);
        }
        println!("seed 0x{seed:x} frames {frames} live {} digest {:016x}",
            w.live_count(), w.digest());
        return;
    }
    let gd = args
        .get(1)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| GameData::parse(&b))
        .unwrap_or_else(|| {
            eprintln!("note: no gamedata.bin, running the synthetic unit table");
            GameData::synthetic()
        });
    let pd = args
        .get(2)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| PlayData::parse(&b))
        .unwrap_or_else(|| {
            eprintln!("note: no playdata.bin, running with no costs or menus");
            PlayData::empty()
        });

    println!("gamedata: {} unit types, real={}", gd.units.len(), gd.is_real);
    println!(
        "playdata: {} units, {} buildings, {} producer edges, real={}",
        pd.units.len(),
        pd.blds.len(),
        pd.edges.len(),
        pd.is_real
    );
    if pd.is_real {
        println!(
            "  GATHER_RATE={}  PEASANT worker rate={}/rate  OIL worker rate={}/rate",
            pd.rules.at(636),
            econ::worker_rate(&pd.rules, false),
            econ::worker_rate(&pd.rules, true)
        );
        let start: Vec<i32> = (0..6).map(|i| pd.rules.at(564 + i * 4)).collect();
        println!("  STARTING_GOODS {start:?}   POP_CAP[3]={}", pd.rules.at(964 + 12));
        for producer in [414, 415, 427, 428, 430, 432, 435, 436] {
            let n = pd.products_of(producer).count();
            if n > 0 {
                println!("  producer {producer} trains {n}");
            }
        }
    }

    let mut w = GameWorld::new(&gd, &pd, 0xC0FFEE);
    println!(
        "\nnew game: {} objects, player 0 stock {:?}",
        w.live_count(),
        w.players[0].econ.stockpile
    );

    // --- put every citizen on the nearest gatherable tile -------------------------------
    let (sx, sy) = start_tile(0);
    let mut assigned = 0;
    let citizens: Vec<usize> = (0..w.live_count() as usize)
        .filter(|&r| w.aux[r].owner == 0 && !w.aux[r].is_building)
        .collect();
    for (k, &row) in citizens.iter().enumerate() {
        let id = w.id_of_row(row);
        let Some(tile) = nearest_node(&w, sx, sy, k as i32) else { continue };
        w.cmd_group(0, [id as i16].into_iter());
        if w.cmd_gather(0, tile) > 0 {
            assigned += 1;
        }
    }
    println!("assigned {assigned} workers to nodes");

    let t0 = std::time::Instant::now();
    for _ in 0..900 {
        w.step(&gd, &pd);
    }
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let p = &w.players[0];
    println!(
        "\nafter 900 frames ({:.1} game seconds at 67 ms/tick):",
        900.0 * 0.067
    );
    println!("  stock   {:?}", p.econ.stockpile);
    println!("  income  {:?}  (displayed, 1/16 per GATHER_RATE frames)", p.income);
    println!("  cap     {:?}", p.econ.commerce_cap);
    println!("  workers {:?}", p.workers);
    println!("  pop {}/{}  age {}", p.pop, p.pop_cap, p.econ.age);
    println!("  sim: {ms:.2} ms for 900 frames = {:.4} ms/frame", ms / 900.0);

    // --- build something ----------------------------------------------------------------
    let mut built = None;
    if pd.is_real {
        let ids: Vec<i16> = (0..w.live_count() as usize)
            .filter(|&r| w.aux[r].owner == 0 && !w.aux[r].is_building)
            .map(|r| w.id_of_row(r) as i16)
            .take(3)
            .collect();
        w.cmd_group(0, ids.into_iter());
        // Barracks, 427. Search outward from the start for a fully clear anchor.
        'search: for r in 3..20 {
            for dy in -r..=r {
                for dx in -r..=r {
                    let (tx, ty) = (sx + dx, sy + dy);
                    if w.grade_placement(&pd, 0, 427, tx, ty) == 4 {
                        if w.cmd_build(&pd, 0, tx, ty, 427) > 0 {
                            built = Some((tx, ty));
                            break 'search;
                        }
                    }
                }
            }
        }
    }
    match built {
        Some((tx, ty)) => println!("\nplaced a Barracks at tile ({tx}, {ty}) [FULLY_CLEAR]"),
        None => println!("\nno Barracks placed"),
    }
    for _ in 0..2000 {
        w.step(&gd, &pd);
    }
    let done = (0..w.live_count() as usize)
        .filter(|&r| w.aux[r].is_building && w.aux[r].type_id == 427 && w.aux[r].build_progress < 0)
        .count();
    println!("finished Barracks: {done}");

    // --- train from it ------------------------------------------------------------------
    let mut trained = 0;
    if done > 0 {
        let brow = (0..w.live_count() as usize)
            .find(|&r| w.aux[r].is_building && w.aux[r].type_id == 427)
            .unwrap();
        let bid = w.id_of_row(brow);
        w.cmd_group(0, [bid as i16].into_iter());
        let product = pd.products_of(427).next().unwrap_or(-1);
        let before = count_of(&w, product);
        let queued = w.cmd_queue_up(&pd, 0, product, 2);
        for _ in 0..1200 {
            w.step(&gd, &pd);
        }
        trained = count_of(&w, product) - before;
        println!(
            "queued {queued} x type {product} at the Barracks -> {trained} appeared \
             (pop {}/{})",
            w.players[0].pop, w.players[0].pop_cap
        );
    }

    // --- age advance --------------------------------------------------------------------
    let age_before = w.players[0].econ.age;
    if pd.is_real {
        w.players[0].econ.stockpile = [9999; 6];
        w.cmd_queue_up(&pd, 0, 544, 1);
        for _ in 0..700 {
            w.step(&gd, &pd);
        }
    }
    println!("age {age_before} -> {}", w.players[0].econ.age);

    println!("\nissued-but-unexecuted counters: {:?}", w.gaps);
    println!("gap names: {:?}", GAP_NAMES);

    let mut bad = 0;
    if pd.is_real {
        if assigned == 0 {
            eprintln!("FAIL: no worker could be assigned to a node");
            bad += 1;
        }
        if w.players[0].econ.stockpile.iter().all(|&v| v == 0) {
            eprintln!("FAIL: the economy never paid out");
            bad += 1;
        }
        if done == 0 {
            eprintln!("FAIL: no building was completed");
            bad += 1;
        }
        if trained <= 0 {
            eprintln!("FAIL: nothing was trained");
            bad += 1;
        }
        if w.players[0].econ.age == age_before {
            eprintln!("FAIL: age did not advance");
            bad += 1;
        }
    }
    if bad > 0 {
        std::process::exit(1);
    }
}

const GAP_NAMES: [&str; gap::COUNT] = [
    "unknown_opcode",
    "no_selection",
    "cannot_afford",
    "pop_capped",
    "placement_blocked",
    "not_a_producer",
    "queue_full",
    "no_worker",
    "not_gatherable",
    "wrong_age",
    "capacity_full",
    "path_stuck",
];

fn start_tile(p: usize) -> (i32, i32) {
    let inset = MAP_TILES / 5;
    match p % 4 {
        0 => (inset, inset),
        1 => (MAP_TILES - inset, MAP_TILES - inset),
        2 => (MAP_TILES - inset, inset),
        _ => (inset, MAP_TILES - inset),
    }
}

/// The `skip`-th distinct gatherable tile found spiralling out from a point.
fn nearest_node(w: &GameWorld, sx: i32, sy: i32, skip: i32) -> Option<i32> {
    let mut seen = 0;
    for r in 1..30i32 {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs() != r && dy.abs() != r {
                    continue;
                }
                let (tx, ty) = (sx + dx, sy + dy);
                if w.tile_resource(tx, ty).is_some() {
                    if seen == skip {
                        return Some(ty * MAP_TILES + tx);
                    }
                    seen += 1;
                }
            }
        }
    }
    None
}

fn count_of(w: &GameWorld, type_id: i32) -> i32 {
    (0..w.live_count() as usize)
        .filter(|&r| w.aux[r].owner == 0 && w.aux[r].type_id == type_id)
        .count() as i32
}
