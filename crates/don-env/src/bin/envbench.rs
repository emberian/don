//! End-to-end RoNEval benchmark: reset, simulation step, observation and mask emission.
//!
//! This is an engineering benchmark, not a fidelity measurement. A `step` row includes
//! action decode/application, `frames_per_step` simulation frames, reward terms, entity
//! selection, every observation plane, and every packed action mask. Output buffers stay
//! owned by `VecEnv`; the timed region performs no Python or NumPy work.
//!
//! Run the release binary on an otherwise idle machine:
//! `DON_ENV_BENCH_BUDGET=0.35 cargo run --release -p don-env --bin envbench`.

use don_env::{EnvConfig, VecEnv};
use std::hint::black_box;
use std::time::{Duration, Instant};

const MIB: f64 = 1024.0 * 1024.0;

fn budget() -> Duration {
    let seconds = std::env::var("DON_ENV_BENCH_BUDGET")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.35);
    Duration::from_secs_f64(seconds.max(0.02))
}

fn samples() -> usize {
    std::env::var("DON_ENV_BENCH_SAMPLES")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(5)
        .max(1)
}

#[derive(Clone, Copy)]
struct Timing {
    median_ns: f64,
    best_ns: f64,
    worst_ns: f64,
}

fn measure(mut f: impl FnMut()) -> Timing {
    let mut runs = Vec::with_capacity(samples());
    for _ in 0..samples() {
        let t0 = Instant::now();
        let mut iterations = 0u64;
        while t0.elapsed() < budget() {
            f();
            iterations += 1;
        }
        runs.push(t0.elapsed().as_nanos() as f64 / iterations as f64);
    }
    runs.sort_by(f64::total_cmp);
    Timing {
        median_ns: runs[runs.len() / 2],
        best_ns: runs[0],
        worst_ns: *runs.last().unwrap(),
    }
}

fn report(label: &str, n: usize, t: Timing) {
    let batch_per_s = 1e9 / t.median_ns;
    let env_per_s = batch_per_s * n as f64;
    println!(
        "  {label:<8} {:>9.3} ms/batch  {:>9.0} env/s  {:>8.0} batch/s  spread {:>5.2}x",
        t.median_ns / 1e6,
        env_per_s,
        batch_per_s,
        t.worst_ns / t.best_ns,
    );
}

fn bench_case(n: usize, threads: usize) {
    let cfg = EnvConfig {
        grid_w: 64,
        grid_h: 64,
        max_entities: 64,
        max_controlled: 32,
        num_agents: 2,
        frames_per_step: 1,
        max_steps: 0,
        start_units: 16,
        fog: false,
        seed: 0x5EED,
    };
    let mut env = VecEnv::new(n, cfg, None, None, threads).expect("benchmark environment");
    let ua = vec![
        0i32;
        n * env.cfg.num_agents * env.cfg.max_controlled * don_env::generated::N_UNIT_HEADS
    ];
    let pa = vec![0i32; n * env.cfg.num_agents * don_env::generated::N_PLAYER_HEADS];

    // Warm pages, code and worker stacks before collecting samples.
    for _ in 0..8 {
        env.step(black_box(&ua), black_box(&pa));
    }
    let step = measure(|| env.step(black_box(&ua), black_box(&pa)));
    let sample = measure(|| {
        env.sample_masked();
        black_box(env.sampled_unit_actions());
    });
    let reset = measure(|| {
        env.reset_all();
        black_box(env.spatial());
    });

    let mem = env.bytes_reserved();
    let action_bytes = (ua.capacity() + pa.capacity()) * std::mem::size_of::<i32>();
    println!("{n:>4} envs, {threads:>2} threads");
    report("step", n, step);
    report("reset", n, reset);
    report("sample", n, sample);
    println!(
        "  memory   {:>8.2} MiB total = {:>7.2} world + {:>7.2} output + \
         {:>6.2} sampler + {:>5.2} work + {:>5.2} shared; caller actions {:>5.2} MiB; {:>7.1} KiB/env",
        mem.total() as f64 / MIB,
        mem.worlds as f64 / MIB,
        mem.outputs as f64 / MIB,
        mem.sampler as f64 / MIB,
        mem.workspace as f64 / MIB,
        mem.shared_rules as f64 / MIB,
        action_bytes as f64 / MIB,
        mem.total() as f64 / n as f64 / 1024.0,
    );
    println!(
        "  checksum frame={} spatial={:#010x} masks={:#010x}",
        env.worlds[0].sim.frame,
        fold_f32(env.spatial()),
        fold_u8(env.unit_masks()),
    );
}

fn fold_f32(values: &[f32]) -> u32 {
    values.iter().fold(0x811C_9DC5, |h, v| {
        (h ^ v.to_bits()).wrapping_mul(0x0100_0193)
    })
}

fn fold_u8(values: &[u8]) -> u32 {
    values.iter().fold(0x811C_9DC5, |h, v| {
        (h ^ u32::from(*v)).wrapping_mul(0x0100_0193)
    })
}

fn main() {
    let available = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let threads = std::env::var("DON_ENV_BENCH_THREADS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(available)
        .max(1);
    let cases: Vec<usize> = match std::env::args().nth(1) {
        Some(v) => vec![v.parse().expect("env count")],
        None => vec![1, 16, 64, 256],
    };
    println!(
        "don-env end-to-end benchmark; release={} budget={:.2}s samples={} available_threads={available}\n",
        !cfg!(debug_assertions),
        budget().as_secs_f64(),
        samples(),
    );
    for n in cases {
        bench_case(n, threads.min(n));
        println!();
    }
}
