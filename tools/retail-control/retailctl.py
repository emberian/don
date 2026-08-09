#!/usr/bin/env python3
"""Host-side controller for the fail-closed retail-control DLL."""

from __future__ import annotations

import argparse
import http.server
import json
import os
from pathlib import Path
import random
import re
import socketserver
import subprocess
import sys
import threading
import time
import xml.etree.ElementTree as ET


HERE = Path(__file__).resolve().parent
VM = "Windows 11"
DEFAULT_GENERATION = "v2"
LEGACY_GENERATION = "v1"
GUEST_ROOT_BASE = r"C:\Users\Public\don-retail-control"
EXPECTED_SHA256 = "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079"


def validate_generation(generation: str) -> str:
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,47}", generation):
        raise SystemExit(f"unsafe controller generation {generation!r}")
    return generation


def generation_root(generation: str) -> str:
    generation = validate_generation(generation)
    if generation == LEGACY_GENERATION:
        return GUEST_ROOT_BASE
    return f"{GUEST_ROOT_BASE}-{generation}"


def generation_dll(generation: str) -> str:
    generation = validate_generation(generation)
    if generation == LEGACY_GENERATION:
        return "retail_control.dll"
    return f"retail_control-{generation}.dll"


def run(cmd: list[str], *, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                          check=check)


def guest_cmd(command: str, *, check: bool = True) -> str:
    p = run(["prlctl", "exec", VM, "cmd.exe", "/d", "/s", "/c", command], check=check)
    return p.stdout.replace("\r\n", "\n").strip()


def guest_ps(command: str, *, check: bool = True) -> str:
    p = run(["prlctl", "exec", VM, "powershell.exe", "-NoProfile", "-Command", command],
            check=check)
    return p.stdout.replace("\r\n", "\n").strip()


def pid() -> int:
    out = guest_cmd("for /f \"tokens=2\" %p in ('tasklist /nh /fi \"imagename eq riseofnations.exe\"') do @echo %p")
    values = [line.strip() for line in out.splitlines() if line.strip().isdigit()]
    if len(values) != 1:
        raise SystemExit(f"expected one riseofnations.exe, got {values!r}:\n{out}")
    return int(values[0])


class ReusableTCPServer(socketserver.TCPServer):
    allow_reuse_address = True


