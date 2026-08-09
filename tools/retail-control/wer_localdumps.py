#!/usr/bin/env python3
"""Scoped WER LocalDumps management for the supported retail executable.

This utility is intentionally separate from ``retailctl.py``.  ``check`` and
``verify`` are read-only.  ``setup`` and ``remove`` are the only mutating actions,
both refuse while retail is running, and neither action recursively removes the
dump directory or any file in it.
"""

from __future__ import annotations

import argparse
import csv
from dataclasses import dataclass
from datetime import datetime, timezone
import json
from pathlib import PureWindowsPath
import re
import subprocess
import sys
import time
from typing import Any


VM = "Windows 11"
PROCESS_NAME = "riseofnations.exe"
TARGET_PATH = (
    r"C:\Program Files (x86)\Steam\steamapps\common\Rise of Nations\riseofnations.exe"
)
EXPECTED_SHA256 = "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079"
WER_KEY = (
    r"HKLM\SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps"
    + "\\"
    + PROCESS_NAME
)
DUMP_DIR = r"C:\Users\Public\don-crashdumps\riseofnations"
REGISTRY_VIEWS = ("64", "32")
DUMP_COUNT = 2
DUMP_TYPE = 2
MIN_FREE_BYTES = 12 * 1024**3

SYSTEM_SID = "S-1-5-18"
ADMINISTRATORS_SID = "S-1-5-32-544"
FULL_CONTROL = 2_032_127
# `icacls (M)` persists FILE_GENERIC_* plus SYNCHRONIZE.  Get-Acl therefore
# reports 0x1301BF, not the bare .NET FileSystemRights.Modify value 0x301BF.
MODIFY = 1_245_631
OBJECT_AND_CONTAINER_INHERIT = 3


class WorkflowError(RuntimeError):
    """A fail-closed precondition, mutation, or round-trip failure."""


@dataclass(frozen=True)
class CommandResult:
    returncode: int
    stdout: str


class GuestTransport:
    """Minimal Parallels SYSTEM-session transport used elsewhere in this repo."""

    def _run(self, command: list[str], timeout: float) -> CommandResult:
        try:
            process = subprocess.run(
                command,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                timeout=timeout,
                check=False,
            )
        except (OSError, subprocess.SubprocessError) as error:
            raise WorkflowError(f"guest command failed to start: {error}") from error
        return CommandResult(
            process.returncode,
            process.stdout.replace("\r\n", "\n").strip(),
        )

    def cmd(
        self, command: str, *, check: bool = True, timeout: float = 30.0
    ) -> CommandResult:
        result = self._run(
            ["prlctl", "exec", VM, "cmd.exe", "/d", "/s", "/c", command],
            timeout,
        )
        if check and result.returncode:
            raise WorkflowError(
                f"guest cmd.exe failed ({result.returncode}): {command}\n{result.stdout}"
            )
        return result

    def ps(
        self, command: str, *, check: bool = True, timeout: float = 30.0
    ) -> CommandResult:
        result = self._run(
            [
                "prlctl",
                "exec",
                VM,
                "powershell.exe",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                command,
            ],
            timeout,
        )
        if check and result.returncode:
            raise WorkflowError(
                f"guest PowerShell failed ({result.returncode}): {command}\n{result.stdout}"
            )
        return result


def _validate_user(user: str) -> str:
    if not re.fullmatch(r"[A-Za-z0-9_.-]{1,64}", user):
        raise WorkflowError(f"unsafe interactive user name {user!r}")
    return user


def _ps_literal(value: str) -> str:
    return "'" + value.replace("'", "''") + "'"


def _parse_json(text: str, context: str) -> Any:
    try:
        return json.loads(text)
    except json.JSONDecodeError as error:
        raise WorkflowError(f"invalid JSON from {context}: {text!r}") from error


def _task_pids(output: str, returncode: int = 0) -> list[int]:
    if returncode:
        raise WorkflowError(
            f"tasklist failed ({returncode}); process state is unknown: {output!r}"
        )
    lines = [line for line in output.splitlines() if line.strip()]
    if len(lines) == 1 and lines[0].strip().casefold() == (
        "info: no tasks are running which match the specified criteria."
    ):
        return []
    if not lines:
        raise WorkflowError("tasklist returned no parseable process state")

    pids: list[int] = []
    for row in csv.reader(lines):
        if len(row) < 2 or row[0].strip().casefold() != PROCESS_NAME:
            raise WorkflowError(f"malformed tasklist row: {row!r}")
        try:
            pids.append(int(row[1].replace(",", "").strip()))
        except ValueError as error:
            raise WorkflowError(f"malformed tasklist PID: {row!r}") from error
    if not pids:
        raise WorkflowError("tasklist did not establish whether retail is running")
    return sorted(set(pids))


