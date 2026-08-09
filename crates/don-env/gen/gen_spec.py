#!/usr/bin/env python3
"""Generate the don-env action/observation spec from derived artefacts.

Two outputs, deliberately separated by copyright:

  crates/don-env/src/generated.rs   -- names, opcodes, enum values, head layout.
                                       Sourced from schema/command-wire.json and
                                       schema/types.json, both of which are derived
                                       from the PDB type stream and are committed.
                                       Contains NO game balance data.

  schema/live/env-typecaps.bin      -- per-TypeIndex capability records and the
                                       producer -> product edge list, extracted from
                                       ron-data/unitrules.xml + buildingrules.xml.
                                       That is shipped game data, so it lands in the
                                       gitignored schema/live/ tree and don-env loads
                                       it at runtime. Without it the env still runs;
                                       masks fall back to permissive and say so.

Run from the repo root:  python3 crates/don-env/gen/gen_spec.py
"""

import json
import os
import re
import struct
import sys
import xml.etree.ElementTree as ET

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))


def load(p):
    with open(os.path.join(ROOT, p), encoding="utf-8") as f:
        return json.load(f)


# ---------------------------------------------------------------------------
# 1. Command opcodes -> verbs.
#
# Every one of the 82 opcodes is classified exactly once. The classification is a
# judgement about which opcodes an *agent* may emit; the opcode list, its names and
# its field layouts are all measured (schema/command-wire.json, CommandTypes enum).
# ---------------------------------------------------------------------------

# opcode -> group. UNIT verbs are directed at entities the actor owns; PLAYER verbs
# are emitted once per actor per step; the rest are not agent actions at all.
GROUPS = {
    # --- UNIT ---
    2: "UNIT", 3: "UNIT", 4: "UNIT", 5: "UNIT", 6: "UNIT", 7: "UNIT", 8: "UNIT",
    9: "UNIT", 10: "UNIT", 11: "UNIT", 12: "UNIT", 13: "UNIT", 14: "UNIT",
    15: "UNIT", 16: "UNIT", 17: "UNIT", 18: "UNIT", 19: "UNIT", 20: "UNIT",
    21: "UNIT", 22: "UNIT", 23: "UNIT", 24: "UNIT", 25: "UNIT", 26: "UNIT",
    28: "UNIT", 29: "UNIT", 30: "UNIT", 31: "UNIT", 35: "UNIT", 36: "UNIT",
    48: "UNIT", 49: "UNIT",
    # --- PLAYER ---
    27: "PLAYER", 32: "PLAYER", 33: "PLAYER", 37: "PLAYER", 38: "PLAYER",
    39: "PLAYER", 40: "PLAYER", 41: "PLAYER", 42: "PLAYER", 43: "PLAYER",
    44: "PLAYER", 45: "PLAYER", 46: "PLAYER", 47: "PLAYER", 70: "PLAYER",
    73: "PLAYER",
    # --- SELECTION: expressed by the (entity, action) action format itself, so it
    #     never needs to be an emitted verb. Kept classified, not dropped.
    0: "SELECTION", 34: "SELECTION",
    # --- UI / presentation, no simulation effect on the acting player's state ---
    50: "UI", 51: "UI", 68: "UI", 69: "UI", 72: "UI", 75: "UI", 78: "UI", 79: "UI",
    # --- ADMIN: lockstep protocol, session control, speed ---
    1: "ADMIN", 52: "ADMIN", 53: "ADMIN", 54: "ADMIN", 55: "ADMIN", 56: "ADMIN",
    57: "ADMIN", 58: "ADMIN", 71: "ADMIN", 74: "ADMIN", 76: "ADMIN", 77: "ADMIN",
    80: "ADMIN", 81: "ADMIN",
    # --- CHEAT ---
    59: "CHEAT", 60: "CHEAT", 61: "CHEAT", 62: "CHEAT", 63: "CHEAT", 64: "CHEAT",
    65: "CHEAT", 66: "CHEAT", 67: "CHEAT",
}

