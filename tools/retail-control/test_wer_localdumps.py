#!/usr/bin/env python3
"""Tests for the scoped WER utility; every guest mutation is modelled in memory."""

from __future__ import annotations

import copy
import hashlib
import json
import os
from pathlib import Path, PureWindowsPath
import re
import struct
import sys
import tempfile
from types import SimpleNamespace
import unittest


sys.path.insert(0, str(Path(__file__).resolve().parent))

import wer_localdumps as wer  # noqa: E402


CAPTURED_QUSER = """ USERNAME              SESSIONNAME        ID  STATE   IDLE TIME  LOGON TIME
 ember                 console             1  Active      none   8/8/2026 11:07 AM
"""
CAPTURED_NO_TASKS = (
    "INFO: No tasks are running which match the specified criteria."
)
CAPTURED_RUNNING = (
    '"riseofnations.exe","12324","Console","1","813,244 K"'
)
USER_SID = "S-1-5-21-1000-1001-1002-1003"
HASH = wer.EXPECTED_SHA256.upper()


def minidump_fixture(
    *,
    pid: int = 12324,
    process_name: str = wer.PROCESS_NAME,
    full_memory: bool = True,
) -> bytes:
    """Build a compact but structurally complete x86 full-memory minidump."""

    stream_types = (7, 4, 3, 6, 15, 9)
    image_base = 0x0040_0000
    image_size = 0x2000
    stack_start = 0x7000_0000
    stack_size = 0x1000
    thread_id = 77
    data = bytearray(32 + len(stream_types) * 12)

    def allocate(blob: bytes | bytearray, alignment: int = 4) -> int:
        while len(data) % alignment:
            data.append(0)
        rva = len(data)
        data.extend(blob)
        return rva

    context = bytes((index * 17) & 0xFF for index in range(256))
    context_rva = allocate(context)
    encoded_name = (
        rf"C:\Program Files (x86)\Steam\{process_name}".encode("utf-16-le")
    )
    module_name_rva = allocate(struct.pack("<I", len(encoded_name)) + encoded_name)

    memory_payload = bytes(image_size + stack_size)
    memory_base_rva = allocate(memory_payload, alignment=8)

    system_info = bytearray(56)
    struct.pack_into("<HHHBB", system_info, 0, 0, 6, 0x3A09, 4, 1)
    struct.pack_into("<IIII", system_info, 8, 10, 0, 19045, 2)

    module = bytearray(108)
    struct.pack_into(
        "<QIIII",
        module,
        0,
        image_base,
        image_size,
        0,
        0x63A1_B2C3,
        module_name_rva,
    )
    module_list = struct.pack("<I", 1) + module

    thread = struct.pack(
        "<IIIIQQIIII",
        thread_id,
        0,
        0,
        0,
        0x7FFD_E000,
        stack_start,
        stack_size,
        memory_base_rva + image_size,
        len(context),
        context_rva,
    )
    thread_list = struct.pack("<I", 1) + thread

    exception = bytearray(168)
    struct.pack_into("<IIII", exception, 0, thread_id, 0, 0xC000_0005, 0)
    struct.pack_into("<QQ", exception, 16, 0, image_base + 0x1000)
    struct.pack_into("<I", exception, 32, 0)
    struct.pack_into("<II", exception, 160, len(context), context_rva)

    misc_info = struct.pack(
        "<IIIIII", 24, wer.MINIDUMP_MISC1_PROCESS_ID, pid, 1_700_000_000, 1, 1
    )
    memory64 = struct.pack(
        "<QQQQQQ",
        2,
        memory_base_rva,
        image_base,
        image_size,
        stack_start,
        stack_size,
    )

    payloads = {
        7: bytes(system_info),
        4: bytes(module_list),
        3: bytes(thread_list),
        6: bytes(exception),
        15: misc_info,
        9: memory64,
    }
    directories: list[tuple[int, int, int]] = []
    for stream_type in stream_types:
        payload = payloads[stream_type]
        directories.append((stream_type, len(payload), allocate(payload)))

    flags = wer.MINIDUMP_WITH_FULL_MEMORY if full_memory else 0
    struct.pack_into(
        "<IIIIIIQ",
        data,
        0,
        wer.MINIDUMP_SIGNATURE,
        wer.MINIDUMP_VERSION,
        len(directories),
        32,
        0,
        1_723_170_600,
        flags,
    )
    for index, directory in enumerate(directories):
        struct.pack_into("<III", data, 32 + index * 12, *directory)
    return bytes(data)


