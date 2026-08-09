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
import hashlib
import json
import os
from pathlib import Path, PureWindowsPath
import re
import stat
import struct
import subprocess
import sys
import time
from typing import Any, BinaryIO, Callable


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

# MINIDUMP_HEADER and MINIDUMP_STREAM_TYPE values from minidumpapiset.h.  WER
# DumpType=2 promises a full dump, so verification requires both the header flag
# and the Memory64 stream that carries its process-memory ranges.
MINIDUMP_SIGNATURE = 0x504D444D
MINIDUMP_VERSION = 0xA793
MINIDUMP_WITH_FULL_MEMORY = 0x00000002
MINIDUMP_STREAM_NAMES = {
    3: "ThreadListStream",
    4: "ModuleListStream",
    5: "MemoryListStream",
    6: "ExceptionStream",
    7: "SystemInfoStream",
    9: "Memory64ListStream",
    15: "MiscInfoStream",
}
REQUIRED_MINIDUMP_STREAMS = frozenset((3, 4, 6, 7, 15))
MINIDUMP_MISC1_PROCESS_ID = 0x00000001
MAX_MINIDUMP_STREAMS = 65_536
MAX_MINIDUMP_RECORDS = 4_000_000
ARCHIVE_NAME_RE = re.compile(r"^(?P<sha256>[0-9a-f]{64})\.dmp$")

SYSTEM_SID = "S-1-5-18"
ADMINISTRATORS_SID = "S-1-5-32-544"
FULL_CONTROL = 2_032_127
# `icacls (M)` persists FILE_GENERIC_* plus SYNCHRONIZE.  Get-Acl therefore
# reports 0x1301BF, not the bare .NET FileSystemRights.Modify value 0x301BF.
MODIFY = 1_245_631
OBJECT_AND_CONTAINER_INHERIT = 3


class WorkflowError(RuntimeError):
    """A fail-closed precondition, mutation, or round-trip failure."""


class _MinidumpFormatError(ValueError):
    """An invalid offset, count, or fixed-layout record in an offline dump."""


class _RandomAccessReader:
    """Small bounds-checking adapter; never materialises a full-memory dump."""

    def __init__(self, source: bytes | bytearray | memoryview | BinaryIO) -> None:
        if isinstance(source, (bytes, bytearray, memoryview)):
            self._bytes = memoryview(source)
            self._file: BinaryIO | None = None
            self.size = len(self._bytes)
            return
        if not hasattr(source, "read") or not hasattr(source, "seek"):
            raise TypeError("minidump source must be bytes or a seekable binary file")
        self._bytes = None
        self._file = source
        try:
            position = source.tell()
            source.seek(0, os.SEEK_END)
            self.size = source.tell()
            source.seek(position)
        except (OSError, ValueError) as error:
            raise _MinidumpFormatError(f"minidump is not seekable: {error}") from error

    def read(self, offset: int, size: int, context: str) -> bytes:
        if offset < 0 or size < 0 or offset > self.size or size > self.size - offset:
            raise _MinidumpFormatError(
                f"{context} range is outside the file: offset={offset} size={size} "
                f"file_size={self.size}"
            )
        if self._bytes is not None:
            return self._bytes[offset : offset + size].tobytes()
        assert self._file is not None
        try:
            position = self._file.tell()
            self._file.seek(offset)
            data = self._file.read(size)
            self._file.seek(position)
        except (OSError, ValueError) as error:
            raise _MinidumpFormatError(f"could not read {context}: {error}") from error
        if len(data) != size:
            raise _MinidumpFormatError(
                f"short read for {context}: wanted={size} received={len(data)}"
            )
        return data

    def unpack(self, fmt: str, offset: int, context: str) -> tuple[Any, ...]:
        size = struct.calcsize(fmt)
        return struct.unpack(fmt, self.read(offset, size, context))


def _checked_count(
    count: int, record_size: int, available: int, context: str
) -> int:
    if count < 0 or count > MAX_MINIDUMP_RECORDS:
        raise _MinidumpFormatError(f"implausible {context} count: {count}")
    required = count * record_size
    if required > available:
        raise _MinidumpFormatError(
            f"truncated {context}: count={count} record_size={record_size} "
            f"available={available}"
        )
    return required


def _range_covered(start: int, size: int, ranges: list[tuple[int, int]]) -> bool:
    if size <= 0:
        return False
    end = start + size
    if end <= start or end > 1 << 64:
        return False
    cursor = start
    for range_start, range_size in sorted(ranges):
        range_end = range_start + range_size
        if range_end <= cursor:
            continue
        if range_start > cursor:
            return False
        cursor = range_end
        if cursor >= end:
            return True
    return False


