//! `svx-diff OLD.svx NEW.svx` — field-attributed differential of two
//! consecutive retail saves.
//!
//! Loads both files through the same `DataWalk` loader used by Gate 1,
//! aligns the two span traces by their deterministic grammar paths
//! (resynchronizing across count changes and reporting them as structural
//! differences), then prints every differing span with both file offsets,
//! lengths, hex for spans <= 64 bytes, and — where the span covers a class
//! with a generated PDB layout — the retail field name(s) covering each
//! differing byte range. Ends with a per-class summary.
//!
//! The alignment, diff-range and field-attribution logic lives in
//! `don_state::spandiff` (shared with `frame-burndown`).

use std::path::PathBuf;

use don_state::spandiff::{self, AlignEvent, ClassAgg};
use don_state::{container, load, Span};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: svx-diff OLD.svx NEW.svx");
        std::process::exit(2);
    }
    let (old_svx, new_svx) = (PathBuf::from(&args[1]), PathBuf::from(&args[2]));
    let old_raw = container::load_svx(&old_svx).unwrap_or_else(|e| {
        eprintln!("{}: {e}", old_svx.display());
        std::process::exit(1);
    });
    let new_raw = container::load_svx(&new_svx).unwrap_or_else(|e| {
        eprintln!("{}: {e}", new_svx.display());
        std::process::exit(1);
    });
    let old = load(&old_raw).unwrap_or_else(|e| {
        eprintln!("{}: load stopped at {e:#?}", old_svx.display());
        std::process::exit(1);
    });
    let new = load(&new_raw).unwrap_or_else(|e| {
        eprintln!("{}: load stopped at {e:#?}", new_svx.display());
        std::process::exit(1);
    });
    println!(
        "old {}: consumed {:#x}/{:#x}, {} spans",
        old_svx.display(),
        old.consumed,
        old_raw.len(),
        old.spans.len()
    );
    println!(
        "new {}: consumed {:#x}/{:#x}, {} spans",
        new_svx.display(),
        new.consumed,
        new_raw.len(),
        new.spans.len()
    );
    if old.consumed != old_raw.len() || new.consumed != new_raw.len() {
        println!("WARNING: unparsed tail(s); residue spans are not aligned");
    }

    let mut structural = 0usize;
    let mut diff_count = 0usize;
    let mut by_class: ClassAgg = ClassAgg::new();
    for ev in spandiff::align(&old.spans, &new.spans) {
        match ev {
            AlignEvent::Aligned(i, j) => {
                report_span(&old.spans[i], &new.spans[j], &old_raw, &new_raw, &mut by_class, &mut diff_count);
            }
            AlignEvent::Removed(idxs) => {
                println!("STRUCTURAL {} span(s) removed/absent in new:", idxs.len());
                for &i in &idxs {
                    let s = &old.spans[i];
                    println!("  - {} @ {:#x} +{:#x}", s.path, s.offset, s.len);
                    structural += 1;
                }
            }
            AlignEvent::Added(idxs) => {
                println!("STRUCTURAL {} span(s) added in new:", idxs.len());
                for &j in &idxs {
                    let s = &new.spans[j];
                    println!("  + {} @ {:#x} +{:#x}", s.path, s.offset, s.len);
                    structural += 1;
                }
            }
            AlignEvent::Unresolved(i, j) => {
                println!("STRUCTURAL unresolvable divergence:");
                println!("  - [{}] {} @ {:#x} +{:#x}", i, old.spans[i].path, old.spans[i].offset, old.spans[i].len);
                println!("  + [{}] {} @ {:#x} +{:#x}", j, new.spans[j].path, new.spans[j].offset, new.spans[j].len);
                structural += 2;
            }
            AlignEvent::Tail { old, new } => {
                println!("STRUCTURAL tail divergence: {old} old spans and {new} new spans unaligned");
            }
        }
    }

    println!("\n== per-class summary ({} differing spans, {structural} structural) ==", diff_count);
    for (c, (n, bytes)) in &by_class {
        println!("  {c:<24} {n:>5} spans  {bytes:>7} bytes");
    }
}

fn report_span(
    a: &Span,
    b: &Span,
    a_raw: &[u8],
    b_raw: &[u8],
    by_class: &mut ClassAgg,
    diff_count: &mut usize,
) {
    let ab = &a_raw[a.offset..a.offset + a.len];
    let bb = &b_raw[b.offset..b.offset + b.len];
    let ranges = spandiff::diff_ranges(ab, bb);
    if ranges.is_empty() {
        return;
    }
    *diff_count += 1;
    let class = spandiff::span_class(&a.path);
    let cname = class.map(|c| c.name).unwrap_or("?");
    let nbytes: usize = ranges.iter().map(|r| r.1 - r.0).sum();
    let e = by_class.entry(cname.to_string()).or_default();
    e.0 += 1;
    e.1 += nbytes;

    println!(
        "DIFF {} old@{:#x}+{} new@{:#x}+{} [{}B]",
        a.path,
        a.offset,
        a.len,
        b.offset,
        b.len,
        nbytes
    );
    if a.len <= 64 && b.len <= 64 {
        println!("  old: {}", spandiff::hex(ab));
        println!("  new: {}", spandiff::hex(bb));
    }
    if let Some(l) = class {
        if spandiff::image_base(&a.path, l.name, a.len).is_some() {
            for (s, e) in &ranges {
                if let Some(fields) = spandiff::fields_covering(&a.path, l, a.len, *s, *e) {
                    if !fields.is_empty() {
                        let base = spandiff::image_base(&a.path, l.name, a.len).unwrap() as usize;
                        println!(
                            "  +{s:#04x}..+{e:#04x} (img +{:#x}..+{:#x}): {}",
                            base + s,
                            base + e,
                            fields.join(", ")
                        );
                        if e - s <= 32 {
                            println!("    old: {}", spandiff::hex(&ab[*s..(*e).min(ab.len())]));
                            println!("    new: {}", spandiff::hex(&bb[*s..(*e).min(bb.len())]));
                        }
                    }
                }
            }
        }
    }
}
