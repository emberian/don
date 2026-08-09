#!/usr/bin/env python3
"""Regenerate schema/coverage.json — the whole-game implementation ledger.

    cd /Users/ember/dev/don/ron-bin
    uv run --quiet --with capstone --with pefile python ../tools/coverage-ledger.py

Writes `schema/coverage.json` and prints the headline. Everything it reports is
computed here from the binary, the PDB and the Rust tree; nothing is typed in by
hand except the four small tables at the bottom of this file (the 15 channel
walkers, the 29 do_frame steps, the 28 do_job arms, the lane->module map), each
of which is independently re-derivable and says how.

The measurements, and what each does NOT mean:

  SEEDED     Direct-call closure over .text from known roots plus every
             main/game/ `X::process` / `inc_time` / `work` / `walk_data` /
             `do_*` method, used as stand-ins for unresolved indirect calls.
             This is a potential-reachability approximation with both false
             positives and false negatives, not a tick call graph or bound.

  CITED      Every `0x00xxxxxx` literal in crates/**/*.rs, resolved to the PDB
             procedure containing it. This is DECLARED derivation: a claim that
             the surrounding Rust came from that retail function. It is an
             UPPER bound on fidelity — a citation is not a proof, and only 12
             differential cases exist (schema/oracle-regression.json).

  TESTED     Retail functions covered by the differential oracle registry. This
             is a floor: it does not include every useful local or fixture test.

The output also records textual `systems::` references and a manually curated
source partition. Neither is a call graph and neither measures tick execution.
"""
import bisect
import collections
import glob
import json
import os
import re
import sys

DON = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(DON, "schema", "coverage.json")

# --------------------------------------------------------------------------
# 1. procedures
# --------------------------------------------------------------------------
sym = json.load(open(f"{DON}/schema/symbols.json"))
procs = {}
for f in sym["functions"]:
    if f.get("kind") == "public":
        continue
    va = int(f["va"], 16)
    sz = f.get("size") or 0
    if sz <= 0:
        continue
    if va not in procs or sz > procs[va]["size"]:
        procs[va] = {"size": sz, "name": f.get("name") or "",
                     "file": (f.get("file") or "").replace("\\", "/").lower()}
starts = sorted(procs)
GAME = re.compile(r"/main/game/")


def resolve(va):
    if va in procs:
        return va
    i = bisect.bisect_right(starts, va) - 1
    if i >= 0 and starts[i] <= va < starts[i] + procs[starts[i]]["size"]:
        return starts[i]
    return None


def gamefile(p):
    return procs[p]["file"].split("/main/")[-1]


# --------------------------------------------------------------------------
# 2. seeded potential-reachability closure
# --------------------------------------------------------------------------
import pefile                                             # noqa: E402
from capstone import Cs, CS_ARCH_X86, CS_MODE_32          # noqa: E402

pe = pefile.PE(f"{DON}/ron-bin/riseofnations.exe")
txt = [s for s in pe.sections if s.Name.rstrip(b"\x00") == b".text"][0]
base = pe.OPTIONAL_HEADER.ImageBase + txt.VirtualAddress
data = txt.get_data()
end = base + len(data)
md = Cs(CS_ARCH_X86, CS_MODE_32)

edges = collections.defaultdict(set)
for va, p in procs.items():
    off = va - base
    if off < 0 or off + p["size"] > len(data):
        continue
    for i in md.disasm(data[off:off + p["size"]], va):
        if i.mnemonic in ("call", "jmp") and i.op_str.startswith("0x"):
            t = int(i.op_str, 16)
            if base <= t < end and t != va:
                edges[va].add(t)

VIRT = re.compile(r"::(process|process_all|inc_time|work|walk_data|do_[a-z_]+)$")
SEEDS = {
    0x00591EF0,  # Game::do_frame
    0x0093EF10,  # CommandManager::process_turn
    0x00936560,  # CheckSums::check_all
    0x0065DCE0,  # Objects::process_all
    0x0094A700,  # CommandPackage::process
}
seeds = set(SEEDS)
for va, p in procs.items():
    if GAME.search(p["file"]) and VIRT.search(p["name"]):
        seeds.add(va)