def stream_rva(dump: bytes | bytearray, wanted: int) -> int:
    _, _, count, directory_rva = struct.unpack_from("<IIII", dump, 0)
    for index in range(count):
        stream_type, _, rva = struct.unpack_from(
            "<III", dump, directory_rva + index * 12
        )
        if stream_type == wanted:
            return rva
    raise AssertionError(f"missing fixture stream {wanted}")


def exact_values() -> dict[str, dict[str, object]]:
    return {
        "DumpFolder": {"type": "REG_EXPAND_SZ", "value": wer.DUMP_DIR},
        "DumpCount": {"type": "REG_DWORD", "value": wer.DUMP_COUNT},
        "DumpType": {"type": "REG_DWORD", "value": wer.DUMP_TYPE},
    }


def exact_acl() -> dict[str, object]:
    return {
        "protected": True,
        "rules": [
            {
                "sid": wer.SYSTEM_SID,
                "rights": wer.FULL_CONTROL,
                "allow": True,
                "inherited": False,
                "inheritance": wer.OBJECT_AND_CONTAINER_INHERIT,
                "propagation": 0,
            },
            {
                "sid": wer.ADMINISTRATORS_SID,
                "rights": wer.FULL_CONTROL,
                "allow": True,
                "inherited": False,
                "inheritance": wer.OBJECT_AND_CONTAINER_INHERIT,
                "propagation": 0,
            },
            {
                "sid": USER_SID,
                "rights": wer.MODIFY,
                "allow": True,
                "inherited": False,
                "inheritance": wer.OBJECT_AND_CONTAINER_INHERIT,
                "propagation": 0,
            },
        ],
    }