def serve_once(port: int):
    handler = lambda *a, **kw: http.server.SimpleHTTPRequestHandler(  # noqa: E731
        *a, directory=str(HERE), **kw
    )
    server = ReusableTCPServer(("0.0.0.0", port), handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server


def build() -> None:
    p = run(["sh", str(HERE / "build.sh")])
    print(p.stdout, end="")


def preflight(target_pid: int) -> None:
    out = guest_ps(f"(Get-FileHash -Algorithm SHA256 (Get-Process -Id {target_pid}).Path).Hash")
    compact = "".join(out.lower().split())
    if EXPECTED_SHA256 not in compact:
        raise SystemExit(f"REFUSING unsupported target digest; expected {EXPECTED_SHA256}:\n{out}")


def loaded_module(target_pid: int, dll_name: str) -> str:
    # The host PowerShell is 64-bit and does not reliably enumerate emulated x86
    # modules. Use the already-deployed x86 Toolhelp probe in the same ABI instead.
    injector = r"C:\Users\ember\donhook\donject.exe"
    out = guest_cmd(f'"{injector}" base {target_pid} "{dll_name}"', check=False)
    for line in out.splitlines():
        value = line.strip()
        if re.fullmatch(r"[0-9A-Fa-f]{8}", value) and int(value, 16):
            return f"{dll_name}@0x{value.lower()}"
    return ""


def deploy(target_pid: int, port: int, generation: str) -> None:
    root = generation_root(generation)
    dll_name = generation_dll(generation)
    preflight(target_pid)
    mapped = loaded_module(target_pid, dll_name)
    if mapped:
        raise SystemExit(
            f"REFUSING to replace mapped generation {generation!r}: {mapped}\n"
            "choose a new --generation; mapped controller DLLs remain parked by design"
        )
    build()
    server = serve_once(port)
    try:
        guest_cmd(f'if not exist "{root}" mkdir "{root}"')
        guest_cmd(
            f'curl.exe -f -sS -o "{root}\\{dll_name}.download" '
            f'http://10.211.55.2:{port}/retail_control.dll'
        )
        guest_cmd(f'move /y "{root}\\{dll_name}.download" "{root}\\{dll_name}" >nul')
        guest_cmd(
            f'del /q "{root}\\STOP" "{root}\\ready.txt" "{root}\\request.txt" '
            f'"{root}\\request.tmp" "{root}\\events.ndjson" 2>nul & exit /b 0'
        )
        injector = r"C:\Users\ember\donhook\donject.exe"
        out = guest_cmd(f'"{injector}" inject {target_pid} "{root}\\{dll_name}"')
        print(out)
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            ready = guest_cmd(f'if exist "{root}\\ready.txt" type "{root}\\ready.txt"',
                              check=False)
            if "state=armed" in ready:
                print(ready)
                return
            if "state=refused" in ready:
                raise SystemExit(ready)
            time.sleep(0.1)
        raise SystemExit("DLL loaded but did not publish state=armed")
    finally:
        guest_cmd(f'del /q "{root}\\{dll_name}.download" 2>nul & exit /b 0', check=False)
        server.shutdown()
        server.server_close()


def next_seq() -> int:
    return ((int(time.time() * 1000) & 0x7FFFFFFF) ^ random.getrandbits(20)) or 1


def validate_words(words: list[str]) -> None:
    if not words:
        raise SystemExit("a retail command is required")
    allowed = {"observe", "pause", "speed", "speed-up", "speed-down", "checksum",
               "move", "halt", "attack", "trace-move", "observe-guys",
               "observe-player", "validate-queue", "validate-build", "gather",
               "queue", "build", "run-frames", "find-build", "find-gather-build"}
    if words[0] not in allowed:
        raise SystemExit(f"unsupported verb {words[0]!r}")
    for word in words:
        if not word or any(c not in "abcdefghijklmnopqrstuvwxyz-0123456789xABCDEF" for c in word):
            raise SystemExit(f"unsafe token {word!r}")


def send(words: list[str], timeout: float, root: str) -> list[dict]:
    validate_words(words)
    seq = next_seq()
    line = " ".join([str(seq), *words])
    # A rename makes the one-slot request atomic from the worker's point of view.
    guest_cmd(
        f'(echo {line})>"{root}\\request.tmp" && '
        f'move /y "{root}\\request.tmp" "{root}\\request.txt" >nul'
    )
    deadline = time.monotonic() + timeout
    seen: dict[str, dict] = {}
    while time.monotonic() < deadline:
        out = guest_cmd(f'if exist "{root}\\events.ndjson" type "{root}\\events.ndjson"',
                        check=False)
        for raw in out.splitlines():
            raw = raw.strip()
            if not raw.startswith("{"):
                continue
            try:
                event = json.loads(raw)
            except json.JSONDecodeError:
                continue
            if event.get("seq") == seq:
                seen[event["phase"]] = event
        if "rejected" in seen or "observed" in seen or "applied" in seen:
            break
        if "queued" in seen and words[0] not in {"pause", "speed", "move", "halt", "attack"}:
            break
        time.sleep(0.05)
    if not seen:
        raise SystemExit(
            "no main-thread response; the process is attached but TurnControl::do_frame is not running"
        )
    for event in seen.values():
        print(json.dumps(event, sort_keys=True))
    return list(seen.values())


def normalized_trace_event(event: dict, start_frame: int, base: int) -> dict:
    runtime_vtable = int(event["order_vtable"], 16)
    preferred_vtable = runtime_vtable - base + 0x00400000 if runtime_vtable else 0
    order_kind = "MoveOrder" if preferred_vtable == 0x00B4A12C else (
        "none" if not runtime_vtable else "unresolved"
    )
    out = {
        "frame": event["frame"],
        "frame_delta": event["frame"] - start_frame,
        "seconds": event["seconds"],
        "pause": event["paused"],
        "speed": event["speed"],
        "position": {"x": event["unit_x"], "y": event["unit_y"]},
        "heading_u32": event["unit_angle"],
        "queued_action_heading_u32": event["unit_dest_angle"],
        "queued_action_position": {
            "x": event["unit_orders_x"], "y": event["unit_orders_y"]
        },
        "order": {
            "kind": order_kind,
            "length": event["order_length"],
            "preferred_vtable": f"0x{preferred_vtable:08x}",
            "flags": event["order_flags"],
            "metric": event["order_metric"],
        },
    }
    if event["move_valid"]:
        out["move_order"] = {
            "x": event["move_x"], "y": event["move_y"],
            "arrival_angle": event["move_angle"], "dest_latch": event["move_dest"],
            "tolerance": event["move_tolerance"], "pause": event["move_pause"],
            "retry": event["move_retry"], "attempts": event["move_attempts"],
            "timer": event["move_timer"], "facing": event["move_facing"],
            "active_destination": {
                "x": event["move_dest_x"], "y": event["move_dest_y"]
            },
            "last": {"x": event["move_last_x"], "y": event["move_last_y"]},
            "collision": {"x": event["move_coll_x"], "y": event["move_coll_y"]},
            "origin": {"x": event["move_orig_x"], "y": event["move_orig_y"]},
            "offset": {"x": event["move_off_x"], "y": event["move_off_y"]},
        }
    return out


def executable_base(root: str) -> int:
    ready = guest_cmd(f'type "{root}\\ready.txt"')
    for line in ready.splitlines():
        if line.startswith("base="):
            return int(line.split("=", 1)[1], 16)
    raise RuntimeError("controller ready record has no executable base")


def type_names() -> dict[int, str]:
    names: dict[int, str] = {}
    path = HERE.parents[1] / "schema/live/type-names.txt"
    for line in path.read_text().splitlines():
        fields = line.split("\t")
        if (len(fields) >= 4 and fields[0] in {"UnitType", "BuildType", "TechType", "ObjectType"}
                and fields[2].isdigit() and fields[3] and int(fields[2]) not in names):
            names[int(fields[2])] = fields[3]
    return names


PUBLIC_OBJECT_VTABLES = {
    0x00B417D0: "unit",
    0x00B4145C: "animal",
    0x00B42174: "build",
    0x00B42CF8: "wall",
}

# Concrete UnitOrder-subobject vtables and the exact value returned by their
# shipped get_type virtual.  PatrolOrder really returns NONE (0) in this build;
# it is intentionally not relabeled as PATROL (5).
PUBLIC_ORDER_VTABLES = {
    0x00B47628: ("AttackOrder", 10),
    0x00B47B08: ("StrafeOrder", 16),
    0x00B47E34: ("GroupAttackToOrder", 21),
    0x00B47F74: ("RepairOrder", 13),
    0x00B480BC: ("AwaitBoardOrder", 9),
    0x00B48208: ("BoardOrder", 8),
    0x00B4834C: ("BuildOrder", 6),
    0x00B48498: ("GroupPatrolOrder", 22),
    0x00B485D8: ("FleeToOrder", 4),
    0x00B48714: ("ExploreToOrder", 3),
    0x00B48850: ("AttackToOrder", 2),
    0x00B489B8: ("TradeOrder", 15),
    0x00B48AEC: ("PatrolOrder", 0),
    0x00B48C50: ("AirPatrolOrder", 17),
    0x00B48D88: ("ThinkOrder", 27),
    0x00B48EE8: ("GarrisonOrder", 26),
    0x00B49078: ("SpecialAnimOrder", 25),
    0x00B491FC: ("GroupAttackOrder", 20),
    0x00B494B4: ("GroupMoveOrder", 19),
    0x00B49608: ("FormOrder", 18),
    0x00B4976C: ("CastOrder", 14),
    0x00B498F4: ("GuardOrder", 12),
    0x00B49A40: ("FollowOrder", 11),
    0x00B49C1C: ("GatherOrder", 7),
    0x00B49D90: ("AirAttackGroundOrder", 24),
    0x00B49F1C: ("AttackGroundOrder", 23),
    0x00B4A12C: ("MoveOrder", 1),
}


def normalize_player_observation(event: dict, generation: str, base: int) -> dict:
    names = type_names()
    categories = {1: "unit", 2: "build", 3: "wall"}
    objects = []
    for item in event["player_objects"]:
        runtime_order_vtable = int(item["order_vtable"], 16)
        order_vtable = (runtime_order_vtable - base + 0x00400000
                        if runtime_order_vtable else 0)
        runtime_class_vtable = int(item["class_vtable"], 16)
        class_vtable = (runtime_class_vtable - base + 0x00400000
                        if runtime_class_vtable else 0)
        if item["order_length"] == 0 and not order_vtable:
            order_kind, order_index, order_valid = "none", 0, True
        elif order_vtable in PUBLIC_ORDER_VTABLES:
            order_kind, order_index = PUBLIC_ORDER_VTABLES[order_vtable]
            order_valid = True
        else:
            order_kind, order_index, order_valid = "unresolved", None, False
        type_valid = bool(item["type_valid"])
        category = categories.get(item["category"], "unknown")
        public_object = {
            "id": {
                "slot": event["local_player"],
                "band": category,
                "o": item["id"],
                "uid": item["uid"],
            },
            "object_id": item["id"],
            "category": category,
            "runtime_class": PUBLIC_OBJECT_VTABLES.get(class_vtable, "unknown"),
            "preferred_class_vtable": f"0x{class_vtable:08x}",
            "type_index": item["type"] if type_valid else None,
            "type_valid": type_valid,
            "type_name": (names.get(item["type"], f"TypeIndex({item['type']})")
                          if type_valid else "unresolved"),
            "position": {"x": item["x"], "y": item["y"], "z": item["z"]},
            "hits": item["hits"],
            "flags": item["flags"],
        }
        if category == "unit":
            public_object.update({
                "heading_u32": item["angle"],
                "physical_body_count": item["guy_length"],
                "order": {
                    "length": item["order_length"],
                    "kind": order_kind,
                    "index": order_index,
                    "index_valid": order_valid,
                    "preferred_vtable": f"0x{order_vtable:08x}",
                    "flags": item["order_flags"],
                    "metric": item["order_metric"],
                },
            })
            if item["order_target_valid"]:
                public_object["order"]["own_target"] = {
                    "object_id": item["order_target_id"],
                    "uid": item["order_target_uid"],
                }
            if item.get("queued_build_target_valid"):
                public_object["order"]["queued_build_target"] = {
                    "object_id": item["queued_build_target_id"],
                    "uid": item["queued_build_target_uid"],
                }
            if item.get("queued_build_order_seen"):
                public_object["order"]["queued_build_order_present"] = True
        elif category == "build":
            gather_resource = {417: "food", 418: "timber", 419: "metal",
                               420: "knowledge", 421: "oil", 422: "oil"}.get(
                                   item["type"] if type_valid else -1)
            public_object["complete"] = bool(item["flags"] & 4)
            public_object["gathering"] = {
                "capacity": max(0, item["gather_max"]),
                "raw_signed_i8": item["gather_max"],
                "resource": gather_resource,
            }
            public_object["production_queue"] = {
                "logical_length": item["queue_logical"],
                "storage_length": item["queue_size"],
                "truncated": bool(item["queue_truncated"]),
                "items": [
                    {
                        "type_index": queue_item["type"],
                        "type_name": names.get(queue_item["type"],
                                               f"TypeIndex({queue_item['type']})"),
                        "elapsed": queue_item["elapsed"],
                    }
                    for queue_item in item["queue"]
                ],
            }
        objects.append(public_object)
    resources = ["food", "timber", "wealth", "knowledge", "metal", "oil"]
    tech_bits = bytes.fromhex(event["player_tech_bits_hex"])
    owned_techs = [type_index for type_index in range(806)
                   if tech_bits[type_index >> 3] & (1 << (type_index & 7))]
    queued_types = [
        {
            "type_index": item["type"],
            "type_name": names.get(item["type"], f"TypeIndex({item['type']})"),
            "count": item["count"],
        }
        for item in event["player_queued_types"]
    ]
    return {
        "schema": "don.retail-player-observation.v3",
        "protocol": "don.retail-player.v3",
        "retail_executable_sha256": EXPECTED_SHA256,
        "controller_generation": generation,
        "public_scope": {
            "owner": event["local_player"],
            "includes": ["own active object bands", "own stockpile", "own commerce cap",
                         "own population", "own building gather capacity", "public game clock"],
            "excludes": ["enemy and neutral object tables", "enemy resources",
                         "fog-hidden map state", "target-object dereferences",
                         "visibility flags not proven local-slot-specific"],
        },
        "frame": event["frame"],
        "seconds": event["seconds"],
        "paused": event["paused"],
        "speed": event["speed"],
        "world": {
            "tile_xs": event["world_tile_xs"],
            "tile_ys": event["world_tile_ys"],
            "coordinate_units_per_tile": 192,
        },
        "player": {
            "owner": event["local_player"],
            "slot": event["local_player"],
            "who": event["local_player"],
            "tribe": event["player_tribe"],
            "team": event["player_team"],
            "leader_flags": event["player_leader_flags"],
            "game_info_flags": event["player_identity_flags"],
        },
        "economy": {
            "resource_order": resources,
            "stockpile_i32": event["player_resources"],
            "commerce_cap_x16_i32": event["player_resource_caps"][:6],
            "capped_state_i32": event["player_over_cap"],
        },
        "population": {"current": event["player_pop"], "cap": event["player_pop_cap"]},
        "technology": {
            "age": event["player_age"],
            "epochs": dict(zip(["military", "civic", "commerce", "science"],
                               event["player_epochs"])),
            "owned_type_indices": owned_techs,
        },
        "queued_types": queued_types,
        "object_slots": event["player_slots"],
        "object_marks": {
            "unit": event["player_unit_mark"],
            "building": event["player_build_mark"],
            "wall": event["player_wall_mark"],
        },
        "objects": sorted(objects, key=lambda item: item["object_id"]),
    }


def player_observation(root: str, generation: str) -> dict:
    events = send(["observe-player"], 8.0, root)
    event = next((e for e in events if e.get("phase") == "observed"), None)
    if not event:
        raise RuntimeError("retail did not publish a player observation")
    if event.get("note"):
        raise RuntimeError(f"retail player observation failed closed (note={event['note']})")
    if event.get("player_object_truncated"):
        raise RuntimeError("retail player observation exceeded MAX_PUBLIC_OBJECTS")
    if event.get("player_queued_type_truncated"):
        raise RuntimeError("retail queued-type observation exceeded MAX_QUEUED_TYPES")
    if event.get("paused") != 1:
        raise RuntimeError("REFUSING player observation unless the supervised match is paused")
    observation = normalize_player_observation(event, generation, executable_base(root))
    if any(not obj["type_valid"] or obj["runtime_class"] == "unknown"
           for obj in observation["objects"]):
        raise RuntimeError("retail player observation contains an unresolved own object")
    if any(obj["order"]["length"] < 0 or not obj["order"]["index_valid"]
           for obj in observation["objects"] if obj["category"] == "unit"):
        raise RuntimeError("retail player observation contains an unresolved own-unit order")
    if any(obj["order"]["kind"] == "GatherOrder" and "own_target" not in obj["order"]
           for obj in observation["objects"] if obj["category"] == "unit"):
        raise RuntimeError("retail player observation contains an unresolved own GatherOrder target")
    if any(obj["order"]["kind"] == "BuildOrder" and "own_target" not in obj["order"]
           for obj in observation["objects"] if obj["category"] == "unit"):
        raise RuntimeError("retail player observation contains an unresolved own BuildOrder target")
    if any(obj["order"].get("queued_build_order_present") and
           "queued_build_target" not in obj["order"]
           for obj in observation["objects"] if obj["category"] == "unit"):
        raise RuntimeError("retail player observation contains an unresolved queued BuildOrder")
    if any(obj["production_queue"]["truncated"]
           for obj in observation["objects"] if obj["category"] == "build"):
        raise RuntimeError("retail player observation contains a truncated production queue")
    return observation


def scout_policy(observation: dict) -> dict:
    owner = observation["player"]["owner"]
    candidates = [
        obj for obj in observation["objects"]
        if obj["category"] == "unit" and obj["type_name"] == "Scout"
        and obj["order"]["length"] == 0 and obj["hits"] > 0
    ]
    actions = []
    reason = "no live idle owned Scout; preserve economy and issue no command"
    if candidates:
        scout = min(candidates, key=lambda obj: obj["object_id"])
        x = scout["position"]["x"]
        y = scout["position"]["y"]
        max_x = observation["world"]["tile_xs"] * 192
        target_x = x + 192 if x + 192 < max_x else x - 192
        actions.append({
            "id": "scout-step-0",
            "verb": "move",
            "owner": owner,
            "object_ids": [scout["object_id"]],
            "target": {"x": target_x, "y": y},
            "queue": "new",
            "order": "MOVE_TO",
            "max_frames": 60,
            "reason": "lowest object-id live idle owned Scout; one-tile bounded east/west step",
        })
        reason = "deterministic scout step; citizens, merchants, and buildings are untouched"
    return {
        "schema": "don.retail-player-action-batch.v1",
        "protocol": "don.retail-player.v1",
        "policy": "deterministic-scout-economy-safe.v1",
        "observation_frame": observation["frame"],
        "max_actions": 4,
        "reason": reason,
        "actions": actions,
    }


def validate_action_batch(batch: dict, observation: dict) -> None:
    actions = batch.get("actions", [])
    if len(actions) > 4:
        raise RuntimeError("REFUSING action batch larger than four")
    owned = {obj["object_id"]: obj for obj in observation["objects"]}
    owner = observation["player"]["owner"]
    max_x = observation["world"]["tile_xs"] * 192
    max_y = observation["world"]["tile_ys"] * 192
    for action in actions:
        if action.get("verb") != "move" or action.get("owner") != owner:
            raise RuntimeError("v1 executor accepts only own-player move actions")
        ids = action.get("object_ids", [])
        if len(ids) != 1 or ids[0] not in owned or owned[ids[0]]["category"] != "unit":
            raise RuntimeError("v1 move must select exactly one observed owned live unit")
        target = action.get("target", {})
        if not (0 <= target.get("x", -1) < max_x and 0 <= target.get("y", -1) < max_y):
            raise RuntimeError("v1 move target lies outside observed public world bounds")
        if not (1 <= action.get("max_frames", 0) <= 180):
            raise RuntimeError("v1 move exceeds the bounded trace frame limit")


OPENING_RESEARCH_TYPES = [565, 558, 572, 544, 551]
MARSHAL_CAP_TECH_TYPES = [565, 558, 544, 551]


def exact_validation(root: str, words: list[str]) -> dict:
    events = send(words, 8.0, root)
    event = next((item for item in events if item.get("phase") == "observed"), None)
    if not event or event.get("paused") != 1:
        raise RuntimeError("retail legality query did not run in the paused main-thread callback")
    return event


def queue_validation(root: str, owner: int, producer_id: int, type_index: int) -> dict:
    return exact_validation(root, ["validate-queue", str(owner), str(producer_id),
                                   str(type_index)])


def build_validation(root: str, owner: int, worker_id: int, x: int, y: int,
                     x2: int, y2: int, type_index: int) -> dict:
    return exact_validation(root, ["validate-build", str(owner), str(x), str(y),
                                   str(x2), str(y2), str(type_index), "2",
                                   str(worker_id)])


def find_build_site(root: str, observation: dict, worker_id: int,
                    type_index: int, radius: int = 8) -> dict:
    """Ask retail's main-thread GroupData::validate_build for a bounded legal site."""
    owned = {obj["object_id"]: obj for obj in observation["objects"]}
    worker = owned.get(worker_id)
    if (not worker or worker["category"] != "unit" or
            worker.get("type_index") not in {50, 51}):
        raise RuntimeError("placement query requires one observed own citizen")
    if not 0 <= radius <= 8:
        raise RuntimeError("placement query radius exceeds the bounded retail callback limit")
    origin_x = ((worker["position"]["x"] + 24) // 48) * 48
    origin_y = ((worker["position"]["y"] + 24) // 48) * 48
    event = exact_validation(root, ["find-build", str(observation["player"]["owner"]),
                                    str(origin_x), str(origin_y), str(radius),
                                    str(type_index), str(worker_id)])
    accepted = bool(event["validation_result"])
    result = {
        "schema": "don.retail-build-placement-query.v1",
        "protocol": observation["protocol"],
        "frame": observation["frame"],
        "worker_id": worker_id,
        "type_index": type_index,
        "origin": {"x": origin_x, "y": origin_y},
        "radius_ucoord": radius,
        "lattice_coord_units": 48,
        "tested": event["placement_tested"],
        "retail_result": event["validation_result"],
        "accepted": accepted,
        "site": ({"x": event["placement_x"], "y": event["placement_y"],
                  "x2": -1, "y2": -1} if accepted else None),
        "gesture": "retail simple pick: (x, y, -1, -1)",
    }
    if accepted:
        max_x = observation["world"]["tile_xs"] * 192
        max_y = observation["world"]["tile_ys"] * 192
        if not (0 <= result["site"]["x"] < max_x and
                0 <= result["site"]["y"] < max_y):
            raise RuntimeError("retail placement query returned a site outside public world bounds")
        if result["site"]["x"] % 48 or result["site"]["y"] % 48:
            raise RuntimeError("retail placement query escaped the exact UCoord lattice")
    return result


def gather_build_query(root: str, observation: dict, worker_id: int,
                       type_index: int, origin_x: int, origin_y: int,
                       first_ring: int, last_ring: int) -> dict:
    """Score one bounded Arena tile-ring chunk through exact retail queries."""
    owned = {obj["object_id"]: obj for obj in observation["objects"]}
    worker = owned.get(worker_id)
    if (not worker or worker["category"] != "unit" or
            worker.get("type_index") not in {50, 51}):
        raise RuntimeError("gather placement query requires one observed own citizen")
    if not (2 <= first_ring <= last_ring <= 23 and last_ring - first_ring <= 1):
        raise RuntimeError("gather placement query exceeds the bounded Arena ring chunk")
    origin_x = ((origin_x + 96) // 192) * 192
    origin_y = ((origin_y + 96) // 192) * 192
    event = exact_validation(root, ["find-gather-build",
                                    str(observation["player"]["owner"]),
                                    str(origin_x), str(origin_y), str(first_ring),
                                    str(last_ring),
                                    str(type_index), str(worker_id)])
    accepted = bool(event["validation_result"] and event["placement_capacity"] > 0)
    return {
        "origin": {"x": origin_x, "y": origin_y},
        "ring_range_tiles": [first_ring, last_ring],
        "retail_note": event.get("note", 0),
        "tested": event["placement_tested"],
        "legal": event["placement_legal"],
        "seen_legal": event["placement_seen"],
        "accepted": accepted,
        "retail_result": event["validation_result"],
        "capacity": event["placement_capacity"],
        "ring": event["placement_ring"],
        "site": ({"x": event["placement_x"], "y": event["placement_y"],
                  "x2": -1, "y2": -1,
                  "snapped_x": event["placement_snap_x"],
                  "snapped_y": event["placement_snap_y"]} if accepted else None),
    }


def marshal_gather_state(observation: dict) -> dict:
    """Reproduce Marshal's exact useful-slot and seat-gap predicates from retail v3."""
    if observation.get("protocol") != "don.retail-player.v3":
        raise RuntimeError("gather-state planning requires retail-player.v3")
    city_gather, peasant_rate, _ = arena_rule_ints()
    complete_cities = sum(1 for obj in observation["objects"]
                          if obj["category"] == "build" and obj.get("complete") and
                          obj.get("type_index") in {414, 415, 416})
    caps = observation["economy"]["commerce_cap_x16_i32"]
    useful = [max(0, (caps[r] - complete_cities * city_gather[r] * 16) //
                  (max(1, peasant_rate) * 16)) for r in range(6)]
    seats = [0] * 6
    resource_index = {name: i for i, name in
                      enumerate(observation["economy"]["resource_order"])}
    for obj in observation["objects"]:
        gathering = obj.get("gathering")
        if obj["category"] != "build" or not gathering or not gathering.get("resource"):
            continue
        seats[resource_index[gathering["resource"]]] += gathering["capacity"]
    return {
        "complete_city_count": complete_cities,
        "useful_slots": useful,
        "seats": seats,
        "food_gap": useful[0] - seats[0],
        "wood_gap": useful[1] - seats[1],
        "formula": ("max(0,(cap_x16-complete_cities*CITY_GATHER*16)//"
                    "(PEASANT_RATE*16)) - sum(positive signed gather_max)"),
    }


def marshal_builder_for(observation: dict, site: dict) -> int | None:
    """Retail-v3 form of Arena builder_for_except with no active scout exclusion."""
    by_id = {obj["object_id"]: obj for obj in observation["objects"]}
    citizens = [obj for obj in observation["objects"]
                if obj["category"] == "unit" and obj.get("type_index") in {50, 51}]
    best: tuple[int, int] | None = None
    tx, ty = site["snapped_x"] // 192, site["snapped_y"] // 192
    for worker in citizens:
        order = worker["order"]
        if order["kind"] == "none":
            busy = 0
        elif order["kind"] == "GatherOrder" and order.get("own_target"):
            target = by_id.get(order["own_target"]["object_id"])
            capacity = (target or {}).get("gathering", {}).get("capacity", 0)
            busy = 200 + 400 // max(1, capacity)
        elif order["kind"] == "BuildOrder":
            busy = 1200
        else:
            busy = 1500
        wx, wy = worker["position"]["x"] // 192, worker["position"]["y"] // 192
        score = busy + max(abs(wx - tx), abs(wy - ty))
        candidate = (score, worker["object_id"])
        if best is None or candidate < best:
            best = candidate
    return best[1] if best else None


def find_visible_gather_site(root: str, observation: dict,
                             type_index: int) -> dict:
    """Run Arena best_gather_site rings using fog-safe exact retail capacity."""
    citizens = sorted((obj for obj in observation["objects"]
                       if obj["category"] == "unit" and obj.get("type_index") in {50, 51}),
                      key=lambda obj: obj["object_id"])
    if not citizens:
        return {"accepted": False, "queries": [], "reason": "no own Citizen"}
    cities = sorted((obj for obj in observation["objects"]
                     if obj["category"] == "build" and obj.get("complete") and
                     obj.get("type_index") in {414, 415, 416}),
                    key=lambda obj: obj["object_id"])
    if not cities:
        return {"accepted": False, "queries": [], "reason": "no complete own capital"}
    capital = cities[0]
    origin_x, origin_y = capital["position"]["x"], capital["position"]["y"]
    queries = []
    candidates = []
    for ring in range(2, 24):
        query = gather_build_query(root, observation, citizens[0]["object_id"],
                                   type_index, origin_x, origin_y, ring, ring)
        queries.append(query)
        if query["accepted"]:
            candidates.append(query)
            # Preserve Marshal's documented policy threshold. This is not a claim
            # that five is retail's maximum; the exact retail capacity is retained.
            if query["capacity"] >= 5:
                return {"accepted": True, "queries": queries, "best": query,
                        "selection": ("Arena first ring reaching policy threshold 5; "
                                      "retail capacity is untruncated")}
    if not candidates:
        return {"accepted": False, "queries": queries,
                "reason": "no fully-currently-visible retail-capacity site"}
    best = min(candidates, key=lambda query: (-query["capacity"], query["ring"]))
    return {"accepted": True, "queries": queries, "best": best,
            "selection": "Arena capacity*1000-ring after exhausting rings 2..23"}


def find_visible_ordinary_site(root: str, observation: dict,
                               type_index: int) -> dict:
    """Run Arena site_near rings with retail legality behind exact current fog."""
    citizens = sorted((obj for obj in observation["objects"]
                       if obj["category"] == "unit" and obj.get("type_index") in {50, 51}),
                      key=lambda obj: obj["object_id"])
    cities = sorted((obj for obj in observation["objects"]
                     if obj["category"] == "build" and obj.get("complete") and
                     obj.get("type_index") in {414, 415, 416}),
                    key=lambda obj: obj["object_id"])
    if not citizens or not cities:
        return {"accepted": False, "queries": [],
                "reason": "no own Citizen or complete own capital"}
    origin_x, origin_y = cities[0]["position"]["x"], cities[0]["position"]["y"]
    queries = []
    # Arena site_near(capital, max_r=18) uses the half-open range 2..18.
    for ring in range(2, 18):
        query = gather_build_query(root, observation, citizens[0]["object_id"],
                                   type_index, origin_x, origin_y, ring, ring)
        queries.append(query)
        if query["accepted"]:
            return {"accepted": True, "queries": queries, "best": query,
                    "selection": "Arena first legal site_near candidate on rings 2..17"}
    return {"accepted": False, "queries": queries,
            "reason": "no fully-currently-visible retail-legal site"}


def building_row(type_index: int) -> dict[str, int | str]:
    path = HERE.parents[1] / "schema/live/live-tables-building.tsv"
    lines = path.read_text().splitlines()
    header = lines[0].split("\t")
    for line in lines[1:]:
        fields = line.split("\t")
        if int(fields[header.index("type_id")]) == type_index:
            numeric = {name for name in ["type_id", "age", "preq0", "preq1", "preq2",
                                         "cost0", "cost1", "cost2", "cost3", "cost4",
                                         "cost5"]}
            return {name: (int(value) if name in numeric else value)
                    for name, value in zip(header, fields)}
    raise RuntimeError(f"live building table has no TypeIndex {type_index}")


def build_cost_factor() -> int:
    constants = ET.parse(HERE.parents[1] / "ron-data/rules.xml").getroot().find("CONSTANTS")
    node = constants.find("BUILD_COST_FACTOR") if constants is not None else None
    if node is None:
        raise RuntimeError("rules.xml lacks BUILD_COST_FACTOR")
    match = re.search(r"-?\d+", node.attrib["value"])
    if not match:
        raise RuntimeError("BUILD_COST_FACTOR has no integer value")
    return int(match.group())


def static_build_legality(type_index: int, observation: dict) -> dict:
    """Necessary public prerequisite/base-cost gate preceding retail's exact gates."""
    row = building_row(type_index)
    held = set(observation["technology"]["owned_type_indices"])
    prerequisites = [int(row[f"preq{i}"]) for i in range(3) if int(row[f"preq{i}"]) >= 0]
    raw_cost = [int(row[f"cost{i}"]) for i in range(6)]
    cost = [value * build_cost_factor() for value in raw_cost]
    stock = observation["economy"]["stockpile_i32"]
    reasons = []
    if type_index not in held:
        reasons.append("building TypeIndex is not enabled in the local public tech bitset")
    missing = [value for value in prerequisites if value not in held]
    if missing:
        reasons.append(f"missing prerequisite TypeIndex values {missing}")
    if int(row["age"]) > observation["technology"]["age"]:
        reasons.append("building age exceeds the observed local age")
    short = [i for i, needed in enumerate(cost) if stock[i] < needed]
    if short:
        reasons.append(f"insufficient public stockpile channels {short}")
    return {
        "accepted": not reasons,
        "reasons": reasons,
        "type_name": row["name_display"],
        "prerequisites": prerequisites,
        "base_cost_i32": cost,
        "cost_scope": ("live table base cost times BUILD_COST_FACTOR; retail issue remains the "
                       "authority for count ramping and civilization modifiers"),
        "resource_order": observation["economy"]["resource_order"],
    }


def conservative_opening_policy(observation: dict, root: str) -> dict:
    owner = observation["player"]["owner"]
    by_type: dict[int, list[dict]] = {}
    for obj in observation["objects"]:
        if obj["type_valid"]:
            by_type.setdefault(obj["type_index"], []).append(obj)
    queued = {item["type_index"]: item["count"] for item in observation["queued_types"]}
    validations: list[dict] = []

    cities = sorted(
        [obj for type_index in (414, 415, 416) for obj in by_type.get(type_index, [])],
        key=lambda obj: obj["object_id"],
    )
    if (observation["population"]["current"] + queued.get(50, 0) < 12 and cities):
        producer = cities[0]
        result = queue_validation(root, owner, producer["object_id"], 50)
        validations.append({"verb": "validate-queue", "producer_id": producer["object_id"],
                            "type_index": 50, "retail_result": result["validation_result"]})
        if result["validation_result"]:
            return {
                "schema": "don.retail-economy-action-plan.v1",
                "protocol": observation["protocol"],
                "policy": "deterministic-conservative-opening.v1",
                "observation_frame": observation["frame"],
                "validations": validations,
                "reason": "population below 12 and retail BuildData::can_queue accepts Citizen",
                "action": {"verb": "queue", "owner": owner,
                           "producer_id": producer["object_id"], "type_index": 50,
                           "type_name": "Citizen", "count": 1},
            }

    libraries = sorted(by_type.get(435, []), key=lambda obj: obj["object_id"])
    owned_techs = set(observation["technology"]["owned_type_indices"])
    if libraries:
        for type_index in OPENING_RESEARCH_TYPES:
            if type_index in owned_techs or queued.get(type_index, 0):
                continue
            producer = libraries[0]
            result = queue_validation(root, owner, producer["object_id"], type_index)
            validations.append({"verb": "validate-queue", "producer_id": producer["object_id"],
                                "type_index": type_index,
                                "retail_result": result["validation_result"]})
            if result["validation_result"]:
                return {
                    "schema": "don.retail-economy-action-plan.v1",
                    "protocol": observation["protocol"],
                    "policy": "deterministic-conservative-opening.v1",
                    "observation_frame": observation["frame"],
                    "validations": validations,
                    "reason": "first fixed-priority missing tech accepted by retail can_queue",
                    "action": {"verb": "queue", "owner": owner,
                               "producer_id": producer["object_id"],
                               "type_index": type_index,
                               "type_name": type_names().get(type_index,
                                                              f"TypeIndex({type_index})"),
                               "count": 1},
                }

    return {
        "schema": "don.retail-economy-action-plan.v1",
        "protocol": observation["protocol"],
        "policy": "deterministic-conservative-opening.v1",
        "observation_frame": observation["frame"],
        "validations": validations,
        "reason": ("no conservative queue action passed the shipped retail legality gates; "
                   "this queue-only opening policy does not invent a building demand"),
        "action": None,
    }


def arena_rule_ints() -> tuple[list[int], int, int]:
    constants = ET.parse(HERE.parents[1] / "ron-data/rules.xml").getroot().find("CONSTANTS")
    if constants is None:
        raise RuntimeError("rules.xml has no CONSTANTS block")
    city = constants.find("CITY_GATHER")
    peasant = constants.find("PEASANT_RATE")
    tech_factor = constants.find("TECH_COST_FACTOR")
    if city is None or peasant is None or tech_factor is None:
        raise RuntimeError("rules.xml lacks a Marshal economy constant")
    def first_int(value: str) -> int:
        match = re.search(r"-?\d+", value)
        if not match:
            raise RuntimeError(f"retail rule has no integer value: {value!r}")
        return int(match.group())
    return ([first_int(city.attrib[f"entry{i}"]) for i in range(6)],
            first_int(peasant.attrib["value"]), first_int(tech_factor.attrib["value"]))


def live_tech_raw_food_cost(type_index: int) -> int:
    lines = (HERE.parents[1] / "schema/live/live-tables-tech.tsv").read_text().splitlines()
    header = lines[0].split("\t")
    type_col, cost_col = header.index("type_id"), header.index("cost0")
    for line in lines[1:]:
        fields = line.split("\t")
        if int(fields[type_col]) == type_index:
            return int(fields[cost_col])
    raise RuntimeError(f"live tech table has no TypeIndex {type_index}")


def arena_marshal_extracted_plan(observation: dict, root: str,
                                 queue_query=queue_validation,
                                 gather_site_query=find_visible_gather_site,
                                 ordinary_site_query=find_visible_ordinary_site) -> dict:
    """Faithful supported subsequence of Marshal::act, in its source command order."""
    protocol = observation.get("protocol")
    if protocol not in {"don.retail-player.v2", "don.retail-player.v3"}:
        raise RuntimeError("Arena Marshal adapter requires fog-safe retail-player.v2/v3")
    owner = observation["player"]["owner"]
    objects = observation["objects"]
    by_type: dict[int, list[dict]] = {}
    for obj in objects:
        if obj.get("type_valid"):
            by_type.setdefault(obj["type_index"], []).append(obj)
    queued = {item["type_index"]: item["count"] for item in observation["queued_types"]}
    held = set(observation["technology"]["owned_type_indices"])
    trace: list[dict] = []
    supported: list[dict] = []

    # Marshal::sense cannot infer threat or an enemy base: no enemy list and no
    # last-damaged timestamp are exposed.  Missing evidence means initial Massing, not a
    # fabricated peaceful enemy observation.
    trace.append({
        "stage": "sense",
        "source": "Marshal::sense",
        "result": "Massing",
        "reason": f"{protocol} contains no fog-approved enemy sightings or last-damaged field",
    })

    # Marshal::economy calls next_tech in this exact order. next_tech does not skip an
    # already queued tech; queue_at then suppresses it, and does not fall through.
    next_tech = next((type_index for type_index in OPENING_RESEARCH_TYPES
                      if type_index not in held), None)
    libraries = sorted(by_type.get(435, []), key=lambda obj: obj["object_id"])
    if next_tech is None:
        trace.append({"stage": "economy.tech", "result": "complete"})
    elif queued.get(next_tech, 0):
        trace.append({
            "stage": "economy.tech", "type_index": next_tech,
            "type_name": type_names().get(next_tech, f"TypeIndex({next_tech})"),
            "result": "suppressed", "reason": "Marshal queue_at rejects a tech already queued",
        })
    elif not libraries:
        trace.append({"stage": "economy.tech", "type_index": next_tech,
                      "result": "suppressed", "reason": "no own complete Library"})
    else:
        producer = min(libraries,
                       key=lambda obj: (obj["production_queue"]["logical_length"],
                                        obj["object_id"]))
        validation = queue_query(root, owner, producer["object_id"], next_tech)
        accepted = bool(validation["validation_result"])
        trace.append({
            "stage": "economy.tech", "type_index": next_tech,
            "type_name": type_names().get(next_tech, f"TypeIndex({next_tech})"),
            "producer_id": producer["object_id"], "retail_can_queue": int(accepted),
            "result": "emit" if accepted else "suppressed",
        })
        if accepted:
            supported.append({"verb": "queue", "owner": owner,
                              "producer_id": producer["object_id"],
                              "type_index": next_tech,
                              "type_name": type_names().get(next_tech), "count": 1})

    if protocol == "don.retail-player.v2":
        trace.append({
            "stage": "economy.placement",
            "source": "Marshal::economy/place_except",
            "result": "unsupported",
            "reason": ("BUILD_AT ingress and the retail simple-pick oracle are proven, but v2 "
                       "lacks exact own gather capacity and a fog-gated prospective terrain "
                       "oracle; no Farm or other Build command is substituted"),
        })
        gather_state = None
        placement_action = None
    else:
        gather_state = marshal_gather_state(observation)
        city_gather, peasant_rate, tech_cost_factor = arena_rule_ints()
        cap_first_want = next((t for t in MARSHAL_CAP_TECH_TYPES if t not in held), None)
        classical_food_cost = live_tech_raw_food_cost(544) * tech_cost_factor
        food_locked_for_placement = (
            cap_first_want == 544 and 544 not in held and
            observation["economy"]["stockpile_i32"][0] * 10 >= classical_food_cost * 6
        )
        type_count = lambda t: len(by_type.get(t, []))
        wants: list[tuple[int, str]] = []
        # A fresh adapter has no approved threat sighting, hence Massing: tower false.
        if 572 in held and type_count(427) < 1:
            wants.append((427, "Barracks after The Art of War"))
        if gather_state["wood_gap"] > 0 and type_count(418) < 4:
            wants.append((418, "positive timber seat gap; Camp precedes Mine/City/Farm"))
        if (544 in held and gather_state["useful_slots"][4] > gather_state["seats"][4]
                and type_count(419) < 3):
            wants.append((419, "positive Classical metal seat gap"))
        if (565 in held and type_count(414) + type_count(415) + type_count(416) < 2
                and not food_locked_for_placement):
            wants.append((414, "City State expansion while not defending/food-locked"))
        if (gather_state["food_gap"] > 0 and not food_locked_for_placement
                and type_count(417) < 9):
            wants.append((417, "positive food seat gap after higher placement priorities"))

        attempts: list[dict] = []
        placement_action = None
        blocked_by = None
        for type_index, reason in wants:
            public_gate = static_build_legality(type_index, observation)
            attempt: dict = {
                "type_index": type_index,
                "type_name": type_names().get(type_index),
                "want_reason": reason,
                "public_gate": public_gate,
            }
            attempts.append(attempt)
            if not public_gate["accepted"]:
                attempt["result"] = "suppressed"
                attempt["reason"] = "Arena legal/can_pay necessary public gate failed"
                continue
            # Cycle 8 proves the Camp/Farm branches. If an earlier wanted branch is not
            # supported, fail closed: whether it emitted determines whether Marshal
            # would break before reaching a lower priority.
            if type_index not in {417, 418}:
                attempt["result"] = "blocked"
                attempt["reason"] = "publicly eligible higher branch lacks an exact adapter"
                blocked_by = {"type_index": type_index,
                              "type_name": type_names().get(type_index),
                              "reason": "higher-priority Marshal placement is not yet adapted"}
                break
            if type_index != 417 and any(not obj.get("complete", False)
                                         for obj in by_type.get(type_index, [])):
                attempt["result"] = "suppressed"
                attempt["reason"] = "place_except forbids duplicate incomplete non-Farm"
                continue
            site_result = (gather_site_query(root, observation, type_index)
                           if type_index == 418 else
                           ordinary_site_query(root, observation, type_index))
            attempt["site_query"] = site_result
            if not site_result["accepted"]:
                attempt["result"] = "suppressed"
                attempt["reason"] = "no exact currently-visible retail-legal site"
                continue
            chosen = site_result["best"]
            worker_id = marshal_builder_for(observation, chosen["site"])
            if worker_id is None:
                attempt["result"] = "suppressed"
                attempt["reason"] = "builder_for_except found no own Citizen"
                continue
            attempt["result"] = "emit"
            attempt["worker_id"] = worker_id
            placement_action = {
                "verb": "build", "owner": owner, "worker_ids": [worker_id],
                "type_index": type_index, "type_name": type_names().get(type_index),
                "x1": chosen["site"]["x"], "y1": chosen["site"]["y"],
                "x2": -1, "y2": -1, "queue": 2,
                "placement_evidence": {
                    "origin": chosen["origin"], "ring": chosen["ring"],
                    "capacity": chosen["capacity"],
                    "snapped_x": chosen["site"]["snapped_x"],
                    "snapped_y": chosen["site"]["snapped_y"],
                    "visibility": ("every exact calc_gather W block's four F cells were "
                                   "currently visible before validate_build/max_gatherers"),
                    "selection": site_result["selection"],
                },
            }
            supported.append(placement_action)
            break
        trace.append({
            "stage": "economy.placement",
            "source": "Marshal::economy/place_except",
            "result": ("emit" if placement_action else
                       "blocked" if blocked_by else "suppressed"),
            "gather_state": gather_state,
            "food_locked": food_locked_for_placement,
            "wants": [{"type_index": t, "type_name": type_names().get(t), "reason": why}
                      for t, why in wants],
            "attempts": attempts,
            "blocked_by": blocked_by,
            "reason": ("first supported placement emitted in exact Marshal priority"
                       if placement_action else
                       "a higher unsupported placement prevents lower-branch substitution"
                       if blocked_by else "no supported placement emitted"),
        })

    # CapFirst::target_citizens = useful food seats + useful timber seats + 3 builders.
    # Consume live commerce-cap x16 values, and shipped rule constants, preserving the
    # same integer division as useful_slots.
    city_gather, peasant_rate, tech_cost_factor = arena_rule_ints()
    cities = sum(1 for t in (414, 415, 416) for obj in by_type.get(t, [])
                 if protocol == "don.retail-player.v2" or obj.get("complete", False))
    caps = observation["economy"]["commerce_cap_x16_i32"]
    useful = [max(0, (caps[r] - cities * city_gather[r] * 16) //
                  max(1, peasant_rate * 16)) for r in (0, 1)]
    target_citizens = max(12, useful[0] + useful[1] + 3)
    citizen_count = len(by_type.get(50, [])) + len(by_type.get(51, [])) + queued.get(50, 0)
    cap_first_want = next((t for t in MARSHAL_CAP_TECH_TYPES if t not in held), None)
    classical_food_cost = live_tech_raw_food_cost(544) * tech_cost_factor
    food_locked = (cap_first_want == 544 and 544 not in held and
                   observation["economy"]["stockpile_i32"][0] * 10 >=
                   classical_food_cost * 6)
    cities_by_queue = sorted(
        [obj for type_index in (414, 415, 416) for obj in by_type.get(type_index, [])],
        key=lambda obj: (obj["production_queue"]["logical_length"], obj["object_id"]),
    )
    if (not food_locked and citizen_count < target_citizens and
            observation["population"]["current"] < observation["population"]["cap"] and
            cities_by_queue):
        producer = cities_by_queue[0]
        validation = queue_query(root, owner, producer["object_id"], 50)
        accepted = bool(validation["validation_result"])
        trace.append({
            "stage": "economy.citizen", "source": "Marshal::economy/CapFirst",
            "current_with_queued": citizen_count, "target": target_citizens,
            "producer_id": producer["object_id"], "retail_can_queue": int(accepted),
            "result": "emit" if accepted else "suppressed",
        })
        if accepted:
            supported.append({"verb": "queue", "owner": owner,
                              "producer_id": producer["object_id"], "type_index": 50,
                              "type_name": "Citizen", "count": 1})
    else:
        trace.append({
            "stage": "economy.citizen", "current_with_queued": citizen_count,
            "target": target_citizens, "food_locked": food_locked,
            "result": "suppressed",
        })

    trace.extend([
        {"stage": "scout", "source": "Marshal::do_scout", "result": "unsupported",
         "reason": "nearest-unexplored waypoint requires a fog-safe explored map plane"},
        {"stage": "military", "source": "Marshal::military", "result": "suppressed",
         "reason": ("before Marshal military_from horizon" if observation["frame"] < 2250
                    else "no supported public candidate selected")},
        {"stage": "army_control", "source": "Marshal::army_control", "result": "suppressed",
         "reason": "no own live unit satisfies Arena TypeRow::is_military"},
        {"stage": "employ", "source": "Marshal::employ_except", "result": "unsupported",
         "reason": ("v2 omits exact gather_max needed to allocate a free seat"
                    if protocol.endswith(".v2") else
                    "v3 exposes capacity but not the complete exact free-seat chain")},
    ])

    action = supported[0] if supported else None
    if action and action["verb"] == "queue":
        heads = [23, 0, 0, 0, action["type_index"], 0, 0, 0, 0, action["count"]]
    elif action and action["verb"] == "build":
        heads = [24, action["placement_evidence"]["snapped_x"] // 192,
                 action["placement_evidence"]["snapped_y"] // 192, 0,
                 action["type_index"], 0, 0, 0, 0, 0]
    else:
        heads = None
    return {
        "schema": "don.retail-arena-marshal-plan.v1",
        "protocol": observation["protocol"],
        "policy": "Arena Marshal faithful-supported-subsequence",
        "source": "crates/don-ai/src/arena/bots/marshal.rs Marshal::act",
        "observation_frame": observation["frame"],
        "command_order": ["sense", "economy", "scout", "military", "army_control", "employ"],
        "trace": trace,
        "supported_actions": supported,
        "selected_action": action,
        "selected_don_env_heads": heads,
        "selection_rule": "first supported emitted command in Marshal source order; max one live action",
    }


def validate_economy_action(action: dict, observation: dict, root: str) -> dict:
    owner = observation["player"]["owner"]
    owned = {obj["object_id"]: obj for obj in observation["objects"]}
    if action.get("owner") != owner:
        raise RuntimeError("economy action owner differs from observed human slot")
    if action["verb"] == "queue":
        producer = owned.get(action.get("producer_id"))
        if not producer or producer["category"] != "build" or action.get("count") not in {-1, 1}:
            raise RuntimeError("queue action requires one observed own producer and count +/-1")
        return queue_validation(root, owner, producer["object_id"], action["type_index"])
    if action["verb"] == "gather":
        worker = owned.get(action.get("worker_id"))
        target = owned.get(action.get("target_id"))
        if (not worker or worker["category"] != "unit" or worker["type_index"] not in {50, 51}
                or not target or target["category"] != "build"):
            raise RuntimeError("gather action requires an observed own citizen and own building")
        return {"validation_result": 1, "validation": "retail GroupOut::issue_gather gate"}
    if action["verb"] == "build":
        workers = action.get("worker_ids", [])
        if len(workers) != 1 or workers[0] not in owned or owned[workers[0]]["type_index"] not in {50, 51}:
            raise RuntimeError("build action requires exactly one observed own citizen")
        if action.get("x2") != -1 or action.get("y2") != -1:
            raise RuntimeError("build action requires retail's canonical simple-pick -1 endpoints")
        if action["x1"] % 48 or action["y1"] % 48:
            raise RuntimeError("build action must use the exact 48-Coord UCoord lattice")
        max_x = observation["world"]["tile_xs"] * 192
        max_y = observation["world"]["tile_ys"] * 192
        if not (0 <= action["x1"] < max_x and 0 <= action["y1"] < max_y):
            raise RuntimeError("build action lies outside observed public world bounds")
        public_gate = static_build_legality(action["type_index"], observation)
        if not public_gate["accepted"]:
            return {"validation_result": 0, "public_gate": public_gate,
                    "validation": "necessary public tech/prerequisite/age/base-cost gate"}
        evidence = action.get("placement_evidence")
        revalidation = None
        if evidence:
            revalidation = gather_build_query(
                root, observation, workers[0], action["type_index"],
                evidence["origin"]["x"], evidence["origin"]["y"],
                evidence["ring"], evidence["ring"],
            )
            site = revalidation.get("site") or {}
            if (not revalidation["accepted"] or
                    site.get("x") != action["x1"] or site.get("y") != action["y1"] or
                    site.get("snapped_x") != evidence["snapped_x"] or
                    site.get("snapped_y") != evidence["snapped_y"] or
                    revalidation["capacity"] != evidence["capacity"]):
                return {
                    "validation_result": 0, "public_gate": public_gate,
                    "placement_revalidation": revalidation,
                    "validation": ("prospective full-current-visibility retail site/capacity "
                                   "changed since planning"),
                }
        result = build_validation(root, owner, workers[0], action["x1"], action["y1"],
                                  action["x2"], action["y2"], action["type_index"])
        result["public_gate"] = public_gate
        if revalidation is not None:
            result["placement_revalidation"] = revalidation
        result["validation"] = ("retail GroupData::validate_build plus necessary public "
                                "tech/prerequisite/age/base-cost gate and exact current-fog "
                                "prospective capacity replay")
        return result
    raise RuntimeError(f"unsupported economy verb {action['verb']!r}")


def economy_action_words(action: dict) -> list[str]:
    if action["verb"] == "queue":
        return ["queue", str(action["owner"]), str(action["type_index"]),
                str(action["count"]), str(action["producer_id"])]
    if action["verb"] == "gather":
        return ["gather", str(action["owner"]), str(action["target_id"]), "2",
                str(action["worker_id"])]
    if action["verb"] == "build":
        return ["build", str(action["owner"]), str(action["x1"]), str(action["y1"]),
                str(action["x2"]), str(action["y2"]), str(action["type_index"]),
                str(action.get("queue", 2)), *[str(i) for i in action["worker_ids"]]]
    raise RuntimeError(f"unsupported economy verb {action['verb']!r}")


def observation_identity(observation: dict) -> dict:
    """Fields that must remain stable across one supervised live-player transaction."""
    return {
        "retail_executable_sha256": observation["retail_executable_sha256"],
        "player": {key: observation["player"][key]
                   for key in ["owner", "slot", "who", "tribe", "team"]},
        "world": observation["world"],
    }


def paused_observation_token(observation: dict) -> dict:
    """Complete public own-state token compared between paused plan and apply."""
    return {
        "identity": observation_identity(observation),
        "frame": observation["frame"],
        "paused": observation["paused"],
        "economy": observation["economy"],
        "population": observation["population"],
        "technology": observation["technology"],
        "queued_types": observation["queued_types"],
        "object_slots": observation["object_slots"],
        "object_marks": observation["object_marks"],
        "objects": observation["objects"],
    }


def prove_economy_action(root: str, generation: str, action: dict, output: Path,
                         expected_before: dict | None = None,
                         settlement_frames: int = 30,
                         settlement_limit_frames: int = 180) -> dict:
    if not 1 <= settlement_frames <= 30:
        raise RuntimeError("economy proof settlement boundary must be 1..30 frames")
    if not 1 <= settlement_limit_frames <= 180:
        raise RuntimeError("economy proof settlement limit must be 1..180 frames")
    before = player_observation(root, generation)
    if (expected_before is not None and
            paused_observation_token(before) != paused_observation_token(expected_before)):
        raise RuntimeError("own public state/identity changed between Marshal plan and apply")
    validation = validate_economy_action(action, before, root)
    if not validation.get("validation_result"):
        raise RuntimeError("shipped retail legality predicate rejected economy action")
    command_events = send(economy_action_words(action), 8.0, root)
    queued_event = next((event for event in command_events if event.get("phase") == "queued"), None)
    if not queued_event or not queued_event.get("command_hex"):
        raise RuntimeError("retail did not serialize the economy command")
    command_bytes = bytes.fromhex(queued_event["command_hex"])
    expected_opcode = {"gather": 0x13, "queue": 0x18, "build": 0x19}[action["verb"]]
    packed_length = {"gather": 9, "queue": 9, "build": 25}[action["verb"]]
    # Retail's group prefix is variable-length (a building id crosses a compressed
    # id band).  The issue methods append these fixed-size packed commands to it.
    opcode_offset = len(command_bytes) - packed_length
    if opcode_offset < 0 or command_bytes[opcode_offset] != expected_opcode:
        raise RuntimeError(
            f"retail serialized opcode "
            f"{command_bytes[opcode_offset] if opcode_offset >= 0 else None!r}; "
            f"expected 0x{expected_opcode:02x} before the fixed-size payload"
        )
    settlements: list[dict] = []
    if action["verb"] == "build":
        before_ids = {(obj["object_id"], obj["id"]["uid"]) for obj in before["objects"]}
        after = before
        elapsed = 0
        while elapsed < settlement_limit_frames:
            boundary = min(settlement_frames, settlement_limit_frames - elapsed)
            settlements.append(advance_frames(root, boundary))
            elapsed += boundary
            after = player_observation(root, generation)
            if any(obj["type_index"] == action["type_index"] and
                   (obj["object_id"], obj["id"]["uid"]) not in before_ids
                   for obj in after["objects"] if obj["category"] == "build"):
                break
    else:
        after = player_observation(root, generation)
    if before["paused"] != 1 or after["paused"] != 1:
        raise RuntimeError("economy transaction did not restore its paused boundary")
    if action["verb"] == "build":
        if after["frame"] - before["frame"] != sum(item["requested"] for item in settlements):
            raise RuntimeError("bounded build settlement crossed an unaccounted frame boundary")
    elif before["frame"] != after["frame"]:
        raise RuntimeError("economy transaction escaped its paused zero-sim-frame boundary")
    if action["verb"] == "queue":
        def queued_count(obs: dict) -> int:
            return next((item["count"] for item in obs["queued_types"]
                         if item["type_index"] == action["type_index"]), 0)
        if queued_count(after) - queued_count(before) != action["count"]:
            raise RuntimeError("retail aggregate queued type did not change by requested count")
        before_build = next(obj for obj in before["objects"]
                            if obj["object_id"] == action["producer_id"])
        after_build = next(obj for obj in after["objects"]
                           if obj["object_id"] == action["producer_id"])
        if (after_build["production_queue"]["logical_length"] -
                before_build["production_queue"]["logical_length"] != action["count"]):
            raise RuntimeError("retail producer queue did not change by requested count")
    elif action["verb"] == "gather":
        worker = next(obj for obj in after["objects"] if obj["object_id"] == action["worker_id"])
        if (worker["order"]["kind"] != "GatherOrder" or
                worker["order"].get("own_target", {}).get("object_id") != action["target_id"]):
            raise RuntimeError("retail did not apply the GatherOrder to the own target")
    elif action["verb"] == "build":
        new_builds = [obj for obj in after["objects"] if obj["category"] == "build" and
                      obj["type_index"] == action["type_index"] and
                      (obj["object_id"], obj["id"]["uid"]) not in before_ids]
        if not new_builds:
            raise RuntimeError("retail did not materialize the requested building")
        worker = next(obj for obj in after["objects"]
                      if obj["object_id"] == action["worker_ids"][0])
        target = (worker["order"].get("own_target")
                  if worker["order"]["kind"] == "BuildOrder" else
                  worker["order"].get("queued_build_target", {}))
        if not any(target.get("object_id") == build["object_id"] and
                   target.get("uid") == build["id"]["uid"] for build in new_builds):
            raise RuntimeError("retail did not transition the chosen worker to the new BuildOrder")
    artifact = {
        "schema": "don.retail-economy-action-proof.v1",
        "protocol": before["protocol"],
        "controller_generation": generation,
        "mode": "apply",
        "action": action,
        "retail_validation": validation,
        "retail_command_hex": queued_event["command_hex"],
        "frame_boundary": {"before": before["frame"], "after": after["frame"]},
        "pause_before_after": [before["paused"], after["paused"]],
        "bounded_settlement": settlements,
        "before": before,
        "after": after,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(artifact, indent=2) + "\n")
    return artifact


def recover_build_action_proof(root: str, generation: str, action: dict,
                               after: dict, output: Path) -> dict:
    """Recover a positive proof after an over-strict observer assertion, never reissue."""
    if action.get("verb") != "build" or after.get("paused") != 1:
        raise RuntimeError("recovery accepts only an already-applied paused build")
    raw = guest_cmd(f'if exist "{root}\\events.ndjson" type "{root}\\events.ndjson"',
                    check=False)
    events = []
    for line in raw.splitlines():
        try:
            events.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    build_rows = [(i, event) for i, event in enumerate(events)
                  if event.get("verb") == "build" and event.get("phase") == "queued"]
    if not build_rows:
        raise RuntimeError("recovery found no serialized BUILD_AT")
    build_index, queued_event = build_rows[-1]
    command = bytes.fromhex(queued_event.get("command_hex", ""))
    if len(command) < 25 or command[-25] != 0x19:
        raise RuntimeError("recovery BUILD_AT lacks retail's packed 0x19 payload")
    payload = command[-24:]
    decoded = {
        "x1": int.from_bytes(payload[0:4], "little", signed=True),
        "y1": int.from_bytes(payload[4:8], "little", signed=True),
        "x2": int.from_bytes(payload[8:12], "little", signed=True),
        "y2": int.from_bytes(payload[12:16], "little", signed=True),
        "type_index": int.from_bytes(payload[16:20], "little", signed=True),
        "queue": int.from_bytes(payload[20:24], "little", signed=True),
    }
    expected = {key: action[key] for key in ["x1", "y1", "x2", "y2", "type_index"]}
    expected["queue"] = action.get("queue", 2)
    if decoded != expected:
        raise RuntimeError(f"serialized BUILD_AT differs from planned action: {decoded}")
    before_event = next((event for event in reversed(events[:build_index])
                         if event.get("verb") == "observe-player" and
                         event.get("phase") == "observed" and
                         event.get("frame") == queued_event.get("frame")), None)
    if not before_event:
        raise RuntimeError("recovery found no coherent pre-command observation")
    before = normalize_player_observation(before_event, generation, executable_base(root))
    validation = next((event for event in reversed(events[:build_index])
                       if event.get("verb") == "validate-build" and
                       event.get("validation_result")), None)
    evidence = action["placement_evidence"]
    placement = next((event for event in reversed(events[:build_index])
                      if event.get("verb") == "find-gather-build" and
                      event.get("placement_ring") == evidence["ring"] and
                      event.get("placement_x") == action["x1"] and
                      event.get("placement_y") == action["y1"] and
                      event.get("placement_capacity") == evidence["capacity"]), None)
    if not validation or not placement:
        raise RuntimeError("recovery lacks the same-frame retail validation/capacity replay")
    terminal = next((event for event in events[build_index + 1:]
                     if event.get("verb") == "run-frames" and
                     event.get("phase") == "trace-complete" and
                     event.get("frame") == after["frame"] and event.get("paused") == 1), None)
    if not terminal or after["frame"] - before["frame"] != 30:
        raise RuntimeError("recovery lacks the exact 30-frame paused settlement boundary")
    before_ids = {(obj["object_id"], obj["id"]["uid"]) for obj in before["objects"]}
    new_builds = [obj for obj in after["objects"] if obj["category"] == "build" and
                  obj["type_index"] == action["type_index"] and
                  (obj["object_id"], obj["id"]["uid"]) not in before_ids]
    worker = next(obj for obj in after["objects"]
                  if obj["object_id"] == action["worker_ids"][0])
    target = worker["order"].get("queued_build_target", {})
    if len(new_builds) != 1 or not (
            target.get("object_id") == new_builds[0]["object_id"] and
            target.get("uid") == new_builds[0]["id"]["uid"]):
        raise RuntimeError("recovery did not prove the pending BuildOrder's exact own target")
    artifact = {
        "schema": "don.retail-economy-action-proof.v1",
        "protocol": before["protocol"],
        "controller_generation": generation,
        "mode": "apply",
        "action": action,
        "retail_validation": {
            "validation_result": validation["validation_result"],
            "placement_revalidation": {
                "retail_result": placement["validation_result"],
                "capacity": placement["placement_capacity"],
                "ring": placement["placement_ring"],
                "site": {"x": placement["placement_x"], "y": placement["placement_y"],
                         "snapped_x": placement["placement_snap_x"],
                         "snapped_y": placement["placement_snap_y"]},
            },
        },
        "retail_command_hex": queued_event["command_hex"],
        "frame_boundary": {"before": before["frame"], "after": after["frame"]},
        "pause_before_after": [before["paused"], after["paused"]],
        "bounded_settlement": [{"verb": "run-frames", "requested": 30,
                                "frame_before": before["frame"],
                                "frame_after": after["frame"], "pause_after": 1}],
        "observer_recovery": ("initial proof required BuildOrder at queue front; retail "
                              "correctly retained a front MoveOrder for the distant site, "
                              "then v15 proved the exact pending BuildOrder target"),
        "before": before,
        "after": after,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(artifact, indent=2) + "\n")
    return artifact


def advance_frames(root: str, frames: int, timeout: float = 10.0) -> dict:
    if not 1 <= frames <= 30:
        raise RuntimeError("run-frames boundary must be between 1 and 30")
    seq = next_seq()
    words = ["run-frames", str(frames)]
    validate_words(words)
    line = " ".join([str(seq), *words])
    guest_cmd(
        f'(echo {line})>"{root}\\request.tmp" && '
        f'move /y "{root}\\request.tmp" "{root}\\request.txt" >nul'
    )
    events: list[dict] = []
    seen: set[tuple] = set()
    terminal: dict | None = None
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        raw_events = guest_cmd(
            f'if exist "{root}\\events.ndjson" type "{root}\\events.ndjson"',
            check=False,
        )
        for raw in raw_events.splitlines():
            try:
                event = json.loads(raw)
            except json.JSONDecodeError:
                continue
            if event.get("seq") != seq:
                continue
            key = (event.get("phase"), event.get("frame"), event.get("command_hex"))
            if key in seen:
                continue
            seen.add(key)
            events.append(event)
            if event.get("phase") == "rejected":
                raise RuntimeError(f"retail rejected run-frames (note={event.get('note')})")
            if event.get("phase") in {"trace-complete", "trace-bounded"}:
                terminal = event
        if terminal:
            break
        time.sleep(0.05)
    queued = next((event for event in events if event.get("phase") == "queued"), None)
    if not queued or not terminal:
        raise RuntimeError("run-frames did not reach its supervised terminal boundary")
    if (terminal.get("paused") != 1 or terminal["frame"] - queued["frame"] != frames or
            terminal.get("phase") != "trace-complete"):
        raise RuntimeError("run-frames stopped outside its exact frame/pause boundary")
    return {
        "verb": "run-frames",
        "requested": frames,
        "frame_before": queued["frame"],
        "frame_after": terminal["frame"],
        "pause_after": terminal["paused"],
        "unpause_command_hex": queued["command_hex"],
    }


def economy_policy_run(root: str, generation: str, output: Path, apply: bool) -> None:
    failure: BaseException | None = None
    try:
        observation = player_observation(root, generation)
        plan = conservative_opening_policy(observation, root)
        artifact: dict = {
            "schema": "don.retail-economy-policy-run.v1",
            "protocol": observation["protocol"],
            "mode": "apply" if apply else "dry-run",
            "observation": observation,
            "plan": plan,
            "proof": None,
        }
        if apply and plan["action"]:
            artifact["proof"] = prove_economy_action(root, generation, plan["action"],
                                                      output.with_name("retail-economy-action-proof-v1.json"))
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(artifact, indent=2) + "\n")
        print(json.dumps(plan, indent=2))
        print(f"wrote economy policy run to {output}")
    except BaseException as exc:
        failure = exc
    finally:
        try:
            stop(root)
        except BaseException as exc:
            if failure is None:
                failure = exc
    if failure is not None:
        raise failure


def arena_marshal_policy_run(root: str, generation: str, output: Path, apply: bool) -> None:
    failure: BaseException | None = None
    try:
        observation = player_observation(root, generation)
        plan = arena_marshal_extracted_plan(observation, root)
        proof_summary = None
        if apply and plan["selected_action"]:
            proof_name = ("retail-arena-marshal-camp-action-proof-v1.json"
                          if observation["protocol"] == "don.retail-player.v3" else
                          "retail-arena-marshal-action-proof-v1.json")
            proof_path = output.with_name(proof_name)
            proof = prove_economy_action(root, generation, plan["selected_action"], proof_path)
            proof_summary = {
                "artifact": proof_path.name,
                "schema": proof["schema"],
                "action": proof["action"],
                "retail_command_hex": proof["retail_command_hex"],
                "frame_boundary": proof.get("frame_boundary", {
                    "before": proof["before"]["frame"], "after": proof["after"]["frame"]}),
                "pause_before_after": proof["pause_before_after"],
            }
        artifact = {
            "schema": "don.retail-arena-marshal-run.v1",
            "protocol": observation["protocol"],
            "controller_generation": generation,
            "mode": "apply" if apply else "dry-run",
            "observation_summary": {
                "frame": observation["frame"], "paused": observation["paused"],
                "population": observation["population"],
                "queued_types": observation["queued_types"],
                "owned_type_counts": {
                    str(type_index): sum(1 for obj in observation["objects"]
                                         if obj["type_index"] == type_index)
                    for type_index in sorted({obj["type_index"] for obj in observation["objects"]})
                },
            },
            "plan": plan,
            "proof": proof_summary,
        }
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(artifact, indent=2) + "\n")
        print(json.dumps(plan, indent=2))
        print(f"wrote Arena Marshal {'apply' if apply else 'dry-run'} to {output}")
    except BaseException as exc:
        failure = exc
    finally:
        try:
            stop(root)
        except BaseException as exc:
            if failure is None:
                failure = exc
    if failure is not None:
        raise failure


def arena_marshal_supervised_loop(root: str, generation: str, output: Path,
                                  decisions: int, frames_per_decision: int,
                                  apply: bool) -> None:
    """Finite observe/plan/apply/advance/reobserve loop over proven retail verbs."""
    if not 1 <= decisions <= 8:
        raise RuntimeError("Marshal loop requires 1..8 bounded decisions")
    if not 1 <= frames_per_decision <= 30:
        raise RuntimeError("Marshal loop frame boundary must be 1..30")
    if not apply and decisions != 1:
        raise RuntimeError("dry-run Marshal loop is one decision; repeated decisions require --apply")
    artifact: dict = {
        "schema": "don.retail-arena-marshal-supervised-loop.v1",
        "protocol": "don.retail-player.v3",
        "controller_generation": generation,
        "mode": "apply" if apply else "dry-run",
        "requested_decisions": decisions,
        "frames_per_decision": frames_per_decision,
        "status": "running",
        "safety": {
            "max_actions_per_decision": 1,
            "proven_action_verbs": ["queue", "build"],
            "unsupported_action": "explicit no-op",
            "fog": "retail-player.v3 own-state only; placement oracle is current-fog gated",
            "identity": "exact executable/player/world every decision; same-frame object uid token before apply",
            "pause": "every decision begins and ends paused",
            "stop": "STOP restores the original five retail call-site bytes on every exit",
        },
        "decisions": [],
        "status_detail": None,
        "parked_ready_record": None,
    }
    output.parent.mkdir(parents=True, exist_ok=True)

    def checkpoint() -> None:
        output.write_text(json.dumps(artifact, indent=2) + "\n")

    failure: BaseException | None = None
    stable_identity: dict | None = None
    try:
        for index in range(decisions):
            step: dict = {"index": index, "status": "observing"}
            artifact["decisions"].append(step)
            checkpoint()
            before = player_observation(root, generation)
            identity = observation_identity(before)
            if stable_identity is None:
                stable_identity = identity
                artifact["stable_identity"] = identity
            elif identity != stable_identity:
                raise RuntimeError("retail executable/player/world identity changed between decisions")
            step["before"] = before
            step["status"] = "planning"
            checkpoint()

            plan = arena_marshal_extracted_plan(before, root)
            action = plan["selected_action"]
            step["plan"] = plan
            step["selected_action"] = action
            step["action_mode"] = (
                "apply" if action and action.get("verb") in {"queue", "build"} and apply else
                "dry-run" if action and not apply else
                "no-op-unsupported" if action else "no-op-no-supported-action"
            )
            step["status"] = "planned"
            checkpoint()

            proof = None
            advance = None
            proven = action and action.get("verb") in {"queue", "build"}
            if apply and proven:
                proof_path = output.with_name(
                    f"{output.stem}-step-{index:02d}-action-proof.json"
                )
                proof = prove_economy_action(root, generation, action, proof_path,
                                              expected_before=before,
                                              settlement_frames=frames_per_decision,
                                              settlement_limit_frames=frames_per_decision)
                step["proof"] = {
                    "artifact": proof_path.name,
                    "schema": proof["schema"],
                    "retail_validation": proof["retail_validation"],
                    "retail_command_hex": proof["retail_command_hex"],
                    "frame_boundary": proof["frame_boundary"],
                    "pause_before_after": proof["pause_before_after"],
                }
                checkpoint()
                if action["verb"] == "build":
                    delta = proof["after"]["frame"] - proof["before"]["frame"]
                    if delta != frames_per_decision:
                        raise RuntimeError("BUILD_AT proof crossed a non-decision frame boundary")
                    advance = {
                        "source": "bounded build settlement",
                        "requested": frames_per_decision,
                        "frame_before": proof["before"]["frame"],
                        "frame_after": proof["after"]["frame"],
                        "pause_after": proof["after"]["paused"],
                        "boundaries": proof["bounded_settlement"],
                    }
                else:
                    advance = advance_frames(root, frames_per_decision)
            elif apply:
                # Unsupported or absent commands are literal no-ops. Time still advances
                # to the next finite decision horizon; no substitute command is issued.
                advance = advance_frames(root, frames_per_decision)

            if apply:
                after = player_observation(root, generation)
                if after["frame"] != before["frame"] + frames_per_decision:
                    raise RuntimeError("Marshal decision escaped its exact frame horizon")
                if observation_identity(after) != stable_identity:
                    raise RuntimeError("retail executable/player/world identity changed after action")
                if before["paused"] != 1 or after["paused"] != 1:
                    raise RuntimeError("Marshal decision escaped its paused boundaries")
            else:
                after = before
            step["advance"] = advance
            step["after"] = after
            step["invariants"] = {
                "identity_stable": observation_identity(after) == stable_identity,
                "pause_before_after": [before["paused"], after["paused"]],
                "frame_delta": after["frame"] - before["frame"],
                "actions_applied": 1 if apply and proven else 0,
                "unsupported_substitution": False,
            }
            step["status"] = "complete"
            checkpoint()
        artifact["status"] = "complete"
        artifact["status_detail"] = f"completed {decisions} finite decisions"
    except BaseException as exc:
        failure = exc
        artifact["status"] = "failed"
        artifact["status_detail"] = f"{type(exc).__name__}: {exc}"
    finally:
        try:
            send(["pause", "1"], 5.0, root)
        except BaseException as exc:
            if failure is None:
                failure = exc
                artifact["status"] = "failed"
                artifact["status_detail"] = f"pause restore failed: {exc}"
        try:
            stop(root)
            artifact["parked_ready_record"] = guest_cmd(
                f'if exist "{root}\\ready.txt" type "{root}\\ready.txt"', check=False
            )
        except BaseException as exc:
            if failure is None:
                failure = exc
                artifact["status"] = "failed"
                artifact["status_detail"] = f"STOP restore failed: {exc}"
        checkpoint()
    if failure is not None:
        raise failure


def economy_action_command(root: str, generation: str, action: dict, output: Path) -> None:
    failure: BaseException | None = None
    try:
        prove_economy_action(root, generation, action, output)
        print(f"wrote bounded economy proof to {output}")
    except BaseException as exc:
        failure = exc
    finally:
        try:
            stop(root)
        except BaseException as exc:
            if failure is None:
                failure = exc
    if failure is not None:
        raise failure


def placement_query_command(root: str, generation: str, worker_id: int,
                            type_index: int, radius: int, output: Path) -> None:
    failure: BaseException | None = None
    try:
        observation = player_observation(root, generation)
        public_gate = static_build_legality(type_index, observation)
        query = find_build_site(root, observation, worker_id, type_index, radius)
        artifact = {
            "schema": "don.retail-build-placement-proof.v1",
            "controller_generation": generation,
            "retail_executable_sha256": EXPECTED_SHA256,
            "mode": "validation-only",
            "public_gate": public_gate,
            "query": query,
            "observation": observation,
        }
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(artifact, indent=2) + "\n")
        print(json.dumps(query, indent=2))
        print(f"wrote bounded placement query to {output}")
    except BaseException as exc:
        failure = exc
    finally:
        try:
            stop(root)
        except BaseException as exc:
            if failure is None:
                failure = exc
    if failure is not None:
        raise failure


def trajectory(owner: int, unit_id: int, x: int, y: int, max_frames: int,
               timeout: float, root: str, generation: str, output: Path) -> None:
    initial_events = send(["observe"], 5.0, root)
    initial = next((e for e in initial_events if e.get("phase") == "observed"), None)
    if not initial or initial.get("paused") != 1:
        raise SystemExit("REFUSING trace: retail must begin paused so the run is bounded")
    seq = next_seq()
    words = ["trace-move", str(owner), str(unit_id), str(x), str(y), str(max_frames)]
    validate_words(words)
    line = " ".join([str(seq), *words])
    guest_cmd(
        f'(echo {line})>"{root}\\request.tmp" && '
        f'move /y "{root}\\request.tmp" "{root}\\request.txt" >nul'
    )
    events: list[dict] = []
    seen: set[tuple] = set()
    terminal: dict | None = None
    failure: BaseException | None = None
    try:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            raw_events = guest_cmd(
                f'if exist "{root}\\events.ndjson" type "{root}\\events.ndjson"',
                check=False,
            )
            for raw in raw_events.splitlines():
                try:
                    event = json.loads(raw)
                except json.JSONDecodeError:
                    continue
                if event.get("seq") != seq:
                    continue
                key = (event.get("phase"), event.get("frame"), event.get("tick"),
                       event.get("command_hex"))
                if key in seen:
                    continue
                seen.add(key)
                events.append(event)
                print(json.dumps(event, sort_keys=True))
                if event.get("phase") == "rejected":
                    raise RuntimeError(f"retail rejected bounded trace (note={event.get('note')})")
                if event.get("phase") in {"trace-complete", "trace-bounded"}:
                    terminal = event
            if terminal:
                break
            time.sleep(0.05)
        if not terminal:
            raise TimeoutError("trajectory did not publish a bounded terminal record")
        queued = next((e for e in events if e.get("phase") == "queued"), None)
        if not queued:
            raise RuntimeError("trace completed without its initial queued-state record")

        base = int(initial["game"], 16)  # replaced below by the executable base from ready.txt
        ready = guest_cmd(f'type "{root}\\ready.txt"')
        for ready_line in ready.splitlines():
            if ready_line.startswith("base="):
                base = int(ready_line.split("=", 1)[1], 16)
        samples = [
            normalized_trace_event(e, queued["frame"], base)
            for e in events if e.get("phase") == "trace-sample"
        ]
        artifact = {
            "schema": "don.retail-move-trajectory.v1",
            "retail_executable_sha256": EXPECTED_SHA256,
            "controller_generation": generation,
            "coordinate_units_per_tile": 192,
            "simulation_frames_per_game_second": 15,
            "angle_encoding": "u32 binary angle; 2^32 is one turn; 0 points north/-y",
            "object_coordinate_encoding": "decoded from stored_u32 XOR 0x00063637",
            "subject": {"owner": owner, "object_id": unit_id},
            "request": {"x": x, "y": y, "max_frames": max_frames},
            "initial": normalized_trace_event(queued, queued["frame"], base),
            "retail_command_hex": queued["command_hex"],
            "termination": terminal["phase"],
            "terminal": normalized_trace_event(terminal, queued["frame"], base),
            "checksum": {
                "available": False,
                "reason": "retail network flag was 0; solo intentionally emits no checksum packet",
            },
            "samples": samples,
        }
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(artifact, indent=2) + "\n")
        print(f"wrote {len(samples)} samples to {output}")
    except BaseException as exc:
        failure = exc
    finally:
        # The recorder owns both fail-safes: restore the initial pause state, then
        # restore retail's original five call-site bytes and park this generation.
        try:
            send(["pause", "1"], 5.0, root)
        except BaseException as exc:
            if failure is None:
                failure = exc
        try:
            stop(root)
        except BaseException as exc:
            if failure is None:
                failure = exc
    if failure is not None:
        raise failure


def player_observe_command(root: str, generation: str, output: Path) -> None:
    failure: BaseException | None = None
    try:
        observation = player_observation(root, generation)
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(observation, indent=2) + "\n")
        print(f"wrote {len(observation['objects'])} own public objects to {output}")
    except BaseException as exc:
        failure = exc
    finally:
        try:
            stop(root)
        except BaseException as exc:
            if failure is None:
                failure = exc
    if failure is not None:
        raise failure


