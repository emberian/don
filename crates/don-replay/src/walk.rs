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
    /// A base-class or member `walk_data` call, resolved to a class in `SPECS`.
    Sub { class: usize },
    /// A `walk_data` call whose target class the PDB does not name.
    SubUnknown { va: u32 },
    /// `call [reg+0x7c]` — dispatch on the walked object's own vtable. Only
    /// resolvable with a concrete receiver type.
    Virtual,
    /// A resolved byte count over a stack temporary rather than a struct field.
    Scratch { bytes: u32 },
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
}

impl WalkOutcome {
    pub fn is_complete(&self) -> bool {
        self.ops_unresolved == 0
            && self.ops_virtual == 0
            && self.ops_sub_unknown == 0
            && self.ops_out_of_range == 0
    }
    pub fn merge(&mut self, o: WalkOutcome) {
        self.bytes_walked += o.bytes_walked;
        self.ops_executed += o.ops_executed;
        self.ops_unresolved += o.ops_unresolved;
        self.ops_virtual += o.ops_virtual;
        self.ops_sub_unknown += o.ops_sub_unknown;
        self.ops_out_of_range += o.ops_out_of_range;
    }
}

/// Run class `class`'s `walk_data` over the object image `img`.
///
/// `Sub` ops recurse on the *same* image, which is correct for base-class
/// chains (`Unit::walk_data` → `Object::walk_data` → `ObjectData::walk_data`,
/// all at `this+0`) and wrong for member sub-objects at a non-zero offset. The
/// schema records the target class but not the member offset, so member
/// sub-walks are the known gap; `max_depth` bounds the recursion either way.
pub fn walk_class<W: DataWalk + ?Sized>(
    class: usize,
    img: &[u8],
    w: &mut W,
    max_depth: u32,
) -> WalkOutcome {
    let mut out = WalkOutcome::default();
    if max_depth == 0 || class >= SPECS.len() {
        out.ops_unresolved += 1;
        return out;
    }
    for op in SPECS[class].ops {
        match *op {
            WalkOp::Bytes { begin, end } => {
                let (b, e) = (begin as usize, end as usize);
                if e > img.len() || b > e {
                    out.ops_out_of_range += 1;
                    continue;
                }
                w.walk(&img[b..e]);
                out.bytes_walked += (e - b) as u64;
                out.ops_executed += 1;
            }
            WalkOp::Tag => {
                w.walk_tag(0);
                out.ops_executed += 1;
            }
            WalkOp::Sub { class: c } => {
                out.merge(walk_class(c, img, w, max_depth - 1));
            }
            WalkOp::SubUnknown { .. } => out.ops_sub_unknown += 1,
            WalkOp::Virtual => out.ops_virtual += 1,
            WalkOp::Scratch { bytes } => {
                // A value built on the stack. We do not have it; count it.
                let _ = bytes;
                out.ops_unresolved += 1;
            }
            WalkOp::Unresolved => out.ops_unresolved += 1,
        }
    }
    out
}

/// Coverage of the generated table, for the report. Counting the ops we can and
/// cannot execute is the difference between "the walker is done" and "the
/// walker runs".
pub fn table_coverage() -> (usize, usize, usize, usize, usize) {
    let (mut bytes, mut tag, mut sub, mut unres, mut other) = (0, 0, 0, 0, 0);
    for s in SPECS.iter() {
        for op in s.ops {
            match op {
                WalkOp::Bytes { .. } => bytes += 1,
                WalkOp::Tag => tag += 1,
                WalkOp::Sub { .. } => sub += 1,
                WalkOp::Unresolved | WalkOp::Scratch { .. } => unres += 1,
                WalkOp::Virtual | WalkOp::SubUnknown { .. } => other += 1,
            }
        }
    }
    (bytes, tag, sub, unres, other)
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
        assert_eq!(total, crate::walk_gen::ORDERED_OPS, "every ordered op is present");
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
        assert!(out.bytes_walked >= 111, "at least Unit's own 111 bytes: {out:?}");
        // and the bytes we walked really are what the adler reflects
        assert_eq!(cs.bytes, out.bytes_walked);
    }

    #[test]
    fn a_walk_of_nothing_leaves_the_checksum_at_one() {
        let mut cs = CheckSum::new();
        assert_eq!(cs.checksum, 1);
        let _ = cs;
        assert_eq!(adler32(1, &[]), 1);
    }
}