# Command field name -> which action head supplies it. Derived by reading the field
# names in schema/command-wire.json; anything a head cannot supply is listed in
# UNSUPPLIED and gets a documented constant.
FIELD_TO_HEAD = {
    "to_x": "TargetX", "x": "TargetX", "x2": "TargetX",
    "to_y": "TargetY", "y": "TargetY", "y2": "TargetY",
    "ox": "TargetEntity", "whom": "TargetEntity",
    "type": "Type",
    "queued": "QueuePos",
    "stance": "Stance",
    "form": "Form", "rotate": "Form",
    "orders": "OrderMods",
    "num": "Count",
    "good": "Good",
    "amount": "Amount",
    "treaty": "Treaty",
    "who": "TargetPlayer",
}
# Fields no head supplies, and the constant used instead. Each is a real gap.
UNSUPPLIED = {
    "ignore": "0",            # AttackCommand: 'attack even if it looks futile'
    "tolerance": "0",         # MoveNearCommand: radius slack, engine default
    "set_angle": "0",         # facing not exposed; engine picks
    "angle": "0",
    "width": "0",             # formation front width
    "disembark": "0",
    "shift": "0", "ctrl": "0", "alt": "0",   # modifier keys on Flight/LaunchPatrol
    "flag": "1",              # SetTransportCommand toggle
    "all": "0",               # DisbandCommand: disband whole selection
    "action": "0",            # GatherPointCommand action code
    "add_to_end": "0",
    "back_to_work": "0", "eject_o": "-1", "eject_who": "-1",
    "oxx": "-1", "whose": "-1",   # TradeCommand second endpoint
    "uid": "0",
    "o": "-1",
    "t": "0",
    "unitmask": "0", "buildmask": "0", "set": "1",
    "onoff": "1",
    "flags": "0",
    "play": "1",
    "leader_option": "0",
    "list": "0",
    "spline_type": "0", "spline_flags": "0", "spline_cmd": "0", "len": "0",
    "vert_data": "0",
    "orders_index": "0",
}


def rustify(name):
    """COMMAND_MOVE_TO -> MoveTo"""
    return "".join(p.capitalize() for p in name.split("_"))