class FakeTransport:
    """A small stateful guest model driven by captured command-output shapes."""

    def __init__(self) -> None:
        self.tasklist = CAPTURED_NO_TASKS
        self.tasklist_returncode = 0
        self.quser = CAPTURED_QUSER
        self.sid = USER_SID
        self.digest = HASH
        self.free_bytes = 72 * 1024**3
        self.registry: dict[str, dict[str, dict[str, object]] | None] = {
            "64": None,
            "32": None,
        }
        self.folder_exists = False
        self.folder_names: list[str] = []
        self.acl: dict[str, object] | None = None
        self.dump_snapshots: list[list[dict[str, object]]] = []
        self.dump_identity = {
            "magic": "MDMP",
            "sha256": "a" * 64,
        }
        self.events: list[dict[str, object]] = []
        self.mutations: list[str] = []
        self.reads: list[str] = []
        self.fail_once: str | None = None
        self.registry_query_error: dict[str, tuple[int, str]] = {}
        self.folder_probe_error: tuple[int, str] | None = None
        self.acl_probe_error: tuple[int, str] | None = None
        self.mkdir_race = False

    def configured(self, *, files: list[str] | None = None) -> None:
        self.registry = {
            "64": exact_values(),
            "32": exact_values(),
        }
        self.folder_exists = True
        self.folder_names = list(files or [])
        self.acl = exact_acl()

    @staticmethod
    def _view(command: str) -> str:
        match = re.search(r"/reg:(32|64)", command)
        if not match:
            raise AssertionError(f"registry command lacks explicit view: {command}")
        return match.group(1)

    def _finish(
        self, command: str, returncode: int, stdout: str, check: bool
    ) -> wer.CommandResult:
        if check and returncode:
            raise wer.WorkflowError(
                f"synthetic guest failure ({returncode}): {command}\n{stdout}"
            )
        return wer.CommandResult(returncode, stdout)

    def _maybe_fail(self, command: str, check: bool) -> wer.CommandResult | None:
        if self.fail_once and self.fail_once in command:
            self.fail_once = None
            return self._finish(command, 1, "synthetic failure", check)
        return None

    def cmd(
        self, command: str, *, check: bool = True, timeout: float = 30.0
    ) -> wer.CommandResult:
        del timeout
        if command.startswith("tasklist "):
            self.reads.append(command)
            return self._finish(
                command, self.tasklist_returncode, self.tasklist, check
            )
        if command == "whoami":
            self.reads.append(command)
            return self._finish(command, 0, "nt authority\\system", check)
        if command == "quser":
            self.reads.append(command)
            return self._finish(command, 0, self.quser, check)
        if command.startswith("reg.exe query "):
            self.reads.append(command)
            view = self._view(command)
            if view in self.registry_query_error:
                returncode, output = self.registry_query_error[view]
                return self._finish(command, returncode, output, check)
            values = self.registry[view]
            if values is None:
                return self._finish(
                    command,
                    1,
                    "ERROR: The system was unable to find the specified registry key or value.",
                    check,
                )
            lines = [
                wer.WER_KEY.replace("HKLM\\", "HKEY_LOCAL_MACHINE\\", 1)
            ]
            for name, item in values.items():
                value = item["value"]
                if item["type"] == "REG_DWORD":
                    value = hex(int(value))
                lines.append(f"    {name}    {item['type']}    {value}")
            return self._finish(command, 0, "\n".join(lines), check)

        self.mutations.append(command)
        failed = self._maybe_fail(command, check)
        if failed is not None:
            return failed
        if command.startswith(f'mkdir "{wer.DUMP_DIR}"'):
            if self.mkdir_race:
                self.folder_exists = True
                self.folder_names = []
                self.acl = None
                return self._finish(command, 1, "another actor created it", check)
            if self.folder_exists:
                return self._finish(command, 1, "already exists", check)
            self.folder_exists = True
            self.folder_names = []
            self.acl = None
            return self._finish(command, 0, "", check)
        if command.startswith(f'icacls "{wer.DUMP_DIR}" /inheritance:r'):
            return self._finish(command, 0, "Successfully processed 1 files", check)
        if command.startswith(f'icacls "{wer.DUMP_DIR}" /grant:r'):
            self.acl = exact_acl()
            return self._finish(command, 0, "Successfully processed 1 files", check)
        if command.startswith("reg.exe add "):
            view = self._view(command)
            name_match = re.search(r" /v (\S+) /t (REG_\S+) /d \"([^\"]*)\"", command)
            if not name_match:
                raise AssertionError(f"cannot parse synthetic reg add: {command}")
            name, kind, raw = name_match.groups()
            value: object = int(raw) if kind == "REG_DWORD" else raw
            if self.registry[view] is None:
                self.registry[view] = {}
            self.registry[view][name] = {"type": kind, "value": value}
            return self._finish(command, 0, "The operation completed successfully.", check)
        if command.startswith("reg.exe delete "):
            view = self._view(command)
            if self.registry[view] is None:
                return self._finish(command, 1, "key not found", check)
            self.registry[view] = None
            return self._finish(command, 0, "The operation completed successfully.", check)
        if command == f'rmdir "{wer.DUMP_DIR}"':
            if self.folder_exists and not self.folder_names:
                self.folder_exists = False
                self.acl = None
                return self._finish(command, 0, "", check)
            return self._finish(command, 1, "directory not empty", check)
        raise AssertionError(f"unexpected guest cmd: {command}")

    def ps(
        self, command: str, *, check: bool = True, timeout: float = 30.0
    ) -> wer.CommandResult:
        del timeout
        self.reads.append(command)
        if "System.Security.Principal.NTAccount" in command:
            return self._finish(command, 0, self.sid, check)
        if "Get-FileHash -LiteralPath" in command and "File]::Open" not in command:
            return self._finish(command, 0, self.digest, check)
        if "Get-PSDrive -Name C" in command:
            return self._finish(command, 0, str(self.free_bytes), check)
        if "Test-Path -LiteralPath" in command:
            if self.folder_probe_error is not None:
                returncode, output = self.folder_probe_error
                return self._finish(command, returncode, output, check)
            body = {
                "exists": self.folder_exists,
                "count": len(self.folder_names),
                "names": list(self.folder_names),
            }
            return self._finish(command, 0, json.dumps(body, separators=(",", ":")), check)
        if "Get-Acl -LiteralPath" in command:
            if self.acl_probe_error is not None:
                returncode, output = self.acl_probe_error
                return self._finish(command, returncode, output, check)
            if not self.folder_exists or self.acl is None:
                return self._finish(command, 1, "path or ACL absent", check)
            return self._finish(command, 0, json.dumps(self.acl, separators=(",", ":")), check)
        if "$files=@(Get-ChildItem" in command:
            if self.dump_snapshots:
                files = self.dump_snapshots.pop(0)
            else:
                files = []
            return self._finish(
                command,
                0,
                json.dumps({"files": files}, separators=(",", ":")),
                check,
            )
        if "[System.IO.File]::Open" in command:
            return self._finish(
                command,
                0,
                json.dumps(self.dump_identity, separators=(",", ":")),
                check,
            )
        if "Get-WinEvent" in command:
            return self._finish(
                command,
                0,
                json.dumps({"events": self.events}, separators=(",", ":")),
                check,
            )
        raise AssertionError(f"unexpected guest PowerShell: {command}")