def _active_console_user(output: str, wanted: str) -> bool:
    wanted = wanted.casefold()
    for raw in output.splitlines():
        line = raw.strip().lstrip(">").strip()
        fields = line.split()
        if not fields or fields[0].casefold() != wanted:
            continue
        if any(field.casefold() == "console" for field in fields) and any(
            field.casefold() == "active" for field in fields
        ):
            return True
    return False


def _parse_registry(output: str, returncode: int) -> dict[str, Any]:
    if returncode:
        error_lines = [line.strip().casefold() for line in output.splitlines() if line.strip()]
        missing = "error: the system was unable to find the specified registry key or value."
        if returncode == 1 and error_lines == [missing]:
            return {"exists": False, "values": {}}
        raise WorkflowError(
            f"reg.exe query failed ({returncode}); key state is unknown: {output!r}"
        )

    canonical_suffix = (
        r"\software\microsoft\windows\windows error reporting\localdumps"
        + "\\"
        + PROCESS_NAME
    ).casefold()
    headers = [
        line.strip().casefold()
        for line in output.splitlines()
        if line.strip().upper().startswith("HKEY_LOCAL_MACHINE")
    ]
    if not any(header.endswith(canonical_suffix) for header in headers):
        raise WorkflowError(f"reg.exe query returned malformed success output: {output!r}")

    values: dict[str, dict[str, Any]] = {}
    for line in output.splitlines():
        match = re.match(r"^\s*(\S+)\s+(REG_\S+)\s+(.+?)\s*$", line)
        if not match:
            continue
        name, kind, raw = match.groups()
        value: Any = raw
        if kind == "REG_DWORD":
            try:
                value = int(raw, 0)
            except ValueError:
                value = raw
        values[name] = {"type": kind, "value": value}
    return {"exists": True, "values": values}


def _registry_state(transport: GuestTransport, view: str) -> dict[str, Any]:
    result = transport.cmd(
        f'reg.exe query "{WER_KEY}" /reg:{view}', check=False
    )
    return _parse_registry(result.stdout, result.returncode)


def _folder_state(transport: GuestTransport) -> dict[str, Any]:
    path = _ps_literal(DUMP_DIR)
    command = (
        "$ErrorActionPreference='Stop'; "
        f"$p={path}; if (Test-Path -LiteralPath $p) {{ "
        "if (-not (Test-Path -LiteralPath $p -PathType Container)) { "
        "throw 'scoped dump path exists but is not a directory' }; "
        "$items=@(Get-ChildItem -LiteralPath $p -Force -ErrorAction Stop); "
        "[pscustomobject]@{exists=$true;count=$items.Count;"
        "names=@($items | ForEach-Object {$_.Name})} | "
        "ConvertTo-Json -Compress -Depth 3 } else { "
        "[pscustomobject]@{exists=$false;count=0;names=@()} | "
        "ConvertTo-Json -Compress -Depth 3 }"
    )
    result = transport.ps(command, check=False)
    if result.returncode:
        raise WorkflowError(
            f"dump-directory probe failed ({result.returncode}): {result.stdout}"
        )
    parsed = _parse_json(result.stdout, "dump-directory inspection")
    if not isinstance(parsed, dict) or not isinstance(parsed.get("exists"), bool):
        raise WorkflowError(f"malformed dump-directory state: {parsed!r}")
    try:
        parsed["count"] = int(parsed["count"])
    except (KeyError, TypeError, ValueError) as error:
        raise WorkflowError(f"malformed dump-directory count: {parsed!r}") from error
    if parsed["count"] < 0 or not isinstance(parsed.get("names"), list):
        raise WorkflowError(f"malformed dump-directory contents: {parsed!r}")
    parsed["names"] = list(parsed["names"])
    if len(parsed["names"]) != parsed["count"]:
        raise WorkflowError(f"incoherent dump-directory contents: {parsed!r}")
    if not parsed["exists"] and (parsed["count"] or parsed["names"]):
        raise WorkflowError(f"absent dump directory reported contents: {parsed!r}")
    return parsed


