//! Head-to-head between the optimiser-derived opening player and the shipped
//! designers' boom order, inside the measured Ancient-age economy.
//!
//! ```sh
//! cargo run -p don-ai --example boom_arena
//! ```

use don_ai::optimum::econ::{mmss, Assume, World, FOOD, TIMBER, WEALTH};
use don_ai::optimum::player::{
    run_order, CapFirst, Goal, Run, ShippedBoom, SHIPPED_ORDER, SHIPPED_ORDER_SWAPPED,
};

fn show(label: &str, r: &Run) {
    println!(
        "== {label} -> {} ({} frames){}",
        mmss(r.frames),
        r.frames,
        if r.stalled { "  [STALLED]" } else { "" }
    );
    for (start, done, name) in r.order() {
        println!(
            "   {:>5} {:>5}  {:<20} (done {})",
            start,
            mmss(start),
            name,
            mmss(done)
        );
    }
    let w: &World = &r.world;
    println!(
        "   end: {} citizens ({} food / {} timber), {} farms, {} camps, {} cities, {} markets",
        w.built("Citizen"),
        w.on[FOOD],
        w.on[TIMBER],
        w.built("Farm"),
        w.built("Woodcutter's Camp"),
        w.built("Small City"),
        w.built("Market")
    );
    let rate = w.rate();
    println!(
        "   rate: food {} timber {} wealth {} per 30 s (cap {})",
        rate[FOOD],
        rate[TIMBER],
        rate[WEALTH],
        w.cap16()[FOOD] / 16
    );
    println!();
}

fn main() {
    let a = Assume::default();
    let g = Goal::default();
    let opt = CapFirst.run(a, g, 90_000);
    let ship = ShippedBoom.run(a, g, 90_000);
    show("CapFirst (optimiser-derived)", &opt);
    show("economic.bhs cases 6-18 (Sobota / Engle)", &ship);
    let swapped = run_order(SHIPPED_ORDER_SWAPPED, a, g, 90_000);
    show(
        "economic.bhs with case 6 <-> case 14 (Barter first)",
        &swapped,
    );
    // Ablations on the designers' own order.  Each variant buys exactly the same
    // multiset of things; only the ORDER changes, so any difference is a scheduling
    // decision and nothing else.
    const V_WW_LAST: &[&str] = &[
        "City State",
        "Citizen",
        "Citizen",
        "Citizen",
        "Citizen",
        "Farm",
        "Small City",
        "Citizen",
        "Citizen",
        "Farm",
        "Woodcutter's Camp",
        "Barter",
        "Market",
        "Citizen",
        "Citizen",
        "Citizen",
        "Farm",
        "Farm",
        "Classical Age",
        "Written Word",
    ];
    const V_TAIL_AFTER_AGE: &[&str] = &[
        "City State",
        "Citizen",
        "Citizen",
        "Citizen",
        "Citizen",
        "Farm",
        "Small City",
        "Citizen",
        "Citizen",
        "Farm",
        "Barter",
        "Market",
        "Citizen",
        "Citizen",
        "Citizen",
        "Farm",
        "Farm",
        "Classical Age",
        "Written Word",
        "Woodcutter's Camp",
    ];
    const V_FARM_FIRST: &[&str] = &[
        "Written Word",
        "City State",
        "Farm",
        "Citizen",
        "Citizen",
        "Citizen",
        "Citizen",
        "Small City",
        "Citizen",
        "Citizen",
        "Farm",
        "Woodcutter's Camp",
        "Barter",
        "Market",
        "Citizen",
        "Citizen",
        "Citizen",
        "Farm",
        "Farm",
        "Classical Age",
    ];
    let variants: [(&str, &[&str]); 5] = [
        ("shipped, as written", SHIPPED_ORDER),
        (
            "swap case 6 <-> case 14 (Barter first)",
            SHIPPED_ORDER_SWAPPED,
        ),
        ("Written Word moved to last", V_WW_LAST),
        ("Written Word AND camp #2 after the age", V_TAIL_AFTER_AGE),
        ("first Farm before the four citizens", V_FARM_FIRST),
    ];
    println!("ablations on the designers' order -- same multiset, different order:");
    for (label, ord) in variants {
        let r = run_order(ord, a, g, 90_000);
        println!(
            "   {:<40} {} ({} frames){}",
            label,
            mmss(r.frames),
            r.frames,
            if r.stalled { "  STALLED" } else { "" }
        );
    }
    println!();

    let d = ship.frames - opt.frames;
    println!(
        "gap: {} frames = {} ({:+.0}%)",
        d,
        mmss(d.abs()),
        100.0 * d as f64 / opt.frames as f64
    );
}
