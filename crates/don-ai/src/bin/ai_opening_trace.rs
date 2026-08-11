//! Accepted production-decision traces for the human-envelope analysis.
//!
//! This is deliberately an opening/economy probe, not a match evaluator.  Each policy
//! plays beside an inert leader on the same deterministic Arena maps, in both seats.  A
//! row is emitted only after [`World::submit`] accepts a `Queue` or `Build` command, so
//! repeated unaffordable attempts cannot masquerade as decisions the simulated game made.
//!
//! The `ShippedOpening` label means the purchase order in shipped `economic.bhs` cases
//! 6..18.  It does *not* mean that retail AI decisions were recovered from a replay.

use don_ai::arena::bots::ai::Ai;
use don_ai::arena::bots::boom::ShippedOpening;
use don_ai::arena::bots::Bot;
use don_ai::arena::cmd::Cmd;
use don_ai::arena::map::MapParams;
use don_ai::arena::match_run::{load_world, MatchConfig};
use don_ai::arena::obs::Obs;
use don_ai::arena::world::{World, FPS};
use don_ai::orders::OrderResult;

#[derive(Default)]
struct Silent;

impl Bot for Silent {
    fn name(&self) -> String {
        "Silent".into()
    }

    fn act(&mut self, _obs: &Obs, _out: &mut Vec<Cmd>) {}
}

fn value(args: &[String], key: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == key)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn evaluation_seed(index: u32) -> u32 {
    0x5EED_0001u32.wrapping_add(index.wrapping_mul(0x9E37_79B9))
}

fn policy(label: &str) -> Box<dyn Bot> {
    match label {
        "Ai" => Box::<Ai>::default(),
        "ShippedOpening" => Box::<ShippedOpening>::default(),
        _ => unreachable!("closed policy set"),
    }
}

fn production_row(cmd: Cmd, world: &World) -> Option<(i32, String, String)> {
    let type_id = match cmd {
        Cmd::Queue { type_id, .. } | Cmd::Build { type_id, .. } => type_id,
        _ => return None,
    };
    let ty = world.types.get(type_id)?;
    let kind = if ty.kind_building {
        "building"
    } else if ty.kind_unit {
        "unit"
    } else {
        "tech"
    };
    Some((type_id, kind.into(), ty.name.clone()))
}

fn run(label: &str, seed: u32, seat: usize, minutes: i64) -> Result<(), String> {
    let cfg = MatchConfig {
        minutes,
        map: MapParams {
            seed,
            ..MapParams::default()
        },
        logging: false,
        ..MatchConfig::default()
    };
    let mut world = load_world(&cfg)?;
    let mut bots: Vec<Box<dyn Bot>> = if seat == 0 {
        vec![policy(label), Box::<Silent>::default()]
    } else {
        vec![Box::<Silent>::default(), policy(label)]
    };
    let mut commands = Vec::new();
    let limit = minutes * 60 * FPS;

    while world.frame < limit && world.players[seat].alive {
        let period = bots[seat].decide_period().max(1);
        if world.frame % period == seat as i64 % period {
            commands.clear();
            {
                let obs = Obs::of(&world, seat);
                bots[seat].act(&obs, &mut commands);
            }
            for cmd in commands.drain(..) {
                let row = production_row(cmd, &world);
                let before = row
                    .as_ref()
                    .map(|(type_id, _, _)| world.count_type(seat, *type_id, true));
                let result = world.submit(seat as u8, cmd);
                if let (Some((type_id, kind, name)), Some(before), OrderResult::Ok(_)) =
                    (row, before, result)
                {
                    let accepted = world.count_type(seat, type_id, true) - before;
                    if accepted <= 0 {
                        return Err(format!(
                            "accepted production command did not increase type {type_id}"
                        ));
                    }
                    println!(
                        "{label}\t{seed:#010x}\t{seat}\t{}\t{kind}\t{name}\t{accepted}",
                        world.frame
                    );
                }
            }
        }
        world.step();
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let minutes = value(&args, "--minutes")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(12);
    let seeds = value(&args, "--seeds")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(3);
    if !(1..=30).contains(&minutes) || !(1..=32).contains(&seeds) {
        eprintln!("ai-opening-trace requires --minutes 1..30 and --seeds 1..32");
        std::process::exit(2);
    }

    println!("don.ai.accepted-production-trace.v1");
    println!("meta\tminutes\t{minutes}\tseeds\t{seeds}\tfps\t{FPS}");
    println!("policy\tseed\tseat\tframe\tkind\ttype\tcount");
    for label in ["ShippedOpening", "Ai"] {
        for index in 0..seeds {
            let seed = evaluation_seed(index);
            for seat in 0..2 {
                if let Err(error) = run(label, seed, seat, minutes) {
                    eprintln!("ai-opening-trace failed: {error}");
                    std::process::exit(2);
                }
            }
        }
    }
}
