//! Native twin of the browser's readouts.
//!
//! Builds the *same* shard the wasm shim builds, through the *same* entry points, so two
//! numbers the browser prints can be compared against a host build:
//!
//! * `digest` — `don_sim::Batch::digest()` after N frames. Two independently compiled
//!   targets (aarch64, where LLVM autovectorises the tick kernels to NEON, and wasm32,
//!   where it does not) agreeing on a 64-bit digest over tens of thousands of unit-frames
//!   is a check that could have failed, and fails loudly if the wasm build differs.
//!
//! * `bench` — single-threaded `sim_step` + `sim_gather` throughput, which is exactly what
//!   one browser worker does. The browser's `stepMs`/`gatherMs` are directly comparable,
//!   so "how much does wasm cost us" becomes a measurement instead of a guess.
//!
//! ```sh
//! cargo run --release --bin digest -- digest <worlds> <units> <capacity> <frames> <seed-hex>
//! cargo run --release --bin digest -- bench  <worlds> <units> <capacity> <frames>
//! ```

use std::time::Instant;

fn usage() -> ! {
    eprintln!("usage: digest digest <worlds> <units> <capacity> <frames> <seed-hex>");
    eprintln!("       digest bench  <worlds> <units> <capacity> <frames>");
    std::process::exit(2);
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.is_empty() {
        usage();
    }
    let n = |i: usize| -> u32 { a.get(i).and_then(|s| s.parse().ok()).unwrap_or_else(|| usage()) };

    match a[0].as_str() {
        "digest" => {
            if a.len() != 6 {
                usage();
            }
            let (worlds, units, capacity, frames) = (n(1), n(2), n(3), n(4));
            let seed = u64::from_str_radix(a[5].trim_start_matches("0x"), 16).unwrap_or_else(|_| usage());
            let s = don_web::sim_create(worlds, units, capacity, seed as u32, (seed >> 32) as u32);
            assert!(!s.is_null(), "sim_create returned null");
            // SAFETY: `s` is the handle just returned by `sim_create`, destroyed below.
            unsafe {
                don_web::sim_step(s, frames);
                println!("worlds={worlds} units={units} capacity={capacity} frames={frames} seed=0x{seed:016x}");
                println!("live_total={}", don_web::sim_live_total(s));
                println!("digest=0x{:08x}{:08x}", don_web::sim_digest_hi(s), don_web::sim_digest_lo(s));
                don_web::sim_destroy(s);
            }
        }
        "bench" => {
            if a.len() != 5 {
                usage();
            }
            let (worlds, units, capacity, frames) = (n(1), n(2), n(3), n(4));
            let s = don_web::sim_create(worlds, units, capacity, 0xC0FFEE, 0);
            assert!(!s.is_null(), "sim_create returned null");
            // SAFETY: as above.
            unsafe {
                // Warm up: first touch of every column page is not steady state, and at
                // 4096 worlds it is most of a short run.
                don_web::sim_step(s, 30);
                don_web::sim_gather(s);

                let t0 = Instant::now();
                don_web::sim_step(s, frames);
                let step = t0.elapsed().as_secs_f64();

                let g0 = Instant::now();
                for _ in 0..frames {
                    don_web::sim_gather(s);
                }
                let gather = g0.elapsed().as_secs_f64();

                let live = don_web::sim_live_total(s) as f64;
                println!("native single-thread, {worlds} worlds x {units} units (capacity {capacity})");
                println!(
                    "  step   {:8.4} ms/frame   {:9.1} M unit-steps/s",
                    step / frames as f64 * 1e3,
                    live * frames as f64 / step / 1e6
                );
                println!(
                    "  gather {:8.4} ms/call    {:9.1} M cells/s   ({} cells)",
                    gather / frames as f64 * 1e3,
                    (worlds as f64 * don_web::sim_stride(s) as f64) * frames as f64 / gather / 1e6,
                    worlds as u64 * don_web::sim_stride(s) as u64
                );
                don_web::sim_destroy(s);
            }
        }
        _ => usage(),
    }
}