seen, stack = set(), list(seeds)
while stack:
    v = stack.pop()
    if v in seen:
        continue
    seen.add(v)
    stack.extend(t for t in edges.get(v, ()) if t not in seen)

reach = {v for v in seen if v in procs and GAME.search(procs[v]["file"])}
reach_funcs = len(reach)
reach_bytes = sum(procs[v]["size"] for v in reach)

# --------------------------------------------------------------------------
# 3. citations
# --------------------------------------------------------------------------
VA = re.compile(r"0x00([0-9a-fA-F]{6})\b")
cited = {}
lits = collections.Counter()
for path in glob.glob(f"{DON}/crates/**/*.rs", recursive=True):
    crate = path.split("/crates/")[1].split("/")[0]
    for m in VA.finditer(open(path, errors="ignore").read()):
        v = int("0x00" + m.group(1), 16)
        if v < 0x00401000:
            continue
        lits[crate] += 1
        r = resolve(v)
        if r is not None:
            cited.setdefault(r, set()).add(crate)

cited_game = {p for p in cited if GAME.search(procs[p]["file"])}
cited_reach = cited_game & reach
cited_bytes = sum(procs[p]["size"] for p in cited_reach)

# --------------------------------------------------------------------------
# 4. textual cross-module references (diagnostic only; not execution evidence)
# --------------------------------------------------------------------------
external_system_references = []
for path in glob.glob(f"{DON}/crates/**/*.rs", recursive=True):
    if "/don-sim/src/systems/" in path:
        continue
    for n, line in enumerate(open(path, errors="ignore"), 1):
        if "systems::" in line and not line.lstrip().startswith("//"):
            external_system_references.append({"file": path[len(DON) + 1:], "line": n,
                                               "text": line.strip()[:120]})

# A source inventory, not a reachability result. This allowlist identifies the
# small runtime core known to be used by current frontends; files under systems/
# and support/generated files remain in `other`. Update it deliberately when a
# real runtime call path is reviewed. Do not rename these buckets to wired/unwired.
RUNTIME_CORE = {"mechanics.rs", "world.rs", "simd.rs", "objects.rs", "balance.rs",
                "rng.rs", "lib.rs", "batch.rs"}
source_partition = {"runtime_core_allowlist": {"lines": 0, "procs": set(), "files": []},
                    "other": {"lines": 0, "procs": set(), "files": []}}
for path in sorted(glob.glob(f"{DON}/crates/don-sim/src/**/*.rs", recursive=True)):
    b = os.path.basename(path)
    k = "runtime_core_allowlist" if (b in RUNTIME_CORE and "/systems/" not in path) else "other"
    txt = open(path, errors="ignore").read()
    vs = {resolve(int("0x00" + m.group(1), 16)) for m in VA.finditer(txt)}
    vs = {v for v in vs if v}
    source_partition[k]["lines"] += txt.count("\n")
    source_partition[k]["procs"] |= vs
    source_partition[k]["files"].append(path[len(DON) + 1:])
for k in source_partition:
    source_partition[k]["retail_bytes"] = sum(
        procs[v]["size"] for v in source_partition[k]["procs"])
    source_partition[k]["procs"] = len(source_partition[k]["procs"])

# --------------------------------------------------------------------------
# 5. per-file classification of main/game/
# --------------------------------------------------------------------------
EDITOR = re.compile(r"(editor|mapedit)", re.I)
CAMPAIGN = re.compile(r"(conquest|campaign|tutorial|scenario)", re.I)
NETCODE = re.compile(r"(gamespy|lobby|netdaemon|netsys|network|steam|dropcontrol|"
                     r"timesync|packagefifo|matchmak|crossplay|party|voice|socket)", re.I)
PRESENT = re.compile(r"(out\.cpp$|win\.cpp$|win\.h$|iface|render|draw|anim|sound|music|"
                     r"movie|font|texture|shader|^(graphicpieces|borders|worldmap|scene|"
                     r"particle|surf|cursor|minimap|nuke|tileset|terrain\w*)\.cpp$)", re.I)
