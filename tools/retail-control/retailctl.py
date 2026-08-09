#!/usr/bin/env python3
"""Host-side controller for the fail-closed retail-control DLL."""

from __future__ import annotations

import argparse
import base64
import copy
import hashlib
import http.server
import ipaddress
import json
import os
from pathlib import Path, PureWindowsPath
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
DEFAULT_GENERATION_BUDGET = 1
INJECTOR = r"C:\Users\ember\donhook\donject.exe"
TURN_CALL_RVA = 0x00191686
TURN_DO_FRAME_RVA = 0x00557DD0
ORIGINAL_TURN_CALL = bytes.fromhex("e8 45 67 3c 00")
PREFLIGHT_JSON_BEGIN = "DON_RETAIL_PREFLIGHT_JSON_BEGIN"
PREFLIGHT_JSON_END = "DON_RETAIL_PREFLIGHT_JSON_END"
EXPECTED_DUMP_FOLDER = r"C:\Users\Public\don-crashdumps\riseofnations"
MIN_DUMP_FREE_BYTES = 12 * 1024 * 1024 * 1024
DAMAGE_HOOK = HERE.parent / "damage-hook"
INJECTOR_SOURCE = DAMAGE_HOOK / "donject.c"
HOST_INJECTOR = DAMAGE_HOOK / "donject.exe"
RETAIL_ROOT = r"C:\Program Files (x86)\Steam\steamapps\common\Rise of Nations"
RETAIL_EXE = RETAIL_ROOT + r"\riseofnations.exe"
RETAIL_NETSYS_DLL = RETAIL_ROOT + r"\CrossplayNetLib.dll"
EXPECTED_NETSYS_SHA256 = "d716caafa565fbe9a914ae912b981573d14e7fa19efb73d500bbd5016de6ab60"
EXPECTED_NETSYS_SIZE = 1_198_592
NETSYS_ROOT = r"C:\Users\Public\don-netsys-experiment"
NETSYS_BACKUP = NETSYS_ROOT + r"\CrossplayNetLib.shipped.dll"
NETSYS_STAGED = NETSYS_ROOT + r"\CrossplayNetLib.experimental.dll"
NETSYS_MANIFEST = NETSYS_ROOT + r"\manifest.json"
NETSYS_LAUNCHER = NETSYS_ROOT + r"\launch.cmd"
NETSYS_LOAD_TRACE = NETSYS_ROOT + r"\trace-load-only.log"
NETSYS_HOST_TRACE = NETSYS_ROOT + r"\trace-host.log"
NETSYS_BRIDGE_TRACE = NETSYS_ROOT + r"\trace-host-bridge.log"
NETSYS_LOAD_EXIT = NETSYS_ROOT + r"\exit-load-only.txt"
NETSYS_HOST_EXIT = NETSYS_ROOT + r"\exit-host.txt"
NETSYS_BRIDGE_EXIT = NETSYS_ROOT + r"\exit-host-bridge.txt"
NETSYS_TASK_NAME = "don-netsys-experiment"
DEFAULT_NETSYS_SHIM = (
    HERE.parents[1] / "crates/netsys-shim/target/i686-pc-windows-msvc/release/"
    "CrossplayNetLib.dll"
)
NETSYS_SCHEMA = "don.retail-netsys-experiment.v1"
NETSYS_JSON_BEGIN = "DON_NETSYS_JSON_BEGIN"
NETSYS_JSON_END = "DON_NETSYS_JSON_END"
NETSYS_NORMAL_EXIT_CODES = frozenset({0, 8008})


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


def normalize_windows_path(path: str) -> str:
    normalized = path.replace("/", "\\")
    if normalized.startswith("\\\\?\\UNC\\"):
        normalized = "\\\\" + normalized[len("\\\\?\\UNC\\"):]
    elif normalized.startswith("\\\\?\\"):
        normalized = normalized[len("\\\\?\\"):]
    return normalized.rstrip("\\").lower()


def parse_donject_fields(record: str) -> dict[str, str] | None:
    """Parse donject's space-separated machine record without unescaping Windows paths."""
    fields: dict[str, str] = {}
    cursor = 0
    token = re.compile(r'([a-z][a-z0-9_]*)=(?:"([^"\r\n]*)"|([^\s"]+))')
    while cursor < len(record):
        match = token.match(record, cursor)
        if not match:
            return None
        key = match.group(1)
        value = match.group(2) if match.group(2) is not None else match.group(3)
        if key in fields or not value:
            return None
        fields[key] = value
        cursor = match.end()
        if cursor == len(record):
            break
        separator = re.match(r" +", record[cursor:])
        if not separator:
            return None
        cursor += separator.end()
        if cursor == len(record):
            return None
    return fields


def run(cmd: list[str], *, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                          check=check)


def guest_cmd(command: str, *, check: bool = True) -> str:
    p = run(["prlctl", "exec", VM, "cmd.exe", "/d", "/s", "/c", command], check=check)
    return p.stdout.replace("\r\n", "\n").strip()


def guest_cmd_status(command: str) -> tuple[int, str]:
    p = run(["prlctl", "exec", VM, "cmd.exe", "/d", "/s", "/c", command], check=False)
    return p.returncode, p.stdout.replace("\r\n", "\n").strip()


def guest_ps(command: str, *, check: bool = True) -> str:
    p = run(["prlctl", "exec", VM, "powershell.exe", "-NoProfile", "-Command", command],
            check=check)
    return p.stdout.replace("\r\n", "\n").strip()


def guest_ps_encoded(command: str, *, check: bool = True) -> str:
    """Run PowerShell without letting prlctl consume the script's quotes."""
    encoded = base64.b64encode(command.encode("utf-16le")).decode("ascii")
    p = run(
        ["prlctl", "exec", VM, "powershell.exe", "-NoProfile", "-EncodedCommand", encoded],
        check=check,
    )
    return p.stdout.replace("\r\n", "\n").strip()


def ps_literal(value: str) -> str:
    return "'" + value.replace("'", "''") + "'"


def pid() -> int:
    try:
        values, out = process_pids()
    except RuntimeError as exc:
        raise SystemExit(str(exc)) from exc
    if len(values) != 1:
        raise SystemExit(f"expected one riseofnations.exe, got {values!r}:\n{out}")
    return values[0]


def process_pids() -> tuple[list[int], str]:
    returncode, out = guest_cmd_status(
        "for /f \"tokens=2\" %p in "
        "('tasklist /nh /fi \"imagename eq riseofnations.exe\"') do @echo %p"
    )
    if returncode != 0:
        raise RuntimeError(
            f"could not enumerate riseofnations.exe processes (exit {returncode}):\n{out}"
        )
    values = [line.strip() for line in out.splitlines() if line.strip().isdigit()]
    return sorted({int(value) for value in values}), out


class ReusableTCPServer(socketserver.TCPServer):
    allow_reuse_address = True


