//! `bhs-census` — measure which BHS builtins the shipped scripts actually call.
//!
//! ```sh
//! cargo run -p don-bhs --bin bhs-census                 # ron-data/bhs-corpus
//! cargo run -p don-bhs --bin bhs-census -- <dir> --all  # any tree, full list
//! cargo run -p don-bhs --bin bhs-census -- --json
//! ```
//!
//! The output is the ordered debt list: every registered builtin the corpus calls,
//! most-called first, with the ones this crate already implements marked. That
//! ordering is the only defensible way to choose what to implement next.

use don_bhs::builtins;
use don_bhs::corpus;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");
    let all = args.iter().any(|a| a == "--all");
    let dir = args.iter().find(|a| !a.starts_with("--")).cloned();

    let root = match dir {
        Some(d) => std::path::PathBuf::from(d),
        None => match corpus::default_corpus() {
            Some(d) => d,
            None => {
                eprintln!(
                    "ron-data/bhs-corpus not found (it is gitignored game content); \
                     pass a directory explicitly"
                );
                std::process::exit(2);
            }
        },
    };

    let c = match corpus::scan_dir(&root) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("scan {}: {e}", root.display());
            std::process::exit(2);
        }
    };
    let done = builtins::implemented_indices();

    if json {
        println!("{{");
        println!("  \"root\": {:?},", root.display().to_string());
        println!("  \"files\": {},", c.files.len());
        println!("  \"lines\": {},", c.lines);
        println!("  \"distinct_builtins_called\": {},", c.distinct_called());
        println!("  \"total_builtin_calls\": {},", c.total_calls());
        println!(
            "  \"call_share_implemented\": {:.6},",
            c.coverage_fraction(&done)
        );
        println!("  \"builtins\": [");
        for (i, r) in c.builtins.iter().enumerate() {
            println!(
                "    {{\"index\": {}, \"name\": {:?}, \"func_set\": {:?}, \"calls\": {}, \"files\": {}, \"implemented\": {}}}{}",
                r.index,
                r.name,
                r.func_set(),
                r.calls,
                r.files,
                done.contains(&r.index),
                if i + 1 == c.builtins.len() { "" } else { "," }
            );
        }
        println!("  ]");
        println!("}}");
        return;
    }

    println!("corpus: {}", root.display());
    println!(
        "{} files, {} lines, {} of 873 registered builtins called, {} call sites",
        c.files.len(),
        c.lines,
        c.distinct_called(),
        c.total_calls()
    );
    println!(
        "implemented here: {} builtins, covering {:.2}% of measured call sites",
        done.len(),
        100.0 * c.coverage_fraction(&done)
    );
    println!(
        "\n{:>7}  {:>4}  {:<34} {:<20} {}",
        "calls", "idx", "name", "funcset", "files"
    );
    let n = if all { c.builtins.len() } else { 60 };
    for r in c.builtins.iter().take(n) {
        println!(
            "{:>7}  {:>4}  {}{:<32} {:<20} {}",
            r.calls,
            r.index,
            if done.contains(&r.index) { "* " } else { "  " },
            r.name,
            r.func_set(),
            r.files
        );
    }
    if !all && c.builtins.len() > n {
        println!("... {} more (pass --all)", c.builtins.len() - n);
    }
    println!("\n(* = implemented by don_bhs::builtins; everything else is owed)");
}
