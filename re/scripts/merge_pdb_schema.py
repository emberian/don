#!/usr/bin/env python3
"""Merge schema/pdb-types.json into schema/state-schema.json.

`gen_state_schema.py` REGENERATES schema/state-schema.json from the DataWalk trace and
therefore drops anything merged in afterwards. Re-run this script after it:

    python3 re/scripts/gen_state_schema.py
    python3 re/scripts/merge_pdb_schema.py

What it adds, per class the DataWalk pass named:
  classes[<name>].pdb              full TPI layout -- size, bases, virtual bases,
                                   own fields and flattened fields (each tagged
                                   source: "pdb"), static members
  classes[<name>].sizeof_vs_pdb    "agree" or "DISAGREE datawalk=N pdb=M"
and, at the top level:
  pdb_only_classes                 classes present in pdb-types.json that the
                                   DataWalk pass never reached

schema/pdb-types.json itself is produced from ron-bin/sbl/rise.pdb (GUID
51D4F219-61C6-4F84-9D5B-C3361B0D291F age 1, verified against the PE CodeView record).
See docs/derivation/pdb-types.md.
"""
import json
import os
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
PDB_TYPES = os.path.join(ROOT, "schema", "pdb-types.json")
STATE = os.path.join(ROOT, "schema", "state-schema.json")

KEEP = ("source", "size", "has_vfptr", "bases", "virtual_bases",
        "fields", "flattened", "static_members")


def main():
    if not os.path.exists(PDB_TYPES):
        sys.exit(f"missing {PDB_TYPES}")
    pdbt = json.load(open(PDB_TYPES))
    ss = json.load(open(STATE))

    classes = ss.setdefault("classes", {})
    meta = ss.setdefault("_meta", {})
    src = meta.setdefault("sources", {})
    src["datawalk"] = ("abstract interpretation of the DataWalk visitors; supplies the byte "
                       "RANGES that are save/checksum-critical, and the order they are walked in.")
    src["pdb"] = ("rise.pdb TPI type stream; supplies real field NAMES, TYPES, OFFSETS and SIZES. "
                  "Entries under classes[*].pdb are tagged source=\"pdb\". Full closure in "
                  "schema/pdb-types.json; derivation in docs/derivation/pdb-types.md.")

    merged = agree = disagree = 0
    for name, e in pdbt["classes"].items():
        tgt = classes.get(name)
        if tgt is None:
            continue
        merged += 1
        tgt["pdb"] = {k: e[k] for k in KEEP if k in e}
        sz = tgt.get("sizeof")
        if sz is not None:
            if sz == e["size"]:
                tgt["sizeof_vs_pdb"] = "agree"
                agree += 1
            else:
                tgt["sizeof_vs_pdb"] = f"DISAGREE datawalk={sz} pdb={e['size']}"
                disagree += 1

    ss["pdb_only_classes"] = sorted(n for n in pdbt["classes"] if n not in classes)
    json.dump(ss, open(STATE, "w"), indent=1)
    print(f"merged PDB layout into {merged} classes; sizeof agree={agree} disagree={disagree}; "
          f"pdb_only={len(ss['pdb_only_classes'])}")


if __name__ == "__main__":
    main()
