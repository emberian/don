//! Does turning the order branch into a partition actually pay?
//!
//! Four implementations of one tick over the same arena, all asserted bit-identical before a
//! single timing is printed:
//!
//! - `virt` — one indirect call per entity through the 28-entry order table. The direct port
//!   of the engine's virtual `Order` dispatch.
//! - `match` — one `match` per entity. The obvious Rust rewrite, and strictly kinder to the
//!   branch predictor than the indirect call, so it is the *fair* baseline.
//! - `part` — partition the whole batch by order once, then 28 dense kernels.
//! - `pworld` — partition each world separately. Same answer, `worlds` times smaller buckets.
//!   This is the "one lane per world" question, asked as a measurement.
//!
//! Order mixes matter more than anything else here, so all three are swept: `uniform`
//! (maximum divergence), `skewed` (a realistic army roster, *easier* for a predictor), and
//! `single` (one archetype — a perfectly predicted branch, the honest floor on any speedup).

use don_gpu::arena::{Arena, Mix, PhaseA, Resolve, StepPlan};
use don_gpu::orders::{Order, ORDER_COUNT, ORDER_NAMES};
use std::time::{Duration, Instant};

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

/// Best of `reps`, on a freshly built arena each time. Best-of because laptop noise is
/// one-sided and we are after the machine's capability.
fn best(
    reps: usize,
    mut build: impl FnMut() -> Arena,
    plan: StepPlan,
    frames: usize,
) -> (u64, Duration) {
    let mut t = Duration::MAX;
    let mut digest = 0;
    for _ in 0..reps {
        let mut a = build();
        let t0 = Instant::now();
        for _ in 0..frames {
            a.step(plan);
        }
        let dt = t0.elapsed();
        t = t.min(dt);
        digest = a.digest();
    }
    (digest, t)
}

fn main() {
    let quick = std::env::args().any(|a| a == "--quick");
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    println!("don-gpu orderbench — archetype partitioning vs per-entity dispatch");
    println!("host threads: {threads}\n");

    let frames = if quick { 40 } else { 200 };
    let cfgs: &[(u32, u32)] = if quick {
        &[(64, 64)]
    } else {
        // (worlds, entities per world). Same total population along the diagonal, so the
        // world-count effect is separable from the entity-count effect.
        &[
            (1, 4096),
            (16, 256),
            (64, 64),
            (256, 64),
            (1024, 64),
            (4096, 64),
            (64, 1024),
            (256, 256),
        ]
    };
    let mixes: &[(&str, Mix)] = &[
        ("uniform", Mix::Uniform),
        ("skewed", Mix::Skewed),
        ("single", Mix::Single(Order::Attack)),
    ];

    section_dispatch(cfgs, mixes, frames);
    section_threads(threads, frames, quick);
    section_resolve(threads, frames, quick);
    section_histogram();
}

fn section_dispatch(cfgs: &[(u32, u32)], mixes: &[(&str, Mix)], frames: usize) {
    println!("== 1. phase A: dispatch strategy, single-threaded ==");
    println!(
        "{:<9} {:>7} {:>7} {:>10} {:>9} {:>9} {:>9} {:>9} {:>11} {:>11}",
        "mix",
        "worlds",
        "ents/w",
        "Ment-steps",
        "virt ms",
        "match ms",
        "part ms",
        "pworld ms",
        "part/virt",
        "part/match"
    );
    for &(name, mix) in mixes {
        for &(worlds, per) in cfgs {
            let build = || {
                let mut a = Arena::new(worlds, per);
                a.populate(per, mix, 0x5EED);
                a
            };
            let total = (worlds as u64) * (per as u64) * frames as u64;
            let plans = [
                StepPlan {
                    phase_a: PhaseA::Virtual,
                    resolve: Resolve::Sequential,
                },
                StepPlan {
                    phase_a: PhaseA::Match,
                    resolve: Resolve::Sequential,
                },
                StepPlan {
                    phase_a: PhaseA::Partitioned,
                    resolve: Resolve::Sequential,
                },
                StepPlan {
                    phase_a: PhaseA::PartitionedPerWorld,
                    resolve: Resolve::Sequential,
                },
            ];
            let mut out = Vec::new();
            let mut want = None;
            for p in plans {
                let (d, t) = best(3, build, p, frames);
                match want {
                    None => want = Some(d),
                    Some(w) => assert_eq!(d, w, "{p:?} changed the state at {worlds}x{per} {name}"),
                }
                out.push(t);
            }
            println!(
                "{:<9} {:>7} {:>7} {:>10.1} {:>9.1} {:>9.1} {:>9.1} {:>9.1} {:>10.2}x {:>10.2}x",
                name,
                worlds,
                per,
                total as f64 / 1e6,
                ms(out[0]),
                ms(out[1]),
                ms(out[2]),
                ms(out[3]),
                ms(out[0]) / ms(out[2]),
                ms(out[1]) / ms(out[2]),
            );
        }
        println!();
    }
    println!(
        "`part/match` is the honest speedup: `match` is already a friendlier baseline than the\n\
         engine's virtual call, so any win over it is a win over layout alone.\n"
    );
}