class ParsingTests(unittest.TestCase):
    def test_captured_tasklist_and_session_outputs(self) -> None:
        self.assertEqual(wer._task_pids(CAPTURED_NO_TASKS), [])
        self.assertEqual(wer._task_pids(CAPTURED_RUNNING), [12324])
        self.assertTrue(wer._active_console_user(CAPTURED_QUSER, "ember"))
        self.assertFalse(wer._active_console_user(CAPTURED_QUSER, "someone"))

    def test_captured_registry_types_are_preserved(self) -> None:
        captured = f"""
HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\Windows Error Reporting\\LocalDumps\\riseofnations.exe
    DumpFolder    REG_EXPAND_SZ    {wer.DUMP_DIR}
    DumpCount    REG_DWORD    0x2
    DumpType    REG_DWORD    0x2
"""
        state = wer._parse_registry(captured, 0)
        self.assertTrue(wer._registry_is_exact(state))
        state["values"]["DumpFolder"]["type"] = "REG_SZ"
        self.assertFalse(wer._registry_is_exact(state))

    def test_registry_absence_is_distinct_from_query_failure(self) -> None:
        absent = wer._parse_registry(
            "ERROR: The system was unable to find the specified registry key or value.",
            1,
        )
        self.assertFalse(absent["exists"])
        with self.assertRaisesRegex(wer.WorkflowError, "key state is unknown"):
            wer._parse_registry("ERROR: Access is denied.", 1)
        with self.assertRaisesRegex(wer.WorkflowError, "key state is unknown"):
            wer._parse_registry(
                "ERROR: The system was unable to find the specified registry key or value.\n"
                "ERROR: Access is denied.",
                1,
            )
        with self.assertRaisesRegex(wer.WorkflowError, "malformed success"):
            wer._parse_registry("The operation completed successfully.", 0)

    def test_tasklist_failure_and_malformed_success_are_unknown(self) -> None:
        with self.assertRaisesRegex(wer.WorkflowError, "process state is unknown"):
            wer._task_pids("ERROR: Access is denied.", 1)
        with self.assertRaisesRegex(wer.WorkflowError, "no parseable process state"):
            wer._task_pids("", 0)
        with self.assertRaisesRegex(wer.WorkflowError, "malformed tasklist row"):
            wer._task_pids("unexpected output", 0)

    def test_since_requires_an_offset(self) -> None:
        with self.assertRaises(wer.WorkflowError):
            wer._parse_since("2026-08-09T03:00:00")