def _acl_state(transport: GuestTransport) -> dict[str, Any]:
    path = _ps_literal(DUMP_DIR)
    command = (
        "$ErrorActionPreference='Stop'; "
        f"$a=Get-Acl -LiteralPath {path}; $rules=@($a.Access | ForEach-Object {{ "
        "$sid=$_.IdentityReference.Translate("
        "[System.Security.Principal.SecurityIdentifier]).Value; "
        "[pscustomobject]@{sid=$sid;rights=[int]$_.FileSystemRights;"
        "allow=($_.AccessControlType.ToString() -eq 'Allow');"
        "inherited=$_.IsInherited;inheritance=[int]$_.InheritanceFlags;"
        "propagation=[int]$_.PropagationFlags} }); "
        "[pscustomobject]@{protected=$a.AreAccessRulesProtected;rules=$rules} | "
        "ConvertTo-Json -Compress -Depth 4"
    )
    result = transport.ps(command, check=False)
    if result.returncode:
        raise WorkflowError(f"dump-directory ACL probe failed: {result.stdout}")
    parsed = _parse_json(result.stdout, "dump-directory ACL")
    if not isinstance(parsed, dict) or not isinstance(parsed.get("protected"), bool):
        raise WorkflowError(f"malformed dump-directory ACL state: {parsed!r}")
    if not isinstance(parsed.get("rules"), list):
        raise WorkflowError(f"malformed dump-directory ACL rules: {parsed!r}")
    parsed["rules"] = list(parsed["rules"])
    return parsed


def _acl_is_exact(acl: dict[str, Any] | None, user_sid: str) -> bool:
    if not acl or acl.get("protected") is not True:
        return False
    expected = {
        (SYSTEM_SID, FULL_CONTROL, True, False, OBJECT_AND_CONTAINER_INHERIT, 0),
        (
            ADMINISTRATORS_SID,
            FULL_CONTROL,
            True,
            False,
            OBJECT_AND_CONTAINER_INHERIT,
            0,
        ),
        (user_sid, MODIFY, True, False, OBJECT_AND_CONTAINER_INHERIT, 0),
    }
    actual = {
        (
            str(rule.get("sid")),
            int(rule.get("rights", -1)),
            bool(rule.get("allow")),
            bool(rule.get("inherited")),
            int(rule.get("inheritance", -1)),
            int(rule.get("propagation", -1)),
        )
        for rule in acl.get("rules", [])
    }
    return actual == expected


def _registry_is_exact(state: dict[str, Any]) -> bool:
    if not state.get("exists"):
        return False
    return state.get("values") == {
        "DumpFolder": {"type": "REG_EXPAND_SZ", "value": DUMP_DIR},
        "DumpCount": {"type": "REG_DWORD", "value": DUMP_COUNT},
        "DumpType": {"type": "REG_DWORD", "value": DUMP_TYPE},
    }


