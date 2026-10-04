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

use std::collections::BTreeMap;
use std::path::PathBuf;

use don_state::generated::layout::{self, ClassLayout};
use don_state::{container, load, Span};

/// How far ahead to scan for a matching path when the two traces diverge.
const RESYNC: usize = 64;

/// Image-relative base of the serialized body range for spans whose path
/// does not encode it (the bases come from the walk_data transcriptions in
/// `sections.rs`, not from the PDB).
fn image_base(path: &str, class: &str, len: usize) -> Option<u32> {
    let last = path.rsplit('.').next().unwrap_or(path);
    match (class, last, len) {
        // SubObject body: who,o,z,x,y,ptype_index at +9..+0x1c.
        ("SubObject", _, 19) => Some(0x9),
        // ObjectData gated mid range +0x20..+0x42.
        ("Object", _, 34) => Some(0x20),
        // UnitData body +0x48..+0xb7.
        ("Unit", "Unit", 111) => Some(0x48),
        // AnimalData +0x150..+0x155 (ox, whom, aid).
        ("Animal", "Animal", 5) => Some(0x150),
        // LeaderData body +0x08..+0x692a.
        ("Leader", _, 0x6922) | ("LeaderData", _, 0x6922) => Some(0x8),
        // CityData pod +6..+114.
        ("City", "pod", _) => Some(0x6),
        // Group 72-byte header at the image start.
        ("Group", _, 72) => Some(0x0),
        // Army +0x02..+0x98 scalars.
        ("Army", _, _) => Some(0x2),
        _ => None,
    }
}

/// The innermost path token that names a generated class layout.
/// `Leader[i]` span paths carry the LeaderData image, so prefer that.
fn span_class(path: &str) -> Option<&'static ClassLayout> {
    if path.starts_with("Leader[") || path.contains(".Leader[") {
        return layout::lookup("LeaderData");
    }
    if path.contains(".guys[") {
        return layout::lookup("Guy");
    }
    let mut best: Option<&'static ClassLayout> = None;
    let mut token = String::new();
    let mut flush = |token: &mut String| {
        if !token.is_empty() {
            if let Some(l) = layout::lookup(token) {
                best = Some(l);
            }
            token.clear();
        }
    };
    for c in path.chars() {
        if c.is_alphanumeric() || c == '_' {
            token.push(c);
        } else {
            flush(&mut token);
        }
    }
    flush(&mut token);
    best
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}

/// Ranges (relative to the span) where the two byte images differ.
/// Length differences extend the final range.
fn diff_ranges(a: &[u8], b: &[u8]) -> Vec<(usize, usize)> {
    let n = a.len().min(b.len());
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < n {
        if a[i] != b[i] {
            let s = i;
            while i < n && a[i] != b[i] {
                i += 1;
            }
            out.push((s, i));
        } else {
            i += 1;
        }
    }
    if a.len() != b.len() {
        let s = n;
        let e = a.len().max(b.len());
        match out.last_mut() {
            Some(r) if r.1 == s => r.1 = e,
            _ => out.push((s, e)),
        }
    }
    out
}

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

    // Sequential merge of the two traces with a bounded resync window.
    let (os, ns) = (&old.spans, &new.spans);
    let (mut i, mut j) = (0usize, 0usize);
    let mut structural = 0usize;
    let mut diff_count = 0usize;
    // class -> (differing spans, differing bytes)
    let mut by_class: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    while i < os.len() && j < ns.len() {
        if os[i].path == ns[j].path {
            report_span(&os[i], &ns[j], &old_raw, &new_raw, &mut by_class, &mut diff_count);
            i += 1;
            j += 1;
            continue;
        }
        // Resync: find os[i].path ahead in ns, or ns[j].path ahead in os.
        let j2 = (j..(j + RESYNC).min(ns.len())).find(|&k| ns[k].path == os[i].path);
        let i2 = (i..(i + RESYNC).min(os.len())).find(|&k| os[k].path == ns[j].path);
        match (i2, j2) {
            (None, None) => {
                println!("STRUCTURAL unresolvable divergence:");
                println!("  - [{}] {} @ {:#x} +{:#x}", i, os[i].path, os[i].offset, os[i].len);
                println!("  + [{}] {} @ {:#x} +{:#x}", j, ns[j].path, ns[j].offset, ns[j].len);
                structural += 2;
                i += 1;
                j += 1;
            }
            (Some(k), None) | (Some(k), Some(_)) => {
                println!("STRUCTURAL {} span(s) removed/absent in new:", k - i);
                for s in &os[i..k] {
                    println!("  - {} @ {:#x} +{:#x}", s.path, s.offset, s.len);
                    structural += 1;
                }
                i = k;
            }
            (None, Some(k)) => {
                println!("STRUCTURAL {} span(s) added in new:", k - j);
                for s in &ns[j..k] {
                    println!("  + {} @ {:#x} +{:#x}", s.path, s.offset, s.len);
                    structural += 1;
                }
                j = k;
            }
        }
    }
    if i < os.len() || j < ns.len() {
        println!(
            "STRUCTURAL tail divergence: {} old spans and {} new spans unaligned",
            os.len() - i,
            ns.len() - j
        );
        for s in &os[i..(i + 8).min(os.len())] {
            println!("  - {} @ {:#x} +{:#x}", s.path, s.offset, s.len);
        }
        for s in &ns[j..(j + 8).min(ns.len())] {
            println!("  + {} @ {:#x} +{:#x}", s.path, s.offset, s.len);
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
    by_class: &mut BTreeMap<String, (usize, usize)>,
    diff_count: &mut usize,
) {
    let ab = &a_raw[a.offset..a.offset + a.len];
    let bb = &b_raw[b.offset..b.offset + b.len];
    let ranges = diff_ranges(ab, bb);
    if ranges.is_empty() {
        return;
    }
    *diff_count += 1;
    let class = span_class(&a.path);
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
        println!("  old: {}", hex(ab));
        println!("  new: {}", hex(bb));
    }
    if let Some(l) = class {
        if let Some(base) = image_base(&a.path, l.name, a.len) {
            for (s, e) in &ranges {
                let fields = layout::fields_covering(l, base as usize + s, base as usize + e);
                if !fields.is_empty() {
                    println!(
                        "  +{s:#04x}..+{e:#04x} (img +{:#x}..+{:#x}): {}",
                        base as usize + s,
                        base as usize + e,
                        fields.join(", ")
                    );
                    // Hex of the differing subrange when short.
                    if e - s <= 32 {
                        println!("    old: {}", hex(&ab[*s..*e.min(&ab.len())]));
                        println!("    new: {}", hex(&bb[*s..*e.min(&bb.len())]));
                    }
                }
            }
        }
    }
}
