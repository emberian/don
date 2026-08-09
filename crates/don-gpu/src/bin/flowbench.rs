//! Honest CPU-vs-GPU benchmark for the batched flow-field solve.
//!
//! Three sections, because there are two different fair questions and they have different
//! answers:
//!
//! A. **Kernel for kernel.** The same Jacobi relaxation on both devices. This is the
//!    apples-to-apples GPU comparison and the GPU flatters itself here.
//! B. **Engineering reality.** The GPU's Jacobi against the best CPU algorithm for the same
//!    problem, a bucket-queue Dijkstra that visits each cell a constant number of times.
//!    This is the comparison that decides whether the GPU goes in the trainer.
//! C. **Sync and blocking cost.** How `inner_steps` (temporal blocking) and
//!    `rounds_per_poll` (convergence-flag readback frequency) move the number.
//!
//! Every GPU result is checked against the CPU Dial result for bit equality before its
//! timing is reported, so a fast wrong answer cannot slip through.

use don_gpu::cpu::{solve_batch_parallel, solve_batch_serial, CpuKernel};
use don_gpu::field::FieldBatch;
use don_gpu::gpu::{Gpu, SolveOptions, SolveStats};
use std::time::{Duration, Instant};

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

fn time<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let t = Instant::now();
    let v = f();
    (v, t.elapsed())
}

/// Best of `reps` runs on a fresh clone each time. Best-of, not mean: we are after the
/// machine's capability, and the noise on a laptop is one-sided.
fn best(
    reps: usize,
    base: &FieldBatch,
    mut solve: impl FnMut(&mut FieldBatch),
) -> (FieldBatch, Duration) {
    let mut out = base.clone();
    let mut t = Duration::MAX;
    for _ in 0..reps {
        let mut b = base.clone();
        let (_, dt) = time(|| solve(&mut b));
        if dt < t {
            t = dt;
        }
        out = b;
    }
    (out, t)
}

const REPS: usize = 3;

struct Cfg {
    w: u32,
    h: u32,
    fields: u32,
}

fn main() {
    let quick = std::env::args().any(|a| a == "--quick");
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    println!("don-gpu flowbench — batched integer flow-field relaxation");
    println!("host threads: {threads}");

    let gpu = match Gpu::new() {
        Ok(g) => {
            println!(
                "adapter: {} ({:?}, {:?})",
                g.info.name, g.info.device_type, g.info.backend
            );
            println!(
                "limits: max_storage_buffer_binding_size {} MiB, max_buffer_size {} MiB, \
                 workgroup storage {} B",
                g.limits.max_storage_buffer_binding_size / (1 << 20),
                g.limits.max_buffer_size / (1 << 20),
                g.limits.max_compute_workgroup_storage_size
            );
            Some(g)
        }
        Err(e) => {
            println!("GPU unavailable ({e}) — CPU sections only");
            None
        }
    };
    // Warm up: the first dispatch in a process pays Metal/Vulkan pipeline compilation and
    // first-touch allocation. Paying it here keeps it out of every reported number.
    if let Some(g) = &gpu {
        let mut warm = FieldBatch::synth_terrain(64, 64, 8, 1);
        let mut s = g.solver(64, 64, 8);
        s.upload(&warm);
        s.run(SolveOptions::default());
        s.download(&mut warm);
        let mut dirs = vec![0u32; warm.total_cells()];
        s.directions(&mut dirs);
    }
    println!();

    let args: Vec<String> = std::env::args().collect();
    let pick = |s: &str| args.iter().any(|a| a == s);
    let all = !(pick("--a") || pick("--b") || pick("--c") || pick("--d") || pick("--e"));
    if all || pick("--a") {
        section_a(&gpu, threads, quick);
    }
    if all || pick("--b") {
        section_b(&gpu, threads, quick);
    }
    if all || pick("--c") {
        section_c(&gpu, quick);
    }
    if all || pick("--d") {
        section_d(&gpu, threads, quick);
    }
    if all || pick("--e") {
        section_e(&gpu, threads, quick);
    }
}

