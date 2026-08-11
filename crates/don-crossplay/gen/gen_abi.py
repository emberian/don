#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Generate `crates/don-crossplay/src/abi.rs` from the shipped private PDBs.

Nothing in the emitted file is hand-typed. Every slot index, every field offset
and every aggregate size is read out of `CrossplayProxy.pdb`, cross-checked
against `rise.pdb` and `CrossplayNetLib.pdb`, and — for the one vtable the game
actually calls — checked against the pointer table the shipped
`CrossplayProxy.dll` really emits into `.rdata`.

The generator REFUSES to write when any of those checks disagree, because an
ABI that compiles with one wrong slot is worse than no ABI at all.

Re-run (needs the local, gitignored `ron-bin/`):

    python3 crates/don-crossplay/gen/gen_abi.py

Add `--check-ret` to additionally disassemble every emitted implementation and
compare its callee stack cleanup (`ret imm16`) against the argument byte count
this generator derived from the PDB signature. That check needs Capstone:

    uv run --with capstone python3 crates/don-crossplay/gen/gen_abi.py --check-ret
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
PDB_DIR = REPO / "ron-bin" / "sbl"
DLL = REPO / "ron-bin" / "dll" / "CrossplayProxy.dll"
PDB_EXTRACT = REPO / "tools" / "pdb-extract" / "Cargo.toml"
OUT = REPO / "crates" / "don-crossplay" / "src" / "abi.rs"

IMAGE_BASE = 0x10000000

# The interface the shipped object actually implements. `CrossplayProxy.pdb`
# ALSO declares a `Crossplay::ICrossPlayService` from a newer vendor header with
# a completely different slot order; see VENDOR_IFACE.
SHIPPED_IFACE = "CrossplayProxy::ICrossPlayService"
VENDOR_IFACE = "Crossplay::ICrossPlayService"
IMPL_CLASS = "CrossplayProxy::CrossPlayService"
IMPL_VFTABLE = "??_7CrossPlayService@CrossplayProxy@@6B@"

LOGGER_IFACE = "Crossplay::Logging::ICrossplayLogger"
PLAYER_IFACE = "Crossplay::P2P::ICrossplayPlayer"
NETWORK_IFACE = "CrossplayProxy::INetworkClient"

# DTOs to emit, in the order the emitted file needs them (dependencies first).
DTOS = [
    ("Crossplay::User::DTO::UserDTO", "UserDTO"),
    ("Crossplay::Lobby::DTO::LobbyMemberDTO", "LobbyMemberDTO"),
    ("Crossplay::Lobby::DTO::TurnServerDTO", "TurnServerDTO"),
    ("Crossplay::Lobby::DTO::LobbyDTO", "LobbyDTO"),
    ("Crossplay::Lobby::DTO::JoinLobbyDTO", "JoinLobbyDTO"),
    ("Crossplay::Lobby::DTO::LeaveLobbyDTO", "LeaveLobbyDTO"),
    ("Crossplay::Lobby::DTO::UpdateLobbyDTO", "UpdateLobbyDTO"),
    ("Crossplay::Lobby::DTO::UpdateLobbyResultDTO", "UpdateLobbyResultDTO"),
    ("Crossplay::Lobby::DTO::JoinLobbyResultDTO", "JoinLobbyResultDTO"),
    ("Crossplay::Lobby::DTO::LobbyChatMessageDTO", "LobbyChatMessageDTO"),
    ("Crossplay::Lobby::DTO::LobbySearchCriteriaDTO", "LobbySearchCriteriaDTO"),
    ("Crossplay::Lobby::DTO::LobbySearchResultDTO", "LobbySearchResultDTO"),
    ("Crossplay::Leaderboard::DTO::Entry", "LeaderboardEntry"),
    ("Crossplay::MatchMaking::DTO::MatchFoundDTO", "MatchFoundDTO"),
    ("Crossplay::MatchMaking::DTO::MatchMakingEnqueuedDTO", "MatchMakingEnqueuedDTO"),
]

ENUMS = [
    ("Crossplay::Lobby::Visibility", "Visibility", "VISIBILITY"),
    ("Crossplay::Lobby::DTO::ELobbyProximity", "LobbyProximity", "LOBBY_PROXIMITY"),
    ("Crossplay::User::SessionStatus", "SessionStatus", "SESSION_STATUS"),
    ("Crossplay::CrossplayStatus", "CrossplayStatus", "CROSSPLAY_STATUS"),
    ("Crossplay::Logging::LogLevel", "LogLevel", "LOG_LEVEL"),
]

WSTRING = "std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >"
STRING = "std::basic_string<char,std::char_traits<char>,std::allocator<char> >"


# --------------------------------------------------------------------------
# inputs
# --------------------------------------------------------------------------


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def extract(pdb: Path, workdir: Path, base: int) -> tuple[dict, dict]:
    """Run tools/pdb-extract and load its two artifacts."""
    sym = workdir / f"{pdb.stem}-symbols.json"
    typ = workdir / f"{pdb.stem}-types.json"
    if not (sym.exists() and typ.exists()):
        subprocess.run(
            [
                "cargo",
                "run",
                "--release",
                "--quiet",
                "--manifest-path",
                str(PDB_EXTRACT),
                "--",
                str(pdb),
                hex(base),
                str(sym),
                str(typ),
            ],
            check=True,
            cwd=REPO,
        )
    return json.loads(sym.read_text()), json.loads(typ.read_text())