def inspect_state(transport: GuestTransport, user: str) -> dict[str, Any]:
    """Collect all read-only setup ownership and identity facts."""

    user = _validate_user(user)
    tasklist = transport.cmd(
        f'tasklist /NH /FI "IMAGENAME eq {PROCESS_NAME}" /FO CSV', check=False
    )
    whoami = transport.cmd("whoami", check=False)
    sessions = transport.cmd("quser", check=False)
    sid_command = (
        "$a=New-Object System.Security.Principal.NTAccount($env:COMPUTERNAME,"
        f"{_ps_literal(user)}); "
        "$a.Translate([System.Security.Principal.SecurityIdentifier]).Value"
    )
    sid_result = transport.ps(sid_command, check=False)
    sid = sid_result.stdout.strip() if sid_result.returncode == 0 else ""
    if sid and not re.fullmatch(r"S-\d+(?:-\d+)+", sid):
        sid = ""

    hash_result = transport.ps(
        f"(Get-FileHash -LiteralPath {_ps_literal(TARGET_PATH)} "
        "-Algorithm SHA256).Hash",
        check=False,
    )
    digest = "".join(hash_result.stdout.split()).casefold()
    if hash_result.returncode or not re.fullmatch(r"[0-9a-f]{64}", digest):
        digest = ""

    free_result = transport.ps("[int64](Get-PSDrive -Name C).Free", check=False)
    try:
        free_bytes = int(free_result.stdout.strip()) if free_result.returncode == 0 else -1
    except ValueError:
        free_bytes = -1

    folder = _folder_state(transport)
    acl = _acl_state(transport) if folder.get("exists") else None
    registry = {
        view: _registry_state(transport, view) for view in REGISTRY_VIEWS
    }
    return {
        "schema": "don.wer-localdumps-state.v1",
        "vm": VM,
        "transport_account": whoami.stdout.strip(),
        "process_name": PROCESS_NAME,
        "process_pids": _task_pids(tasklist.stdout, tasklist.returncode),
        "target_path": TARGET_PATH,
        "target_sha256": digest,
        "target_sha256_expected": EXPECTED_SHA256,
        "interactive_user": user,
        "interactive_user_sid": sid,
        "interactive_console_active": _active_console_user(sessions.stdout, user),
        "free_bytes_c": free_bytes,
        "minimum_free_bytes": MIN_FREE_BYTES,
        "registry": registry,
        "dump_directory": folder,
        "acl": acl,
    }


def _identity_errors(state: dict[str, Any], *, require_disk: bool) -> list[str]:
    errors: list[str] = []
    if str(state.get("transport_account", "")).casefold() != "nt authority\\system":
        errors.append("Parallels guest transport is not running as NT AUTHORITY\\SYSTEM")
    if state.get("process_pids"):
        errors.append(f"{PROCESS_NAME} is running: {state['process_pids']}")
    if state.get("target_sha256") != EXPECTED_SHA256:
        errors.append("installed retail executable is missing or has the wrong SHA-256")
    if not state.get("interactive_console_active"):
        errors.append("requested interactive user is not active on the console session")
    if not state.get("interactive_user_sid"):
        errors.append("requested interactive user's SID could not be resolved")
    if require_disk and int(state.get("free_bytes_c", -1)) < MIN_FREE_BYTES:
        errors.append("C: has less than the required 12 GiB free")
    return errors


def check_report(state: dict[str, Any]) -> dict[str, Any]:
    user_sid = str(state.get("interactive_user_sid", ""))
    checks = {
        "system_transport": str(state.get("transport_account", "")).casefold()
        == "nt authority\\system",
        "no_retail_process": not state.get("process_pids"),
        "pinned_target_hash": state.get("target_sha256") == EXPECTED_SHA256,
        "active_console_user": bool(state.get("interactive_console_active") and user_sid),
        "disk_budget": int(state.get("free_bytes_c", -1)) >= MIN_FREE_BYTES,
        "registry_64_exact": _registry_is_exact(state["registry"]["64"]),
        "registry_32_exact": _registry_is_exact(state["registry"]["32"]),
        "dump_directory_exists": bool(state["dump_directory"].get("exists")),
        "protected_acl_exact": _acl_is_exact(state.get("acl"), user_sid),
    }
    return {
        "schema": "don.wer-localdumps-check.v1",
        "ok": all(checks.values()),
        "checks": checks,
        "state": state,
    }


def _set_acl(transport: GuestTransport, user_sid: str) -> None:
    transport.cmd(
        f'icacls "{DUMP_DIR}" /grant:r '
        f'"*{SYSTEM_SID}:(OI)(CI)(F)" '
        f'"*{ADMINISTRATORS_SID}:(OI)(CI)(F)" '
        f'"*{user_sid}:(OI)(CI)(M)"'
    )
    # Keep the explicit grants above, then remove only the inherited Public-folder ACEs.
    transport.cmd(f'icacls "{DUMP_DIR}" /inheritance:r')


def _write_registry_view(transport: GuestTransport, view: str) -> None:
    values = (
        ("DumpFolder", "REG_EXPAND_SZ", DUMP_DIR),
        ("DumpCount", "REG_DWORD", str(DUMP_COUNT)),
        ("DumpType", "REG_DWORD", str(DUMP_TYPE)),
    )
    for name, kind, value in values:
        transport.cmd(
            f'reg.exe add "{WER_KEY}" /v {name} /t {kind} '
            f'/d "{value}" /f /reg:{view}'
        )


