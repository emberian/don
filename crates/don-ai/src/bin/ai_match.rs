//! Headless AI-vs-AI match runner.
//!
//! ```text
//! cargo run -p don-ai --bin ai-match -- [--minutes N] [--players Romans,Greeks]
//!                                       [--difficulty tough] [--data DIR]
//!                                       [--timeline] [--sweep-camp-size]
//! ```
//!
//! Every player is the transcribed `economic.bhs` production script driving
//! `don_ai::game::Game` through `don_ai::orders::Order`. The ten *compiled*
//! production stages are stubs, so what this measures is the script layer on its
//! own — see `crates/don-ai/src/game.rs` for the model boundary.

use std::collections::BTreeMap;

use don_ai::abi::Difficulty;
use don_ai::game::{Game, FRAMES_PER_SECOND};
use don_ai::rules::{default_data_dir, Rules, NRES, RES_NAMES};
use don_ai::scheduler::AiSet;

fn parse_difficulty(s: &str) -> Option<Difficulty> {
    Some(match s.to_ascii_lowercase().as_str() {
        "easiest" => Difficulty::Easiest,
        "easy" => Difficulty::Easy,
        "moderate" => Difficulty::Moderate,
        "tough" => Difficulty::Tough,
        "tougher" => Difficulty::Tougher,
        "toughest" => Difficulty::Toughest,
        _ => return None,
    })
}

struct Args {
    minutes: i64,
    nations: Vec<String>,
    difficulty: Difficulty,
    data: std::path::PathBuf,
    timeline: bool,
    sweep: bool,
    trigger_model: bool,
    fast_economy: bool,
    income_probe: bool,
}

fn parse_args() -> Args {
    let mut a = Args {
        minutes: 10,
        nations: vec!["Romans".into(), "Greeks".into()],
        difficulty: Difficulty::Tough,
        data: default_data_dir(),
        timeline: false,
        sweep: false,
        trigger_model: false,
        fast_economy: false,
        income_probe: false,
    };
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--minutes" => {
                i += 1;
                a.minutes = argv
                    .get(i)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(a.minutes);
            }
            "--players" => {
                i += 1;
                if let Some(v) = argv.get(i) {
                    a.nations = v.split(',').map(|s| s.trim().to_string()).collect();
                }
            }
            "--difficulty" => {
                i += 1;
                if let Some(d) = argv.get(i).and_then(|v| parse_difficulty(v)) {
                    a.difficulty = d;
                }
            }
            "--data" => {
                i += 1;
                if let Some(v) = argv.get(i) {
                    a.data = std::path::PathBuf::from(v);
                }
            }
            "--timeline" => a.timeline = true,
            // Turns on the UNVERIFIED `city_placement` trigger model. Without
            // it the shipped build order parks on step 10 and its own hang
            // watchdog retires it; see crates/don-ai/src/library.rs.
            "--trigger-model" => a.trigger_model = true,
            // ModelParams::gather_period_shift = 0. See its doc comment.
            "--fast-economy" => a.fast_economy = true,
            "--sweep-camp-size" => a.sweep = true,
            "--income-probe" => a.income_probe = true,
            other => eprintln!("ignoring unknown argument {other}"),
        }
        i += 1;
    }
    a
}

fn run(
    rules: Rules,
    nations: &[String],
    diff: Difficulty,
    frames: i64,
    timeline: bool,
    trigger_model: bool,
    fast_economy: bool,
) -> Game {
    let refs: Vec<&str> = nations.iter().map(|s| s.as_str()).collect();
    let mut g = Game::new(rules, &refs, diff);
    g.logging = timeline;
    if fast_economy {
        g.params.gather_period_shift = 0;
    }
    let mut ai = AiSet::new(refs.len()).with_trigger_model(trigger_model);
    for _ in 0..frames {
        ai.tick(&mut g);
        g.step();
    }
    // Stash the counters where the report can read them.
    print_report(&g, &ai, timeline);
    println!(
        "\ncity_placement trigger model: {}",
        if trigger_model {
            "ON (UNVERIFIED interpretation)"
        } else {
            "off (faithful stub)"
        }
    );
    g
}

