//! Run step 8 of `Game::do_frame` — `Leaders::process_all` `0x006ED2A0` — for real, and
//! print what executed.
//!
//! ```sh
//! cargo run -p don-sim --example step8 -- 27000
//! ```
//!
//! This exists because `docs/mechanics/COVERAGE.md` §5 names *derived code without proven
//! execution* as the finding that dominates the others. Every number below is produced by
//! running the ported code, not asserted about it.

use don_sim::systems::economy::{NUM_RESOURCES, RES_NAMES};
use don_sim::systems::leaders::{
    flag, GraceTimer, RareMask, StatObject, Step8Driver, NUM_LEADER_SLOTS,
};

fn main() {
    let frames: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(27_000);

    let mut d = Step8Driver::new();

    for i in 0..NUM_LEADER_SLOTS {
        let l = &mut d.leaders.leaders[i];
        l.activate();
        // Everyone is a live threat to everyone else, and slots 0/1 are allied.
        l.flags |= flag::COUNTS_AS_HOSTILE;
        l.econ.age = 1;
        // A capital-loss debt for the odd slots, so the grace timers have work to do.
        if i % 2 == 1 {
            l.timers[0] = GraceTimer {
                value: -(60 * (i as i32)),
                frozen: 0,
            };
        }
        // A taunt every 900 frames.
        l.taunt_frame[0] = 900 * (1 + i as i32);
        l.taunt_kind[0] = i as i32;

        let e = &mut d.env.leaders[i];
        // The four object-graph loops `Leader::calc_gather` runs are not ported, so their
        // sum is supplied — the same boundary `economy::GatherInputs::object_income` names.
        e.gather.object_income = [
            3200 + 400 * i as i32,
            2400,
            1600,
            800,
            600 + 100 * i as i32,
            0,
        ];
        e.attrition.attrition_preqs = [true, i >= 2, i >= 4, i >= 6];
        e.attrition.colosseum = i == 3;
        e.attrition.anti_preqs = [i >= 1, i >= 3, i >= 5];
        e.attrition.mongol = i == 7;
        // A town of buildings, walls and units for the two stat passes to walk.
        e.objects.band_2000 = vec![
            StatObject {
                active: true,
                ..Default::default()
            };
            6
        ];
        e.objects.band_3000 = vec![
            StatObject {
                active: true,
                wall_active: true,
                ..Default::default()
            };
            10
        ];
        e.objects.units = vec![
            StatObject {
                active: true,
                captain: true,
                owner_in_game: true,
                hit_inputs: Some(don_sim::systems::leaders::ObjectHitInputs {
                    base_hits: 100,
                    ..Default::default()
                }),
                type_los: Some(4),
                ..Default::default()
            };
            24
        ];
    }
    d.leaders.leaders[0].diplo[1] = don_sim::systems::leaders::DIPLO_ALLIED;
    d.leaders.leaders[1].diplo[0] = don_sim::systems::leaders::DIPLO_ALLIED;

    let channel_before = d.econ_channel();
    let mut hostile_frames = 0u64;
    let mut rare_events = 0u64;

    for f in 0..frames {
        // Trade routes come and go: flip a rare on every leader on a slow beat, which is
        // the only thing in the engine that arms the two stat-dirty bits.
        if f % 4096 == 0 {
            for i in 0..NUM_LEADER_SLOTS {
                let bit = (f / 4096 + i) % 44;
                let mut m = d.leaders.leaders[i].rare_b;
                m.set(bit, !m.get(bit));
                d.leaders.leaders[i].rare_b = m;
            }
        }
        let t = d.frame();
        hostile_frames += t.hostile_seen.iter().filter(|b| **b).count() as u64;
        rare_events += t.rare_mask_changed.iter().filter(|b| **b).count() as u64;
    }

    let c = d.totals;
    println!("Leaders::process_all 0x006ED2A0 — {frames} frames, 8 leaders");
    println!("  frames                          {}", c.frames);
    println!("  Game::seconds reached           {}", d.seconds);
    println!("  leader-frames processed         {}", c.leader_frames);
    println!("  Leader::gather calls            {}", c.gathers);
    println!("  Leader::calc_gather recomputes  {}", c.gross_recomputes);
    println!("  rare-mask unions that changed   {rare_events}");
    println!("  Leader::calc_wall_stats passes  {}", c.wall_stat_passes);
    println!("  Leader::calc_unit_stats passes  {}", c.unit_stat_passes);
    println!("  process_elimination call sites  {}", c.elimination_calls);
    println!("  process_taunt dispatches        {}", c.taunts);
    println!("  grace-timer creeps              {}", c.timer_creeps);
    println!("  leader-frames with a hostile    {hostile_frames}");

    println!("\nwhole resources credited, all leaders:");
    for r in 0..NUM_RESOURCES {
        println!("  {:<10} {}", RES_NAMES[r], c.whole_resources_credited[r]);
    }

    println!("\nper-leader final state:");
    println!("  slot   food  timber  wealth   know   metal    attrition  anti");
    for i in 0..NUM_LEADER_SLOTS {
        let l = &d.leaders.leaders[i];
        println!(
            "  {:>4} {:>6} {:>7} {:>7} {:>6} {:>7} {:>12} {:>7.1}",
            i,
            l.econ.stockpile[0],
            l.econ.stockpile[1],
            l.econ.stockpile[2],
            l.econ.stockpile[3],
            l.econ.stockpile[4],
            l.attrition,
            l.anti_attrition
        );
    }

    println!(
        "\nmodelled leaders channel (economy::LeaderEcon::image over 8 blocks)\n  before 0x{:08x}  after 0x{:08x}",
        channel_before,
        d.econ_channel()
    );
    println!(
        "  NOT comparable to retail: LeaderData::walk_data 0x006D6750 walks 27,182 bytes,\n  economy::LeaderEcon::image emits {}.",
        don_sim::systems::economy::econ_offsets::MODELLED_LEN
    );
    println!("  titanium rare bit = {}", RareMask::TITANIUM);
}