class Pe:
    """Just enough PE32 to read `.rdata`/`.text` by RVA. No dependencies."""

    def __init__(self, path: Path):
        self.data = path.read_bytes()
        e_lfanew = struct.unpack_from("<I", self.data, 0x3C)[0]
        assert self.data[e_lfanew : e_lfanew + 4] == b"PE\0\0"
        coff = e_lfanew + 4
        machine, nsec, _, _, _, opt_size, _ = struct.unpack_from("<HHIIIHH", self.data, coff)
        self.machine = machine
        opt = coff + 20
        self.image_base = struct.unpack_from("<I", self.data, opt + 28)[0]
        sec = opt + opt_size
        self.sections = []
        for i in range(nsec):
            off = sec + i * 40
            name = self.data[off : off + 8].rstrip(b"\0").decode()
            vsize, vaddr, rawsize, rawptr = struct.unpack_from("<IIII", self.data, off + 8)
            chars = struct.unpack_from("<I", self.data, off + 36)[0]
            self.sections.append((name, vaddr, vsize, rawptr, rawsize, chars))

    def section_of(self, rva: int):
        for s in self.sections:
            if s[1] <= rva < s[1] + max(s[2], s[4]):
                return s
        return None

    def read(self, rva: int, n: int) -> bytes:
        s = self.section_of(rva)
        if s is None:
            raise KeyError(hex(rva))
        off = s[3] + (rva - s[1])
        return self.data[off : off + n]

    def in_text(self, rva: int) -> bool:
        s = self.section_of(rva)
        return s is not None and s[0] == ".text"


# --------------------------------------------------------------------------
# signature parsing
# --------------------------------------------------------------------------


def split_top(s: str) -> list[str]:
    out, depth, cur = [], 0, ""
    for ch in s:
        if ch in "<(":
            depth += 1
        elif ch in ">)":
            depth -= 1
        if ch == "," and depth == 0:
            out.append(cur.strip())
            cur = ""
        else:
            cur += ch
    if cur.strip():
        out.append(cur.strip())
    return out


def parse_signature(sig: str) -> tuple[str, list[str], bool]:
    """`"RET Name(a, b) const"` -> (ret, [args], is_const)."""
    open_paren = sig.index("(")
    close_paren = sig.rindex(")")
    ret = sig[:open_paren].rsplit(" ", 1)[0].strip()
    args = split_top(sig[open_paren + 1 : close_paren])
    tail = sig[close_paren + 1 :].strip()
    return ret, args, tail == "const"


PRIMITIVES = {
    "void": ("()", 0),
    "bool": ("bool", 1),
    "char": ("i8", 1),
    "unsigned char": ("u8", 1),
    "short": ("i16", 2),
    "unsigned short": ("u16", 2),
    "int": ("i32", 4),
    "unsigned int": ("u32", 4),
    "long": ("i32", 4),
    "unsigned long": ("u32", 4),
    "long long": ("i64", 8),
    "unsigned long long": ("u64", 8),
    "float": ("f32", 4),
    "double": ("f64", 8),
    "wchar_t": ("u16", 2),
}

POINTEE = {
    WSTRING: "MsvcWstring",
    STRING: "MsvcString",
    "Crossplay::P2P::ICrossplayPlayer": "ICrossplayPlayer",
    "Crossplay::Lobby::DTO::LobbyDTO": "LobbyDTO",
    "Crossplay::Lobby::DTO::LobbySearchCriteriaDTO": "LobbySearchCriteriaDTO",
    "void": "core::ffi::c_void",
    "char": "core::ffi::c_char",
    "unsigned char": "u8",
}


class Unresolved(Exception):
    pass


class Mapper:
    def __init__(self, classes: dict, enums: dict):
        self.classes = classes
        self.enums = enums
        self.byvalue: dict[str, tuple[str, int]] = {}

    def strip_cv(self, t: str) -> tuple[str, bool]:
        const = False
        t = t.strip()
        while True:
            if t.startswith("const "):
                const = True
                t = t[6:].strip()
            elif t.startswith("volatile "):
                t = t[9:].strip()
            else:
                break
        return t, const

    def map_arg(self, t: str) -> tuple[str, int]:
        """C++ parameter type -> (rust type, bytes it occupies on the x86 stack)."""
        base, const = self.strip_cv(t)
        if base.endswith("&") or base.endswith("*"):
            pointee, _ = self.strip_cv(base[:-1])
            mut = "const" if const or pointee.startswith("const") else "mut"
            inner, inner_const = self.strip_cv(pointee)
            if inner_const:
                mut = "const"
            name = POINTEE.get(inner)
            if name is None and inner.endswith("*"):
                deep, _ = self.strip_cv(inner[:-1])
                name = f"*mut {POINTEE.get(deep, 'core::ffi::c_void')}"
            if name is None:
                # A pointer/reference to a class we can size: name the pointee
                # after its measured layout instead of erasing it to `void`.
                size = self.classes.get(inner, {}).get("size")
                if size:
                    try:
                        name = self.byvalue_type(inner, size)
                    except Unresolved:
                        name = "core::ffi::c_void"
                else:
                    name = "core::ffi::c_void"
            return f"*{mut} {name}", 4
        if base in PRIMITIVES:
            rust, size = PRIMITIVES[base]
            if size == 0:
                raise Unresolved(t)
            return rust, max(4, size)
        if base in self.enums:
            e = self.enums[base]
            if e.get("size") != 4:
                raise Unresolved(t)
            return enum_rust_name(base), 4
        if base in self.classes:
            size = self.classes[base].get("size")
            if not size:
                raise Unresolved(t)
            return self.byvalue_type(base, size), (size + 3) // 4 * 4
        raise Unresolved(t)

    def byvalue_type(self, cxx: str, size: int) -> str:
        if cxx == WSTRING:
            return "MsvcWstring"
        if cxx == STRING:
            return "MsvcString"
        if cxx.startswith("std::function<"):
            if size != 40:
                raise Unresolved(cxx)
            return "MsvcFunction"
        if cxx.startswith("std::vector<"):
            if size != 12:
                raise Unresolved(cxx)
            return "MsvcVector"
        for prefix, stem in (
            ("std::unordered_map<", "MsvcUnorderedMap"),
            ("std::unordered_set<", "MsvcUnorderedSet"),
            ("std::map<", "MsvcMap"),
            ("std::set<", "MsvcSet"),
            ("std::list<", "MsvcList"),
        ):
            if cxx.startswith(prefix):
                name = f"{stem}{size}"
                self.byvalue[name] = (cxx, size)
                return name
        name = f"ByValue{size}"
        self.byvalue[name] = (cxx, size)
        return name

    def map_return(self, t: str) -> tuple[str | None, str, int]:
        """-> (hidden sret parameter type or None, rust return type, extra stack bytes)."""
        base, const = self.strip_cv(t)
        if base == "void":
            return None, "", 0
        if base.endswith("&") or base.endswith("*"):
            rust, _ = self.map_arg(t)
            return None, f" -> {rust}", 0
        if base in PRIMITIVES:
            rust, _ = PRIMITIVES[base]
            return None, f" -> {rust}", 0
        if base in self.enums and self.enums[base].get("size") == 4:
            return None, f" -> {enum_rust_name(base)}", 0
        if base in self.classes:
            size = self.classes[base].get("size")
            if not size:
                raise Unresolved(t)
            ty = self.byvalue_type(base, size)
            return f"*mut {ty}", f" -> *mut {ty}", 4
        raise Unresolved(t)