def _delete_registry_view(
    transport: GuestTransport, view: str, *, check: bool
) -> CommandResult:
    return transport.cmd(
        f'reg.exe delete "{WER_KEY}" /f /reg:{view}', check=check
    )


def setup(transport: GuestTransport, user: str) -> dict[str, Any]:
    """Explicitly install the scoped configuration, with bounded rollback."""

    state = inspect_state(transport, user)
    errors = _identity_errors(state, require_disk=True)
    if any(view["exists"] for view in state["registry"].values()):
        errors.append("a per-application LocalDumps key already exists")
    if state["dump_directory"].get("exists"):
        errors.append("dump directory already exists; ownership is not assumed")
    if errors:
        raise WorkflowError("setup refused: " + "; ".join(errors))

    touched_views: list[str] = []
    folder_created = False
    try:
        # A successful mkdir is the ownership boundary. If another actor wins the race,
        # mkdir fails and rollback must not touch their directory.
        transport.cmd(f'mkdir "{DUMP_DIR}"')
        folder_created = True
        _set_acl(transport, state["interactive_user_sid"])
        folder = _folder_state(transport)
        acl = _acl_state(transport)
        if not folder.get("exists") or folder.get("count") != 0:
            raise WorkflowError("new dump directory is not empty")
        if not _acl_is_exact(acl, state["interactive_user_sid"]):
            raise WorkflowError("new dump directory ACL did not round-trip exactly")

        for view in REGISTRY_VIEWS:
            touched_views.append(view)
            _write_registry_view(transport, view)

        final_state = inspect_state(transport, user)
        report = check_report(final_state)
        if not report["ok"]:
            failed = [name for name, ok in report["checks"].items() if not ok]
            raise WorkflowError("setup round-trip failed: " + ", ".join(failed))
        return {
            "schema": "don.wer-localdumps-setup.v1",
            "ok": True,
            "configuration": report,
        }
    except BaseException as error:
        rollback_errors: list[str] = []
        for view in reversed(touched_views):
            try:
                _delete_registry_view(transport, view, check=False)
                if _registry_state(transport, view)["exists"]:
                    rollback_errors.append(f"registry view {view} remains present")
            except BaseException as rollback_error:
                rollback_errors.append(f"registry view {view}: {rollback_error}")
        if folder_created:
            # Non-recursive rmdir cannot delete a dump or any other retained evidence.
            try:
                transport.cmd(f'rmdir "{DUMP_DIR}"', check=False)
                folder = _folder_state(transport)
                if folder.get("exists") and folder.get("count") == 0:
                    rollback_errors.append("empty tool-created dump directory remains")
            except BaseException as rollback_error:
                rollback_errors.append(f"dump directory: {rollback_error}")
        if rollback_errors:
            raise WorkflowError(
                f"{error}; transactional rollback incomplete: "
                + "; ".join(rollback_errors)
            ) from error
        raise


def remove(transport: GuestTransport, user: str) -> dict[str, Any]:
    """Remove only an exact owned configuration; retain the directory and dumps."""

    state = inspect_state(transport, user)
    errors = _identity_errors(state, require_disk=False)
    for view in REGISTRY_VIEWS:
        if not _registry_is_exact(state["registry"][view]):
            errors.append(f"registry view {view} is absent or not exactly tool-owned")
    if not state["dump_directory"].get("exists"):
        errors.append("owned dump directory is absent")
    if not _acl_is_exact(state.get("acl"), state.get("interactive_user_sid", "")):
        errors.append("dump directory ACL is not exactly tool-owned")
    if errors:
        raise WorkflowError("remove refused: " + "; ".join(errors))

    deleted: list[str] = []
    try:
        for view in REGISTRY_VIEWS:
            _delete_registry_view(transport, view, check=True)
            deleted.append(view)
        for view in REGISTRY_VIEWS:
            if _registry_state(transport, view)["exists"]:
                raise WorkflowError(f"registry view {view} survived deletion")
    except BaseException as error:
        # The values were exact before deletion, so restoring a deleted view is safe.
        rollback_errors: list[str] = []
        for view in deleted:
            try:
                _write_registry_view(transport, view)
                if not _registry_is_exact(_registry_state(transport, view)):
                    rollback_errors.append(f"registry view {view} did not round-trip")
            except BaseException as rollback_error:
                rollback_errors.append(f"registry view {view}: {rollback_error}")
        if rollback_errors:
            raise WorkflowError(
                f"{error}; remove rollback incomplete: " + "; ".join(rollback_errors)
            ) from error
        raise

    folder = _folder_state(transport)
    return {
        "schema": "don.wer-localdumps-remove.v1",
        "ok": True,
        "removed_registry_views": list(REGISTRY_VIEWS),
        "dump_directory_preserved": bool(folder.get("exists")),
        "retained_item_count": int(folder.get("count", 0)),
        "retained_items": list(folder.get("names") or []),
    }


