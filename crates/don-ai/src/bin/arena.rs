//! `cargo run --release -p don-ai --bin arena` — the head-to-head harness.
//!
//! ```text
//! arena h2h                       ShippedOpening vs CapFirst, economy only, both openings
//! arena vs A B [--minutes N]      one match between two named bots
//! arena tourney [--seeds N]       every pairing on N fair maps, both seats
//! arena counters                  what the real balance matrix says to build against what
//! arena boom                      the economy race, no combat, with the boom-completion clock
//! arena levels                    the three Marshal levels against each other
//! ```
//!
//! Bot names: `shipped`, `capfirst`, `recruit`, `veteran`, `marshal`, `idle`.

use don_ai::arena::bots::marshal::{buildable_military, counter_pick, Level, Marshal};
use don_ai::arena::bots::{boom, Bot, HeadPolicy};
use don_ai::arena::match_run::{clock, load_world, run_match, MatchConfig, Outcome};
use don_ai::arena::obs::Obs;
use don_ai::arena::world::FPS;

fn make(name: &str) -> Option<Box<dyn Bot>> {
    Some(match name {
        "shipped" => Box::<boom::ShippedOpening>::default() as Box<dyn Bot>,
        "capfirst" => Box::<boom::CapFirst>::default(),
        "recruit" => Box::new(Marshal::new(Level::RECRUIT)),
        "veteran" => Box::new(Marshal::new(Level::VETERAN)),
        "marshal" => Box::new(Marshal::new(Level::MARSHAL)),
        // The drop-in seam, exercised: a "policy" that emits `don-env` action heads.
        "idle" => Box::new(HeadPolicy {
            label: "IdlePolicy".into(),
            f: |_: &Obs| Vec::new(),
        }),
        _ => return None,
    })
}

const ALL: [&str; 5] = ["shipped", "capfirst", "recruit", "veteran", "marshal"];

fn arg(args: &[String], key: &str) -> Option<String> {
    args.iter()
        .position(|a| a == key)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("h2h");
    let minutes: i64 = arg(&args, "--minutes")
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let seeds: u32 = arg(&args, "--seeds")
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);

    match cmd {
        "counters" => counters(),
        "boom" => boom_race(minutes),
        "vs" => {
            let a = args.get(1).cloned().unwrap_or_else(|| "marshal".into());
            let b = args.get(2).cloned().unwrap_or_else(|| "capfirst".into());
            one(&a, &b, minutes, 0x5EED_0001, true);
        }
        "tourney" => tourney(&ALL, minutes, seeds),
        "levels" => tourney(&["recruit", "veteran", "marshal"], minutes, seeds),
        _ => {
            println!("== the economy race, one world, both openings ==");
            boom_race(minutes);
            println!();
            println!("== and what happens when one side can fight ==");
            one("capfirst", "marshal", minutes, 0x5EED_0001, false);
        }
    }
}

fn cfg(minutes: i64, seed: u32, logging: bool) -> MatchConfig {
    let mut c = MatchConfig {
        minutes,
        logging,
        ..MatchConfig::default()
    };
    c.map.seed = seed;
    c
}

fn one(a: &str, b: &str, minutes: i64, seed: u32, verbose: bool) {
    let (Some(ba), Some(bb)) = (make(a), make(b)) else {
        eprintln!("unknown bot: {a} / {b}");
        return;
    };
    let mut bots: Vec<Box<dyn Bot>> = vec![ba, bb];
    let c = cfg(minutes, seed, verbose);
    match run_match(&c, &mut bots) {
        Err(e) => eprintln!("arena unavailable: {e}"),
        Ok(r) => {
            let (w, why) = r.verdict();
            println!(
                "{} vs {}  ->  {} ({}), ended {}",
                r.names[0],
                r.names[1],
                w.map(|i| r.names[i].clone()).unwrap_or("draw".into()),
                why,
                clock(r.frames)
            );
            for i in 0..2 {
                let s = &r.scores[i];
                println!(
                    "  {:<18} cities {} army {:>2} (value {:>4}) buildings {:>2} civ {:>2} \
                     age {} res {:>6} dmg {:>6} | cmds {} ok {} refused {} invalid {}",
                    r.names[i],
                    s.cities,
                    s.army,
                    s.army_value,
                    s.buildings,
                    s.civilians,
                    s.age,
                    s.resources,
                    s.damage_dealt,
                    r.commands[i],
                    r.orders_ok[i],
                    r.orders_refused[i],
                    r.orders_invalid[i]
                );
            }
            for ((who, verb, tag), n) in &r.rejects {
                println!("  P{} {:<10} {:<8} x{}", who + 1, verb, tag, n);
            }
            if r.shots > 0 {
                println!(
                    "  shots {} | flank levels 0/1/2 = {}/{}/{}",
                    r.shots, r.flank_hist[0], r.flank_hist[1], r.flank_hist[2]
                );
            }
            if verbose {
                for e in r.log.iter().take(120) {
                    println!("    [{:>6}] P{} {}", clock(e.frame), e.who + 1, e.text);
                }
            }
        }
    }
}