/// The convergence flag, measured both ways on the same code.
///
/// §4e of `docs/derivation/gpu-architecture.md` found the batch-wide flag makes every field
/// run for as long as the *slowest* field in the batch, and predicted up to a 23x cost. This
/// section runs the identical solve with `per_field_convergence` off and on, so the
/// prediction is either confirmed with a number or it is not.
fn section_e(gpu: &Option<Gpu>, threads: usize, quick: bool) {
    println!("== E. batch-wide convergence flag vs per-field flags + active-list compaction ==");
    let Some(g) = gpu else {
        println!("(no GPU)\n");
        return;
    };
    println!(
        "{:<10} {:>7} {:>8} {:>10} {:>10} {:>7} {:>10} {:>7} {:>12} {:>12} {:>9} {:>7}",
        "grid",
        "fields",
        "case",
        "batch ms",
        "field ms",
        "gain",
        "field@2 ms",
        "gain@2",
        "rounds b/f",
        "fieldrounds",
        "conv av/mx",
        "dial Nt"
    );
    // (grid, fields, perturbation). `local` reproduces §4e's worst case: a small change
    // makes most fields converge almost at once while a few run long, which is exactly
    // when a batch-wide flag hurts most.
    let cfgs: &[(u32, u32, u32, bool)] = if quick {
        &[(64, 64, 256, true)]
    } else {
        &[
            (64, 64, 256, true),
            (64, 64, 4096, true),
            (128, 128, 1024, true),
            (256, 256, 256, true),
            (256, 256, 256, false),
            (128, 128, 1024, false),
        ]
    };
    for &(w, h, fields, local) in cfgs {
        let base = FieldBatch::synth_terrain(w, h, fields, 0xC0FFEE);
        let mut warm = base.clone();
        solve_batch_serial(&mut warm, CpuKernel::Dial, 0);
        let mut perturbed = warm.clone();
        let mut cold = base.clone();
        for f in 0..fields {
            if local {
                for y in 2..6 {
                    for x in 2..6 {
                        let i = perturbed.index(f, x, y);
                        perturbed.cost[i] = 1;
                        cold.cost[i] = 1;
                    }
                }
            } else {
                let (gx, gy) = (w * 3 / 4, h * 3 / 4);
                let i = perturbed.index(f, gx, gy);
                perturbed.cost[i] = 1;
                perturbed.dist[i] = 0;
                cold.cost[i] = 1;
                cold.dist[i] = 0;
            }
        }
        let (reference, t_dial) = best(2, &cold, |b| {
            solve_batch_parallel(b, CpuKernel::Dial, 0, threads)
        });

        let mut solver = g.solver(w, h, fields);
        let mut run = |per_field: bool, poll: u32| {
            let opts = SolveOptions {
                per_field_convergence: per_field,
                rounds_per_poll: poll,
                ..SolveOptions::default()
            };
            let mut t = Duration::MAX;
            let mut best_stats = SolveStats::default();
            let mut out = perturbed.clone();
            for _ in 0..2 {
                out = perturbed.clone();
                solver.upload(&out);
                let st = solver.run(opts);
                solver.download(&mut out);
                if st.compute < t {
                    best_stats = st.clone();
                }
                t = t.min(st.compute);
            }
            assert_eq!(
                out.dist, reference.dist,
                "{w}x{h} x{fields} per_field={per_field} missed the fixed point"
            );
            (t, best_stats)
        };
        let (t_batch, s_batch) = run(false, 8);
        let (t_field, s_field) = run(true, 8);
        // A field can only leave the dispatch at a poll boundary, so `rounds_per_poll`
        // quantises how early it can drop out — the floor on the observable mean is the poll
        // size itself. Polling 4x more often trades readback stalls for a tighter fit.
        let (t_field2, _) = run(true, 2);
        let (av, mx) = s_field.convergence_mean_max();
        println!(
            "{:<10} {:>7} {:>8} {:>10.1} {:>10.1} {:>6.2}x {:>10.1} {:>6.2}x {:>12} {:>12} {:>9} {:>7.1}",
            format!("{w}x{h}"),
            fields,
            if local { "local" } else { "wide" },
            ms(t_batch),
            ms(t_field),
            ms(t_batch) / ms(t_field),
            ms(t_field2),
            ms(t_batch) / ms(t_field2),
            format!("{}/{}", s_batch.rounds, s_field.rounds),
            format!(
                "{:.2}x",
                s_field.field_rounds_uncompacted as f64 / s_field.field_rounds.max(1) as f64
            ),
            format!("{av:.0}/{mx}"),
            ms(t_dial),
        );
    }
    println!(
        "\n`case` is the perturbation: `local` = a 4x4 patch freed next to the goal, `wide` = a\n\
         second goal opened three quarters across. `rounds b/f` is global rounds with the\n\
         batch-wide flag vs with per-field flags; `fieldrounds` is how much less field-work the\n\
         compacted run did; `conv av/mx` is the round each field converged at, averaged and\n\
         maximised — the gap between those two *is* the waste the batch-wide flag was paying.\n"
    );
}

