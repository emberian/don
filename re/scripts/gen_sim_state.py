#!/usr/bin/env python3
"""Generate the don-sim state layer from the shipped PDB type stream.

Inputs (both [measured], both produced by earlier lanes from ron-bin/sbl/rise.pdb):

    schema/pdb-types.json    class layouts: every field with offset, size, C type,
                             plus `flattened` which folds in base-class subobjects.
    schema/state-schema.json the DataWalk pass: which byte ranges each class's
                             walk_data actually visits, in program order. Because
                             CheckSum / SaveGame / LoadGame are the only DataWalk
                             implementations, a byte inside a walked range is
                             lockstep-critical state and a byte outside it is not.

Output:

    crates/don-sim/src/generated/state.rs

The output is a structure-of-arrays column block per class. Storage is one flat
allocation per *width* (i32 / i16 / i8 / f32), sliced into planes; a scalar field owns
one plane and is therefore a contiguous `&[i32]` over rows -- the shape the batch
kernels want -- while a fixed-size array field owns `count` planes' worth of space laid
out row-major so one row's array is contiguous.

Rules the generator enforces, from the project's ground truth:

  * All integers unless the PDB says `float`. Coord / WCoord / TCoord are one `int`
    each and are emitted as i32.
  * Fields sharing an offset are anonymous-union members. They share one plane; the
    first name is canonical and the rest become alias accessors, because they *are*
    the same storage.
  * Pointers, String, and the container templates are not materialised. They are still
    emitted into the descriptor table with `Repr::Aggregate` so the coverage report
    counts the bytes we are not modelling instead of hiding them.
  * Arrays above MAX_ARRAY_ELEMS elements are `Repr::Deferred`: recorded, sized, not
    allocated. Materialising LeaderData's `unsigned short[129][64]` would cost 165 KB
    per world before a single unit exists.

Also emitted: `sine_table[256]`, the engine's integer sine lookup. It lives at
0x00E32F40 in .bss and is filled at runtime by `trig_init` 0x00A46980, whose body is
    for i in (0..256).step_by(2): store trunc(sin(i * 1.570796327 / 255.0) * 65535.0)
with all three doubles read out of .rdata (0xB69BB0, 0xB69BD0, 0xB69BE0). The formula
is [measured]; the values are computed here from it, and the generator reports how many
entries sit close enough to an integer boundary that a last-ulp difference in libm's
sin could move them.

Usage:  python3 re/scripts/gen_sim_state.py [--check]
"""

from __future__ import annotations

import json
import math
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PDB_TYPES = ROOT / "schema" / "pdb-types.json"
STATE_SCHEMA = ROOT / "schema" / "state-schema.json"
OUT = ROOT / "crates" / "don-sim" / "src" / "generated" / "state.rs"

# Arrays with more elements than this are recorded but not allocated.
MAX_ARRAY_ELEMS = 384

# class in pdb-types.json  ->  class whose walk_data defines its sim-critical bytes.
# The walker is named for the behaviour class (`Unit::walk_data`) but walks the data
# class's layout (`UnitData`), which is why these differ.
CLASSES = [
    # (pdb class,        rust module, walk class in state-schema.json)
    ("ObjectData", "object", "Object"),
    ("UnitData", "unit", "Unit"),
    ("BuildData", "build", "BuildData"),
    ("WallData", "wall", "WallData"),
    ("CityData", "city", "City"),
    ("LeaderData", "leader", "LeaderData"),
    ("AmmoData", "ammo", "AmmoData"),
    ("GuyData", "guy", "GuyData"),
    ("ObjectTypeData", "object_type", "ObjectType"),
    ("UnitTypeData", "unit_type", "UnitType"),
    ("BuildTypeData", "build_type", "BuildType"),
    ("TechTypeData", "tech_type", None),
]

# ---------------------------------------------------------------------------------
# C type -> storage
# ---------------------------------------------------------------------------------

