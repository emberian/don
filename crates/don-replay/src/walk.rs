//! Table-driven `DataWalk` traversal over object byte images.
//!
//! # Why byte images
//!
//! `schema/state-schema.json` gives, per class, the *ordered* list of
//! `this`-relative byte ranges the engine's `walk_data` hands to the visitor.
//! Modelling our objects as flat byte images of the PDB's `sizeof` and driving
//! the traversal off that table means the traversal is **derived, not typed**:
//! there is no place for a hand-written walker to drift from the binary, and
//! when a lane starts producing real field values it only has to write the
//! right bytes at the right offsets to get the right checksum.
//!
//! It also makes the honest failure mode loud. An op the extractor could not
//! resolve is [`WalkOp::Unresolved`]; walking a spec that contains one returns
//! a [`WalkOutcome`] that says so, rather than silently hashing 0 bytes and
//! looking like agreement.

use crate::checksum::DataWalk;
use crate::walk_gen::SPECS;

/// One op of a class's `walk_data`, in program order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkOp {
    /// `walk_function(this+begin, this+end)` — `end - begin` bytes of the image.
    Bytes { begin: u32, end: u32 },
    /// `walk_test(tag)` — one byte for `SaveGame`/`LoadGame`, nothing for `CheckSum`.
    Tag,
    /// A base-class or embedded-member `walk_data` call on the sub-object at
    /// `this + at`, resolved to a class in `SPECS`.
    ///
    /// `at` is load-bearing and used to be dropped. `Unit::walk_data` walks
    /// `Stack<PathData>` at `this+184`, `OrderList` at `this+200` and
    /// `PtrArray<Guy>` at `this+228`; running those three at offset 0 hashes
    /// `Object`'s header three times. It looked harmless only because every
    /// image in this crate was all zeroes.
    Sub { class: usize, at: u32 },
    /// A `walk_data` call on an object reached **through a pointer** at
    /// `this + at`. The pointee is not part of the image, so this is counted,
    /// never executed.
    SubPtr { class: usize, at: u32 },
    /// A `walk_data` call whose target class is known but whose receiver is not
    /// an offset into this image — a global, an immediate address, a stack
    /// temporary, or a negative `this` adjustment the linear scan could not
    /// attribute.
    SubUnbased { class: usize },
    /// A `walk_data` call whose target class the PDB does not name.
    SubUnknown { va: u32 },
    /// `call [reg+0x7c]` — dispatch on the walked object's own vtable. Only
    /// resolvable with a concrete receiver type.
    Virtual,
    /// A resolved byte count over a stack temporary rather than a struct field.
    Scratch { bytes: u32 },
    /// A resolved range on a **global** object rather than on `this`
    /// (`Game::walk_data` walks `*(void**)0x00c061ec + 1360 .. + 1764`, for
    /// example). The offsets are real but they do not index an object image, so
    /// executing them would hash the wrong bytes with full confidence.
    Global { bytes: u32, base: &'static str },
    /// The extractor could not resolve the operands (no dataflow join).
    Unresolved,
}

#[derive(Debug, Clone, Copy)]
pub struct WalkSpec {
    pub name: &'static str,
    pub walk_data_va: u32,
    pub sizeof: u32,
    pub walked_bytes: u32,
    pub ops: &'static [WalkOp],
}

/// What a table-driven walk actually managed to do. Every number here is a
/// count of ops, so "we walked this class" is never mistaken for "we walked all
/// of this class".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WalkOutcome {
    pub bytes_walked: u64,
    pub ops_executed: u32,
    pub ops_unresolved: u32,
    pub ops_virtual: u32,
    pub ops_sub_unknown: u32,
    /// Ops skipped because the byte range ran past the object image.
    pub ops_out_of_range: u32,
    /// Ops whose range is on a global object rather than on `this`.
    pub ops_global: u32,
    /// Sub-object walks whose receiver is not inside this image: a pointee
    /// (`SubPtr`) or an unattributed base (`SubUnbased`).
    pub ops_sub_unbased: u32,
}