UI_SUFFIX = re.compile(r"(Win|Screen|Dlg|Button|Menu|Bar|Box|Panel|Slider|List|Tab)$")


def klass(name):
    return name.split("::")[0].split("<")[0] if "::" in name else None


def suffix_bucket(name):
    c = klass(name)
    if c is None:
        return "free"
    if c.endswith("Out") or UI_SUFFIX.search(c):
        return "presentation"
    if c.endswith("Data"):
        return "sim"
    if c.endswith("Type"):
        return "rules"
    return "behaviour"


files = collections.defaultdict(lambda: {
    "funcs": 0, "bytes": 0, "walkers": 0,
    "sb": collections.Counter(), "sf": collections.Counter(),
    "reach_funcs": 0, "reach_bytes": 0, "cited_funcs": 0, "cited_bytes": 0})

for va, p in procs.items():
    if not GAME.search(p["file"]):
        continue
    fn = gamefile(va)
    e = files[fn]
    e["funcs"] += 1
    e["bytes"] += p["size"]
    b = suffix_bucket(p["name"])
    e["sb"][b] += p["size"]
    e["sf"][b] += 1
    if p["name"].endswith("::walk_data"):
        e["walkers"] += 1
    if va in reach:
        e["reach_funcs"] += 1
        e["reach_bytes"] += p["size"]
    if va in cited_reach:
        e["cited_funcs"] += 1
        e["cited_bytes"] += p["size"]

file_rows = []
for fn, e in files.items():
    b = fn.split("/")[-1]
    if EDITOR.search(b):
        label = "editor"
    elif NETCODE.search(b):
        label = "netcode"
    elif CAMPAIGN.search(b):
        label = "campaign"
    elif PRESENT.search(b):
        label = "presentation"
    else:
        sim_b = e["sb"]["sim"] + e["sb"]["rules"] + e["sb"]["behaviour"]
        label = "sim" if sim_b >= e["sb"]["presentation"] else "presentation"
    if e["walkers"] > 0 and label == "presentation":
        label = "sim"          # the engine's own answer outranks the heuristic
    file_rows.append({
        "file": fn, "label": label, "funcs": e["funcs"], "bytes": e["bytes"],
        "walk_data": e["walkers"],
        "reach_funcs": e["reach_funcs"], "reach_bytes": e["reach_bytes"],
        "cited_funcs": e["cited_funcs"], "cited_bytes": e["cited_bytes"],
        "suffix_bytes": dict(e["sb"]),
    })
file_rows.sort(key=lambda r: -r["bytes"])

by_label = collections.defaultdict(lambda: collections.Counter())
for r in file_rows:
    L = by_label[r["label"]]
    L["files"] += 1
    for k in ("funcs", "bytes", "reach_funcs", "reach_bytes", "cited_funcs", "cited_bytes"):
        L[k] += r[k]

