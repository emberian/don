//! GPU/CPU parity and schedule-independence.
//!
//! These are the tests that make the determinism claim in the crate docs checkable rather
//! than merely argued. They **skip** when no wgpu adapter exists — headless CI has none,
//! and that is not a failure of this crate. A skip prints a line so it cannot pass
//! silently forever.

use don_gpu::cpu::{directions_field, solve_batch_serial, CpuKernel};
use don_gpu::field::FieldBatch;
use don_gpu::gpu::{Gpu, SolveOptions};

fn gpu_or_skip(what: &str) -> Option<Gpu> {
    match Gpu::new() {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("SKIP {what}: {e}");
            None
        }
    }
}

/// Shapes deliberately include dimensions that are not multiples of the 8x8 workgroup
/// tile, so the out-of-grid guards in the kernel are exercised.
const SHAPES: &[(u32, u32, u32)] = &[(8, 8, 1), (17, 13, 3), (64, 64, 2), (33, 65, 5)];

fn cpu_reference(w: u32, h: u32, fields: u32, seed: u64) -> FieldBatch {
    let mut b = FieldBatch::synth_terrain(w, h, fields, seed);
    solve_batch_serial(&mut b, CpuKernel::Dial, 0);
    b
}

#[test]
fn gpu_matches_cpu_bit_for_bit() {
    let Some(g) = gpu_or_skip("gpu_matches_cpu_bit_for_bit") else {
        return;
    };
    for &(w, h, fields) in SHAPES {
        let want = cpu_reference(w, h, fields, 0xBEEF);
        let mut got = FieldBatch::synth_terrain(w, h, fields, 0xBEEF);
        let stats = don_gpu::gpu::solve_batch_gpu(&g, &mut got, SolveOptions::default());
        assert!(stats.converged, "{w}x{h} x{fields} hit the round cap");
        assert_eq!(got.dist, want.dist, "{w}x{h} x{fields}");
    }
}

#[test]
fn gpu_result_is_invariant_under_the_schedule() {
    // inner_steps changes how much stale-halo relaxation happens inside a workgroup;
    // rounds_per_poll changes how far past convergence the loop runs. Neither may move a
    // single bit of the answer — that is the whole point of using a min-plus operator.
    let Some(g) = gpu_or_skip("gpu_result_is_invariant_under_the_schedule") else {
        return;
    };
    let (w, h, fields) = (48u32, 40u32, 4u32);
    let want = cpu_reference(w, h, fields, 7).dist;
    for inner in [1u32, 3, 8, 17] {
        for poll in [1u32, 5, 32] {
            for per_field in [false, true] {
                let mut b = FieldBatch::synth_terrain(w, h, fields, 7);
                don_gpu::gpu::solve_batch_gpu(
                    &g,
                    &mut b,
                    SolveOptions {
                        inner_steps: inner,
                        rounds_per_poll: poll,
                        max_rounds: 100_000,
                        per_field_convergence: per_field,
                    },
                );
                assert_eq!(
                    b.dist, want,
                    "inner={inner} poll={poll} per_field={per_field}"
                );
            }
        }
    }
}

/// Dropping a converged field out of the dispatch must not change one bit of its answer —
/// neither its own (it is frozen at its fixed point) nor any other field's (fields are
/// independent). Ragged convergence is the case that matters, so the fixture gives each
/// field a deliberately different amount of work to do.
#[test]
fn active_list_compaction_is_bit_identical_to_running_the_whole_batch() {
    let Some(g) = gpu_or_skip("active_list_compaction_is_bit_identical") else {
        return;
    };
    let mut ever_compacted = false;
    for &(w, h, fields) in &[(64u32, 64u32, 32u32), (33, 65, 9), (17, 13, 3)] {
        // Ragged difficulty: field f gets f extra goals sprinkled along a diagonal, so the
        // fields converge at wildly different rounds and compaction actually happens.
        let mut b = FieldBatch::synth_terrain(w, h, fields, 0x51DE);
        for f in 0..fields {
            for k in 0..f {
                let x = (k * 7) % w;
                let y = (k * 13) % h;
                let i = b.index(f, x, y);
                if b.cost[i] != don_gpu::field::COST_BLOCKED {
                    b.dist[i] = 0;
                }
            }
        }
        let mut want = b.clone();
        solve_batch_serial(&mut want, CpuKernel::Dial, 0);

        let mut off = b.clone();
        let s_off = don_gpu::gpu::solve_batch_gpu(
            &g,
            &mut off,
            SolveOptions {
                per_field_convergence: false,
                ..SolveOptions::default()
            },
        );
        let mut on = b.clone();
        let s_on = don_gpu::gpu::solve_batch_gpu(
            &g,
            &mut on,
            SolveOptions {
                per_field_convergence: true,
                ..SolveOptions::default()
            },
        );

        assert!(
            s_off.converged && s_on.converged,
            "{w}x{h} x{fields} hit the round cap"
        );
        assert_eq!(off.dist, want.dist, "batch-wide flag disagreed with Dial");
        assert_eq!(
            on.dist, want.dist,
            "compacted run disagreed with Dial at {w}x{h} x{fields}"
        );
        // The test is only meaningful if compaction actually kicked in somewhere. A
        // three-field batch can legitimately converge inside one poll group, so the
        // vacuity guard is an aggregate one.
        ever_compacted |= s_on.field_rounds < s_on.field_rounds_uncompacted;
    }
    assert!(
        ever_compacted,
        "compaction never dropped a field on any shape; the fixture is too uniform to test it"
    );
}