def main():
    cmds = {c["op"]: c for c in load("schema/command-wire.json")}
    types = load("schema/types.json")
    enums = types["enums"]
    cmd_names = {v: n for n, v in enums["CommandTypes"]["values"] if n != "NUM_COMMANDTYPES"}

    assert len(cmds) == 82, len(cmds)
    assert set(GROUPS) == set(range(82)), sorted(set(range(82)) - set(GROUPS))

    unit_ops = sorted(op for op, g in GROUPS.items() if g == "UNIT")
    player_ops = sorted(op for op, g in GROUPS.items() if g == "PLAYER")

    # TypeIndex categories, from the NUM_* members of the same enum. These are the
    # engine's own partition of the 806 type ids.
    ti = enums["TypeIndex"]["values"]
    num = dict(ti)
    NUM_TYPES = num["NUM_TYPES"]
    NUM_GOODTYPES = num["NUM_GOODTYPES"]
    NUM_UNITTYPES = num["NUM_UNITTYPES"]
    NUM_GAIATYPES = num["NUM_GAIATYPES"]
    NUM_BUILDTYPES = num["NUM_BUILDTYPES"]
    NUM_COMMON = num["NUM_COMMON"]
    UNIT_BASE = NUM_GOODTYPES
    GAIA_BASE = UNIT_BASE + NUM_UNITTYPES
    BUILD_BASE = GAIA_BASE + NUM_GAIATYPES
    assert BUILD_BASE == 414, BUILD_BASE

    orders = [(n, v) for n, v in enums["OrderIndex"]["values"] if n != "NUM_UNIT_ORDERS"]
    stances = [(n, v) for n, v in enums["StanceTypes"]["values"]
               if not n.startswith("NUM_") and v < 100]
    forms = [(n, v) for n, v in enums["FormIndex"]["values"]
             if not n.startswith("NUM_") and v < 100]
    queuepos = enums["QueuePos"]["values"]
    diplo = [(n, v) for n, v in enums["<unnamed-enum-WAR>"]["values"] if v < 100
             and not n.startswith("NUM_")]
    order_flags = enums["<unnamed-enum-ORDER_PATHED>"]["values"]
    terrain_flags = enums["<unnamed-enum-FLAG_GOODY>"]["values"]

    out = []
    w = out.append
    w("// @generated by crates/don-env/gen/gen_spec.py -- DO NOT EDIT BY HAND.")
    w("//")
    w("// Sources, all [measured]:")
    w("//   schema/command-wire.json  82 command opcodes, struct sizes, field layouts")
    w("//   schema/types.json         PDB TPI enums: CommandTypes, OrderIndex, TypeIndex,")
    w("//                             StanceTypes, FormIndex, QueuePos, diplomacy, order flags")
    w("//")
    w("// No game balance data appears here; that lives in schema/live/env-typecaps.bin.")
    w("#![allow(dead_code)]")
    w("")
    w("/// Total TypeIndex universe. `NUM_TYPES` in the PDB enum.")
    w(f"pub const NUM_TYPES: usize = {NUM_TYPES};")
    w(f"pub const NUM_GOODTYPES: usize = {NUM_GOODTYPES};")
    w(f"pub const NUM_UNITTYPES: usize = {NUM_UNITTYPES};")
    w(f"pub const NUM_GAIATYPES: usize = {NUM_GAIATYPES};")
    w(f"pub const NUM_BUILDTYPES: usize = {NUM_BUILDTYPES};")
    w("/// The six gathered resources: FOOD TIMBER WEALTH KNOWLEDGE METAL OIL.")
    w(f"pub const NUM_COMMON: usize = {NUM_COMMON};")
    w(f"pub const UNIT_TYPE_BASE: usize = {UNIT_BASE};")
    w(f"pub const GAIA_TYPE_BASE: usize = {GAIA_BASE};")
    w(f"pub const BUILD_TYPE_BASE: usize = {BUILD_BASE};")
    w("")
    w("/// Owner slots iterated by `Objects::process_all`, which rotates the start each")
    w("/// frame as `(frame + i) % 10` [measured]. Eight are players; the remainder are")
    w("/// gaia/neutral slots.")
    w("pub const NUM_OWNER_SLOTS: usize = 10;")
    w("pub const NUM_PLAYERS: usize = 8;")
    w("")
    w("/// Milliseconds per simulation tick at each speed setting, from")
    w("/// `TurnControl::timings` at 0x00AFC4A4 [measured]. Index 1 is Normal.")
    w("pub const TICK_MS: [u32; 5] = [200, 125, 67, 50, 1];")
    w("pub const TICK_MS_NORMAL: u32 = TICK_MS[2];")
    w("")

    def emit_enum(name, doc, entries, repr_ty="u16"):
        w(f"/// {doc}")
        w("#[derive(Clone, Copy, PartialEq, Eq, Debug)]")
        w(f"#[repr({repr_ty})]")
        w(f"pub enum {name} {{")
        for n, v in entries:
            w(f"    {rustify(n)} = {v},")
        w("}")
        w("")

    emit_enum("OrderIndex", "`UnitOrder` subclasses, the engine's own order taxonomy.", orders)
    emit_enum("Stance", "`StanceTypes`.", [(n, v) for n, v in stances], "u8")
    emit_enum("QueuePos", "`QueuePos`, the `queued` field of every queued command.", queuepos, "u8")
    emit_enum("Diplo", "Diplomatic state; `TreatyCommand::treaty` / `DeclareCommand::treaty`.",
              diplo, "u8")

    w("/// `FormIndex`. `Line..ELeft` are the five real formations; the rest are")
    w("/// selectable extras and the 253..255 values are relative selectors.")
    w("pub const FORMS: [(&str, u8); %d] = [" % len(forms))
    for n, v in forms:
        w(f'    ("{n}", {v}),')
    w("];")
    w("")
    w("/// `ORDER_*` bit flags carried in the `orders` byte of the move commands.")
    w("pub const ORDER_FLAGS: [(&str, u32); %d] = [" % len(order_flags))
    for n, v in order_flags:
        w(f'    ("{n}", {v}),')
    w("];")
    w("")
    w("/// Terrain coordinate flags (`FLAG_*`), the natural source of spatial planes.")
    w("pub const TERRAIN_FLAGS: [(&str, u32); %d] = [" % len(terrain_flags))
    for n, v in terrain_flags:
        w(f'    ("{n}", {v}),')
    w("];")
    w("")

    # ---- verb tables -------------------------------------------------------
    def verb_table(kind, ops):
        rows = []
        for op in ops:
            c = cmds[op]
            cname = cmd_names[op]
            verb = cname[len("COMMAND_"):]
            heads, consts = [], []
            for f in c["fields"]:
                fn = f["name"]
                if fn in FIELD_TO_HEAD:
                    heads.append(FIELD_TO_HEAD[fn])
                elif fn in UNSUPPLIED:
                    consts.append(fn)
                else:
                    raise SystemExit(f"unclassified field {cname}.{fn}")
            # `whom`/`who` mean owner-slot in unit commands and target player in the
            # diplomacy commands; the group decides.
            if kind == "PLAYER":
                heads = ["TargetPlayer" if h == "TargetEntity" else h for h in heads]
            rows.append((op, verb, c["struct"], c["size"], sorted(set(heads)), consts))
        return rows

    unit_rows = verb_table("UNIT", unit_ops)
    player_rows = verb_table("PLAYER", player_ops)

    unit_heads = ["Verb", "TargetX", "TargetY", "TargetEntity", "Type", "QueuePos",
                  "Stance", "Form", "OrderMods", "Count"]
    player_heads = ["Verb", "TargetPlayer", "Good", "Amount", "Treaty"]

    for kind, heads in (("Unit", unit_heads), ("Player", player_heads)):
        w(f"/// Parameter heads of the {kind.lower()} action group, in array order.")
        w("#[derive(Clone, Copy, PartialEq, Eq, Debug)]")
        w("#[repr(usize)]")
        w(f"pub enum {kind}Head {{")
        for i, h in enumerate(heads):
            w(f"    {h} = {i},")
        w("}")
        w(f"pub const N_{kind.upper()}_HEADS: usize = {len(heads)};")
        w(f'pub const {kind.upper()}_HEAD_NAMES: [&str; {len(heads)}] = [{", ".join(chr(34) + h + chr(34) for h in heads)}];')
        w("")

    w("/// One agent-emittable verb: the engine opcode it becomes, and which heads")
    w("/// supply its parameters.")
    w("pub struct VerbDef {")
    w("    pub name: &'static str,")
    w("    /// `CommandTypes` opcode this verb serialises to.")
    w("    pub opcode: u8,")
    w("    /// `sizeof` the wire struct, from the PDB.")
    w("    pub wire_size: u16,")
    w("    /// Bit i set => head i of this group is read for this verb.")
    w("    pub heads: u32,")
    w("    /// Command fields no head supplies; each is a documented gap.")
    w("    pub unsupplied: &'static [&'static str],")
    w("}")
    w("")

    def emit_verbs(kind, rows, heads):
        idx = {h: i for i, h in enumerate(heads)}
        w(f"/// {len(rows)} {kind.lower()} verbs. Index 0 of the Verb head is NOOP, so the")
        w(f"/// head size is {len(rows)} + 1.")
        w(f"pub const {kind.upper()}_VERBS: [VerbDef; {len(rows)}] = [")
        for op, verb, struct_name, size, hs, consts in rows:
            mask = 1 << idx["Verb"]
            for h in hs:
                if h in idx:
                    mask |= 1 << idx[h]
            cs = ", ".join(f'"{c}"' for c in consts)
            w(f'    VerbDef {{ name: "{verb}", opcode: {op}, wire_size: {size}, '
              f"heads: 0x{mask:x}, unsupplied: &[{cs}] }},  // {struct_name}")
        w("];")
        w(f"pub const N_{kind.upper()}_VERBS: usize = {len(rows)};")
        w("")

    emit_verbs("Unit", unit_rows, unit_heads)
    emit_verbs("Player", player_rows, player_heads)

    # Verb name -> Rust const index, so mask code can be readable.
    w("/// Indices into `UNIT_VERBS`, +1 to get the Verb head value (0 = NOOP).")
    w("pub mod uv {")
    for i, (_op, verb, *_r) in enumerate(unit_rows):
        w(f"    pub const {verb}: usize = {i};")
    w("}")
    w("pub mod pv {")
    for i, (_op, verb, *_r) in enumerate(player_rows):
        w(f"    pub const {verb}: usize = {i};")
    w("}")
    w("")

    # Non-agent opcodes, recorded so the classification is auditable from Rust.
    for g in ("SELECTION", "UI", "ADMIN", "CHEAT"):
        ops = sorted(op for op, gg in GROUPS.items() if gg == g)
        items = ", ".join(f'({op}, "{cmd_names[op][8:]}")' for op in ops)
        w(f"/// Opcodes classified {g}: not agent actions.")
        w(f"pub const {g}_OPCODES: [(u8, &str); {len(ops)}] = [{items}];")
    w("")

    dst = os.path.join(ROOT, "crates/don-env/src/generated.rs")
    with open(dst, "w", encoding="utf-8") as f:
        f.write("\n".join(out) + "\n")
    print(f"wrote {dst}  ({len(unit_rows)} unit verbs, {len(player_rows)} player verbs)")

    # ---- capability table --------------------------------------------------
    emit_typecaps(NUM_TYPES, UNIT_BASE, BUILD_BASE, NUM_BUILDTYPES)