SCALARS = {
    "int": ("I32", "i32", 4),
    "long": ("I32", "i32", 4),
    "unsigned int": ("U32", "u32", 4),
    "unsigned long": ("U32", "u32", 4),
    "short": ("I16", "i16", 2),
    "unsigned short": ("U16", "u16", 2),
    "char": ("I8", "i8", 1),
    "signed char": ("I8", "i8", 1),
    "unsigned char": ("U8", "u8", 1),
    "bool": ("U8", "u8", 1),
    "float": ("F32", "f32", 4),
    # Coord/WCoord/TCoord each wrap exactly one int (README-LLM, "the sim is INTEGERS").
    "Coord": ("I32", "i32", 4),
    "WCoord": ("I32", "i32", 4),
    "TCoord": ("I32", "i32", 4),
}

POOL_ENUM = {"w4": "W4", "w2": "W2", "w1": "W1", "wf": "WF", None: "None"}

ARRAY_RE = re.compile(r"^(.+?)((?:\[\d+\])+)$")
DIM_RE = re.compile(r"\[(\d+)\]")

# `r#` works for most keywords; these five cannot be raw identifiers.
HARD_KEYWORDS = {"self", "Self", "crate", "super", "extern"}
KEYWORDS = {
    "as", "break", "const", "continue", "dyn", "else", "enum", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "static", "struct", "trait", "true", "type", "unsafe", "use", "where",
    "while", "async", "await", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "typeof", "unsized", "virtual", "yield", "try",
}


def classify(ctype: str, size: int):
    """-> (repr_name, rust_ty, elem_size, count) or (None, ...) for aggregates."""
    t = ctype.strip()
    if t.startswith("enum "):
        # Every enum in this build is a 4-byte int (checked: `size` is always 4).
        return ("I32", "i32", 4, 1) if size == 4 else (None, None, 0, 0)
    if t in SCALARS:
        r, rt, es = SCALARS[t]
        return (r, rt, es, 1)
    m = ARRAY_RE.match(t)
    if m:
        base, dims = m.group(1).strip(), DIM_RE.findall(m.group(2))
        count = 1
        for d in dims:
            count *= int(d)
        if base.startswith("enum "):
            base_key = "int"
        else:
            base_key = base
        if base_key in SCALARS:
            r, rt, es = SCALARS[base_key]
            if es * count == size:
                return (r, rt, es, count)
    return (None, None, 0, 0)


def ident(name: str) -> str:
    s = re.sub(r"[^0-9a-zA-Z_]", "_", name)
    if not s or s[0].isdigit():
        s = "f_" + s
    s = s[0].lower() + s[1:] if s[0].isupper() else s
    if s in HARD_KEYWORDS:
        return s + "_"
    if s in KEYWORDS:
        return "r#" + s
    return s


# Method names the column block itself defines. A PDB field is allowed to be called
# `pop`; the accessor for it then has to move out of the way. (LeaderData and
# BuildData both have a field named `pop`, which is how this list came to exist.)
RESERVED = {
    "len", "is_empty", "capacity", "with_capacity", "bytes_reserved", "push_zeroed",
    "zero_row", "copy_row", "pop", "truncate", "clone",
    "w4_pair_mut", "w4_plane", "w4_plane_mut", "w2_plane", "w2_plane_mut",
    "w1_plane", "w1_plane_mut",
    "w4_slice", "w4_slice_mut", "w4_arr", "w4_arr_mut",
    "w2_slice", "w2_slice_mut", "w2_arr", "w2_arr_mut",
    "w1_slice", "w1_slice_mut", "w1_arr", "w1_arr_mut",
    "wf_slice", "wf_slice_mut", "wf_arr", "wf_arr_mut",
}


def method_name(name: str) -> str:
    """Accessor names cannot be raw identifiers in every position we use them, and a
    trailing `_mut` on `r#type` would be `r#type_mut` which is not a keyword at all."""
    s = re.sub(r"[^0-9a-zA-Z_]", "_", name)
    if not s or s[0].isdigit():
        s = "f_" + s
    s = s[0].lower() + s[1:] if s[0].isupper() else s
    if s in KEYWORDS or s in HARD_KEYWORDS or s in RESERVED:
        return s + "_"
    return s


# ---------------------------------------------------------------------------------
# walked-byte sets
# ---------------------------------------------------------------------------------