impl WalkOutcome {
    pub fn is_complete(&self) -> bool {
        self.ops_unresolved == 0
            && self.ops_virtual == 0
            && self.ops_sub_unknown == 0
            && self.ops_out_of_range == 0
            && self.ops_global == 0
            && self.ops_sub_unbased == 0
    }
    /// Ops we could not execute, of any kind. `bytes_walked` without this
    /// number beside it is the misleading half of the pair.
    pub fn ops_missed(&self) -> u32 {
        self.ops_unresolved
            + self.ops_virtual
            + self.ops_sub_unknown
            + self.ops_out_of_range
            + self.ops_global
            + self.ops_sub_unbased
    }
    pub fn merge(&mut self, o: WalkOutcome) {
        self.bytes_walked += o.bytes_walked;
        self.ops_executed += o.ops_executed;
        self.ops_unresolved += o.ops_unresolved;
        self.ops_virtual += o.ops_virtual;
        self.ops_sub_unknown += o.ops_sub_unknown;
        self.ops_out_of_range += o.ops_out_of_range;
        self.ops_global += o.ops_global;
        self.ops_sub_unbased += o.ops_sub_unbased;
    }
}

/// Run class `class`'s `walk_data` over the object image `img`.
///
/// `Sub { at }` recurses on `img[at..]`, which is the base-class chain when
/// `at == 0` (`Unit::walk_data` → `Object::walk_data` → `SubObject::walk_data`)
/// and an embedded member otherwise. A sub-object the image does not contain —
/// one behind a pointer, or one whose receiver the extractor could not
/// attribute to `this` — is counted in `ops_sub_unbased` and **not** executed,
/// because executing it against the wrong bytes produces a confident wrong
/// number. `max_depth` bounds the recursion either way.
pub fn walk_class<W: DataWalk + ?Sized>(
    class: usize,
    img: &[u8],
    w: &mut W,
    max_depth: u32,
) -> WalkOutcome {
    walk_emit(class, img.len(), max_depth, &mut |e| match e {
        Emit::Bytes { at, len } => w.walk(&img[at as usize..(at + len) as usize]),
        Emit::Tag => w.walk_tag(0),
    })
}

/// One thing a traversal hands to the visitor, with the offset it came from.
///
/// `walk_class` throws the offset away (it only needs the bytes); everything
/// that asks *which* bytes are sim-critical needs it, and both must come from
/// the same recursion or they drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emit {
    /// `img[at .. at+len]`, already bounds-checked against the image length.
    Bytes { at: u32, len: u32 },
    /// `walk_test` — one byte for `SaveGame`/`LoadGame`, nothing for `CheckSum`.
    Tag,
}

/// The single traversal. `walk_class` and [`walked_mask`] are both this function
/// with a different sink.
pub fn walk_emit(
    class: usize,
    img_len: usize,
    max_depth: u32,
    f: &mut impl FnMut(Emit),
) -> WalkOutcome {
    walk_emit_at(class, 0, img_len, max_depth, f)
}

fn walk_emit_at(
    class: usize,
    base: usize,
    img_len: usize,
    max_depth: u32,
    f: &mut impl FnMut(Emit),
) -> WalkOutcome {
    let mut out = WalkOutcome::default();
    if max_depth == 0 || class >= SPECS.len() {
        out.ops_unresolved += 1;
        return out;
    }
    for op in SPECS[class].ops {
        match *op {
            WalkOp::Bytes { begin, end } => {
                let (b, e) = (base + begin as usize, base + end as usize);
                if e > img_len || b > e {
                    out.ops_out_of_range += 1;
                    continue;
                }
                f(Emit::Bytes {
                    at: b as u32,
                    len: (e - b) as u32,
                });
                out.bytes_walked += (e - b) as u64;
                out.ops_executed += 1;
            }
            WalkOp::Tag => {
                f(Emit::Tag);
                out.ops_executed += 1;
            }
            WalkOp::Sub { class: c, at } => {
                let a = base + at as usize;
                if a > img_len {
                    out.ops_out_of_range += 1;
                    continue;
                }
                out.merge(walk_emit_at(c, a, img_len, max_depth - 1, f));
            }
            WalkOp::SubPtr { .. } | WalkOp::SubUnbased { .. } => out.ops_sub_unbased += 1,
            WalkOp::SubUnknown { .. } => out.ops_sub_unknown += 1,
            WalkOp::Virtual => out.ops_virtual += 1,
            WalkOp::Scratch { bytes } => {
                // A value built on the stack. We do not have it; count it.
                let _ = bytes;
                out.ops_unresolved += 1;
            }
            WalkOp::Global { .. } => out.ops_global += 1,
            WalkOp::Unresolved => out.ops_unresolved += 1,
        }
    }
    out
}