def _parse_since(value: str) -> datetime:
    normalized = value.strip()
    if normalized.endswith("Z"):
        normalized = normalized[:-1] + "+00:00"
    try:
        parsed = datetime.fromisoformat(normalized)
    except ValueError as error:
        raise WorkflowError(f"invalid --since timestamp {value!r}") from error
    if parsed.tzinfo is None:
        raise WorkflowError("--since must include a UTC offset or Z")
    return parsed.astimezone(timezone.utc)


def _list_dumps(
    transport: GuestTransport, since: datetime
) -> list[dict[str, Any]]:
    since_text = since.isoformat().replace("+00:00", "Z")
    command = (
        f"$since=[DateTimeOffset]::Parse({_ps_literal(since_text)}).UtcDateTime; "
        f"$files=@(Get-ChildItem -LiteralPath {_ps_literal(DUMP_DIR)} "
        f"-Filter {_ps_literal(PROCESS_NAME + '*.dmp')} -File -ErrorAction SilentlyContinue | "
        "Where-Object {$_.LastWriteTimeUtc -ge $since} | "
        "Sort-Object FullName | ForEach-Object { "
        "[pscustomobject]@{path=$_.FullName;name=$_.Name;length=[int64]$_.Length;"
        "last_write_utc=$_.LastWriteTimeUtc.ToString('o')} }); "
        "[pscustomobject]@{files=$files} | ConvertTo-Json -Compress -Depth 4"
    )
    result = transport.ps(command, check=False, timeout=60.0)
    if result.returncode:
        raise WorkflowError(f"dump listing failed: {result.stdout}")
    parsed = _parse_json(result.stdout, "dump listing")
    files = parsed.get("files") or []
    if isinstance(files, dict):
        files = [files]
    return list(files)


def _safe_dump_path(path: str) -> bool:
    candidate = PureWindowsPath(path)
    root = PureWindowsPath(DUMP_DIR)
    name = candidate.name.casefold()
    return (
        str(candidate.parent).casefold() == str(root).casefold()
        and name.startswith(PROCESS_NAME.casefold())
        and name.endswith(".dmp")
    )


def _dump_identity(transport: GuestTransport, path: str) -> dict[str, str]:
    if not _safe_dump_path(path):
        raise WorkflowError(f"refusing dump path outside the scoped directory: {path!r}")
    literal = _ps_literal(path)
    command = (
        f"$p={literal}; $s=[System.IO.File]::Open($p,"
        "[System.IO.FileMode]::Open,[System.IO.FileAccess]::Read,"
        "[System.IO.FileShare]::ReadWrite); try { $b=New-Object byte[] 4; "
        "$n=$s.Read($b,0,4) } finally { $s.Dispose() }; "
        "$magic=if($n -eq 4){[System.Text.Encoding]::ASCII.GetString($b)}else{''}; "
        "$hash=(Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash.ToLowerInvariant(); "
        "[pscustomobject]@{magic=$magic;sha256=$hash} | ConvertTo-Json -Compress"
    )
    result = transport.ps(command, check=False, timeout=300.0)
    if result.returncode:
        raise WorkflowError(f"dump identity failed for {path}: {result.stdout}")
    parsed = _parse_json(result.stdout, "dump identity")
    return {"magic": str(parsed.get("magic", "")), "sha256": str(parsed.get("sha256", ""))}


