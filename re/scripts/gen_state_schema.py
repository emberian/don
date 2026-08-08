#!/usr/bin/env python3
"""Emit /Users/ember/dev/don/schema/state-schema.json -- the simulation-state
schema: for every class the DataWalk visitor serialises, the ORDERED list of
fields it writes, with PDB names, types and sizes.

Inputs
  scratchpad/walkops4.json     instruction-level op traces (walkscan3.py)
  schema/rise-symbols.tsv      PDB public symbols   -> class name per VA
  ron-bin/sbl/rise.pdb         PDB type stream      -> field layout per class
"""
import json
import re
import subprocess
import os
import sys
from collections import defaultdict

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import pdb_layout as P

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..")
UNDNAME = os.environ.get("UNDNAME", "/opt/homebrew/opt/llvm/bin/llvm-undname")

ops_by_va = json.load(open(os.environ.get("WALKOPS_OUT",
                       os.path.join(ROOT, "schema", "walkops.json"))))

sym = defaultdict(list)
for line in open(os.path.join(ROOT, "schema", "rise-symbols.tsv")):
    va, sec, name = line.rstrip("\n").split("\t")
    sym[va].append(name)

WALK = re.compile(r"^\?(walk[a-z_]*data|walk_[a-z_]*)@")
walkers = {va: [n for n in ns if WALK.match(n)]
           for va, ns in sym.items() if any(WALK.match(n) for n in ns)}

allm = sorted({n for v in walkers.values() for n in v} |
              {n for ns in sym.values() for n in ns[:1]})
dem = {}
CH = 4000
for i in range(0, len(allm), CH):
    part = allm[i:i + CH]
    res = subprocess.run([UNDNAME] + part, capture_output=True, text=True)
    lines = res.stdout.split("\n")
    j = 0
    ps = set(part)
    while j + 1 < len(lines):
        if lines[j] in ps:
            dem[lines[j]] = lines[j + 1].strip()
            j += 2
        else:
            j += 1

CLS = re.compile(r"__(?:thiscall|cdecl|stdcall|fastcall)\s+(.+?)::([\w~]+)[(`]")


def classof(mangled):
    m = CLS.search(dem.get(mangled, ""))
    if not m:
        return None, None
    return (m.group(1).replace("class ", "").replace("struct ", "").strip(),
            m.group(2))


def is_thunk(n):
    return "$4" in n or dem.get(n, "").startswith("[thunk]")


_lay = {}


def layout(c):
    if c not in _lay:
        try:
            _lay[c] = P.flatten(c) or []
        except Exception:
            _lay[c] = []
    return _lay[c]


def fields_in(c, lo, hi):
    out = []
    for f in layout(c):
        if f["off"] is None or not f["size"]:
            continue
        if f["off"] < hi and f["off"] + f["size"] > lo:
            out.append(dict(off=f["off"], size=f["size"], name=f["name"],
                            type=f["type"],
                            partial=not (lo <= f["off"] and f["off"] + f["size"] <= hi)))
    return out


def hexva(v):
    return "0x%08x" % v


classes = {}
va_to_classes = {}
for va, names in walkers.items():
    cs = sorted({classof(n)[0] for n in names if not is_thunk(n)} - {None})
    if not cs:
        cs = sorted({classof(n)[0] for n in names} - {None})
    va_to_classes[va] = cs