def serve_once(port: int, directory: Path = HERE):
    handler = lambda *a, **kw: http.server.SimpleHTTPRequestHandler(  # noqa: E731
        *a, directory=str(directory), **kw
    )
    server = ReusableTCPServer(("0.0.0.0", port), handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server


def build() -> None:
    p = run(["sh", str(HERE / "build.sh")])
    print(p.stdout, end="")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def build_injector(output: Path = HOST_INJECTOR) -> str:
    output.parent.mkdir(parents=True, exist_ok=True)
    command = [
        "zig", "cc", "-target", "x86-windows-gnu", "-O2",
        "-Wall", "-Wextra", "-Werror", "-s", "-o", str(output), str(INJECTOR_SOURCE),
    ]
    result = run(command)
    if result.stdout:
        print(result.stdout, end="")
    identity = run(["file", str(output)]).stdout.strip()
    if "PE32 executable" not in identity or "Intel 80386" not in identity:
        raise RuntimeError(f"injector build has unexpected identity: {identity}")
    return sha256_file(output)


def parse_single_sha256(output: str) -> str | None:
    hashes = sorted(set(value.lower() for value in re.findall(
        r"(?<![0-9A-Fa-f])([0-9A-Fa-f]{64})(?![0-9A-Fa-f])", output
    )))
    return hashes[0] if len(hashes) == 1 else None


def guest_sha256(path: str) -> str | None:
    output = guest_ps(
        f"if (Test-Path -LiteralPath '{path}' -PathType Leaf) {{ "
        f"(Get-FileHash -Algorithm SHA256 -LiteralPath '{path}').Hash }}",
        check=False,
    )
    return parse_single_sha256(output)


def injector_diagnostic() -> dict:
    try:
        host_hash = build_injector()
    except Exception as exc:
        return {
            "ready": False,
            "host_sha256": None,
            "guest_sha256": None,
            "selftest": None,
            "issues": [f"strict host injector build failed: {exc}"],
            "mutation": "guest read-only; host build artifact only",
        }
    guest_hash = guest_sha256(INJECTOR)
    selftest_returncode, selftest_output = guest_cmd_status(f'"{INJECTOR}" selftest')
    selftest_ok = (
        selftest_returncode == 0 and
        "selftest: status=ok architecture=PE32/i386" in selftest_output
    )
    issues = []
    if guest_hash != host_hash:
        issues.append("guest injector hash does not match the strict current host build")
    if not selftest_ok:
        issues.append("guest injector selftest did not report PE32/i386 success")
    return {
        "ready": not issues,
        "host_sha256": host_hash,
        "guest_sha256": guest_hash,
        "selftest": selftest_output,
        "issues": issues,
        "mutation": "guest read-only; host build artifact only",
    }


def prepare_injector(port: int = 18081) -> dict:
    host_hash = build_injector()
    server = serve_once(port, DAMAGE_HOOK)
    download = INJECTOR + ".download"
    try:
        guest_cmd(r'if not exist "C:\Users\ember\donhook" mkdir "C:\Users\ember\donhook"')
        guest_cmd(
            f'curl.exe -f -sS -o "{download}" '
            f'http://10.211.55.2:{port}/{HOST_INJECTOR.name}'
        )
        downloaded_hash = guest_sha256(download)
        if downloaded_hash != host_hash:
            raise SystemExit(
                "REFUSING injector install: guest download hash does not match host build"
            )
        guest_cmd(f'move /y "{download}" "{INJECTOR}" >nul')
        installed_hash = guest_sha256(INJECTOR)
        if installed_hash != host_hash:
            raise SystemExit(
                "REFUSING injector install: atomically installed guest hash does not match"
            )
        returncode, selftest = guest_cmd_status(f'"{INJECTOR}" selftest')
        if (returncode != 0 or
                "selftest: status=ok architecture=PE32/i386" not in selftest):
            raise SystemExit(f"REFUSING injector install: guest selftest failed:\n{selftest}")
        result = {
            "schema": "don.injector-prepare.v1",
            "host_sha256": host_hash,
            "guest_sha256": installed_hash,
            "selftest": selftest,
            "ready": True,
        }
        print(json.dumps(result, indent=2, sort_keys=True))
        return result
    finally:
        guest_cmd(f'del /q "{download}" 2>nul & exit /b 0', check=False)
        server.shutdown()
        server.server_close()


def preflight(target_pid: int) -> None:
    out = guest_ps(f"(Get-FileHash -Algorithm SHA256 (Get-Process -Id {target_pid}).Path).Hash")
    compact = "".join(out.lower().split())
    if EXPECTED_SHA256 not in compact:
        raise SystemExit(f"REFUSING unsupported target digest; expected {EXPECTED_SHA256}:\n{out}")


def extract_json_between(output: str, begin: str, end: str) -> object:
    lines = output.splitlines()
    starts = [i for i, line in enumerate(lines) if line.strip() == begin]
    ends = [i for i, line in enumerate(lines) if line.strip() == end]
    if len(starts) != 1 or len(ends) != 1 or ends[0] <= starts[0]:
        raise ValueError("response has missing or ambiguous JSON markers")
    payload = "\n".join(lines[starts[0] + 1:ends[0]]).strip()
    if not payload:
        raise ValueError("response has an empty JSON payload")
    try:
        return json.loads(payload)
    except json.JSONDecodeError as exc:
        raise ValueError("response contains malformed JSON") from exc


def extract_marked_json(output: str) -> object:
    return extract_json_between(output, PREFLIGHT_JSON_BEGIN, PREFLIGHT_JSON_END)


def parse_ready_record(raw: object) -> dict:
    record: dict[str, object] = {"present": raw is not None, "values": {}, "errors": []}
    if raw is None:
        return record
    if not isinstance(raw, str):
        record["errors"].append("ready record is not text")
        return record
    values: dict[str, str] = {}
    for line in raw.replace("\r\n", "\n").splitlines():
        if not line:
            continue
        if "=" not in line:
            record["errors"].append(f"malformed ready line: {line!r}")
            continue
        key, value = line.split("=", 1)
        if not re.fullmatch(r"[a-z][a-z0-9_]*", key) or not value:
            record["errors"].append(f"malformed ready line: {line!r}")
            continue
        if key in values:
            record["errors"].append(f"duplicate ready key: {key}")
            continue
        values[key] = value
    record["values"] = values
    return record


def generation_from_root_name(root_name: str) -> str | None:
    base = GUEST_ROOT_BASE.replace("/", "\\").rstrip("\\").rsplit("\\", 1)[-1]
    if root_name.lower() == base.lower():
        return LEGACY_GENERATION
    prefix = base + "-"
    if not root_name.lower().startswith(prefix.lower()):
        return None
    generation = root_name[len(prefix):]
    try:
        return validate_generation(generation)
    except SystemExit:
        return None


def parse_deployed_generations(output: str) -> tuple[list[dict], list[str]]:
    payload = extract_marked_json(output)
    if not isinstance(payload, list):
        raise ValueError("deployed-generation payload is not an array")
    rows: list[dict] = []
    errors: list[str] = []
    for item in payload:
        if not isinstance(item, dict):
            errors.append("deployed-generation row is not an object")
            continue
        root_name = item.get("root_name")
        root = item.get("root")
        dlls = item.get("dlls")
        downloads = item.get("downloads")
        if (not isinstance(root_name, str) or not isinstance(root, str) or
                not isinstance(dlls, list) or not isinstance(downloads, list) or
                any(not isinstance(value, str) for value in [*dlls, *downloads])):
            errors.append("deployed-generation row has invalid fields")
            continue
        generation = generation_from_root_name(root_name)
        if generation is None:
            errors.append(f"unsafe or unrecognized controller root: {root_name!r}")
            continue
        expected_root = generation_root(generation)
        if root.lower() != expected_root.lower():
            errors.append(f"controller root does not match generation {generation!r}: {root!r}")
            continue
        rows.append({
            "generation": generation,
            "root": root,
            "expected_dll": generation_dll(generation),
            "dlls": sorted(set(dlls), key=str.lower),
            "downloads": sorted(set(downloads), key=str.lower),
            "ready": parse_ready_record(item.get("ready")),
        })
    rows.sort(key=lambda row: row["generation"].lower())
    return rows, errors


def deployed_generations() -> tuple[list[dict], list[str]]:
    script = f"""
$ErrorActionPreference = 'Stop'
$rows = @()
Get-ChildItem -LiteralPath 'C:\\Users\\Public' -Directory -Filter 'don-retail-control*' |
    Sort-Object Name | ForEach-Object {{
        $readyPath = Join-Path $_.FullName 'ready.txt'
        $ready = if (Test-Path -LiteralPath $readyPath -PathType Leaf) {{
            [IO.File]::ReadAllText($readyPath)
        }} else {{ $null }}
        $dlls = @(Get-ChildItem -LiteralPath $_.FullName -File -Filter 'retail_control*.dll' |
            Sort-Object Name | ForEach-Object {{ $_.Name }})
        $downloads = @(Get-ChildItem -LiteralPath $_.FullName -File -Filter '*.download' |
            Sort-Object Name | ForEach-Object {{ $_.Name }})
        $rows += [pscustomobject]@{{
            root_name = $_.Name
            root = $_.FullName
            dlls = $dlls
            downloads = $downloads
            ready = $ready
        }}
    }}
Write-Output '{PREFLIGHT_JSON_BEGIN}'
ConvertTo-Json -InputObject @($rows) -Compress -Depth 5
Write-Output '{PREFLIGHT_JSON_END}'
"""
    return parse_deployed_generations(guest_ps(script))


def parse_module_base_output(output: str) -> dict:
    records = [line.strip() for line in output.splitlines()
               if line.strip().startswith("protocol=donject.v2 ")]
    if len(records) != 1:
        return {
            "status": "error",
            "detail": "module probe did not return exactly one donject.v2 record",
        }
    fields = parse_donject_fields(records[0])
    if (fields is None or fields.get("protocol") != "donject.v2" or
            fields.get("command") != "base"):
        return {
            "status": "error",
            "detail": "module probe did not return an unambiguous donject.v2 record",
        }
    status = fields.get("status")
    if status == "mapped":
        base = fields.get("module_base", "")
        size = fields.get("module_size", "")
        if (not re.fullmatch(r"0x[0-9A-Fa-f]{8}", base) or int(base, 16) == 0 or
                not re.fullmatch(r"0x[0-9A-Fa-f]{8}", size) or int(size, 16) == 0):
            return {"status": "error", "detail": "mapped module record lacks base or size"}
        return {
            "status": "mapped",
            "base": int(base, 16),
            "size": int(size, 16),
            "pid": fields.get("pid"),
            "module_name": fields.get("module_name"),
            "module_path": fields.get("module_path"),
        }
    if status == "absent":
        return {
            "status": "absent",
            "pid": fields.get("pid"),
            "module_name": fields.get("module_name"),
        }
    if status == "error":
        return {
            "status": "error",
            "detail": "injector module probe reported an error",
            "stage": fields.get("stage"),
            "win32_error": fields.get("win32_error"),
        }
    return {"status": "error", "detail": "injector module status was not recognized"}


def parse_module_list_output(output: str) -> dict:
    records = []
    for raw in output.splitlines():
        line = raw.strip()
        if not line.startswith("protocol=donject.v2 "):
            continue
        fields = parse_donject_fields(line)
        if fields is None:
            return {"status": "error", "detail": "module-list record has invalid fields"}
        if fields.get("protocol") == "donject.v2" and fields.get("command") == "modules":
            records.append(fields)
    headers = [record for record in records if record.get("status") == "ok"]
    errors = [record for record in records if record.get("status") == "error"]
    modules = [record for record in records if record.get("status") == "module"]
    if errors:
        return {
            "status": "error",
            "detail": "injector module enumeration reported an error",
            "record": errors[0],
        }
    if len(headers) != 1:
        return {"status": "error", "detail": "module-list header is missing or ambiguous"}
    header = headers[0]
    try:
        target_pid = int(header.get("pid", ""))
        count = int(header.get("count", ""))
    except ValueError:
        return {"status": "error", "detail": "module-list header count or pid is invalid"}
    if count < 0 or count > 4096 or len(modules) != count:
        return {"status": "error", "detail": "module-list count is invalid or incomplete"}
    normalized = []
    indices = set()
    for module in modules:
        try:
            module_pid = int(module.get("pid", ""))
            index = int(module.get("index", ""))
            base_text = module.get("module_base", "")
            size_text = module.get("module_size", "")
            base = int(base_text, 16)
            size = int(size_text, 16)
        except ValueError:
            return {"status": "error", "detail": "module-list entry has invalid numerics"}
        name = module.get("module_name")
        path = module.get("module_path")
        if (module_pid != target_pid or index in indices or index < 0 or index >= count or
                not isinstance(name, str) or not name or
                not isinstance(path, str) or not path or
                not re.fullmatch(r"0x[0-9A-Fa-f]{8}", base_text) or base == 0 or
                not re.fullmatch(r"0x[0-9A-Fa-f]{8}", size_text) or size == 0):
            return {"status": "error", "detail": "module-list entry identity is invalid"}
        indices.add(index)
        normalized.append({
            "name": name,
            "path": path,
            "base": base,
            "size": size,
            "base_hex": f"0x{base:08x}",
            "size_hex": f"0x{size:x}",
        })
    if indices != set(range(count)):
        return {"status": "error", "detail": "module-list indices are not contiguous"}
    return {"status": "ok", "pid": target_pid, "modules": normalized}


def remote_modules(target_pid: int) -> dict:
    returncode, out = guest_cmd_status(f'"{INJECTOR}" modules {target_pid}')
    result = parse_module_list_output(out)
    if result["status"] == "ok" and result["pid"] != target_pid:
        result = {"status": "error", "detail": "module-list pid does not match request"}
    if result["status"] == "ok" and returncode != 0:
        result = {
            "status": "error",
            "detail": f"module-list status/exit mismatch: host observed {returncode}",
        }
    if result["status"] == "error":
        result["output"] = out
    return result


def module_probe(target_pid: int, dll_name: str) -> dict:
    # The host PowerShell is 64-bit and does not reliably enumerate emulated x86
    # modules. Use the already-deployed x86 Toolhelp probe in the same ABI instead.
    returncode, out = guest_cmd_status(f'"{INJECTOR}" base {target_pid} "{dll_name}"')
    result = parse_module_base_output(out)
    result["name"] = dll_name
    try:
        reported_pid = int(result.get("pid", ""))
    except (TypeError, ValueError):
        reported_pid = None
    reported_name = result.get("module_name")
    if (result["status"] != "error" and
            (reported_pid != target_pid or not isinstance(reported_name, str) or
             reported_name.lower() != dll_name.lower())):
        result = {
            "status": "error",
            "detail": "module probe identity does not match its request",
            "name": dll_name,
        }
    expected_returncode = 0 if result["status"] == "mapped" else (
        10 if result["status"] == "absent" else None
    )
    if expected_returncode is not None and returncode != expected_returncode:
        result = {
            "status": "error",
            "detail": (f"module probe status/exit mismatch: status expected "
                       f"{expected_returncode}, host observed {returncode}"),
            "name": dll_name,
        }
    if result["status"] == "mapped":
        result["base_hex"] = f"0x{result['base']:08x}"
        result["size_hex"] = f"0x{result['size']:x}"
    elif result["status"] == "error":
        result["output"] = out
    return result


def loaded_module(target_pid: int, dll_name: str) -> str:
    result = module_probe(target_pid, dll_name)
    if result["status"] == "mapped":
        return f"{dll_name}@{result['base_hex']}"
    if result["status"] == "error":
        raise SystemExit(
            f"REFUSING because module state for {dll_name!r} is indeterminate: "
            f"{result.get('detail', 'unknown injector error')}"
        )
    return ""


def parse_inject_output(output: str, returncode: int, target_pid: int,
                        dll_name: str, dll_path: str, expected_sha256: str) -> dict:
    matches = []
    pattern = re.compile(
        r"inject: result=(loaded|already-loaded) status=ok pid=([0-9]+) "
        r"module=([^\s]+) base=([0-9A-Fa-f]{8}) path=(.+) "
        r"sha256=([0-9A-Fa-f]{64})"
    )
    for raw in output.splitlines():
        match = pattern.fullmatch(raw.strip())
        if match:
            matches.append(match)
    if returncode != 0:
        return {
            "status": "indeterminate" if "inject: INDETERMINATE " in output else "error",
            "detail": output,
            "restart_required": True,
        }
    if len(matches) != 1:
        return {"status": "error", "detail": "injector success record is missing or ambiguous"}
    match = matches[0]
    result_kind, pid_text, module_name, base_text, module_path, dll_sha256 = match.groups()
    if (result_kind != "loaded" or int(pid_text) != target_pid or
            module_name.lower() != dll_name.lower() or
            normalize_windows_path(module_path) != normalize_windows_path(dll_path) or
            int(base_text, 16) == 0 or
            dll_sha256.lower() != expected_sha256.lower()):
        return {
            "status": "error",
            "detail": "injector success record does not match the requested new module",
        }
    return {
        "status": "loaded",
        "module_base": int(base_text, 16),
        "module_base_hex": f"0x{int(base_text, 16):08x}",
        "module_path": module_path,
    }


def parse_hook_peek_output(output: str) -> dict:
    header_matches: list[dict[str, str]] = []
    byte_matches = []
    for raw in output.splitlines():
        line = raw.strip()
        if line.startswith("# "):
            fields: dict[str, str] = {}
            valid = True
            for token in line[2:].split():
                if token.count("=") != 1:
                    valid = False
                    break
                key, value = token.split("=", 1)
                if not key or not value or key in fields:
                    valid = False
                    break
                fields[key] = value
            if valid:
                header_matches.append(fields)
            continue
        data = re.fullmatch(
            r"([0-9A-Fa-f]{8}):\s+"
            r"([0-9A-Fa-f]{2})\s+([0-9A-Fa-f]{2})\s+([0-9A-Fa-f]{2})\s+"
            r"([0-9A-Fa-f]{2})\s+([0-9A-Fa-f]{2})",
            line,
        )
        if data:
            byte_matches.append((
                int(data.group(1), 16),
                bytes(int(data.group(i), 16) for i in range(2, 7)),
            ))
    if len(header_matches) != 1 or len(byte_matches) != 1:
        return {"status": "unreadable", "detail": "ambiguous hook-byte response"}
    header = header_matches[0]
    required = {
        "base", "addr", "len", "module", "rva", "deref", "nderef", "off",
        "root", "pointer_addr", "root_value", "stable",
    }
    if set(header) != required or header["module"].lower() != "riseofnations.exe":
        return {"status": "unreadable", "detail": "hook-byte header identity mismatch"}
    try:
        base = int(header["base"], 16)
        address = int(header["addr"], 16)
        length = int(header["len"], 16)
        rva = int(header["rva"], 16)
        deref = int(header["deref"])
        nderef = int(header["nderef"])
        offset = int(header["off"], 16)
        root = int(header["root"], 16)
        pointer_addr = int(header["pointer_addr"], 16)
        root_value = int(header["root_value"], 16)
        stable = int(header["stable"])
    except ValueError:
        return {"status": "unreadable", "detail": "hook-byte header numerics are invalid"}
    if (
        base <= 0
        or length != 5
        or rva != TURN_CALL_RVA
        or deref != 0
        or nderef != 0
        or offset != 0
        or root != base + TURN_CALL_RVA
        or root > 0xFFFFFFFF
        or pointer_addr != root
        or root_value != 0
        or stable != -1
        or address != root
    ):
        return {"status": "unreadable", "detail": "hook-byte header bounds mismatch"}
    byte_address, call = byte_matches[0]
    if address != byte_address or address != base + TURN_CALL_RVA:
        return {"status": "unreadable", "detail": "hook-byte response address mismatch"}
    result = {
        "executable_base": f"0x{base:08x}",
        "call_site": f"0x{address:08x}",
        "bytes": call.hex(),
    }
    if call == ORIGINAL_TURN_CALL:
        result.update({
            "status": "original",
            "call_target": f"0x{base + TURN_DO_FRAME_RVA:08x}",
        })
        return result
    if call[0] != 0xE8:
        result.update({"status": "unknown", "detail": "call site is not a relative call"})
        return result
    displacement = int.from_bytes(call[1:], "little", signed=True)
    result.update({
        "status": "patched",
        "call_target": f"0x{(address + 5 + displacement) & 0xffffffff:08x}",
    })
    return result


def hook_call_state(target_pid: int) -> dict:
    out = guest_cmd(
        f'"{INJECTOR}" peek {target_pid} riseofnations.exe '
        f'{TURN_CALL_RVA:x} 0 0 5',
        check=False,
    )
    result = parse_hook_peek_output(out)
    if result["status"] == "unreadable":
        result["output"] = out
    return result


def controller_inventory(target_pid: int, extra_generation: str | None = None) -> dict:
    rows, scan_errors = deployed_generations()
    if extra_generation is not None:
        extra_generation = validate_generation(extra_generation)
        if not any(row["generation"].lower() == extra_generation.lower() for row in rows):
            rows.append({
                "generation": extra_generation,
                "root": generation_root(extra_generation),
                "expected_dll": generation_dll(extra_generation),
                "dlls": [],
                "downloads": [],
                "ready": parse_ready_record(None),
            })
            rows.sort(key=lambda row: row["generation"].lower())

    basename_roots: dict[str, list[str]] = {}
    for row in rows:
        names = set(row["dlls"])
        names.add(row["expected_dll"])
        for name in names:
            if not re.fullmatch(r"retail_control(?:-[A-Za-z0-9._-]+)?\.dll", name,
                                re.IGNORECASE):
                scan_errors.append(f"unexpected controller DLL name: {name!r}")
                continue
            basename_roots.setdefault(name.lower(), []).append(row["root"])

    listing = remote_modules(target_pid)
    if listing["status"] != "ok":
        scan_errors.append("x86 remote module enumeration failed")
        remote_controller_modules = []
    else:
        remote_controller_modules = [
            module for module in listing["modules"]
            if re.fullmatch(
                r"retail_control(?:-[A-Za-z0-9._-]+)?\.dll",
                module["name"], re.IGNORECASE,
            )
        ]
    remote_by_name: dict[str, list[dict]] = {}
    for module in remote_controller_modules:
        remote_by_name.setdefault(module["name"].lower(), []).append(module)
    probes: dict[str, dict] = {}
    for basename in sorted(basename_roots):
        matches = remote_by_name.get(basename, [])
        if len(matches) == 1:
            probes[basename] = {**matches[0], "status": "mapped"}
        elif not matches:
            probes[basename] = {"name": basename, "status": "absent"}
        else:
            probes[basename] = {
                "name": basename, "status": "error",
                "detail": "multiple mapped modules share this basename",
            }
    issues = list(scan_errors)
    for basename, roots in basename_roots.items():
        if len(set(root.lower() for root in roots)) > 1:
            issues.append(f"duplicate controller DLL basename across roots: {basename}")
    for probe in probes.values():
        if probe["status"] == "error":
            issues.append(f"could not determine module state for {probe['name']}")
    for basename, modules in remote_by_name.items():
        if basename not in basename_roots:
            issues.append(
                f"mapped controller has no immutable generation root: "
                f"{modules[0]['path']}"
            )
        if len(modules) > 1:
            issues.append(f"multiple mapped controller modules share basename: {basename}")

    generation_rows = []
    armed_mapped = []
    for row in rows:
        expected_name = row["expected_dll"].lower()
        probe = probes.get(expected_name, {
            "name": expected_name, "status": "error", "detail": "name was not probed"
        })
        ready = row["ready"]
        values = ready["values"]
        row_issues = []
        ready_issues = list(ready["errors"])
        if row["downloads"]:
            row_issues.append("incomplete DLL download remains on disk")
        if ready["present"]:
            if normalize_windows_path(values.get("root", "")) != normalize_windows_path(
                    row["root"]):
                ready_issues.append("ready root does not match generation root")
            try:
                ready_pid = int(values.get("pid", ""))
            except ValueError:
                ready_pid = None
                ready_issues.append("ready pid is missing or invalid")
            state = values.get("state")
            if not isinstance(state, str) or not (
                    state in {"armed", "parked"} or state.startswith("refused")):
                ready_issues.append("ready state is missing or invalid")
        else:
            ready_pid = None
            state = None
        if probe["status"] == "mapped":
            row_issues.extend(ready_issues)
            expected_path = f"{row['root']}\\{row['expected_dll']}"
            if normalize_windows_path(probe.get("path", "")) != normalize_windows_path(
                    expected_path):
                row_issues.append("mapped controller path does not match immutable generation")
            if not ready["present"]:
                row_issues.append("mapped controller has no ready record")
            elif ready_pid != target_pid:
                row_issues.append("mapped controller ready pid does not match target")
            elif state == "armed":
                armed_mapped.append(row["generation"])
        elif ready["present"] and ready_pid == target_pid:
            row_issues.extend(ready_issues)
            row_issues.append(
                f"current-pid {state or 'invalid'} ready record has no matching mapped module"
            )
        for name in row["dlls"]:
            normalized_name = name.lower()
            if normalized_name != expected_name and probes.get(normalized_name, {}).get(
                    "status") == "mapped":
                row_issues.append(f"unexpected mapped DLL in generation root: {name}")
        issues.extend(f"generation {row['generation']}: {issue}" for issue in row_issues)
        generation_rows.append({
            **row,
            "module": probe,
            "issues": row_issues,
        })

    hook = hook_call_state(target_pid)
    hook_owner = None
    if hook["status"] in {"unknown", "unreadable"}:
        issues.append("retail hook call site could not be classified")
    elif hook["status"] == "original" and armed_mapped:
        issues.append("armed ready record conflicts with original retail call bytes")
    elif hook["status"] == "patched":
        if len(armed_mapped) != 1:
            issues.append("patched retail call has ambiguous controller ownership")
        else:
            hook_owner = armed_mapped[0]
    mapped_modules = sorted(
        remote_controller_modules,
        key=lambda module: (module["name"].lower(), module["path"].lower()),
    )
    return {
        "enumeration": "complete x86 Toolhelp module list reconciled to immutable roots",
        "complete": listing["status"] == "ok" and not scan_errors and all(
            probe["status"] != "error" for probe in probes.values()
        ),
        "generations": generation_rows,
        "mapped_modules": mapped_modules,
        "mapped_generation_count": len(mapped_modules),
        "hook": hook,
        "hook_owner": hook_owner,
        "issues": sorted(set(issues)),
    }


def parse_wer_diagnostics(output: str) -> dict:
    payload = extract_marked_json(output)
    if not isinstance(payload, dict):
        raise ValueError("WER payload is not an object")
    views = payload.get("views")
    free_bytes = payload.get("free_bytes")
    service_status = payload.get("wer_service_status")
    if (not isinstance(views, list) or not isinstance(free_bytes, int) or
            not isinstance(service_status, str)):
        raise ValueError("WER payload has invalid fields")
    issues = []
    normalized_views = []
    seen_views = set()
    for view in views:
        if not isinstance(view, dict):
            raise ValueError("WER registry view is not an object")
        name = view.get("view")
        present = view.get("present")
        folder = view.get("folder")
        expanded_folder = view.get("expanded_folder")
        folder_exists = view.get("folder_exists")
        dump_type = view.get("dump_type")
        dump_count = view.get("dump_count")
        if (name not in {"32", "64"} or name in seen_views or
                not isinstance(present, bool) or
                folder is not None and not isinstance(folder, str) or
                expanded_folder is not None and not isinstance(expanded_folder, str) or
                not isinstance(folder_exists, bool) or
                dump_type is not None and not isinstance(dump_type, int) or
                dump_count is not None and not isinstance(dump_count, int)):
            raise ValueError("WER registry view has invalid fields")
        seen_views.add(name)
        view_issues = []
        if not present:
            view_issues.append("scoped riseofnations.exe LocalDumps key is missing")
        if not folder:
            view_issues.append("DumpFolder is not explicitly configured")
        elif (expanded_folder or "").rstrip("\\").lower() != EXPECTED_DUMP_FOLDER.lower():
            view_issues.append(f"DumpFolder is not the scoped capture path {EXPECTED_DUMP_FOLDER}")
        elif not folder_exists:
            view_issues.append("DumpFolder does not exist")
        if dump_type != 2:
            view_issues.append("DumpType is not an explicit full dump (2)")
        if dump_count != 2:
            view_issues.append("DumpCount is not explicitly 2")
        issues.extend(f"registry view {name}: {issue}" for issue in view_issues)
        normalized_views.append({
            "view": name,
            "present": present,
            "folder": folder,
            "expanded_folder": expanded_folder,
            "folder_exists": folder_exists,
            "dump_type": dump_type,
            "dump_count": dump_count,
            "ready": not view_issues,
            "issues": view_issues,
        })
    if seen_views != {"32", "64"}:
        raise ValueError("WER payload does not contain both registry views")
    if free_bytes < MIN_DUMP_FREE_BYTES:
        issues.append(
            f"dump volume has less than {MIN_DUMP_FREE_BYTES} bytes free"
        )
    return {
        "scope": (r"HKLM\SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps"
                  r"\riseofnations.exe"),
        "required_registry_views": ["32", "64"],
        "views": sorted(normalized_views, key=lambda view: view["view"]),
        "expected_folder": EXPECTED_DUMP_FOLDER,
        "free_bytes": free_bytes,
        "minimum_free_bytes": MIN_DUMP_FREE_BYTES,
        "wer_service_status": service_status,
        "wer_service_note": "demand-start service state is informational",
        "ready": not issues,
        "issues": issues,
        "mutation": "none; diagnostics are read-only",
    }


def wer_diagnostics() -> dict:
    script = rf"""
$ErrorActionPreference = 'Stop'
$subpath = 'SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps\riseofnations.exe'
$views = @()
foreach ($viewName in @('64', '32')) {{
    $registryView = if ($viewName -eq '64') {{
        [Microsoft.Win32.RegistryView]::Registry64
    }} else {{
        [Microsoft.Win32.RegistryView]::Registry32
    }}
    $baseKey = [Microsoft.Win32.RegistryKey]::OpenBaseKey(
        [Microsoft.Win32.RegistryHive]::LocalMachine, $registryView)
    try {{
        $key = $baseKey.OpenSubKey($subpath, $false)
        try {{
            $present = $null -ne $key
            $folder = if ($present) {{
                $key.GetValue('DumpFolder', $null,
                    [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
            }} else {{ $null }}
            $dumpType = if ($present) {{ $key.GetValue('DumpType', $null) }} else {{ $null }}
            $dumpCount = if ($present) {{ $key.GetValue('DumpCount', $null) }} else {{ $null }}
            $expanded = if ($null -ne $folder) {{
                [Environment]::ExpandEnvironmentVariables([string]$folder)
            }} else {{ $null }}
            $folderExists = ($null -ne $expanded) -and
                (Test-Path -LiteralPath $expanded -PathType Container)
            $views += [pscustomobject]@{{
                view = $viewName
                present = [bool]$present
                folder = $folder
                expanded_folder = $expanded
                folder_exists = [bool]$folderExists
                dump_type = $dumpType
                dump_count = $dumpCount
            }}
        }} finally {{
            if ($null -ne $key) {{ $key.Dispose() }}
        }}
    }} finally {{
        $baseKey.Dispose()
    }}
}}
$service = Get-Service -Name WerSvc -ErrorAction SilentlyContinue
$result = [pscustomobject]@{{
    views = $views
    free_bytes = [int64](Get-PSDrive -Name 'C').Free
    wer_service_status = if ($null -eq $service) {{ 'missing' }} else {{ [string]$service.Status }}
}}
Write-Output '{PREFLIGHT_JSON_BEGIN}'
ConvertTo-Json -InputObject $result -Compress -Depth 3
Write-Output '{PREFLIGHT_JSON_END}'
"""
    return parse_wer_diagnostics(guest_ps(script))


def target_digest_diagnostic(target_pid: int) -> dict:
    out = guest_ps(f"(Get-FileHash -Algorithm SHA256 (Get-Process -Id {target_pid}).Path).Hash")
    hashes = sorted(set(value.lower() for value in re.findall(
        r"(?<![0-9A-Fa-f])([0-9A-Fa-f]{64})(?![0-9A-Fa-f])", out
    )))
    return {
        "expected_sha256": EXPECTED_SHA256,
        "observed_sha256": hashes[0] if len(hashes) == 1 else None,
        "supported": hashes == [EXPECTED_SHA256],
        "response_unambiguous": len(hashes) == 1,
    }


def positive_generation_budget(value: str) -> int:
    try:
        budget = int(value)
    except ValueError as exc:
        raise argparse.ArgumentTypeError("generation budget must be an integer") from exc
    if budget < 1 or budget > 64:
        raise argparse.ArgumentTypeError("generation budget must be between 1 and 64")
    return budget


def prelaunch_report(max_generations: int, target_pid: int | None = None) -> dict:
    issues = []
    injector = injector_diagnostic()
    issues.extend(f"injector: {issue}" for issue in injector["issues"])
    try:
        pids, _ = process_pids()
        process_error = None
    except RuntimeError as exc:
        pids = []
        process_error = str(exc)
        issues.append(process_error)
    if process_error is None and target_pid is not None and target_pid not in pids:
        issues.append(f"requested PID {target_pid} is not a running riseofnations.exe")
    if target_pid is None and len(pids) == 1:
        target_pid = pids[0]
    if len(pids) > 1:
        issues.append(f"multiple riseofnations.exe processes are running: {pids}")
    process = {
        "status": ("error" if process_error else
                   ("absent" if not pids else
                    ("single" if len(pids) == 1 else "ambiguous"))),
        "pids": pids,
        "selected_pid": target_pid,
        "error": process_error,
    }
    inventory = None
    digest = None
    if target_pid is not None and target_pid in pids:
        try:
            digest = target_digest_diagnostic(target_pid)
        except Exception as exc:
            digest = {
                "supported": False,
                "error": f"target digest query failed: {exc}",
            }
        if not digest["supported"]:
            issues.append("selected retail executable identity is unsupported or ambiguous")
        try:
            if not injector["ready"]:
                raise RuntimeError("current hash-bound guest injector is not ready")
            inventory = controller_inventory(target_pid)
        except Exception as exc:
            detail = f"controller inventory failed: {exc}"
            issues.append(detail)
            inventory = {
                "enumeration": "failed",
                "complete": False,
                "generations": [],
                "mapped_modules": [],
                "mapped_generation_count": 0,
                "hook": {"status": "unknown"},
                "hook_owner": None,
                "issues": [detail],
            }
        issues.extend(inventory["issues"])
        if inventory["mapped_generation_count"] > max_generations:
            issues.append(
                f"mapped controller generations exceed budget "
                f"({inventory['mapped_generation_count']} > {max_generations})"
            )
    else:
        try:
            rows, row_errors = deployed_generations()
        except Exception as exc:
            rows = []
            row_errors = [f"deployed-generation scan failed: {exc}"]
        issues.extend(row_errors)
        stale_ready_records = [{
            "generation": row["generation"],
            "state": row["ready"]["values"].get("state"),
            "pid": row["ready"]["values"].get("pid"),
            "relation": "historical-only; no retail process is running",
        } for row in rows if row["ready"]["present"]]
        inventory = {
            "enumeration": "on-disk only; retail process is absent",
            "complete": not row_errors,
            "generations": rows,
            "stale_ready_records": stale_ready_records,
            "mapped_modules": [],
            "mapped_generation_count": 0,
            "hook": {"status": "not-applicable"},
            "hook_owner": None,
            "issues": row_errors,
        }
    try:
        wer = wer_diagnostics()
    except Exception as exc:
        wer = {
            "ready": False,
            "issues": [f"WER diagnostics failed: {exc}"],
            "mutation": "none; diagnostic query failed before any mutation",
        }
    issues.extend(f"WER: {issue}" for issue in wer["issues"])
    return {
        "schema": "don.retail-control-preflight.v1",
        "mode": "guest/process read-only; strict host injector build only",
        "injector": injector,
        "process": process,
        "target": digest,
        "controllers": inventory,
        "generation_budget": {
            "maximum_mapped": max_generations,
            "current_mapped": inventory["mapped_generation_count"],
            "within_budget": inventory["mapped_generation_count"] <= max_generations,
        },
        "wer_local_dumps": wer,
        "ready": not issues,
        "issues": sorted(set(issues)),
    }


def prelaunch_command(max_generations: int, target_pid: int | None = None) -> None:
    report = prelaunch_report(max_generations, target_pid)
    print(json.dumps(report, indent=2, sort_keys=True))
    if not report["ready"]:
        raise SystemExit(2)


def enforce_generation_budget(target_pid: int, generation: str,
                              max_generations: int,
                              require_unhooked: bool = True) -> dict:
    inventory = controller_inventory(target_pid, generation)
    if not inventory["complete"]:
        raise SystemExit(
            "REFUSING deployment because controller module inventory is incomplete:\n" +
            "\n".join(inventory["issues"])
        )
    requested_name = generation_dll(generation).lower()
    already_mapped = any(
        module["name"].lower() == requested_name
        for module in inventory["mapped_modules"]
    )
    projected = inventory["mapped_generation_count"] + (0 if already_mapped else 1)
    if projected > max_generations:
        raise SystemExit(
            f"REFUSING deployment: generation {generation!r} would map controller "
            f"{projected} of a configured maximum {max_generations}; restart retail "
            "instead of accumulating parked DLL generations"
        )
    if inventory["issues"]:
        raise SystemExit(
            "REFUSING deployment because controller ownership is not clean:\n" +
            "\n".join(inventory["issues"])
        )
    if require_unhooked and inventory["hook"]["status"] != "original":
        raise SystemExit(
            "REFUSING deployment while another controller owns the retail call site; "
            "park that generation with upgrade or stop first"
        )
    return inventory


def ready_identity_errors(record: dict, target_pid: int, root: str) -> list[str]:
    errors = list(record["errors"])
    values = record["values"]
    if not record["present"]:
        return [*errors, "ready record is missing"]
    try:
        ready_pid = int(values.get("pid", ""))
    except ValueError:
        ready_pid = None
    if ready_pid != target_pid:
        errors.append("ready pid does not match target")
    if normalize_windows_path(values.get("root", "")) != normalize_windows_path(root):
        errors.append("ready root does not match controller root")
    try:
        base_text = values.get("base", "")
        base = int(base_text, 16) if re.fullmatch(r"0x[0-9A-Fa-f]{8}", base_text) else None
        call_text = values.get("turn_call_site", "")
        call = int(call_text, 16) if re.fullmatch(r"0x[0-9A-Fa-f]{8}", call_text) else None
        turn_text = values.get("turn_do_frame", "")
        turn = int(turn_text, 16) if re.fullmatch(r"0x[0-9A-Fa-f]{8}", turn_text) else None
    except (TypeError, ValueError):
        base = call = turn = None
    if base is None or call != base + TURN_CALL_RVA or turn != base + TURN_DO_FRAME_RVA:
        errors.append("ready executable addresses are missing or inconsistent")
    return errors


def read_ready(root: str) -> tuple[str, dict]:
    raw = guest_cmd(f'if exist "{root}\\ready.txt" type "{root}\\ready.txt"', check=False)
    return raw, parse_ready_record(raw if raw else None)


def wait_for_ready_state(root: str, target_pid: int, desired: str,
                         timeout: float) -> str:
    deadline = time.monotonic() + timeout
    last_detail = "ready record was not present"
    while time.monotonic() < deadline:
        raw, record = read_ready(root)
        errors = ready_identity_errors(record, target_pid, root)
        state = record["values"].get("state")
        if not errors and state == desired:
            return raw
        if not errors and isinstance(state, str) and state.startswith("refused"):
            raise SystemExit(raw)
        last_detail = "; ".join(errors) if errors else f"state={state!r}"
        time.sleep(0.05)
    raise SystemExit(
        f"controller did not publish identity-bound state={desired}: {last_detail}"
    )


def require_armed_controller(root: str) -> tuple[int, str]:
    injector = injector_diagnostic()
    if not injector["ready"]:
        raise SystemExit(
            "REFUSING retail request because the hash-bound guest injector is not ready: " +
            "; ".join(injector["issues"])
        )
    target_pid = pid()
    preflight(target_pid)
    root_name = root.replace("/", "\\").rstrip("\\").rsplit("\\", 1)[-1]
    generation = generation_from_root_name(root_name)
    if generation is None:
        raise SystemExit(f"REFUSING request through unrecognized controller root {root!r}")
    inventory = controller_inventory(target_pid)
    if (not inventory["complete"] or inventory["issues"] or
            inventory["hook_owner"] != generation):
        detail = "; ".join(inventory["issues"]) or (
            f"hook owner is {inventory['hook_owner']!r}, expected {generation!r}"
        )
        raise SystemExit("REFUSING request because controller ownership is not exact: " + detail)
    return target_pid, generation


def deploy(target_pid: int, port: int, generation: str,
           max_generations: int = DEFAULT_GENERATION_BUDGET,
           injector_port: int = 18081, prepare: bool = True) -> None:
    root = generation_root(generation)
    dll_name = generation_dll(generation)
    if prepare:
        prepare_injector(injector_port)
    preflight(target_pid)
    enforce_generation_budget(target_pid, generation, max_generations)
    mapped = loaded_module(target_pid, dll_name)
    if mapped:
        raise SystemExit(
            f"REFUSING to replace mapped generation {generation!r}: {mapped}\n"
            "choose a new --generation; mapped controller DLLs remain parked by design"
        )
    build()
    dll_hash = sha256_file(HERE / "retail_control.dll")
    server = serve_once(port)
    try:
        guest_cmd(f'if not exist "{root}" mkdir "{root}"')
        guest_cmd(
            f'curl.exe -f -sS -o "{root}\\{dll_name}.download" '
            f'http://10.211.55.2:{port}/retail_control.dll'
        )
        downloaded_hash = guest_sha256(f"{root}\\{dll_name}.download")
        if downloaded_hash != dll_hash:
            raise SystemExit(
                "REFUSING controller deployment: guest download hash does not match host DLL"
            )
        guest_cmd(f'move /y "{root}\\{dll_name}.download" "{root}\\{dll_name}" >nul')
        installed_hash = guest_sha256(f"{root}\\{dll_name}")
        if installed_hash != dll_hash:
            raise SystemExit(
                "REFUSING controller deployment: installed guest DLL hash does not match host"
            )
        guest_cmd(
            f'del /q "{root}\\STOP" "{root}\\ready.txt" "{root}\\request.txt" '
            f'"{root}\\request.tmp" "{root}\\events.ndjson" 2>nul & exit /b 0'
        )
        dll_path = f"{root}\\{dll_name}"
        inject_returncode, out = guest_cmd_status(
            f'"{INJECTOR}" inject {target_pid} "{dll_path}" {dll_hash}'
        )
        print(out)
        inject_result = parse_inject_output(
            out, inject_returncode, target_pid, dll_name, dll_path, dll_hash
        )
        if inject_result["status"] == "indeterminate":
            raise SystemExit(
                "INDETERMINATE injection: this retail process is tainted and must be "
                "terminated before any retry\n" + inject_result["detail"]
            )
        if inject_result["status"] != "loaded":
            raise SystemExit("REFUSING after failed injector postcondition; do not retry "
                             "this retail process: " +
                             inject_result["detail"])
        ready = wait_for_ready_state(root, target_pid, "armed", 10.0)
        hook = hook_call_state(target_pid)
        if hook["status"] != "patched":
            raise SystemExit(
                f"controller reported armed but external hook bytes are {hook['status']}"
            )
        mapped_after = module_probe(target_pid, dll_name)
        if (mapped_after["status"] != "mapped" or
                mapped_after["base"] != inject_result["module_base"] or
                normalize_windows_path(mapped_after.get("module_path", "")) !=
                normalize_windows_path(dll_path)):
            raise SystemExit("controller reported armed without an identity-bound mapped DLL")
        print(ready)
    finally:
        guest_cmd(f'del /q "{root}\\{dll_name}.download" 2>nul & exit /b 0', check=False)
        server.shutdown()
        server.server_close()


def next_seq() -> int:
    return ((int(time.time() * 1000) & 0x7FFFFFFF) ^ random.getrandbits(20)) or 1


def validate_words(words: list[str]) -> None:
    if not words:
        raise SystemExit("a retail command is required")
    allowed = {"observe", "observe-network", "pause", "speed", "speed-up", "speed-down",
               "move", "halt", "attack", "attack-visible", "trace-move", "observe-guys",
               "observe-player", "validate-queue", "validate-build", "gather",
               "queue", "build", "run-frames", "find-build", "find-gather-build",
               "find-scout-step", "validate-attack"}
    if words[0] not in allowed:
        raise SystemExit(f"unsupported verb {words[0]!r}")
    for word in words:
        if not word or any(c not in "abcdefghijklmnopqrstuvwxyz-0123456789xABCDEF" for c in word):
            raise SystemExit(f"unsafe token {word!r}")


def validate_multiplayer_events(verb: str, events: list[dict]) -> None:
    if verb not in {"observe-network", "checksum"}:
        return
    gates = {
        "eligible", "no_game", "playback", "network_clear", "immediate_process",
        "no_console", "bad_play", "player_invalid", "player_terminal",
        "not_connected", "package_invalid", "package_full", "torn",
    }
    terminal = [event for event in events if event.get("phase") in {
        "observed", "queued", "rejected"
    }]
    if len(terminal) != 1:
        raise SystemExit(f"REFUSING malformed {verb} response: expected one terminal event")
    event = terminal[0]
    gate = event.get("checksum_gate")
    if (gate not in gates or
            event.get("mutates_outgoing_package") != (0 if verb == "observe-network" else 1) or
            not isinstance(event.get("network_last_num_received"), list) or
            len(event["network_last_num_received"]) != 8 or
            not all(isinstance(value, int) for value in event["network_last_num_received"]) or
            not isinstance(event.get("network_peer_checksums"), list) or
            len(event["network_peer_checksums"]) != 8 or
            not all(isinstance(value, int) for value in event["network_peer_checksums"])):
        raise SystemExit(f"REFUSING malformed {verb} network-gate evidence")
    if verb == "observe-network":
        if event.get("phase") != "observed" or event.get("checksum_capture_valid") != 0:
            raise SystemExit("REFUSING observe-network response that is not passive observation")
        return
    if event.get("phase") == "queued":
        words = event.get("checksum_words")
        if (gate != "eligible" or event.get("checksum_capture_valid") != 1 or
                event.get("checksum_total_consistent") != 1 or
                event.get("checksum_adler_shaped") != 1 or
                not isinstance(words, list) or len(words) != 16 or
                not all(isinstance(value, int) for value in words)):
            raise SystemExit("REFUSING checksum packet without complete retail capture evidence")
    elif gate == "eligible":
        raise SystemExit("REFUSING eligible checksum response that did not capture a packet")


def send(words: list[str], timeout: float, root: str) -> list[dict]:
    validate_words(words)
    require_armed_controller(root)
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
        if "queued" in seen and words[0] not in {
                "pause", "speed", "move", "halt", "attack", "attack-visible"}:
            break
        time.sleep(0.05)
    if not seen:
        raise SystemExit(
            "no main-thread response; the process is attached but TurnControl::do_frame is not running"
        )
    events = list(seen.values())
    validate_multiplayer_events(words[0], events)
    for event in events:
        print(json.dumps(event, sort_keys=True))
    return events


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
    visible_enemies = []
    for item in event.get("visible_enemy_objects", []):
        runtime_class_vtable = int(item["class_vtable"], 16)
        class_vtable = runtime_class_vtable - base + 0x00400000
        category = categories.get(item["category"], "unknown")
        type_valid = bool(item["type_valid"])
        visible = {
            "id": {
                "slot": item["owner"], "band": category,
                "o": item["id"], "uid": item["uid"],
            },
            "owner": item["owner"],
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
            "visibility": "shipped ObjectData::is_seen(local_who,0)",
        }
        if category == "unit":
            visible["heading_u32"] = item["angle"]
        elif category == "build":
            visible["complete"] = bool(item["flags"] & 4)
        visible_enemies.append(visible)
    protocol_v4 = "visible_enemy_objects" in event
    protocol = "don.retail-player.v4" if protocol_v4 else "don.retail-player.v3"
    return {
        "schema": ("don.retail-player-observation.v4" if protocol_v4 else
                   "don.retail-player-observation.v3"),
        "protocol": protocol,
        "retail_executable_sha256": EXPECTED_SHA256,
        "controller_generation": generation,
        "public_scope": {
            "owner": event["local_player"],
            "includes": ["own active object bands", "own stockpile", "own commerce cap",
                         "own population", "own building gather capacity", "public game clock"] +
                        (["enemy objects accepted by shipped is_seen(local_who,0)"]
                         if protocol_v4 else []),
            "excludes": (["enemy and neutral object tables"] if not protocol_v4 else
                         ["fog-hidden enemy objects and all neutral object tables"]) +
                        ["enemy resources", "fog-hidden map state",
                         "unproven target-object dereferences"],
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
        **({"visible_enemies": sorted(
                visible_enemies,
                key=lambda item: (item["owner"], item["category"], item["object_id"]),
            )} if protocol_v4 else {}),
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
    if event.get("visible_enemy_truncated"):
        raise RuntimeError("retail visible-enemy observation exceeded MAX_VISIBLE_ENEMIES")
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
    if any(not obj["type_valid"] or obj["runtime_class"] == "unknown"
           for obj in observation.get("visible_enemies", [])):
        raise RuntimeError("retail player observation contains an unresolved visible enemy")
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
    if event.get("note"):
        raise RuntimeError(f"retail legality query failed closed (note={event['note']})")
    return event


def scout_step_validation(root: str, observation: dict, unit_id: int,
                          goal_x: int, goal_y: int) -> dict:
    owner = observation["player"]["owner"]
    event = exact_validation(
        root, ["find-scout-step", str(owner), str(unit_id), str(goal_x), str(goal_y)]
    )
    return {
        "accepted": bool(event["validation_result"]),
        "retail_result": event["validation_result"],
        "currently_visible": bool(event["placement_seen"]),
        "retail_passable": bool(event["placement_legal"]),
        "target": ({"x": event["placement_x"], "y": event["placement_y"]}
                   if event["validation_result"] else None),
        "oracle": ("current local-slot WorldData::is_really_seen precedes shipped "
                   "WorldData::is_passable for each diagonal-then-cardinal candidate; "
                   "target is one 192-Coord frontier step"),
    }


def visible_attack_validation(root: str, observation: dict, actor_id: int,
                              target: dict) -> dict:
    owner = observation["player"]["owner"]
    event = exact_validation(root, [
        "validate-attack", str(owner), str(actor_id), str(target["owner"]),
        str(target["object_id"]), str(target["id"]["uid"]),
    ])
    return {
        "accepted": bool(event["validation_result"]),
        "retail_result": event["validation_result"],
        "target": target["id"],
        "oracle": ("LeaderData::is_enemy plus category-specific shipped "
                   "ObjectData::is_seen(local_who,0) and exact {who,o,uid}"),
    }


def live_unit_policy_rows() -> dict[int, dict]:
    lines = (HERE.parents[1] / "schema/live/live-tables-unit.tsv").read_text().splitlines()
    header = lines[0].split("\t")
    columns = {name: header.index(name) for name in
               ["type_id", "cat", "attack", "cost0", "cost1", "cost2",
                "cost3", "cost4", "cost5"]}
    rows = {}
    for line in lines[1:]:
        fields = line.split("\t")
        type_index = int(fields[columns["type_id"]])
        cat = int(fields[columns["cat"]])
        attack = int(fields[columns["attack"]])
        rows[type_index] = {
            "cat": cat,
            "attack": attack,
            "is_military": attack > 0 and cat not in {5, 8},
            "value": sum(int(fields[columns[f"cost{i}"]]) for i in range(6)),
        }
    return rows


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
    """Reproduce Marshal's useful-slot and seat-gap predicates from retail v3/v4."""
    if observation.get("protocol") not in {"don.retail-player.v3", "don.retail-player.v4"}:
        raise RuntimeError("gather-state planning requires retail-player.v3/v4")
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
    """Retail-v3/v4 form of Arena builder_for_except with no active scout exclusion."""
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


MARSHAL_RING_DIRECTIONS = [
    (4096, 0), (3547, 2048), (2048, 3547), (0, 4096),
    (-2048, 3547), (-3547, 2048), (-4096, 0), (-3547, -2048),
    (-2048, -3547), (0, -4096), (2048, -3547), (3547, -2048),
]


def marshal_ring_goals(observation: dict, unit: dict, leg: int) -> list[dict]:
    """Arena Marshal's 12-leg integer ring geometry, before retail fog/path gates."""
    width = observation["world"]["tile_xs"]
    height = observation["world"]["tile_ys"]
    cx, cy = width // 2, height // 2
    radius = min(width, height) * 3 // 8
    sx = unit["position"]["x"] // 192
    sy = unit["position"]["y"] // 192
    offsets = [(radius * dx // 4096, radius * dy // 4096)
               for dx, dy in MARSHAL_RING_DIRECTIONS]
    start = min(range(12), key=lambda k: (
        max(abs(offsets[k][0] - (sx - cx)), abs(offsets[k][1] - (sy - cy))), k
    ))
    goals = []
    for extra in range(12):
        k = (start + leg + extra) % 12
        tx = min(max(cx + offsets[k][0], 1), width - 2)
        ty = min(max(cy + offsets[k][1], 1), height - 2)
        goals.append({
            "tile_x": tx, "tile_y": ty, "ring_index": k,
            "coord_x": tx * 192 + 96, "coord_y": ty * 192 + 96,
        })
    return goals


def marshal_scout_tactical_action(observation: dict, root: str, state: dict,
                                   already_supported: list[dict]) -> tuple[dict, dict | None]:
    owner = observation["player"]["owner"]
    visible = observation.get("visible_enemies", [])
    enemy_building = next((obj for obj in visible if obj["category"] == "build"), None)
    if enemy_building and state.get("enemy_base") is None:
        state["enemy_base"] = {
            "tile_x": enemy_building["position"]["x"] // 192,
            "tile_y": enemy_building["position"]["y"] // 192,
            "target": enemy_building["id"],
        }
    if state.get("enemy_base") is not None or observation["frame"] > 420 * 15:
        return ({
            "stage": "scout", "source": "Marshal::do_scout", "result": "suppressed",
            "reason": ("visible enemy building fixed enemy_base; Halt remains an explicit "
                       "unsupported tactical verb" if state.get("enemy_base") else
                       "past Marshal scout_until horizon"),
        }, None)

    objects = observation["objects"]
    citizens = [obj for obj in objects if obj["category"] == "unit" and
                obj["type_index"] in {50, 51} and obj["hits"] > 0]
    cities = [obj for obj in objects if obj["category"] == "build" and
              obj["type_index"] in {414, 415, 416} and obj.get("complete")]
    if not citizens or not cities:
        return ({"stage": "scout", "source": "Marshal::do_scout",
                 "result": "suppressed", "reason": "no own Citizen or complete capital"}, None)
    capital = min(cities, key=lambda obj: obj["object_id"])
    if "reserved_builder_ids" not in state:
        idle = [obj for obj in citizens if obj["order"]["length"] == 0]
        state["reserved_builder_ids"] = ([min(idle, key=lambda obj: obj["object_id"])["object_id"]]
                                          if idle else [])
    blocked_actors = {
        (action.get("worker_ids") or [action.get("worker_id")])[0]
        for action in already_supported
        if action.get("worker_ids") or action.get("worker_id") is not None
    }
    blocked_actors.update(state["reserved_builder_ids"])
    by_id = {obj["object_id"]: obj for obj in citizens}
    scout = by_id.get(state.get("scout_id"))
    if scout is None:
        candidates = [obj for obj in citizens if obj["object_id"] not in blocked_actors]
        if not candidates:
            return ({"stage": "scout", "source": "Marshal::do_scout",
                     "result": "suppressed", "reason": "all own Citizens are protected actors"},
                    None)
        cx = capital["position"]["x"] // 192
        cy = capital["position"]["y"] // 192
        scout = max(candidates, key=lambda obj: (
            max(abs(obj["position"]["x"] // 192 - cx),
                abs(obj["position"]["y"] // 192 - cy)),
            obj["object_id"],
        ))
        state["scout_id"] = scout["object_id"]

    goal = state.get("scout_goal")
    sx, sy = scout["position"]["x"] // 192, scout["position"]["y"] // 192
    arrived = (goal is None or
               max(abs(goal["tile_x"] - sx), abs(goal["tile_y"] - sy)) <= 3)
    if not arrived and scout["order"]["kind"] == "MoveOrder":
        return ({
            "stage": "scout", "source": "Marshal::do_scout", "result": "in-flight",
            "scout_id": scout["object_id"], "goal": goal,
            "reason": "existing exact MoveOrder continues toward the persistent ring goal",
        }, None)
    if not arrived and scout["order"]["length"] != 0:
        return ({
            "stage": "scout", "source": "Marshal::do_scout", "result": "suppressed",
            "scout_id": scout["object_id"], "goal": goal,
            "reason": "persistent scout is busy and has not reached its goal",
        }, None)

    state["scout_leg"] = int(state.get("scout_leg", 0)) + 1
    attempts = []
    for candidate in marshal_ring_goals(observation, scout, state["scout_leg"]):
        query = scout_step_validation(
            root, observation, scout["object_id"],
            candidate["coord_x"], candidate["coord_y"],
        )
        attempts.append({"goal": candidate, "retail_frontier": query})
        if not query["accepted"]:
            continue
        state["scout_goal"] = candidate
        target = query["target"]
        action = {
            "verb": "move", "owner": owner, "object_ids": [scout["object_id"]],
            "actor": scout["id"],
            "target": target, "queue": 2, "order": 1,
            "form": -1, "width": -1, "disembark": 0,
            "scout_evidence": {
                "policy_goal": candidate,
                "frontier": query,
                "selection": ("Arena 12-leg ring geometry; one retail-fog-visible, shipped-"
                              "passable 192-Coord frontier step toward the persistent goal"),
            },
        }
        return ({
            "stage": "scout", "source": "Marshal::do_scout", "result": "emit",
            "scout_id": scout["object_id"], "goal": candidate,
            "attempts": attempts,
        }, action)
    return ({
        "stage": "scout", "source": "Marshal::do_scout", "result": "suppressed",
        "scout_id": scout["object_id"], "attempts": attempts,
        "reason": "no adjacent step was both currently visible and shipped-passable",
    }, None)


def marshal_army_tactical_action(observation: dict, root: str,
                                 state: dict) -> tuple[dict, dict | None]:
    rows = live_unit_policy_rows()
    army = [obj for obj in observation["objects"] if obj["category"] == "unit" and
            rows.get(obj["type_index"], {}).get("is_military") and obj["hits"] > 0]
    if not army:
        return ({"stage": "army_control", "source": "Marshal::army_control",
                 "result": "suppressed", "reason": "no own live military unit"}, None)
    value = sum(rows[obj["type_index"]]["value"] for obj in army)
    mode = state.setdefault("mode", "Massing")
    if mode == "Pushing" and value * 100 < int(state.get("push_value", 0)) * 40:
        state["mode"] = "Massing"
        return ({
            "stage": "army_control", "source": "Marshal::army_control",
            "result": "suppressed", "mode": "Massing", "army_value": value,
            "reason": "the observed own army fell below Marshal's 60-percent-loss retreat gate",
        }, None)
    if mode == "Massing" and state.get("enemy_base") is not None and value >= 420:
        state["mode"] = "Pushing"
        state["push_value"] = value
        state["pushes"] = int(state.get("pushes", 0)) + 1
        return ({
            "stage": "army_control", "source": "Marshal::army_control",
            "result": "state-transition", "mode": "Pushing", "army_value": value,
            "reason": "observed own army reached Level::MARSHAL.push_threshold=420",
        }, None)
    visible = observation.get("visible_enemies", [])
    if state.get("mode") == "Pushing" and visible:
        actor = min(army, key=lambda obj: obj["object_id"])
        ax, ay = actor["position"]["x"], actor["position"]["y"]
        target = min(visible, key=lambda obj: (
            0 if (obj["category"] == "unit" and
                  rows.get(obj["type_index"], {}).get("cat") == 5) else
            1 if (obj["category"] == "unit" and
                  rows.get(obj["type_index"], {}).get("is_military")) else
            2 if obj["category"] == "build" else 3,
            max(abs(obj["position"]["x"] - ax), abs(obj["position"]["y"] - ay)),
            obj["owner"], obj["object_id"],
        ))
        validation = visible_attack_validation(root, observation, actor["object_id"], target)
        if validation["accepted"]:
            return ({
                "stage": "army_control", "source": "Marshal::army_control",
                "result": "emit", "mode": "Pushing", "actor_id": actor["object_id"],
                "target": target["id"], "retail_visibility_replay": validation,
            }, {
                "verb": "attack", "owner": observation["player"]["owner"],
                "object_ids": [actor["object_id"]],
                "actor": actor["id"],
                "target": target["id"], "target_owner": target["owner"],
                "target_id": target["object_id"], "target_uid": target["id"]["uid"],
                "flags": 0, "queue": 2,
                "visibility_evidence": validation,
            })
    return ({
        "stage": "army_control", "source": "Marshal::army_control",
        "result": "suppressed", "mode": state.get("mode", "Massing"),
        "reason": "no observation-safe attack emitted in the current mode",
    }, None)


def arena_marshal_extracted_plan(observation: dict, root: str,
                                 queue_query=queue_validation,
                                 gather_site_query=find_visible_gather_site,
                                 ordinary_site_query=find_visible_ordinary_site,
                                 tactical_state: dict | None = None) -> dict:
    """Faithful supported subsequence of Marshal::act, in its source command order."""
    protocol = observation.get("protocol")
    if protocol not in {"don.retail-player.v2", "don.retail-player.v3",
                        "don.retail-player.v4"}:
        raise RuntimeError("Arena Marshal adapter requires fog-safe retail-player.v2/v3/v4")
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
    visible = observation.get("visible_enemies", [])
    first_enemy_building = next((obj for obj in visible if obj["category"] == "build"), None)
    if tactical_state is not None:
        tactical_state.setdefault("mode", "Massing")
        if first_enemy_building and tactical_state.get("enemy_base") is None:
            tactical_state["enemy_base"] = {
                "tile_x": first_enemy_building["position"]["x"] // 192,
                "tile_y": first_enemy_building["position"]["y"] // 192,
                "target": first_enemy_building["id"],
            }
    trace.append({
        "stage": "sense",
        "source": "Marshal::sense",
        "result": tactical_state.get("mode", "Massing") if tactical_state else "Massing",
        "visible_enemy_count": len(visible),
        "enemy_base": tactical_state.get("enemy_base") if tactical_state else None,
        "reason": ("shipped is_seen(local_who,0) supplied current fog-approved enemies"
                   if protocol == "don.retail-player.v4" else
                   f"{protocol} contains no fog-approved enemy sightings or last-damaged field"),
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

    if tactical_state is None or protocol != "don.retail-player.v4":
        scout_trace = {
            "stage": "scout", "source": "Marshal::do_scout", "result": "unsupported",
            "reason": "retail-player.v4 current-visibility/passability oracle is required",
        }
        scout_action = None
        army_trace = {
            "stage": "army_control", "source": "Marshal::army_control",
            "result": "suppressed", "reason": "tactical controller is not enabled",
        }
        army_action = None
    else:
        scout_trace, scout_action = marshal_scout_tactical_action(
            observation, root, tactical_state, supported
        )
        if scout_action:
            supported.append(scout_action)
        army_trace, army_action = marshal_army_tactical_action(
            observation, root, tactical_state
        )
        if army_action:
            supported.append(army_action)
    trace.extend([
        scout_trace,
        {"stage": "military", "source": "Marshal::military", "result": "suppressed",
         "reason": ("before Marshal military_from horizon" if observation["frame"] < 2250
                    else "no supported public production candidate selected")},
        army_trace,
        {"stage": "employ", "source": "Marshal::employ_except", "result": "unsupported",
         "reason": ("v2 omits exact gather_max needed to allocate a free seat"
                    if protocol.endswith(".v2") else
                    f"{protocol} exposes capacity but not the complete exact free-seat chain")},
    ])

    tactical = [candidate for candidate in supported
                if candidate["verb"] in {"move", "attack"}]
    action = (tactical[0] if tactical and tactical_state is not None else
              supported[0] if supported else None)
    if action and action["verb"] == "queue":
        heads = [23, 0, 0, 0, action["type_index"], 0, 0, 0, 0, action["count"]]
    elif action and action["verb"] == "build":
        heads = [24, action["placement_evidence"]["snapped_x"] // 192,
                 action["placement_evidence"]["snapped_y"] // 192, 0,
                 action["type_index"], 0, 0, 0, 0, 0]
    elif action and action["verb"] == "move":
        heads = [6, action["target"]["x"] // 192,
                 action["target"]["y"] // 192, 0, 0, 0, 0, 0, 0, 0]
    elif action and action["verb"] == "attack":
        visible_slots = {
            (obj["owner"], obj["object_id"], obj["id"]["uid"]): i + 1
            for i, obj in enumerate(observation.get("visible_enemies", []))
        }
        target_slot = visible_slots.get(
            (action["target_owner"], action["target_id"], action["target_uid"]), 0
        )
        heads = [3, 0, 0, target_slot, 0, 0, 0, 0, 0, 0]
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
        "selection_rule": (
            "one-action supervisor preserves an emitted tactical command before deferred "
            "economy commands; otherwise first supported command in Marshal source order"
            if tactical_state is not None else
            "first supported emitted command in Marshal source order; max one live action"
        ),
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


def validate_tactical_action(action: dict, observation: dict, root: str) -> dict:
    owner = observation["player"]["owner"]
    owned = {obj["object_id"]: obj for obj in observation["objects"]}
    ids = action.get("object_ids", [])
    if action.get("owner") != owner or len(ids) != 1:
        raise RuntimeError("tactical action requires exactly one actor from the observed slot")
    actor = owned.get(ids[0])
    if not actor or actor["category"] != "unit" or actor["hits"] <= 0:
        raise RuntimeError("tactical actor is not an observed own live unit")
    if action.get("actor") != actor["id"]:
        raise RuntimeError("tactical actor identity changed since planning")
    if action["verb"] == "move":
        if (action.get("queue"), action.get("order"), action.get("form"),
                action.get("width"), action.get("disembark")) != (2, 1, -1, -1, 0):
            raise RuntimeError("scout move differs from the bounded Marshal command shape")
        evidence = action.get("scout_evidence") or {}
        goal = evidence.get("policy_goal") or {}
        if not all(isinstance(goal.get(key), int) for key in ("coord_x", "coord_y")):
            raise RuntimeError("scout move lacks its public ring goal")
        replay = scout_step_validation(
            root, observation, actor["object_id"], goal["coord_x"], goal["coord_y"]
        )
        if not replay["accepted"] or replay["target"] != action.get("target"):
            return {"validation_result": 0, "frontier_replay": replay,
                    "validation": "current-fog/passability scout frontier changed"}
        return {"validation_result": 1, "frontier_replay": replay,
                "validation": "exact current-fog/passability scout frontier replay"}
    if action["verb"] == "attack":
        if action.get("flags") != 0 or action.get("queue") not in {0, 1, 2}:
            raise RuntimeError("visible attack differs from the bounded retail command shape")
        target_key = (action.get("target_owner"), action.get("target_id"),
                      action.get("target_uid"))
        target = next((obj for obj in observation.get("visible_enemies", [])
                       if (obj["owner"], obj["object_id"], obj["id"]["uid"]) == target_key),
                      None)
        if target is None or action.get("target") != target["id"]:
            raise RuntimeError("attack target is not in the same paused visible-enemy set")
        replay = visible_attack_validation(root, observation, actor["object_id"], target)
        return {"validation_result": int(replay["accepted"]),
                "visibility_replay": replay,
                "validation": "exact enemy/identity/shipped-visibility replay"}
    raise RuntimeError(f"unsupported tactical verb {action['verb']!r}")


def tactical_action_words(action: dict) -> list[str]:
    actor = str(action["object_ids"][0])
    if action["verb"] == "move":
        return ["move", str(action["owner"]), str(action["target"]["x"]),
                str(action["target"]["y"]), str(action["queue"]),
                str(action["order"]), str(action["form"]), str(action["width"]),
                str(action["disembark"]), actor]
    if action["verb"] == "attack":
        return ["attack-visible", str(action["owner"]), str(action["target_owner"]),
                str(action["target_id"]), str(action["target_uid"]),
                str(action["flags"]), str(action["queue"]), actor]
    raise RuntimeError(f"unsupported tactical verb {action['verb']!r}")


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
        "visible_enemies": observation.get("visible_enemies", []),
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


def prove_tactical_action(root: str, generation: str, action: dict, output: Path,
                          expected_before: dict | None = None) -> dict:
    """Apply one paused move/visible-attack transaction with exact identity replay."""
    before = player_observation(root, generation)
    if (expected_before is not None and
            paused_observation_token(before) != paused_observation_token(expected_before)):
        raise RuntimeError("public state/identity changed between tactical plan and apply")
    validation = validate_tactical_action(action, before, root)
    if not validation.get("validation_result"):
        raise RuntimeError("retail visibility/legality replay rejected tactical action")
    command_events = send(tactical_action_words(action), 8.0, root)
    queued = next((event for event in command_events if event.get("phase") == "queued"), None)
    applied = next((event for event in command_events if event.get("phase") == "applied"), None)
    if not queued or not queued.get("command_hex") or not applied:
        raise RuntimeError("retail did not serialize and apply the tactical command")
    if queued.get("paused") != 1 or applied.get("paused") != 1:
        raise RuntimeError("tactical command escaped the paused main-thread boundary")
    if action["verb"] == "move":
        target = action["target"]
        if (not applied.get("move_valid") or applied.get("move_x") != target["x"] or
                applied.get("move_y") != target["y"]):
            raise RuntimeError("retail applied a different MoveOrder destination")
        expected_order = "MoveOrder"
    else:
        if (not applied.get("attack_valid") or
                applied.get("attack_target_who") != action["target_owner"] or
                applied.get("attack_target_id") != action["target_id"] or
                applied.get("attack_target_uid") != action["target_uid"]):
            raise RuntimeError("retail applied a different AttackOrder target identity")
        expected_order = "AttackOrder"
    after = player_observation(root, generation)
    if before["paused"] != 1 or after["paused"] != 1 or before["frame"] != after["frame"]:
        raise RuntimeError("tactical transaction escaped its paused zero-frame boundary")
    if observation_identity(after) != observation_identity(before):
        raise RuntimeError("retail executable/player/world identity changed during tactical apply")
    actor = next((obj for obj in after["objects"]
                  if obj["object_id"] == action["object_ids"][0]), None)
    if actor is None or actor["id"] != action["actor"] or actor["order"]["kind"] != expected_order:
        raise RuntimeError(f"retail did not retain the exact {expected_order} on the actor")
    artifact = {
        "schema": "don.retail-tactical-action-proof.v1",
        "protocol": before["protocol"],
        "controller_generation": generation,
        "mode": "apply",
        "action": action,
        "retail_validation": validation,
        "retail_command_hex": queued["command_hex"],
        "retail_applied_event": applied,
        "frame_boundary": {"before": before["frame"], "after": after["frame"]},
        "pause_before_after": [before["paused"], after["paused"]],
        "bounded_settlement": [],
        "before": before,
        "after": after,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(artifact, indent=2) + "\n")
    return artifact


def prove_supported_action(root: str, generation: str, action: dict, output: Path,
                           expected_before: dict | None = None,
                           settlement_frames: int = 30,
                           settlement_limit_frames: int = 180) -> dict:
    if action.get("verb") in {"move", "attack"}:
        return prove_tactical_action(root, generation, action, output, expected_before)
    return prove_economy_action(
        root, generation, action, output, expected_before,
        settlement_frames, settlement_limit_frames,
    )


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
        "protocol": "don.retail-player.v4",
        "controller_generation": generation,
        "mode": "apply" if apply else "dry-run",
        "requested_decisions": decisions,
        "frames_per_decision": frames_per_decision,
        "status": "running",
        "safety": {
            "max_actions_per_decision": 1,
            "proven_action_verbs": ["queue", "build", "move", "attack-visible"],
            "unsupported_action": "explicit no-op",
            "fog": ("retail-player.v4 own state plus shipped-current-visible enemies; "
                    "placement, scout, and attack queries replay visibility before hidden state"),
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
    tactical_state: dict = {"mode": "Massing", "scout_leg": 0}
    try:
        for index in range(decisions):
            step: dict = {"index": index, "status": "observing"}
            artifact["decisions"].append(step)
            checkpoint()
            before = player_observation(root, generation)
            if before["protocol"] != "don.retail-player.v4":
                raise RuntimeError("tactical Marshal loop requires retail-player.v4")
            identity = observation_identity(before)
            if stable_identity is None:
                stable_identity = identity
                artifact["stable_identity"] = identity
            elif identity != stable_identity:
                raise RuntimeError("retail executable/player/world identity changed between decisions")
            step["before"] = before
            step["status"] = "planning"
            checkpoint()

            step["tactical_state_before"] = copy.deepcopy(tactical_state)
            plan = arena_marshal_extracted_plan(
                before, root, tactical_state=tactical_state
            )
            step["tactical_state_after_plan"] = copy.deepcopy(tactical_state)
            action = plan["selected_action"]
            step["plan"] = plan
            step["selected_action"] = action
            step["action_mode"] = (
                "apply" if action and action.get("verb") in
                {"queue", "build", "move", "attack"} and apply else
                "dry-run" if action and not apply else
                "no-op-unsupported" if action else "no-op-no-supported-action"
            )
            step["status"] = "planned"
            checkpoint()

            proof = None
            advance = None
            proven = action and action.get("verb") in {"queue", "build", "move", "attack"}
            if apply and proven:
                proof_path = output.with_name(
                    f"{output.stem}-step-{index:02d}-action-proof.json"
                )
                proof = prove_supported_action(
                    root, generation, action, proof_path,
                    expected_before=before,
                    settlement_frames=frames_per_decision,
                    settlement_limit_frames=frames_per_decision,
                )
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
        artifact["tactical_state"] = tactical_state
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
    target_pid = pid()
    guest_cmd(f'(echo stop)>"{root}\\STOP"')
    ready = wait_for_ready_state(root, target_pid, "parked", 8.0)
    hook = hook_call_state(target_pid)
    if hook["status"] != "original":
        raise SystemExit(
            f"STOP remains asserted: parked record arrived but external hook bytes are "
            f"{hook['status']}"
        )
    print(ready)


def rearm(root: str) -> None:
    target_pid = pid()
    preflight(target_pid)
    root_name = root.replace("/", "\\").rstrip("\\").rsplit("\\", 1)[-1]
    generation = generation_from_root_name(root_name)
    if generation is None:
        raise SystemExit(f"REFUSING rearm of unrecognized controller root {root!r}")
    probe = module_probe(target_pid, generation_dll(generation))
    if probe["status"] != "mapped":
        raise SystemExit("REFUSING rearm without an identity-bound mapped controller DLL")
    guest_cmd(f'del /q "{root}\\STOP"')
    ready = wait_for_ready_state(root, target_pid, "armed", 5.0)
    hook = hook_call_state(target_pid)
    if hook["status"] != "patched":
        raise SystemExit(
            f"controller reported armed but external hook bytes are {hook['status']}"
        )
    print(ready)


def netsys_environment(mode: str, bind: str = "127.0.0.1:31337") -> dict[str, str]:
    if mode not in {"load-only", "host", "host-bridge"}:
        raise ValueError(f"unsupported NetSys mode {mode!r}")
    host, separator, port_text = bind.rpartition(":")
    try:
        address = ipaddress.ip_address(host) if separator else None
        port = int(port_text) if separator else 0
    except ValueError as exc:
        raise ValueError(f"invalid host bind {bind!r}") from exc
    if (address is None or address.version != 4 or
            not (address.is_loopback or address == ipaddress.ip_address("0.0.0.0")) or
            not 1 <= port <= 65535):
        raise ValueError("host bind must be loopback or 0.0.0.0 with a valid port")
    trace, _ = netsys_mode_paths(mode)
    environment = {
        "DON_NET_ROLE": "host",
        "DON_NET_BIND": "127.0.0.1:31337" if mode == "load-only" else bind,
        "DON_NET_ID": "1",
        "DON_NET_NAME": "Ai",
        "DON_NET_TRACE": trace,
    }
    if mode == "load-only":
        environment["DON_NET_LOAD_ONLY"] = "1"
    elif mode == "host-bridge":
        environment["DON_NET_SETUP_BRIDGE"] = "1"
    return environment


def netsys_mode_paths(mode: str) -> tuple[str, str]:
    paths = {
        "load-only": (NETSYS_LOAD_TRACE, NETSYS_LOAD_EXIT),
        "host": (NETSYS_HOST_TRACE, NETSYS_HOST_EXIT),
        "host-bridge": (NETSYS_BRIDGE_TRACE, NETSYS_BRIDGE_EXIT),
    }
    try:
        return paths[mode]
    except KeyError as exc:
        raise ValueError(f"unsupported NetSys mode {mode!r}") from exc


def netsys_launcher_text(mode: str, environment: dict[str, str]) -> str:
    if environment != netsys_environment(mode, environment.get("DON_NET_BIND", "")):
        raise ValueError("NetSys launcher environment is not the exact supported profile")
    _, exit_path = netsys_mode_paths(mode)
    lines = [
        "@echo off",
        "setlocal",
        'set "DON_NET_ADDR="',
        'set "DON_NET_LOAD_ONLY="',
        'set "DON_NET_SETUP_BRIDGE="',
    ]
    for key in ["DON_NET_ROLE", "DON_NET_BIND", "DON_NET_ID", "DON_NET_NAME",
                "DON_NET_TRACE", "DON_NET_LOAD_ONLY", "DON_NET_SETUP_BRIDGE"]:
        if key in environment:
            lines.append(f'set "{key}={environment[key]}"')
    lines += [
        f'cd /d "{RETAIL_ROOT}"',
        f'"{RETAIL_EXE}"',
        "set DON_NET_EXIT_CODE=%ERRORLEVEL%",
        f'>"{exit_path}" echo exit_code=%DON_NET_EXIT_CODE%',
        "exit /b %DON_NET_EXIT_CODE%",
    ]
    return "\r\n".join(lines) + "\r\n"


def parse_netsys_trace(raw: str, expected_pid: int | None = None) -> dict:
    encoded = raw.encode("utf-8")
    if len(encoded) > 1024 * 1024:
        raise ValueError("NetSys trace exceeds the 1 MiB evidence bound")
    lines = raw.replace("\r\n", "\n").splitlines()
    if not lines or len(lines) > 4096:
        raise ValueError("NetSys trace is empty or exceeds 4096 records")
    records = []
    expected_sequence = 1
    first_pid = None
    sensitive = re.compile(
        r"(?i)(?:ticket|token|secret|credential|lobby_id|platform_id|steam_id)="
    )
    for line in lines:
        match = re.fullmatch(r"seq=([0-9]+) pid=([0-9]+) ([ -~]+)", line)
        if not match:
            raise ValueError("NetSys trace has a malformed or non-ASCII record")
        sequence = int(match.group(1))
        trace_pid = int(match.group(2))
        detail = match.group(3)
        if sequence != expected_sequence:
            raise ValueError("NetSys trace sequence is not contiguous from one")
        if expected_pid is not None and trace_pid != expected_pid:
            raise ValueError("NetSys trace PID does not match the retail process")
        if first_pid is None:
            first_pid = trace_pid
        elif trace_pid != first_pid:
            raise ValueError("NetSys trace mixes multiple process identities")
        if sensitive.search(detail):
            raise ValueError("NetSys trace contains prohibited credential/identity material")
        records.append({"sequence": sequence, "pid": trace_pid, "detail": detail})
        expected_sequence += 1
    factory = [record for record in records if record["detail"].startswith("factory=ready ")]
    return {
        "records": records,
        "factory_ready": factory[-1]["detail"] if len(factory) == 1 else None,
        "load_only": len(factory) == 1 and " load_only=true " in factory[0]["detail"],
    }


def validate_netsys_load_only_frontier(trace: dict) -> None:
    if not trace.get("load_only") or trace.get("factory_ready") is None:
        raise ValueError("trace does not prove a load-only factory boundary")
    details = [record.get("detail") for record in trace.get("records", [])]
    required = [
        "call=factory.get_netsys_object_ptr",
        trace["factory_ready"],
        "call=vtable.ns_error_set_callback",
        "call=vtable.ns_set_profiler",
        "call=vtable.ns_init",
        "init=stored messenger=true crossplay_service=true object_size=0x3d4",
        "call=vtable.ns_close",
        "call=vtable.ns_cleanup_system",
    ]
    positions = []
    for detail in required:
        matches = [index for index, value in enumerate(details) if value == detail]
        if len(matches) != 1:
            raise ValueError(f"load-only trace lacks one exact {detail!r} boundary")
        positions.append(matches[0])
    if positions != sorted(positions):
        raise ValueError("load-only loader frontier is not chronological")


def validate_netsys_bridge_off_frontier(trace: dict) -> None:
    if trace.get("load_only") or trace.get("factory_ready") is None:
        raise ValueError("trace does not prove a transport-enabled factory boundary")
    details = [record.get("detail") for record in trace.get("records", [])]
    factory_call = [index for index, detail in enumerate(details)
                    if detail == "call=factory.get_netsys_object_ptr"]
    factory_ready = [index for index, detail in enumerate(details)
                     if detail == trace["factory_ready"]]
    host_call = [index for index, detail in enumerate(details)
                 if detail == "call=vtable.ns_host"]
    additions = [index for index, detail in enumerate(details)
                 if detail == "callback=NetMessenger.on_player_added player_non_null=true"]
    if len(factory_call) != 1 or len(factory_ready) != 1 or len(host_call) != 1:
        raise ValueError("bridge-off trace lacks exact factory/host boundaries")
    if len(additions) < 1:
        raise ValueError("bridge-off trace does not prove a player-added callback frontier")
    if not factory_call[0] < factory_ready[0] < host_call[0] < additions[0]:
        raise ValueError("bridge-off host/roster frontier is not chronological")


def parse_netsys_exit(raw: bytes) -> dict:
    try:
        text = raw.decode("ascii", errors="strict").strip()
    except UnicodeDecodeError as exc:
        raise ValueError("retail exit record is not ASCII") from exc
    match = re.fullmatch(r"exit_code=(-?[0-9]+)", text)
    if not match:
        raise ValueError("retail exit record is malformed")
    value = int(match.group(1))
    if not -(2 ** 31) <= value < 2 ** 32:
        raise ValueError("retail exit code is outside the Windows process range")
    return {"exit_code": value}


def _netsys_file_identity(value: object, expected_path: str) -> dict:
    if not isinstance(value, dict):
        raise ValueError(f"NetSys manifest identity for {expected_path} is not an object")
    if set(value) != {"path", "size", "sha256"}:
        raise ValueError(f"NetSys manifest identity for {expected_path} has invalid fields")
    path = value.get("path")
    size = value.get("size")
    digest = value.get("sha256")
    if (not isinstance(path, str) or normalize_windows_path(path) !=
            normalize_windows_path(expected_path) or
            not isinstance(size, int) or size <= 0 or
            not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest)):
        raise ValueError(f"NetSys manifest identity for {expected_path} is invalid")
    return value


def validate_netsys_manifest(manifest: object) -> dict:
    if not isinstance(manifest, dict):
        raise ValueError("NetSys manifest is not an object")
    required = {
        "schema", "state", "created_unix_ms", "credential_material", "task_name",
        "retail_executable", "original_dll", "backup_dll", "shim", "mode",
        "environment", "launcher_sha256",
    }
    if set(manifest) != required:
        raise ValueError("NetSys manifest fields are incomplete or unexpected")
    if (manifest.get("schema") != NETSYS_SCHEMA or
            manifest.get("state") not in {"snapshot", "replacement-staged", "installed",
                                           "restored"} or
            not isinstance(manifest.get("created_unix_ms"), int) or
            manifest.get("credential_material") != "none" or
            manifest.get("task_name") != NETSYS_TASK_NAME):
        raise ValueError("NetSys manifest header is invalid")
    exe = _netsys_file_identity(manifest["retail_executable"], RETAIL_EXE)
    original = _netsys_file_identity(manifest["original_dll"], RETAIL_NETSYS_DLL)
    backup = _netsys_file_identity(manifest["backup_dll"], NETSYS_BACKUP)
    if (exe["sha256"] != EXPECTED_SHA256 or
            original["sha256"] != EXPECTED_NETSYS_SHA256 or
            original["size"] != EXPECTED_NETSYS_SIZE or
            backup["sha256"] != original["sha256"] or
            backup["size"] != original["size"]):
        raise ValueError("NetSys manifest is not bound to the supported shipped files")
    shim = manifest["shim"]
    mode = manifest["mode"]
    environment = manifest["environment"]
    launcher_sha256 = manifest["launcher_sha256"]
    if manifest["state"] == "snapshot" or (
            manifest["state"] == "restored" and shim is None):
        if shim is not None or mode is not None or environment != {} or launcher_sha256 is not None:
            raise ValueError("snapshot NetSys manifest unexpectedly contains launch state")
    else:
        shim = _netsys_file_identity(shim, NETSYS_STAGED)
        if (shim["sha256"] == EXPECTED_NETSYS_SHA256 or
                not 64 * 1024 <= shim["size"] <= 16 * 1024 * 1024):
            raise ValueError("NetSys replacement identity is not a bounded distinct DLL")
        if mode not in {"load-only", "host", "host-bridge"} or not isinstance(
                environment, dict):
            raise ValueError("NetSys manifest mode/environment is invalid")
        if environment != netsys_environment(mode, environment.get("DON_NET_BIND", "")):
            raise ValueError("NetSys manifest environment is not an exact supported profile")
        if not isinstance(launcher_sha256, str) or not re.fullmatch(
                r"[0-9a-f]{64}", launcher_sha256):
            raise ValueError("NetSys manifest launcher identity is invalid")
    return manifest


def guest_file_record(path: str) -> dict:
    literal = ps_literal(path)
    script = f"""
$ErrorActionPreference = 'Stop'
$path = {literal}
if (Test-Path -LiteralPath $path -PathType Leaf) {{
    $file = Get-Item -LiteralPath $path
    $record = [pscustomobject]@{{
        present = $true
        path = $file.FullName
        size = [int64]$file.Length
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant()
    }}
}} else {{
    $record = [pscustomobject]@{{ present = $false; path = $path }}
}}
Write-Output '{NETSYS_JSON_BEGIN}'
ConvertTo-Json -InputObject $record -Compress
Write-Output '{NETSYS_JSON_END}'
"""
    record = extract_json_between(
        guest_ps_encoded(script), NETSYS_JSON_BEGIN, NETSYS_JSON_END
    )
    if not isinstance(record, dict) or not isinstance(record.get("present"), bool):
        raise ValueError(f"guest returned an invalid file record for {path}")
    if (not isinstance(record.get("path"), str) or
            normalize_windows_path(record["path"]) != normalize_windows_path(path)):
        raise ValueError(f"guest file record path does not match {path}")
    if record["present"]:
        if (set(record) != {"present", "path", "size", "sha256"} or
                not isinstance(record.get("size"), int) or record["size"] <= 0 or
                not isinstance(record.get("sha256"), str) or
                not re.fullmatch(r"[0-9a-f]{64}", record["sha256"])):
            raise ValueError(f"guest returned an invalid file identity for {path}")
    elif set(record) != {"present", "path"}:
        raise ValueError(f"guest returned unexpected missing-file fields for {path}")
    return record


def guest_read_bytes(path: str, maximum: int) -> bytes:
    if maximum <= 0 or maximum > 4 * 1024 * 1024:
        raise ValueError("guest read bound is invalid")
    script = f"""
$ErrorActionPreference = 'Stop'
$path = {ps_literal(path)}
$file = Get-Item -LiteralPath $path
if ($file.Length -gt {maximum}) {{ throw 'file exceeds host evidence bound' }}
Write-Output '{NETSYS_JSON_BEGIN}'
Write-Output ([Convert]::ToBase64String([IO.File]::ReadAllBytes($path)))
Write-Output '{NETSYS_JSON_END}'
"""
    output = guest_ps_encoded(script)
    lines = output.splitlines()
    starts = [i for i, line in enumerate(lines) if line.strip() == NETSYS_JSON_BEGIN]
    ends = [i for i, line in enumerate(lines) if line.strip() == NETSYS_JSON_END]
    if len(starts) != 1 or len(ends) != 1 or ends[0] != starts[0] + 2:
        raise ValueError("guest byte response has missing or ambiguous markers")
    try:
        data = base64.b64decode(lines[starts[0] + 1].strip(), validate=True)
    except ValueError as exc:
        raise ValueError("guest byte response is not canonical base64") from exc
    if len(data) > maximum:
        raise ValueError("guest byte response exceeds its declared bound")
    return data


def guest_write_bytes(path: str, data: bytes) -> None:
    encoded = base64.b64encode(data).decode("ascii")
    if not encoded or '"' in path or any(char in path for char in "\r\n"):
        raise ValueError("guest byte destination or payload is invalid")

    # Parallels' guest-exec transport can hang indefinitely when a complete file is
    # embedded in one doubly-base64-encoded PowerShell command.  Feed certutil bounded
    # canonical-base64 lines through cmd.exe instead.  The temporary files are adjacent
    # to the destination, and the final move is a same-volume replacement.
    encoded_temp = path + ".b64.tmp"
    decoded_temp = path + ".write.tmp"
    guest_cmd(
        f'if not exist "{str(PureWindowsPath(path).parent)}" '
        f'mkdir "{str(PureWindowsPath(path).parent)}" & '
        f'del /q "{encoded_temp}" "{decoded_temp}" 2>nul & exit /b 0'
    )
    try:
        for offset in range(0, len(encoded), 2048):
            redirect = ">" if offset == 0 else ">>"
            guest_cmd(f'{redirect}"{encoded_temp}" echo {encoded[offset:offset + 2048]}')
        guest_cmd(
            f'certutil.exe -f -decode "{encoded_temp}" "{decoded_temp}" >nul && '
            f'move /y "{decoded_temp}" "{path}" >nul'
        )
    finally:
        guest_cmd(
            f'del /q "{encoded_temp}" "{decoded_temp}" 2>nul & exit /b 0',
            check=False,
        )


def read_netsys_manifest() -> dict:
    try:
        raw = guest_read_bytes(NETSYS_MANIFEST, 64 * 1024)
        manifest = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ValueError("NetSys manifest is not canonical UTF-8 JSON") from exc
    return validate_netsys_manifest(manifest)


def write_netsys_manifest(manifest: dict) -> None:
    validate_netsys_manifest(manifest)
    guest_write_bytes(
        NETSYS_MANIFEST,
        (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode("utf-8"),
    )


def require_retail_absent(operation: str) -> None:
    try:
        pids, detail = process_pids()
    except RuntimeError as exc:
        raise SystemExit(f"REFUSING {operation}: {exc}") from exc
    if pids:
        raise SystemExit(
            f"REFUSING {operation} while riseofnations.exe is running as {pids}; "
            "close the disposable retail process first"
        )
    if detail and any(line.strip().isdigit() for line in detail.splitlines()):
        raise SystemExit(f"REFUSING {operation}: process enumeration was ambiguous")


def host_netsys_identity(shim: Path) -> dict:
    shim = shim.resolve()
    if not shim.is_file() or shim.name.lower() != "crossplaynetlib.dll":
        raise SystemExit("replacement must be an existing file named CrossplayNetLib.dll")
    if shim != DEFAULT_NETSYS_SHIM.resolve():
        raise SystemExit("replacement must be the current parity-gated netsys-shim build")
    parity = run([
        "uv", "run", "--with", "pefile", "--with", "capstone", "python",
        str(HERE.parents[1] / "crates/netsys-shim/check-exports.py"),
    ])
    if not parity.stdout.splitlines() or parity.stdout.splitlines()[-1].strip() != "PASS":
        raise SystemExit(f"replacement export/ABI parity gate did not pass:\n{parity.stdout}")
    identity = run(["file", str(shim)]).stdout.strip()
    if "PE32 executable" not in identity or "Intel 80386" not in identity or "DLL" not in identity:
        raise SystemExit(f"replacement is not a PE32/i386 DLL: {identity}")
    size = shim.stat().st_size
    digest = sha256_file(shim)
    if not 64 * 1024 <= size <= 16 * 1024 * 1024 or digest == EXPECTED_NETSYS_SHA256:
        raise SystemExit("replacement DLL identity is unbounded or identical to the shipped DLL")
    return {"path": str(shim), "size": size, "sha256": digest}


def netsys_snapshot() -> dict:
    require_retail_absent("NetSys snapshot")
    if guest_file_record(NETSYS_MANIFEST)["present"]:
        raise SystemExit("REFUSING to overwrite an existing NetSys experiment manifest")
    exe = guest_file_record(RETAIL_EXE)
    target = guest_file_record(RETAIL_NETSYS_DLL)
    stale_paths = [
        path for path in [NETSYS_BACKUP, NETSYS_STAGED, NETSYS_LAUNCHER,
                          NETSYS_LOAD_TRACE, NETSYS_HOST_TRACE, NETSYS_BRIDGE_TRACE,
                          NETSYS_LOAD_EXIT, NETSYS_HOST_EXIT, NETSYS_BRIDGE_EXIT]
        if guest_file_record(path)["present"]
    ]
    if stale_paths:
        raise SystemExit(
            "REFUSING orphaned NetSys experiment files without a manifest: " +
            ", ".join(stale_paths)
        )
    if (not exe["present"] or exe["sha256"] != EXPECTED_SHA256 or
            not target["present"] or target["sha256"] != EXPECTED_NETSYS_SHA256 or
            target["size"] != EXPECTED_NETSYS_SIZE):
        raise SystemExit(
            "REFUSING snapshot: retail executable/DLL identity is not shipped ground truth"
        )
    script = f"""
$ErrorActionPreference = 'Stop'
[IO.Directory]::CreateDirectory({ps_literal(NETSYS_ROOT)}) | Out-Null
$temp = {ps_literal(NETSYS_BACKUP + '.tmp')}
if (Test-Path -LiteralPath $temp) {{ Remove-Item -LiteralPath $temp -Force }}
[IO.File]::Copy({ps_literal(RETAIL_NETSYS_DLL)}, $temp, $false)
[IO.File]::Move($temp, {ps_literal(NETSYS_BACKUP)})
"""
    guest_ps_encoded(script)
    backup = guest_file_record(NETSYS_BACKUP)
    if (not backup["present"] or backup["sha256"] != EXPECTED_NETSYS_SHA256 or
            backup["size"] != EXPECTED_NETSYS_SIZE):
        raise SystemExit("REFUSING experiment: the immutable backup did not verify")
    manifest = {
        "schema": NETSYS_SCHEMA,
        "state": "snapshot",
        "created_unix_ms": int(time.time() * 1000),
        "credential_material": "none",
        "task_name": NETSYS_TASK_NAME,
        "retail_executable": {key: exe[key] for key in ("path", "size", "sha256")},
        "original_dll": {key: target[key] for key in ("path", "size", "sha256")},
        "backup_dll": {key: backup[key] for key in ("path", "size", "sha256")},
        "shim": None,
        "mode": None,
        "environment": {},
        "launcher_sha256": None,
    }
    write_netsys_manifest(manifest)
    print(json.dumps(manifest, indent=2, sort_keys=True))
    return manifest


def netsys_replace(shim: Path, port: int) -> dict:
    require_retail_absent("NetSys replacement")
    manifest = read_netsys_manifest()
    if manifest["state"] not in {"snapshot", "restored"}:
        raise SystemExit("REFUSING replacement: experiment is not at a shipped-DLL boundary")
    target = guest_file_record(RETAIL_NETSYS_DLL)
    backup = guest_file_record(NETSYS_BACKUP)
    if (not target["present"] or target["sha256"] != EXPECTED_NETSYS_SHA256 or
            not backup["present"] or backup["sha256"] != EXPECTED_NETSYS_SHA256):
        raise SystemExit("REFUSING replacement: target/backup does not prove shipped state")
    host = host_netsys_identity(shim)
    server = serve_once(port, shim.resolve().parent)
    download = NETSYS_STAGED + ".download"
    try:
        guest_cmd(
            f'curl.exe -f -sS -o "{download}" '
            f'http://10.211.55.2:{port}/CrossplayNetLib.dll'
        )
        downloaded = guest_file_record(download)
        if (not downloaded["present"] or downloaded["sha256"] != host["sha256"] or
                downloaded["size"] != host["size"]):
            raise SystemExit("REFUSING replacement: guest download does not match host DLL")
        guest_ps_encoded(
            f"Move-Item -LiteralPath {ps_literal(download)} "
            f"-Destination {ps_literal(NETSYS_STAGED)} -Force"
        )
        staged = guest_file_record(NETSYS_STAGED)
        if staged["sha256"] != host["sha256"] or staged["size"] != host["size"]:
            raise SystemExit("REFUSING replacement: staged DLL identity changed")
        environment = netsys_environment("load-only")
        launcher = netsys_launcher_text("load-only", environment).encode("ascii")
        guest_write_bytes(NETSYS_LAUNCHER, launcher)
        launcher_hash = hashlib.sha256(launcher).hexdigest()
        manifest.update({
            "state": "replacement-staged",
            "shim": {
                "path": NETSYS_STAGED,
                "size": staged["size"],
                "sha256": staged["sha256"],
            },
            "mode": "load-only",
            "environment": environment,
            "launcher_sha256": launcher_hash,
        })
        write_netsys_manifest(manifest)
        require_retail_absent("NetSys replacement final gate")
        final_target = guest_file_record(RETAIL_NETSYS_DLL)
        final_backup = guest_file_record(NETSYS_BACKUP)
        if (final_target.get("sha256") != EXPECTED_NETSYS_SHA256 or
                final_backup.get("sha256") != EXPECTED_NETSYS_SHA256):
            raise SystemExit("REFUSING replacement: shipped target/backup changed before swap")
        temp = RETAIL_NETSYS_DLL + ".don-next"
        script = f"""
$ErrorActionPreference = 'Stop'
if (Test-Path -LiteralPath {ps_literal(temp)}) {{
    Remove-Item -LiteralPath {ps_literal(temp)} -Force
}}
[IO.File]::Copy({ps_literal(NETSYS_STAGED)}, {ps_literal(temp)}, $false)
[IO.File]::Replace({ps_literal(temp)}, {ps_literal(RETAIL_NETSYS_DLL)}, $null)
"""
        guest_ps_encoded(script)
        installed = guest_file_record(RETAIL_NETSYS_DLL)
        if installed["sha256"] != staged["sha256"] or installed["size"] != staged["size"]:
            raise SystemExit("replacement completed without the exact staged DLL identity")
        manifest["state"] = "installed"
        write_netsys_manifest(manifest)
        print(json.dumps(manifest, indent=2, sort_keys=True))
        return manifest
    finally:
        guest_cmd(f'del /q "{download}" 2>nul & exit /b 0', check=False)
        server.shutdown()
        server.server_close()


def netsys_configure_host(bind: str) -> dict:
    require_retail_absent("NetSys host configuration")
    manifest = read_netsys_manifest()
    if manifest["state"] != "installed" or manifest["mode"] != "load-only":
        raise SystemExit("REFUSING host mode before an installed load-only experiment")
    target = guest_file_record(RETAIL_NETSYS_DLL)
    if not target["present"] or target["sha256"] != manifest["shim"]["sha256"]:
        raise SystemExit("REFUSING host mode: installed DLL no longer matches the manifest")
    trace_file = guest_file_record(NETSYS_LOAD_TRACE)
    if not trace_file["present"]:
        raise SystemExit("REFUSING host mode without a flushed load-only retail trace")
    try:
        trace = parse_netsys_trace(guest_read_bytes(NETSYS_LOAD_TRACE, 1024 * 1024).decode("utf-8"))
    except (UnicodeDecodeError, ValueError) as exc:
        raise SystemExit(f"REFUSING host mode: invalid load-only trace: {exc}") from exc
    try:
        validate_netsys_load_only_frontier(trace)
    except ValueError as exc:
        raise SystemExit(f"REFUSING host mode: {exc}") from exc
    exit_file = guest_file_record(NETSYS_LOAD_EXIT)
    if not exit_file["present"]:
        raise SystemExit("REFUSING host mode until the load-only retail process exits cleanly")
    try:
        load_exit = parse_netsys_exit(guest_read_bytes(NETSYS_LOAD_EXIT, 1024))
    except ValueError as exc:
        raise SystemExit(f"REFUSING host mode: {exc}") from exc
    if load_exit["exit_code"] not in NETSYS_NORMAL_EXIT_CODES:
        raise SystemExit(
            f"REFUSING host mode after load-only exit code {load_exit['exit_code']}"
        )
    if guest_file_record(NETSYS_HOST_TRACE)["present"]:
        raise SystemExit("REFUSING to append to an existing host-mode trace")
    environment = netsys_environment("host", bind)
    launcher = netsys_launcher_text("host", environment).encode("ascii")
    guest_write_bytes(NETSYS_LAUNCHER, launcher)
    manifest.update({
        "mode": "host",
        "environment": environment,
        "launcher_sha256": hashlib.sha256(launcher).hexdigest(),
    })
    write_netsys_manifest(manifest)
    print(json.dumps(manifest, indent=2, sort_keys=True))
    return manifest


def netsys_configure_bridge() -> dict:
    require_retail_absent("NetSys setup-bridge configuration")
    manifest = read_netsys_manifest()
    if manifest["state"] != "installed" or manifest["mode"] != "host":
        raise SystemExit("REFUSING setup bridge before a bridge-off host experiment")
    target = guest_file_record(RETAIL_NETSYS_DLL)
    if not target["present"] or target["sha256"] != manifest["shim"]["sha256"]:
        raise SystemExit("REFUSING setup bridge: installed DLL no longer matches the manifest")
    trace_file = guest_file_record(NETSYS_HOST_TRACE)
    if not trace_file["present"]:
        raise SystemExit("REFUSING setup bridge without a flushed bridge-off host trace")
    try:
        trace = parse_netsys_trace(
            guest_read_bytes(NETSYS_HOST_TRACE, 1024 * 1024).decode("utf-8")
        )
        validate_netsys_bridge_off_frontier(trace)
    except (UnicodeDecodeError, ValueError) as exc:
        raise SystemExit(f"REFUSING setup bridge: invalid host trace: {exc}") from exc
    exit_file = guest_file_record(NETSYS_HOST_EXIT)
    if not exit_file["present"]:
        raise SystemExit("REFUSING setup bridge until the bridge-off host exits cleanly")
    try:
        host_exit = parse_netsys_exit(guest_read_bytes(NETSYS_HOST_EXIT, 1024))
    except ValueError as exc:
        raise SystemExit(f"REFUSING setup bridge: {exc}") from exc
    if host_exit["exit_code"] != 0:
        raise SystemExit(
            f"REFUSING setup bridge after bridge-off exit code {host_exit['exit_code']}"
        )
    if guest_file_record(NETSYS_BRIDGE_TRACE)["present"]:
        raise SystemExit("REFUSING to append to an existing setup-bridge trace")
    bind = manifest["environment"]["DON_NET_BIND"]
    environment = netsys_environment("host-bridge", bind)
    launcher = netsys_launcher_text("host-bridge", environment).encode("ascii")
    guest_write_bytes(NETSYS_LAUNCHER, launcher)
    manifest.update({
        "mode": "host-bridge",
        "environment": environment,
        "launcher_sha256": hashlib.sha256(launcher).hexdigest(),
    })
    write_netsys_manifest(manifest)
    print(json.dumps(manifest, indent=2, sort_keys=True))
    return manifest


def delete_netsys_task() -> None:
    guest_cmd(
        f'schtasks.exe /delete /tn "\\{NETSYS_TASK_NAME}" /f >nul 2>nul & exit /b 0',
        check=False,
    )


def netsys_process_record(target_pid: int) -> dict:
    script = f"""
$ErrorActionPreference = 'Stop'
$process = Get-Process -Id {target_pid}
$record = [pscustomobject]@{{
    pid = [int]$process.Id
    path = $process.Path
    session_id = [int]$process.SessionId
    start_utc = $process.StartTime.ToUniversalTime().ToString('o')
}}
Write-Output '{NETSYS_JSON_BEGIN}'
ConvertTo-Json -InputObject $record -Compress
Write-Output '{NETSYS_JSON_END}'
"""
    record = extract_json_between(
        guest_ps_encoded(script), NETSYS_JSON_BEGIN, NETSYS_JSON_END
    )
    if (not isinstance(record, dict) or set(record) !=
            {"pid", "path", "session_id", "start_utc"} or
            record.get("pid") != target_pid or
            not isinstance(record.get("session_id"), int) or record["session_id"] <= 0 or
            not isinstance(record.get("start_utc"), str) or
            not isinstance(record.get("path"), str) or
            normalize_windows_path(record["path"]) != normalize_windows_path(RETAIL_EXE)):
        raise ValueError("retail process identity/session record is invalid")
    return record


def netsys_launch(timeout: float) -> dict:
    require_retail_absent("NetSys launch")
    manifest = read_netsys_manifest()
    if manifest["state"] != "installed":
        raise SystemExit("REFUSING launch without an installed experiment DLL")
    target = guest_file_record(RETAIL_NETSYS_DLL)
    launcher = guest_file_record(NETSYS_LAUNCHER)
    trace_path, exit_path = netsys_mode_paths(manifest["mode"])
    if (not target["present"] or target["sha256"] != manifest["shim"]["sha256"] or
            not launcher["present"] or launcher["sha256"] != manifest["launcher_sha256"]):
        raise SystemExit("REFUSING launch: target DLL or process-local launcher changed")
    if guest_file_record(trace_path)["present"] or guest_file_record(exit_path)["present"]:
        raise SystemExit("REFUSING launch because this mode already has trace/exit evidence")
    delete_netsys_task()
    script = f"""
$ErrorActionPreference = 'Stop'
$user = (Get-CimInstance Win32_ComputerSystem).UserName
if ([string]::IsNullOrWhiteSpace($user)) {{ throw 'no interactive Windows user' }}
$sessions = @(Get-Process -Name explorer -ErrorAction Stop |
    Where-Object {{ $_.SessionId -gt 0 }} | Select-Object -ExpandProperty SessionId -Unique)
if ($sessions.Count -ne 1) {{ throw 'interactive explorer session is missing or ambiguous' }}
$service = New-Object -ComObject 'Schedule.Service'
$service.Connect()
$folder = $service.GetFolder('\\')
$definition = $service.NewTask(0)
$definition.RegistrationInfo.Description = 'Disposable credential-free DON NetSys launch'
$definition.Settings.Enabled = $true
$definition.Settings.AllowDemandStart = $true
$definition.Settings.DisallowStartIfOnBatteries = $false
$definition.Settings.StopIfGoingOnBatteries = $false
$definition.Settings.ExecutionTimeLimit = 'PT0S'
$definition.Principal.UserId = $user
$definition.Principal.LogonType = 3
$definition.Principal.RunLevel = 0
$action = $definition.Actions.Create(0)
$action.Path = "$env:SystemRoot\\System32\\cmd.exe"
$action.Arguments = {ps_literal('/d /c call "' + NETSYS_LAUNCHER + '"')}
$action.WorkingDirectory = {ps_literal(NETSYS_ROOT)}
$task = $folder.RegisterTaskDefinition(
    {ps_literal(NETSYS_TASK_NAME)}, $definition, 6, $user, $null, 3, $null)
$null = $task.Run($null)
$record = [pscustomobject]@{{ session_id = [int]$sessions[0]; task_started = $true }}
Write-Output '{NETSYS_JSON_BEGIN}'
ConvertTo-Json -InputObject $record -Compress
Write-Output '{NETSYS_JSON_END}'
"""
    try:
        launched = extract_json_between(
            guest_ps_encoded(script), NETSYS_JSON_BEGIN, NETSYS_JSON_END
        )
        if (not isinstance(launched, dict) or launched.get("task_started") is not True or
                not isinstance(launched.get("session_id"), int)):
            raise SystemExit("interactive launch task returned an invalid record")
        deadline = time.monotonic() + timeout
        target_pid = None
        while time.monotonic() < deadline:
            pids, _ = process_pids()
            if len(pids) == 1:
                target_pid = pids[0]
                break
            if len(pids) > 1:
                raise SystemExit(f"launch created ambiguous retail processes: {pids}")
            time.sleep(0.1)
        if target_pid is None:
            raise SystemExit("interactive launch did not produce a retail process")
        process = netsys_process_record(target_pid)
        if process["session_id"] != launched["session_id"]:
            raise SystemExit("retail launched outside the one interactive Explorer session")
        result = {
            "schema": NETSYS_SCHEMA,
            "operation": "launch",
            "credential_material": "none",
            "mode": manifest["mode"],
            "environment": manifest["environment"],
            "process": process,
            "host_activation": (
                "not-applicable-load-only" if manifest["mode"] == "load-only" else
                "staged-awaiting-retail-ui-or-explicit-invoke; launcher does not call ns_host"
            ),
        }
        print(json.dumps(result, indent=2, sort_keys=True))
        return result
    finally:
        delete_netsys_task()


def netsys_listener_records(target_pid: int) -> list[dict]:
    script = f"""
$ErrorActionPreference = 'Stop'
$rows = @(Get-NetTCPConnection -OwningProcess {target_pid} -ErrorAction SilentlyContinue |
    Sort-Object LocalAddress, LocalPort, State |
    ForEach-Object {{
        [pscustomobject]@{{
            local_address = $_.LocalAddress
            local_port = [int]$_.LocalPort
            state = [string]$_.State
        }}
    }})
Write-Output '{NETSYS_JSON_BEGIN}'
ConvertTo-Json -InputObject $rows -Compress
Write-Output '{NETSYS_JSON_END}'
"""
    rows = extract_json_between(
        guest_ps_encoded(script), NETSYS_JSON_BEGIN, NETSYS_JSON_END
    )
    if not isinstance(rows, list):
        raise ValueError("TCP listener response is not an array")
    normalized = []
    for row in rows:
        if (not isinstance(row, dict) or set(row) !=
                {"local_address", "local_port", "state"} or
                not isinstance(row.get("local_address"), str) or
                not isinstance(row.get("local_port"), int) or
                not 0 <= row["local_port"] <= 65535 or
                not isinstance(row.get("state"), str)):
            raise ValueError("TCP listener response has an invalid row")
        normalized.append(row)
    return normalized


def netsys_status() -> dict:
    manifest_record = guest_file_record(NETSYS_MANIFEST)
    manifest = read_netsys_manifest() if manifest_record["present"] else None
    try:
        pids, process_detail = process_pids()
        process_error = None
    except RuntimeError as exc:
        pids, process_detail, process_error = [], "", str(exc)
    files = {}
    for label, path in {
        "retail_executable": RETAIL_EXE,
        "target_dll": RETAIL_NETSYS_DLL,
        "backup_dll": NETSYS_BACKUP,
        "staged_dll": NETSYS_STAGED,
        "launcher": NETSYS_LAUNCHER,
        "load_only_trace": NETSYS_LOAD_TRACE,
        "host_trace": NETSYS_HOST_TRACE,
        "host_bridge_trace": NETSYS_BRIDGE_TRACE,
    }.items():
        files[label] = guest_file_record(path)
    exits = {}
    for label, path in {
        "load-only": NETSYS_LOAD_EXIT,
        "host": NETSYS_HOST_EXIT,
        "host-bridge": NETSYS_BRIDGE_EXIT,
    }.items():
        record = guest_file_record(path)
        if record["present"]:
            try:
                exits[label] = parse_netsys_exit(guest_read_bytes(path, 1024))
            except ValueError as exc:
                exits[label] = {"error": str(exc)}
        else:
            exits[label] = None
    module = None
    if len(pids) == 1:
        listing = remote_modules(pids[0])
        matches = [item for item in listing.get("modules", [])
                   if item["name"].lower() == "crossplaynetlib.dll"]
        if listing["status"] == "ok" and len(matches) == 1:
            item = matches[0]
            identity = guest_file_record(item["path"])
            module = {**item, "file_size": identity.get("size"),
                      "sha256": identity.get("sha256")}
        else:
            module = {"status": "unavailable", "detail": listing.get("detail"),
                      "match_count": len(matches)}
    result = {
        "schema": NETSYS_SCHEMA,
        "operation": "status",
        "mutation": "none",
        "credential_material": "none",
        "manifest": manifest,
        "files": files,
        "exits": exits,
        "process": {
            "pids": pids,
            "enumeration_detail": process_detail,
            "error": process_error,
        },
        "loaded_module": module,
    }
    print(json.dumps(result, indent=2, sort_keys=True))
    return result


def netsys_capture(output: Path, generation: str | None, timeout: float) -> dict:
    manifest = read_netsys_manifest()
    if manifest["state"] != "installed":
        raise SystemExit("REFUSING capture without an installed experiment")
    target_pid = pid()
    process = netsys_process_record(target_pid)
    exe = guest_file_record(RETAIL_EXE)
    target = guest_file_record(RETAIL_NETSYS_DLL)
    if (exe["sha256"] != EXPECTED_SHA256 or
            target["sha256"] != manifest["shim"]["sha256"]):
        raise SystemExit("REFUSING capture: executable or installed DLL identity changed")
    listing = remote_modules(target_pid)
    if listing["status"] != "ok":
        raise SystemExit("REFUSING capture: x86 module inventory is incomplete")
    matches = [item for item in listing["modules"]
               if item["name"].lower() == "crossplaynetlib.dll"]
    if len(matches) != 1:
        raise SystemExit("REFUSING capture: CrossplayNetLib module is missing or ambiguous")
    loaded = matches[0]
    if normalize_windows_path(loaded["path"]) != normalize_windows_path(RETAIL_NETSYS_DLL):
        raise SystemExit("REFUSING capture: retail loaded CrossplayNetLib from another path")
    loaded_file = guest_file_record(loaded["path"])
    if loaded_file["sha256"] != manifest["shim"]["sha256"]:
        raise SystemExit("REFUSING capture: mapped module file does not match the manifest")
    trace_path, exit_path = netsys_mode_paths(manifest["mode"])
    trace_file = guest_file_record(trace_path)
    if not trace_file["present"]:
        raise SystemExit("REFUSING capture without the explicit flushed mode trace")
    try:
        trace_raw = guest_read_bytes(trace_path, 1024 * 1024).decode("utf-8")
        trace = parse_netsys_trace(trace_raw, target_pid)
    except (UnicodeDecodeError, ValueError) as exc:
        raise SystemExit(f"REFUSING malformed NetSys trace: {exc}") from exc
    if trace["factory_ready"] is None:
        raise SystemExit("REFUSING trace without exactly one factory-ready boundary")
    if (manifest["mode"] == "load-only") != trace["load_only"]:
        raise SystemExit("REFUSING trace whose load-only state contradicts the manifest")
    if manifest["mode"] == "load-only":
        try:
            validate_netsys_load_only_frontier(trace)
        except ValueError as exc:
            raise SystemExit(f"REFUSING incomplete load-only trace: {exc}") from exc
    elif manifest["mode"] == "host-bridge":
        details = [record["detail"] for record in trace["records"]]
        if (not any(detail.startswith("setup_bridge=ok action=add_player ")
                    for detail in details) or
                any(detail.startswith("setup_bridge=refused ") for detail in details)):
            raise SystemExit("REFUSING setup-bridge capture without one clean bridged slot")
    observation = None
    if generation is not None:
        events = send(["observe-network"], timeout, generation_root(generation))
        terminal = [event for event in events if event.get("phase") == "observed"]
        if len(terminal) != 1:
            raise SystemExit("REFUSING capture without one passive network observation")
        observation = terminal[0]
    listeners = netsys_listener_records(target_pid)
    if manifest["mode"] == "load-only":
        host_activation = {"status": "not-applicable-load-only"}
    else:
        try:
            validate_netsys_bridge_off_frontier(trace)
            host_activation = {"status": "ns_host-and-player-callback-observed"}
        except ValueError as exc:
            host_activation = {
                "status": "staged-awaiting-retail-ui-or-explicit-invoke",
                "detail": str(exc),
            }
    exit_record = None
    if guest_file_record(exit_path)["present"]:
        try:
            exit_record = parse_netsys_exit(guest_read_bytes(exit_path, 1024))
        except ValueError as exc:
            raise SystemExit(f"REFUSING malformed retail exit record: {exc}") from exc
    artifact = {
        "schema": NETSYS_SCHEMA,
        "operation": "capture",
        "mutation": "none except an explicitly requested passive main-thread observation",
        "credential_material": "none",
        "mode": manifest["mode"],
        "environment": manifest["environment"],
        "process": process,
        "retail_executable": {key: exe[key] for key in ("path", "size", "sha256")},
        "installed_dll": {key: target[key] for key in ("path", "size", "sha256")},
        "loaded_module": {
            "path": loaded["path"],
            "base": loaded["base_hex"],
            "size_of_image": loaded["size"],
            "file_size": loaded_file["size"],
            "sha256": loaded_file["sha256"],
        },
        "local_tcp_endpoints": listeners,
        "trace": {
            "path": trace_path,
            "size": trace_file["size"],
            "sha256": trace_file["sha256"],
            "factory_ready": trace["factory_ready"],
            "records": trace["records"],
        },
        "controller_observe_network": observation,
        "host_activation": host_activation,
        "exit": exit_record,
        "redacted_by_design": [
            "environment outside DON_NET_*", "tickets", "tokens", "lobby ids",
            "platform ids", "Steam ids", "packet payloads", "remote endpoints",
        ],
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(artifact, indent=2, sort_keys=True) + "\n")
    print(json.dumps(artifact, indent=2, sort_keys=True))
    print(f"wrote credential-free NetSys evidence to {output}")
    return artifact


def netsys_restore() -> dict:
    require_retail_absent("NetSys restore")
    manifest = read_netsys_manifest()
    backup = guest_file_record(NETSYS_BACKUP)
    target = guest_file_record(RETAIL_NETSYS_DLL)
    if (not backup["present"] or backup["sha256"] != EXPECTED_NETSYS_SHA256 or
            backup["size"] != EXPECTED_NETSYS_SIZE):
        raise SystemExit("REFUSING restore: immutable shipped backup does not verify")
    allowed_current = {EXPECTED_NETSYS_SHA256}
    if manifest["shim"] is not None:
        allowed_current.add(manifest["shim"]["sha256"])
    if not target["present"] or target["sha256"] not in allowed_current:
        raise SystemExit("REFUSING restore over an unknown current CrossplayNetLib.dll")
    delete_netsys_task()
    if target["sha256"] != EXPECTED_NETSYS_SHA256:
        require_retail_absent("NetSys restore final gate")
        final_target = guest_file_record(RETAIL_NETSYS_DLL)
        final_backup = guest_file_record(NETSYS_BACKUP)
        if (final_target.get("sha256") != target["sha256"] or
                final_backup.get("sha256") != EXPECTED_NETSYS_SHA256):
            raise SystemExit("REFUSING restore: target/backup changed before swap")
        temp = RETAIL_NETSYS_DLL + ".don-restore"
        script = f"""
$ErrorActionPreference = 'Stop'
if (Test-Path -LiteralPath {ps_literal(temp)}) {{
    Remove-Item -LiteralPath {ps_literal(temp)} -Force
}}
[IO.File]::Copy({ps_literal(NETSYS_BACKUP)}, {ps_literal(temp)}, $false)
[IO.File]::Replace({ps_literal(temp)}, {ps_literal(RETAIL_NETSYS_DLL)}, $null)
"""
        guest_ps_encoded(script)
    restored = guest_file_record(RETAIL_NETSYS_DLL)
    if (restored["sha256"] != EXPECTED_NETSYS_SHA256 or
            restored["size"] != EXPECTED_NETSYS_SIZE):
        raise SystemExit("restore did not reproduce the shipped DLL identity")
    guest_cmd(f'del /q "{NETSYS_LAUNCHER}" 2>nul & exit /b 0', check=False)
    manifest["state"] = "restored"
    write_netsys_manifest(manifest)
    result = {
        "schema": NETSYS_SCHEMA,
        "operation": "restore",
        "state": "restored",
        "credential_material": "none",
        "restored_dll": {key: restored[key] for key in ("path", "size", "sha256")},
        "backup_retained": NETSYS_BACKUP,
        "manifest_retained": NETSYS_MANIFEST,
    }
    print(json.dumps(result, indent=2, sort_keys=True))
    return result


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    sub = ap.add_subparsers(dest="action", required=True)
    def add_generation(parser: argparse.ArgumentParser, default: str = DEFAULT_GENERATION) -> None:
        parser.add_argument("--generation", default=default)

    status_parser = sub.add_parser("status")
    add_generation(status_parser)
    preflight_parser = sub.add_parser(
        "preflight", help="read-only retail relaunch and controller-lifecycle diagnostics"
    )
    preflight_parser.add_argument("--pid", type=int)
    preflight_parser.add_argument(
        "--max-generations", type=positive_generation_budget,
        default=DEFAULT_GENERATION_BUDGET,
    )
    prepare_parser = sub.add_parser(
        "prepare-injector", help="strict-build and atomically install the x86 guest injector"
    )
    prepare_parser.add_argument("--port", type=int, default=18081)
    sub.add_parser("build")
    sub.add_parser(
        "netsys-snapshot",
        help="snapshot the closed retail CrossplayNetLib.dll into an immutable hash-bound backup",
    )
    netsys_replace_parser = sub.add_parser(
        "netsys-replace",
        help="replace the closed retail DLL and create a load-only process-local launcher",
    )
    netsys_replace_parser.add_argument("--shim", type=Path, default=DEFAULT_NETSYS_SHIM)
    netsys_replace_parser.add_argument("--port", type=int, default=18083)
    netsys_host_parser = sub.add_parser(
        "netsys-configure-host",
        help=("stage bridge-off host transport after load-only; does not bypass retail "
              "UI/auth gates or invoke ns_host"),
    )
    netsys_host_parser.add_argument(
        "--bind", choices=["127.0.0.1:31337", "0.0.0.0:31337"],
        default="127.0.0.1:31337",
    )
    sub.add_parser(
        "netsys-configure-bridge",
        help="promote a proven bridge-off host run to DON_NET_SETUP_BRIDGE=1",
    )
    netsys_launch_parser = sub.add_parser(
        "netsys-launch",
        help="launch retail once in the interactive session with process-local DON_NET_* values",
    )
    netsys_launch_parser.add_argument("--timeout", type=float, default=30.0)
    sub.add_parser("netsys-status", help="read-only NetSys experiment and module status")
    netsys_capture_parser = sub.add_parser(
        "netsys-capture",
        help="capture credential-free process/module/trace/local-network evidence",
    )
    netsys_capture_parser.add_argument("--generation")
    netsys_capture_parser.add_argument("--timeout", type=float, default=5.0)
    netsys_capture_parser.add_argument(
        "--output", type=Path,
        default=HERE.parents[1] / "schema/live/retail-netsys-experiment-v1.json",
    )
    sub.add_parser(
        "netsys-restore",
        help="atomically restore the hash-bound shipped CrossplayNetLib.dll",
    )
    d = sub.add_parser("deploy")
    d.add_argument("--pid", type=int)
    d.add_argument("--port", type=int, default=18082)
    d.add_argument("--max-generations", type=positive_generation_budget,
                   default=DEFAULT_GENERATION_BUDGET)
    d.add_argument("--injector-port", type=int, default=18081)
    add_generation(d)
    u = sub.add_parser("upgrade")
    u.add_argument("--pid", type=int)
    u.add_argument("--port", type=int, default=18082)
    u.add_argument("--from-generation", default=LEGACY_GENERATION)
    u.add_argument("--max-generations", type=positive_generation_budget,
                   default=DEFAULT_GENERATION_BUDGET)
    u.add_argument("--injector-port", type=int, default=18081)
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
    elif a.action == "preflight": prelaunch_command(a.max_generations, a.pid)
    elif a.action == "prepare-injector": prepare_injector(a.port)
    elif a.action == "build": build()
    elif a.action == "netsys-snapshot": netsys_snapshot()
    elif a.action == "netsys-replace": netsys_replace(a.shim, a.port)
    elif a.action == "netsys-configure-host": netsys_configure_host(a.bind)
    elif a.action == "netsys-configure-bridge": netsys_configure_bridge()
    elif a.action == "netsys-launch": netsys_launch(a.timeout)
    elif a.action == "netsys-status": netsys_status()
    elif a.action == "netsys-capture": netsys_capture(
        a.output.resolve(), a.generation, a.timeout
    )
    elif a.action == "netsys-restore": netsys_restore()
    elif a.action == "deploy": deploy(
        a.pid or pid(), a.port, a.generation, a.max_generations, a.injector_port
    )
    elif a.action == "upgrade":
        target_pid = a.pid or pid()
        prepare_injector(a.injector_port)
        enforce_generation_budget(
            target_pid, a.generation, a.max_generations, require_unhooked=False
        )
        stop(generation_root(a.from_generation))
        deploy(
            target_pid, a.port, a.generation, a.max_generations,
            a.injector_port, prepare=False,
        )
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