def _events(transport: GuestTransport, since: datetime) -> list[dict[str, Any]]:
    since_text = since.isoformat().replace("+00:00", "Z")
    command = (
        f"$since=[DateTimeOffset]::Parse({_ps_literal(since_text)}).UtcDateTime; "
        "$events=@(Get-WinEvent -FilterHashtable @{LogName='Application';StartTime=$since} "
        "-ErrorAction SilentlyContinue | Where-Object {($_.Id -eq 1000 -or $_.Id -eq 1001) "
        f"-and $_.Message -like {_ps_literal('*' + PROCESS_NAME + '*')}}} | "
        "Sort-Object TimeCreated | ForEach-Object { [pscustomobject]@{"
        "time_created_utc=$_.TimeCreated.ToUniversalTime().ToString('o');"
        "id=$_.Id;provider=$_.ProviderName;record_id=$_.RecordId;message=$_.Message} }); "
        "[pscustomobject]@{events=$events} | ConvertTo-Json -Compress -Depth 5"
    )
    result = transport.ps(command, check=False, timeout=60.0)
    if result.returncode:
        return [{"error": result.stdout}]
    parsed = _parse_json(result.stdout, "Application event query")
    events = parsed.get("events") or []
    if isinstance(events, dict):
        events = [events]
    return list(events)


def verify(
    transport: GuestTransport, since_text: str, *, stable_wait_seconds: float = 2.0
) -> dict[str, Any]:
    """Verify retained dumps in place without copying or modifying them."""

    since = _parse_since(since_text)
    first = {item["path"]: item for item in _list_dumps(transport, since)}
    time.sleep(stable_wait_seconds)
    second = {item["path"]: item for item in _list_dumps(transport, since)}

    records: list[dict[str, Any]] = []
    for path in sorted(set(first) | set(second)):
        earlier = first.get(path)
        later = second.get(path)
        reasons: list[str] = []
        if later is None:
            records.append(
                {
                    **first[path],
                    "stable": False,
                    "valid": False,
                    "reasons": ["dump disappeared during stability window"],
                }
            )
            continue
        try:
            length = int(later.get("length", 0))
        except (TypeError, ValueError):
            length = 0
        if not _safe_dump_path(path):
            reasons.append("path outside scoped dump directory")
        if length <= 0:
            reasons.append("empty dump")
        if earlier is None:
            reasons.append("dump appeared during stability window")
        else:
            if int(earlier.get("length", -1)) != length:
                reasons.append("dump length changed during stability window")
            if earlier.get("last_write_utc") != later.get("last_write_utc"):
                reasons.append("dump write timestamp changed during stability window")

        identity: dict[str, str] = {}
        if not reasons:
            identity = _dump_identity(transport, path)
            if identity.get("magic") != "MDMP":
                reasons.append("missing MDMP signature")
            if not re.fullmatch(r"[0-9a-f]{64}", identity.get("sha256", "")):
                reasons.append("invalid SHA-256")
        records.append(
            {
                **later,
                **identity,
                "stable": earlier is not None and not any("stability" in r or "changed" in r for r in reasons),
                "valid": not reasons,
                "reasons": reasons,
            }
        )

    return {
        "schema": "don.wer-localdumps-verify.v1",
        "since_utc": since.isoformat().replace("+00:00", "Z"),
        "ok": bool(records) and all(record["valid"] for record in records),
        "dumps": records,
        "application_events": _events(transport, since),
        "copied_dumps": False,
    }


def _emit(payload: dict[str, Any], stream: Any = sys.stdout) -> None:
    print(json.dumps(payload, indent=2, sort_keys=True), file=stream)


def main(argv: list[str] | None = None, transport: GuestTransport | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "action", choices=("check", "setup", "verify", "remove")
    )
    parser.add_argument("--interactive-user", default="ember")
    parser.add_argument("--since", help="UTC timestamp for verify, including Z or offset")
    args = parser.parse_args(argv)
    guest = transport or GuestTransport()
    try:
        if args.action == "check":
            payload = check_report(inspect_state(guest, args.interactive_user))
            _emit(payload)
            return 0 if payload["ok"] else 1
        if args.action == "setup":
            _emit(setup(guest, args.interactive_user))
            return 0
        if args.action == "remove":
            _emit(remove(guest, args.interactive_user))
            return 0
        if not args.since:
            parser.error("verify requires --since with a UTC offset or Z")
        payload = verify(guest, args.since)
        _emit(payload)
        return 0 if payload["ok"] else 1
    except WorkflowError as error:
        _emit(
            {"schema": "don.wer-localdumps-error.v1", "ok": False, "error": str(error)},
            sys.stderr,
        )
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
