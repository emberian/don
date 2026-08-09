//! Throughput of the tick, and the layout ceiling above it.
//!
//! Two things are measured and they must not be confused:
//!
//! * **tick** rows run `World::step`, i.e. `Game::do_frame` `0x00591EF0`: the ordered
//!   29-step schedule, the `(frame + i) % 10` owner rotation, and the per-unit
//!   `process -> work -> do_job -> do_move` chain with real order dispatch. This is a
//!   simulation speed, for the subsystems that exist.
//! * **hot** rows run `World::step_hot`, the element-wise subset (position integration
//!   plus the `hold_frames` decrement). That is an UPPER BOUND on what any layout can
//!   deliver, not a simulation speed — every unported subsystem only adds work.
//!
//! The lane-major experiment lives entirely in the second category, because a cross-world
//! SIMD layout can only help arithmetic without per-unit control flow.
//!
//! Every row runs to a **time budget** (default 400 ms, `DON_BENCH_BUDGET=<seconds>` to
//! change it) rather than a fixed frame count, and prints the wall time it actually used.
//! A configuration that got 20x faster during this lane would otherwise have finished in
//! two milliseconds and reported mostly timer noise.
//!
//! Modes:
//! * `step/frame` — one `step_parallel` per frame: includes a thread spawn+join per frame.
//! * `run`        — one `run_parallel` per chunk of frames: spawn cost amortised away.
//! * `hot run`    — the same, over the element-wise subset.
//! * `lane-major` — cross-world interleaved layout, one SIMD lane per world.
use don_sim::order::{Order, OrderIndex};
use don_sim::{simd, Batch, LaneBatch, World};
use std::time::{Duration, Instant};

fn budget() -> Duration {
    let secs = std::env::var("DON_BENCH_BUDGET")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.4);
    Duration::from_secs_f64(secs)
}

struct Run {
    frames: usize,
    dt: f64,
}

/// Repeat `chunk`-frame batches of work until the budget is spent.
fn measure(chunk: usize, mut work: impl FnMut(usize)) -> Run {
    let budget = budget();
    let mut frames = 0usize;
    let t0 = Instant::now();
    while t0.elapsed() < budget {
        work(chunk);
        frames += chunk;
    }
    Run {
        frames,
        dt: t0.elapsed().as_secs_f64(),
    }
}

fn row(label: &str, worlds: usize, units: usize, threads: usize, mode: &str, r: &Run, digest: u64) {
    let world_steps = (worlds * r.frames) as f64;
    let unit_steps = world_steps * units as f64;
    println!(
        "{label:<26} {worlds:>5}w x{units:>5}u {threads:>3}t {mode:<11} \
         {:>8.2} Ms/s world {:>9.1} Ms/s unit {:>9.0}x rt  {:>7} frames in {:>5.0} ms  {digest:#018x}",
        world_steps / r.dt / 1e6,
        unit_steps / r.dt / 1e6,
        world_steps / 15.0 / r.dt,
        r.frames,
        r.dt * 1e3,
    );
}

/// `capacity = 0` provisions every world to the hard cap, which is what the batch did
/// before this lane; anything else right-sizes it to the population.
/// Populate a batch and give every unit a live `MOVE_TO` order, so the tick exercises the
/// jump table instead of idling.
///
/// A batch of orderless units dispatches arm 0 every frame and measures almost nothing;
/// the destination is deliberately far enough away that no unit arrives inside a run.
fn populate_busy(b: &mut Batch, units: usize) {
    b.populate(units);
    for w in b.worlds.iter_mut() {
        for row in 0..w.live_count() as usize {
            w.units.myspeed_mut()[row] = 24;
            let far = don_sim::world::MAP_SPAN / 2;
            w.orders_mut(row).replace(Order {
                kind: OrderIndex::MoveTo,
                x: far,
                y: far,
                tolerance: 1,
                ..Order::default()
            });
        }
    }
}

fn bench(
    label: &str,
    worlds: usize,
    units: usize,
    threads: usize,
    capacity: usize,
    per_frame: bool,
) {
    let cap = if capacity == 0 {
        don_sim::MAX_UNITS
    } else {
        capacity
    };
    let mut b = Batch::with_capacity(worlds, 0x5EED, cap);
    populate_busy(&mut b, units);
    b.run_parallel(20, threads); // warm up: steady state, not first-touch page faults
    let r = if per_frame {
        measure(1, |_| b.step_parallel(threads))
    } else {
        measure(256, |f| b.run_parallel(f, threads))
    };
    row(
        label,
        worlds,
        units,
        threads,
        if per_frame { "step/frame" } else { "run" },
        &r,
        b.digest(),
    );
}