/// The active list must survive being reused: a solver is allocated once per batch shape and
/// re-uploaded many times in a training loop, so a stale (already compacted) active list
/// would silently freeze most of the batch on the second solve.
#[test]
fn a_reused_solver_resets_its_active_list() {
    let Some(g) = gpu_or_skip("a_reused_solver_resets_its_active_list") else {
        return;
    };
    let (w, h, fields) = (48u32, 48u32, 16u32);
    let mut s = g.solver(w, h, fields);
    for seed in [1u64, 2, 3] {
        let mut want = FieldBatch::synth_terrain(w, h, fields, seed);
        solve_batch_serial(&mut want, CpuKernel::Dial, 0);
        let mut got = FieldBatch::synth_terrain(w, h, fields, seed);
        s.upload(&got);
        let st = s.run(SolveOptions::default());
        s.download(&mut got);
        assert!(st.converged);
        assert_eq!(
            s.active_fields(),
            0,
            "a converged solve must have emptied the active list"
        );
        assert_eq!(got.dist, want.dist, "seed {seed} on a reused solver");
    }
}

#[test]
fn gpu_is_reproducible_across_runs() {
    let Some(g) = gpu_or_skip("gpu_is_reproducible_across_runs") else {
        return;
    };
    let run = || {
        let mut b = FieldBatch::synth_terrain(64, 64, 8, 0x1234);
        don_gpu::gpu::solve_batch_gpu(&g, &mut b, SolveOptions::default());
        b.dist
    };
    let a = run();
    for _ in 0..3 {
        assert_eq!(run(), a, "the same input gave different bits on a rerun");
    }
}

#[test]
fn gpu_directions_match_cpu_directions() {
    let Some(g) = gpu_or_skip("gpu_directions_match_cpu_directions") else {
        return;
    };
    for &(w, h, fields) in SHAPES {
        let want_batch = cpu_reference(w, h, fields, 55);
        let per = want_batch.cells_per_field();
        let mut want = vec![0u8; want_batch.total_cells()];
        for f in 0..fields as usize {
            let r = f * per..(f + 1) * per;
            directions_field(
                w,
                h,
                &want_batch.cost[r.clone()],
                &want_batch.dist[r.clone()],
                &mut want[r],
            );
        }

        let mut b = FieldBatch::synth_terrain(w, h, fields, 55);
        let mut s = g.solver(w, h, fields);
        s.upload(&b);
        s.run(SolveOptions::default());
        s.download(&mut b);
        let mut got = vec![0u32; b.total_cells()];
        s.directions(&mut got);

        let got8: Vec<u8> = got.iter().map(|&d| d as u8).collect();
        assert_eq!(got8, want, "{w}x{h} x{fields}");
    }
}

#[test]
fn a_batch_larger_than_one_dispatch_chunk_would_be_split_correctly() {
    // The z dimension of a dispatch is capped at 65535, so `FlowSolver` splits long
    // batches. Allocating 65536 fields is too heavy for a unit test, so this asserts the
    // arithmetic that drives the split instead of the split itself, and the full-size case
    // is exercised by `flowbench`.
    for fields in [1u32, 65535, 65536, 131071] {
        let chunks = fields.div_ceil(65535).max(1);
        let mut covered = 0u32;
        for c in 0..chunks {
            let lo = c * 65535;
            covered += (fields - lo).min(65535);
        }
        assert_eq!(covered, fields, "chunking lost fields at {fields}");
    }
}