# --------------------------------------------------------------------------
# 6. hand tables (each re-derivable; the derivation is in the comment)
# --------------------------------------------------------------------------
# Re-derive: disassemble CheckSums::check_all 0x00936560 and take the direct
# calls in address order, dropping SyncLogger::logToMemory.  The per-channel
# accumulator is the CheckSum object field at +0x10, reset to 1 before each.
CHANNELS = [
    (1,  "units",      "0x009371D0", "CheckSums::check_units",     "Unit::walk_data 0x0060CF40",       "movement,groups_guys,combat,air,naval", "partial"),
    (2,  "builds",     "0x00937290", "CheckSums::check_builds",    "BuildData::walk_data 0x0062F270",  "production",           "partial"),
    (3,  "walls",      "0x00937360", "CheckSums::check_walls",     "WallData::walk_data 0x00642510",   "walls,production",     "partial"),
    (4,  "ammo",       "0x009374E0", "CheckSums::check_ammo",      "AmmoData::walk_data 0x0067AB50",   "ammo,air",             "partial"),
    (5,  "deaths",     "0x00936BB0", "CheckSums::check_deaths",    "DeathObjData (inline)",            "combat",               "partial"),
    (6,  "groups",     "0x00937530", "CheckSums::check_groups",    "Group::walk_data 0x00708400",      "groups_guys",          "partial"),
    (7,  "guys",       "0x00937430", "CheckSums::check_guys",      "GuyData::walk_data 0x005E0210",    "groups_guys",          "partial"),
    (8,  "leaders",    "inline",     "LeaderData::walk_data",      "LeaderData::walk_data 0x006D6750", "economy,tech_cities,victory_score", "partial"),
    (9,  "cities",     "0x00937600", "CheckSums::check_cities",    "City::walk_data 0x00489220",       "tech_cities",          "partial"),
    (10, "items",      "0x00937790", "CheckSums::check_items",     "Item::walk_data 0x00677150",       "items",                "partial"),
    (11, "goods",      "0x00937710", "CheckSums::check_goods",     "Good::walk_data 0x0066E5D0",       "economy",              "partial"),
    (12, "world",      "inline",     "World::walk_data",           "World::walk_data 0x006B5CF0",      "borders_fog,map_terrain", "partial"),
    (13, "rules",      "inline",     "Game::walk_rules_data",      "Types + Balance walk_rules_data",  "rules_channel",          "partial"),
    (14, "scenario",   "inline",     "ScenarioData::walk_data",    "ScenarioData::walk_data 0x00997AD0", "scenario_channel",     "partial"),
    (15, "script",     "inline",     "RunTimeEnv::walk_data",      "RunTimeEnv::walk_data 0x009C41A0", "script_channel",       "partial"),
]

# Re-derive: `python tools/pdb/calls.py 00591ef0` (architecture.md §3.3).
# The retail order is measured. Rust status is a hand-maintained inventory:
# `module_exists` means a systems module claims the step; it says nothing about
# whether a runnable Rust tick calls that module.
DO_FRAME = [
    (0,  "AutoSave::restore",                  "0x005A20C0", "out_of_scope"),
    (1,  "GameLog::begin_frame",               "0x00932A70", "out_of_scope"),
    (2,  "Random::get (artificial lag)",       "0x00A39D70", "out_of_scope"),
    (3,  "CommandManager::issue_player_speed", "0x00943100", "out_of_scope"),
    (4,  "RunTimeEnv::run_script",             "0x0043D0E0", "module_exists"),
    (5,  "ConquestGame::place_reinforcements", "0x00798880", "out_of_scope"),
    (6,  "TutorialPromptWin::exec",            "0x007C2810", "out_of_scope"),
    (7,  "SteamLeaderboards::UploadScore",     "0x00A36190", "out_of_scope"),
    (8,  "Leaders::process_all",               "0x006ED2A0", "module_exists"),
    (9,  "NetDaemon::process_all",             "0x00951300", "out_of_scope"),
    (10, "AI diplomacy chat",                  None,         "out_of_scope"),
    (11, "Leaders::strategy_all",              "0x006ED430", "absent"),
    (12, "GameDaemon::process_all",            "0x00732700", "module_exists"),
    (13, "Armies::process_all",                "0x006F3B00", "absent"),
    (14, "Objects::process_all",               "0x0065DCE0", "implemented"),
    (15, "Objects::inc_time",                  "0x0065DB70", "module_exists"),
    (16, "GraphicEvents::process",             "0x008E50A0", "out_of_scope"),
    (17, "Leaders::end_process_all",           "0x006ED070", "absent"),
    (18, "Achieve::capture_data",              "0x007AF980", "out_of_scope"),
    (19, "Leader::process_event_frame",        "0x006EC180", "absent"),
    (20, "Game::frame++",                      "0x005924BF", "implemented"),
    (21, "OrdersMemManager::cycle",            "0x00730E20", "out_of_scope"),
    (22, "Roads::scan_and_kill_stray_roads",   "0x008956A0", "absent"),
    (23, "frame % 15 -> Game::seconds++",      "0x005924CF", "implemented"),
    (24, "TurnControl::check_cannon_time",     "0x009579E0", "module_exists"),
    (25, "SaveGame / LoadGame",                "0x005A8220", "out_of_scope"),
    (26, "GameLog::end_frame",                 "0x009329D0", "out_of_scope"),
    (27, "Game::process_end_game",             "0x00591CE0", "module_exists"),
    (28, "Scene::process_capture_sequence",    "0x008C13C0", "out_of_scope"),
]

