//! `don-tick-trace` — run N ticks of a populated world through the wired
//! `Game::do_frame` and print which of the 29 steps actually executed.
//!
//! ```sh
//! cargo run -p don-sim --bin don-tick-trace -- --frames 200 --trace 8
//! ```
//!
//! The scenario below is **setup, not derivation**: unit placement, LOS values, who is
//! building what, and which unit shoots which are chosen so that every wired subsystem has
//! inputs. Nothing about the scenario claims to match a real match's opening. What the
//! output is evidence for is *execution* — that the ported systems run, in retail's order,
//! against shared state — and never for fidelity.

use don_sim::balance::BalanceTable;
use don_sim::order::{Order, OrderIndex};
use don_sim::systems::{ammo, casters_animals, production, walls};
use don_sim::tick::{Sim, NUM_STEPS};
use don_sim::world::{Handle, UnitTypeStats};

/// Two real `TypeIndex` values from the balance matrix (ids start at 50), so the damage
/// pipeline reads a real `Balance::final_balance_table` entry rather than a made-up one.
const TYPE_A: i32 = 50;
const TYPE_B: i32 = 51;

/// Where the fighters meet, in world Coord units.
const ARENA_X: i32 = 24_000;
const ARENA_Y: i32 = 24_000;

fn stats(type_id: i32, attack_x10: i32, armor: i32, hits: i32) -> UnitTypeStats {
    UnitTypeStats {
        type_id,
        attack: attack_x10,
        armor,
        hits,
        recharge: 15,
        max_range: 4 * 192,
        min_range: 0,
    }
}

fn build_scenario(seed: u64) -> Sim {
    // 64 WCoord cells square = 256 tiles = `world::MAP_SPAN`.
    let mut sim = Sim::new(seed, 64);

    // Shared rules: the real 493x493 matrix if it is checked out, plus two unit types.
    match BalanceTable::load_default() {
        Ok(t) => {
            sim.world.rules.balance = Some(std::sync::Arc::new(t));
            println!("balance: loaded schema/live/balance-real.bin");
        }
        Err(e) => println!("balance: NOT loaded ({e}) - the attack arm will not fire"),
    }
    sim.world.rules.unit_stats =
        std::sync::Arc::new(vec![stats(TYPE_A, 120, 3, 120), stats(TYPE_B, 90, 5, 160)]);
    // TYPE_B is a ranged shooter, so its attacks emit projectiles through
    // `Object::fire_ammo` and resolve at step 15 instead of in the object pass.
    sim.shooter_rules.push((
        TYPE_B,
        ammo::ShooterRules {
            to_hit: 90,
            attenuate: 2,
            ammo_per_att: 1,
            proj_speed: 6,
            target_size: 48,
            x_size: 1,
            y_size: 1,
            ..Default::default()
        },
    ));

    // Show the real balance entry and the damage it produces, so a zero row in the
    // matrix cannot be mistaken for a broken combat path.
    if let Some(b) = sim.world.rules.balance.clone() {
        for (a, d) in [(TYPE_A, TYPE_B), (TYPE_B, TYPE_B), (TYPE_B, TYPE_A)] {
            println!("balance[{a}][{d}] = {:?}", b.get(a, d));
        }
    }

    let mut fighters: Vec<(usize, Handle)> = Vec::new();
    for who in 0..4usize {
        sim.activate(who);
        // The econ block starts with income so `Leader::do_gather` has something to move.
        let l = &mut sim.leaders[who];
        l.econ.age = 1;
        l.econ.gross = [640, 480, 320, 160, 96, 0];
        l.dirty = false;
        l.last_calc_frame = -1;

        let base_x = 4000 + (who as i32) * 9000;
        let base_y = 4000 + (who as i32 % 2) * 9000;

        // A construction site plus two builders, so `Wall::do_construct`'s helper divisor
        // sees builders arrive in the object traversal's rotated order.
        let mut bd = production::BuildData {
            constr_time: 900,
            myhits: 600,
            ..Default::default()
        };
        bd.flags |= production::flag::VALID | production::flag::STARTED;
        let site = sim.spawn_build(who, bd);
        for k in 0..2i32 {
            let h = sim
                .spawn_unit(who, TYPE_A, base_x + k * 200, base_y, 3)
                .unwrap();
            let mut o = Order::default();
            o.kind = OrderIndex::BuildAt;
            o.target_who = who as i8;
            o.target_o = site as i16;
            sim.issue(h, o);
        }

        // Two wall segments.
        for _ in 0..2 {
            let mut w = walls::WallState::default();
            w.flags |= 1;
            sim.spawn_wall(who, w);
        }

        // Six movers with real destinations, so the pathfinder and `Unit::move_step` run.
        for k in 0..6i32 {
            let x = base_x + 600 + k * 320;
            let y = base_y + 900;
            let h = sim.spawn_unit(who, TYPE_A, x, y, 5).unwrap();
            sim.issue(h, Order::move_to(x + 5_000, y + 3_500, 96));
        }

        // Two fighters per player, all four players ringed around one arena so every
        // pair starts inside `max_range` and the combat path fires on frame 0.
        for k in 0..2i32 {
            let x = ARENA_X + (who as i32 - 2) * 300 + k * 90;
            let y = ARENA_Y + (who as i32 % 2) * 240;
            let h = sim.spawn_unit(who, TYPE_B, x, y, 4).unwrap();
            fighters.push((who, h));
        }
    }

    // Point each player's fighters at the next player's, addressed the way the engine
    // addresses an object: `(who, o)`, where `o` is the index in that owner's unit band.
    // Each owner spawns 2 builders, 6 movers, then 2 fighters, so the fighters are o=8,9.
    let n = fighters.len();
    for i in 0..n {
        let (_, h) = fighters[i];
        let (twho, _) = fighters[(i + 2) % n];
        let mut o = Order::default();
        o.kind = OrderIndex::Attack;
        o.target_who = twho as i8;
        o.target_o = (8 + ((i + 2) % n) % 2) as i16;
        sim.issue(h, o);
    }

    // Two herds, so the `frame % 64` arm of `Objects::process_all` has a live slot.
    for k in 0..2 {
        sim.herds.push(casters_animals::HerdData {
            cx: 20 + k * 5,
            cy: 20,
            type_id: 0,
            herd_flags: 1,
            ..Default::default()
        });
    }

    // One volley pre-seeded so step 15 has work on frame 0 as well; after that the
    // combat path keeps the pool fed through `Object::fire_ammo`.
    for k in 0..4i32 {
        let shooter = ammo::ObjView {
            alive: true,
            is_unit: true,
            x: 5000 + k * 400,
            y: 5000,
            ..Default::default()
        };
        let target = ammo::ObjView {
            alive: true,
            is_unit: true,
            x: 5000 + k * 400,
            y: 8000,
            ..Default::default()
        };
        let ord = ammo::LaunchOrder {
            gpiece: 1,
            start: ammo::SpawnPoint {
                x: shooter.x,
                y: shooter.y,
                z: 40,
            },
            who: 0,
            o: 0,
            whom: 1,
            ox: 1,
            angle: 0,
            cosmetic: false,
        };
        sim.launch_ammo(&ord, &shooter, Some(&target), 3000, 11);
    }

    sim
}