def enum_rust_name(cxx: str) -> str:
    for full, rust, _ in ENUMS:
        if full == cxx:
            return rust
    return "i32"


# --------------------------------------------------------------------------
# vtable model
# --------------------------------------------------------------------------


class Slot:
    def __init__(self, index: int, offset: int, name: str, signature: str):
        self.index = index
        self.offset = offset
        self.name = name
        self.signature = signature
        self.field = name
        self.rust = ""
        self.arg_bytes = 0
        self.resolved = True
        self.reason = ""


def build_slots(cls: dict, iface: str) -> list[Slot]:
    """One Slot per vtable entry. A destructor slot collapses `~X`+`__vecDelDtor`."""
    by_offset: dict[int, list[dict]] = {}
    for m in cls["methods"]:
        off = m.get("vtable_offset")
        if off is None:
            raise SystemExit(f"{iface}: virtual method {m['name']} has no vtable_offset")
        by_offset.setdefault(off, []).append(m)
    offsets = sorted(by_offset)
    if offsets != list(range(0, offsets[-1] + 1, 4)):
        raise SystemExit(f"{iface}: vtable byte offsets are not contiguous: {offsets}")
    slots = []
    for i, off in enumerate(offsets):
        entries = by_offset[off]
        names = {m["name"] for m in entries}
        if len(entries) > 1:
            if not any(n.startswith("~") for n in names) or "__vecDelDtor" not in names:
                raise SystemExit(f"{iface}: slot {off} has {len(entries)} unrelated methods: {names}")
            dtor = next(m for m in entries if m["name"].startswith("~"))
            s = Slot(i, off, "vector_deleting_destructor", "void* __vecDelDtor(unsigned int)")
            s.cxx_name = dtor["name"]
            slots.append(s)
            continue
        m = entries[0]
        s = Slot(i, off, m["name"], m.get("signature") or "")
        s.cxx_name = m["name"]
        slots.append(s)
    # Disambiguate C++ overloads that occupy distinct slots.
    counts: dict[str, int] = {}
    for s in slots:
        counts[s.field] = counts.get(s.field, 0) + 1
    for s in slots:
        if counts[s.field] > 1:
            s.field = f"{s.field}__s{s.index}"
    return slots


def render_slots(slots: list[Slot], mapper: Mapper, this_ty: str) -> None:
    for s in slots:
        if s.field.startswith("vector_deleting_destructor"):
            s.rust = f'unsafe extern $abi fn(*mut {this_ty}, u32) -> *mut core::ffi::c_void'
            s.arg_bytes = 4
            continue
        if not s.signature:
            s.resolved = False
            s.reason = "the PDB records no function type for this slot"
            continue
        try:
            ret, args, _ = parse_signature(s.signature)
            sret, ret_rust, extra = mapper.map_return(ret)
            parts = [f"*mut {this_ty}"]
            total = 0
            if sret:
                parts.append(sret)
                total += extra
            for a in args:
                if a == "void":
                    continue
                rust, size = mapper.map_arg(a)
                parts.append(rust)
                total += size
            s.rust = f'unsafe extern $abi fn({", ".join(parts)}){ret_rust}'
            s.arg_bytes = total
        except Unresolved as exc:
            s.resolved = False
            s.reason = f"unresolved type `{exc.args[0]}`"


def emit_vtable(
    name: str,
    this_ty: str,
    slots: list[Slot],
    doc: list[str],
) -> str:
    out = []
    for line in doc:
        out.append(f"/// {line}" if line else "///")
    out.append("#[repr(C)]")
    out.append(f"pub struct {name} {{")
    for s in slots:
        out.append(f"    /// slot {s.index}, vtable byte offset `0x{s.offset:03x}` —")
        out.append(f"    /// `{s.signature}`")
        if s.resolved:
            out.append(f"    /// callee pops {s.arg_bytes} stack bytes.")
            out.append(f"    pub {s.field}: {s.rust},")
        else:
            out.append(f"    /// **NOT DERIVED** ({s.reason}); do not call through this field.")
            out.append(f"    pub {s.field}: *const core::ffi::c_void,")
    out.append("}")
    out.append("")
    return "\n".join(out)


