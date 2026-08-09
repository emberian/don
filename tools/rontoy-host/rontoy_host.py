#!/usr/bin/env python3
"""RoNtoy loopback snapshot host and deterministic economy advisor.

The module intentionally uses only the Python standard library.  It can be imported
for tests or launched through ``server.py``.
"""

from __future__ import annotations

import argparse
import copy
import dataclasses
import hmac
import ipaddress
import json
import math
import pathlib
import re
import secrets
import signal
import threading
import time
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any, Callable, Mapping, Optional
from urllib.parse import urlsplit


SCHEMA_VERSION = 1
ADVISOR_VERSION = 1
DEFAULT_PORT = 17360
DEFAULT_MAX_BODY_BYTES = 64 * 1024
DEFAULT_MIN_INTERVAL_MS = 50
DEFAULT_HEARTBEAT_SECONDS = 15.0
DEFAULT_STALE_AFTER_MS = 3000
DEFAULT_MAX_CONNECTIONS = 32
MAX_RATE_AGE_FRAMES_FOR_ADVICE = 45
# Retail indices: 0 food, 1 timber, 2 wealth, 3 knowledge, 4 metal, 5 oil.
RESOURCE_NAMES = ("food", "timber", "wealth", "knowledge", "metal", "oil")
SESSION_ID_RE = re.compile(r"^[A-Za-z0-9_.:-]{1,128}$")
WEB_PUBLIC_ROOT = pathlib.Path(__file__).resolve().parents[2] / "web" / "public"
DONFEED_SCHEMA = "rontoy.observation"
DONFEED_VERSION = {"major": 1, "minor": 0}
SUPPORTED_ENTRY_RVA = 0x0015_D699
SUPPORTED_IMAGE_SIZE = 0x00BB_4000
SUPPORTED_MODULE_SIZE = 9_925_120
SUPPORTED_MODULE_SHA256 = "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079"
DONFEED_REQUIRED_COMPONENTS = (1 << 0) | (1 << 3) | (1 << 4)  # Game, Leaders, Econ.


class AdmissionError(ValueError):
    """A snapshot was not safe or coherent enough to admit."""

    def __init__(self, code: str, message: str, status: int = 422) -> None:
        super().__init__(message)
        self.code = code
        self.status = status


def _reject_constant(value: str) -> None:
    raise AdmissionError("invalid_json", f"non-finite JSON number is not allowed: {value}", 400)


def _unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise AdmissionError("duplicate_key", f"duplicate JSON key: {key}", 400)
        result[key] = value
    return result


def parse_snapshot_json(raw: bytes, max_body_bytes: int = DEFAULT_MAX_BODY_BYTES) -> dict[str, Any]:
    value = parse_json_object(raw, max_body_bytes)
    validate_snapshot(value)
    return value


def parse_json_object(raw: bytes, max_body_bytes: int = DEFAULT_MAX_BODY_BYTES) -> dict[str, Any]:
    if not raw:
        raise AdmissionError("empty_body", "request body is empty", 400)
    if len(raw) > max_body_bytes:
        raise AdmissionError("body_too_large", f"snapshot exceeds {max_body_bytes} bytes", 413)
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise AdmissionError("invalid_utf8", "snapshot must be UTF-8", 400) from exc
    try:
        value = json.loads(text, parse_constant=_reject_constant, object_pairs_hook=_unique_object)
    except AdmissionError:
        raise
    except (ValueError, RecursionError) as exc:
        raise AdmissionError("invalid_json", "request body is not valid JSON", 400) from exc
    if not isinstance(value, dict):
        raise AdmissionError("invalid_schema", "snapshot root must be an object")
    return value


