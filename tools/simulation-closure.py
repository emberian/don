#!/usr/bin/env python3
"""Generate and gate the whole-simulation closure inventory."""

from __future__ import annotations

import argparse
import csv
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUT = ROOT / "schema" / "simulation-closure.json"

# (slug, description, status, note).
#
# `status` is one of "required" (nothing of the stage executes yet) or "partial" (the stage
# has a real executable path end to end, with the remaining blockers named in `note`).
# `complete` is computed below and is deliberately NOT settable here: a global stage only
# goes complete once every domain row it depends on is complete, which no stage is.
GLOBAL_STAGES = [
    ("initial_world", "deterministic map, starts, players, nations, teams, diplomacy",
     "required", ""),
    ("save_load", "complete save/load and resumed deterministic execution", "required", ""),
    ("scenario_runtime", "BHS/scenario execution wired into the retail tick", "required", ""),
    ("victory_endgame", "game modes, victory, defeat, scoring, and end-game transition",
     "partial",
     "A match reaches a real end state through the real path: decoded opcode 70/71 -> "
     "Player::resign/quit 0x006EDCB0/0x006EDC00 -> Player::leave_game 0x006EE010 -> "
     "Leader::defeat 0x006ECB00 -> Game::check_victory 0x005926B0 -> do_frame step 27 "
     "Game::process_end_game. Host: tick::lifecycle_host (Sim::tail_command_facts / "
     "Sim::apply_tail_command_transaction); evidence: crates/don-sim/tests/"
     "victory_endgame_wire.rs, docs/mechanics/victory-endgame-wire.md. Still red: "
     "tick 11 children Leader::plan_strategy and Leader::diplomacy are call-counted gaps; "
     "score inputs (num_units/num_buildings/territory/encrypted economy) are not all live; "
     "drop states 1 and 2 need command row 38 Leader::action_declare 0x006DAB50; the "
     "capital-elimination ending stops at LeaderData::find_capital 0x006EB930; "
     "Game::process_end_game's statistics/leaderboard/menu tail is a product boundary."),
    ("multiplayer_match", "owned-client setup, launch, lockstep turns, checksums, drop/rejoin",
     "required", ""),
    ("frontend_game_flow", "setup through completed match without retail UI", "required", ""),
    ("rl_complete_dynamics", "honest actions over complete shared game dynamics", "required", ""),
    ("ai_evaluation", "non-cheating full-game opponents and evaluation matrix", "required", ""),
    ("content_release", "legal assets/content path, installer, packaging, release audit",
     "required", ""),
]