class MinidumpValidationTests(unittest.TestCase):
    def test_complete_full_dump_binds_pid_module_exception_and_stack(self) -> None:
        report = wer.validate_minidump(
            minidump_fixture(),
            expected_pid=12324,
            expected_process_create_time=1_700_000_000,
            expected_module_timestamp=0x63A1_B2C3,
            expected_module_size=0x2000,
        )
        self.assertTrue(report["valid"], report["reasons"])
        self.assertEqual(report["header"]["version"], wer.MINIDUMP_VERSION)
        self.assertEqual(report["misc_info"]["process_id"], 12324)
        self.assertEqual(
            PureWindowsPath(report["target_module"]["name"]).name.casefold(),
            wer.PROCESS_NAME,
        )
        self.assertEqual(report["memory_kind"], "Memory64ListStream")
        self.assertTrue(report["exception_coverage"]["thread_present"])
        self.assertTrue(report["exception_coverage"]["context_present"])
        self.assertTrue(report["exception_coverage"]["stack_covered"])
        self.assertTrue(report["exception_coverage"]["address_covered"])

    def test_truncated_header_and_stream_directory_fail_closed(self) -> None:
        header = wer.validate_minidump(minidump_fixture()[:20])
        self.assertFalse(header["valid"])
        self.assertIn("truncated MINIDUMP_HEADER", header["reasons"])

        directory = bytearray(minidump_fixture())
        struct.pack_into("<I", directory, 12, len(directory) - 4)
        report = wer.validate_minidump(directory)
        self.assertFalse(report["valid"])
        self.assertTrue(
            any("MINIDUMP_DIRECTORY" in reason for reason in report["reasons"]),
            report["reasons"],
        )

    def test_torn_memory64_payload_and_missing_full_memory_flag_are_rejected(self) -> None:
        torn = bytearray(minidump_fixture())
        memory64_rva = stream_rva(torn, 9)
        struct.pack_into("<Q", torn, memory64_rva + 40, 0x1000_0000)
        report = wer.validate_minidump(torn)
        self.assertFalse(report["valid"])
        self.assertTrue(
            any("Memory64" in reason for reason in report["reasons"]),
            report["reasons"],
        )

        no_flag = wer.validate_minidump(minidump_fixture(full_memory=False))
        self.assertFalse(no_flag["valid"])
        self.assertIn("full-memory dump flag is absent", no_flag["reasons"])

    def test_exception_thread_pid_and_module_identity_mismatches_are_rejected(self) -> None:
        pid = wer.validate_minidump(minidump_fixture(), expected_pid=999)
        self.assertFalse(pid["valid"])
        self.assertTrue(any("expected PID" in reason for reason in pid["reasons"]))

        create_time = wer.validate_minidump(
            minidump_fixture(), expected_process_create_time=1_700_000_001
        )
        self.assertFalse(create_time["valid"])
        self.assertIn(
            "dump process creation time does not match expected attempt",
            create_time["reasons"],
        )

        missing_module = wer.validate_minidump(
            minidump_fixture(process_name="someone-else.exe")
        )
        self.assertFalse(missing_module["valid"])
        self.assertTrue(
            any("exactly one" in reason for reason in missing_module["reasons"])
        )

        missing_thread = bytearray(minidump_fixture())
        exception_rva = stream_rva(missing_thread, 6)
        struct.pack_into("<I", missing_thread, exception_rva, 999)
        report = wer.validate_minidump(missing_thread)
        self.assertFalse(report["valid"])
        self.assertIn(
            "ExceptionStream thread is absent from ThreadListStream", report["reasons"]
        )