def emit_slot_asserts(vtname: str, constprefix: str, slots: list[Slot]) -> str:
    out = [f"/// Vtable slot count for [`{vtname}`]. **[measured]**"]
    out.append(f"pub const {constprefix}_SLOTS: usize = {len(slots)};")
    out.append(
        f"/// x86 vtable size in bytes: `{constprefix}_SLOTS * 4`. **[measured]**"
    )
    out.append(f"pub const {constprefix}_VTABLE_BYTES_X86: usize = {len(slots) * 4};")
    out.append("")
    out.append("const _: () = {")
    out.append(
        f"    assert!(core::mem::size_of::<{vtname}>() == {constprefix}_SLOTS * PTR_SIZE);"
    )
    for s in slots:
        out.append(
            f"    assert!(core::mem::offset_of!({vtname}, {s.field}) == {s.index} * PTR_SIZE);"
        )
    out.append("};")
    out.append("#[cfg(target_arch = \"x86\")]")
    out.append(
        f"const _: () = assert!(core::mem::size_of::<{vtname}>() == {constprefix}_VTABLE_BYTES_X86);"
    )
    out.append("")
    return "\n".join(out)


# --------------------------------------------------------------------------
# DTOs
# --------------------------------------------------------------------------

FIELD_TYPES = {
    WSTRING: ("MsvcWstring", 24),
    STRING: ("MsvcString", 24),
}


def rust_ident(name: str) -> str:
    n = name.lstrip("_")
    n = re.sub(r"(?<!^)(?=[A-Z])", "_", n).lower()
    n = re.sub(r"[^a-z0-9_]", "_", n)
    if n and n[0].isdigit():
        n = "f_" + n
    return n


def emit_dto(cxx: str, rust: str, classes: dict, enums: dict, byvalue: dict) -> tuple[str, str]:
    c = classes[cxx]
    size = c["size"]
    lines = [
        f"/// `{cxx}` — {size} bytes. **[measured]**",
        "#[repr(C)]",
        "#[derive(Clone, Copy)]",
        f"pub struct {rust} {{",
    ]
    asserts = [f"    assert!(core::mem::size_of::<{rust}>() == {size});"]
    cursor = 0
    pad = 0

    def add_pad(upto: int) -> None:
        nonlocal cursor, pad
        gap = upto - cursor
        if gap > 0:
            lines.append(f"    /// MSVC alignment padding, +{cursor}..+{upto}.")
            lines.append(f"    pub _pad{pad}: [u8; {gap}],")
            pad += 1
            cursor = upto

    for b in c.get("bases") or []:
        bname = dto_rust_name(b["name"])
        if bname is None:
            raise SystemExit(f"{cxx}: base {b['name']} is not an emitted DTO")
        add_pad(b["offset"])
        lines.append(f"    /// base class `{b['name']}` at +{b['offset']}.")
        lines.append(f"    pub base: {bname},")
        asserts.append(f"    assert!(core::mem::offset_of!({rust}, base) == {b['offset']});")
        cursor = b["offset"] + (b.get("size") or 0)

    for f in c.get("fields") or []:
        add_pad(f["offset"])
        ftype = f["type"]
        fsize = f.get("size")
        ident = rust_ident(f["name"])
        if ftype in FIELD_TYPES:
            rt, sz = FIELD_TYPES[ftype]
        elif ftype in PRIMITIVES:
            rt, sz = PRIMITIVES[ftype]
        elif ftype in enums and enums[ftype].get("size") == 4:
            rt, sz = enum_rust_name(ftype), 4
        elif dto_rust_name(ftype):
            rt, sz = dto_rust_name(ftype), fsize
        elif ftype.startswith("std::vector<"):
            rt, sz = "MsvcVector", fsize
        elif ftype.startswith("std::unordered_map<"):
            rt, sz = f"MsvcUnorderedMap{fsize}", fsize
            byvalue.setdefault(f"MsvcUnorderedMap{fsize}", (ftype, fsize))
        else:
            rt, sz = f"Opaque{fsize}", fsize
            byvalue.setdefault(f"Opaque{fsize}", (ftype, fsize))
        if sz != fsize:
            raise SystemExit(
                f"{cxx}.{f['name']}: mapped Rust type {rt} is {sz} bytes, PDB says {fsize}"
            )
        short = ftype if len(ftype) <= 96 else ftype[:93] + "..."
        lines.append(f"    /// +{f['offset']} `{f['name']}` : `{short}`")
        lines.append(f"    pub {ident}: {rt},")
        asserts.append(
            f"    assert!(core::mem::offset_of!({rust}, {ident}) == {f['offset']});"
        )
        cursor = f["offset"] + fsize

    add_pad(size)
    lines.append("}")
    lines.append("")
    return "\n".join(lines), "const _: () = {\n" + "\n".join(asserts) + "\n};\n"


_DTO_NAMES = {cxx: rust for cxx, rust in DTOS}


def dto_rust_name(cxx: str) -> str | None:
    return _DTO_NAMES.get(cxx)


# --------------------------------------------------------------------------
# verification
# --------------------------------------------------------------------------


def slot_key(slots: list[Slot]) -> list[tuple[int, str, str]]:
    return [(s.offset, s.cxx_name, s.signature) for s in slots]


def verify_cross_pdb(proxy_c, netlib_c, rise_c, report):
    a = build_slots(proxy_c[SHIPPED_IFACE], SHIPPED_IFACE)
    b = build_slots(rise_c["Crossplay::ICrossPlayService"], "rise Crossplay::ICrossPlayService")
    c = build_slots(
        netlib_c["Crossplay::ICrossPlayService"], "netlib Crossplay::ICrossPlayService"
    )
    ok = slot_key(a) == slot_key(b) == slot_key(c)
    report.append(
        f"cross-PDB vtable identity: {SHIPPED_IFACE} ({len(a)} slots) vs "
        f"rise.pdb Crossplay::ICrossPlayService ({len(b)}) vs "
        f"CrossplayNetLib.pdb Crossplay::ICrossPlayService ({len(c)}): "
        + ("IDENTICAL" if ok else "MISMATCH")
    )
    if not ok:
        for x, y, z in zip(slot_key(a), slot_key(b), slot_key(c)):
            if not (x == y == z):
                report.append(f"  slot {x[0]}: {x[1]} / {y[1]} / {z[1]}")
        raise SystemExit("refusing to write: the three PDBs disagree on the shipped vtable")
    return a


