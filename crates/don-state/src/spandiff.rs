//! Shared span-trace alignment and byte-diff machinery, factored out of the
//! `svx-diff` binary so `frame-burndown` can reuse it.
//!
//! Span paths are deterministic under the walk grammar, so two span traces
//! align by path; count changes surface as `Added`/`Removed` structural
//! events instead of silent byte-offset drift.

use std::collections::BTreeMap;

use crate::generated::layout::{self, ClassLayout};
use crate::Span;

/// How far ahead to scan for a matching path when the two traces diverge.
pub const RESYNC: usize = 64;

/// One merge event between the old and new span traces.
pub enum AlignEvent {
    /// `old_spans[i]` and `new_spans[j]` share a deterministic path.
    Aligned(usize, usize),
    /// Spans present only in the old trace (indices into `old_spans`).
    Removed(Vec<usize>),
    /// Spans present only in the new trace (indices into `new_spans`).
    Added(Vec<usize>),
    /// Neither path appears within the resync window.
    Unresolved(usize, usize),
    /// Traces ended at different lengths; remaining indices unaligned.
    Tail { old: usize, new: usize },
}

/// Sequential merge of two span traces by path with a bounded resync
/// window — the alignment layer every byte-level comparison is built on,
/// because raw file offsets shift whenever a variable-length collection
/// changes size.
pub fn align(old_spans: &[Span], new_spans: &[Span]) -> Vec<AlignEvent> {
    let mut events = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < old_spans.len() && j < new_spans.len() {
        if old_spans[i].path == new_spans[j].path {
            events.push(AlignEvent::Aligned(i, j));
            i += 1;
            j += 1;
            continue;
        }
        let j2 = (j..(j + RESYNC).min(new_spans.len())).find(|&k| new_spans[k].path == old_spans[i].path);
        let i2 = (i..(i + RESYNC).min(old_spans.len())).find(|&k| old_spans[k].path == new_spans[j].path);
        match (i2, j2) {
            (None, None) => {
                events.push(AlignEvent::Unresolved(i, j));
                i += 1;
                j += 1;
            }
            (Some(k), None) | (Some(k), Some(_)) => {
                events.push(AlignEvent::Removed((i..k).collect()));
                i = k;
            }
            (None, Some(k)) => {
                events.push(AlignEvent::Added((j..k).collect()));
                j = k;
            }
        }
    }
    if i < old_spans.len() || j < new_spans.len() {
        events.push(AlignEvent::Tail {
            old: old_spans.len() - i,
            new: new_spans.len() - j,
        });
    }
    events
}

/// Ranges (relative to the span) where the two byte images differ.
/// Length differences extend the final range.
pub fn diff_ranges(a: &[u8], b: &[u8]) -> Vec<(usize, usize)> {
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

/// Image-relative base of the serialized body range for spans whose path
/// does not encode it (the bases come from the walk_data transcriptions in
/// `sections.rs`, not from the PDB).
pub fn image_base(path: &str, class: &str, len: usize) -> Option<u32> {
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
pub fn span_class(path: &str) -> Option<&'static ClassLayout> {
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

/// Class name for a span path, `?` when no generated layout matches.
pub fn span_class_name(path: &str) -> &'static str {
    span_class(path).map(|c| c.name).unwrap_or("?")
}

/// PDB field names covering span-relative byte range `[s, e)` in the
/// class image, using `image_base` to place the span inside the image.
/// Returns None when the span has no known base mapping.
pub fn fields_covering(path: &str, layout: &ClassLayout, span_len: usize, s: usize, e: usize) -> Option<Vec<&'static str>> {
    let base = image_base(path, layout.name, span_len)? as usize;
    Some(layout::fields_covering(layout, base + s, base + e))
}

/// Per-span burn-down accounting for one aligned retail pair (N -> N+1)
/// plus our post-`do_frame` bytes at the same path.
///
/// `retail_changed`: positions where retail N and retail N+1 differ.
/// `explained`: of those, positions where ours == retail N+1.
/// `unexplained`: of those, positions where ours != retail N+1
/// (including positions we have no byte for).
/// `introduced`: positions where retail kept the byte (or the span has
/// no retail change) but ours differs from retail N — must be 0 for a
/// correctly ported system.
#[derive(Clone, Debug, Default)]
pub struct PairBurn {
    pub path: String,
    pub retail_changed: usize,
    pub explained: usize,
    pub unexplained: usize,
    pub introduced: usize,
    /// Span-relative ranges of unexplained retail change (for field
    /// attribution).
    pub unexplained_ranges: Vec<(usize, usize)>,
    /// Span-relative ranges we changed but retail did not.
    pub introduced_ranges: Vec<(usize, usize)>,
}

/// Compare one aligned span triple. `a`/`b` are the retail N and N+1
/// spans (same path); `ours` is our emitted bytes for the same path
/// (`None` when our trace lacks it — all retail change is unexplained
/// and any ours-only bytes are introduced by the caller's accounting).
/// `excluded` (nondeterministic field) zeroes all counters.
pub fn burn_span(
    a: &Span,
    b: &Span,
    old_raw: &[u8],
    new_raw: &[u8],
    ours: Option<&[u8]>,
    excluded: bool,
) -> PairBurn {
    let mut out = PairBurn {
        path: a.path.clone(),
        ..Default::default()
    };
    if excluded {
        return out;
    }
    let ab = &old_raw[a.offset..a.offset + a.len];
    let bb = &new_raw[b.offset..b.offset + b.len];
    let n = ab.len().max(bb.len());
    let mut run_unx: Option<(usize, usize)> = None;
    let mut run_int: Option<(usize, usize)> = None;
    for i in 0..n {
        let av = ab.get(i).copied().unwrap_or(0);
        let bv = bb.get(i).copied().unwrap_or(0);
        let ov = ours.and_then(|o| o.get(i)).copied();
        let retail_diff = av != bv;
        if retail_diff {
            out.retail_changed += 1;
            if ov == Some(bv) {
                out.explained += 1;
            } else {
                out.unexplained += 1;
                match &mut run_unx {
                    Some(r) if r.1 == i => r.1 = i + 1,
                    _ => run_unx = Some((i, i + 1)),
                }
            }
        }
        if let Some(ov) = ov {
            if ov != av && !retail_diff {
                out.introduced += 1;
                match &mut run_int {
                    Some(r) if r.1 == i => r.1 = i + 1,
                    _ => run_int = Some((i, i + 1)),
                }
            }
        }
        // Close runs at boundaries.
        if !retail_diff || ov == Some(bv) {
            if let Some(r) = run_unx.take() {
                out.unexplained_ranges.push(r);
            }
        }
        if !(ov.is_some() && ov != Some(av) && !retail_diff) {
            if let Some(r) = run_int.take() {
                out.introduced_ranges.push(r);
            }
        }
    }
    if let Some(r) = run_unx.take() {
        out.unexplained_ranges.push(r);
    }
    if let Some(r) = run_int.take() {
        out.introduced_ranges.push(r);
    }
    out
}

/// Aggregate helper: class -> (differing spans, differing bytes).
pub type ClassAgg = BTreeMap<String, (usize, usize)>;

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}