# TypeCaps flag bits. Mirrored in crates/don-env/src/typecaps.rs.
F_MOVE, F_ATTACK, F_CIVILIAN, F_SIEGE = 1 << 0, 1 << 1, 1 << 2, 1 << 3
F_GARR_TOWN, F_GARR_FORT, F_CASTER, F_AIR = 1 << 4, 1 << 5, 1 << 6, 1 << 7
F_SEA, F_TRANSPORT, F_STEALTH, F_DETECT = 1 << 8, 1 << 9, 1 << 10, 1 << 11
F_ANTIAIR, F_PRODUCER, F_BUILDING, F_UNIT = 1 << 12, 1 << 13, 1 << 14, 1 << 15

RES_LETTER = {"f": 0, "t": 1, "g": 2, "k": 3, "m": 4, "o": 5}
RES_WORD = {"food": 0, "timber": 1, "wealth": 2, "knowledge": 3, "metal": 4, "oil": 5}


def parse_cost(s):
    """'5t' -> [0,5,0,0,0,0]; '2f 1m' -> both. Values are as written in the file;
    the unitrules comment says COST is the base cost multiplied by 10."""
    out = [0] * 6
    if not s:
        return out
    for num_s, letter in re.findall(r"(\d+)\s*([ftgkmo])", s):
        out[RES_LETTER[letter]] += int(num_s)
    return out