def _read_minidump_string(reader: _RandomAccessReader, rva: int, context: str) -> str:
    (byte_length,) = reader.unpack("<I", rva, context + " length")
    if byte_length % 2:
        raise _MinidumpFormatError(f"{context} has an odd UTF-16 byte length")
    raw = reader.read(rva + 4, byte_length, context)
    try:
        return raw.decode("utf-16-le")
    except UnicodeDecodeError as error:
        raise _MinidumpFormatError(f"{context} is not valid UTF-16LE") from error


def validate_minidump(
    source: bytes | bytearray | memoryview | BinaryIO,
    *,
    expected_pid: int | None = None,
    expected_process_create_time: int | None = None,
    expected_process_name: str | None = PROCESS_NAME,
    expected_module_timestamp: int | None = None,
    expected_module_size: int | None = None,
    require_full_memory: bool = DUMP_TYPE == 2,
) -> dict[str, Any]:
    """Parse and validate a minidump without loading or debugging its process.

    The result is deliberately data-only so the same contract can be emitted by
    the guest-side single-handle probe and validated by offline archive tooling.
    """

    reasons: list[str] = []
    report: dict[str, Any] = {
        "valid": False,
        "reasons": reasons,
        "required_full_memory": bool(require_full_memory),
    }

    def reject(reason: str) -> None:
        if reason not in reasons:
            reasons.append(reason)

    try:
        reader = _RandomAccessReader(source)
        report["file_size"] = reader.size
        if reader.size < 32:
            raise _MinidumpFormatError("truncated MINIDUMP_HEADER")
        (
            signature,
            version,
            stream_count,
            directory_rva,
            checksum,
            timestamp,
            flags,
        ) = reader.unpack("<IIIIIIQ", 0, "MINIDUMP_HEADER")
        report["header"] = {
            "signature": signature,
            "version": version,
            "stream_count": stream_count,
            "directory_rva": directory_rva,
            "checksum": checksum,
            "timestamp": timestamp,
            "flags": flags,
        }
        if signature != MINIDUMP_SIGNATURE:
            reject("missing MDMP signature")
        if version & 0xFFFF != MINIDUMP_VERSION:
            reject("unsupported minidump version")
        if not 0 < stream_count <= MAX_MINIDUMP_STREAMS:
            raise _MinidumpFormatError(
                f"implausible minidump stream count: {stream_count}"
            )
        reader.read(
            directory_rva,
            stream_count * 12,
            "MINIDUMP_DIRECTORY array",
        )

        streams: dict[int, tuple[int, int]] = {}
        stream_records: list[dict[str, Any]] = []
        for index in range(stream_count):
            stream_type, data_size, rva = reader.unpack(
                "<III", directory_rva + index * 12, f"stream directory {index}"
            )
            reader.read(rva, data_size, f"stream {stream_type}")
            stream_records.append(
                {
                    "type": stream_type,
                    "name": MINIDUMP_STREAM_NAMES.get(
                        stream_type, f"StreamType{stream_type}"
                    ),
                    "size": data_size,
                    "rva": rva,
                }
            )
            if stream_type in streams:
                reject(f"duplicate {MINIDUMP_STREAM_NAMES.get(stream_type, stream_type)}")
            else:
                streams[stream_type] = (data_size, rva)
        report["streams"] = stream_records
        for required in sorted(REQUIRED_MINIDUMP_STREAMS):
            if required not in streams:
                reject(f"missing required {MINIDUMP_STREAM_NAMES[required]}")
        if 5 not in streams and 9 not in streams:
            reject("missing required Memory64ListStream/MemoryListStream")
        if require_full_memory:
            if not flags & MINIDUMP_WITH_FULL_MEMORY:
                reject("full-memory dump flag is absent")
            if 9 not in streams:
                reject("full-memory contract requires Memory64ListStream")

        if 7 in streams:
            size, rva = streams[7]
            if size < 56:
                reject("truncated SystemInfoStream")
            else:
                architecture, level, revision, processors, product_type = reader.unpack(
                    "<HHHBB", rva, "SystemInfoStream"
                )
                major, minor, build, platform = reader.unpack(
                    "<IIII", rva + 8, "SystemInfoStream versions"
                )
                report["system_info"] = {
                    "processor_architecture": architecture,
                    "processor_level": level,
                    "processor_revision": revision,
                    "number_of_processors": processors,
                    "product_type": product_type,
                    "major_version": major,
                    "minor_version": minor,
                    "build_number": build,
                    "platform_id": platform,
                }

        process_id: int | None = None
        process_create_time: int | None = None
        if 15 in streams:
            size, rva = streams[15]
            if size < 12:
                reject("truncated MiscInfoStream")
            else:
                size_of_info, misc_flags, misc_pid = reader.unpack(
                    "<III", rva, "MiscInfoStream"
                )
                if size_of_info < 12 or size_of_info > size:
                    reject("invalid MiscInfoStream SizeOfInfo")
                if misc_flags & MINIDUMP_MISC1_PROCESS_ID:
                    process_id = misc_pid
                if size >= 16 and size_of_info >= 16:
                    (process_create_time,) = reader.unpack(
                        "<I", rva + 12, "MiscInfoStream ProcessCreateTime"
                    )
                if expected_pid is not None:
                    if process_id is None:
                        reject("MiscInfoStream does not bind a process ID")
                    elif process_id != expected_pid:
                        reject(
                            f"dump process ID {process_id} does not match expected PID "
                            f"{expected_pid}"
                        )
                if expected_process_create_time is not None:
                    if process_create_time is None:
                        reject("MiscInfoStream does not bind a process creation time")
                    elif process_create_time != expected_process_create_time:
                        reject(
                            "dump process creation time does not match expected attempt"
                        )
                report["misc_info"] = {
                    "size_of_info": size_of_info,
                    "flags": misc_flags,
                    "process_id": process_id,
                    "process_create_time": process_create_time,
                }

        modules: list[dict[str, Any]] = []
        if 4 in streams:
            size, rva = streams[4]
            if size < 4:
                reject("truncated ModuleListStream")
            else:
                (count,) = reader.unpack("<I", rva, "ModuleListStream count")
                _checked_count(count, 108, size - 4, "module list")
                for index in range(count):
                    base = rva + 4 + index * 108
                    image_base, image_size, image_checksum, image_timestamp, name_rva = (
                        reader.unpack("<QIIII", base, f"module {index}")
                    )
                    name = _read_minidump_string(
                        reader, name_rva, f"module {index} name"
                    )
                    modules.append(
                        {
                            "base": image_base,
                            "size": image_size,
                            "checksum": image_checksum,
                            "timestamp": image_timestamp,
                            "name": name,
                        }
                    )
        report["modules"] = modules
        target_module: dict[str, Any] | None = None
        if expected_process_name is not None:
            matches = [
                module
                for module in modules
                if PureWindowsPath(str(module["name"])).name.casefold()
                == expected_process_name.casefold()
            ]
            if len(matches) != 1:
                reject(
                    f"ModuleListStream does not contain exactly one "
                    f"{expected_process_name} module"
                )
            else:
                target_module = matches[0]
                if (
                    expected_module_timestamp is not None
                    and target_module["timestamp"] != expected_module_timestamp
                ):
                    reject("target module timestamp does not match expected identity")
                if (
                    expected_module_size is not None
                    and target_module["size"] != expected_module_size
                ):
                    reject("target module image size does not match expected identity")
        report["target_module"] = target_module

        threads: dict[int, dict[str, Any]] = {}
        if 3 in streams:
            size, rva = streams[3]
            if size < 4:
                reject("truncated ThreadListStream")
            else:
                (count,) = reader.unpack("<I", rva, "ThreadListStream count")
                _checked_count(count, 48, size - 4, "thread list")
                for index in range(count):
                    base = rva + 4 + index * 48
                    (
                        thread_id,
                        suspend_count,
                        priority_class,
                        priority,
                        teb,
                        stack_start,
                        stack_size,
                        stack_rva,
                        context_size,
                        context_rva,
                    ) = reader.unpack("<IIIIQQIIII", base, f"thread {index}")
                    if thread_id in threads:
                        reject(f"duplicate thread ID {thread_id}")
                    if stack_size == 0:
                        reject(f"thread {thread_id} has an empty stack descriptor")
                    else:
                        reader.read(stack_rva, stack_size, f"thread {thread_id} stack")
                    if context_size == 0:
                        reject(f"thread {thread_id} has an empty context descriptor")
                    else:
                        reader.read(
                            context_rva,
                            context_size,
                            f"thread {thread_id} context",
                        )
                    threads[thread_id] = {
                        "thread_id": thread_id,
                        "suspend_count": suspend_count,
                        "priority_class": priority_class,
                        "priority": priority,
                        "teb": teb,
                        "stack_start": stack_start,
                        "stack_size": stack_size,
                        "stack_rva": stack_rva,
                        "context_size": context_size,
                        "context_rva": context_rva,
                    }
        report["threads"] = list(threads.values())

        exception: dict[str, Any] | None = None
        if 6 in streams:
            size, rva = streams[6]
            if size < 168:
                reject("truncated ExceptionStream")
            else:
                thread_id, alignment, code, exception_flags = reader.unpack(
                    "<IIII", rva, "ExceptionStream"
                )
                exception_record, exception_address = reader.unpack(
                    "<QQ", rva + 16, "ExceptionStream record"
                )
                parameter_count = reader.unpack(
                    "<I", rva + 32, "ExceptionStream parameter count"
                )[0]
                context_size, context_rva = reader.unpack(
                    "<II", rva + 160, "ExceptionStream context"
                )
                if alignment != 0:
                    reject("ExceptionStream alignment field is nonzero")
                if parameter_count > 15:
                    reject("ExceptionStream has too many exception parameters")
                if context_size == 0:
                    reject("ExceptionStream has an empty context descriptor")
                else:
                    reader.read(context_rva, context_size, "exception thread context")
                exception = {
                    "thread_id": thread_id,
                    "code": code,
                    "flags": exception_flags,
                    "record": exception_record,
                    "address": exception_address,
                    "parameter_count": parameter_count,
                    "context_size": context_size,
                    "context_rva": context_rva,
                }
                if thread_id not in threads:
                    reject("ExceptionStream thread is absent from ThreadListStream")
        report["exception"] = exception

        memory_ranges: list[tuple[int, int]] = []
        memory_kind: str | None = None
        if 9 in streams:
            size, rva = streams[9]
            if size < 16:
                reject("truncated Memory64ListStream")
            else:
                count, base_rva = reader.unpack("<QQ", rva, "Memory64ListStream")
                _checked_count(count, 16, size - 16, "Memory64 range list")
                total = 0
                previous_end = 0
                for index in range(count):
                    start, range_size = reader.unpack(
                        "<QQ", rva + 16 + index * 16, f"Memory64 range {index}"
                    )
                    if range_size == 0 or start + range_size > 1 << 64:
                        reject(f"invalid Memory64 range {index}")
                    if index and start < previous_end:
                        reject("Memory64 ranges overlap or are out of order")
                    previous_end = start + range_size
                    total += range_size
                    if total > reader.size:
                        reject("Memory64 payload exceeds the dump length")
                        break
                    memory_ranges.append((start, range_size))
                reader.read(base_rva, total, "Memory64 payload")
                report["memory64"] = {
                    "range_count": count,
                    "base_rva": base_rva,
                    "payload_size": total,
                }
                memory_kind = "Memory64ListStream"
        if 5 in streams:
            size, rva = streams[5]
            if size < 4:
                reject("truncated MemoryListStream")
            else:
                (count,) = reader.unpack("<I", rva, "MemoryListStream count")
                _checked_count(count, 16, size - 4, "memory range list")
                list_ranges: list[tuple[int, int]] = []
                for index in range(count):
                    start, range_size, data_rva = reader.unpack(
                        "<QII", rva + 4 + index * 16, f"memory range {index}"
                    )
                    if range_size == 0:
                        reject(f"invalid MemoryList range {index}")
                    else:
                        reader.read(data_rva, range_size, f"memory range {index} payload")
                    list_ranges.append((start, range_size))
                if not memory_ranges:
                    memory_ranges = list_ranges
                    memory_kind = "MemoryListStream"
                report["memory_list"] = {"range_count": count}
        report["memory_kind"] = memory_kind
        report["memory_ranges"] = [
            {"start": start, "size": size} for start, size in memory_ranges
        ]

        exception_thread = (
            threads.get(int(exception["thread_id"])) if exception is not None else None
        )
        stack_covered = bool(
            exception_thread
            and _range_covered(
                int(exception_thread["stack_start"]),
                int(exception_thread["stack_size"]),
                memory_ranges,
            )
        )
        address_covered = bool(
            exception
            and _range_covered(int(exception["address"]), 1, memory_ranges)
        )
        report["exception_coverage"] = {
            "thread_present": exception_thread is not None,
            "context_present": bool(exception and exception["context_size"]),
            "stack_covered": stack_covered,
            "address_covered": address_covered,
        }
        if exception_thread is not None and not stack_covered:
            reject("exception thread stack is not covered by a memory stream")
        if require_full_memory and exception is not None and not address_covered:
            reject("exception address is not covered by full-memory ranges")
    except (_MinidumpFormatError, struct.error, OverflowError) as error:
        reject(str(error))

    report["valid"] = not reasons
    return report