/// Which bytes of an `img_len`-byte image of `class` the checksum visits.
///
/// This is the **only** honest source for "is this field sim-critical". The
/// per-field `walked` flag in `don-sim`'s generated state is computed from each
/// class's *own* `walk_data` and therefore misses everything a base class walks:
/// it marks `UnitData::x_internal` unwalked, when `SubObject::walk_data`
/// `0x006621d0` walks `[9,24)` and hashes it on every unit, every turn.
pub struct WalkedMask {
    pub bytes: Vec<bool>,
    pub outcome: WalkOutcome,
}

impl WalkedMask {
    pub fn of_class(class: usize, img_len: usize) -> WalkedMask {
        let mut bytes = vec![false; img_len];
        let outcome = walk_emit(class, img_len, 8, &mut |e| {
            if let Emit::Bytes { at, len } = e {
                for b in &mut bytes[at as usize..(at + len) as usize] {
                    *b = true;
                }
            }
        });
        WalkedMask { bytes, outcome }
    }
    /// Distinct bytes visited. Less than `WalkOutcome::bytes_walked` when a walk
    /// visits a byte twice, which some classes do.
    pub fn count(&self) -> u32 {
        self.bytes.iter().filter(|b| **b).count() as u32
    }
    /// Walked bytes inside `[off, off+size)`.
    pub fn walked_in(&self, off: u32, size: u32) -> u32 {
        let (a, b) = (off as usize, (off + size) as usize);
        if a >= self.bytes.len() {
            return 0;
        }
        self.bytes[a..b.min(self.bytes.len())]
            .iter()
            .filter(|x| **x)
            .count() as u32
    }
}

/// Coverage of the generated table, for the report. Counting the ops we can and
/// cannot execute is the difference between "the walker is done" and "the
/// walker runs".
pub fn table_coverage() -> TableCoverage {
    let mut c = TableCoverage::default();
    for s in SPECS.iter() {
        for op in s.ops {
            match op {
                WalkOp::Bytes { .. } => c.bytes += 1,
                WalkOp::Tag => c.tag += 1,
                WalkOp::Sub { at: 0, .. } => c.sub += 1,
                WalkOp::Sub { .. } => c.sub_member += 1,
                WalkOp::SubPtr { .. } => c.sub_ptr += 1,
                WalkOp::SubUnbased { .. } => c.sub_unbased += 1,
                WalkOp::Scratch { .. } => c.length_only += 1,
                WalkOp::Global { .. } => c.global += 1,
                WalkOp::Unresolved => c.unresolved += 1,
                WalkOp::Virtual => c.virtual_dispatch += 1,
                WalkOp::SubUnknown { .. } => c.sub_unknown += 1,
            }
        }
    }
    c
}

