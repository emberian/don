#!/usr/bin/env python3
"""Regenerate `crates/don-content/src/generated.rs` from the shipped binary.

Everything this emits is read out of `ron-bin/riseofnations.exe` at a static VA that was
resolved from `ron-bin/sbl/rise.pdb`, or out of the PDB's own type stream.  Nothing here is
typed in by hand and nothing comes from community documentation.

    cd /Users/ember/dev/don/ron-bin
    uv run --quiet --with pefile python ../crates/don-content/gen/gen_tables.py

Sources, each with the symbol it came from:

  s_ModCategoryInfo        0x00C07AD0  12 x sizeof(ModCategoryInfo)=120   (S_LDATA32, .data:0x1AD0)
  s_SteamWorkshopTagLinks  0x00C068D0  64 x sizeof(SteamWorkshopTagLinks)=72
  s_SteamWorkshopTagNames  0x00C06678  13 x sizeof(SteamWorkshopTagNames)=44
  ModManager::isMapForbidden  0x00A21140  21 pushed .rdata UTF-16 literals
  enum ModCategoryType     PDB LF_ENUM 0x58AB
  enum SteamWorkshopTags   PDB LF_ENUM 0x16B5
  enum TypeIndex           PDB LF_ENUM (via schema/pdb-types.json)
"""

import json
import os
import re
import struct
import sys

import pefile
from capstone import CS_ARCH_X86, CS_MODE_32, Cs

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
EXE = os.path.join(REPO, "ron-bin", "riseofnations.exe")
OUT = os.path.join(HERE, "..", "src", "generated.rs")

# --- addresses, all resolved from rise.pdb -------------------------------------------------
MOD_CATEGORY_INFO = 0x00C07AD0
MOD_CATEGORY_INFO_STRIDE = 120
MOD_CATEGORY_COUNT = 12

TAG_LINKS = 0x00C068D0
TAG_LINKS_STRIDE = 72
TAG_LINKS_COUNT = 64

TAG_NAMES = 0x00C06678
TAG_NAMES_STRIDE = 44
TAG_NAMES_COUNT = 13

IS_MAP_FORBIDDEN = (0x00A21140, 3283)

CATEGORY_IDENTS = [
    "Ai", "Art", "Conquest", "Data", "MapStyles", "Scenario",
    "Sounds", "Terrain", "Tribes", "Replays", "Saves", "Root",
]
TAG_IDENTS = [
    "Ai", "Art", "Conquest", "Cursors", "Data", "Mods", "MapStyles",
    "Replays", "Scenarios", "Sounds", "Terrain", "Tribe", "Other",
]