fn print_report(g: &Game, ai: &AiSet, timeline: bool) {
    println!(
        "\n=== {} players, {} difficulty, {} frames ({} s at {} fps) ===",
        g.players.len(),
        format!("{:?}", g.global_difficulty),
        g.frame,
        g.frame / FRAMES_PER_SECOND as i64,
        FRAMES_PER_SECOND
    );
    for (idx, p) in g.players.iter().enumerate() {
        let rt = &ai.runtimes[idx];
        println!("\n--- P{} {} ---", p.who, p.nation);
        println!(
            "  script:   step={} runs={} block={} done={}  production_step={}",
            rt.script_step,
            rt.counters.script_runs,
            rt.counters.script_blocked,
            rt.counters.script_done,
            rt.production_step
        );
        println!(
            "  compiled: setup={} found_cities={} research={} upgrade={} units={} builds={} make={}",
            rt.counters.production_ai_setup,
            rt.counters.found_cities,
            rt.counters.research_techs,
            rt.counters.upgrade_units,
            rt.counters.create_units,
            rt.counters.create_buildings,
            rt.counters.make_stuff
        );
        println!(
            "  orders:   ok={} refused={} invalid={}",
            p.orders_ok, p.orders_refused, p.orders_invalid
        );
        let mut stock = String::new();
        for r in 0..NRES {
            stock.push_str(&format!("{}={} ", RES_NAMES[r], p.stock[r]));
        }
        println!("  stock:    {stock}");
        println!(
            "  age={} pop={} cities={} citizens={} assigned={}",
            g.age_of(idx),
            g.population(idx),
            p.cities.len(),
            p.units.get("Citizen").copied().unwrap_or(0),
            p.assigned
        );
        let mut by_type: BTreeMap<&str, i32> = BTreeMap::new();
        for b in &p.buildings {
            *by_type.entry(b.ty.as_str()).or_insert(0) += 1;
        }
        println!(
            "  buildings: {}",
            by_type
                .iter()
                .map(|(k, v)| format!("{k}x{v}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let mut techs: Vec<&String> = p.techs.iter().collect();
        techs.sort();
        println!("  techs ({}): {}", techs.len(), {
            let s: Vec<&str> = techs.iter().map(|t| t.as_str()).collect();
            s.join(", ")
        });
    }
    if timeline {
        println!("\n--- timeline ---");
        for e in &g.log {
            println!(
                "  t={:>6} ({:>4}s) P{} {}",
                e.frame,
                e.frame / FRAMES_PER_SECOND as i64,
                e.who,
                e.text
            );
        }
    }
}

fn main() {
    let a = parse_args();
    let rules = match Rules::load(&a.data) {
        Ok(r) => r,
        Err(e) => {
            eprintln!(
                "cannot load shipped rules from {}: {e}\n\
                 ron-data/ is gitignored game content; pass --data <dir>.",
                a.data.display()
            );
            std::process::exit(2);
        }
    };
    let frames = a.minutes * 60 * FRAMES_PER_SECOND as i64;

    if a.income_probe {
        // Quantify the difficulty cheat in isolation: identical starting town,
        // no AI at all, so nothing is spent and the only difference between
        // rows is `LeaderData::get_gather_handicap`.
        let mut rows = Vec::new();
        for d in [
            Difficulty::Easiest,
            Difficulty::Easy,
            Difficulty::Moderate,
            Difficulty::Tough,
            Difficulty::Tougher,
            Difficulty::Toughest,
        ] {
            let mut g = Game::new(rules.clone(), &["Romans"], d);
            g.logging = false;
            if a.fast_economy {
                g.params.gather_period_shift = 0;
            }
            let start = g.players[0].stock;
            for _ in 0..frames {
                g.step();
            }
            rows.push((
                d,
                d.income_bonus_percent(),
                g.players[0].stock[0] - start[0],
                g.players[0].stock[1] - start[1],
            ));
        }
        let base_food = rows
            .iter()
            .find(|r| r.0 == Difficulty::Tough)
            .map(|r| r.2)
            .unwrap_or(1);
        let base_timber = rows
            .iter()
            .find(|r| r.0 == Difficulty::Tough)
            .map(|r| r.3)
            .unwrap_or(1);
        println!("difficulty,bonus_pct,food,timber,food_ratio,timber_ratio,nominal_ratio");
        for (d, pct, f, t) in rows {
            println!(
                "{:?},{},{},{},{:.4},{:.4},{:.4}",
                d,
                pct,
                f,
                t,
                f as f64 / base_food as f64,
                t as f64 / base_timber as f64,
                (100 + pct) as f64 / 100.0
            );
        }
        return;
    }

    if a.sweep {
        // The one model number that dominates the opening: how many citizens a
        // Woodcutter's Camp absorbs. Sweep it and print the effect, so the
        // sensitivity is a measurement rather than an assumption.
        println!("camp_size,citizens,food,timber,buildings,orders_ok");
        for size in 1..=8 {
            let refs: Vec<&str> = a.nations.iter().map(|s| s.as_str()).collect();
            let mut g = Game::new(rules.clone(), &refs[..1], a.difficulty);
            g.logging = false;
            if a.fast_economy {
                g.params.gather_period_shift = 0;
            }
            g.params.gather_max.insert("Woodcutter's Camp".into(), size);
            let mut ai = AiSet::new(1).with_trigger_model(true);
            for _ in 0..frames {
                ai.tick(&mut g);
                g.step();
            }
            println!(
                "{},{},{},{},{},{}",
                size,
                g.players[0].units.get("Citizen").copied().unwrap_or(0),
                g.players[0].stock[0],
                g.players[0].stock[1],
                g.players[0].buildings.len(),
                g.players[0].orders_ok
            );
        }
        return;
    }

    run(
        rules,
        &a.nations,
        a.difficulty,
        frames,
        a.timeline,
        a.trigger_model,
        a.fast_economy,
    );
}
