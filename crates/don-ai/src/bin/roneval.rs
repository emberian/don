//! Deterministic batched policy evaluation over the arena model.
//!
//! ```text
//! cargo run --release -p don-ai --bin roneval -- --minutes 8 --seeds 4 --threads 8
//! ```

use don_ai::arena::eval::evaluate_verified;

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
    let available = std::thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(1);
    let threads = value(&args, "--threads")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(available)
        .max(1);

    match evaluate_verified(minutes, seeds, threads) {
        Ok(report) => print!("{}", report.render_text()),
        Err(error) => {
            eprintln!("RoNEval failed closed: {error}");
            std::process::exit(2);
        }
    }
}