def static_rows() -> list[list[str]]:
    run = subprocess.run(
        ["cargo", "run", "-q", "-p", "don-replay", "--bin", "don-closure"],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if run.returncode:
        raise RuntimeError(run.stderr.strip() or "don-closure failed")
    rows = list(csv.reader(run.stdout.splitlines(), delimiter="\t"))
    if not rows or rows[0] != ["META", "don.simulation-closure.static.v1"]:
        raise RuntimeError("don-closure protocol mismatch")
    return rows[1:]


def build(rows: list[list[str]], replay: dict) -> dict:
    domains: dict[str, list[dict]] = {
        "tick": [],
        "orders": [],
        "group_actions": [],
        "opcodes": [],
        "checksums": [],
        "product_blockers": [],
        "global_stages": [],
    }
    counts_row = None
    replay_channels = replay["totals"]["per_channel"]
    for row in rows:
        kind, f = row[0], row[1:]
        if kind == "TICK":
            status = f[4]
            domains["tick"].append({
                "id": int(f[0]), "name": f[1], "retail_va": f[2] or None,
                "source": f[3], "status": status, "note": f[5],
                "complete": status in {"implemented", "out_of_scope"},
            })
        elif kind == "ORDER":
            status = f[4]
            domains["orders"].append({
                "id": int(f[0]), "name": f[1], "retail_va": f[2] or None,
                "symbol": f[3], "status": status, "note": f[5],
                "complete": status in {"implemented", "faithfully_empty"},
            })
        elif kind == "ACTION":
            domains["group_actions"].append({
                "name": f[0], "retail_va": f[1], "retail_size": int(f[2]),
                "call_sites": int(f[3]), "installs": [x for x in f[4].split(",") if x],
                "delegates": [x for x in f[5].split(",") if x], "status": f[6],
                "complete": f[6] == "complete",
            })
        elif kind == "OPCODE":
            complete = f[7] == "presentation" or f[8] == "complete"
            domains["opcodes"].append({
                "op": int(f[0]), "name": f[1], "method": f[2], "retail_va": f[3],
                "receiver": f[4], "action": f[5] or None, "wire_bytes": f[6],
                "class": f[7], "bridge_status": f[8], "complete": complete,
            })
        elif kind == "CHECKSUM":
            stats = replay_channels[f[1]]
            substantive_full = (
                stats["compares"] > 0
                and stats["matches"] == stats["compares"]
                and stats["nontrivial_compares"] == stats["compares"]
            )
            domains["checksums"].append({
                "id": int(f[0]), "name": f[1], "walker": f[2],
                "element_class": f[3] or None, "producer": f[4],
                "compares": stats["compares"], "matches": stats["matches"],
                "nontrivial_compares": stats["nontrivial_compares"],
                "best_survived_turns": stats["best_survived_turns"],
                "complete": f[4] != "absent" and substantive_full,
            })
        elif kind == "BLOCKER":
            domains["product_blockers"].append({
                "slug": f[0], "title": f[1], "status": f[2], "seam": f[3] or None,
                "retail_evidence": f[4], "complete": False,
            })
        elif kind == "COUNTS":
            counts_row = [int(x) for x in f]
        else:
            raise RuntimeError(f"unknown don-closure row {kind!r}")

    for slug, description, status, note in GLOBAL_STAGES:
        row = {"slug": slug, "description": description, "status": status, "complete": False}
        if note:
            row["note"] = note
        domains["global_stages"].append(row)

    expected = [29, 28, 42, 82, 15]
    actual = [len(domains[x]) for x in ("tick", "orders", "group_actions", "opcodes", "checksums")]
    if actual != expected or counts_row is None or counts_row[:5] != expected:
        raise RuntimeError(f"inventory cardinality drift: actual={actual} protocol={counts_row}")
    if counts_row[5] != len(domains["product_blockers"]):
        raise RuntimeError("product blocker inventory mismatch")

    summary = {}
    for name, entries in domains.items():
        complete = sum(bool(x["complete"]) for x in entries)
        summary[name] = {"total": len(entries), "complete": complete, "red": len(entries) - complete}
    red = sum(x["red"] for x in summary.values())
    return {
        "schema": "don.simulation-closure.v1",
        "generated_by": "tools/simulation-closure.py",
        "ground_truth": "compiled retail-addressed inventories plus schema/replay-validation.json",
        "definition": "complete means recovered, implemented, wired into the real path, and evidenced; partial ports remain red",
        "ready": red == 0,
        "red": red,
        "summary": summary,
        "domains": domains,
    }


def render(report: dict) -> None:
    print(f"simulation closure: {'READY' if report['ready'] else 'RED'} ({report['red']} rows remain)")
    for name, x in report["summary"].items():
        print(f"  {name:<18} {x['complete']:>3}/{x['total']:<3} complete   {x['red']:>3} red")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--write", nargs="?", const=str(DEFAULT_OUT), metavar="PATH")
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--status", action="store_true")
    ns = ap.parse_args()
    if ns.status:
        report = json.loads(DEFAULT_OUT.read_text())
    else:
        replay = json.loads((ROOT / "schema" / "replay-validation.json").read_text())
        report = build(static_rows(), replay)
        if ns.write:
            path = Path(ns.write)
            path.write_text(json.dumps(report, indent=2) + "\n")
    render(report)
    return 3 if ns.check and not report["ready"] else 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, KeyError, ValueError, RuntimeError) as exc:
        print(f"simulation-closure: {exc}", file=sys.stderr)
        raise SystemExit(2)