class ArchiveValidationTests(unittest.TestCase):
    def _archive(self, directory: str, payload: bytes, name_hash: str | None = None) -> Path:
        digest = name_hash or hashlib.sha256(payload).hexdigest()
        path = Path(directory, digest + ".dmp")
        path.write_bytes(payload)
        path.chmod(0o444)
        return path

    def test_content_addressed_read_only_archive_round_trips(self) -> None:
        payload = minidump_fixture()
        digest = hashlib.sha256(payload).hexdigest()
        with tempfile.TemporaryDirectory() as directory:
            path = self._archive(directory, payload)
            report = wer.validate_content_addressed_archive(
                path,
                expected_sha256=digest,
                expected_length=len(payload),
                expected_pid=12324,
                expected_process_create_time=1_700_000_000,
            )
        self.assertTrue(report["valid"], report["reasons"])
        self.assertEqual(report["sha256"], digest)
        self.assertEqual(report["content_address"], "sha256:" + digest)
        self.assertTrue(report["read_only"])
        self.assertTrue(report["dump"]["valid"])

    def test_archive_rejects_hash_mismatch_writable_and_truncated_content(self) -> None:
        payload = minidump_fixture()
        wrong = "0" * 64
        with tempfile.TemporaryDirectory() as directory:
            path = self._archive(directory, payload, wrong)
            mismatch = wer.validate_content_addressed_archive(
                path, expected_sha256="1" * 64, expected_pid=12324
            )
            self.assertFalse(mismatch["valid"])
            self.assertIn(
                "archive filename hash does not match file content", mismatch["reasons"]
            )
            self.assertIn(
                "archive SHA-256 does not match expected hash", mismatch["reasons"]
            )

            path.chmod(0o644)
            writable = wer.validate_content_addressed_archive(path, expected_pid=12324)
            self.assertFalse(writable["valid"])
            self.assertIn("archive file is writable", writable["reasons"])

            truncated = payload[:64]
            truncated_path = self._archive(directory, truncated)
            broken = wer.validate_content_addressed_archive(
                truncated_path, expected_pid=12324
            )
            self.assertFalse(broken["valid"])
            self.assertIn("archived minidump is structurally invalid", broken["reasons"])

    def test_archive_detects_path_identity_changing_during_validation(self) -> None:
        payload = minidump_fixture()
        with tempfile.TemporaryDirectory() as directory:
            path = self._archive(directory, payload)
            original = os.lstat(path)
            changed = SimpleNamespace(
                st_dev=original.st_dev,
                st_ino=original.st_ino,
                st_mode=original.st_mode,
                st_size=original.st_size,
                st_mtime_ns=original.st_mtime_ns + 1,
                st_ctime_ns=original.st_ctime_ns,
            )
            snapshots = iter((original, changed))
            report = wer.validate_content_addressed_archive(
                path,
                expected_pid=12324,
                _stat_provider=lambda _: next(snapshots),
            )
        self.assertFalse(report["valid"])
        self.assertIn(
            "archive path changed while it was being validated", report["reasons"]
        )