/// Warm start: what a real tick does.
///
/// A flow field is not recomputed from nothing every frame. The interesting cost is
/// *updating* a converged field after the world changed a little. Min-plus relaxation
/// warm-starts trivially — and Dijkstra does not, because a converged field makes every
/// cell a source.
///
/// **The soundness boundary, stated plainly:** warm relaxation is only valid for changes
/// that can lower distances (a goal added, an obstacle removed, terrain made cheaper). The
/// old field is then an upper bound and relaxation descends to the new fixed point. For
/// changes that *raise* distances (a wall built, a goal removed) the stale field is below
/// the new answer and relaxation can never climb back — that case needs a D*-Lite style
/// raise phase or a full recompute. This benchmark measures only the sound direction.
fn section_d(gpu: &Option<Gpu>, threads: usize, quick: bool) {
    println!("== D. warm restart after a distance-lowering change ==");
    for local in [false, true] {
        println!(
            "\n-- perturbation: {} --",
            if local {
                "a 4x4 patch of terrain made free, next to the existing goal (small affected region)"
            } else {
                "a second goal opened three quarters of the way across the map (large affected region)"
            }
        );
        section_d_one(gpu, threads, quick, local);
    }
}

fn section_d_one(gpu: &Option<Gpu>, threads: usize, quick: bool, local: bool) {
    println!(
        "{:<10} {:>7} {:>12} {:>12} {:>13} {:>11} {:>10} {:>10}",
        "grid",
        "fields",
        "cold dial ms",
        "warm row ms",
        "sweeps av/max",
        "warm gpu ms",
        "gpu rounds",
        "gpu/cpu"
    );
    let cfgs: &[(u32, u32, u32)] = if quick {
        &[(64, 64, 256)]
    } else {
        &[
            (64, 64, 256),
            (64, 64, 4096),
            (128, 128, 1024),
            (256, 256, 256),
        ]
    };
    for &(w, h, fields) in cfgs {
        let base = FieldBatch::synth_terrain(w, h, fields, 0xC0FFEE);
        // converged starting point
        let mut warm_base = base.clone();
        solve_batch_serial(&mut warm_base, CpuKernel::Dial, 0);
        let mut perturbed = warm_base.clone();
        let mut cold = base.clone();
        for f in 0..fields {
            if local {
                // free ground in a small patch near the top-left, where the goal is
                for y in 2..6 {
                    for x in 2..6 {
                        let i = perturbed.index(f, x, y);
                        perturbed.cost[i] = 1;
                        cold.cost[i] = 1;
                    }
                }
            } else {
                let (gx, gy) = (w * 3 / 4, h * 3 / 4);
                let i = perturbed.index(f, gx, gy);
                perturbed.cost[i] = 1;
                perturbed.dist[i] = 0;
                cold.cost[i] = 1;
                cold.dist[i] = 0;
            }
        }

        let (cold_ref, t_cold) = best(2, &cold, |b| {
            solve_batch_parallel(b, CpuKernel::Dial, 0, threads)
        });

        let (warm_cpu, t_warm) = best(2, &perturbed, |b| {
            solve_batch_parallel(b, CpuKernel::JacobiRows, 1 << 20, threads);
        });
        // Sweeps to converge *per field*. The mean is what the CPU pays (each field stops
        // on its own); the max is what a GPU with a single batch-wide convergence flag
        // pays for every field in the batch. The gap between them is the cost of not
        // tracking convergence per field.
        let (mut smean, mut smax) = (0f64, 0u32);
        {
            let mut probe = perturbed.clone();
            let per = probe.cells_per_field();
            let costs = probe.cost.clone();
            for f in 0..fields as usize {
                let r = f * per..(f + 1) * per;
                let s = don_gpu::cpu::solve_field_jacobi_rows(
                    w,
                    h,
                    &costs[r.clone()],
                    &mut probe.dist[r],
                    1 << 20,
                );
                smean += s as f64;
                smax = smax.max(s);
            }
            smean /= fields as f64;
        }
        let sweeps = format!("{:.0}/{}", smean, smax);
        assert_eq!(
            warm_cpu.dist, cold_ref.dist,
            "warm CPU restart missed the fixed point"
        );

        let (gms, grounds, ratio) = match gpu {
            Some(g) => {
                let mut s = g.solver(w, h, fields);
                let mut t = Duration::MAX;
                let mut bg = perturbed.clone();
                let mut rounds = 0;
                for _ in 0..2 {
                    bg = perturbed.clone();
                    s.upload(&bg);
                    let st = s.run(SolveOptions::default());
                    s.download(&mut bg);
                    rounds = st.rounds;
                    if st.compute < t {
                        t = st.compute;
                    }
                }
                assert_eq!(
                    bg.dist, cold_ref.dist,
                    "warm GPU restart missed the fixed point"
                );
                (ms(t), rounds, ms(t_warm) / ms(t))
            }
            None => (f64::NAN, 0, f64::NAN),
        };
        println!(
            "{:<10} {:>7} {:>12.1} {:>12.1} {:>13} {:>11.1} {:>10} {:>10.2}",
            format!("{w}x{h}"),
            fields,
            ms(t_cold),
            ms(t_warm),
            sweeps,
            gms,
            grounds,
            ratio
        );
    }
    println!();
}