def policy_run(root: str, generation: str, output: Path, trace_output: Path,
               apply: bool) -> None:
    failure: BaseException | None = None
    observation: dict | None = None
    batch: dict | None = None
    traces: list[dict] = []
    after: dict | None = None
    try:
        observation = player_observation(root, generation)
        batch = scout_policy(observation)
        validate_action_batch(batch, observation)
        print(json.dumps(batch, indent=2))
        if apply:
            for index, action in enumerate(batch["actions"]):
                if index:
                    rearm(root)
                action_trace = trace_output if len(batch["actions"]) == 1 else trace_output.with_name(
                    f"{trace_output.stem}-{index}{trace_output.suffix}"
                )
                trajectory(
                    action["owner"], action["object_ids"][0],
                    action["target"]["x"], action["target"]["y"],
                    action["max_frames"], 45.0, root, generation, action_trace,
                )
                traces.append(json.loads(action_trace.read_text()))
            if batch["actions"]:
                rearm(root)
                after = player_observation(root, generation)
            else:
                after = observation
        artifact = {
            "schema": "don.retail-player-policy-run.v1",
            "protocol": "don.retail-player.v1",
            "mode": "apply" if apply else "dry-run",
            "before": observation,
            "action_batch": batch,
            "traces": traces,
            "after": after,
        }
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(artifact, indent=2) + "\n")
        print(f"wrote supervised policy run to {output}")
    except BaseException as exc:
        failure = exc
    finally:
        try:
            stop(root)
        except BaseException as exc:
            if failure is None:
                failure = exc
    if failure is not None:
        raise failure