/// The element-wise ceiling: same shapes, `World::step_hot`.
fn bench_hot(label: &str, worlds: usize, units: usize, threads: usize, capacity: usize) {
    let cap = if capacity == 0 {
        don_sim::MAX_UNITS
    } else {
        capacity
    };
    let mut b = Batch::with_capacity(worlds, 0x5EED, cap);
    b.populate(units);
    b.energise();
    b.run_hot_parallel(20, threads);
    let r = measure(256, |f| b.run_hot_parallel(f, threads));
    row(label, worlds, units, threads, "hot run", &r, b.digest());
}

fn bench_lanes(label: &str, worlds: usize, units: usize, threads: usize) {
    let mut b = Batch::with_capacity(worlds, 0x5EED, units.max(1));
    b.populate(units);
    b.energise();
    let mut lanes = LaneBatch::from_batch(&b);
    lanes.run_parallel(20, threads);
    let r = measure(256, |f| lanes.run_parallel(f, threads));
    // Written back so the digest is comparable with the per-world rows. The transpose is
    // outside the timed region: it is a one-off compile step, and this note is here so
    // that exclusion is visible rather than buried.
    lanes.write_back(&mut b);
    row(label, worlds, units, threads, "lane-major", &r, b.digest());
}

/// Compare the three kernel forms on one world.
///
/// Measured **twice, in opposite orders**, because this harness has a position bias worth
/// about 3-4%: on hbox, two entry points to bit-identical SSE2 code measured 3692 and 3552
/// Ms/s purely because one ran first. A 3% difference between two paths in a single-order
/// run is therefore not a result. Read the pair, not either row alone.
fn single_world(units: usize) {
    // All three are `step_hot` variants: the point is the kernel, not the tick.
    let paths: [(&str, u8); 3] = [
        ("hot shipped", 0u8),
        ("hot hand simd", 1),
        ("hot scalar branchy", 2),
    ];
    for (pass, order) in [("pass A", [0usize, 1, 2]), ("pass B", [2usize, 1, 0])] {
        for i in order {
            let (name, path) = paths[i];
            let mut w = World::with_capacity(units, 1);
            for _ in 0..units {
                w.spawn(0);
            }
            for row in 0..w.live_count() as usize {
                w.set_move_step(row, (row as i32 % 251) - 125, (row as i32 % 197) - 98);
            }
            for _ in 0..64 {
                w.step_hot();
            }
            let r = measure(64, |f| {
                for _ in 0..f {
                    match path {
                        0 => w.step_hot(),
                        1 => w.step_hand_simd(),
                        _ => w.step_scalar_reference(),
                    }
                }
            });
            row(
                &format!("{pass}, {name}"),
                1,
                units,
                1,
                "serial",
                &r,
                w.digest(),
            );
        }
    }
}

/// Every mode must produce the same state from the same start, or the fast ones are fast
/// at something else. The unit tests assert this too; it is repeated here so a benchmark
/// run cannot report a speedup that came from doing different work.
fn cross_check(worlds: usize, units: usize, frames: usize, threads: usize) {
    let fresh = || {
        let mut b = Batch::with_capacity(worlds, 0x5EED, units.max(1));
        populate_busy(&mut b, units);
        b
    };
    let mut serial = fresh();
    serial.run_serial(frames);
    let want = serial.digest();

    let mut per_frame = fresh();
    for _ in 0..frames {
        per_frame.step_parallel(threads);
    }
    let mut run = fresh();
    run.run_parallel(frames, threads);

    println!(
        "cross-check TICK {worlds}w x {units}u over {frames} frames — serial {want:#018x}  \
         step/frame {:#018x}  run {:#018x}   {}",
        per_frame.digest(),
        run.digest(),
        if per_frame.digest() == want && run.digest() == want {
            "ALL AGREE"
        } else {
            "*** DIVERGED ***"
        }
    );

    // The lane-major layout mirrors `step_hot`, so it is cross-checked against that and
    // never against the tick. `energise` is required or the comparison is vacuous.
    let fresh_hot = || {
        let mut b = Batch::with_capacity(worlds, 0x5EED, units.max(1));
        b.populate(units);
        b.energise();
        b
    };
    let mut hot_ref = fresh_hot();
    let start = hot_ref.digest();
    hot_ref.run_hot_serial(frames);
    let hot_want = hot_ref.digest();
    let mut laneb = fresh_hot();
    let mut lanes = LaneBatch::from_batch(&laneb);
    lanes.run_parallel(frames, threads);
    lanes.write_back(&mut laneb);
    println!(
        "cross-check HOT  {worlds}w x {units}u over {frames} frames — serial {hot_want:#018x}  \
         lane-major {:#018x}   {}{}",
        laneb.digest(),
        if laneb.digest() == hot_want {
            "ALL AGREE"
        } else {
            "*** DIVERGED ***"
        },
        if hot_want == start {
            "  *** VACUOUS: state did not move ***"
        } else {
            ""
        }
    );
}