/// Same kernel on both devices.
fn section_a(gpu: &Option<Gpu>, threads: usize, quick: bool) {
    println!("== A. same kernel (Jacobi relaxation to fixed point) ==");
    println!(
        "{:<12} {:>7} {:>8} {:>11} {:>11} {:>11} {:>10} {:>10}",
        "grid", "fields", "sweeps", "cpu-1t ms", "cpu-Nt ms", "cpu-row ms", "gpu ms", "speedup"
    );
    let cfgs: Vec<Cfg> = if quick {
        vec![Cfg {
            w: 64,
            h: 64,
            fields: 24,
        }]
    } else {
        vec![
            Cfg {
                w: 64,
                h: 64,
                fields: 24,
            },
            Cfg {
                w: 64,
                h: 64,
                fields: 96,
            },
            Cfg {
                w: 128,
                h: 128,
                fields: 24,
            },
            Cfg {
                w: 128,
                h: 128,
                fields: 96,
            },
            Cfg {
                w: 256,
                h: 256,
                fields: 24,
            },
        ]
    };
    for c in &cfgs {
        let base = FieldBatch::synth_terrain(c.w, c.h, c.fields, 0xC0FFEE);

        // sweep count for a single field, for context on the O(iters x cells) cost
        let mut one = FieldBatch::synth_terrain(c.w, c.h, 1, 0xC0FFEE);
        let onecost = one.cost.clone();
        let sweeps = don_gpu::cpu::solve_field_jacobi(c.w, c.h, &onecost, &mut one.dist, 1 << 20);

        let (b1, t1) = best(REPS, &base, |b| {
            solve_batch_serial(b, CpuKernel::Jacobi, 1 << 20)
        });
        let (bn, tn) = best(REPS, &base, |b| {
            solve_batch_parallel(b, CpuKernel::Jacobi, 1 << 20, threads)
        });
        let (br, tr) = best(REPS, &base, |b| {
            solve_batch_parallel(b, CpuKernel::JacobiRows, 1 << 20, threads)
        });
        assert_eq!(b1.dist, bn.dist, "thread count changed the CPU answer");
        assert_eq!(b1.dist, br.dist, "row kernel disagreed with scalar");

        let (gms, ratio) = match gpu {
            Some(g) => {
                let mut s = g.solver(c.w, c.h, c.fields);
                let mut gt = Duration::MAX;
                let mut bg = base.clone();
                for _ in 0..REPS {
                    bg = base.clone();
                    s.upload(&bg);
                    let st = s.run(SolveOptions::default());
                    if st.compute < gt {
                        gt = st.compute;
                    }
                    s.download(&mut bg);
                }
                assert_eq!(
                    bg.dist, b1.dist,
                    "GPU disagreed with CPU at {}x{}",
                    c.w, c.h
                );
                (ms(gt), ms(tn) / ms(gt))
            }
            None => (f64::NAN, f64::NAN),
        };
        println!(
            "{:<12} {:>7} {:>8} {:>11.1} {:>11.1} {:>11.1} {:>10.1} {:>10.2}",
            format!("{}x{}", c.w, c.h),
            c.fields,
            sweeps,
            ms(t1),
            ms(tn),
            ms(tr),
            gms,
            ratio
        );
    }
    println!();
}