def _object(value: Any, path: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise AdmissionError("invalid_schema", f"{path} must be an object")
    return value


def _exact_keys(value: Mapping[str, Any], required: set[str], optional: set[str], path: str) -> None:
    missing = required - value.keys()
    if missing:
        raise AdmissionError("invalid_schema", f"{path} is missing: {', '.join(sorted(missing))}")
    unknown = value.keys() - required - optional
    if unknown:
        raise AdmissionError("invalid_schema", f"{path} has unknown fields: {', '.join(sorted(unknown))}")


def _integer(value: Any, path: str, minimum: int, maximum: int) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or not minimum <= value <= maximum:
        raise AdmissionError("invalid_schema", f"{path} must be an integer in [{minimum}, {maximum}]")
    return value


def _number(value: Any, path: str, minimum: float, maximum: float) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise AdmissionError("invalid_schema", f"{path} must be a number")
    number = float(value)
    if not math.isfinite(number) or not minimum <= number <= maximum:
        raise AdmissionError("invalid_schema", f"{path} must be finite and in [{minimum}, {maximum}]")
    return number


def _string(value: Any, path: str, minimum: int = 1, maximum: int = 256) -> str:
    if not isinstance(value, str) or not minimum <= len(value) <= maximum:
        raise AdmissionError("invalid_schema", f"{path} must be a string of {minimum}..{maximum} characters")
    return value


def _integer_array(value: Any, path: str, length: int, minimum: int, maximum: int) -> list[int]:
    if not isinstance(value, list) or len(value) != length:
        raise AdmissionError("invalid_observation", f"{path} must contain exactly {length} integers")
    return [_integer(item, f"{path}[{index}]", minimum, maximum) for index, item in enumerate(value)]


def validate_snapshot(snapshot: Mapping[str, Any]) -> None:
    """Validate the complete v1 schema without mutating the supplied object."""

    _exact_keys(
        snapshot,
        {"schema_version", "source", "capture", "game", "economy"},
        {"goals", "notes"},
        "snapshot",
    )
    if snapshot["schema_version"] != SCHEMA_VERSION or isinstance(snapshot["schema_version"], bool):
        raise AdmissionError("unsupported_schema", f"schema_version must be {SCHEMA_VERSION}")

    source = _object(snapshot["source"], "source")
    _exact_keys(
        source,
        {
            "session_id",
            "sequence",
            "captured_at_ms",
            "process_id",
            "process_started_100ns",
            "module_sha256",
            "module_size",
            "image_entry_rva",
            "image_size",
        },
        {"reader_version"},
        "source",
    )
    session_id = _string(source["session_id"], "source.session_id", 1, 128)
    if not SESSION_ID_RE.fullmatch(session_id):
        raise AdmissionError("invalid_schema", "source.session_id contains unsupported characters")
    _integer(source["sequence"], "source.sequence", 0, (1 << 63) - 1)
    _integer(source["captured_at_ms"], "source.captured_at_ms", 0, (1 << 63) - 1)
    _integer(source["process_id"], "source.process_id", 1, (1 << 32) - 1)
    process_started = _string(source["process_started_100ns"], "source.process_started_100ns", 1, 20)
    if (
        not process_started.isascii()
        or not process_started.isdigit()
        or not 0 < int(process_started) <= (1 << 64) - 1
    ):
        raise AdmissionError("invalid_schema", "source.process_started_100ns must be a nonzero decimal u64 string")
    module_sha256 = _string(source["module_sha256"], "source.module_sha256", 64, 64)
    if not re.fullmatch(r"[0-9a-f]{64}", module_sha256):
        raise AdmissionError("invalid_schema", "source.module_sha256 must be 64 lowercase hexadecimal characters")
    _integer(source["module_size"], "source.module_size", 1, 1 << 40)
    _integer(source["image_entry_rva"], "source.image_entry_rva", 1, (1 << 32) - 1)
    _integer(source["image_size"], "source.image_size", 1, (1 << 32) - 1)
    if "reader_version" in source:
        _string(source["reader_version"], "source.reader_version", 1, 64)

    capture = _object(snapshot["capture"], "capture")
    _exact_keys(
        capture,
        {"frame_start", "frame_end", "complete", "advice_allowed", "duration_us", "read_count", "bytes_read"},
        set(),
        "capture",
    )
    frame_start = _integer(capture["frame_start"], "capture.frame_start", 0, (1 << 63) - 1)
    frame_end = _integer(capture["frame_end"], "capture.frame_end", 0, (1 << 63) - 1)
    if not isinstance(capture["complete"], bool):
        raise AdmissionError("invalid_schema", "capture.complete must be a boolean")
    if not isinstance(capture["advice_allowed"], bool):
        raise AdmissionError("invalid_schema", "capture.advice_allowed must be a boolean")
    _integer(capture["duration_us"], "capture.duration_us", 0, 60_000_000)
    _integer(capture["read_count"], "capture.read_count", 1, 1_000_000)
    _integer(capture["bytes_read"], "capture.bytes_read", 1, 64 * 1024 * 1024)
    if not capture["complete"] or frame_start != frame_end:
        raise AdmissionError("incoherent_capture", "capture must be complete and start/end on the same game frame")

    game = _object(snapshot["game"], "game")
    _exact_keys(
        game,
        {"frame", "player_id", "mode", "paused", "human_count", "human_selection_basis"},
        {"age", "match_name", "seconds"},
        "game",
    )
    game_frame = _integer(game["frame"], "game.frame", 0, (1 << 63) - 1)
    if game_frame != frame_end:
        raise AdmissionError("incoherent_capture", "game.frame must equal the coherent capture frame")
    _integer(game["player_id"], "game.player_id", 0, 15)
    if game["mode"] not in {"single_player", "multiplayer", "unknown"}:
        raise AdmissionError("invalid_schema", "game.mode must be single_player, multiplayer, or unknown")
    _integer(game["human_count"], "game.human_count", 0, 16)
    if game["human_selection_basis"] != "unique_active_in_play_console_flags":
        raise AdmissionError(
            "invalid_schema", "game.human_selection_basis must be unique_active_in_play_console_flags"
        )
    if "age" in game:
        _integer(game["age"], "game.age", 0, 8)
    if game["paused"] is not None and not isinstance(game["paused"], bool):
        raise AdmissionError("invalid_schema", "game.paused must be a boolean or null")
    if "seconds" in game:
        _integer(game["seconds"], "game.seconds", 0, (1 << 32) - 1)
    if "match_name" in game:
        _string(game["match_name"], "game.match_name", 1, 128)

    economy = _object(snapshot["economy"], "economy")
    _exact_keys(economy, {"resources", "population", "rate_sample"}, {"production"}, "economy")
    rate_sample = _object(economy["rate_sample"], "economy.rate_sample")
    _exact_keys(rate_sample, {"basis", "gather_stamp_raw", "age_frames", "confidence"}, set(), "economy.rate_sample")
    if rate_sample["basis"] not in {"engine_direct_gather_cache", "sampled_stock_delta", "unavailable"}:
        raise AdmissionError("invalid_schema", "economy.rate_sample.basis is unsupported")
    gather_stamp = rate_sample["gather_stamp_raw"]
    if gather_stamp is not None:
        _integer(gather_stamp, "economy.rate_sample.gather_stamp_raw", 0, (1 << 32) - 1)
    claimed_rate_age_frames = _integer(rate_sample["age_frames"], "economy.rate_sample.age_frames", 0, 60 * 60 * 15)
    if rate_sample["confidence"] not in {"direct", "estimated", "unavailable"}:
        raise AdmissionError("invalid_schema", "economy.rate_sample.confidence is unsupported")
    if rate_sample["basis"] == "engine_direct_gather_cache":
        if gather_stamp is None or rate_sample["confidence"] != "direct":
            raise AdmissionError("invalid_schema", "engine direct rate requires a gather stamp and direct confidence")
        # Retail game frame and gather stamp are 32-bit counters.  Preserve their
        # wrap semantics and independently derive age in simulation frames.  Do
        # not turn this into wall time: speed and pause change that relationship.
        gather_age_frames = ((game_frame & 0xFFFF_FFFF) - gather_stamp) & 0xFFFF_FFFF
        if gather_age_frames > 60 * 60 * 15 or claimed_rate_age_frames != gather_age_frames:
            raise AdmissionError("invalid_schema", "economy.rate_sample.age_frames does not match gather stamp/frame")
    elif rate_sample["basis"] == "sampled_stock_delta":
        if gather_stamp is not None or rate_sample["confidence"] != "estimated":
            raise AdmissionError("invalid_schema", "sampled delta rate requires a null stamp and estimated confidence")
    elif gather_stamp is not None or rate_sample["confidence"] != "unavailable":
        raise AdmissionError("invalid_schema", "unavailable rate requires a null stamp and unavailable confidence")
    resources = _object(economy["resources"], "economy.resources")
    unknown_resources = resources.keys() - set(RESOURCE_NAMES)
    if unknown_resources:
        raise AdmissionError(
            "invalid_schema", f"economy.resources has unknown resources: {', '.join(sorted(unknown_resources))}"
        )
    if not resources:
        raise AdmissionError("invalid_schema", "economy.resources must contain at least one resource")
    for name, raw_resource in resources.items():
        resource = _object(raw_resource, f"economy.resources.{name}")
        _exact_keys(
            resource,
            {"stock", "income_per_min"},
            {"gatherers", "gatherers_basis"},
            f"economy.resources.{name}",
        )
        _number(resource["stock"], f"economy.resources.{name}.stock", 0, 1e12)
        _number(resource["income_per_min"], f"economy.resources.{name}.income_per_min", -1e9, 1e9)
        if ("gatherers" in resource) != ("gatherers_basis" in resource):
            raise AdmissionError("invalid_schema", f"economy.resources.{name} gatherers require an explicit basis")
        if "gatherers" in resource:
            _integer(resource["gatherers"], f"economy.resources.{name}.gatherers", 0, 1_000_000)
            if resource["gatherers_basis"] != "direct_count":
                raise AdmissionError("invalid_schema", f"economy.resources.{name}.gatherers_basis must be direct_count")

    population = _object(economy["population"], "economy.population")
    _exact_keys(population, {"used", "cap"}, {"idle_citizens", "idle_basis"}, "economy.population")
    used = _integer(population["used"], "economy.population.used", 0, 1_000_000)
    cap = _integer(population["cap"], "economy.population.cap", 0, 1_000_000)
    if ("idle_citizens" in population) != ("idle_basis" in population):
        raise AdmissionError("invalid_schema", "economy.population idle_citizens require an explicit basis")
    if "idle_citizens" in population:
        _integer(population["idle_citizens"], "economy.population.idle_citizens", 0, 1_000_000)
        if population["idle_basis"] != "direct_count":
            raise AdmissionError("invalid_schema", "economy.population.idle_basis must be direct_count")

    if "production" in economy:
        production = _object(economy["production"], "economy.production")
        _exact_keys(production, {"queue_depth", "active_sites", "queue_basis"}, set(), "economy.production")
        _integer(production["queue_depth"], "economy.production.queue_depth", 0, 1_000_000)
        _integer(production["active_sites"], "economy.production.active_sites", 0, 1_000_000)
        if production["queue_basis"] != "direct_build_queue":
            raise AdmissionError("invalid_schema", "economy.production.queue_basis must be direct_build_queue")

    goals = snapshot.get("goals", [])
    if not isinstance(goals, list) or len(goals) > 32:
        raise AdmissionError("invalid_schema", "goals must be an array of at most 32 entries")
    goal_ids: set[str] = set()
    for index, raw_goal in enumerate(goals):
        path = f"goals[{index}]"
        goal = _object(raw_goal, path)
        _exact_keys(goal, {"id", "label", "priority", "cost"}, set(), path)
        goal_id = _string(goal["id"], f"{path}.id", 1, 64)
        if goal_id in goal_ids:
            raise AdmissionError("invalid_schema", f"duplicate goal id: {goal_id}")
        goal_ids.add(goal_id)
        _string(goal["label"], f"{path}.label", 1, 128)
        _integer(goal["priority"], f"{path}.priority", 0, 1000)
        cost = _object(goal["cost"], f"{path}.cost")
        if not cost:
            raise AdmissionError("invalid_schema", f"{path}.cost must not be empty")
        unknown_costs = cost.keys() - set(RESOURCE_NAMES)
        if unknown_costs:
            names = ", ".join(sorted(unknown_costs))
            raise AdmissionError("invalid_schema", f"{path}.cost has unknown resources: {names}")
        for name, amount in cost.items():
            _number(amount, f"{path}.cost.{name}", 0, 1e12)

    if "notes" in snapshot:
        _string(snapshot["notes"], "notes", 0, 1024)


def normalize_donfeed_observation(observation: Mapping[str, Any]) -> dict[str, Any]:
    """Strictly normalize one donfeed NDJSON observation into canonical snapshot v1.

    Proven boolean pause state is preserved.  A null pause remains observation-only,
    so the adapter never upgrades incomplete evidence into coaching evidence.
    """

    _exact_keys(
        observation,
        {
            "schema",
            "version",
            "session_id",
            "capture_seq",
            "source",
            "game",
            "capture",
            "resource_order",
            "ok",
            "note",
            "human_slot",
            "leader",
        },
        set(),
        "observation",
    )
    if observation["schema"] != DONFEED_SCHEMA:
        raise AdmissionError("unsupported_observation", f"observation.schema must be {DONFEED_SCHEMA}")
    version = _object(observation["version"], "observation.version")
    _exact_keys(version, {"major", "minor"}, set(), "observation.version")
    if version != DONFEED_VERSION:
        raise AdmissionError("unsupported_observation", "only rontoy.observation v1.0 is supported")
    session_id = _string(observation["session_id"], "observation.session_id", 16, 16)
    if not re.fullmatch(r"[0-9a-f]{16}", session_id):
        raise AdmissionError("invalid_observation", "observation.session_id must be 16 lowercase hex characters")
    sequence = _integer(observation["capture_seq"], "observation.capture_seq", 0, (1 << 63) - 1)

    source = _object(observation["source"], "observation.source")
    _exact_keys(
        source,
        {
            "pid",
            "process_started_100ns",
            "image_base",
            "entry_rva",
            "image_size",
            "module_size",
            "module_sha256",
        },
        set(),
        "observation.source",
    )
    process_id = _integer(source["pid"], "observation.source.pid", 1, (1 << 32) - 1)
    process_started = _string(source["process_started_100ns"], "observation.source.process_started_100ns", 1, 20)
    if not process_started.isascii() or not process_started.isdigit() or not 0 < int(process_started) <= (1 << 64) - 1:
        raise AdmissionError("invalid_observation", "process_started_100ns must be a nonzero decimal u64 string")
    _integer(source["image_base"], "observation.source.image_base", 1, (1 << 32) - 1)
    entry_rva = _integer(source["entry_rva"], "observation.source.entry_rva", 1, (1 << 32) - 1)
    image_size = _integer(source["image_size"], "observation.source.image_size", 1, (1 << 32) - 1)
    if entry_rva != SUPPORTED_ENTRY_RVA or image_size != SUPPORTED_IMAGE_SIZE:
        raise AdmissionError("unsupported_build", "observation PE identity is not the supported retail build")
    module_size = _integer(source["module_size"], "observation.source.module_size", 1, 1 << 40)
    module_sha256 = _string(source["module_sha256"], "observation.source.module_sha256", 64, 64)
    if module_size != SUPPORTED_MODULE_SIZE or module_sha256 != SUPPORTED_MODULE_SHA256:
        raise AdmissionError("unsupported_build", "observation module fingerprint is not the supported retail build")

    game = _object(observation["game"], "observation.game")
    _exact_keys(game, {"mode", "paused", "seconds"}, set(), "observation.game")
    if game["mode"] not in {"single_player", "multiplayer", "unknown"}:
        raise AdmissionError("invalid_observation", "observation.game.mode is invalid")
    if game["paused"] is not None and not isinstance(game["paused"], bool):
        raise AdmissionError("invalid_observation", "observation.game.paused must be a boolean or null")
    game_seconds = _integer(game["seconds"], "observation.game.seconds", 0, (1 << 32) - 1)

    capture = _object(observation["capture"], "observation.capture")
    _exact_keys(
        capture,
        {
            "captured_unix_ms",
            "monotonic_us",
            "duration_us",
            "frame_start",
            "frame_end",
            "coherence",
            "retry_count",
            "valid_components",
            "reads",
            "short_reads",
            "bytes",
        },
        set(),
        "observation.capture",
    )
    captured_unix_ms = _integer(
        capture["captured_unix_ms"], "observation.capture.captured_unix_ms", 0, (1 << 63) - 1
    )
    _integer(capture["monotonic_us"], "observation.capture.monotonic_us", 0, (1 << 63) - 1)
    duration_us = _integer(capture["duration_us"], "observation.capture.duration_us", 0, 60_000_000)
    frame_start = _integer(capture["frame_start"], "observation.capture.frame_start", 0, (1 << 32) - 1)
    frame_end = _integer(capture["frame_end"], "observation.capture.frame_end", 0, (1 << 32) - 1)
    _integer(capture["retry_count"], "observation.capture.retry_count", 0, 3)
    valid_components = _integer(capture["valid_components"], "observation.capture.valid_components", 0, (1 << 32) - 1)
    reads = _integer(capture["reads"], "observation.capture.reads", 1, 1_000_000)
    short_reads = _integer(capture["short_reads"], "observation.capture.short_reads", 0, 1_000_000)
    bytes_read = _integer(capture["bytes"], "observation.capture.bytes", 1, 64 * 1024 * 1024)
    if capture["coherence"] != "coherent" or frame_start != frame_end or short_reads != 0:
        raise AdmissionError("incoherent_capture", "donfeed capture is torn, mixed-frame, or short")
    if valid_components & DONFEED_REQUIRED_COMPONENTS != DONFEED_REQUIRED_COMPONENTS:
        raise AdmissionError("invalid_observation", "donfeed capture lacks Game, Leaders, or Econ validity")
    if observation["ok"] is not True:
        raise AdmissionError("invalid_observation", "donfeed marked the observation unavailable")
    note = _string(observation["note"], "observation.note", 0, 1024)
    human_slot = _integer(observation["human_slot"], "observation.human_slot", 0, 7)
    if observation["resource_order"] != list(RESOURCE_NAMES):
        raise AdmissionError("invalid_observation", "donfeed resource order is not retail order")

    leader = _object(observation["leader"], "observation.leader")
    scalar_fields = {
        "slot",
        "who",
        "tribe",
        "team_color",
        "flags",
        "validity",
        "score",
        "population",
        "city_num",
        "gather_stamp",
        "gather_cache_age_frames",
        "free_peasants",
        "gatherers",
        "fishermen",
        "idle_fishermen",
        "peasants",
        "scholars",
        "active_wars",
        "attacked",
        "age",
        "epochs",
        "discovered",
    }
    array_lengths = {
        "gather_slots": 6,
        "filled_gather_slots": 6,
        "queued_attack_class_cache": 6,
        "stockpile": 6,
        "leftover": 6,
        "resource_cap_x16": 7,
        "over_cap": 6,
        "gross_x16": 6,
        "support_x16": 6,
        "income_x16": 6,
        "ai_planning_rate": 6,
        "bonus": 6,
        "epoch": 4,
    }
    _exact_keys(leader, scalar_fields | array_lengths.keys(), set(), "observation.leader")
    slot = _integer(leader["slot"], "observation.leader.slot", 0, 7)
    who = _integer(leader["who"], "observation.leader.who", 0, 7)
    flags = _integer(leader["flags"], "observation.leader.flags", 0, (1 << 32) - 1)
    validity = _integer(leader["validity"], "observation.leader.validity", 0, (1 << 32) - 1)
    if slot != human_slot or who != slot or flags & 7 != 7 or validity & 3 != 3:
        raise AdmissionError("invalid_observation", "leader is not the unique guarded active human economy")
    for name in scalar_fields - {"slot", "who", "population", "gather_stamp", "gather_cache_age_frames"}:
        _integer(leader[name], f"observation.leader.{name}", -(1 << 31), (1 << 32) - 1)
    population = _object(leader["population"], "observation.leader.population")
    _exact_keys(population, {"current", "cap"}, set(), "observation.leader.population")
    population_current = _integer(population["current"], "observation.leader.population.current", 0, 1_000_000)
    population_cap = _integer(population["cap"], "observation.leader.population.cap", 0, 1_000_000)
    gather_stamp = _integer(leader["gather_stamp"], "observation.leader.gather_stamp", 0, (1 << 32) - 1)
    gather_age = _integer(
        leader["gather_cache_age_frames"], "observation.leader.gather_cache_age_frames", 0, (1 << 32) - 1
    )
    if gather_age != ((frame_end - gather_stamp) & 0xFFFF_FFFF):
        raise AdmissionError("invalid_observation", "gather cache age does not match frame/stamp")
    arrays = {
        name: _integer_array(leader[name], f"observation.leader.{name}", length, -(1 << 31), (1 << 31) - 1)
        for name, length in array_lengths.items()
    }
    if any(value < 0 for value in arrays["stockpile"]):
        raise AdmissionError("invalid_observation", "stockpile contains a negative resource")

    resources = {
        name: {
            "stock": arrays["stockpile"][index],
            # Engine cache is x16 resources per 30 game seconds: x16 / 8
            # is resources per game minute.
            "income_per_min": arrays["income_x16"][index] / 8.0,
        }
        for index, name in enumerate(RESOURCE_NAMES)
    }
    normalized = {
        "schema_version": SCHEMA_VERSION,
        "source": {
            "session_id": f"donfeed-{session_id}",
            "sequence": sequence,
            "captured_at_ms": captured_unix_ms,
            "reader_version": "donfeed-observation-1.0",
            "process_id": process_id,
            "process_started_100ns": process_started,
            "module_sha256": module_sha256,
            "module_size": module_size,
            "image_entry_rva": entry_rva,
            "image_size": image_size,
        },
        "capture": {
            "frame_start": frame_start,
            "frame_end": frame_end,
            "complete": True,
            "advice_allowed": game["mode"] == "single_player" and game["paused"] is False,
            "duration_us": duration_us,
            "read_count": reads,
            "bytes_read": bytes_read,
        },
        "game": {
            "frame": frame_end,
            "player_id": who,
            "mode": game["mode"],
            "paused": game["paused"],
            "seconds": game_seconds,
            "human_count": 1,
            "human_selection_basis": "unique_active_in_play_console_flags",
        },
        "economy": {
            "resources": resources,
            "population": {"used": population_current, "cap": population_cap},
            "rate_sample": {
                "basis": "engine_direct_gather_cache",
                "gather_stamp_raw": gather_stamp,
                "age_frames": gather_age,
                "confidence": "direct",
            },
        },
        "notes": note,
    }
    validate_snapshot(normalized)
    return normalized


def analyze_snapshot(snapshot: Mapping[str, Any]) -> dict[str, Any]:
    """Return economy metrics and advice determined solely by ``snapshot``.

    Advice order is stable: severity, rule order, then resource/goal identifier.  No
    clock, random source, prior snapshot, or external game knowledge participates.
    """

    resources: Mapping[str, Mapping[str, Any]] = snapshot["economy"]["resources"]
    population: Mapping[str, Any] = snapshot["economy"]["population"]
    total_stock = sum(float(resource["stock"]) for resource in resources.values())
    total_income = sum(float(resource["income_per_min"]) for resource in resources.values())
    gatherer_distribution = {
        name: int(resources[name]["gatherers"]) for name in sorted(resources) if "gatherers" in resources[name]
    }
    reported_gatherers_total = sum(gatherer_distribution.values()) if gatherer_distribution else None
    headroom = int(population["cap"]) - int(population["used"])
    rate_sample: Mapping[str, Any] = snapshot["economy"]["rate_sample"]

    suppressed_reasons: list[str] = []
    if not snapshot["capture"]["advice_allowed"]:
        suppressed_reasons.append("reader_disallowed_advice")
    if snapshot["game"]["mode"] != "single_player":
        suppressed_reasons.append("not_single_player")
    if snapshot["game"]["human_count"] != 1:
        suppressed_reasons.append("not_unique_human")
    if snapshot["game"]["paused"] is None:
        suppressed_reasons.append("pause_state_unknown")
    elif snapshot["game"]["paused"]:
        suppressed_reasons.append("game_paused")
    advice_allowed = not suppressed_reasons
    rate_suppressed_reasons: list[str] = []
    if rate_sample["basis"] != "engine_direct_gather_cache" or rate_sample["confidence"] != "direct":
        rate_suppressed_reasons.append("income_rate_not_engine_direct")
    if int(rate_sample["age_frames"]) > MAX_RATE_AGE_FRAMES_FOR_ADVICE:
        rate_suppressed_reasons.append("income_rate_too_old_for_eta")
    rate_advice_allowed = advice_allowed and not rate_suppressed_reasons

    goal_etas: list[dict[str, Any]] = []
    goals = (
        sorted(snapshot.get("goals", []), key=lambda goal: (-goal["priority"], goal["id"]))
        if rate_advice_allowed
        else []
    )
    for goal in goals:
        waits: list[tuple[float, str, float]] = []
        unreachable: list[str] = []
        for name in sorted(goal["cost"]):
            cost = float(goal["cost"][name])
            resource = resources.get(name)
            stock = float(resource["stock"]) if resource else 0.0
            deficit = max(0.0, cost - stock)
            if deficit == 0:
                waits.append((0.0, name, deficit))
            elif not resource or float(resource["income_per_min"]) <= 0:
                unreachable.append(name)
            else:
                waits.append((deficit / float(resource["income_per_min"]), name, deficit))
        if unreachable:
            eta_minutes: Optional[float] = None
            bottleneck = sorted(unreachable)[0]
        else:
            wait, bottleneck, _ = max(waits, key=lambda item: (item[0], item[1]))
            eta_minutes = round(wait, 3)
        goal_etas.append(
            {
                "id": goal["id"],
                "label": goal["label"],
                "priority": goal["priority"],
                "eta_minutes": eta_minutes,
                "bottleneck": bottleneck,
                "unreachable_resources": sorted(unreachable),
            }
        )

    advice: list[dict[str, Any]] = []
    idle = int(population.get("idle_citizens", 0))
    if idle:
        advice.append(
            _advice(
                "idle_citizens_observed",
                "warning",
                "Idle citizens observed",
                f"{idle} citizen{'s are' if idle != 1 else ' is'} idle.",
                "Assign them only where a valid, reachable work site is visible in-game.",
                {"idle_citizens": idle},
                0,
            )
        )
    if headroom <= 0:
        queue_depth = int(snapshot["economy"].get("production", {}).get("queue_depth", 0))
        if queue_depth:
            title = "Population pressure with a live queue"
            detail = f"Population is {abs(headroom)} over cap." if headroom < 0 else "Population is at its current cap."
            detail += f" The reader reports {queue_depth} queued production item{'s' if queue_depth != 1 else ''}."
            action = "Inspect the queue and add population capacity if population blocks it."
            severity = "warning"
        else:
            title = "At or above population cap"
            detail = (
                f"Population is {abs(headroom)} over the reported cap."
                if headroom < 0
                else "Population is at its current cap."
            )
            action = "Review population capacity before starting more unit production."
            severity = "info"
        advice.append(
            _advice(
                "population_pressure",
                severity,
                title,
                detail,
                action,
                {"used": population["used"], "cap": population["cap"], "queue_depth": queue_depth},
                1,
            )
        )
    elif headroom <= 2:
        advice.append(
            _advice(
                "population_headroom_low",
                "warning",
                "Population headroom low",
                f"Only {headroom} population slot{'s remain' if headroom != 1 else ' remains'}.",
                "Start capacity before committing more unit production.",
                {"used": population["used"], "cap": population["cap"], "headroom": headroom},
                2,
            )
        )

    if goal_etas:
        target = goal_etas[0]
        eta = target["eta_minutes"]
        if eta is None or eta >= 0.5:
            bottleneck = target["bottleneck"]
            if eta is None:
                detail = (
                    f"Under current reported rates, {target['label']} cannot progress: "
                    f"{bottleneck} has a deficit and no positive income."
                )
                severity = "critical"
            else:
                detail = (
                    f"Optimistic ETA for {target['label']} is {eta:.2f} min; {bottleneck} is the modeled limiter."
                )
                severity = "warning"
            action = (
                f"If this remains your goal, verify a safe way to increase {bottleneck} income and avoid spending it."
            )
            advice.append(
                _advice(
                    f"goal_bottleneck_{target['id']}",
                    severity,
                    "Priority goal bottleneck",
                    detail,
                    action,
                    {"goal_id": target["id"], "bottleneck": bottleneck, "eta_minutes": eta},
                    4,
                )
            )

    severity_rank = {"critical": 0, "warning": 1, "info": 2}
    advice.sort(key=lambda item: (severity_rank[item["severity"]], item.pop("_rule_order"), item["code"]))
    if not advice_allowed:
        advice = []
    return {
        "advisor_version": ADVISOR_VERSION,
        "advice_allowed": advice_allowed,
        "rate_advice_allowed": rate_advice_allowed,
        "suppressed_reasons": suppressed_reasons,
        "rate_suppressed_reasons": rate_suppressed_reasons,
        "source_sequence": snapshot["source"]["sequence"],
        "game_frame": snapshot["game"]["frame"],
        "metrics": {
            "total_stock": round(total_stock, 3),
            "total_income_per_min": round(total_income, 3),
            "reported_gatherers_total": reported_gatherers_total,
            "population_headroom": headroom,
            "gatherer_distribution": gatherer_distribution,
            "goal_etas": goal_etas,
            "goal_eta_model": "constant_reported_income_no_future_spending_optimistic",
            "capture_duration_us": snapshot["capture"]["duration_us"],
            "capture_read_count": snapshot["capture"]["read_count"],
            "capture_bytes_read": snapshot["capture"]["bytes_read"],
            "income_rate_basis": rate_sample["basis"],
            "income_rate_age_frames": rate_sample["age_frames"],
            "income_rate_confidence": rate_sample["confidence"],
        },
        "advice": advice,
    }


def _advice(
    code: str,
    severity: str,
    title: str,
    detail: str,
    action: str,
    evidence: Mapping[str, Any],
    rule_order: int,
) -> dict[str, Any]:
    return {
        "code": code,
        "severity": severity,
        "title": title,
        "detail": detail,
        "action": action,
        "evidence": dict(evidence),
        "_rule_order": rule_order,
    }


@dataclasses.dataclass(frozen=True)
class SnapshotRecord:
    revision: int
    received_at_ms: int
    received_monotonic: float
    snapshot: Mapping[str, Any]
    analysis: Mapping[str, Any]

    def envelope(self) -> dict[str, Any]:
        return {
            "stream_revision": self.revision,
            "received_at_ms": self.received_at_ms,
            "snapshot": self.snapshot,
            "analysis": self.analysis,
        }


class SnapshotStore:
    """Single-slot, condition-signalled snapshot storage.

    No queue or historical snapshot is retained.  A slow SSE reader skips directly to
    the latest revision.
    """

    def __init__(
        self,
        min_interval_ms: int = DEFAULT_MIN_INTERVAL_MS,
        monotonic: Callable[[], float] = time.monotonic,
        wall_time: Callable[[], float] = time.time,
        stale_after_ms: int = DEFAULT_STALE_AFTER_MS,
    ) -> None:
        if min_interval_ms < 0:
            raise ValueError("min_interval_ms must be non-negative")
        self.min_interval_ms = min_interval_ms
        self._monotonic = monotonic
        self._wall_time = wall_time
        self.stale_after_ms = stale_after_ms
        self._condition = threading.Condition()
        self._latest: Optional[SnapshotRecord] = None
        self._last_admitted_monotonic: Optional[float] = None
        self._revision = 0
        self.accepted = 0
        self.rejected = 0
        self.rejections_by_code: dict[str, int] = {}
        self.started_monotonic = monotonic()

    def admit(self, snapshot: Mapping[str, Any]) -> SnapshotRecord:
        validate_snapshot(snapshot)
        # Disconnect the retained slot from a library caller that might reuse and
        # mutate its producer dictionary after admission.
        snapshot = copy.deepcopy(snapshot)
        with self._condition:
            now = self._monotonic()
            if self._last_admitted_monotonic is not None:
                elapsed_ms = (now - self._last_admitted_monotonic) * 1000
                # A small tolerance avoids rejecting an exact boundary because a
                # binary float represented 100 ms as 99.99999999999 ms.
                if elapsed_ms + 1e-9 < self.min_interval_ms:
                    self.reject("rate_limited")
                    retry_ms = max(1, math.ceil(self.min_interval_ms - elapsed_ms))
                    raise AdmissionError("rate_limited", f"retry after {retry_ms} ms", 429)
            if self._latest is not None:
                prior = self._latest.snapshot
                if prior["source"]["session_id"] != snapshot["source"]["session_id"]:
                    self.reject("source_conflict")
                    raise AdmissionError(
                        "source_conflict", "host is bound to one session; restart it to attach a new process", 409
                    )
                else:
                    identity_fields = (
                        "process_id",
                        "process_started_100ns",
                        "module_sha256",
                        "module_size",
                        "image_entry_rva",
                        "image_size",
                    )
                    if any(prior["source"][field] != snapshot["source"][field] for field in identity_fields):
                        self.reject("identity_changed")
                        raise AdmissionError("identity_changed", "process identity changed within a session", 409)
                    if snapshot["source"]["sequence"] <= prior["source"]["sequence"]:
                        self.reject("stale_sequence")
                        raise AdmissionError("stale_sequence", "sequence must increase within a session", 409)
                    if snapshot["source"]["captured_at_ms"] < prior["source"]["captured_at_ms"]:
                        self.reject("stale_capture")
                        raise AdmissionError("stale_capture", "captured_at_ms moved backwards", 409)
                    if snapshot["game"]["frame"] < prior["game"]["frame"]:
                        self.reject("stale_frame")
                        raise AdmissionError("stale_frame", "game frame moved backwards", 409)
                    if snapshot["game"]["frame"] == prior["game"]["frame"] and not snapshot["game"]["paused"]:
                        self.reject("stalled_frame")
                        raise AdmissionError("stalled_frame", "unpaused game frame did not advance", 409)
            self._revision += 1
            record = SnapshotRecord(
                revision=self._revision,
                received_at_ms=int(self._wall_time() * 1000),
                received_monotonic=now,
                snapshot=snapshot,
                analysis=analyze_snapshot(snapshot),
            )
            self._latest = record
            self._last_admitted_monotonic = now
            self.accepted += 1
            self._condition.notify_all()
            return record

    def reject(self, code: str) -> None:
        with self._condition:
            self.rejected += 1
            self.rejections_by_code[code] = self.rejections_by_code.get(code, 0) + 1

    def latest(self) -> Optional[SnapshotRecord]:
        with self._condition:
            return self._latest

    def wait_after(self, revision: int, timeout: float) -> Optional[SnapshotRecord]:
        with self._condition:
            self._condition.wait_for(
                lambda: self._latest is not None and self._latest.revision > revision,
                timeout=timeout,
            )
            if self._latest is not None and self._latest.revision > revision:
                return self._latest
            return None

    def status(self) -> dict[str, Any]:
        with self._condition:
            latest = self._latest
            now_monotonic = self._monotonic()
            age_ms = 0 if latest is None else max(0, round((now_monotonic - latest.received_monotonic) * 1000))
            return {
                "ok": True,
                "schema_version": SCHEMA_VERSION,
                "advisor_version": ADVISOR_VERSION,
                "retention": "latest-only",
                "accepted": self.accepted,
                "rejected": self.rejected,
                "rejections_by_code": dict(sorted(self.rejections_by_code.items())),
                "min_interval_ms": self.min_interval_ms,
                "stale_after_ms": self.stale_after_ms,
                "uptime_seconds": round(max(0.0, self._monotonic() - self.started_monotonic), 3),
                "latest": None
                if latest is None
                else {
                    "stream_revision": latest.revision,
                    "received_at_ms": latest.received_at_ms,
                    "age_ms": age_ms,
                    "stale": age_ms > self.stale_after_ms,
                    "session_id": latest.snapshot["source"]["session_id"],
                    "sequence": latest.snapshot["source"]["sequence"],
                    "frame": latest.snapshot["game"]["frame"],
                },
            }


class RoNtoyServer(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True
    request_queue_size = 16

    def __init__(
        self,
        address: tuple[str, int],
        store: SnapshotStore,
        max_body_bytes: int = DEFAULT_MAX_BODY_BYTES,
        heartbeat_seconds: float = DEFAULT_HEARTBEAT_SECONDS,
        ingest_token: Optional[str] = None,
        max_connections: int = DEFAULT_MAX_CONNECTIONS,
    ) -> None:
        host = ipaddress.ip_address(address[0])
        if not host.is_loopback:
            raise ValueError("RoNtoy host refuses non-loopback bind addresses")
        self.store = store
        self.max_body_bytes = max_body_bytes
        self.heartbeat_seconds = heartbeat_seconds
        self.ingest_token = ingest_token or secrets.token_urlsafe(24)
        if max_connections <= 0:
            raise ValueError("max_connections must be positive")
        self.max_connections = max_connections
        self._connection_slots = threading.BoundedSemaphore(max_connections)
        self._connection_count_lock = threading.Lock()
        self._active_connections = 0
        super().__init__(address, RoNtoyHandler)

    @property
    def active_connections(self) -> int:
        with self._connection_count_lock:
            return self._active_connections

    def process_request(self, request: Any, client_address: Any) -> None:
        if not self._connection_slots.acquire(blocking=False):
            self.store.reject("connection_limited")
            body = b'{"error":{"code":"connection_limited","message":"too many connections"},"ok":false}'
            response = (
                b"HTTP/1.1 503 Service Unavailable\r\n"
                b"Content-Type: application/json\r\n"
                + f"Content-Length: {len(body)}\r\n".encode("ascii")
                + b"Connection: close\r\n\r\n"
                + body
            )
            try:
                request.sendall(response)
            finally:
                self.shutdown_request(request)
            return
        with self._connection_count_lock:
            self._active_connections += 1
        try:
            super().process_request(request, client_address)
        except BaseException:
            self._release_connection_slot()
            raise

    def process_request_thread(self, request: Any, client_address: Any) -> None:
        try:
            super().process_request_thread(request, client_address)
        finally:
            self._release_connection_slot()

    def _release_connection_slot(self) -> None:
        with self._connection_count_lock:
            self._active_connections -= 1
        self._connection_slots.release()


class RoNtoyHandler(BaseHTTPRequestHandler):
    server: RoNtoyServer
    protocol_version = "HTTP/1.1"

    def setup(self) -> None:
        super().setup()
        self.connection.settimeout(10.0)

    def handle(self) -> None:
        try:
            super().handle()
        except (BrokenPipeError, ConnectionResetError, TimeoutError):
            # A browser closing an SSE or keep-alive socket is routine, not a
            # server fault worth a traceback.
            return

    def log_message(self, format: str, *args: Any) -> None:
        print(f"{self.log_date_time_string()} {self.client_address[0]} {format % args}")

    def do_GET(self) -> None:
        if not self._admit_host_header():
            return
        path = urlsplit(self.path).path
        if path == "/healthz":
            self._json(HTTPStatus.OK, {"ok": True})
        elif path == "/v1/status":
            status = self.server.store.status()
            status["active_connections"] = self.server.active_connections
            status["max_connections"] = self.server.max_connections
            self._json(HTTPStatus.OK, status)
        elif path == "/v1/latest":
            latest = self.server.store.latest()
            if latest is None:
                self._error(HTTPStatus.NOT_FOUND, "no_snapshot", "no snapshot has been admitted")
            else:
                self._json(HTTPStatus.OK, latest.envelope())
        elif path == "/v1/stream":
            self._stream()
        elif path in {"/", "/rontoy.html"}:
            self._static(WEB_PUBLIC_ROOT / "rontoy.html", "text/html; charset=utf-8")
        elif path == "/js/rontoy.js":
            self._static(WEB_PUBLIC_ROOT / "js" / "rontoy.js", "text/javascript; charset=utf-8")
        else:
            self._error(HTTPStatus.NOT_FOUND, "not_found", "unknown endpoint")

    def do_HEAD(self) -> None:
        if not self._admit_host_header():
            return
        path = urlsplit(self.path).path
        if path in {"/", "/rontoy.html"}:
            self._static(WEB_PUBLIC_ROOT / "rontoy.html", "text/html; charset=utf-8", head_only=True)
        elif path == "/js/rontoy.js":
            self._static(WEB_PUBLIC_ROOT / "js" / "rontoy.js", "text/javascript; charset=utf-8", head_only=True)
        elif path == "/healthz":
            body = b'{"ok":true}'
            self._bytes(HTTPStatus.OK, body, "application/json; charset=utf-8", send_body=False)
        else:
            self._error(HTTPStatus.NOT_FOUND, "not_found", "unknown endpoint")

    def do_POST(self) -> None:
        if not self._admit_host_header():
            return
        path = urlsplit(self.path).path
        if path != "/v1/snapshot":
            self._error(HTTPStatus.NOT_FOUND, "not_found", "unknown endpoint")
            return
        supplied_token = self.headers.get("X-RoNtoy-Token", "")
        if not hmac.compare_digest(supplied_token, self.server.ingest_token):
            self.server.store.reject("unauthorized")
            self.close_connection = True
            self._error(HTTPStatus.UNAUTHORIZED, "unauthorized", "missing or invalid X-RoNtoy-Token")
            return
        content_type = self.headers.get("Content-Type", "").split(";", 1)[0].strip().lower()
        if content_type != "application/json":
            self.server.store.reject("unsupported_media_type")
            self.close_connection = True
            self._error(HTTPStatus.UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type", "use application/json")
            return
        raw_length = self.headers.get("Content-Length")
        if raw_length is None:
            self.server.store.reject("length_required")
            self.close_connection = True
            self._error(HTTPStatus.LENGTH_REQUIRED, "length_required", "Content-Length is required")
            return
        try:
            length = int(raw_length, 10)
        except ValueError:
            self.server.store.reject("invalid_length")
            self.close_connection = True
            self._error(HTTPStatus.BAD_REQUEST, "invalid_length", "Content-Length is invalid")
            return
        if length < 0 or length > self.server.max_body_bytes:
            self.server.store.reject("body_too_large")
            self.close_connection = True
            self._error(HTTPStatus.REQUEST_ENTITY_TOO_LARGE, "body_too_large", "snapshot body is too large")
            return
        try:
            raw = self.rfile.read(length)
            if len(raw) != length:
                raise AdmissionError("short_body", "request ended before Content-Length", 400)
            snapshot = parse_snapshot_json(raw, self.server.max_body_bytes)
            record = self.server.store.admit(snapshot)
        except AdmissionError as exc:
            # SnapshotStore counts admissions rejected after schema validation; all
            # parsing/schema failures are counted here.
            if exc.code not in {
                "rate_limited",
                "stale_sequence",
                "stale_capture",
                "stale_frame",
                "stalled_frame",
                "identity_changed",
                "source_conflict",
            }:
                self.server.store.reject(exc.code)
            self._error(exc.status, exc.code, str(exc))
            return
        self._json(HTTPStatus.ACCEPTED, record.envelope())

    def _admit_host_header(self) -> bool:
        hosts = self.headers.get_all("Host", [])
        port = self.server.server_port
        allowed = {f"127.0.0.1:{port}", f"localhost:{port}"}
        if len(hosts) == 1 and hosts[0].lower() in allowed:
            return True
        self.server.store.reject("invalid_host")
        self.close_connection = True
        self._error(HTTPStatus.MISDIRECTED_REQUEST, "invalid_host", "Host must name this loopback service")
        return False

    def _stream(self) -> None:
        # SSE owns this HTTP connection until either peer closes it.  Prevent
        # BaseHTTPRequestHandler from trying to parse another request afterward.
        self.close_connection = True
        self.send_response(HTTPStatus.OK)
        self.send_header("Content-Type", "text/event-stream; charset=utf-8")
        self.send_header("Cache-Control", "no-cache, no-transform")
        self.send_header("Connection", "keep-alive")
        self.send_header("X-Accel-Buffering", "no")
        self.end_headers()
        revision = 0
        try:
            while True:
                record = self.server.store.wait_after(revision, self.server.heartbeat_seconds)
                if record is None:
                    self.wfile.write(b": keepalive\n\n")
                else:
                    revision = record.revision
                    payload = json.dumps(record.envelope(), separators=(",", ":"), sort_keys=True)
                    message = f"id: {revision}\nevent: snapshot\ndata: {payload}\n\n".encode("utf-8")
                    self.wfile.write(message)
                self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError, TimeoutError, OSError):
            return

    def _error(self, status: int, code: str, message: str) -> None:
        self._json(status, {"ok": False, "error": {"code": code, "message": message}})

    def _json(self, status: int, value: Mapping[str, Any]) -> None:
        body = json.dumps(value, separators=(",", ":"), sort_keys=True).encode("utf-8")
        self._bytes(status, body, "application/json; charset=utf-8")

    def _static(self, path: pathlib.Path, content_type: str, head_only: bool = False) -> None:
        try:
            body = path.read_bytes()
        except OSError:
            self._error(HTTPStatus.SERVICE_UNAVAILABLE, "dashboard_unavailable", "dashboard asset is unavailable")
            return
        self._bytes(
            HTTPStatus.OK,
            body,
            content_type,
            {
                "Cache-Control": "no-store",
                "Content-Security-Policy": (
                    "default-src 'self'; script-src 'self'; style-src 'unsafe-inline'; "
                    "connect-src 'self'; img-src data:; object-src 'none'; base-uri 'none'; form-action 'none'"
                ),
            },
            send_body=not head_only,
        )

    def _bytes(
        self,
        status: int,
        body: bytes,
        content_type: str,
        extra_headers: Optional[Mapping[str, str]] = None,
        send_body: bool = True,
    ) -> None:
        # One request per short-lived connection keeps the thread/connection
        # boundary bounded and avoids unread rejected bodies being reinterpreted.
        self.close_connection = True
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Referrer-Policy", "no-referrer")
        self.send_header("Connection", "close")
        for name, value in (extra_headers or {}).items():
            self.send_header(name, value)
        self.end_headers()
        if send_body:
            self.wfile.write(body)


def demo_snapshot(sequence: int) -> dict[str, Any]:
    """A deterministic fixture for UI/advisor development without a live reader."""

    step = sequence % 20
    return {
        "schema_version": 1,
        "source": {
            "session_id": "demo",
            "sequence": sequence,
            "captured_at_ms": sequence * 1000,
            "reader_version": "demo-1",
            "process_id": 1,
            "process_started_100ns": "1",
            "module_sha256": "0" * 64,
            "module_size": 1,
            "image_entry_rva": SUPPORTED_ENTRY_RVA,
            "image_size": SUPPORTED_IMAGE_SIZE,
        },
        "game": {
            "frame": sequence * 15,
            "player_id": 1,
            "mode": "single_player",
            "human_count": 1,
            "human_selection_basis": "unique_active_in_play_console_flags",
            "age": 2,
            "paused": False,
            "match_name": "Demo",
        },
        "capture": {
            "frame_start": sequence * 15,
            "frame_end": sequence * 15,
            "complete": True,
            "advice_allowed": True,
            "duration_us": 250,
            "read_count": 12,
            "bytes_read": 256,
        },
        "economy": {
            "resources": {
                "food": {
                    "stock": 85 + step * 2,
                    "income_per_min": 65,
                    "gatherers": 7,
                    "gatherers_basis": "direct_count",
                },
                "timber": {
                    "stock": 35 + step,
                    "income_per_min": 42,
                    "gatherers": 5,
                    "gatherers_basis": "direct_count",
                },
                "wealth": {
                    "stock": 20 + step,
                    "income_per_min": 24,
                    "gatherers": 3,
                    "gatherers_basis": "direct_count",
                },
                "knowledge": {
                    "stock": 12 + step,
                    "income_per_min": 16,
                    "gatherers": 2,
                    "gatherers_basis": "direct_count",
                },
            },
            "population": {
                "used": 29 + min(step // 7, 2),
                "cap": 32,
                "idle_citizens": 1 if step == 0 else 0,
                "idle_basis": "direct_count",
            },
            "rate_sample": {
                "basis": "engine_direct_gather_cache",
                "gather_stamp_raw": sequence * 15,
                "age_frames": 0,
                "confidence": "direct",
            },
            "production": {"queue_depth": 2, "active_sites": 1, "queue_basis": "direct_build_queue"},
        },
        "goals": [
            {"id": "age-up", "label": "Next age", "priority": 100, "cost": {"food": 500, "knowledge": 200}}
        ],
        "notes": "Synthetic data; never present this as a live reading.",
    }


def _run_demo(store: SnapshotStore, stop: threading.Event, interval: float) -> None:
    sequence = 0
    while not stop.is_set():
        try:
            store.admit(demo_snapshot(sequence))
        except AdmissionError:
            pass
        sequence += 1
        stop.wait(interval)


def build_argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="RoNtoy loopback snapshot host")
    parser.add_argument("--port", type=int, default=DEFAULT_PORT, help=f"loopback port (default {DEFAULT_PORT})")
    parser.add_argument(
        "--max-body-bytes", type=int, default=DEFAULT_MAX_BODY_BYTES, help="maximum admitted JSON request size"
    )
    parser.add_argument(
        "--min-interval-ms", type=int, default=DEFAULT_MIN_INTERVAL_MS, help="minimum interval between snapshots"
    )
    parser.add_argument("--demo", action="store_true", help="publish deterministic synthetic snapshots")
    parser.add_argument("--demo-interval", type=float, default=1.0, help="seconds between synthetic snapshots")
    parser.add_argument(
        "--max-connections", type=int, default=DEFAULT_MAX_CONNECTIONS, help="maximum simultaneous HTTP connections"
    )
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    args = build_argument_parser().parse_args(argv)
    if not 0 <= args.port <= 65535:
        raise SystemExit("--port must be in 0..65535")
    if args.max_body_bytes <= 0:
        raise SystemExit("--max-body-bytes must be positive")
    if args.min_interval_ms < 0:
        raise SystemExit("--min-interval-ms must be non-negative")
    if args.demo_interval <= 0:
        raise SystemExit("--demo-interval must be positive")
    if args.max_connections <= 0:
        raise SystemExit("--max-connections must be positive")

    store = SnapshotStore(min_interval_ms=args.min_interval_ms)
    server = RoNtoyServer(
        ("127.0.0.1", args.port),
        store,
        max_body_bytes=args.max_body_bytes,
        max_connections=args.max_connections,
    )
    stop = threading.Event()
    demo_thread: Optional[threading.Thread] = None
    if args.demo:
        demo_thread = threading.Thread(target=_run_demo, args=(store, stop, args.demo_interval), daemon=True)
        demo_thread.start()

    def request_shutdown(_signum: int, _frame: Any) -> None:
        threading.Thread(target=server.shutdown, daemon=True).start()

    signal.signal(signal.SIGINT, request_shutdown)
    signal.signal(signal.SIGTERM, request_shutdown)
    print(f"RoNtoy listening on http://127.0.0.1:{server.server_port} (latest-only, schema v{SCHEMA_VERSION})")
    print(f"Snapshot ingest token: {server.ingest_token}")
    try:
        server.serve_forever(poll_interval=0.25)
    finally:
        stop.set()
        server.server_close()
        if demo_thread is not None:
            demo_thread.join(timeout=2)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
