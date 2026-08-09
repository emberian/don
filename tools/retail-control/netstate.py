#!/usr/bin/env python3
"""Fail-closed, read-only retail multiplayer-menu snapshot.

This tool deliberately composes the existing x86 ``donject modules`` and ``peek``
operations instead of adding another injector command before the retail relaunch.  It
reads only module identities, pointer presence/scalar fields, and the PlayFab title-id
``std::string`` object.  It never reads the adjacent developer secret or follows player,
lobby, descriptor, ticket, token, platform-id, or display-name fields.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import json
import re
import shlex
import subprocess
from typing import Protocol


VM = "Windows 11"
INJECTOR = r"C:\Users\ember\donhook\donject.exe"

EXPECTED_MODULES = {
    "riseofnations.exe": (None, "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079"),
    "crossplayproxy.dll": (0xCF000, "ec7a6c18f03c6d7463fc3d36b7f663ec23c1477225d46b202b29756ef7554abc"),
    "crossplaynetlib.dll": (0x133000, "d716caafa565fbe9a914ae912b981573d14e7fa19efb73d500bbd5016de6ab60"),
    "partywin.dll": (0x24C000, "2f88d21fd95f5fc87b75bcf85f9fc905c9feec1dbcc5ef415613a6fa0a46ae2a"),
    "playfabmultiplayerwin.dll": (0x35F000, "3a6e7a0fadce8c9ddb5da8a571de4c58fb9f040faf6a5f68c9962d93e4c4fad8"),
}

NETSYS_RVA = 0x11E5F4
NETLIB_SYS_RVA = 0x11EDF8
PLAYFAB_SETTINGS_RVA = 0x0C2ED8
TITLE_ID_OFFSET = 56
TITLE_OBJECT_SIZE = 24
MAX_TITLE_ID_BYTES = 32


class NetStateError(RuntimeError):
    """A deliberately redacted refusal."""


@dataclass(frozen=True)
class Module:
    name: str
    path: str
    base: int
    size: int


@dataclass(frozen=True)
class Peek:
    module: str
    base: int
    address: int
    root_address: int
    root_value: int
    stable: int
    data: bytes


class Probe(Protocol):
    def modules(self, pid: int) -> list[Module]: ...

    def sha256(self, path: str) -> str: ...

    def peek(
        self, pid: int, module: str, rva: int, derefs: int, offset: int, length: int
    ) -> Peek: ...


def _run(arguments: list[str]) -> tuple[int, str]:
    result = subprocess.run(
        arguments,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    return result.returncode, result.stdout.replace("\r\n", "\n")


def _parse_kv_record(line: str) -> dict[str, str]:
    try:
        tokens = shlex.split(line)
    except ValueError as exc:
        raise NetStateError("injector record has invalid quoting") from exc
    fields: dict[str, str] = {}
    for token in tokens:
        if "=" not in token:
            raise NetStateError("injector record contains a bare token")
        key, value = token.split("=", 1)
        if not re.fullmatch(r"[a-z][a-z0-9_]*", key) or not value or key in fields:
            raise NetStateError("injector record contains invalid or duplicate fields")
        fields[key] = value
    return fields


def parse_modules(output: str, expected_pid: int) -> list[Module]:
    records = []
    for raw in output.splitlines():
        line = raw.strip()
        if line.startswith("protocol=donject.v2 "):
            records.append(_parse_kv_record(line))
    headers = [row for row in records if row.get("status") == "ok"]
    errors = [row for row in records if row.get("status") == "error"]
    rows = [row for row in records if row.get("status") == "module"]
    if errors or len(headers) != 1:
        raise NetStateError("module inventory is incomplete or reported an error")
    if any(
        row.get("protocol") != "donject.v2" or row.get("command") != "modules"
        for row in records
    ):
        raise NetStateError("module inventory contains a foreign protocol record")
    header = headers[0]
    if header.get("protocol") != "donject.v2" or header.get("command") != "modules":
        raise NetStateError("module inventory protocol identity is wrong")
    try:
        pid = int(header["pid"])
        count = int(header["count"])
    except (KeyError, ValueError) as exc:
        raise NetStateError("module inventory header numerics are invalid") from exc
    if pid != expected_pid or not 0 <= count <= 4096 or len(rows) != count:
        raise NetStateError("module inventory count or process identity is invalid")

    modules: list[Module] = []
    indices = set()
    for row in rows:
        try:
            row_pid = int(row["pid"])
            index = int(row["index"])
            base = int(row["module_base"], 16)
            size = int(row["module_size"], 16)
            name = row["module_name"]
            path = row["module_path"]
        except (KeyError, ValueError) as exc:
            raise NetStateError("module inventory row is malformed") from exc
        if (
            row_pid != pid
            or index in indices
            or not 0 <= index < count
            or base <= 0
            or size <= 0
            or not name
            or not path
        ):
            raise NetStateError("module inventory row identity is invalid")
        indices.add(index)
        modules.append(Module(name=name, path=path, base=base, size=size))
    if indices != set(range(count)):
        raise NetStateError("module inventory indices are not contiguous")
    return modules


def parse_peek(output: str, expected_module: str, expected_length: int) -> Peek:
    headers: list[dict[str, str]] = []
    row_bytes: dict[int, bytes] = {}
    for raw in output.splitlines():
        line = raw.strip()
        if line.startswith("# "):
            headers.append(_parse_kv_record(line[2:]))
            continue
        match = re.fullmatch(r"([0-9A-Fa-f]{8}):((?: [0-9A-Fa-f]{2}){1,16})", line)
        if match:
            address = int(match.group(1), 16)
            if address in row_bytes:
                raise NetStateError("peek contains duplicate data rows")
            row_bytes[address] = bytes.fromhex(match.group(2))
    if len(headers) != 1:
        raise NetStateError("peek header is missing or ambiguous")
    header = headers[0]
    required = {
        "base", "addr", "len", "module", "rva", "deref", "nderef", "off",
        "root", "pointer_addr", "root_value", "stable",
    }
    if set(header) != required or header["module"].lower() != expected_module.lower():
        raise NetStateError("peek header identity is invalid")
    try:
        base = int(header["base"], 16)
        address = int(header["addr"], 16)
        length = int(header["len"], 16)
        root = int(header["root"], 16)
        pointer_addr = int(header["pointer_addr"], 16)
        root_value = int(header["root_value"], 16)
        stable = int(header["stable"])
        deref = int(header["deref"])
        nderef = int(header["nderef"])
        rva = int(header["rva"], 16)
        offset = int(header["off"], 16)
    except ValueError as exc:
        raise NetStateError("peek header numerics are invalid") from exc
    if (
        base <= 0
        or length != expected_length
        or pointer_addr != root
        or deref != nderef
        or stable not in {-1, 0, 1}
        or root != base + rva
        or root > 0xFFFFFFFF
    ):
        raise NetStateError("peek header bounds or stability are invalid")
    if deref == 0:
        if stable != -1 or root_value != 0 or address != root + offset:
            raise NetStateError("direct peek address/stability contract is invalid")
    elif deref == 1:
        if stable not in {0, 1} or root_value == 0 or address != root_value + offset:
            raise NetStateError("rooted peek address/stability contract is invalid")
    else:
        raise NetStateError("menu-state parser accepts only direct or one-root peeks")
    data = bytearray()
    cursor = address
    while len(data) < length:
        chunk = row_bytes.get(cursor)
        if chunk is None or len(chunk) > length - len(data):
            raise NetStateError("peek data rows are incomplete or non-contiguous")
        data.extend(chunk)
        cursor += len(chunk)
    if len(row_bytes) != (length + 15) // 16:
        raise NetStateError("peek contains unexpected extra data rows")
    return Peek(
        module=header["module"],
        base=base,
        address=address,
        root_address=root,
        root_value=root_value,
        stable=stable,
        data=bytes(data),
    )


class DonjectProbe:
    def modules(self, pid: int) -> list[Module]:
        code, output = _run(
            ["prlctl", "exec", VM, "cmd.exe", "/d", "/s", "/c", f'"{INJECTOR}" modules {pid}']
        )
        if code != 0:
            raise NetStateError("x86 module inventory command failed")
        return parse_modules(output, pid)

    def sha256(self, path: str) -> str:
        escaped = path.replace("'", "''")
        code, output = _run(
            [
                "prlctl", "exec", VM, "powershell.exe", "-NoProfile", "-Command",
                f"(Get-FileHash -Algorithm SHA256 -LiteralPath '{escaped}').Hash",
            ]
        )
        hashes = set(re.findall(r"(?i)(?<![0-9a-f])([0-9a-f]{64})(?![0-9a-f])", output))
        if code != 0 or len(hashes) != 1:
            raise NetStateError("loaded module hash query failed")
        return next(iter(hashes)).lower()

    def peek(
        self, pid: int, module: str, rva: int, derefs: int, offset: int, length: int
    ) -> Peek:
        if (
            pid <= 0
            or not re.fullmatch(r"[A-Za-z0-9_.-]+", module)
            or not 0 <= rva <= 0xFFFFFFFF
            or not 0 <= derefs <= 32
            or not 0 <= offset <= 0xFFFFFFFF
            or not 1 <= length <= 4096
        ):
            raise NetStateError("peek request is outside the bounded menu-state contract")
        command = (
            f'"{INJECTOR}" peek {pid} {module} {rva:x} {derefs} {offset:x} {length:x}'
        )
        code, output = _run(["prlctl", "exec", VM, "cmd.exe", "/d", "/s", "/c", command])
        if code != 0:
            raise NetStateError("bounded menu-state read failed")
        return parse_peek(output, module, length)


def _module_map(modules: list[Module]) -> dict[str, Module]:
    result: dict[str, Module] = {}
    for module in modules:
        key = module.name.lower()
        if key in result:
            raise NetStateError("required module name is mapped more than once")
        result[key] = module
    return result


def _rooted_read(
    probe: Probe, pid: int, module: str, rva: int, offset: int, length: int
) -> Peek:
    result = probe.peek(pid, module, rva, 1, offset, length)
    if result.stable != 1 or result.root_value == 0:
        raise NetStateError("singleton root changed or was null during the bounded read")
    return result


def _absolute_read(
    probe: Probe, pid: int, modules: list[Module], address: int, length: int
) -> bytes:
    candidates = [module for module in modules if module.base <= address]
    if not candidates:
        raise NetStateError("title-id buffer is below every mapped module base")
    anchor = min(candidates, key=lambda module: module.base)
    rva = address - anchor.base
    if rva > 0xFFFFFFFF or address + length > 0x1_0000_0000:
        raise NetStateError("title-id buffer address is outside the x86 address space")
    result = probe.peek(pid, anchor.name, rva, 0, 0, length)
    if result.address != address or result.stable != -1:
        raise NetStateError("absolute title-id read returned the wrong address contract")
    return result.data


def _decode_title_object(
    probe: Probe, pid: int, modules: list[Module], raw: bytes
) -> bytes:
    if len(raw) != TITLE_OBJECT_SIZE:
        raise NetStateError("title-id string object has the wrong size")
    size = int.from_bytes(raw[16:20], "little")
    capacity = int.from_bytes(raw[20:24], "little")
    if not 1 <= size <= MAX_TITLE_ID_BYTES or capacity < size or capacity > 4096:
        raise NetStateError("title-id string size/capacity is invalid")
    if capacity < 16:
        if capacity != 15:
            raise NetStateError("title-id small-string capacity is not the MSVC sentinel")
        return raw[:size]
    pointer = int.from_bytes(raw[:4], "little")
    if pointer == 0:
        raise NetStateError("title-id heap string has a null data pointer")
    first = _absolute_read(probe, pid, modules, pointer, size)
    second = _absolute_read(probe, pid, modules, pointer, size)
    if first != second:
        raise NetStateError("title-id heap bytes changed during the bracket")
    return first


def _u32(raw: bytes) -> int:
    if len(raw) != 4:
        raise NetStateError("scalar read has the wrong width")
    return int.from_bytes(raw, "little")


def collect(pid: int, probe: Probe) -> dict:
    if pid <= 0:
        raise NetStateError("process id must be positive")
    modules = probe.modules(pid)
    by_name = _module_map(modules)
    identities = {}
    for name, (expected_size, expected_hash) in EXPECTED_MODULES.items():
        module = by_name.get(name)
        if module is None:
            raise NetStateError(f"required module is absent: {name}")
        if expected_size is not None and module.size != expected_size:
            raise NetStateError(f"required module image size is wrong: {name}")
        actual_hash = probe.sha256(module.path)
        if actual_hash != expected_hash:
            raise NetStateError(f"required module hash is wrong: {name}")
        identities[name] = {"sha256": actual_hash, "image_size": module.size}

    netsys = _rooted_read(probe, pid, "CrossplayNetLib.dll", NETSYS_RVA, 4, 44)
    netsys_b = _rooted_read(probe, pid, "CrossplayNetLib.dll", NETSYS_RVA, 4, 44)
    if netsys.root_value != netsys_b.root_value or netsys.data != netsys_b.data:
        raise NetStateError("NetSys pointer snapshot changed during the bracket")
    player_count = _u32(netsys.data[:4])
    if player_count > 8:
        raise NetStateError("NetSys player count exceeds the eight-slot retail bound")
    player_ptrs = [
        _u32(netsys.data[4 + index * 4:8 + index * 4]) for index in range(8)
    ]
    local_ptr = _u32(netsys.data[36:40])
    host_ptr = _u32(netsys.data[40:44])
    present_mask = sum((pointer != 0) << index for index, pointer in enumerate(player_ptrs))
    if present_mask.bit_count() < player_count:
        raise NetStateError("NetSys player pointers are inconsistent with its count")

    sys_flags_a = _rooted_read(probe, pid, "CrossplayNetLib.dll", NETLIB_SYS_RVA, 88, 4)
    session_a = _rooted_read(probe, pid, "CrossplayNetLib.dll", NETLIB_SYS_RVA, 96, 4)
    launched_a = _rooted_read(probe, pid, "CrossplayNetLib.dll", NETLIB_SYS_RVA, 468, 1)
    sys_flags_b = _rooted_read(probe, pid, "CrossplayNetLib.dll", NETLIB_SYS_RVA, 88, 4)
    session_b = _rooted_read(probe, pid, "CrossplayNetLib.dll", NETLIB_SYS_RVA, 96, 4)
    launched_b = _rooted_read(probe, pid, "CrossplayNetLib.dll", NETLIB_SYS_RVA, 468, 1)
    roots = {
        sys_flags_a.root_value, session_a.root_value, launched_a.root_value,
        sys_flags_b.root_value, session_b.root_value, launched_b.root_value,
    }
    if (
        len(roots) != 1
        or sys_flags_a.data != sys_flags_b.data
        or session_a.data != session_b.data
        or launched_a.data != launched_b.data
    ):
        raise NetStateError("Crossplay system snapshot changed during the bracket")

    title_a = _rooted_read(
        probe, pid, "CrossplayProxy.dll", PLAYFAB_SETTINGS_RVA, TITLE_ID_OFFSET,
        TITLE_OBJECT_SIZE,
    )
    title_bytes = _decode_title_object(probe, pid, modules, title_a.data)
    title_b = _rooted_read(
        probe, pid, "CrossplayProxy.dll", PLAYFAB_SETTINGS_RVA, TITLE_ID_OFFSET,
        TITLE_OBJECT_SIZE,
    )
    if title_a.root_value != title_b.root_value or title_a.data != title_b.data:
        raise NetStateError("PlayFab title-id object changed during the bracket")
    try:
        title_id = title_bytes.decode("ascii")
    except UnicodeDecodeError as exc:
        raise NetStateError("PlayFab title id is not ASCII") from exc
    if not re.fullmatch(r"[A-Za-z0-9_-]{1,32}", title_id):
        raise NetStateError("PlayFab title id contains unexpected characters")

    modules_after = _module_map(probe.modules(pid))
    for name, module in ((name, by_name[name]) for name in EXPECTED_MODULES):
        if modules_after.get(name) != module:
            raise NetStateError(f"required module identity changed during snapshot: {name}")
        if probe.sha256(module.path) != EXPECTED_MODULES[name][1]:
            raise NetStateError(f"required module file changed during snapshot: {name}")

    return {
        "schema": "don.retail-netstate.v1",
        "ready": True,
        "pid": pid,
        "mutation": "none; bounded read-only module/hash/scalar/title-id snapshot",
        "modules": identities,
        "netsys": {
            "player_count": player_count,
            "player_pointer_present_mask": present_mask,
            "local_player_pointer_present": local_ptr != 0,
            "host_player_pointer_present": host_ptr != 0,
        },
        "crossplay": {
            "flags": _u32(sys_flags_a.data),
            "current_session_present": _u32(session_a.data) != 0,
            "lobby_launched": launched_a.data[0] != 0,
        },
        "playfab_title_id": title_id,
        "redacted_by_design": [
            "developer_secret", "tickets", "tokens", "lobby_ids", "descriptors",
            "platform_ids", "player_ids", "player_names",
        ],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pid", required=True, type=int)
    args = parser.parse_args()
    try:
        result = collect(args.pid, DonjectProbe())
    except NetStateError as exc:
        print(f"REFUSING netstate: {exc}")
        return 2
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
