//! Deterministic batched policy evaluation over the arena model.
//!
//! ```text
//! cargo run --release -p don-ai --bin roneval -- --minutes 8 --seeds 4 --threads 8
//! ```

use don_ai::arena::eval::{
    evaluate_strong_ai_head_to_head_from_verified, evaluate_strong_ai_verified, evaluate_verified,
};

fn value(args: &[String], key: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == key)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let minutes = value(&args, "--minutes")
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(8)
        .max(1);
    let seeds = value(&args, "--seeds")
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(2)
        .max(1);
    let seed_offset = value(&args, "--seed-offset")
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0);
    let available = std::thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(1);
    let threads = value(&args, "--threads")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(available)
        .max(1);

    let require_ai = args.iter().any(|arg| arg == "--require-ai");
    if args.iter().any(|arg| arg == "--strong-only") {
        match evaluate_strong_ai_head_to_head_from_verified(minutes, seed_offset, seeds, threads) {
            Ok(report) if require_ai && !report.gate.passed => {
                eprintln!(
                    "RoNEval failed closed: Ai paired gate failed ({}-{}-{} seeds, p={:.6}, damage delta {})",
                    report.gate.paired_wins,
                    report.gate.paired_losses,
                    report.gate.paired_ties,
                    report.gate.one_sided_p,
                    report.gate.damage_delta,
                );
                std::process::exit(2);
            }
            Ok(report) => print!("{}", report.render_text()),
            Err(error) => {
                eprintln!("RoNEval failed closed: {error}");
                std::process::exit(2);
            }
        }
        return;
    }
    let result = if require_ai {
        evaluate_strong_ai_verified(minutes, seeds, threads)
    } else {
        evaluate_verified(minutes, seeds, threads)
    };
    match result {
        Ok(report) => print!("{}", report.render_text()),
        Err(error) => {
            eprintln!("RoNEval failed closed: {error}");
            std::process::exit(2);
        }
    }
}