# Re-derive: the jump table at 0x00617B94, 28 entries, indexed by OrderIndex.
DO_JOB = [
    (0,  "NONE",              None,         "virtual [unit+0x184] do_idle",  "implemented"),
    (1,  "MOVE_TO",           "0x005F7B30", "Unit::do_move",                 "implemented"),
    (2,  "ATTACK_TO",         "0x005F2320", "Unit::do_attack_to",            "absent"),
    (3,  "EXPLORE_TO",        "0x005F24A0", "Unit::do_explore_to",           "absent"),
    (4,  "FLEE_TO",           "0x005F7B30", "Unit::do_move (SAME ARM)",      "implemented"),
    (5,  "PATROL",            None,         "(default arm, does nothing)",   "faithfully_empty"),
    (6,  "BUILD_AT",          "0x005EEBF0", "Unit::do_build",                "absent"),
    (7,  "GATHER",            "0x005EF2A0", "Unit::do_gather",               "absent"),
    (8,  "BOARD_SHIP",        "0x005ED1F0", "Unit::do_board",                "absent"),
    (9,  "AWAIT_BOARD",       "0x005ED040", "Unit::do_await_board",          "absent"),
    (10, "ATTACK",            "0x005F1B80", "Unit::do_attack",               "partial"),
    (11, "FOLLOW",            "0x005E65D0", "Unit::do_follow",               "absent"),
    (12, "GUARD",             "0x005E5C70", "Unit::do_guard",                "absent"),
    (13, "REPAIR",            "0x005EE420", "Unit::do_repair",               "absent"),
    (14, "CAST_SPELL",        "0x005EBFE0", "Unit::do_cast",                 "absent"),
    (15, "TRADE_ROUTE",       "0x005ED270", "Unit::do_trade",                "absent"),
    (16, "STRAFE",            "0x005EAB00", "Unit::do_strafe",               "absent"),
    (17, "AIR_PATROL",        "0x005EA620", "Unit::do_air_patrol",           "absent"),
    (18, "CHANGE_FORM",       "0x005E8670", "Unit::do_form_change",          "absent"),
    (19, "GROUP_MOVE",        "0x005E79A0", "Unit::do_group_move",           "absent"),
    (20, "GROUP_ATTACK",      "0x005E75A0", "Unit::do_group_attack",         "absent"),
    (21, "GROUP_ATTACK_TO",   "0x005E74E0", "Unit::do_group_attack_to",      "absent"),
    (22, "GROUP_PATROL",      "0x005F1910", "Unit::do_patrol",               "absent"),
    (23, "ATTACK_GROUND",     "0x005F1410", "Unit::do_attack_ground",        "absent"),
    (24, "AIR_ATTACK_GROUND", "0x005EA420", "Unit::do_air_attack_ground",    "absent"),
    (25, "SPECIAL_ANIM",      "0x005E5880", "Unit::do_spec_anim",            "absent"),
    (26, "GARRISON",          "0x005E6B80", "Unit::do_garrison",             "absent"),
    (27, "THINK",             "0x005E5BF0", "Unit::do_think_order",          "absent"),
]

# --------------------------------------------------------------------------
# 7. biggest uncited reachable functions
# --------------------------------------------------------------------------
gaps = sorted(((procs[p]["size"], procs[p]["name"], gamefile(p), hex(p))
               for p in reach - cited_reach), reverse=True)[:60]

# --------------------------------------------------------------------------
# 8. emit
# --------------------------------------------------------------------------
oracle = json.load(open(f"{DON}/schema/oracle-regression.json"))

