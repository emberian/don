#!/usr/bin/env python3
"""Decision-weighted human opening envelope for the independent DoN AI.

The `.rcx` corpus is used only for human production decisions.  Retail AI decisions do
not enter lockstep and therefore do not exist in the recordings.  Candidate traces come
from `ai_opening_trace`, which records accepted production commands from our observation-
only `Ai` and from the shipped `economic.bhs` cases 6..18 purchase order while both run in
the same explicitly non-retail-fidelity Arena model.

This report is diagnostic.  It emits neither Elo nor a win rate.  Its main useful number
is decision weight: how much of recorded human production belongs to an economic family
that the independent AI never exercises in the bounded trace.

Usage:
    python3 analysis/ai/opening_envelope.py
    python3 analysis/ai/opening_envelope.py --trace /tmp/ai-opening.tsv --pretty
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
ANALYSIS = ROOT / "analysis"
FPS = 15
DEFAULT_MINUTES = 12

ECONOMIC_FAMILIES = {
    "basic_labor": {
        "Citizen",
        "Farm",
        "Fishermen",
        "Woodcutter's Camp",
        "Mine",
    },
    "knowledge_economy": {"University", "Scholar"},
    "wealth_economy": {"Market", "Caravan", "Merchant"},
    "gather_upgrades": {"Granary", "Lumber Mill", "Smelter"},
    "city_expansion": {"Small City"},
}

CONFOUNDS = [
    {
        "id": "no_retail_ai_commands_in_rcx",
        "effect": "The replay envelope describes human production only; it cannot recover or grade retail AI decisions.",
    },
    {
        "id": "replay_setup_not_stratified",
        "effect": "Starting age/resources, game speed, nation, map, and victory options are not decoded into this table; timing ranges are bounds, not a standard-start target.",
    },
    {
        "id": "arena_physics_under_test",
        "effect": "Arena travel, ordinary gather capacity/income, construction, combat, and map generation retain declared MODEL paths. Base University/Scholar mechanics and gather-enhancer source rows, city percentages, and integer arithmetic call recovered exact owners inside that larger non-retail Arena; applying an enhancer to generated-map Farm/Camp/Mine gross remains MODEL.",
    },
    {
        "id": "shipped_opening_is_not_full_retail_ai",
        "effect": "ShippedOpening is economic.bhs cases 6..18. Retail's compiled production stages and their later decisions are not represented by that trace.",
    },
    {
        "id": "accepted_issue_time_only",
        "effect": "Candidate rows are accepted issue times, matching replay production commands; completion time and unaffordable retries are intentionally excluded.",
    },
]

PRODUCTION_AI_AUDIT = {
    "source": "crates/don-sim/src/systems/leader_production_ai.rs",
    "control_flow": "Leader::plan_strategy entry/dispatch and the complete 628-byte Leader::production_ai step machine are executable",
    "decision_stage_bodies_owned": 0,
    "reached_unowned_stages": [
        "RunTimeEnv::run_script",
        "Leader::production_ai_setup",
        "MakeList::clear",
        "Leader::found_cities",
        "Leader::research_techs",
        "Leader::upgrade_units",
        "Leader::create_units",
        "Leader::create_buildings",
        "Leader::make_stuff",
    ],
    "interpretation": "Reachability now proves when retail would enter each boundary; it does not yet produce the compiled retail AI's decisions.",
}

KNOWLEDGE_ECONOMY_AUDIT = {
    "scope": "base-level city University -> contained Scholar -> knowledge credit",
    "type_source": "schema/live live UnitType/BuildType rows",
    "exact_owners": [
        "don_sim::systems::production::ramp_cost",
        "don_sim::systems::gathering::MAX_KNOWLEDGE_GATHERERS",
        "don_sim::systems::gathering::site_gross",
        "don_sim::systems::economy::scholar_rate_for_level",
        "don_sim::systems::tech_cities::city_literacy",
        "don_sim::mechanics::resource_tick / credit_resource",
    ],
    "pinned_result": "one city University with seven base Scholars credits 45 knowledge per 450 frames",
    "authority_boundary": "University construction still uses Arena ResearchModel; levels 2..6 need the unhosted BonusType/property resolver; contained-object destruction/come-out teardown is not claimed.",
    "interpretation": "Accepted University/Scholar rows now measure a represented economic feedback loop, not retail-AI imitation or whole-Arena fidelity.",
}

GATHER_UPGRADE_AUDIT = {
    "scope": "first-copy base Granary / Lumber Mill / Smelter policy and city-local enhancer arithmetic, enabled by one completed-Market base tax source",
    "type_source": "validated schema/live BuildType/TechType rows 423/424/425 and 552/553",
    "exact_owners": [
        "don_sim::systems::tech_cities::CityRules enhancer tables",
        "don_sim::systems::tech_cities::calc_gather_enhancers",
        "CityData::enhancer_amount 0x00738360 multiply-then-divide ordering",
        "don_sim::systems::tech_cities::city_taxes over a completed same-city census",
    ],
    "pinned_result": "base completed enhancers yield 120% food, 120% timber, and 150% metal; 11 food gross becomes 13 before accumulation; one completed Market credits exactly 10 wealth per 450 frames",
    "authority_boundary": "Arena construction and ordinary generated-map payout remain MODEL; Market tax retains its exact 7200-point source accumulator but composes with the model economy; Caravan, Merchant and trade remain absent; the nearest-city duplicate gate is a conservative projection of retail's overlapping-city graph; a retained exact Farm keeps its externally supplied enhancer snapshot until explicitly rebound.",
    "interpretation": "Accepted Market and enhancer-building rows remove zero policy surfaces and exercise exact source arithmetic, but do not make Camp/Mine terrain payout, trade, or the whole-Arena economy authoritative.",
}


@dataclass(frozen=True)
class TraceRow:
    policy: str
    seed: str
    seat: int
    frame: int
    kind: str
    name: str
    count: int

    @property
    def run(self) -> tuple[str, str, int]:
        return self.policy, self.seed, self.seat


@dataclass(frozen=True)
class Trace:
    minutes: int
    seeds: int
    fps: int
    rows: list[TraceRow]


def percentile(values: list[int], q: float) -> int:
    if not values:
        return -1
    ordered = sorted(values)
    index = min(len(ordered) - 1, max(0, round(q * (len(ordered) - 1))))
    return ordered[index]


def load_inputs() -> tuple[dict, dict]:
    with (ANALYSIS / "derived.json").open() as f:
        derived = json.load(f)
    with (ANALYSIS / "corpus-commands.json").open() as f:
        corpus = json.load(f)
    return derived, corpus


def human_rows(derived: dict, corpus: dict, horizon: int) -> tuple[list[tuple], dict]:
    names = {int(k): v for k, v in derived["slot_names"].items()}
    human = {
        (file["file"], player["index"]): player["human"]
        for file in corpus["files"]
        for player in file.get("players", [])
        if player.get("present")
    }
    selected = []
    nonhuman_rows = nonhuman_decisions = 0
    for file, stamp, play, kind, type_id, count in corpus["rows"]:
        owner = human.get((file, play))
        if owner is False:
            nonhuman_rows += 1
            nonhuman_decisions += count
        if owner is True and stamp <= horizon:
            selected.append((file, play, stamp, kind, names.get(type_id, f"?{type_id}"), count))
    audit = {
        "present_slots": len(human),
        "human_slots": sum(owner is True for owner in human.values()),
        "nonhuman_slots": sum(owner is False for owner in human.values()),
        "nonhuman_production_rows": nonhuman_rows,
        "nonhuman_production_decisions": nonhuman_decisions,
    }
    return selected, audit


def human_envelope(rows: list[tuple]) -> dict:
    decisions = Counter()
    first_by_game: dict[tuple[str, int, str], int] = {}
    games = set()
    for file, play, stamp, _kind, name, count in rows:
        games.add((file, play))
        decisions[name] += count
        first_by_game.setdefault((file, play, name), stamp)

    firsts: dict[str, list[int]] = defaultdict(list)
    for (_file, _play, name), frame in first_by_game.items():
        firsts[name].append(frame)

    types = {}
    for name, count in sorted(decisions.items(), key=lambda item: (-item[1], item[0])):
        frames = firsts[name]
        types[name] = {
            "decisions": count,
            "player_games": len(frames),
            "first_issue": {
                "min": min(frames),
                "p10": percentile(frames, 0.10),
                "p50": percentile(frames, 0.50),
                "p90": percentile(frames, 0.90),
                "max": max(frames),
            },
        }

    families = {}
    total = sum(decisions.values())
    for family, members in ECONOMIC_FAMILIES.items():
        by_type = {name: decisions[name] for name in sorted(members) if decisions[name]}
        weight = sum(by_type.values())
        families[family] = {
            "decisions": weight,
            "fraction_of_human_production": weight / total if total else 0.0,
            "by_type": by_type,
        }
    return {
        "player_games": len(games),
        "production_decisions": total,
        "types": types,
        "economic_families": families,
    }


def parse_trace(text: str) -> Trace:
    lines = [line for line in text.splitlines() if line.strip()]
    if len(lines) < 3 or lines[0] != "don.ai.accepted-production-trace.v1":
        raise ValueError("trace is not don.ai.accepted-production-trace.v1")
    metadata = lines[1].split("\t")
    if len(metadata) != 7 or [metadata[i] for i in (0, 1, 3, 5)] != [
        "meta",
        "minutes",
        "seeds",
        "fps",
    ]:
        raise ValueError("trace metadata does not match the v1 schema")
    minutes, seeds, fps = map(int, (metadata[2], metadata[4], metadata[6]))
    if not 1 <= minutes <= 30 or not 1 <= seeds <= 32 or fps != FPS:
        raise ValueError("trace metadata is outside the bounded v1 domain")
    if lines[2].split("\t") != ["policy", "seed", "seat", "frame", "kind", "type", "count"]:
        raise ValueError("trace header does not match the v1 schema")
    rows = []
    for number, line in enumerate(lines[3:], 4):
        fields = line.split("\t")
        if len(fields) != 7:
            raise ValueError(f"trace line {number} has {len(fields)} fields")
        policy, seed, seat, frame, kind, name, count = fields
        row = TraceRow(policy, seed, int(seat), int(frame), kind, name, int(count))
        if row.count <= 0 or row.frame < 0:
            raise ValueError(f"trace line {number} has a non-positive count or negative frame")
        rows.append(row)
    return Trace(minutes=minutes, seeds=seeds, fps=fps, rows=rows)


def produce_trace(minutes: int, seeds: int) -> str:
    command = [
        "cargo",
        "run",
        "-q",
        "-p",
        "don-ai",
        "--bin",
        "ai_opening_trace",
        "--",
        "--minutes",
        str(minutes),
        "--seeds",
        str(seeds),
    ]
    result = subprocess.run(command, cwd=ROOT, text=True, capture_output=True, check=False)
    if result.returncode:
        sys.stderr.write(result.stderr)
        raise SystemExit(f"trace command failed with status {result.returncode}")
    return result.stdout


def policy_traces(rows: list[TraceRow], envelope: dict) -> dict:
    by_policy: dict[str, list[TraceRow]] = defaultdict(list)
    for row in rows:
        by_policy[row.policy].append(row)

    reports = {}
    for policy, policy_rows in sorted(by_policy.items()):
        runs = sorted({row.run for row in policy_rows})
        accepted = Counter()
        first: dict[tuple[tuple[str, str, int], str], int] = {}
        for row in policy_rows:
            accepted[row.name] += row.count
            first.setdefault((row.run, row.name), row.frame)

        first_summary = {}
        for name in sorted({row.name for row in policy_rows}):
            frames = [first[(run, name)] for run in runs if (run, name) in first]
            ref = envelope["types"].get(name)
            first_summary[name] = {
                "accepted_decisions": accepted[name],
                "runs_present": len(frames),
                "runs_total": len(runs),
                "candidate_min": min(frames),
                "candidate_p50": percentile(frames, 0.50),
                "candidate_max": max(frames),
                "human_first_issue": ref["first_issue"] if ref else None,
                "human_decision_weight": ref["decisions"] if ref else 0,
            }

        families = {}
        for family, members in ECONOMIC_FAMILIES.items():
            family_count = sum(accepted[name] for name in members)
            human_family = envelope["economic_families"][family]
            present = sorted(name for name in members if accepted[name])
            missing = sorted(name for name in members if human_family["by_type"].get(name) and not accepted[name])
            missing_weight = sum(human_family["by_type"][name] for name in missing)
            families[family] = {
                "accepted_decisions": family_count,
                "accepted_per_run": family_count / len(runs) if runs else 0.0,
                "types_present": present,
                "human_types_missing": missing,
                "missing_human_decision_weight": missing_weight,
            }
        reports[policy] = {
            "runs": len(runs),
            "accepted_production_decisions": sum(accepted.values()),
            "first_issue_trace": first_summary,
            "economic_families": families,
        }
    return reports


def recommend(envelope: dict, policies: dict) -> dict:
    ai = policies.get("Ai")
    if not ai:
        return {"status": "unavailable", "reason": "trace contains no Ai policy rows"}
    candidates = []
    for family, row in ai["economic_families"].items():
        if family in {"basic_labor", "city_expansion"}:
            continue
        if row["accepted_decisions"] == 0 and row["missing_human_decision_weight"]:
            candidates.append((row["missing_human_decision_weight"], family, row))
    if not candidates:
        return {
            "status": "no_zero-coverage_family",
            "reason": "all ranked economic families appear at least once; inspect timing rows rather than inferring a world correction",
        }
    weight, family, row = max(candidates)
    total = envelope["production_decisions"]
    correction = {
        "knowledge_economy": "Add University placement, Scholar production, and their knowledge-income accounting to the independent AI/Arena evaluation path.",
        "wealth_economy": "Add Market/Caravan/Merchant wealth production and their income accounting to the independent AI/Arena evaluation path.",
        "gather_upgrades": "Add gather-upgrade production and its city bonus accounting to the independent AI/Arena evaluation path.",
    }[family]
    return {
        "status": "diagnostic_priority",
        "family": family,
        "correction": correction,
        "missing_human_decisions": weight,
        "fraction_of_human_production": weight / total if total else 0.0,
        "missing_types": row["human_types_missing"],
        "claim_limit": "This ranks a missing economic surface by human decision weight. It is not evidence of retail-AI behaviour, skill, Elo, or causal timing error.",
    }


def build_report(derived: dict, corpus: dict, trace: Trace) -> dict:
    horizon = trace.minutes * 60 * trace.fps
    rows, audit = human_rows(derived, corpus, horizon)
    envelope = human_envelope(rows)
    policies = policy_traces(trace.rows, envelope)
    expected_runs = trace.seeds * 2
    for label in ("Ai", "ShippedOpening"):
        actual = policies.get(label, {}).get("runs", 0)
        if actual != expected_runs:
            raise ValueError(
                f"trace has {actual} {label} runs; v1 metadata requires {expected_runs}"
            )
    return {
        "schema": "don.ai.human-opening-envelope.v1",
        "scope": {
            "minutes": trace.minutes,
            "seeds": trace.seeds,
            "runs_per_policy": expected_runs,
            "fps": trace.fps,
            "candidate_trace": "accepted production issue time in don-ai Arena",
            "candidate_policy_channel": "Bot::act receives arena Obs and every mutation passes through World::submit",
            "difficulty_income_bonus": False,
            "trace_opponent": "inert leader; no bot-vs-bot verdict is computed",
            "reference": "human production commands in shipped .rcx corpus",
            "score_or_elo": False,
        },
        "corpus_audit": audit,
        "shipped_production_ai_audit": PRODUCTION_AI_AUDIT,
        "knowledge_economy_runtime_audit": KNOWLEDGE_ECONOMY_AUDIT,
        "gather_upgrade_runtime_audit": GATHER_UPGRADE_AUDIT,
        "human_envelope": envelope,
        "policy_traces": policies,
        "physics_and_model_confounds": CONFOUNDS,
        "next_model_correction": recommend(envelope, policies),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--trace", type=Path)
    parser.add_argument("--minutes", type=int, default=DEFAULT_MINUTES)
    parser.add_argument("--seeds", type=int, default=3)
    parser.add_argument("--pretty", action="store_true")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if not 1 <= args.minutes <= 30 or not 1 <= args.seeds <= 32:
        parser.error("minutes must be 1..30 and seeds must be 1..32")

    trace_text = args.trace.read_text() if args.trace else produce_trace(args.minutes, args.seeds)
    trace = parse_trace(trace_text)
    if args.trace and args.minutes != trace.minutes:
        parser.error(f"--minutes {args.minutes} does not match trace metadata {trace.minutes}")
    derived, corpus = load_inputs()
    report = build_report(derived, corpus, trace)
    rendered = json.dumps(report, indent=2 if args.pretty else None, sort_keys=True) + "\n"
    if args.output:
        args.output.write_text(rendered)
    else:
        sys.stdout.write(rendered)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