def walked_ranges(state, walk_class):
    """Ordered [begin, end) byte ranges that this class's walk_data visits."""
    if not walk_class:
        return None
    c = state["classes"].get(walk_class)
    if c is None:
        return None
    out = []
    for op in c["ops"]:
        if op.get("kind") != "bytes":
            continue
        b, e = op.get("begin"), op.get("end")
        if b is None or e is None:
            continue
        out.append((int(b), int(e), int(op.get("guard_depth", 0)), bool(op.get("in_loop"))))
    return out


def covers(ranges, off, size):
    if ranges is None:
        return None
    lo, hi = off, off + size
    remaining = [(lo, hi)]
    for b, e, _, _ in ranges:
        nxt = []
        for x, y in remaining:
            if e <= x or b >= y:
                nxt.append((x, y))
                continue
            if b > x:
                nxt.append((x, b))
            if e < y:
                nxt.append((e, y))
        remaining = nxt
        if not remaining:
            return True
    return False


# ---------------------------------------------------------------------------------
# sine table
# ---------------------------------------------------------------------------------


def sine_table():
    """trig_init 0x00A46980, transcribed. Returns (values, at_risk_count)."""
    vals, risk = [], 0
    for i in range(256):
        x = math.sin(i * 1.570796327 / 255.0) * 65535.0
        v = int(x)  # cvttpd2dq truncates toward zero
        if abs(x - round(x)) < 1e-6:
            risk += 1
        vals.append(v)
    return vals, risk


# ---------------------------------------------------------------------------------
# emit
# ---------------------------------------------------------------------------------


def build_class(pdb, state, pdb_name, mod, walk_class):
    c = pdb["classes"].get(pdb_name)
    if c is None:
        raise SystemExit(f"class {pdb_name} not in pdb-types.json")
    fields = c.get("flattened") or c.get("fields") or []
    ranges = walked_ranges(state, walk_class)

    # Dedupe: (offset, name) is unique; same offset different name = union member.
    seen = {}
    ordered = []
    for f in fields:
        key = (f["offset"], f["name"])
        if key in seen:
            continue
        seen[key] = True
        ordered.append(f)
    ordered.sort(key=lambda f: (f["offset"], f["name"]))

    planes = {"I32": 0, "U32": 0, "I16": 0, "U16": 0, "I8": 0, "U8": 0, "F32": 0}
    pool_of = {"I32": "w4", "U32": "w4", "I16": "w2", "U16": "w2", "I8": "w1", "U8": "w1", "F32": "wf"}
    pool_next = {"w4": 0, "w2": 0, "w1": 0, "wf": 0}

    entries = []          # descriptor rows
    by_offset = {}        # offset -> canonical entry index
    for f in ordered:
        name, off, size, ctype = f["name"], f["offset"], f["size"], f["type"]
        if f.get("bitfield"):
            entries.append(dict(name=name, off=off, size=size, ctype=ctype, repr="Bitfield",
                                rust=None, count=0, pool=None, plane=0,
                                declared_in=f.get("declared_in", pdb_name),
                                walked=covers(ranges, off, size), alias_of=None))
            continue
        rep, rust, esz, count = classify(ctype, size)
        if rep is None:
            entries.append(dict(name=name, off=off, size=size, ctype=ctype, repr="Aggregate",
                                rust=None, count=0, pool=None, plane=0,
                                declared_in=f.get("declared_in", pdb_name),
                                walked=covers(ranges, off, size), alias_of=None))
            continue
        if count > MAX_ARRAY_ELEMS:
            entries.append(dict(name=name, off=off, size=size, ctype=ctype, repr="Deferred",
                                rust=rust, count=count, pool=None, plane=0,
                                declared_in=f.get("declared_in", pdb_name),
                                walked=covers(ranges, off, size), alias_of=None))
            continue
        if off in by_offset:
            canon = entries[by_offset[off]]
            # Only alias when the storage really is the same shape.
            if canon["repr"] == rep and canon["count"] == count:
                entries.append(dict(name=name, off=off, size=size, ctype=ctype, repr=rep,
                                    rust=rust, count=count, pool=canon["pool"],
                                    plane=canon["plane"],
                                    declared_in=f.get("declared_in", pdb_name),
                                    walked=covers(ranges, off, size),
                                    alias_of=canon["name"]))
                continue
        pool = pool_of[rep]
        plane = pool_next[pool]
        pool_next[pool] += count
        e = dict(name=name, off=off, size=size, ctype=ctype, repr=rep, rust=rust,
                 count=count, pool=pool, plane=plane,
                 declared_in=f.get("declared_in", pdb_name),
                 walked=covers(ranges, off, size), alias_of=None)
        by_offset.setdefault(off, len(entries))
        entries.append(e)

    return dict(pdb=pdb_name, mod=mod, walk_class=walk_class, sizeof=c["size"],
                entries=entries, pool_planes=dict(pool_next), ranges=ranges,
                walk_va=(state["classes"].get(walk_class) or {}).get("walk_data"))


