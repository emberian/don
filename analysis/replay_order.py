#!/usr/bin/env python3
"""
replay_order.py -- pull a frame-stamped opening build order out of a shipped .rcx
recorded game, using re/scripts/rcx_parse.py.

The two opcodes that matter are `0x18 queue_up` (train a unit / research a tech: payload
u32 type_id, u32 count) and `0x19 queue_up_build` (place a building: payload x,y,x,y,
u32 type_id, u32 count).  Both were derived in docs/derivation/replay-stream.md from
CommandPackage::process (FUN_0094A700).

The type ids resolve against schema/live/{unit,building,tech}-attributes.txt, which is a
completely independent artefact (live memory, not the replay), so the fact that they all
land on sensible types -- 50 Citizen, 417 Farm, 418 Woodcutter's Camp, 414 Small City,
544 Classical Age, 551 Written Word, 565 City State -- is a cross-check that could have
failed and did not.
"""

from __future__ import annotations

import json
import os
import struct
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, os.path.join(ROOT, "re", "scripts"))

D = json.load(open(os.path.join(HERE, "derived.json")))
NAMES = {int(k): v for k, v in D["slot_names"].items()}


def extract(path: str):
    """-> list of (frame, kind, type_id, name, count)."""
    out = subprocess.run(
        [sys.executable, os.path.join(ROOT, "re", "scripts", "rcx_parse.py"), path, "--commands"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    rows = []
    for line in out.splitlines():
        line = line.strip()
        if "queue_up" not in line or "frame=" not in line:
            continue
        frame = int(line.split("frame=")[1].split()[0])
        kind = "build" if "queue_up_build" in line else "queue"
        payload = line.split()[-1] if False else line
        # the payload is the trailing hex-byte run
        toks = payload.split()
        hexes = []
        for t in reversed(toks):
            if len(t) == 2 and all(c in "0123456789abcdefABCDEF" for c in t):
                hexes.append(t)
            else:
                break
        hexes.reverse()
        b = bytes(int(h, 16) for h in hexes)
        if kind == "queue" and len(b) >= 8:
            tid, cnt = struct.unpack_from("<II", b, 0)
        elif kind == "build" and len(b) >= 24:
            tid, cnt = struct.unpack_from("<II", b, 16)
        else:
            continue
        rows.append((frame, kind, tid, NAMES.get(tid, f"?{tid}"), cnt))
    return rows


def main() -> None:
    path = sys.argv[1] if len(sys.argv) > 1 else os.path.join(
        ROOT, "ron-data", "replays", "today.rcx"
    )
    rows = extract(path)
    print(f"{os.path.basename(path)}: {len(rows)} queue commands")
    print(f"{'frame':>7} {'t(mm:ss)':>9}  {'kind':<6} {'id':>4}  name")
    for frame, kind, tid, name, cnt in rows:
        t = frame / 15.0
        print(f"{frame:7d} {int(t)//60:5d}:{int(t)%60:02d}  {kind:<6} {tid:4d}  {name} x{cnt}")


if __name__ == "__main__":
    main()