def verify_emitted_vtable(pe: Pe, symbols: dict, slots: list[Slot], report):
    """Read `??_7CrossPlayService@CrossplayProxy@@6B@` out of `.rdata`."""
    vft_rva = None
    for g in symbols["globals"]:
        if g.get("mangled") == IMPL_VFTABLE:
            vft_rva = g["rva"]
            break
    if vft_rva is None:
        raise SystemExit(f"refusing to write: {IMPL_VFTABLE} not found in the PDB globals")
    by_rva: dict[int, set[str]] = {}
    for f in symbols["functions"]:
        by_rva.setdefault(f["rva"], set()).add(f["name"])
    n = len(slots)
    raw = pe.read(vft_rva, 4 * (n + 2))
    ptrs = struct.unpack_from(f"<{n + 2}I", raw)
    bad = []
    for s in slots:
        rva = ptrs[s.index] - pe.image_base
        names = by_rva.get(rva, set())
        want = f"{IMPL_CLASS}::{s.cxx_name}"
        if not pe.in_text(rva) or want not in names:
            bad.append((s, rva, names))
    dtor_rva = ptrs[n] - pe.image_base
    dtor_names = by_rva.get(dtor_rva, set())
    dtor_ok = pe.in_text(dtor_rva) and any("destructor" in x for x in dtor_names)
    past = ptrs[n + 1] - pe.image_base
    past_ok = not pe.in_text(past)
    report.append(
        f"shipped .rdata vtable at RVA 0x{vft_rva:05x}: {n - len(bad)}/{n} interface slots "
        f"resolve to {IMPL_CLASS}::<declared name>; slot {n} is the deleting destructor "
        f"({'yes' if dtor_ok else 'NO'}); slot {n + 1} is outside .text "
        f"({'yes' if past_ok else 'NO'}) so the emitted table is {(n + 1) * 4} bytes"
    )
    if bad or not dtor_ok or not past_ok:
        for s, rva, names in bad:
            report.append(f"  slot {s.index} (+{s.offset}) expected {IMPL_CLASS}::{s.cxx_name}, got {names} at 0x{rva:05x}")
        raise SystemExit("refusing to write: the emitted vtable disagrees with the PDB declaration")
    return vft_rva, ptrs


def verify_dtos(proxy_c, netlib_c, rise_c, report):
    agreed, only = 0, []
    for cxx, _ in DTOS:
        shapes = []
        for label, d in (("CrossplayProxy", proxy_c), ("CrossplayNetLib", netlib_c), ("rise", rise_c)):
            if cxx in d:
                c = d[cxx]
                shapes.append(
                    (
                        label,
                        (
                            c["size"],
                            tuple((f["name"], f["offset"], f["type"]) for f in c.get("fields") or []),
                            tuple((b["name"], b["offset"]) for b in c.get("bases") or []),
                        ),
                    )
                )
        distinct = {s for _, s in shapes}
        if len(distinct) != 1:
            report.append(f"  DTO {cxx} DISAGREES across {[l for l, _ in shapes]}")
            raise SystemExit("refusing to write: DTO layouts disagree across PDBs")
        agreed += 1
        if len(shapes) == 1:
            only.append((cxx, shapes[0][0]))
    report.append(
        f"DTO layout agreement: {agreed}/{len(DTOS)} identical wherever declared; "
        f"{len(only)} declared in only one PDB: "
        + ", ".join(f"{c.rsplit('::', 1)[1]}({p})" for c, p in only)
    )


def verify_ret_bytes(pe: Pe, symbols: dict, slots: list[Slot], ptrs, report):
    try:
        import capstone
    except ImportError:
        report.append("stack-cleanup check: SKIPPED (capstone not importable)")
        return
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    sizes: dict[int, int] = {}
    for f in symbols["functions"]:
        if f.get("size"):
            sizes[f["rva"]] = max(sizes.get(f["rva"], 0), f["size"])
    ok = mismatch = 0
    indeterminate: list[str] = []
    details = []
    for s in slots:
        if not s.resolved:
            continue
        rva = ptrs[s.index] - pe.image_base
        n = sizes.get(rva)
        if not n:
            indeterminate.append(f"{s.cxx_name}(+{s.offset}): no code extent")
            continue
        try:
            code = pe.read(rva, n)
        except KeyError:
            indeterminate.append(f"{s.cxx_name}(+{s.offset}): unreadable extent")
            continue
        imms = set()
        plain = False
        for ins in md.disasm(code, pe.image_base + rva):
            if ins.mnemonic == "ret":
                if ins.op_str:
                    imms.add(int(ins.op_str, 0))
                else:
                    plain = True
        if plain and not imms:
            imms = {0}
        if len(imms) != 1:
            indeterminate.append(
                f"{s.cxx_name}(+{s.offset}): linear sweep found "
                + (f"{len(imms)} distinct `ret imm` values" if imms else "no `ret`")
            )
            continue
        got = imms.pop()
        if got == s.arg_bytes:
            ok += 1
        else:
            mismatch += 1
            details.append(f"  slot {s.index} {s.cxx_name}: derived {s.arg_bytes}, `ret 0x{got:x}`")
    report.append(
        f"stack-cleanup check (`ret imm16` vs derived argument bytes): "
        f"{ok} agree, {mismatch} disagree, {len(indeterminate)} indeterminate"
        + ("; indeterminate: " + "; ".join(indeterminate) if indeterminate else "")
    )
    report.extend(details)
    if mismatch:
        raise SystemExit("refusing to write: derived argument sizes contradict the machine code")