def status(root: str) -> None:
    target_pid = pid()
    print(f"pid={target_pid}")
    print(guest_cmd(f'tasklist /v /fi "pid eq {target_pid}"'))
    print(guest_cmd(f'if exist "{root}\\ready.txt" type "{root}\\ready.txt"',
                    check=False))


def stop(root: str) -> None:
    guest_cmd(f'(echo stop)>"{root}\\STOP"')
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        ready = guest_cmd(f'type "{root}\\ready.txt"', check=False)
        if "state=parked" in ready:
            print(ready)
            return
        time.sleep(0.05)
    raise SystemExit("STOP written but hook did not report parked")


def rearm(root: str) -> None:
    guest_cmd(f'del /q "{root}\\STOP"')
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        ready = guest_cmd(f'type "{root}\\ready.txt"', check=False)
        if "state=armed" in ready:
            print(ready)
            return
        time.sleep(0.05)
    raise SystemExit("STOP removed but hook did not report armed")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    sub = ap.add_subparsers(dest="action", required=True)
    def add_generation(parser: argparse.ArgumentParser, default: str = DEFAULT_GENERATION) -> None:
        parser.add_argument("--generation", default=default)

    status_parser = sub.add_parser("status")
    add_generation(status_parser)
    sub.add_parser("build")
    d = sub.add_parser("deploy")
    d.add_argument("--pid", type=int)
    d.add_argument("--port", type=int, default=18082)
    add_generation(d)
    u = sub.add_parser("upgrade")
    u.add_argument("--pid", type=int)
    u.add_argument("--port", type=int, default=18082)
    u.add_argument("--from-generation", default=LEGACY_GENERATION)
    add_generation(u)
    s = sub.add_parser("send")
    s.add_argument("--timeout", type=float, default=5.0)
    add_generation(s)
    s.add_argument("command", nargs=argparse.REMAINDER)
    t = sub.add_parser("trajectory")
    t.add_argument("owner", type=int)
    t.add_argument("unit_id", type=int)
    t.add_argument("x", type=int)
    t.add_argument("y", type=int)
    t.add_argument("--max-frames", type=int, default=120)
    t.add_argument("--timeout", type=float, default=45.0)
    t.add_argument("--output", type=Path,
                   default=HERE.parents[1] / "schema/live/retail-move-trajectory-v1.json")
    add_generation(t)
    po = sub.add_parser("player-observe")
    po.add_argument("--output", type=Path,
                    default=HERE.parents[1] / "schema/live/retail-player-observation-v3.json")
    add_generation(po)
    pol = sub.add_parser("policy")
    pol.add_argument("--apply", action="store_true")
    pol.add_argument("--output", type=Path,
                     default=HERE.parents[1] / "schema/live/retail-player-policy-run-v1.json")
    pol.add_argument("--trace-output", type=Path,
                     default=HERE.parents[1] / "schema/live/retail-player-scout-trace-v1.json")
    add_generation(pol)
    ep = sub.add_parser("economy-policy")
    ep.add_argument("--apply", action="store_true")
    ep.add_argument("--output", type=Path,
                    default=HERE.parents[1] / "schema/live/retail-economy-policy-run-v1.json")
    add_generation(ep)
    mp = sub.add_parser("marshal-policy")
    mp.add_argument("--apply", action="store_true")
    mp.add_argument("--output", type=Path,
                    default=HERE.parents[1] / "schema/live/retail-arena-marshal-run-v1.json")
    add_generation(mp)
    ml = sub.add_parser("marshal-loop")
    ml.add_argument("--apply", action="store_true")
    ml.add_argument("--decisions", type=int, default=1)
    ml.add_argument("--frames-per-decision", type=int, default=30)
    ml.add_argument("--output", type=Path,
                    default=HERE.parents[1] /
                    "schema/live/retail-arena-marshal-supervised-loop-v1.json")
    add_generation(ml)
    ea = sub.add_parser("economy-action")
    ea.add_argument("verb", choices=["queue", "gather", "build"])
    ea.add_argument("--owner", type=int, default=0)
    ea.add_argument("--producer-id", type=int)
    ea.add_argument("--worker-id", type=int)
    ea.add_argument("--target-id", type=int)
    ea.add_argument("--type-index", type=int)
    ea.add_argument("--count", type=int, default=1)
    ea.add_argument("--x1", type=int)
    ea.add_argument("--y1", type=int)
    ea.add_argument("--x2", type=int)
    ea.add_argument("--y2", type=int)
    ea.add_argument("--output", type=Path,
                    default=HERE.parents[1] / "schema/live/retail-economy-action-proof-v1.json")
    add_generation(ea)
    pq = sub.add_parser("placement-query")
    pq.add_argument("--worker-id", type=int, required=True)
    pq.add_argument("--type-index", type=int, required=True)
    pq.add_argument("--radius", type=int, default=8)
    pq.add_argument("--output", type=Path,
                    default=HERE.parents[1] / "schema/live/retail-build-placement-proof-v1.json")
    add_generation(pq)
    stop_parser = sub.add_parser("stop")
    add_generation(stop_parser)
    rearm_parser = sub.add_parser("rearm")
    add_generation(rearm_parser)
    a = ap.parse_args()
    if a.action == "status": status(generation_root(a.generation))
    elif a.action == "build": build()
    elif a.action == "deploy": deploy(a.pid or pid(), a.port, a.generation)
    elif a.action == "upgrade":
        target_pid = a.pid or pid()
        stop(generation_root(a.from_generation))
        deploy(target_pid, a.port, a.generation)
    elif a.action == "send": send(a.command, a.timeout, generation_root(a.generation))
    elif a.action == "trajectory":
        trajectory(a.owner, a.unit_id, a.x, a.y, a.max_frames, a.timeout,
                   generation_root(a.generation), a.generation, a.output.resolve())
    elif a.action == "player-observe":
        player_observe_command(generation_root(a.generation), a.generation,
                               a.output.resolve())
    elif a.action == "policy":
        policy_run(generation_root(a.generation), a.generation, a.output.resolve(),
                   a.trace_output.resolve(), a.apply)
    elif a.action == "economy-policy":
        economy_policy_run(generation_root(a.generation), a.generation, a.output.resolve(),
                           a.apply)
    elif a.action == "economy-action":
        if a.verb == "queue":
            if a.producer_id is None or a.type_index is None:
                ap.error("economy-action queue requires --producer-id and --type-index")
            action = {"verb": "queue", "owner": a.owner, "producer_id": a.producer_id,
                      "type_index": a.type_index, "type_name": type_names().get(a.type_index),
                      "count": a.count}
        elif a.verb == "gather":
            if a.worker_id is None or a.target_id is None:
                ap.error("economy-action gather requires --worker-id and --target-id")
            action = {"verb": "gather", "owner": a.owner, "worker_id": a.worker_id,
                      "target_id": a.target_id}
        else:
            if (a.worker_id is None or a.type_index is None or None in
                    {a.x1, a.y1, a.x2, a.y2}):
                ap.error("economy-action build requires worker/type/x1/y1/x2/y2")
            action = {"verb": "build", "owner": a.owner, "worker_ids": [a.worker_id],
                      "type_index": a.type_index, "type_name": type_names().get(a.type_index),
                      "x1": a.x1, "y1": a.y1, "x2": a.x2, "y2": a.y2, "queue": 2}
        economy_action_command(generation_root(a.generation), a.generation, action,
                               a.output.resolve())
    elif a.action == "placement-query":
        placement_query_command(generation_root(a.generation), a.generation,
                                a.worker_id, a.type_index, a.radius, a.output.resolve())
    elif a.action == "marshal-policy":
        arena_marshal_policy_run(generation_root(a.generation), a.generation,
                                 a.output.resolve(), a.apply)
    elif a.action == "marshal-loop":
        arena_marshal_supervised_loop(
            generation_root(a.generation), a.generation, a.output.resolve(),
            a.decisions, a.frames_per_decision, a.apply,
        )
    elif a.action == "stop": stop(generation_root(a.generation))
    elif a.action == "rearm": rearm(generation_root(a.generation))


if __name__ == "__main__":
    main()
