#!/usr/bin/env python3
"""
derive.py -- collect the quantities the build-order study needs, each tagged with
its provenance, into analysis/derived.json.

Sources, in descending order of authority:

  live/*   schema/live/{unit,building,tech}-attributes.txt
           Tables read out of a RUNNING riseofnations.exe (pid 14644) type objects.
           This is the strongest ground truth we have for per-type numbers: it is
           what the engine actually holds after parsing, not what the XML says.

  rules/   docs/derivation/rules-constants.json
           719 rules.xml constants with struct offset, parse mode and the integer
           the engine stores.  828 of 834 were confirmed against live memory
           (docs/provenance-ledger.md), so treat "stored" as measured.

  xml/     ron-data/*.xml -- used ONLY for names and for the authored prose that
           explains a field.  Never as the source of an implemented number.

Nothing in this file comes from community documentation.

usage:  python3 analysis/derive.py            # writes analysis/derived.json
"""

from __future__ import annotations

import json
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LIVE = os.path.join(ROOT, "schema", "live")
RULES_JSON = os.path.join(ROOT, "docs", "derivation", "rules-constants.json")
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "derived.json")

# Resource slot order.  Confirmed three independent ways, each of which could have
# disagreed:
#   1. STARTING_GOODS entry prose in rules.xml: "200food","200timb","100gd",
#      "100know","100met","100oil"                            (rules-constants.json)
#   2. FUN_00738360 (city gather-bonus selector) maps case 0 -> GRANARY_BONUS byte,
#      case 1 -> LUMBERMILL_BONUS byte, case 4 -> SMELTER_BONUS byte, case 5 -> oil.
#      Granary is food, lumber mill is timber, smelter is metal.
#   3. Every live COST vector agrees with the XML COST string: Farm "4t" ->
#      cost1=4; University "6t/3g" -> cost1=6,cost2=3; Smelter "5g/7t" ->
#      cost1=7,cost2=5.
RES = ["food", "timber", "wealth", "knowledge", "metal", "oil"]
FOOD, TIMBER, WEALTH, KNOWLEDGE, METAL, OIL = range(6)


def load_tsv(path: str) -> list[dict]:
    rows = []
    with open(path) as f:
        hdr = f.readline().rstrip("\n").split("\t")
        for line in f:
            if not line.strip():
                continue
            rows.append(dict(zip(hdr, line.rstrip("\n").split("\t"))))
    return rows


def load_rules() -> dict:
    """name -> list of stored ints (one per array entry)."""
    out = {}
    for e in json.load(open(RULES_JSON)):
        vals = [x["stored"] for x in e["entries"]]
        out[e["name"]] = {
            "offset": e["offset"],
            "count": e["count"],
            "parser": e["parser"],
            "scale": e["scale"],
            "values": vals,
            "xml": [x["xml_value"] for x in e["entries"]],
        }
    return out


def main() -> None:
    rules = load_rules()
    units = load_tsv(os.path.join(LIVE, "unit-attributes.txt"))
    builds = load_tsv(os.path.join(LIVE, "building-attributes.txt"))
    techs = load_tsv(os.path.join(LIVE, "tech-attributes.txt"))

    def i(x):
        return int(x)

    def typerec(r, kind):
        return {
            "kind": kind,
            "slot": i(r["slot"]),
            "name": r["name"],
            "job_time": i(r["job_time"]),
            "cost": [i(r[f"cost{k}"]) for k in range(6)],
            "support": [i(r.get("support0", -1)), i(r.get("support1", -1))],
            "support_cost": [
                i(r.get("support_cost0", 0)),
                i(r.get("support_cost1", 0)),
            ],
            "preq": [i(r["preq0"]), i(r["preq1"]), i(r["preq2"])],
            "where": i(r.get("where", -1)),
            "cat": i(r["cat"]),
        }

    U = {}
    for r in units:
        rec = typerec(r, "unit")
        rec["progression"] = i(r["progression"])
        rec["job_extra_time"] = i(r["job_extra_time"])
        rec["pop"] = i(r["control_cost"])
        # first slot wins: the duplicate rows are per-nation graft variants
        U.setdefault(r["name"], rec)

    B = {}
    for r in builds:
        rec = typerec(r, "building")
        rec["build_flags"] = i(r["build_flags"])
        B.setdefault(r["name"], rec)

    T = {}
    for r in techs:
        rec = typerec(r, "tech")
        rec["age"] = i(r["age"])
        T.setdefault(r["name"], rec)

    slot_names = {}
    for d in (U, B, T):
        for n, rec in d.items():
            slot_names[rec["slot"]] = n

    derived = {
        "_provenance": {
            "live_tables": "schema/live/{unit,building,tech}-attributes.txt "
            "(read from a running riseofnations.exe, pid 14644)",
            "rules": "docs/derivation/rules-constants.json "
            "(719 rules.xml constants; 828/834 confirmed against live memory)",
            "resource_order": RES,
        },
        "res": RES,
        "rules": {
            k: rules[k]
            for k in [
                "gather_rate",
                "peasant_rate",
                "oil_rate",
                "city_gather",
                "basic_gather",
                "starting_goods",
                "commerce_cap",
                "pop_cap",
                "max_pop_limit",
                "farms_per_city_base",
                "granary_bonus",
                "lumbermill_bonus",
                "smelter_bonus",
                "fishermen_bonus",
                "merchants_bonus",
                "refinery_bonus",
                "market_taxes",
                "university_literacy",
                "unit_cost_factor",
                "build_cost_factor",
                "tech_cost_factor",
                "unit_scholar_ramp_max",
                "unit_worker_ramp_max",
                "unit_other_civilian_ramp_max",
                "unit_military_ramp_max",
                "unit_rate_base",
                "unit_rate_progression",
                "accel_train",
                "accel_construct",
                "accel_research",
                "scholar_rate",
                "territory_taxes",
                "dutch_interest",
                "dutch_interest_cap",
            ]
            if k in rules
        },
        "units": U,
        "buildings": B,
        "techs": T,
        "slot_names": slot_names,
    }
    with open(OUT, "w") as f:
        json.dump(derived, f, indent=1, sort_keys=False)
    print(f"wrote {OUT}: {len(U)} units, {len(B)} buildings, {len(T)} techs")


if __name__ == "__main__":
    main()