def emit(classes, sine, risk):
    o = []
    w = o.append
    w("//! Simulation state, generated from the shipped PDB.")
    w("//!")
    w("//! GENERATED by `re/scripts/gen_sim_state.py` from `schema/pdb-types.json` and")
    w("//! `schema/state-schema.json`. Do not edit by hand; re-run the generator.")
    w("//!")
    w("//! Every offset, size and C type here is the MSVC-emitted layout for")
    w("//! `riseofnations.exe` sha256 `30478a44..625079` -- [measured], not reconstructed.")
    w("//! `walked` marks a field whose bytes the class's `walk_data` visits, which is")
    w("//! exactly the lockstep/save-game critical set: `CheckSum`, `SaveGame` and")
    w("//! `LoadGame` are the only implementations of the `DataWalk` visitor.")
    w("//!")
    w("//! Storage is one allocation per width, sliced into planes. A scalar field owns")
    w("//! one plane and reads back as a contiguous slice over rows. An array field owns")
    w("//! `count` planes laid out row-major, so one row's array is contiguous instead.")
    w("")
    w("#![allow(dead_code)]")
    w("#![allow(non_snake_case)]")
    w("#![allow(clippy::identity_op)]")
    w("")
    w("/// How a PDB field is stored here.")
    w("#[derive(Clone, Copy, PartialEq, Eq, Debug)]")
    w("pub enum Repr {")
    w("    I32, U32, I16, U16, I8, U8, F32,")
    w("    /// A container, pointer or `String`: recorded so the coverage report counts")
    w("    /// its bytes, deliberately not materialised.")
    w("    Aggregate,")
    w("    /// A scalar array too large to allocate per world (see MAX_ARRAY_ELEMS).")
    w("    Deferred,")
    w("    /// A bitfield packed into a neighbouring storage unit.")
    w("    Bitfield,")
    w("}")
    w("")
    w("impl Repr {")
    w("    /// Whether this field has real columns behind it.")
    w("    pub fn materialised(self) -> bool {")
    w("        !matches!(self, Repr::Aggregate | Repr::Deferred | Repr::Bitfield)")
    w("    }")
    w("}")
    w("")
    w("/// Which width pool a materialised field lives in.")
    w("#[derive(Clone, Copy, PartialEq, Eq, Debug)]")
    w("pub enum Pool { W4, W2, W1, WF, None }")
    w("")
    w("/// One field of one class, exactly as the compiler laid it out.")
    w("#[derive(Clone, Copy, Debug)]")
    w("pub struct FieldDesc {")
    w("    pub name: &'static str,")
    w("    /// Byte offset inside the class, including base subobjects.")
    w("    pub offset: u32,")
    w("    /// Byte size of the whole field (an array counts every element).")
    w("    pub size: u32,")
    w("    /// The C++ type spelled as the PDB spells it.")
    w("    pub ctype: &'static str,")
    w("    /// Which class in the inheritance chain declared it.")
    w("    pub declared_in: &'static str,")
    w("    pub repr: Repr,")
    w("    /// Element count: 1 for a scalar, N for `T[N]`, 0 for an aggregate.")
    w("    pub count: u32,")
    w("    /// `Some(true)` if `walk_data` visits every byte, `Some(false)` if it visits")
    w("    /// none or only part, `None` if no walker was recovered for this class.")
    w("    pub walked: Option<bool>,")
    w("    /// Non-null when this field shares storage with an earlier one (an anonymous")
    w("    /// union member).")
    w("    pub alias_of: Option<&'static str>,")
    w("    /// Which width pool holds it.")
    w("    pub pool: Pool,")
    w("    /// Plane index inside that pool. An array field owns `count` consecutive")
    w("    /// planes' worth of space, laid out row-major.")
    w("    pub plane: u32,")
    w("}")
    w("")
    w("/// One byte range that a class's `walk_data` visits, in program order.")
    w("#[derive(Clone, Copy, Debug)]")
    w("pub struct WalkOp {")
    w("    pub begin: u32,")
    w("    pub end: u32,")
    w("    /// Conditional forward jumps spanning the op. An over-approximation -- see")
    w("    /// the caveats in `schema/state-schema.json`.")
    w("    pub guard_depth: u32,")
    w("    pub in_loop: bool,")
    w("}")
    w("")
    w("/// A whole class: its true `sizeof`, its fields, and its walker.")
    w("#[derive(Clone, Copy, Debug)]")
    w("pub struct ClassDesc {")
    w("    pub name: &'static str,")
    w("    pub sizeof: u32,")
    w("    pub fields: &'static [FieldDesc],")
    w("    /// The class whose `walk_data` defines the sim-critical byte set.")
    w("    pub walk_class: Option<&'static str>,")
    w("    /// VA of that `walk_data`, for anyone re-checking against the binary.")
    w("    pub walk_va: Option<&'static str>,")
    w("    pub walk_ops: &'static [WalkOp],")
    w("}")
    w("")
    w("impl ClassDesc {")
    w("    pub fn field(&self, name: &str) -> Option<&'static FieldDesc> {")
    w("        self.fields.iter().find(|f| f.name == name)")
    w("    }")
    w("    /// (materialised fields, total fields, materialised bytes, sizeof).")
    w("    pub fn coverage(&self) -> (usize, usize, u32, u32) {")
    w("        let mut nf = 0usize;")
    w("        let mut nb = 0u32;")
    w("        for f in self.fields {")
    w("            if f.alias_of.is_some() { continue; }")
    w("            if f.repr.materialised() { nf += 1; nb += f.size; }")
    w("        }")
    w("        let total = self.fields.iter().filter(|f| f.alias_of.is_none()).count();")
    w("        (nf, total, nb, self.sizeof)")
    w("    }")
    w("    /// Bytes this class's `walk_data` visits that we do materialise.")
    w("    pub fn walked_materialised_bytes(&self) -> u32 {")
    w("        self.fields.iter()")
    w("            .filter(|f| f.alias_of.is_none() && f.repr.materialised() && f.walked == Some(true))")
    w("            .map(|f| f.size).sum()")
    w("    }")
    w("}")
    w("")

    for cls in classes:
        emit_class(w, cls)

    w("/// Every generated class, for the coverage report.")
    w(f"pub const CLASSES: [ClassDesc; {len(classes)}] = [")
    for cls in classes:
        w(f"    {cls['mod']}::DESC,")
    w("];")
    w("")

    # --- sine table ---
    w("/// `int sine_table[256]` at `0x00E32F40`.")
    w("///")
    w("/// The array is zero in the image (it sits past `.data`'s raw size) and is filled")
    w("/// at startup by `trig_init` `0x00A46980`, whose body is exactly")
    w("/// `trunc(sin(i * 1.570796327 / 255.0) * 65535.0)` -- the three doubles read from")
    w("/// `.rdata` at `0xB69BB0`, `0xB69BD0`, `0xB69BE0`, and 255 (not 256) is the")
    w("/// divisor, so index 255 is 90 degrees. **The formula is [measured]; these values")
    w("/// are computed from it**, so they inherit whatever `_libm_sse2_sin_precise`")
    w(f"/// rounds to. {risk} of 256 entries land within 1e-6 of an integer, which is the")
    w("/// only place a last-ulp difference could change a truncation.")
    w("pub const SINE_TABLE: [i32; 256] = [")
    for i in range(0, 256, 8):
        w("    " + ", ".join(str(v) for v in sine[i:i + 8]) + ",")
    w("];")
    return "\n".join(o) + "\n"