class WorkflowTests(unittest.TestCase):
    def test_check_is_read_only_against_synthetic_configured_state(self) -> None:
        guest = FakeTransport()
        guest.configured(files=["riseofnations.exe.12324.dmp"])
        report = wer.check_report(wer.inspect_state(guest, "ember"))
        self.assertTrue(report["ok"])
        self.assertEqual(guest.mutations, [])

    def test_setup_writes_and_round_trips_both_registry_views(self) -> None:
        guest = FakeTransport()
        result = wer.setup(guest, "ember")
        self.assertTrue(result["ok"])
        self.assertEqual(guest.registry["64"], exact_values())
        self.assertEqual(guest.registry["32"], exact_values())
        self.assertTrue(guest.folder_exists)
        self.assertEqual(guest.folder_names, [])
        self.assertTrue(wer._acl_is_exact(guest.acl, USER_SID))
        self.assertTrue(any("/reg:64" in command for command in guest.mutations))
        self.assertTrue(any("/reg:32" in command for command in guest.mutations))

    def test_setup_refuses_a_running_process_before_mutation(self) -> None:
        guest = FakeTransport()
        guest.tasklist = CAPTURED_RUNNING
        with self.assertRaisesRegex(wer.WorkflowError, "is running"):
            wer.setup(guest, "ember")
        self.assertEqual(guest.mutations, [])

    def test_setup_blocks_on_tasklist_command_failure_without_mutation(self) -> None:
        guest = FakeTransport()
        guest.tasklist_returncode = 1
        guest.tasklist = "ERROR: Access is denied."
        with self.assertRaisesRegex(wer.WorkflowError, "process state is unknown"):
            wer.setup(guest, "ember")
        self.assertEqual(guest.mutations, [])

    def test_setup_blocks_on_malformed_tasklist_without_mutation(self) -> None:
        guest = FakeTransport()
        guest.tasklist = ""
        with self.assertRaisesRegex(wer.WorkflowError, "no parseable process state"):
            wer.setup(guest, "ember")
        self.assertEqual(guest.mutations, [])

    def test_setup_blocks_on_registry_access_error_without_mutation(self) -> None:
        guest = FakeTransport()
        guest.registry_query_error["64"] = (1, "ERROR: Access is denied.")
        with self.assertRaisesRegex(wer.WorkflowError, "key state is unknown"):
            wer.setup(guest, "ember")
        self.assertEqual(guest.mutations, [])

    def test_setup_blocks_on_folder_probe_error_without_mutation(self) -> None:
        guest = FakeTransport()
        guest.folder_probe_error = (1, "Get-ChildItem: Access is denied.")
        with self.assertRaisesRegex(wer.WorkflowError, "dump-directory probe failed"):
            wer.setup(guest, "ember")
        self.assertEqual(guest.mutations, [])

    def test_remove_blocks_on_acl_probe_error_without_mutation(self) -> None:
        guest = FakeTransport()
        guest.configured()
        guest.acl_probe_error = (1, "Get-Acl: Access is denied.")
        with self.assertRaisesRegex(wer.WorkflowError, "ACL probe failed"):
            wer.remove(guest, "ember")
        self.assertEqual(guest.mutations, [])

    def test_lost_mkdir_race_never_rmdirs_the_unowned_directory(self) -> None:
        guest = FakeTransport()
        guest.mkdir_race = True
        with self.assertRaisesRegex(wer.WorkflowError, "another actor created it"):
            wer.setup(guest, "ember")
        self.assertTrue(guest.folder_exists)
        self.assertFalse(any(command.startswith("rmdir ") for command in guest.mutations))
        self.assertEqual(
            guest.mutations,
            [f'mkdir "{wer.DUMP_DIR}"'],
        )

    def test_setup_rolls_back_partial_registry_and_only_rmdirs_empty_folder(self) -> None:
        guest = FakeTransport()
        guest.fail_once = '/v DumpType /t REG_DWORD /d "2" /f /reg:32'
        with self.assertRaises(wer.WorkflowError):
            wer.setup(guest, "ember")
        self.assertIsNone(guest.registry["64"])
        self.assertIsNone(guest.registry["32"])
        self.assertFalse(guest.folder_exists)
        rmdirs = [command for command in guest.mutations if command.startswith("rmdir ")]
        self.assertEqual(rmdirs, [f'rmdir "{wer.DUMP_DIR}"'])
        self.assertFalse(any("rmdir /s" in command.casefold() for command in guest.mutations))
        self.assertFalse(any(command.casefold().startswith("del ") for command in guest.mutations))

    def test_remove_preserves_every_dump_and_only_deletes_exact_keys(self) -> None:
        guest = FakeTransport()
        retained = ["riseofnations.exe.12324.dmp"]
        guest.configured(files=retained)
        result = wer.remove(guest, "ember")
        self.assertTrue(result["ok"])
        self.assertIsNone(guest.registry["64"])
        self.assertIsNone(guest.registry["32"])
        self.assertTrue(guest.folder_exists)
        self.assertEqual(guest.folder_names, retained)
        self.assertEqual(result["retained_items"], retained)
        self.assertFalse(any(command.startswith("rmdir ") for command in guest.mutations))

    def test_remove_refuses_non_owned_values_without_mutation(self) -> None:
        guest = FakeTransport()
        guest.configured()
        guest.registry["32"]["DumpCount"]["value"] = 10
        with self.assertRaisesRegex(wer.WorkflowError, "not exactly tool-owned"):
            wer.remove(guest, "ember")
        self.assertEqual(guest.mutations, [])

    def test_remove_restores_first_view_if_second_delete_fails(self) -> None:
        guest = FakeTransport()
        guest.configured()
        guest.fail_once = f'reg.exe delete "{wer.WER_KEY}" /f /reg:32'
        with self.assertRaises(wer.WorkflowError):
            wer.remove(guest, "ember")
        self.assertEqual(guest.registry["64"], exact_values())
        self.assertEqual(guest.registry["32"], exact_values())
        self.assertTrue(guest.folder_exists)

    def test_verify_hashes_a_stable_nonzero_mdmp_and_returns_events(self) -> None:
        guest = FakeTransport()
        path = wer.DUMP_DIR + r"\riseofnations.exe.12324.dmp"
        record = {
            "path": path,
            "name": "riseofnations.exe.12324.dmp",
            "length": 1_048_576,
            "last_write_utc": "2026-08-09T03:10:00.0000000Z",
        }
        guest.dump_snapshots = [[copy.deepcopy(record)], [copy.deepcopy(record)]]
        guest.events = [
            {
                "time_created_utc": "2026-08-09T03:10:01.0000000Z",
                "id": 1000,
                "provider": "Application Error",
                "record_id": 99,
                "message": "Faulting application name: riseofnations.exe",
            }
        ]
        result = wer.verify(guest, "2026-08-09T03:00:00Z", stable_wait_seconds=0)
        self.assertTrue(result["ok"])
        self.assertEqual(result["dumps"][0]["magic"], "MDMP")
        self.assertEqual(result["dumps"][0]["sha256"], "a" * 64)
        self.assertEqual(result["application_events"][0]["id"], 1000)
        self.assertFalse(result["copied_dumps"])
        self.assertEqual(guest.mutations, [])

    def test_verify_rejects_a_growing_dump_without_hashing_it(self) -> None:
        guest = FakeTransport()
        path = wer.DUMP_DIR + r"\riseofnations.exe.12324.dmp"
        first = {
            "path": path,
            "name": "riseofnations.exe.12324.dmp",
            "length": 100,
            "last_write_utc": "2026-08-09T03:10:00Z",
        }
        second = {**first, "length": 200, "last_write_utc": "2026-08-09T03:10:01Z"}
        guest.dump_snapshots = [[first], [second]]
        result = wer.verify(guest, "2026-08-09T03:00:00Z", stable_wait_seconds=0)
        self.assertFalse(result["ok"])
        self.assertIn("dump length changed during stability window", result["dumps"][0]["reasons"])
        self.assertFalse(any("[System.IO.File]::Open" in command for command in guest.reads))
        self.assertEqual(guest.mutations, [])


if __name__ == "__main__":
    unittest.main()