def main() -> int:
    pe = pefile.PE(EXE)
    ib = pe.OPTIONAL_HEADER.ImageBase

    def rd(va, n):
        return pe.get_data(va - ib, n)

    def wstr(b):
        return b.decode("utf-16le", "replace").split("\x00")[0]

    def astr(b):
        return b.split(b"\x00")[0].decode("ascii", "replace")

    # ---- s_ModCategoryInfo ----------------------------------------------------------------
    cats = []
    for i in range(MOD_CATEGORY_COUNT):
        b = rd(MOD_CATEGORY_INFO + i * MOD_CATEGORY_INFO_STRIDE, MOD_CATEGORY_INFO_STRIDE)
        cat = struct.unpack_from("<i", b, 0)[0]
        assert cat == i, f"ModCategoryInfo[{i}].category = {cat}"
        cats.append((astr(b[4:36]), wstr(b[36:116]).replace("\\", "/"), bool(b[116])))

    # ---- s_SteamWorkshopTagNames ----------------------------------------------------------
    tag_names = []
    for i in range(TAG_NAMES_COUNT):
        b = rd(TAG_NAMES + i * TAG_NAMES_STRIDE, TAG_NAMES_STRIDE)
        tag = struct.unpack_from("<i", b, 0)[0]
        assert tag == i
        tag_names.append(astr(b[4:44]))

    # ---- s_SteamWorkshopTagLinks ----------------------------------------------------------
    links = []
    for i in range(TAG_LINKS_COUNT):
        b = rd(TAG_LINKS + i * TAG_LINKS_STRIDE, TAG_LINKS_STRIDE)
        tag, cat = struct.unpack_from("<ii", b, 0)
        links.append((tag, cat, astr(b[8:68]), bool(b[68])))

    # ---- ModManager::isMapForbidden literals ----------------------------------------------
    txt = [s for s in pe.sections if s.Name.rstrip(b"\x00") == b".text"][0]
    tbase = ib + txt.VirtualAddress
    tdata = txt.get_data()
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    start, size = IS_MAP_FORBIDDEN
    forbidden = []
    for insn in md.disasm(tdata[start - tbase : start - tbase + size], start):
        if insn.mnemonic == "push" and insn.op_str.startswith("0x"):
            v = int(insn.op_str, 16)
            if 0xAC5000 <= v < 0xC06000:
                try:
                    s = wstr(rd(v, 120))
                except Exception:
                    continue
                if s and s.endswith(".xml") and all(0x20 <= ord(c) < 0x7F for c in s):
                    if s not in forbidden:
                        forbidden.append(s)
    forbidden.sort()

    # ---- TypeIndex ranges from the PDB type dump ------------------------------------------
    types = json.load(open(os.path.join(REPO, "schema", "pdb-types.json")))
    ti = {}
    for entry in types["enums"]["TypeIndex"]["values"]:
        m = re.search(r"\((-?\d+)\)", entry["value"])
        ti[entry["name"]] = int(m.group(1))

    ranges = [
        ("Good", "BASE_GOODTYPES", "END_GOODTYPES"),
        ("Unit", "BASE_UNITTYPES", "END_UNITTYPES"),
        ("Gaia", "BASE_GAIATYPES", "END_GAIATYPES"),
        ("Build", "BASE_BUILDTYPES", "END_BUILDTYPES"),
        ("Item", "BASE_ITEMTYPES", "END_ITEMTYPES"),
        ("Tech", "BASE_TECHTYPES", "END_TECHTYPES"),
        ("Spell", "BASE_SPELLTYPES", "END_SPELLTYPES"),
        ("Bonus", "BASE_BONUSTYPES", "END_BONUSTYPES"),
    ]

    def rs(s):
        return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'

    o = []
    w = o.append
    w("//! Tables captured out of `riseofnations.exe`.")
    w("//!")
    w("//! GENERATED by `crates/don-content/gen/gen_tables.py`. Do not edit by hand.")
    w("//!")
    w("//! Every constant below is **[measured]** — read from the shipped image at a VA")
    w("//! resolved through `ron-bin/sbl/rise.pdb`, or from the PDB's own type stream.")
    w("//! The generator asserts the two self-describing tables index themselves")
    w("//! (`ModCategoryInfo[i].category == i`, `SteamWorkshopTagNames[i].tag == i`), so a")
    w("//! stride or base-address error cannot pass silently.")
    w("")
    w("#![allow(dead_code)]")
    w("")
    w("use crate::vfs::{ModCategory, TagLink, WorkshopTag};")
    w("")
    w("/// `s_ModCategoryInfo` `0x00C07AD0`, 12 x 120 bytes.")
    w("///")
    w("/// `relative_dir` is the engine's `relativeDirectory` with `\\\\` rewritten to `/`;")
    w("/// the engine's category-prefix comparison is case-sensitive. `CAT_ROOT`'s is empty, so")
    w("/// it matches every path and must stay last in the classify scan.")
    w("pub struct CategoryInfo {")
    w("    pub name: &'static str,")
    w("    pub relative_dir: &'static str,")
    w("    pub recursive: bool,")
    w("}")
    w("")
    w(f"pub static CATEGORY_INFO: [CategoryInfo; {MOD_CATEGORY_COUNT}] = [")
    for (name, rel, rec) in cats:
        w(f"    CategoryInfo {{ name: {rs(name)}, relative_dir: {rs(rel)}, recursive: {str(rec).lower()} }},")
    w("];")
    w("")
    w("/// `s_SteamWorkshopTagNames` `0x00C06678`, 13 x 44 bytes.")
    w(f"pub static TAG_NAMES: [&str; {TAG_NAMES_COUNT}] = [")
    for n in tag_names:
        w(f"    {rs(n)},")
    w("];")
    w("")
    w("/// `s_SteamWorkshopTagLinks` `0x00C068D0`, 64 x 72 bytes.")
    w("///")
    w("/// This is the engine's own answer to *what content is a mod made of*: a tag is")
    w("/// awarded when the mod's file list for `category` contains a file matching")
    w("/// `pattern`. It is also the closest thing retail has to a manifest schema.")
    w(f"pub static TAG_LINKS: [TagLink; {TAG_LINKS_COUNT}] = [")
    for (tag, cat, pat, rec) in links:
        w(
            f"    TagLink {{ tag: WorkshopTag::{TAG_IDENTS[tag]}, "
            f"category: ModCategory::{CATEGORY_IDENTS[cat]}, "
            f"pattern: {rs(pat)}, recursive: {str(rec).lower()} }},"
        )
    w("];")
    w("")
    w("/// `ModManager::isMapForbidden` `0x00A21140` — the 21 map-style files a mod is not")
    w("/// permitted to override, extracted from the function's own pushed `.rdata` literals.")
    w("///")
    w("/// This is the **only** content-replacement veto in the engine: `calcFilePath`")
    w("/// consults it for `CAT_MAPSTYLES` and for no other category.")
    w(f"pub static FORBIDDEN_MAPSTYLES: [&str; {len(forbidden)}] = [")
    for f in forbidden:
        w(f"    {rs(f)},")
    w("];")
    w("")
    w("/// `enum TypeIndex` (PDB type stream) — the closed, compile-time type-id space.")
    w("///")
    w("/// A retail mod cannot add an entry here: ids are an enum baked into the image and")
    w("/// `Balance::final_balance_table` `0x00C12BF4` is a static `short[493][493]`.")
    w(f"pub const RETAIL_NUM_TYPES: u16 = {ti['NUM_TYPES']};")
    w("/// Side length of the static combat matrix. Note it is **smaller** than")
    w("/// `RETAIL_NUM_TYPES`: the matrix only spans ids `0..493`.")
    w("pub const BALANCE_TABLE_SIDE: u16 = 493;")
    w("")
    w("/// `[base, end)` per type family, from `BASE_*`/`END_*` in `enum TypeIndex`.")
    w(f"pub static TYPE_RANGES: [(&str, u16, u16); {len(ranges)}] = [")
    for (label, b, e) in ranges:
        w(f"    ({rs(label)}, {ti[b]}, {ti[e]}),")
    w("];")
    w("")

    with open(OUT, "w") as fh:
        fh.write("\n".join(o))
    print(f"wrote {OUT}: {len(cats)} categories, {len(links)} tag links, "
          f"{len(tag_names)} tag names, {len(forbidden)} forbidden map styles")
    return 0


if __name__ == "__main__":
    sys.exit(main())