/// GPU against the best CPU algorithm, sweeping the batch dimension for a crossover.
fn section_b(gpu: &Option<Gpu>, threads: usize, quick: bool) {
    println!("== B. GPU relaxation vs CPU bucket-queue Dijkstra (the real baseline) ==");
    println!(
        "{:<10} {:>7} {:>9} {:>11} {:>11} {:>10} {:>10} {:>10} {:>9}",
        "grid",
        "fields",
        "Mcells",
        "dial-1t ms",
        "dial-Nt ms",
        "gpu ms",
        "gpu+io ms",
        "gpu Mc/s",
        "speedup"
    );
    let grids: &[(u32, u32)] = if quick {
        &[(64, 64)]
    } else {
        &[(64, 64), (128, 128), (256, 256)]
    };
    let field_counts: &[u32] = if quick {
        &[16, 256]
    } else {
        &[1, 8, 64, 256, 1024, 4096, 16384]
    };
    // Keep total cells under a cap so the four device buffers stay comfortable.
    const CELL_CAP: u64 = 24 << 20;

    for &(w, h) in grids {
        for &fields in field_counts {
            let cells = (w as u64) * (h as u64) * (fields as u64);
            if cells > CELL_CAP {
                continue;
            }
            let base = FieldBatch::synth_terrain(w, h, fields, 0xC0FFEE);

            let reps = if cells > (4 << 20) { 2 } else { REPS };
            let (b1, t1) = best(reps, &base, |b| solve_batch_serial(b, CpuKernel::Dial, 0));
            let (bn, tn) = best(reps, &base, |b| {
                solve_batch_parallel(b, CpuKernel::Dial, 0, threads)
            });
            assert_eq!(b1.dist, bn.dist);

            let (gc, gio, speed) = match gpu {
                Some(g) => {
                    let mut s = g.solver(w, h, fields);
                    let (mut gt, mut gio) = (Duration::MAX, Duration::MAX);
                    let mut bg = base.clone();
                    for _ in 0..reps {
                        bg = base.clone();
                        let up = s.upload(&bg);
                        let st = s.run(SolveOptions::default());
                        let down = s.download(&mut bg);
                        if st.compute < gt {
                            gt = st.compute;
                        }
                        if st.compute + up + down < gio {
                            gio = st.compute + up + down;
                        }
                    }
                    assert_eq!(
                        bg.dist, b1.dist,
                        "GPU disagreed with Dial at {w}x{h} x{fields}"
                    );
                    (ms(gt), ms(gio), ms(tn) / ms(gt))
                }
                None => (f64::NAN, f64::NAN, f64::NAN),
            };
            println!(
                "{:<10} {:>7} {:>9.2} {:>11.1} {:>11.1} {:>10.1} {:>10.1} {:>10.1} {:>9.2}",
                format!("{w}x{h}"),
                fields,
                cells as f64 / 1e6,
                ms(t1),
                ms(tn),
                gc,
                gio,
                cells as f64 / (gc / 1e3) / 1e6,
                speed
            );
        }
    }
    println!();
}

/// How the two GPU knobs move the number.
fn section_c(gpu: &Option<Gpu>, quick: bool) {
    let Some(g) = gpu else {
        return;
    };
    println!("== C. GPU knobs: temporal blocking and convergence-poll frequency ==");
    let configs: &[(u32, u32, u32)] = if quick {
        &[(64, 64, 256)]
    } else {
        &[(64, 64, 1024), (256, 256, 64)]
    };
    for &(w, h, fields) in configs {
        let base = FieldBatch::synth_terrain(w, h, fields, 0xC0FFEE);
        let mut reference = base.clone();
        solve_batch_serial(&mut reference, CpuKernel::Dial, 0);

        println!("\n-- {w}x{h} x {fields} fields --");
        println!(
            "{:<12} {:>16} {:>9} {:>7} {:>10} {:>8}",
            "inner_steps", "rounds_per_poll", "rounds", "polls", "ms", "match"
        );
        let mut solver = g.solver(w, h, fields);
        for inner in [1u32, 2, 4, 8, 16, 32] {
            for poll in [1u32, 8, 64] {
                let opts = SolveOptions {
                    inner_steps: inner,
                    rounds_per_poll: poll,
                    max_rounds: 200_000,
                    ..SolveOptions::default()
                };
                let mut bg = base.clone();
                let mut t = Duration::MAX;
                let mut last = SolveStats::default();
                for _ in 0..2 {
                    bg = base.clone();
                    solver.upload(&bg);
                    last = solver.run(opts);
                    solver.download(&mut bg);
                    if last.compute < t {
                        t = last.compute;
                    }
                }
                let ok = bg.dist == reference.dist;
                println!(
                    "{:<12} {:>16} {:>9} {:>7} {:>10.1} {:>8}",
                    inner,
                    poll,
                    last.rounds,
                    last.polls,
                    ms(t),
                    if ok { "yes" } else { "NO" }
                );
                assert!(ok, "schedule ({inner},{poll}) changed the answer");
            }
        }
    }
    println!(
        "\n(all rows must read match=yes: the fixed point is schedule-independent by\n\
         construction, and this table is the check)"
    );
}
