#!/usr/bin/env python3
"""Project the simulation-closure inventory onto the `cv task` fleet queue.

`schema/simulation-closure.json` already carries, per red row, a name, a retail VA, a
status and an evidence note. That is the work breakdown structure; this script publishes it
so lanes can `cv task claim` instead of an orchestrator handing out directories.

Idempotent by title: a row whose title already exists as a non-terminal task is skipped, so
re-running after `tools/simulation-closure.py --write` adds only what is genuinely new.
It never closes anything — a row going green is not evidence the task landed, and deciding
that is `cv task verify`'s job, not this script's.

    python3 tools/closure-tasks.py                # dry run, prints what it would open
    python3 tools/closure-tasks.py --commit       # actually open them
    python3 tools/closure-tasks.py --domain tick  # one domain only
"""

from __future__ import annotations

import argparse
import json
import pathlib
import subprocess
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent
RECORD = REPO / "schema" / "simulation-closure.json"

# Domain -> (title prefix, how to name a row, what to put in the body).
PREFIX = {
    "tick": "tick",
    "orders": "order",
    "group_actions": "group",
    "opcodes": "opcode",
    "checksums": "checksum",
    "product_blockers": "blocker",
    "global_stages": "stage",
}


def row_title(domain: str, row: dict) -> str:
    name = row.get("name") or row.get("slug") or row.get("title") or "?"
    ident = row.get("id")
    lead = f"{PREFIX[domain]}"
    if ident is not None:
        lead = f"{lead} {ident}"
    return f"closure/{lead}: {name}"


def row_body(domain: str, row: dict) -> str:
    lines = [
        "Auto-published from schema/simulation-closure.json by tools/closure-tasks.py.",
        "",
        f"domain: {domain}",
    ]
    for key in (
        "id",
        "name",
        "slug",
        "title",
        "retail_va",
        "symbol",
        "walker",
        "element_class",
        "producer",
        "source",
        "status",
        "seam",
        "retail_evidence",
        "description",
        "note",
    ):
        if key in row and row[key] not in (None, "", []):
            lines.append(f"{key}: {row[key]}")
    if domain == "checksums":
        lines.append(
            "compares={compares} matches={matches} nontrivial={nontrivial_compares} "
            "best_survived={best_survived_turns}".format(**row)
        )
    lines += [
        "",
        "Before Ghidra: grep docs/assembly/ and docs/mechanics/, then re/decomp-all/<EA>.c.",
        "Read docs/tracks/swarm-strategy.md for the build hosts and the review discipline.",
        "Derive from the binary/PDB/shipped data; if a value cannot be derived, stop at a",
        "typed boundary naming the exact retail VA rather than inventing one.",
    ]
    return "\n".join(lines)


def red_rows(record: dict, only: str | None):
    for domain, rows in record["domains"].items():
        if only and domain != only:
            continue
        for row in rows:
            if not row.get("complete"):
                yield domain, row


def existing_titles() -> set[str]:
    try:
        out = subprocess.run(
            ["cv", "task", "list"], capture_output=True, text=True, timeout=60
        ).stdout
    except (OSError, subprocess.SubprocessError):
        return set()
    # `cv task list` is columnar; the title is the tail of the line. Substring containment
    # is the honest test here — we only need "have I already published this row".
    return {line.strip() for line in out.splitlines() if line.strip()}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--commit", action="store_true", help="actually open tasks")
    ap.add_argument("--domain", help="restrict to one closure domain")
    ap.add_argument("--from", dest="endpoint", default=None, help="acting endpoint")
    args = ap.parse_args()

    if not RECORD.is_file():
        print(f"missing {RECORD}", file=sys.stderr)
        return 2
    record = json.loads(RECORD.read_text())
    if args.domain and args.domain not in record["domains"]:
        print(f"unknown domain {args.domain!r}", file=sys.stderr)
        return 2

    listing = existing_titles()
    opened = skipped = 0
    for domain, row in red_rows(record, args.domain):
        title = row_title(domain, row)
        if any(title in line for line in listing):
            skipped += 1
            continue
        if not args.commit:
            print(f"WOULD OPEN  {title}")
            opened += 1
            continue
        cmd = ["cv", "task", "open", title, "--body", row_body(domain, row), "--repo", str(REPO)]
        if args.endpoint:
            cmd += ["--from", args.endpoint]
        res = subprocess.run(cmd, capture_output=True, text=True)
        if res.returncode != 0:
            print(f"FAILED {title}: {res.stderr.strip()}", file=sys.stderr)
            return 1
        print(f"opened {res.stdout.strip()}  {title}")
        opened += 1

    verb = "opened" if args.commit else "would open"
    print(f"\n{verb} {opened}, skipped {skipped} already present")
    if not args.commit:
        print("dry run — pass --commit to publish")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