def parse_range(s):
    m = re.match(r"\s*(\d+)\s*-\s*(\d+)", s or "")
    return (int(m.group(1)), int(m.group(2))) if m else (0, 0)


def as_int(s, default=0):
    try:
        return int(str(s).strip())
    except (TypeError, ValueError):
        m = re.match(r"\s*(-?\d+)", str(s or ""))
        return int(m.group(1)) if m else default


def emit_typecaps(num_types, unit_base, build_base, num_buildtypes):
    up = os.path.join(ROOT, "ron-data/unitrules.xml")
    bp = os.path.join(ROOT, "ron-data/buildingrules.xml")
    if not (os.path.exists(up) and os.path.exists(bp)):
        print("ron-data/ absent; skipping schema/live/env-typecaps.bin", file=sys.stderr)
        return

    units = ET.parse(up).getroot().findall("UNIT")
    builds = ET.parse(bp).getroot().findall("BUILDING")
    # Positional correspondence, cross-validated against the TypeIndex enum:
    # unitrules entry i  <-> TypeIndex 50 + i  (352 units then 12 gaia = 364)
    # buildingrules j    <-> TypeIndex 414 + j (129, exactly NUM_BUILDTYPES)
    assert len(builds) == num_buildtypes, len(builds)
    assert len(units) == 364, len(units)

    caps = [None] * num_types
    build_name_to_type = {}
    for j, b in enumerate(builds):
        t = build_base + j
        obj = b.findtext("OBJ_MASKS") or ""
        flags = F_BUILDING
        attack = as_int(b.findtext("ATTACK"))
        if attack > 0:
            flags |= F_ATTACK
        if as_int(b.findtext("GARRISON_MAX")) > 0:
            flags |= F_GARR_TOWN
        rmin, rmax = parse_range(b.findtext("RANGE"))
        caps[t] = dict(
            flags=flags, attack=attack, hits=as_int(b.findtext("HITS")),
            armor=as_int(b.findtext("ARMOR")), move=0, rmin=rmin, rmax=rmax,
            los=as_int(b.findtext("LOS")), recharge=as_int(b.findtext("RECHARGE")),
            pop=0, domain=0, cat=3, is_plane=False, cost=parse_cost(b.findtext("COST")),
            support=[0] * 6, obj=obj,
        )
        sup = b.findtext("SUPPORT0")
        if sup in RES_WORD:
            caps[t]["support"][RES_WORD[sup]] = as_int(b.findtext("SUPPORTVALUE0"))
        sup = b.findtext("SUPPORT1")
        if sup in RES_WORD:
            caps[t]["support"][RES_WORD[sup]] = as_int(b.findtext("SUPPORTVALUE1"))
        build_name_to_type.setdefault((b.findtext("NAME") or "").strip(), t)

    edges = []
    for i, u in enumerate(units):
        t = unit_base + i
        obj = (u.findtext("OBJ_MASK") or "")
        uflags = (u.findtext("FLAGS") or "")
        dom = (u.findtext("DOMAIN") or "Land").strip().lower()
        attack = as_int(u.findtext("ATTACK"))
        moves = as_int(u.findtext("MOVES"))
        mana = as_int(u.findtext("MANA"))
        flags = F_UNIT
        if moves > 0:
            flags |= F_MOVE
        if attack > 0:
            flags |= F_ATTACK
        if "C" in obj:
            flags |= F_CIVILIAN
        if "r" in uflags or "S" in obj or "B" in obj:
            flags |= F_SIEGE
        if "l" in uflags:
            flags |= F_GARR_TOWN
        if "m" in uflags:
            flags |= F_GARR_FORT
        if mana > 0:
            flags |= F_CASTER
        if dom == "air" or "3" in obj:
            flags |= F_AIR
        if dom == "sea":
            flags |= F_SEA
        if "s" in uflags or "o" in uflags:
            flags |= F_STEALTH
        if "Z" in obj:
            flags |= F_DETECT
        if "6" in obj:
            flags |= F_ANTIAIR
        if (u.findtext("CAT") or "").strip().lower() == "transport":
            flags |= F_TRANSPORT
        rmin, rmax = parse_range(u.findtext("RANGE"))
        caps[t] = dict(
            flags=flags, attack=attack, hits=as_int(u.findtext("HITS")),
            armor=as_int(u.findtext("ARMOR")), move=moves, rmin=rmin, rmax=rmax,
            los=as_int(u.findtext("LOS")), recharge=as_int(u.findtext("RECHARGE")),
            pop=as_int(u.findtext("POP")), domain={"land": 0, "sea": 1, "air": 2}.get(dom, 0),
            is_plane=(dom == "air" and "f" not in uflags),
            cat=1 if t < build_base - 12 else 2,
            cost=parse_cost(u.findtext("COST")), support=parse_cost(u.findtext("SUPPORT")),
            obj=obj,
        )
        where = (u.findtext("WHERE") or "").strip()
        if where in build_name_to_type:
            edges.append((build_name_to_type[where], t))
            caps[build_name_to_type[where]]["flags"] |= F_PRODUCER

    zero = dict(flags=0, attack=0, hits=0, armor=0, move=0, rmin=0, rmax=0, los=0,
                recharge=0, pop=0, domain=0, cat=0, is_plane=False,
                cost=[0] * 6, support=[0] * 6, obj="")
    for t in range(num_types):
        if caps[t] is None:
            caps[t] = dict(zero, cat=0 if t < unit_base else 4)

    # `rmin`/`rmax` are i32: the two nuclear missiles ship RANGE 0-99999 and would
    # silently wrap in i16. Every other numeric field fits comfortably.
    rec = struct.Struct("<HhihhiihhBBBB6i6i")
    assert rec.size == 76, rec.size
    # v2 consumes the byte at record offset 27 for UnitData::is_plane. Rejecting v1 is
    # important: treating every old zero-filled record as a helicopter would silently
    # route fighter patrols to GROUP_PATROL.
    blob = bytearray(b"DONTYPC2")
    blob += struct.pack("<II", num_types, len(edges))
    for c in caps:
        blob += rec.pack(c["flags"], c["attack"], c["hits"], c["armor"], c["move"],
                         c["rmin"], c["rmax"], c["los"], c["recharge"],
                         min(c["pop"], 255), c["domain"], c["cat"], int(c["is_plane"]),
                         *c["cost"], *c["support"])
    for a, b_ in sorted(edges):
        blob += struct.pack("<HH", a, b_)

    dst = os.path.join(ROOT, "schema/live/env-typecaps.bin")
    with open(dst, "wb") as f:
        f.write(blob)
    print(f"wrote {dst}  ({num_types} type records, {len(edges)} producer edges, "
          f"{len(blob)} bytes)")


if __name__ == "__main__":
    main()