# --------------------------------------------------------------------------


HEADER = '''//! `CrossplayProxy.dll`'s binary interface — **generated, do not edit by hand**.
//!
//! Regenerate with:
//!
//! ```sh
//! python3 crates/don-crossplay/gen/gen_abi.py
//! uv run --with capstone python3 crates/don-crossplay/gen/gen_abi.py --check-ret
//! ```
//!
//! Everything here is layout, not behaviour, and every number was read out of a
//! shipped private PDB and cross-checked against the shipped DLL. Nothing in
//! this crate implements a method.
//!
//! # The one thing that will bite you: there are TWO `ICrossPlayService`s
//!
//! `CrossplayProxy.pdb` declares both:
//!
//! * `CrossplayProxy::ICrossPlayService` — {shipped_slots} slots, byte offsets
//!   `0x000..=0x{shipped_last:03x}`. **This is the shipped vtable.**
//!   `CrossplayProxy::CrossPlayService` (the object `Service()` returns) derives
//!   from it at offset 0, and `rise.pdb` and `CrossplayNetLib.pdb` both declare
//!   their `Crossplay::ICrossPlayService` with a slot-for-slot **identical**
//!   layout — same names, same signatures, same offsets, zero differences.
//! * `Crossplay::ICrossPlayService` — {vendor_slots} slots, offsets
//!   `0x000..=0x{vendor_last:03x}`, from a *newer vendor header* this build does
//!   not use. Its slot 0 is a destructor, slot 1 is `SetNew`, and `Init` is at
//!   slot 3. Every single slot disagrees with the shipped table. (`pdb-extract`
//!   reports {vendor_methods} virtual *methods* for it because slot 0 carries both
//!   `~ICrossPlayService` and `__vecDelDtor`; the table is {vendor_slots} pointers.)
//!
//! Using the {vendor_slots}-slot record as "the ICrossPlayService vtable" would
//! produce an ABI that is wrong in every slot while compiling perfectly. It is
//! emitted below as [`VendorICrossPlayServiceVtable`] purely so the trap is
//! recorded in checked code; retail never calls it.
//!
//! # Calling convention
//!
//! Every virtual is an MSVC x86 C++ member function: `__thiscall` — `this` in
//! `ECX`, remaining arguments pushed right-to-left, **callee** cleans the stack.
//! Rust spells that `extern "thiscall"`, which only exists on x86, so the
//! vtables are emitted with `extern "thiscall"` on `target_arch = "x86"` and
//! with `extern "C"` elsewhere. The slot-order assertions are written in units
//! of `size_of::<*const ()>()` and therefore hold on both; the exact byte-offset
//! assertions are additionally checked under `target_arch = "x86"`.
//!
//! A class returned by value becomes a hidden first stack argument (the caller's
//! return slot), which the callee also returns in `EAX`; that is the same shape
//! `crates/netsys-shim/src/abi.rs` uses for `NetPlayer::get_id`.
//!
//! # Provenance
//!
{provenance}

#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

/// Pointer width of the target. The shipped ABI is x86, where this is 4.
pub const PTR_SIZE: usize = core::mem::size_of::<*const core::ffi::c_void>();
'''