fn provisioning(worlds: usize, units: usize) {
    let full = Batch::with_capacity(1, 0, don_sim::MAX_UNITS).worlds[0].bytes_reserved();
    let right = Batch::with_capacity(1, 0, units).worlds[0].bytes_reserved();
    println!(
        "provisioning {worlds}w x {units}u — full capacity {:>8.1} MiB total ({full} B/world), \
         rightsized {:>6.1} MiB total ({right} B/world)",
        (full * worlds) as f64 / (1024.0 * 1024.0),
        (right * worlds) as f64 / (1024.0 * 1024.0),
    );
}

/// What a tick run actually executed. Printed so a throughput number is never read
/// without the coverage that produced it.
fn coverage_report(worlds: usize, units: usize, frames: usize) {
    let mut b = Batch::with_capacity(worlds, 0x5EED, units.max(1));
    populate_busy(&mut b, units);
    b.run_serial(frames);
    let mut cov = don_sim::world::Coverage::default();
    for w in b.worlds.iter() {
        cov.merge(w.coverage());
    }
    let (imp, stub, oos) = don_sim::schedule::ScheduleCoverage::tally();
    let (df, wf, db, wb) = World::digest_field_coverage();
    println!(
        "coverage over {worlds}w x {units}u x {frames}f: \
         do_frame steps {imp} implemented / {stub} stub / {oos} out-of-scope of {}; \
         order arms {}/{} dispatched-with-behaviour ({:.1}% of {} dispatches); \
         Unit::process {}, Guy::move {}; \
         digest covers {df}/{wf} walked UnitData fields ({db}/{wb} B); \
         RNG-divergent wildlife frames skipped: {}",
        don_sim::schedule::DO_FRAME.len(),
        cov.orders.hot().len(),
        don_sim::order::NUM_UNIT_ORDERS,
        cov.orders.covered_fraction() * 100.0,
        cov.orders.total,
        cov.unit_process,
        cov.guy_move,
        cov.wildlife_draws_skipped,
    );
}

fn main() {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    println!(
        "don-bench — tick throughput (World::step = Game::do_frame) and the element-wise ceiling"
    );
    println!(
        "kernels: shipped = {}, hand = {}   threads: {threads}   budget: {:.2}s per row\n",
        simd::shipped_path(),
        simd::hand_path(),
        budget().as_secs_f64()
    );

    cross_check(37, 48, 120, threads);
    cross_check(4096, 64, 64, threads);
    provisioning(4096, 64);
    provisioning(256, 256);
    coverage_report(37, 48, 120);
    println!();

    single_world(512);
    println!();

    // The four configurations the pre-lane benchmark reported, unchanged in shape so the
    // before/after numbers are comparable: full-capacity worlds, one join per frame.
    bench("batch, 1 thread", 256, 256, 1, 0, true);
    bench("batch, all threads", 256, 256, threads, 0, true);
    bench("many small worlds", 4096, 64, threads, 0, true);
    bench("few large worlds", 64, 4096, threads, 0, true);
    println!();

    // Same work, one spawn per rollout chunk instead of one per frame.
    bench("batch, run", 256, 256, threads, 0, false);
    bench("many small, run", 4096, 64, threads, 0, false);
    bench("few large, run", 64, 4096, threads, 0, false);
    println!();

    // The element-wise ceiling, same shapes. Everything above is bounded by these.
    bench_hot("batch, hot", 256, 256, threads, 0);
    bench_hot("many small, hot", 4096, 64, threads, 0);
    bench_hot("few large, hot", 64, 4096, threads, 0);
    println!();

    // Same again with worlds provisioned for the population they actually hold.
    bench("batch, run, rightsized", 256, 256, threads, 256, false);
    bench("many small, rightsized", 4096, 64, threads, 64, false);
    bench("few large, rightsized", 64, 4096, threads, 4096, false);
    println!();

    // Cross-world lane-major layout (one SIMD lane = one world).
    bench_lanes("batch, lane-major", 256, 256, threads);
    bench_lanes("many small, lane-major", 4096, 64, threads);
    bench_lanes("few large, lane-major", 64, 4096, threads);
    println!();

    // Lane-major groups four worlds into one work item, so at low world counts it also
    // coarsens the parallel schedule. Repeat the worst case at a thread count where both
    // layouts divide evenly (64 worlds / 16 groups over 4 threads), separating the layout
    // effect from the scheduling effect.
    bench("few large, run", 64, 4096, 4, 4096, false);
    bench_lanes("few large, lane-major", 64, 4096, 4);
    println!();

    // Thread scaling on one configuration. This machine is 8 performance + 4 efficiency
    // cores, so the last four threads are not the same as the first eight.
    for t in [1usize, 2, 4, 8, threads] {
        bench("scaling, batch run", 256, 256, t, 256, false);
    }
}