fn section_threads(threads: usize, frames: usize, quick: bool) {
    println!("== 2. scaling the partitioned path across threads ==");
    println!(
        "(`per-frame` spawns a scope every frame; `run_par` spawns once for the whole rollout.\n\
         `docs/derivation/simd-batch.md` measured an empty 12-worker scope at 126.6 us against a\n\
         frame costing microseconds, so the first column is expected to be bad — it is here to\n\
         show how bad, because it is the shape an interactive stepper is tempted into.)"
    );
    println!(
        "{:<8} {:>7} {:>7} {:>10} {:>12} {:>10} {:>9} {:>12}",
        "mix", "worlds", "ents/w", "1t ms", "per-frame ms", "run_par ms", "speedup", "Ment-steps/s"
    );
    let cfgs: &[(u32, u32)] = if quick {
        &[(256, 64)]
    } else {
        &[(64, 64), (256, 64), (1024, 64), (4096, 64), (256, 256)]
    };
    for &(name, mix) in &[("uniform", Mix::Uniform), ("skewed", Mix::Skewed)] {
        for &(worlds, per) in cfgs {
            let build = || {
                let mut a = Arena::new(worlds, per);
                a.populate(per, mix, 0x5EED);
                a
            };
            let (d1, t1) = best(
                3,
                build,
                StepPlan {
                    phase_a: PhaseA::Partitioned,
                    resolve: Resolve::Sequential,
                },
                frames,
            );
            let (dn, tn) = best(
                3,
                build,
                StepPlan {
                    phase_a: PhaseA::PartitionedParallel(threads),
                    resolve: Resolve::SegmentedParallel(threads),
                },
                frames,
            );
            assert_eq!(
                d1, dn,
                "thread count changed the state at {worlds}x{per} {name}"
            );

            // Spawn once for the whole rollout instead of once per frame.
            let plan = StepPlan {
                phase_a: PhaseA::Partitioned,
                resolve: Resolve::Sequential,
            };
            let mut tr = Duration::MAX;
            let mut dr = 0;
            for _ in 0..3 {
                let mut a = build();
                let t0 = Instant::now();
                a.run_parallel(frames, threads, plan);
                tr = tr.min(t0.elapsed());
                dr = a.digest();
            }
            assert_eq!(
                d1, dr,
                "run_parallel changed the state at {worlds}x{per} {name}"
            );

            let steps = (worlds as f64) * (per as f64) * frames as f64;
            println!(
                "{:<8} {:>7} {:>7} {:>10.1} {:>12.1} {:>10.1} {:>8.2}x {:>12.1}",
                name,
                worlds,
                per,
                ms(t1),
                ms(tn),
                ms(tr),
                ms(t1) / ms(tr),
                steps / (ms(tr) / 1e3) / 1e6,
            );
        }
    }
    println!();
}

fn section_resolve(threads: usize, frames: usize, quick: bool) {
    println!("== 3. conflict resolution: sequential vs segmented reduce ==");
    println!(
        "(phase A held at `part`, so this isolates the many-to-one resolution cost.\n\
         `seq` is the sequential reference and *defines* the answer; the others must match it.)"
    );
    println!(
        "{:<8} {:>7} {:>7} {:>9} {:>9} {:>12} {:>10}",
        "mix", "worlds", "ents/w", "seq ms", "segm ms", "segm-par ms", "seq/par"
    );
    let cfgs: &[(u32, u32)] = if quick {
        &[(256, 64)]
    } else {
        &[(64, 64), (1024, 64), (256, 256), (64, 1024)]
    };
    for &(worlds, per) in cfgs {
        let build = || {
            let mut a = Arena::new(worlds, per);
            a.populate(per, Mix::Uniform, 0x5EED);
            a
        };
        let mut ts = Vec::new();
        let mut want = None;
        for r in [
            Resolve::Sequential,
            Resolve::Segmented,
            Resolve::SegmentedParallel(threads),
        ] {
            let (d, t) = best(
                3,
                build,
                StepPlan {
                    phase_a: PhaseA::Partitioned,
                    resolve: r,
                },
                frames,
            );
            match want {
                None => want = Some(d),
                Some(w) => assert_eq!(d, w, "{r:?} changed the state at {worlds}x{per}"),
            }
            ts.push(t);
        }
        println!(
            "{:<8} {:>7} {:>7} {:>9.1} {:>9.1} {:>12.1} {:>9.2}x",
            "uniform",
            worlds,
            per,
            ms(ts[0]),
            ms(ts[1]),
            ms(ts[2]),
            ms(ts[0]) / ms(ts[2]),
        );
    }
    println!();
}

fn section_histogram() {
    println!("== 4. what the partition actually looks like ==");
    for (name, mix) in [("uniform", Mix::Uniform), ("skewed", Mix::Skewed)] {
        let mut a = Arena::new(64, 256);
        a.populate(256, mix, 0x5EED);
        let h = a.order_histogram();
        let total: u32 = h.iter().sum();
        let nonempty = h.iter().filter(|&&c| c > 0).count();
        let mx = h.iter().copied().max().unwrap_or(0);
        println!(
            "\n-- {name}: {total} entities, {nonempty}/{ORDER_COUNT} buckets non-empty, largest {mx} ({:.0}%)",
            100.0 * mx as f64 / total as f64
        );
        let mut idx: Vec<usize> = (0..ORDER_COUNT).collect();
        idx.sort_by_key(|&i| std::cmp::Reverse(h[i]));
        for &i in idx.iter().take(6) {
            println!("   {:<18} {:>7}", ORDER_NAMES[i], h[i]);
        }
    }
    println!(
        "\nBucket size is the whole story for vectorisation: a 4096-entity bucket amortises its\n\
         kernel call, a 3-entity bucket does not. That is why partitioning across the batch and\n\
         partitioning per world are measured separately in section 1.\n"
    );
}
