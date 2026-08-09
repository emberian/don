//! `don-content` — inspect a mod the way the engine would.
//!
//! ```text
//! don-content scan  <mods-dir>              what the engine would find, per mod
//! don-content check <mods-dir>              reject anything not consumed end-to-end
//! don-content probe <mods-dir> <path>...    where each content path resolves
//! don-content rules                         the 12 categories and the extension table
//! ```
//!
//! `<mods-dir>` is a directory of mod folders — `<Documents>/My Games/Rise of Nations/mods`
//! on a retail install, or any directory shaped like it.

use std::path::Path;
use std::process::ExitCode;

use don_content::compat::{report, Support};
use don_content::generated::{CATEGORY_INFO, FORBIDDEN_MAPSTYLES, TAG_LINKS};
use don_content::scan::{populated_categories, scan_mods_root};
use don_content::vfs::{ContentStack, ALL_CATEGORIES};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("scan") if args.len() == 2 => cmd_scan(Path::new(&args[1])),
        Some("check") if args.len() == 2 => cmd_check(Path::new(&args[1])),
        Some("probe") if args.len() >= 3 => cmd_probe(Path::new(&args[1]), &args[2..]),
        Some("rules") => cmd_rules(),
        _ => {
            eprintln!("{}", usage());
            ExitCode::from(2)
        }
    }
}

fn usage() -> &'static str {
    "usage:\n  \
     don-content scan  <mods-dir>\n  \
     don-content check <mods-dir>\n  \
     don-content probe <mods-dir> <content-path>...\n  \
     don-content rules"
}

fn load(dir: &Path) -> Result<ContentStack, ExitCode> {
    let mods = match scan_mods_root(dir) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("cannot scan {}: {e}", dir.display());
            return Err(ExitCode::FAILURE);
        }
    };
    let mut stack = ContentStack::new();
    for m in mods {
        stack.push(m);
    }
    stack.sort_via_priority();
    Ok(stack)
}

fn cmd_scan(dir: &Path) -> ExitCode {
    let stack = match load(dir) {
        Ok(s) => s,
        Err(c) => return c,
    };
    if stack.mods().is_empty() {
        println!("no mods under {}", dir.display());
        println!("(a directory with no files in any of the 12 categories is not a mod --");
        println!(" ModPackage::buildPackage 0x00A358B0 drops it)");
        return ExitCode::SUCCESS;
    }
    println!("{} mod(s) under {}\n", stack.mods().len(), dir.display());
    for m in stack.mods() {
        let kind = match (m.is_dropdown_mod(), m.is_data_mod()) {
            (true, _) => "dropdown mod (info.xml at root)",
            (false, true) => "data mod",
            (false, false) => "content-only mod",
        };
        let tags: Vec<&str> = m.steam_tags().iter().map(|t| t.name()).collect();
        println!("  [{}] {}  --  {kind}", m.priority, m.name);
        println!("      workshop tags: {}", tags.join(", "));
        for cat in populated_categories(m) {
            let n = m.files[cat.index()].len();
            println!(
                "      {:<10} {n:>4} file(s)  ({})",
                cat.name(),
                cat.relative_dir()
            );
        }
        let r = report(m);
        let (c, total) = r.consumed_fraction();
        println!(
            "      support: {c}/{total} consumed, {} parsed, {} resolved-only, {} out-of-scope",
            r.count(Support::Parsed),
            r.count(Support::ResolvedOnly),
            r.count(Support::OutOfScope)
        );
        if !r.vetoed_by_retail.is_empty() {
            println!(
                "      retail refuses to load: {}",
                r.vetoed_by_retail.join(", ")
            );
        }
        println!(
            "      runtime gate: {}",
            if r.fully_consumed() { "ACCEPT" } else { "REJECT (inert or unsupported files present)" }
        );
        println!();
    }
    ExitCode::SUCCESS
}

fn cmd_check(dir: &Path) -> ExitCode {
    let stack = match load(dir) {
        Ok(s) => s,
        Err(c) => return c,
    };
    if stack.mods().is_empty() {
        eprintln!("REJECT: no mod packages found under {}", dir.display());
        return ExitCode::FAILURE;
    }

    let mut rejected = 0usize;
    for m in stack.mods() {
        let r = report(m);
        if r.fully_consumed() {
            println!("ACCEPT {}: every declared file has an end-to-end consumer", m.name);
            continue;
        }
        rejected += 1;
        println!("REJECT {}:", m.name);
        for f in r.files.iter().filter(|f| f.support != Support::Consumed) {
            println!(
                "  {}/{}: {} — {}",
                f.category.relative_dir(),
                f.filename,
                f.support.label(),
                f.reason
            );
        }
        for f in &r.vetoed_by_retail {
            println!("  mapstyles/{f}: vetoed by retail");
        }
    }
    if rejected == 0 {
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "{rejected} package(s) rejected; this is a static capability gate, not a live-retail certification"
        );
        ExitCode::FAILURE
    }
}

fn cmd_probe(dir: &Path, paths: &[String]) -> ExitCode {
    let stack = match load(dir) {
        Ok(s) => s,
        Err(c) => return c,
    };
    for p in paths {
        let r = stack.resolve(p);
        let who = if r.mod_index == 0 {
            "shipped".to_string()
        } else {
            format!(
                "mod #{} ({})",
                r.mod_index,
                stack.mods()[r.mod_index - 1].name
            )
        };
        println!(
            "{p}\n  category {}\n  from     {who}\n  path     {}",
            r.category.name(),
            r.path
        );
    }
    ExitCode::SUCCESS
}

fn cmd_rules() -> ExitCode {
    println!("categories  (s_ModCategoryInfo 0x00C07AD0)");
    for (i, c) in CATEGORY_INFO.iter().enumerate() {
        println!(
            "  {i:>2} {:<10} {:<14} recursive={}",
            c.name, c.relative_dir, c.recursive
        );
    }
    println!("\nwhat a mod may contain  (s_SteamWorkshopTagLinks 0x00C068D0)");
    for cat in ALL_CATEGORIES {
        let pats: Vec<&str> = TAG_LINKS
            .iter()
            .filter(|l| l.category == cat)
            .map(|l| l.pattern)
            .collect();
        if !pats.is_empty() {
            println!("  {:<10} {}", cat.name(), pats.join(" "));
        }
    }
    println!(
        "\nnot replaceable  (ModManager::isMapForbidden 0x00A21140, {} names, CAT_MAPSTYLES only)",
        FORBIDDEN_MAPSTYLES.len()
    );
    for chunk in FORBIDDEN_MAPSTYLES.chunks(4) {
        println!("  {}", chunk.join("  "));
    }
    ExitCode::SUCCESS
}