fn main() {
    let mut frames = 200usize;
    let mut trace = 6usize;
    let mut seed = 0x5EEDu64;
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--frames" | "-n" => {
                frames = args[i + 1].parse().unwrap_or(frames);
                i += 2;
            }
            "--trace" => {
                trace = args[i + 1].parse().unwrap_or(trace);
                i += 2;
            }
            "--seed" => {
                seed = args[i + 1].parse().unwrap_or(seed);
                i += 2;
            }
            "--help" | "-h" => {
                println!("don-tick-trace [--frames N] [--trace N] [--seed N]");
                return;
            }
            _ => i += 1,
        }
    }

    let mut sim = build_scenario(seed);
    println!(
        "world: {} units, {} buildings, {} walls, {} projectiles, {} herds\n",
        sim.world.live_count(),
        sim.builds.len(),
        sim.walls.len(),
        sim.ammo.live(),
        sim.herds.len()
    );

    println!("Game::do_frame 0x00591EF0 - per-tick trace");
    println!("  X executed   o ported-but-vacuous   . unimplemented   - out of scope");
    println!("       step:   {}", ruler());
    let mut last = None;
    for f in 0..frames {
        let t = sim.do_frame();
        if f < trace || f + 1 == frames {
            println!("  {}", t.line());
        } else if f == trace && frames > trace + 1 {
            println!("  ...");
        }
        last = Some(t);
    }
    println!();
    if let Some(t) = last {
        println!("{}", t.render());
    }
    println!("{}", sim.coverage_report());
    println!(
        "channel digest after {frames} frames: 0x{:016x}",
        sim.channel_digest()
    );
    println!(
        "Game::frame = {}, Game::seconds = {}",
        sim.world.frame, sim.world.seconds
    );
}

/// A `0123456789...` ruler the trace glyphs line up under.
fn ruler() -> String {
    (0..NUM_STEPS)
        .map(|i| char::from_digit((i % 10) as u32, 10).unwrap())
        .collect()
}