def emit_class(w, cls):
    mod, pdb_name = cls["mod"], cls["pdb"]
    ent = cls["entries"]
    struct = "".join(p.capitalize() for p in mod.split("_")) + "Cols"
    w(f"// ================================================================================")
    w(f"// {pdb_name} -- sizeof {cls['sizeof']}")
    w(f"// ================================================================================")
    w("")
    w(f"pub mod {mod} {{")
    w("    use super::{ClassDesc, FieldDesc, Pool, Repr, WalkOp};")
    w("")
    nf = len([e for e in ent])
    w(f"    pub const FIELDS: [FieldDesc; {nf}] = [")
    for e in ent:
        walked = "None" if e["walked"] is None else ("Some(true)" if e["walked"] else "Some(false)")
        alias = "None" if e["alias_of"] is None else f'Some("{e["alias_of"]}")'
        w(f'        FieldDesc {{ name: "{e["name"]}", offset: {e["off"]}, size: {e["size"]}, '
          f'ctype: "{e["ctype"]}", declared_in: "{e["declared_in"]}", '
          f'repr: Repr::{e["repr"]}, count: {e["count"]}, walked: {walked}, alias_of: {alias}, '
          f'pool: Pool::{POOL_ENUM[e["pool"]]}, plane: {e["plane"]} }},')
    w("    ];")
    w("")
    ops = cls["ranges"] or []
    w(f"    pub const WALK_OPS: [WalkOp; {len(ops)}] = [")
    for b, e2, g, l in ops:
        w(f"        WalkOp {{ begin: {b}, end: {e2}, guard_depth: {g}, in_loop: {str(l).lower()} }},")
    w("    ];")
    w("")
    wc = "None" if not cls["walk_class"] else f'Some("{cls["walk_class"]}")'
    wva = "None" if not cls["walk_va"] else f'Some("{cls["walk_va"]}")'
    w("    pub const DESC: ClassDesc = ClassDesc {")
    w(f'        name: "{pdb_name}",')
    w(f"        sizeof: {cls['sizeof']},")
    w("        fields: &FIELDS,")
    w(f"        walk_class: {wc},")
    w(f"        walk_va: {wva},")
    w("        walk_ops: &WALK_OPS,")
    w("    };")
    w("")
    pp = cls["pool_planes"]
    w(f"    /// i32 planes.")
    w(f"    pub const W4_PLANES: usize = {pp['w4']};")
    w(f"    /// i16 planes.")
    w(f"    pub const W2_PLANES: usize = {pp['w2']};")
    w(f"    /// i8 planes.")
    w(f"    pub const W1_PLANES: usize = {pp['w1']};")
    w(f"    /// f32 planes.")
    w(f"    pub const WF_PLANES: usize = {pp['wf']};")
    w("")
    w(f"    /// Structure-of-arrays columns for `{pdb_name}`.")
    w("    #[derive(Clone)]")
    w(f"    pub struct {struct} {{")
    w("        cap: usize,")
    w("        len: usize,")
    w("        w4: Vec<i32>,")
    w("        w2: Vec<i16>,")
    w("        w1: Vec<i8>,")
    w("        wf: Vec<f32>,")
    w("    }")
    w("")
    w(f"    impl {struct} {{")
    w("        pub fn with_capacity(cap: usize) -> Self {")
    w(f"            {struct} {{")
    w("                cap,")
    w("                len: 0,")
    w("                w4: vec![0; cap * W4_PLANES],")
    w("                w2: vec![0; cap * W2_PLANES],")
    w("                w1: vec![0; cap * W1_PLANES],")
    w("                wf: vec![0.0; cap * WF_PLANES],")
    w("            }")
    w("        }")
    w("        #[inline] pub fn len(&self) -> usize { self.len }")
    w("        #[inline] pub fn is_empty(&self) -> bool { self.len == 0 }")
    w("        #[inline] pub fn capacity(&self) -> usize { self.cap }")
    w("        /// Bytes of column storage reserved, whatever the population.")
    w("        pub fn bytes_reserved(&self) -> usize {")
    w("            self.cap * (W4_PLANES * 4 + W2_PLANES * 2 + W1_PLANES + WF_PLANES * 4)")
    w("        }")
    w("        /// Append a zeroed row. Returns its index, or `None` at capacity.")
    w("        pub fn push_zeroed(&mut self) -> Option<usize> {")
    w("            if self.len >= self.cap { return None; }")
    w("            let row = self.len;")
    w("            self.len += 1;")
    w("            self.zero_row(row);")
    w("            Some(row)")
    w("        }")
    w("        pub fn zero_row(&mut self, row: usize) {")
    w("            for p in 0..W4_PLANES { self.w4[p * self.cap + row] = 0; }")
    w("            for p in 0..W2_PLANES { self.w2[p * self.cap + row] = 0; }")
    w("            for p in 0..W1_PLANES { self.w1[p * self.cap + row] = 0; }")
    w("            for p in 0..WF_PLANES { self.wf[p * self.cap + row] = 0.0; }")
    w("        }")
    w("        /// Copy every column of `src` over `dst`. This is the swap half of a")
    w("        /// swap-remove, so it must touch every plane -- a missed one is a field")
    w("        /// that silently survives a despawn.")
    w("        pub fn copy_row(&mut self, dst: usize, src: usize) {")
    w("            if dst == src { return; }")
    w("            let cap = self.cap;")
    w("            for p in 0..W4_PLANES { self.w4[p * cap + dst] = self.w4[p * cap + src]; }")
    w("            for p in 0..W2_PLANES { self.w2[p * cap + dst] = self.w2[p * cap + src]; }")
    w("            for p in 0..W1_PLANES { self.w1[p * cap + dst] = self.w1[p * cap + src]; }")
    w("            for p in 0..WF_PLANES { self.wf[p * cap + dst] = self.wf[p * cap + src]; }")
    w("        }")
    w("        /// Drop the last row.")
    w("        pub fn pop(&mut self) { if self.len > 0 { self.len -= 1; } }")
    w("        pub fn truncate(&mut self, n: usize) { if n < self.len { self.len = n; } }")
    w("        /// Two distinct i32 planes, both mutable. Plane ids come from the `p_*`")
    w("        /// constants below.")
    w("        pub fn w4_pair_mut(&mut self, a: usize, b: usize) -> (&mut [i32], &mut [i32]) {")
    w("            assert_ne!(a, b, \"a plane cannot be split against itself\");")
    w("            let (cap, len) = (self.cap, self.len);")
    w("            if a < b {")
    w("                let (lo, hi) = self.w4.split_at_mut(b * cap);")
    w("                (&mut lo[a * cap..a * cap + len], &mut hi[..len])")
    w("            } else {")
    w("                let (lo, hi) = self.w4.split_at_mut(a * cap);")
    w("                (&mut hi[..len], &mut lo[b * cap..b * cap + len])")
    w("            }")
    w("        }")
    w("        #[inline] pub fn w4_plane(&self, p: usize) -> &[i32] { &self.w4[p * self.cap..p * self.cap + self.len] }")
    w("        #[inline] pub fn w4_plane_mut(&mut self, p: usize) -> &mut [i32] { let (c, l) = (self.cap, self.len); &mut self.w4[p * c..p * c + l] }")
    w("        #[inline] pub fn w2_plane(&self, p: usize) -> &[i16] { &self.w2[p * self.cap..p * self.cap + self.len] }")
    w("        #[inline] pub fn w2_plane_mut(&mut self, p: usize) -> &mut [i16] { let (c, l) = (self.cap, self.len); &mut self.w2[p * c..p * c + l] }")
    w("        #[inline] pub fn w1_plane(&self, p: usize) -> &[i8] { &self.w1[p * self.cap..p * self.cap + self.len] }")
    w("        #[inline] pub fn w1_plane_mut(&mut self, p: usize) -> &mut [i8] { let (c, l) = (self.cap, self.len); &mut self.w1[p * c..p * c + l] }")
    w("")

    # plane id constants + accessors
    pool_ty = {"w4": ("i32", "w4"), "w2": ("i16", "w2"), "w1": ("i8", "w1"), "wf": ("f32", "wf")}
    for e in ent:
        if not e["pool"]:
            continue
        m = method_name(e["name"])
        store_ty, pool = pool_ty[e["pool"]]
        nat = e["rust"]
        w(f"        /// `{e['ctype']} {e['name']}` at +{e['off']} (`{e['declared_in']}`)"
          + ("" if e["alias_of"] is None else f", union alias of `{e['alias_of']}`"))
        w(f"        pub const P_{m.upper().lstrip('_')}: usize = {e['plane']};")
        if e["count"] == 1:
            w(f"        #[inline] pub fn {m}(&self) -> &[{store_ty}] {{ self.{pool}_slice({e['plane']}) }}")
            w(f"        #[inline] pub fn {m}_mut(&mut self) -> &mut [{store_ty}] {{ self.{pool}_slice_mut({e['plane']}) }}")
            if nat != store_ty:
                w(f"        #[inline] pub fn get_{m}(&self, row: usize) -> {nat} {{ self.{m}()[row] as {nat} }}")
                w(f"        #[inline] pub fn set_{m}(&mut self, row: usize, v: {nat}) {{ self.{m}_mut()[row] = v as {store_ty}; }}")
        else:
            n = e["count"]
            w(f"        #[inline] pub fn {m}(&self, row: usize) -> &[{store_ty}] {{ self.{pool}_arr({e['plane']}, row, {n}) }}")
            w(f"        #[inline] pub fn {m}_mut(&mut self, row: usize) -> &mut [{store_ty}] {{ self.{pool}_arr_mut({e['plane']}, row, {n}) }}")
    w("")
    # pool helpers
    for pool, (ty, _) in pool_ty.items():
        w(f"        #[inline] pub fn {pool}_slice(&self, p: usize) -> &[{ty}] {{ &self.{pool}[p * self.cap..p * self.cap + self.len] }}")
        w(f"        #[inline] pub fn {pool}_slice_mut(&mut self, p: usize) -> &mut [{ty}] {{ let (c, l) = (self.cap, self.len); &mut self.{pool}[p * c..p * c + l] }}")
        w(f"        #[inline] pub fn {pool}_arr(&self, p: usize, row: usize, n: usize) -> &[{ty}] {{ let b = p * self.cap + row * n; &self.{pool}[b..b + n] }}")
        w(f"        #[inline] pub fn {pool}_arr_mut(&mut self, p: usize, row: usize, n: usize) -> &mut [{ty}] {{ let b = p * self.cap + row * n; &mut self.{pool}[b..b + n] }}")
    w("    }")
    w("}")
    w("")
    w(f"pub use {mod}::{struct};")
    w("")


