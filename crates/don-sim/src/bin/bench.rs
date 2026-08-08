//! Throughput ceiling for the world layout and batch scheduler.
//!
//! Reports an UPPER BOUND, not a simulation speed: the systems stepped here are
//! placeholders (see the don-sim crate docs) that touch the state a real tick must touch
//! but compute no derived mechanic. Real systems only add work.
use don_sim::{Batch, World};
use std::time::Instant;

fn bench(label: &str, worlds: usize, units: usize, frames: usize, threads: usize) {
    let mut b = Batch::new(worlds, 0x5EED);
    b.populate(units);
    // warm up so we measure steady state, not first-touch page faults
    for _ in 0..20 {
        b.step_parallel(threads);
    }
    let t0 = Instant::now();
    for _ in 0..frames {
        b.step_parallel(threads);
    }
    let dt = t0.elapsed().as_secs_f64();

    let world_steps = (worlds * frames) as f64;
    let unit_steps = world_steps * units as f64;
    // 15 frames = 1 second of game time at normal speed
    let realtime_x = world_steps / 15.0 / dt;
    println!(
        "{label:<28} {worlds:>5} worlds x {units:>5} units, {threads:>2}t  \
         {:>9.2} Ms/s world-steps  {:>9.2} Ms/s unit-steps  {:>10.0}x realtime  (digest {:#018x})",
        world_steps / dt / 1e6,
        unit_steps / dt / 1e6,
        realtime_x,
        b.digest()
    );
}

fn main() {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    println!("don-bench — UPPER BOUND on layout/scheduler throughput (placeholder systems)\n");
    let mut w = World::new(1);
    for _ in 0..512 {
        w.spawn(0);
    }
    let t0 = Instant::now();
    for _ in 0..10_000 {
        w.step();
    }
    let dt = t0.elapsed().as_secs_f64();
    println!(
        "{:<28} {:>5} world  x {:>5} units,  1t  {:>9.2} Ms/s world-steps  {:>10.0}x realtime\n",
        "single world", 1, 512, 10_000.0 / dt / 1e6, 10_000.0 / 15.0 / dt
    );

    bench("batch, 1 thread", 256, 256, 400, 1);
    bench("batch, all threads", 256, 256, 400, threads);
    bench("many small worlds", 4096, 64, 200, threads);
    bench("few large worlds", 64, 4096, 200, threads);
}