def _archive_stat_fingerprint(value: os.stat_result | Any) -> tuple[int, ...]:
    return (
        int(getattr(value, "st_dev", 0)),
        int(getattr(value, "st_ino", 0)),
        int(getattr(value, "st_mode", 0)),
        int(getattr(value, "st_size", -1)),
        int(getattr(value, "st_mtime_ns", int(getattr(value, "st_mtime", 0) * 1e9))),
        int(getattr(value, "st_ctime_ns", int(getattr(value, "st_ctime", 0) * 1e9))),
    )


def _archive_is_read_only(value: os.stat_result | Any) -> bool:
    attributes = getattr(value, "st_file_attributes", None)
    if attributes is not None and hasattr(stat, "FILE_ATTRIBUTE_READONLY"):
        return bool(attributes & stat.FILE_ATTRIBUTE_READONLY)
    return int(getattr(value, "st_mode", 0)) & 0o222 == 0


def validate_content_addressed_archive(
    path: str | os.PathLike[str],
    *,
    expected_sha256: str | None = None,
    expected_length: int | None = None,
    expected_pid: int | None = None,
    expected_process_create_time: int | None = None,
    expected_process_name: str | None = PROCESS_NAME,
    expected_module_timestamp: int | None = None,
    expected_module_size: int | None = None,
    require_full_memory: bool = DUMP_TYPE == 2,
    require_read_only: bool = True,
    _stat_provider: Callable[[Path], os.stat_result | Any] | None = None,
) -> dict[str, Any]:
    """Validate a local immutable ``<sha256>.dmp`` archive in one open-file epoch.

    The private stat-provider hook exists solely to make replacement/change races
    deterministic in offline tests; production callers use ``os.lstat``.
    """

    archive = Path(path)
    reasons: list[str] = []
    report: dict[str, Any] = {
        "schema": "don.wer-minidump-archive-validation.v1",
        "path": str(archive),
        "valid": False,
        "reasons": reasons,
    }

    def reject(reason: str) -> None:
        if reason not in reasons:
            reasons.append(reason)

    stat_provider = _stat_provider or os.lstat
    name_match = ARCHIVE_NAME_RE.fullmatch(archive.name)
    if name_match is None:
        reject("archive filename is not canonical <sha256>.dmp")
        addressed_sha256 = ""
    else:
        addressed_sha256 = name_match.group("sha256")
    if expected_sha256 is not None:
        expected_sha256 = expected_sha256.casefold()
        if not re.fullmatch(r"[0-9a-f]{64}", expected_sha256):
            reject("expected archive SHA-256 is malformed")

    try:
        before_path = stat_provider(archive)
        if stat.S_ISLNK(int(getattr(before_path, "st_mode", 0))):
            reject("archive path is a symbolic link")
        if not stat.S_ISREG(int(getattr(before_path, "st_mode", 0))):
            reject("archive path is not a regular file")
        if require_read_only and not _archive_is_read_only(before_path):
            reject("archive file is writable")

        with archive.open("rb") as stream:
            before_fd = os.fstat(stream.fileno())
            if _archive_stat_fingerprint(before_path) != _archive_stat_fingerprint(
                before_fd
            ):
                reject("archive path changed before it was opened")
            digest = hashlib.sha256()
            while True:
                chunk = stream.read(1024 * 1024)
                if not chunk:
                    break
                digest.update(chunk)
            sha256 = digest.hexdigest()
            stream.seek(0)
            dump = validate_minidump(
                stream,
                expected_pid=expected_pid,
                expected_process_create_time=expected_process_create_time,
                expected_process_name=expected_process_name,
                expected_module_timestamp=expected_module_timestamp,
                expected_module_size=expected_module_size,
                require_full_memory=require_full_memory,
            )
            after_fd = os.fstat(stream.fileno())
        after_path = stat_provider(archive)

        report.update(
            {
                "sha256": sha256,
                "content_address": f"sha256:{sha256}",
                "length": int(before_fd.st_size),
                "read_only": _archive_is_read_only(before_path),
                "dump": dump,
            }
        )
        if addressed_sha256 and addressed_sha256 != sha256:
            reject("archive filename hash does not match file content")
        if expected_sha256 is not None and expected_sha256 != sha256:
            reject("archive SHA-256 does not match expected hash")
        if expected_length is not None and int(before_fd.st_size) != expected_length:
            reject("archive length does not match expected length")
        if _archive_stat_fingerprint(before_fd) != _archive_stat_fingerprint(after_fd):
            reject("archive changed while it was being validated")
        if _archive_stat_fingerprint(before_path) != _archive_stat_fingerprint(after_path):
            reject("archive path changed while it was being validated")
        if not dump["valid"]:
            reject("archived minidump is structurally invalid")
    except (OSError, ValueError) as error:
        reject(f"archive validation failed: {error}")

    report["valid"] = not reasons
    return report


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
