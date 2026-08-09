#!/usr/bin/env python3
"""Fail-closed sealer for a retail World::walk_data byte capture.

The sealer does not acquire process memory.  It authenticates the raw image
published by the in-process adapter, the exact retail executable, the original
RCX-derived same-group checkpoint report, and a controller-generated capture
context.  Only a complete byte image may produce a localization result.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import struct
import sys
import tempfile
import zlib


SCHEMA = "don.retail-world-walk-capture.v1"
CHECKPOINT_SCHEMA = "don.retail-world-checkpoint.v1"
CONTEXT_SCHEMA = "don.retail-world-walk-context.v1"
SECTION_SCHEMA = "don.world-walk-sections.v1"
EXPECTED_EXE_SHA256 = "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079"
EXPECTED_EXE_BYTES = 9_925_120
EXPECTED_MACHINE = 0x014C
EXPECTED_TIMESTAMP = 0x6674863F
EXPECTED_ENTRY_RVA = 0x0015D699
EXPECTED_IMAGE_BASE = 0x00400000
EXPECTED_IMAGE_SIZE = 0x00BB4000
WORLD_WALK_VA = 0x006B5CF0
WORLD_WALK_BYTES = 903
WORLD_WALK_SHA256 = "adf50b4197020da3932b562442b1cf7d8e80443da42f6181ebd23276bd080e01"
WORLD_CALLSITE_VA = 0x00936A03
WORLD_CALLSITE_BYTES = bytes.fromhex("e8e8f2d7ff")
MAX_CAPTURE_BYTES = 16 * 1024 * 1024
HEX64 = re.compile(r"[0-9a-f]{64}\Z")
HEX32 = re.compile(r"0x[0-9a-f]{8}\Z")


class Refusal(RuntimeError):
    pass


def refuse(condition: bool, message: str) -> None:
    if condition:
        raise Refusal(message)


def read_once(path: Path, maximum: int) -> bytes:
    before = path.lstat()
    refuse(stat.S_ISLNK(before.st_mode), f"symlink input: {path}")
    refuse(not stat.S_ISREG(before.st_mode), f"not a regular file: {path}")
    refuse(before.st_size < 0 or before.st_size > maximum, f"invalid input size: {path}")
    flags = os.O_RDONLY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    descriptor = os.open(path, flags)
    try:
        opened = os.fstat(descriptor)
        refuse(
            (opened.st_dev, opened.st_ino, opened.st_size)
            != (before.st_dev, before.st_ino, before.st_size),
            f"input identity changed while opening: {path}",
        )
        chunks: list[bytes] = []
        remaining = opened.st_size
        while remaining:
            chunk = os.read(descriptor, min(remaining, 1024 * 1024))
            refuse(not chunk, f"short read: {path}")
            chunks.append(chunk)
            remaining -= len(chunk)
        refuse(bool(os.read(descriptor, 1)), f"input grew while reading: {path}")
        after = os.fstat(descriptor)
        refuse(
            (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns)
            != (opened.st_dev, opened.st_ino, opened.st_size, opened.st_mtime_ns),
            f"input changed while reading: {path}",
        )
        return b"".join(chunks)
    finally:
        os.close(descriptor)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def exact_object(value: object, keys: set[str], label: str) -> dict:
    refuse(not isinstance(value, dict), f"{label} is not an object")
    refuse(set(value) != keys, f"{label} fields drift: {sorted(set(value) ^ keys)}")
    return value


def parse_json(data: bytes, label: str) -> dict:
    try:
        value = json.loads(data.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise Refusal(f"malformed {label}: {error}") from error
    refuse(not isinstance(value, dict), f"{label} root is not an object")
    return value


def parse_u32(value: object, label: str) -> int:
    refuse(not isinstance(value, str) or not HEX32.fullmatch(value), f"invalid {label}")
    return int(value, 16)


def parse_checkpoint(data: bytes) -> dict:
    root = exact_object(
        parse_json(data, "checkpoint"),
        {"schema", "replay", "join", "world_checksum", "peers", "byte_agreement_claimed"},
        "checkpoint",
    )
    refuse(root["schema"] != CHECKPOINT_SCHEMA, "wrong checkpoint schema")
    refuse(root["byte_agreement_claimed"] is not False, "checkpoint claims byte agreement")
    replay = exact_object(
        root["replay"],
        {"name", "bytes", "sha256", "payload_bytes", "payload_sha256", "version"},
        "checkpoint.replay",
    )
    refuse(not isinstance(replay["name"], str) or not replay["name"], "missing replay name")
    refuse(not isinstance(replay["bytes"], int) or replay["bytes"] <= 0, "invalid replay size")
    refuse(
        not isinstance(replay["payload_bytes"], int) or replay["payload_bytes"] <= 0,
        "invalid payload size",
    )
    for field in ("sha256", "payload_sha256"):
        refuse(not isinstance(replay[field], str) or not HEX64.fullmatch(replay[field]), f"bad {field}")
    join = exact_object(
        root["join"],
        {"key", "group", "reporters", "all_16_channels_identical"},
        "checkpoint.join",
    )
    refuse(join["key"] != "CommandPackage::group", "checkpoint uses the wrong join key")
    refuse(not isinstance(join["group"], int) or join["group"] < 0, "invalid group")
    refuse(join["all_16_channels_identical"] is not True, "peer tuples do not agree")
    peers = root["peers"]
    refuse(not isinstance(peers, list) or len(peers) < 2, "fewer than two peer packets")
    refuse(join["reporters"] != len(peers), "reporter count drift")
    expected_world = parse_u32(root["world_checksum"], "world checksum")
    plays: set[int] = set()
    evidence_hashes: set[str] = set()
    first_channels: list[int] | None = None
    for index, raw_peer in enumerate(peers):
        peer = exact_object(
            raw_peer,
            {
                "play", "stamp", "packet_bytes", "packet_sha256",
                "packet_evidence_sha256", "channels",
            },
            f"checkpoint.peers[{index}]",
        )
        refuse(not isinstance(peer["play"], int) or peer["play"] < 0, "invalid peer play")
        refuse(peer["play"] in plays, "duplicate peer play")
        plays.add(peer["play"])
        refuse(not isinstance(peer["stamp"], int) or peer["stamp"] < 0, "invalid peer stamp")
        refuse(peer["packet_bytes"] != 65, "checksum packet is not 65 bytes")
        for field in ("packet_sha256", "packet_evidence_sha256"):
            refuse(not isinstance(peer[field], str) or not HEX64.fullmatch(peer[field]), f"bad {field}")
        refuse(
            peer["packet_evidence_sha256"] in evidence_hashes,
            "distinct reporters have duplicate header-bound packet evidence",
        )
        evidence_hashes.add(peer["packet_evidence_sha256"])
        channels_raw = peer["channels"]
        refuse(not isinstance(channels_raw, list) or len(channels_raw) != 16, "channel tuple width drift")
        channels = [parse_u32(word, "channel word") for word in channels_raw]
        refuse(sum(channels[:15]) & 0xFFFFFFFF != channels[15], "checksum total is inconsistent")
        refuse(
            any((word & 0xFFFF) >= 65521 or (word >> 16) >= 65521 for word in channels[:15]),
            "checksum tuple is not Adler-shaped",
        )
        refuse(channels[11] != expected_world, "peer World word drift")
        if first_channels is None:
            first_channels = channels
        else:
            refuse(channels != first_channels, "same-group peer disagreement")
    return {**root, "world_checksum_u32": expected_world}


def parse_context(data: bytes, checkpoint: dict, image: bytes) -> dict:
    root = exact_object(
        parse_json(data, "capture context"),
        {"schema", "controller", "process", "hook", "capture"},
        "capture context",
    )
    refuse(root["schema"] != CONTEXT_SCHEMA, "wrong capture-context schema")
    controller = exact_object(
        root["controller"],
        {
            "generation", "dll_sha256", "attempt", "epoch", "ready_sha256",
            "loaded_module_manifest_sha256",
        },
        "context.controller",
    )
    refuse(not isinstance(controller["generation"], str) or not controller["generation"], "missing generation")
    for field in ("dll_sha256", "ready_sha256", "loaded_module_manifest_sha256"):
        refuse(not isinstance(controller[field], str) or not HEX64.fullmatch(controller[field]), f"bad {field}")
    for field in ("attempt", "epoch"):
        refuse(not isinstance(controller[field], int) or controller[field] <= 0, f"bad {field}")
    process = exact_object(
        root["process"],
        {"pid", "creation_time_100ns", "image_base", "main_thread_id", "retail_executable_sha256"},
        "context.process",
    )
    refuse(not isinstance(process["pid"], int) or process["pid"] <= 0, "invalid pid")
    refuse(
        not isinstance(process["creation_time_100ns"], str)
        or not process["creation_time_100ns"].isdigit()
        or int(process["creation_time_100ns"]) <= 0,
        "invalid process creation time",
    )
    refuse(
        not isinstance(process["image_base"], str)
        or not re.fullmatch(r"0x[0-9a-f]{8}", process["image_base"]),
        "invalid image base",
    )
    refuse(not isinstance(process["main_thread_id"], int) or process["main_thread_id"] <= 0, "invalid main thread")
    refuse(process["retail_executable_sha256"] != EXPECTED_EXE_SHA256, "loaded executable identity drift")
    hook = exact_object(
        root["hook"],
        {
            "callsite_va", "original_bytes", "world_walker_va", "world_walker_bytes",
            "world_walker_sha256", "restoration_owner", "restored_original",
            "adapter_active_at_copy",
        },
        "context.hook",
    )
    refuse(hook["callsite_va"] != f"0x{WORLD_CALLSITE_VA:08x}", "wrong callsite")
    refuse(hook["original_bytes"] != WORLD_CALLSITE_BYTES.hex(), "wrong original call bytes")
    refuse(hook["world_walker_va"] != f"0x{WORLD_WALK_VA:08x}", "wrong walker VA")
    refuse(hook["world_walker_bytes"] != WORLD_WALK_BYTES, "wrong walker extent")
    refuse(hook["world_walker_sha256"] != WORLD_WALK_SHA256, "wrong walker identity")
    refuse(hook["restoration_owner"] != "retail-controller-lifecycle", "unowned callsite lifecycle")
    refuse(hook["restored_original"] is not True, "World callsite was not restored before copy")
    refuse(hook["adapter_active_at_copy"] is not False, "adapter remained active at copy")
    capture = exact_object(
        root["capture"],
        {
            "sequence", "phase", "fault", "group", "target_checksum",
            "original_checksum", "original_bytes", "captured_checksum", "captured_bytes",
            "walk_calls", "tag_calls", "thread_id", "image_sha256", "inflight_at_copy",
            "replay_sha256", "replay_payload_sha256",
        },
        "context.capture",
    )
    refuse(not isinstance(capture["sequence"], int) or capture["sequence"] <= 0, "invalid sequence")
    refuse(capture["phase"] != "frozen" or capture["fault"] != "none", "capture did not freeze cleanly")
    refuse(capture["group"] != checkpoint["join"]["group"], "capture/checkpoint group drift")
    refuse(capture["replay_sha256"] != checkpoint["replay"]["sha256"], "capture replay identity drift")
    refuse(
        capture["replay_payload_sha256"] != checkpoint["replay"]["payload_sha256"],
        "capture replay payload identity drift",
    )
    expected = checkpoint["world_checksum_u32"]
    for field in ("target_checksum", "original_checksum", "captured_checksum"):
        refuse(parse_u32(capture[field], field) != expected, f"{field} is not peer-agreed World")
    refuse(capture["original_bytes"] != len(image), "original walk length drift")
    refuse(capture["captured_bytes"] != len(image), "captured length drift")
    refuse(not isinstance(capture["walk_calls"], int) or capture["walk_calls"] <= 0, "no walk callbacks")
    refuse(not isinstance(capture["tag_calls"], int) or capture["tag_calls"] <= 0, "no tag callback")
    refuse(capture["thread_id"] != process["main_thread_id"], "capture did not run on retail main thread")
    refuse(capture["inflight_at_copy"] != 0, "capture remained inflight at copy")
    for field in ("replay_sha256", "replay_payload_sha256"):
        refuse(not isinstance(capture[field], str) or not HEX64.fullmatch(capture[field]), f"bad {field}")
    refuse(capture["image_sha256"] != sha256(image), "raw image identity drift")
    return root


def pe_identity(executable: bytes) -> dict:
    refuse(len(executable) != EXPECTED_EXE_BYTES, "unsupported executable size")
    executable_hash = sha256(executable)
    refuse(executable_hash != EXPECTED_EXE_SHA256, "unsupported executable SHA-256")
    refuse(executable[:2] != b"MZ", "missing DOS header")
    pe_offset = struct.unpack_from("<I", executable, 0x3C)[0]
    refuse(executable[pe_offset:pe_offset + 4] != b"PE\0\0", "missing PE signature")
    machine, section_count, timestamp, _, _, optional_bytes, _ = struct.unpack_from(
        "<HHIIIHH", executable, pe_offset + 4
    )
    refuse(machine != EXPECTED_MACHINE, "wrong PE machine")
    refuse(timestamp != EXPECTED_TIMESTAMP, f"wrong PE timestamp 0x{timestamp:08x}")
    optional = pe_offset + 24
    refuse(optional_bytes < 0x60 or struct.unpack_from("<H", executable, optional)[0] != 0x10B, "not PE32")
    entry_rva = struct.unpack_from("<I", executable, optional + 0x10)[0]
    image_base = struct.unpack_from("<I", executable, optional + 0x1C)[0]
    image_size = struct.unpack_from("<I", executable, optional + 0x38)[0]
    refuse(entry_rva != EXPECTED_ENTRY_RVA, "entry RVA drift")
    refuse(image_base != EXPECTED_IMAGE_BASE, "preferred image base drift")
    refuse(image_size != EXPECTED_IMAGE_SIZE, "image size drift")
    sections = []
    section_table = optional + optional_bytes
    for index in range(section_count):
        offset = section_table + index * 40
        name = executable[offset:offset + 8].split(b"\0", 1)[0].decode("ascii", "strict")
        virtual_size, virtual_address, raw_size, raw_offset = struct.unpack_from(
            "<IIII", executable, offset + 8
        )
        sections.append((name, virtual_address, virtual_size, raw_offset, raw_size))

    def va_bytes(va: int, size: int) -> bytes:
        rva = va - image_base
        for _, section_rva, virtual_size, raw_offset, raw_size in sections:
            extent = max(virtual_size, raw_size)
            if section_rva <= rva and rva + size <= section_rva + extent:
                file_offset = raw_offset + rva - section_rva
                refuse(file_offset + size > len(executable), "VA maps beyond executable")
                return executable[file_offset:file_offset + size]
        raise Refusal(f"VA 0x{va:08x} is not file-backed")

    walker = va_bytes(WORLD_WALK_VA, WORLD_WALK_BYTES)
    refuse(sha256(walker) != WORLD_WALK_SHA256, "World walker code drift")
    refuse(va_bytes(WORLD_CALLSITE_VA, 5) != WORLD_CALLSITE_BYTES, "World callsite code drift")
    return {
        "bytes": len(executable),
        "sha256": executable_hash,
        "machine": f"0x{machine:04x}",
        "timestamp": f"0x{timestamp:08x}",
        "entry_rva": f"0x{entry_rva:08x}",
        "image_base": f"0x{image_base:08x}",
        "image_size": f"0x{image_size:08x}",
        "world_walker": {
            "va": f"0x{WORLD_WALK_VA:08x}",
            "bytes": len(walker),
            "sha256": sha256(walker),
        },
        "world_callsite": {
            "va": f"0x{WORLD_CALLSITE_VA:08x}",
            "bytes": WORLD_CALLSITE_BYTES.hex(),
        },
    }


def parse_sections(data: bytes, model_len: int) -> list[dict]:
    root = exact_object(parse_json(data, "section map"), {"schema", "bytes", "sections"}, "section map")
    refuse(root["schema"] != SECTION_SCHEMA or root["bytes"] != model_len, "section-map identity drift")
    refuse(not isinstance(root["sections"], list) or not root["sections"], "empty section map")
    cursor = 0
    sections = []
    for index, raw in enumerate(root["sections"]):
        section = exact_object(raw, {"id", "name", "start", "end"}, f"sections[{index}]")
        refuse(section["id"] != index + 1, "section ids are not retail order")
        refuse(not isinstance(section["name"], str) or not section["name"], "missing section name")
        refuse(section["start"] != cursor, "section map is not contiguous")
        refuse(not isinstance(section["end"], int) or section["end"] < cursor, "invalid section end")
        cursor = section["end"]
        sections.append(section)
    refuse(cursor != model_len, "section map does not cover model image")
    return sections


def localize(image: bytes, model: bytes | None, sections: list[dict] | None) -> dict:
    if model is None:
        return {
            "available": False,
            "reason": "no model byte image supplied; checksum-only localization is forbidden",
            "byte_image_equal": None,
            "first_difference": None,
        }
    limit = min(len(image), len(model))
    first = next((index for index in range(limit) if image[index] != model[index]), None)
    if first is None and len(image) != len(model):
        first = limit
    if first is None:
        difference = None
    else:
        section_hit = None
        if sections is not None:
            section_hit = next(
                (
                    {"id": section["id"], "name": section["name"], "offset": first - section["start"]}
                    for section in sections
                    if section["start"] <= first < section["end"]
                ),
                None,
            )
        difference = {
            "global_offset": first,
            "retail_byte": image[first] if first < len(image) else None,
            "model_byte": model[first] if first < len(model) else None,
            "section": section_hit,
        }
    return {
        "available": True,
        "reason": "comparison used the authenticated retail byte image",
        "byte_image_equal": difference is None,
        "first_difference": difference,
    }


def canonical(value: object) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def write_new(path: Path, data: bytes) -> None:
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o444)
    try:
        written = 0
        while written < len(data):
            written += os.write(descriptor, data[written:])
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def seal(args: argparse.Namespace) -> Path:
    image = read_once(args.image, MAX_CAPTURE_BYTES)
    refuse(not image, "empty World walk image")
    checkpoint_bytes = read_once(args.checkpoint, 1024 * 1024)
    checkpoint = parse_checkpoint(checkpoint_bytes)
    context_bytes = read_once(args.context, 1024 * 1024)
    context = parse_context(context_bytes, checkpoint, image)
    executable = read_once(args.executable, 16 * 1024 * 1024)
    executable_identity = pe_identity(executable)
    expected = checkpoint["world_checksum_u32"]
    image_adler = zlib.adler32(image, 1) & 0xFFFFFFFF
    refuse(image_adler != expected, "captured image does not hash to peer-agreed World value")

    model = read_once(args.model_image, MAX_CAPTURE_BYTES) if args.model_image else None
    sections = None
    section_identity = None
    if args.section_map:
        refuse(model is None, "section map requires a model image")
        section_bytes = read_once(args.section_map, 1024 * 1024)
        sections = parse_sections(section_bytes, len(model))
        section_identity = {"bytes": len(section_bytes), "sha256": sha256(section_bytes)}
    localization = localize(image, model, sections)
    core = {
        "schema": SCHEMA,
        "retail_executable": executable_identity,
        "checkpoint": {
            "bytes": len(checkpoint_bytes),
            "sha256": sha256(checkpoint_bytes),
            "replay": checkpoint["replay"],
            "group": checkpoint["join"]["group"],
            "world_checksum": checkpoint["world_checksum"],
            "reporters": checkpoint["join"]["reporters"],
        },
        "capture_context": {
            "bytes": len(context_bytes),
            "sha256": sha256(context_bytes),
            "controller": context["controller"],
            "process": context["process"],
            "hook": context["hook"],
            "capture": context["capture"],
        },
        "image": {
            "bytes": len(image),
            "sha256": sha256(image),
            "adler32": f"0x{image_adler:08x}",
        },
        "model": None if model is None else {"bytes": len(model), "sha256": sha256(model)},
        "section_map": section_identity,
        "localization": localization,
        "simulation_agreement_claimed": False,
    }
    artifact_id = sha256(canonical(core))
    manifest = {**core, "artifact_id": artifact_id}
    output_root = args.output.resolve()
    output_root.mkdir(parents=True, exist_ok=True)
    destination = output_root / artifact_id
    if destination.exists():
        destination_stat = destination.lstat()
        refuse(
            not stat.S_ISDIR(destination_stat.st_mode) or stat.S_ISLNK(destination_stat.st_mode),
            "artifact destination is not an ordinary directory",
        )
        refuse(destination_stat.st_mode & 0o222 != 0, "existing artifact directory is writable")
        expected_files = {
            "image.bin": image,
            "checkpoint.json": checkpoint_bytes,
            "capture-context.json": context_bytes,
            "manifest.json": json.dumps(manifest, indent=2, sort_keys=True).encode("utf-8") + b"\n",
        }
        refuse(
            {entry.name for entry in destination.iterdir()} != set(expected_files),
            "existing artifact file set drift",
        )
        for name, expected_bytes in expected_files.items():
            existing_path = destination / name
            refuse(existing_path.stat().st_mode & 0o222 != 0, f"existing artifact is writable: {name}")
            refuse(read_once(existing_path, MAX_CAPTURE_BYTES) != expected_bytes, f"artifact id collision: {name}")
        return destination
    temporary = Path(tempfile.mkdtemp(prefix=f".{artifact_id}.tmp-", dir=output_root))
    try:
        write_new(temporary / "image.bin", image)
        write_new(temporary / "checkpoint.json", checkpoint_bytes)
        write_new(temporary / "capture-context.json", context_bytes)
        write_new(
            temporary / "manifest.json",
            json.dumps(manifest, indent=2, sort_keys=True).encode("utf-8") + b"\n",
        )
        directory = os.open(temporary, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
        os.chmod(temporary, 0o555)
        os.rename(temporary, destination)
        return destination
    except BaseException:
        os.chmod(temporary, 0o755)
        shutil.rmtree(temporary)
        raise


def parser() -> argparse.ArgumentParser:
    out = argparse.ArgumentParser()
    out.add_argument("--image", type=Path, required=True)
    out.add_argument("--checkpoint", type=Path, required=True)
    out.add_argument("--context", type=Path, required=True)
    out.add_argument("--executable", type=Path, required=True)
    out.add_argument("--output", type=Path, required=True)
    out.add_argument("--model-image", type=Path)
    out.add_argument("--section-map", type=Path)
    return out


def main() -> int:
    try:
        destination = seal(parser().parse_args())
    except (OSError, Refusal) as error:
        print(f"REFUSING world-walk capture: {error}", file=sys.stderr)
        return 2
    print(destination)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