/// The economy race with combat impossible: both sides alone on their own map. This is
/// the honest form of "who booms faster", because neither can interfere with the other.
fn boom_race(minutes: i64) {
    println!("bot,boom_complete,citizens,farms,camps,cities,age,food,timber,resources");
    for name in ["shipped", "capfirst", "marshal"] {
        let Some(mut b) = make(name) else { continue };
        let c = cfg(minutes, 0x5EED_0001, false);
        let mut w = match load_world(&c) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("arena unavailable: {e}");
                return;
            }
        };
        // Solo: player 2 exists but never acts, and the map keeps them far apart.
        let ids = w.ids;
        let limit = minutes * 60 * FPS;
        let mut done: Option<i64> = None;
        let mut buf = Vec::new();
        while w.frame < limit {
            let period = b.decide_period().max(1);
            if w.frame % period == 0 {
                buf.clear();
                {
                    let obs = Obs::of(&w, 0);
                    b.act(&obs, &mut buf);
                    if done.is_none() && boom::BoomGoal::default().met(&obs) {
                        done = Some(w.frame);
                    }
                }
                for cmd in buf.drain(..) {
                    w.submit(0, cmd);
                }
            }
            w.step();
        }
        let s = w.score(0);
        println!(
            "{},{},{},{},{},{},{},{},{},{}",
            b.name(),
            done.map(clock).unwrap_or_else(|| "-".into()),
            w.count_type(0, ids.citizen, false),
            w.count_type(0, ids.farm, false),
            w.count_type(0, ids.camp, false),
            w.count_type(0, ids.small_city, false),
            s.age,
            w.players[0].gathered[0],
            w.players[0].gathered[1],
            s.resources
        );
    }
}

fn tourney(names: &[&str], minutes: i64, seeds: u32) {
    let n = names.len();
    let mut wins = vec![0u32; n];
    let mut draws = vec![0u32; n];
    let mut played = vec![0u32; n];
    println!("seed,a,b,result,why,frames");
    for s in 0..seeds {
        let seed = 0x5EED_0001u32.wrapping_add(s.wrapping_mul(0x9E37_79B9));
        for i in 0..n {
            for j in 0..n {
                if i == j {
                    continue;
                }
                let (Some(a), Some(b)) = (make(names[i]), make(names[j])) else {
                    continue;
                };
                let mut bots: Vec<Box<dyn Bot>> = vec![a, b];
                let Ok(r) = run_match(&cfg(minutes, seed, false), &mut bots) else {
                    eprintln!("arena unavailable (game data missing?)");
                    return;
                };
                let (w, why) = r.verdict();
                played[i] += 1;
                played[j] += 1;
                match w {
                    Some(0) => wins[i] += 1,
                    Some(_) => wins[j] += 1,
                    None => {
                        draws[i] += 1;
                        draws[j] += 1;
                    }
                }
                println!(
                    "{seed:#x},{},{},{},{},{}",
                    names[i],
                    names[j],
                    w.map(|k| if k == 0 { names[i] } else { names[j] })
                        .unwrap_or("draw"),
                    why,
                    r.frames
                );
            }
        }
    }
    println!();
    println!("bot,played,wins,draws,winrate");
    for i in 0..n {
        println!(
            "{},{},{},{},{:.3}",
            names[i],
            played[i],
            wins[i],
            draws[i],
            wins[i] as f64 / played[i].max(1) as f64
        );
    }
}

/// What the real 493x493 table says, and what the bot's exchange metric does with it.
fn counters() {
    let c = MatchConfig::default();
    let w = match load_world(&c) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("arena unavailable: {e}");
            return;
        }
    };
    let obs = Obs::of(&w, 0);
    let names = ["Hoplites", "Bowmen", "Slingers", "Catapult", "Small City"];
    let ids: Vec<i32> = names
        .iter()
        .map(|n| {
            w.types
                .rows
                .values()
                .filter(|r| &r.name == n)
                .map(|r| r.id)
                .min()
                .unwrap_or(-1)
        })
        .collect();
    println!("balance percentages, attacker rows, from schema/live/balance-real.bin");
    print!("{:<12}", "atk\\def");
    for n in names {
        print!("{n:>12}");
    }
    println!();
    for (i, a) in ids.iter().enumerate() {
        print!("{:<12}", names[i]);
        for d in &ids {
            print!("{:>12}", w.balance.get(*a, *d).unwrap_or(-1));
        }
        println!();
    }
    println!();
    println!("counter_pick, given an enemy army of one type (Ancient roster, tribe 6):");
    let cands: Vec<i32> = ids[..3].to_vec();
    for (i, e) in ids[..3].iter().enumerate() {
        let pick = counter_pick(&obs, &cands, &[(*e, 10)]);
        let pn = pick
            .and_then(|p| w.types.get(p))
            .map(|t| t.name.clone())
            .unwrap_or_default();
        println!("  vs 10x {:<10} -> build {}", names[i], pn);
    }
    let mirror = counter_pick(&obs, &cands, &[]);
    println!(
        "  vs unknown           -> build {}",
        mirror
            .and_then(|p| w.types.get(p))
            .map(|t| t.name.clone())
            .unwrap_or_default()
    );
    let _ = buildable_military(&obs);
    let _ = Outcome::Timeout;
}
