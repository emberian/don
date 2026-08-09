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
               "queue", "build", "run-frames"}
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
        elif category == "build":
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
        "schema": "don.retail-player-observation.v2",
        "protocol": "don.retail-player.v2",
        "retail_executable_sha256": EXPECTED_SHA256,
        "controller_generation": generation,
        "public_scope": {
            "owner": event["local_player"],
            "includes": ["own active object bands", "own stockpile", "own commerce cap",
                         "own population", "public game clock"],
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
                   "automatic build placement is fail-closed until the exact four-coordinate "
                   "retail placement gesture has a positive live oracle"),
        "action": None,
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
        return build_validation(root, owner, workers[0], action["x1"], action["y1"],
                                action["x2"], action["y2"], action["type_index"])
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


def prove_economy_action(root: str, generation: str, action: dict, output: Path) -> dict:
    before = player_observation(root, generation)
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
        for _ in range(6):
            settlements.append(advance_frames(root, 30))
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
        if not any(obj["type_index"] == action["type_index"] and
                   (obj["object_id"], obj["id"]["uid"]) not in before_ids
                   for obj in after["objects"] if obj["category"] == "build"):
            raise RuntimeError("retail did not materialize the requested building")
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
                    default=HERE.parents[1] / "schema/live/retail-player-observation-v2.json")
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
    elif a.action == "stop": stop(generation_root(a.generation))
    elif a.action == "rearm": rearm(generation_root(a.generation))


if __name__ == "__main__":
    main()