def main():
    pdb = json.loads(PDB_TYPES.read_text())
    state = json.loads(STATE_SCHEMA.read_text())
    classes = [build_class(pdb, state, a, b, c) for a, b, c in CLASSES]
    sine, risk = sine_table()
    text = emit(classes, sine, risk)

    if "--check" in sys.argv:
        cur = OUT.read_text() if OUT.exists() else ""
        if cur != text:
            print("state.rs is STALE -- re-run re/scripts/gen_sim_state.py", file=sys.stderr)
            return 1
        print("state.rs is up to date")
        return 0

    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(text)

    tot_f = tot_m = 0
    print(f"{'class':16s} {'sizeof':>7s} {'fields':>7s} {'matl':>6s} {'bytes':>7s} {'walked':>7s} {'planes(4/2/1/f)':>18s}")
    for c in classes:
        ent = [e for e in c["entries"] if e["alias_of"] is None]
        matl = [e for e in ent if e["pool"]]
        wb = sum(e["size"] for e in matl if e["walked"])
        mb = sum(e["size"] for e in matl)
        pp = c["pool_planes"]
        tot_f += len(ent)
        tot_m += len(matl)
        print(f"{c['pdb']:16s} {c['sizeof']:7d} {len(ent):7d} {len(matl):6d} {mb:7d} {wb:7d} "
              f"{pp['w4']:5d}/{pp['w2']}/{pp['w1']}/{pp['wf']}")
    print(f"total fields {tot_f}, materialised {tot_m}")
    print(f"sine_table: {risk}/256 entries within 1e-6 of an integer boundary")
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