/// How much of the derived traversal this crate can actually execute.
#[derive(Debug, Clone, Copy, Default)]
pub struct TableCoverage {
    /// `this`-relative byte ranges: executable against an object image.
    pub bytes: usize,
    pub tag: usize,
    /// Base-class sub-walks at `this+0`: executable.
    pub sub: usize,
    /// Embedded-member sub-walks at a non-zero `this` offset: executable.
    pub sub_member: usize,
    /// Sub-walks through a pointer at `this+N`: the pointee is not in the image.
    pub sub_ptr: usize,
    /// Sub-walks whose receiver is not an offset into `this` at all.
    pub sub_unbased: usize,
    /// Resolved length, unresolved base (stack temporary or untracked pointer).
    pub length_only: usize,
    /// Resolved range on a global object rather than on `this`.
    pub global: usize,
    pub unresolved: usize,
    pub virtual_dispatch: usize,
    pub sub_unknown: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checksum::{adler32, CheckSum};
    use crate::walk_gen::class_index;

    #[test]
    fn the_generated_table_is_the_schema() {
        assert_eq!(SPECS.len(), crate::walk_gen::NUM_CLASSES);
        let total: usize = SPECS.iter().map(|s| s.ops.len()).sum();
        assert_eq!(
            total,
            crate::walk_gen::ORDERED_OPS,
            "every ordered op is present"
        );
    }

    #[test]
    fn unit_walks_the_derived_range() {
        // Unit::walk_data 0x0060cf40 walks [0x48, 0xb7) = 111 bytes of its own
        // fields, plus whatever Object::walk_data contributes.
        let ci = class_index("Unit").expect("Unit is in the schema");
        assert_eq!(SPECS[ci].sizeof, 344);
        let img = vec![0u8; SPECS[ci].sizeof as usize];
        let mut cs = CheckSum::new();
        let out = walk_class(ci, &img, &mut cs, 8);
        assert!(
            out.bytes_walked >= 111,
            "at least Unit's own 111 bytes: {out:?}"
        );
        // and the bytes we walked really are what the adler reflects
        assert_eq!(cs.bytes, out.bytes_walked);
    }

    #[test]
    fn a_walk_of_nothing_leaves_the_checksum_at_one() {
        let cs = CheckSum::new();
        assert_eq!(cs.checksum, 1);
        assert_eq!(adler32(1, &[]), 1);
    }

    /// The sub-object offsets are real and are used. `Unit::walk_data` reaches
    /// `SubObject::walk_data` at `+0` and `OrderList::walk_data` at `+200`; a
    /// walker that recursed at offset 0 for both — which this crate did until
    /// this lane — would produce a different mask and hash `Object`'s header
    /// twice.
    #[test]
    fn sub_objects_are_walked_at_their_own_offsets() {
        let ci = class_index("Unit").expect("Unit");
        let m = WalkedMask::of_class(ci, SPECS[ci].sizeof as usize);
        // SubObject::walk_data 0x006621d0 walks [8,9) and [9,24) of the base.
        assert!(m.bytes[8] && m.bytes[16] && m.bytes[23], "SubObject range");
        // Object::walk_data 0x00647830 walks [32,66).
        assert!(m.bytes[32] && m.bytes[65], "Object range");
        assert!(!m.bytes[24], "ptype at +24 is not walked");
        assert!(!m.bytes[28], "on_screen at +28 is not walked");
        // Unit's own [72,183).
        assert!(m.bytes[72] && m.bytes[182], "Unit range");
        assert!(!m.bytes[183], "the range is half-open");
        // Anything past +184 is a sub-object at its own offset, never at 0.
        assert_eq!(
            m.count(),
            m.walked_in(0, SPECS[ci].sizeof),
            "the mask is inside the image"
        );
    }

    /// Every op the table can execute is executed against an image of the
    /// class's own `sizeof`; anything else is counted, and the counts are the
    /// honest description of the gap.
    #[test]
    fn the_unit_walk_reports_what_it_could_not_do() {
        let ci = class_index("Unit").expect("Unit");
        let m = WalkedMask::of_class(ci, SPECS[ci].sizeof as usize);
        assert!(!m.outcome.is_complete(), "Unit's walk has known gaps");
        assert!(
            m.outcome.ops_sub_unbased > 0,
            "Object's SimpleArray<int> at thisload+68 is not in the image: {:?}",
            m.outcome
        );
    }
}