doc = {
    "schema": "don/coverage",
    "schema_version": 2,
    "generator": "tools/coverage-ledger.py",
    "binary": "riseofnations.exe sha256 30478a44..625079, PE32 i386, base 0x00400000",
    "method": {
        "seeded_potential_reachability": "direct call/jmp rel32 closure over .text from Game::do_frame, "
                     "CommandManager::process_turn, CheckSums::check_all, "
                     "Objects::process_all, CommandPackage::process, plus every "
                     "main/game/ X::process|inc_time|work|walk_data|do_* as a virtual "
                     "entry seed. Indirect calls remain invisible and broad virtual seeds "
                     "can add false positives: an approximation, NOT a tick call graph or bound.",
        "cited": "every 0x00xxxxxx literal in crates/**/*.rs resolved to its PDB "
                 "procedure. DECLARED derivation, an UPPER bound on fidelity.",
        "external_system_references": "textual grep for `systems::` outside "
                                      "crates/don-sim/src/systems/; diagnostic only, "
                                      "NOT tick reachability or execution evidence.",
        "source_partition": "manually curated runtime-core allowlist versus other "
                            "don-sim sources; an inventory, NOT a call graph.",
        "tier_b_floor": "schema/oracle-regression.json: differentially tested cases.",
    },
    "headline": {
        "reachable_funcs": reach_funcs,
        "reachable_bytes": reach_bytes,
        "cited_funcs": len(cited_reach),
        "cited_bytes": cited_bytes,
        "pct_funcs": round(100 * len(cited_reach) / reach_funcs, 2),
        "pct_bytes": round(100 * cited_bytes / reach_bytes, 2),
        "external_system_reference_count": len(external_system_references),
        "differential_cases": oracle["summary"]["registered"],
        "differential_trials": oracle["summary"]["total_trials"],
        "differential_game_funcs": len({resolve(int(c["va"], 16)) for c in oracle["cases"]
                                        if resolve(int(c["va"], 16)) is not None
                                        and GAME.search(procs[resolve(int(c["va"], 16))]["file"])}),
        "differential_game_bytes": sum(procs[p]["size"] for p in
                                       {resolve(int(c["va"], 16)) for c in oracle["cases"]
                                        if resolve(int(c["va"], 16)) is not None
                                        and GAME.search(procs[resolve(int(c["va"], 16))]["file"])}),
    },
    "don_sim_source_partition": source_partition,
    "channels": [dict(zip(
        ("idx", "name", "checker_va", "checker", "walker", "rust_modules", "status"), c))
        for c in CHANNELS],
    "do_frame": [dict(zip(("idx", "name", "va", "status"), s)) for s in DO_FRAME],
    "do_job": [dict(zip(("idx", "order", "va", "executor", "status"), a)) for a in DO_JOB],
    "game_files": {
        "total_files": len(file_rows),
        "total_funcs": sum(r["funcs"] for r in file_rows),
        "total_bytes": sum(r["bytes"] for r in file_rows),
        "by_label": {k: dict(v) for k, v in by_label.items()},
        "files": file_rows,
    },
    "citations_per_crate": {c: n for c, n in lits.most_common()},
    "external_system_references": external_system_references,
    "largest_uncited_reachable": [
        {"bytes": s, "name": n, "file": f, "va": v} for s, n, f, v in gaps],
}

json.dump(doc, open(OUT, "w"), indent=1)

h = doc["headline"]
print(f"seeded main/game/ closure : {h['reachable_funcs']} funcs, {h['reachable_bytes']} bytes")
print(f"cited by the Rust        : {h['cited_funcs']} funcs ({h['pct_funcs']}%), "
      f"{h['cited_bytes']} bytes ({h['pct_bytes']}%)")
print(f"external systems:: refs  : {h['external_system_reference_count']} (not call evidence)")
print(f"differentially tested    : {h['differential_cases']} cases, "
      f"{h['differential_trials']:,} trials")
for k in ("sim", "presentation", "campaign", "editor", "netcode"):
    v = by_label[k]
    print(f"  {k:13s} {v['files']:4d} files {v['funcs']:6d} funcs {v['bytes']:8d} B "
          f"reach {v['reach_bytes']:8d} B cited {v['cited_bytes']:7d} B")
print("wrote", OUT)