for va, fn in sorted(ops_by_va.items()):
    cs = va_to_classes.get(va) or []
    primary = cs[0] if cs else "FUN_%s" % va
    meth = None
    for n in walkers.get(va, []):
        c, m = classof(n)
        if c == primary:
            meth = m
    ordered = []
    subs = []
    unresolved = []
    walked = set()
    idx = 0
    for o in fn["ops"]:
        k = o["k"]
        if k == "tag":
            a = o.get("arg")
            ordered.append(dict(i=idx, kind="tag", at=o["at"], bytes=1,
                                note="walk_test -> 1 tag byte",
                                tag_string_index=(a[1] // 20) if a and a[0] == "strpool" else None,
                                guard_depth=o["g"], in_loop=bool(o["L"])))
            idx += 1
        elif k == "walk":
            b, e = o.get("begin"), o.get("end")
            rec = dict(i=idx, kind="bytes", at=o["at"],
                       guard_depth=o["g"], in_loop=bool(o["L"]))
            if b and e and b[0] == "this" and e[0] == "this":
                n = e[1] - b[1]
                rec.update(begin=b[1], end=e[1], bytes=n,
                           fields=fields_in(primary, b[1], e[1]))
                for x in range(b[1], e[1]):
                    walked.add(x)
            elif b and e and b[0] == e[0] and b[0] in ("frame", "esp"):
                rec.update(bytes=e[1] - b[1], source="computed temporary",
                           note="a value built on the stack, not a struct field")
            elif (b and e and b[0] == "gload" and e[0] == "gload"
                  and b[1] == e[1]):
                rec.update(base="*(void**)0x%08x" % b[1], begin=b[2], end=e[2],
                           bytes=e[2] - b[2],
                           note="range on a global object, not on `this`")
            elif b and b[0] == "this" and e is None:
                rec.update(begin=b[1], bytes=None,
                           note="variable length; end computed at run time")
                unresolved.append(o["at"])
            else:
                rec.update(begin=b, end=e, bytes=None, note="operands not resolved")
                unresolved.append(o["at"])
            ordered.append(rec)
            idx += 1
        elif k == "call":
            t = o["target"]
            tc = va_to_classes.get(t)
            if tc or t in walkers:
                th = o.get("this")
                ordered.append(dict(i=idx, kind="sub_object", at=o["at"],
                                    target=t, target_class=(tc or [None])[0],
                                    target_method=classof(sorted(walkers[t], key=len)[0])[1]
                                    if t in walkers else None,
                                    this=th, guard_depth=o["g"],
                                    in_loop=bool(o["L"])))
                subs.append((tc or [None])[0] or t)
                idx += 1
        elif k in ("vcall", "vcall?") and o["slot"] in (0x7c, 0xac, 0xb0, 0x78):
            ordered.append(dict(i=idx, kind="virtual", at=o["at"],
                                slot="0x%x" % o["slot"],
                                note="dispatch on the walked object's own vtable",
                                guard_depth=o["g"], in_loop=bool(o["L"])))
            unresolved.append(o["at"])
            idx += 1

    sz = P.size_of(primary)
    ent = dict(
        walk_data=hexva(int(va, 16)),
        code_size=fn["size"],
        method=meth,
        sizeof=sz,
        shared_with=[c for c in cs if c != primary],
        walked_bytes=len(walked),
        coverage=(round(len(walked) / sz, 4) if sz else None),
        loops=[["0x" + a, "0x" + b] for a, b in fn["backedges"]][:8],
        unresolved_ops=unresolved,
        ops=ordered,
    )
    classes[primary] = ent

meta = dict(
    what="Simulation-state schema of Rise of Nations: Extended Edition. "
         "DataWalk is a two-method pure-virtual visitor; SaveGame, LoadGame and "
         "CheckSum are its only concrete implementations, so the byte ranges "
         "listed here are simultaneously the save-game format, the replay "
         "header format, and the definition of lockstep-critical state.",
    binary="riseofnations.exe sha256 30478a44..625079, PE32 i386, image base 0x00400000",
    pdb="ron-bin/sbl/rise.pdb -- GUID 51D4F219-61C6-4F84-9D5B-C3361B0D291F age 1, "
        "byte-identical to the PE CodeView record, so the type stream is "
        "authoritative for THIS build",
    method="1. every .text symbol start in rise-symbols.tsv, merged with "
           "schema/islands.jsonl, gives function bounds; "
           "2. each function is linearly abstract-interpreted (capstone) tracking "
           "this/walker/frame-relative register values; a walk_data is any "
           "function that calls walker->vt[0](begin,end) or walker->vt[1](tag); "
           "3. the recovered this-relative byte ranges are intersected with the "
           "PDB TPI layout of the class the PDB names for that VA. "
           "Order is program order; guard_depth counts conditional branches that "
           "jump over the op; in_loop marks ops inside a back-edge.",
    caveats=[
        "This is a linear scan with no dataflow join. Ops whose operands are "
        "computed across branches, and loop trip counts, are NOT resolved -- see "
        "unresolved_ops per class and `bytes: null` entries.",
        "guard_depth and in_loop are OVER-APPROXIMATIONS. guard_depth counts "
        "conditional forward jumps whose span contains the op; in_loop is true if "
        "ANY back-edge spans it. Compiler loop rotation and tail merging can make "
        "a straight-line op look guarded or looped. Treat them as `look here`, "
        "not as recovered control flow.",
        "Ranges are what SaveGame/LoadGame walk. CheckSum takes a few different "
        "branches (walker+0x08 != 0) and is additionally gated by the section "
        "mask at walker+0x0c.",
        "Nothing here is Tier A or Tier B: no SMT proof, no differential test "
        "against the retail reader.",
    ],
    interface=dict(
        DataWalk_vftable="0x00b2bcd8 (both slots -> _purecall thunk 0x0055e0a6)",
        slot0="walk_function(void* begin, void* end)",
        slot1="walk_test(const String& tag)",
        SaveGame=dict(vftable="0x00b35ac4", walk_function="0x0043d730",
                      walk_test="0x0043d840",
                      walk_function_is="fwrite(begin,1,end-begin,f) or gzwrite"),
        LoadGame=dict(vftable="0x00b30c88", walk_function="0x0043d950",
                      walk_test="0x0043da60"),
        CheckSum=dict(vftable="0x00b3f920", walk_function="0x00936ff0",
                      walk_test="0x0041bfe0 (a bare `ret 4` -- no-op)"),
        DataWalk_layout={
            "+0x00": "vftable",
            "+0x04": "direction: non-zero => reading (LoadGame)",
            "+0x08": "non-zero => CheckSum; walk_data uses it to skip "
                     "non-checksummed fields",
            "+0x0c": "section mask, gates optional sub-walks",
            "+0x10": "running adler-32 (CheckSum only)",
            "+0x14": "bytes-walked counter (CheckSum only)",
        },
    ),
    primitives=dict(
        integers="raw little-endian, natural width, NO alignment or padding "
                 "inserted by the serialiser -- a walk of [a,b) emits b-a bytes",
        tag_byte="walk_test emits exactly one byte: String::module_id (String+0x10), "
                 "set by FUN_00a1b6b0 when the tag name is hashed. Observed values: "
                 "0x16 Game, 0x42 GameInfo, 0x50 GameInfo player slot",
        String="String::walk_data 0x00a1b2d0 -- u32 character count, then count "
               "UTF-16LE code units, NOT nul-terminated. count 0 emits only the u32",
        Array="Array<T> 0x00471c30 / SimpleArray<T> 0x00473120 / ObjectArray<T> "
              "0x00474420 / PtrArray<T> 0x0045cce0 all share the header: "
              "u32 length; if length != 0 { u32 size; u16 increment; u8 flags; "
              "<elements> }. SimpleArray writes the elements as ONE bulk "
              "walk(list, list + length*sizeof(T)); ObjectArray/Array call each "
              "element's walk_data; PtrArray serialises ids, not pointers",
        Stack="Stack<T> 0x0046d8b0 -- walk(this+4, this+0xd) = 9 bytes "
              "{int size; int length; char increment}, then length x sizeof(T) "
              "bulk element walks",
        varblock="the recurring idiom  walk(p, p+8); walk(p+0xc, p + *(u32*)(p+4) "
                 "+ 0xc)  is a length-prefixed inline buffer: 8-byte header whose "
                 "second dword is the payload length, payload at p+0xc. "
                 "Game::semaphore is the canonical instance",
        pointers="never serialised as pointers. Either an id is extracted from the "
                 "pointee and walked (Object::walk_data 0x006621d0 does exactly "
                 "this) or the pointee's walk_data is called",
    ),
    containers=dict(
        rcx="recorded game. gzip from offset 0, one member. Stream begins "
            "immediately with Game::walk_data (FUN_00589600); no magic and no "
            "version word. Written by CommandManager FUN_00952a50, gzipped "
            "wholesale at game end by FUN_00952b40",
        svx="save game. gzip from offset 0. SaveGame::save_game 0x005a8220 writes "
            "String magic ('RoNSave' | 'RoNMultiSave' | 'RoNCTWSave'), then u32 "
            "sGameSaveVersion [0x00c06240] (0x10 in this build), then "
            "WalkDataGame::walk_data 0x005a2360",
        svx_ctw_map="ConquestSaveGame map backup: magic 'RonCTWMapSave', NOT gzipped",
        version_gate="GameInfo::walk_data branches on sGameSaveVersion >= 0x10 at "
                     "0x005d6734: format 16 writes a mod-package block "
                     "(u32,u32,String,String,String,u32,u32,String); format 15 and "
                     "earlier write four Strings",
    ),
)

counts = dict(
    classes=len(classes),
    walk_data_symbols_in_pdb=len(walkers),
    functions_scanned=len(ops_by_va),
    ordered_ops=sum(len(c["ops"]) for c in classes.values()),
    byte_ranges=sum(1 for c in classes.values() for o in c["ops"] if o["kind"] == "bytes"),
    resolved_byte_ranges=sum(1 for c in classes.values() for o in c["ops"]
                             if o["kind"] == "bytes" and o.get("bytes") is not None),
    named_fields=sum(len(o.get("fields", [])) for c in classes.values() for o in c["ops"]),
    classes_with_pdb_layout=sum(1 for c in classes.values() if c["sizeof"]),
    total_sim_critical_bytes=sum(c["walked_bytes"] for c in classes.values()),
    unresolved_ops=sum(len(c["unresolved_ops"]) for c in classes.values()),
)
meta["counts"] = counts

json.dump(dict(_meta=meta, classes=classes),
          open(os.path.join(ROOT, "schema", "state-schema.json"), "w"), indent=1, sort_keys=False)
print(json.dumps(counts, indent=1))