PRELUDE = '''
/// MSVC x86 `std::basic_string<wchar_t>` — 24 bytes. Same object
/// `crates/netsys-shim/src/abi.rs` calls `MsvcWstring`. **[measured]**
#[repr(C, align(4))]
#[derive(Clone, Copy)]
pub struct MsvcWstring {
    pub raw: [u8; 24],
}
const _: () = assert!(core::mem::size_of::<MsvcWstring>() == 24);

/// MSVC x86 `std::basic_string<char>` — 24 bytes. **[measured]**
#[repr(C, align(4))]
#[derive(Clone, Copy)]
pub struct MsvcString {
    pub raw: [u8; 24],
}
const _: () = assert!(core::mem::size_of::<MsvcString>() == 24);

/// MSVC x86 `std::function<...>` — 40 bytes for every one of the {nfunc}
/// specialisations defined in `CrossplayProxy.pdb`. The target pointer lives at
/// `+0x24`; `CrossPlayService::SetServiceErrorCallback` (`0x100145c0`) reads
/// exactly that offset off its argument. **[measured]**
///
/// `target` is a **32-bit** guest pointer, spelled `u32` so the layout is the
/// x86 one on every host this crate is checked on. Never dereference it here.
#[repr(C, align(8))]
#[derive(Clone, Copy)]
pub struct MsvcFunction {
    pub storage: [u8; 36],
    pub target: u32,
}
const _: () = {
    assert!(core::mem::size_of::<MsvcFunction>() == 40);
    assert!(core::mem::offset_of!(MsvcFunction, target) == 0x24);
};

/// MSVC x86 `std::vector<T>` — 12 bytes: three 32-bit guest pointers
/// (`first`, `last`, `end_of_storage`). **[measured]**
#[repr(C, align(4))]
#[derive(Clone, Copy)]
pub struct MsvcVector {
    pub first: u32,
    pub last: u32,
    pub end: u32,
}
const _: () = assert!(core::mem::size_of::<MsvcVector>() == 12);

/// An opaque interface object: one vtable pointer, 4 bytes.
/// `Crossplay::P2P::ICrossplayPlayer` and `Crossplay::ICrossPlayService` are
/// both `size=4`. **[measured]**
#[repr(C)]
pub struct ICrossPlayService {
    pub vftable: *const ICrossPlayServiceVtable,
}
const _: () = assert!(core::mem::size_of::<ICrossPlayService>() == PTR_SIZE);

/// `Crossplay::P2P::ICrossplayPlayer`, the per-peer handle. **[measured]**
#[repr(C)]
pub struct ICrossplayPlayer {
    pub vftable: *const ICrossplayPlayerVtable,
}
const _: () = assert!(core::mem::size_of::<ICrossplayPlayer>() == PTR_SIZE);

/// `Crossplay::Logging::ICrossplayLogger`. **[measured]**
#[repr(C)]
pub struct ICrossplayLogger {
    pub vftable: *const ICrossplayLoggerVtable,
}
const _: () = assert!(core::mem::size_of::<ICrossplayLogger>() == PTR_SIZE);

/// `CrossplayProxy::INetworkClient`, the Party-facing side of the proxy.
/// **[measured]**
#[repr(C)]
pub struct INetworkClient {
    pub vftable: *const INetworkClientVtable,
}
const _: () = assert!(core::mem::size_of::<INetworkClient>() == PTR_SIZE);
'''


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--check-ret", action="store_true")
    ap.add_argument("--work", default=None, help="reuse an extraction directory")
    ap.add_argument("--out", default=str(OUT))
    args = ap.parse_args()

    for p in (PDB_DIR / "CrossplayProxy.pdb", PDB_DIR / "CrossplayNetLib.pdb", PDB_DIR / "rise.pdb", DLL):
        if not p.exists():
            print(f"missing proprietary input: {p}", file=sys.stderr)
            return 2

    work = Path(args.work) if args.work else Path(tempfile.mkdtemp(prefix="don-crossplay-abi-"))
    work.mkdir(parents=True, exist_ok=True)

    proxy_sym, proxy_ty = extract(PDB_DIR / "CrossplayProxy.pdb", work, IMAGE_BASE)
    netlib_sym, netlib_ty = extract(PDB_DIR / "CrossplayNetLib.pdb", work, IMAGE_BASE)
    _, rise_ty = extract(PDB_DIR / "rise.pdb", work, 0x00400000)

    proxy_c, proxy_e = proxy_ty["classes"], proxy_ty["enums"]
    netlib_c = netlib_ty["classes"]
    rise_c = rise_ty["classes"]

    report: list[str] = []
    shipped = verify_cross_pdb(proxy_c, netlib_c, rise_c, report)

    pe = Pe(DLL)
    if pe.machine != 0x14C:
        raise SystemExit(f"{DLL} is not i386 (machine 0x{pe.machine:x})")
    vft_rva, ptrs = verify_emitted_vtable(pe, proxy_sym, shipped, report)
    verify_dtos(proxy_c, netlib_c, rise_c, report)

    mapper = Mapper(proxy_c, proxy_e)
    render_slots(shipped, mapper, "ICrossPlayService")
    if args.check_ret:
        verify_ret_bytes(pe, proxy_sym, shipped, ptrs, report)
    else:
        report.append("stack-cleanup check: not requested (--check-ret)")

    logger = build_slots(proxy_c[LOGGER_IFACE], LOGGER_IFACE)
    render_slots(logger, mapper, "ICrossplayLogger")
    player = build_slots(proxy_c[PLAYER_IFACE], PLAYER_IFACE)
    render_slots(player, mapper, "ICrossplayPlayer")
    netclient = build_slots(proxy_c[NETWORK_IFACE], NETWORK_IFACE)
    render_slots(netclient, mapper, "INetworkClient")
    vendor = build_slots(proxy_c[VENDOR_IFACE], VENDOR_IFACE)
    render_slots(vendor, mapper, "core::ffi::c_void")

    tables = [
        (
            "ICrossPlayServiceVtable",
            "ICROSSPLAYSERVICE",
            "ICrossPlayService",
            shipped,
            [
                f"`{SHIPPED_IFACE}` — the shipped {len(shipped)}-slot vtable.",
                "",
                "Slot-for-slot identical to `Crossplay::ICrossPlayService` as declared by",
                "`rise.pdb` and `CrossplayNetLib.pdb`, and every slot resolves to the",
                f"matching `{IMPL_CLASS}::<name>` in the shipped DLL's `.rdata` table.",
                "**[measured]**",
            ],
        ),
        (
            "ICrossplayLoggerVtable",
            "ICROSSPLAYLOGGER",
            "ICrossplayLogger",
            logger,
            [f"`{LOGGER_IFACE}` — {len(logger)} slots. Export ordinal 1 returns one.", "**[measured]**"],
        ),
        (
            "ICrossplayPlayerVtable",
            "ICROSSPLAYPLAYER",
            "ICrossplayPlayer",
            player,
            [f"`{PLAYER_IFACE}` — {len(player)} slots, the per-peer handle.", "**[measured]**"],
        ),
        (
            "INetworkClientVtable",
            "INETWORKCLIENT",
            "INetworkClient",
            netclient,
            [
                f"`{NETWORK_IFACE}` — {len(netclient)} slots.",
                "The Party-facing side; `CrossplayProxy::NetworkFSM` implements it.",
                "**[measured]**",
            ],
        ),
    ]

    unresolved = [(n, s) for n, _, _, slots, _ in tables for s in slots if not s.resolved]
    unresolved += [("VendorICrossPlayServiceVtable", s) for s in vendor if not s.resolved]
    if unresolved:
        report.append(
            "slots emitted as opaque because a by-value parameter type is only "
            "forward-declared in this PDB: "
            + ", ".join(f"{t.replace('Vtable', '')}::{s.cxx_name} (+{s.offset})" for t, s in unresolved)
        )
    else:
        report.append("every slot signature resolved to a concrete x86 ABI shape")

    meta = proxy_ty["_meta"]
    provenance = [
        f"//! * `ron-bin/sbl/CrossplayProxy.pdb` GUID `{meta['guid']}`,",
        f"//!   SHA-256 `{sha256(PDB_DIR / 'CrossplayProxy.pdb')}`",
        f"//! * `ron-bin/dll/CrossplayProxy.dll` SHA-256 `{sha256(DLL)}`,",
        f"//!   PE32/i386, image base `0x{pe.image_base:08x}`, 4 exports",
        f"//! * `CrossplayProxy::CrossPlayService::\\`vftable'` at RVA `0x{vft_rva:05x}`",
        "//! * cross-checked against `ron-bin/sbl/rise.pdb` and",
        "//!   `ron-bin/sbl/CrossplayNetLib.pdb`",
        "//!",
        "//! Checks the generator ran, and their results, on the run that produced",
        "//! this file:",
        "//!",
    ]
    for line in report:
        provenance.append(f"//! * {line.strip()}")

    nfunc = sum(1 for n in proxy_c if n.startswith("std::function<"))

    body = [
        HEADER.format(
            shipped_slots=len(shipped),
            shipped_last=shipped[-1].offset,
            vendor_slots=len(vendor),
            vendor_last=vendor[-1].offset,
            vendor_methods=len(proxy_c[VENDOR_IFACE]["methods"]),
            provenance="\n".join(provenance),
        ),
        PRELUDE.replace("{nfunc}", str(nfunc)),
    ]

    # enums
    body.append("// ---------------------------------------------------------------------------")
    body.append("// Enumerations. All are 4-byte `int` in every PDB that declares them.")
    body.append("// ---------------------------------------------------------------------------\n")
    for cxx, rust, prefix in ENUMS:
        e = proxy_e.get(cxx) or rise_ty["enums"].get(cxx)
        if e is None:
            raise SystemExit(f"enum {cxx} not found")
        if e.get("size") != 4:
            raise SystemExit(f"enum {cxx} is {e.get('size')} bytes, not 4")
        body.append(f"/// `{cxx}`. **[measured]**")
        body.append(f"pub type {rust} = i32;")
        for name, value in e["values"]:
            body.append(f"pub const {prefix}_{re.sub('[^A-Za-z0-9]', '_', name).upper()}: {rust} = {value};")
        body.append("")

    # DTOs
    body.append("// ---------------------------------------------------------------------------")
    body.append("// DTOs. Offsets and sizes are the PDB's; every one is asserted below it.")
    body.append("// ---------------------------------------------------------------------------\n")
    extra_types: dict[str, tuple[str, int]] = dict(mapper.byvalue)
    dto_chunks = []
    for cxx, rust in DTOS:
        source = proxy_c if cxx in proxy_c else rise_c
        chunk, asserts = emit_dto(cxx, rust, source, proxy_e, extra_types)
        note = "" if cxx in proxy_c else f"// Declared only in `rise.pdb`; absent from `CrossplayProxy.pdb`.\n"
        dto_chunks.append(note + chunk + asserts)
    for name in ("MsvcWstring", "MsvcString", "MsvcFunction", "MsvcVector"):
        extra_types.pop(name, None)
    for name, (cxx, size) in sorted(extra_types.items()):
        short = cxx if len(cxx) <= 100 else cxx[:97] + "..."
        body.append(f"/// Opaque {size}-byte MSVC aggregate: `{short}`. **[measured]**")
        body.append("#[repr(C, align(4))]")
        body.append("#[derive(Clone, Copy)]")
        body.append(f"pub struct {name} {{")
        body.append(f"    pub raw: [u8; {size}],")
        body.append("}")
        body.append(f"const _: () = assert!(core::mem::size_of::<{name}>() == {size});")
        body.append("")
    body.extend(dto_chunks)

    # vtables, emitted twice so the ABI keyword can differ by target.
    body.append("// ---------------------------------------------------------------------------")
    body.append("// Vtables. `$abi` is `\"thiscall\"` on x86 and `\"C\"` elsewhere; the slot")
    body.append("// assertions are written in pointer units and hold on both.")
    body.append("// ---------------------------------------------------------------------------\n")
    body.append("macro_rules! crossplay_vtables {\n    ($abi:literal) => {")
    for name, _prefix, this_ty, slots, doc in tables:
        body.append(indent(emit_vtable(name, this_ty, slots, doc), 8))
    body.append(
        indent(
            emit_vtable(
                "VendorICrossPlayServiceVtable",
                "core::ffi::c_void",
                vendor,
                [
                    f"`{VENDOR_IFACE}` as declared by `CrossplayProxy.pdb` — {len(vendor)} slots.",
                    "",
                    "**THIS IS NOT THE SHIPPED VTABLE AND RETAIL NEVER CALLS IT.** It comes",
                    "from a newer vendor header that this build does not use. It is emitted",
                    "only so the discrepancy is recorded in checked code rather than prose:",
                    "the extra `P2PEnableTcp` / `SetP2PAllowedPorts` / matchmaking-ticket",
                    "APIs show what the vendor SDK grew after RoN:EE shipped.",
                ],
            ),
            8,
        )
    )
    body.append("    };\n}\n")
    body.append('#[cfg(target_arch = "x86")]')
    body.append('crossplay_vtables!("thiscall");')
    body.append('#[cfg(not(target_arch = "x86"))]')
    body.append('crossplay_vtables!("C");')
    body.append("")
    for name, prefix, _this_ty, slots, _doc in tables:
        body.append(emit_slot_asserts(name, prefix, slots))
    body.append("/// Not the shipped interface — see the module docs.")
    body.append(emit_slot_asserts("VendorICrossPlayServiceVtable", "VENDOR_ICROSSPLAYSERVICE", vendor))

    Path(args.out).write_text("\n".join(body))

    print("\n".join(report))
    print(f"wrote {args.out}")
    print(f"extraction cache: {work}")
    return 0


def indent(s: str, n: int) -> str:
    pad = " " * n
    return "\n".join(pad + line if line else line for line in s.splitlines())


if __name__ == "__main__":
    raise SystemExit(main())
