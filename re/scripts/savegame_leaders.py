#!/usr/bin/env python3
"""Parse the retail ``Leaders::walk_data`` save-stream section.

This deliberately starts at the Leaders section and stops at its exact end.  It
does not attempt to discover the section inside a save and it does not parse the
following ``Types::walk_data`` owner.  The byte grammar is recovered from:

* ``Leaders::walk_data`` 0x006e38e0;
* ``LeaderData::walk_data`` 0x006d6750;
* ``Array<Site>::walk_data`` 0x0047cee0;
* ``Array<MakeObject>::walk_data`` 0x0047d440;
* ``SimpleArray<int>::walk_data`` 0x00473120; and
* ``LeaderDataEncrypt::walk_data`` 0x006d9900.

The parser returns only derived metadata, spans, and hashes.  It never embeds or
writes specimen bytes.
"""

from __future__ import annotations

import argparse
import dataclasses
import gzip
import hashlib
import json
import pathlib
import struct
from typing import Sequence


TAG_LEADERS = 0x20
TAG_LEADER_DATA = 0xEE
LEADER_COUNT = 8
LEADER_FIXED_BODY_SIZE = 0x6922  # LeaderData +0x08 .. +0x692a
DIPLOMACY_COUNT = 8
DIPLOMACY_SIZE = 0x5C
PERSONALITY_SIZE = 0x60
ENCRYPTED_WORD_COUNT = 62
MAX_DYNAMIC_LENGTH = 1 << 20

PREFIX_BUFFERS = (
    "tech",
    "tech_at_start",
    "obs_flags",
    "conquest_wonders",
    "conquest_wonders_in_game",
    "conquest_racial_powers",
)
SUFFIX_BUFFERS = ("rare", "rare_owned", "rare_conquest")


def _encrypted_word_names() -> tuple[str, ...]:
    names: list[str] = []
    for i in range(6):
        names.extend(
            (
                f"bucket[{i}]",
                f"leftover[{i}]",
                f"resource_cap[{i}]",
                f"over_cap[{i}]",
                f"resources[{i}]",
                f"support[{i}]",
                f"income[{i}]",
                f"rate[{i}]",
                f"bonus[{i}]",
            )
        )
    names.append("resource_cap[6]")
    names.extend(f"epoch[{i}]" for i in range(4))
    names.extend(("ages", "epochs", "discovered"))
    assert len(names) == ENCRYPTED_WORD_COUNT
    return tuple(names)


ENCRYPTED_WORD_NAMES = _encrypted_word_names()


class LeadersParseError(ValueError):
    """The section contradicts the recovered retail traversal."""


@dataclasses.dataclass(frozen=True)
class Span:
    offset: int
    end: int
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class VariableBuffer:
    name: str
    offset: int
    end: int
    bits: int
    size: int
    payload_sha256: str


@dataclasses.dataclass(frozen=True)
class ArrayImage:
    name: str
    offset: int
    end: int
    length: int
    capacity: int | None
    increment: int | None
    flags: int | None
    element_size: int
    payload_sha256: str


@dataclasses.dataclass(frozen=True)
class LeaderRecord:
    index: int
    offset: int
    end: int
    tag: int
    leader_flags: int
    leader_flags2: int
    active: bool
    who: int | None
    tribe: int | None
    fixed_body: Span | None
    diplomacy: Span | None
    personality: Span | None
    prefix_buffers: tuple[VariableBuffer, ...]
    arrays: tuple[ArrayImage, ...]
    prod_script: str | None
    suffix_buffers: tuple[VariableBuffer, ...]
    encrypted_offset: int | None
    encrypted_end: int | None
    encrypted_values: tuple[tuple[str, int], ...]
    sha256: str


@dataclasses.dataclass(frozen=True)
class LeadersSection:
    offset: int
    end: int
    tag: int
    prod_script_path: str
    records: tuple[LeaderRecord, ...]
    sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


class _Reader:
    def __init__(self, data: bytes | bytearray | memoryview, offset: int):
        self.data = memoryview(data)
        if offset < 0 or offset > len(self.data):
            raise LeadersParseError(f"offset {offset:#x} is outside the stream")
        self.pos = offset

    def _take(self, size: int, what: str) -> memoryview:
        if size < 0 or self.pos + size > len(self.data):
            raise LeadersParseError(
                f"{what} range [{self.pos:#x},{self.pos + size:#x}) "
                f"exceeds {len(self.data):#x}-byte stream"
            )
        start = self.pos
        self.pos += size
        return self.data[start : self.pos]

    def u8(self, what: str) -> int:
        return self._take(1, what)[0]

    def i16(self, what: str) -> int:
        return struct.unpack("<h", self._take(2, what))[0]

    def i32(self, what: str) -> int:
        return struct.unpack("<i", self._take(4, what))[0]

    def u32(self, what: str) -> int:
        return struct.unpack("<I", self._take(4, what))[0]

    def wstr(self, what: str) -> str:
        count = self.u32(f"{what}.length")
        if count > MAX_DYNAMIC_LENGTH:
            raise LeadersParseError(f"{what} has absurd UTF-16 length {count}")
        raw = self._take(count * 2, f"{what}.payload")
        try:
            return raw.tobytes().decode("utf-16-le")
        except UnicodeDecodeError as exc:
            raise LeadersParseError(f"{what} is not valid UTF-16LE: {exc}") from exc

    def span(self, start: int, end: int | None = None) -> Span:
        if end is None:
            end = self.pos
        return Span(start, end, _sha(self.data[start:end]))


def _sha(data: memoryview) -> str:
    return hashlib.sha256(data).hexdigest()


def _parse_buffer(reader: _Reader, name: str) -> VariableBuffer:
    start = reader.pos
    bits = reader.i32(f"{name}.bits")
    size = reader.i32(f"{name}.size")
    if bits < 0 or size < 0 or size > MAX_DYNAMIC_LENGTH or bits > size * 8:
        raise LeadersParseError(
            f"{name} has invalid bit-buffer history bits={bits}, size={size} "
            f"at {start:#x}"
        )
    payload = reader._take(size, f"{name}.payload")
    return VariableBuffer(name, start, reader.pos, bits, size, _sha(payload))


def _parse_array(reader: _Reader, name: str, element_size: int) -> ArrayImage:
    start = reader.pos
    length = reader.i32(f"{name}.length")
    if length < 0 or length > MAX_DYNAMIC_LENGTH:
        raise LeadersParseError(f"{name} has invalid length {length} at {start:#x}")
    if length == 0:
        return ArrayImage(name, start, reader.pos, 0, None, None, None, element_size, _sha(memoryview(b"")))

    capacity = reader.i32(f"{name}.capacity")
    increment = reader.i16(f"{name}.increment")
    flags = reader.u8(f"{name}.flags")
    if capacity < length or capacity > MAX_DYNAMIC_LENGTH:
        raise LeadersParseError(
            f"{name} has invalid history length={length}, capacity={capacity} at {start:#x}"
        )
    payload = reader._take(length * element_size, f"{name}.elements")
    return ArrayImage(
        name,
        start,
        reader.pos,
        length,
        capacity,
        increment,
        flags,
        element_size,
        _sha(payload),
    )


def parse_leaders_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    require_tags: bool = True,
) -> LeadersSection:
    """Parse one exact retail Leaders section beginning at ``offset``.

    The returned ``end`` is the first byte owned by the following
    ``Types::walk_data`` call.
    """

    reader = _Reader(data, offset)
    section_tag = reader.u8("Leaders tag")
    if require_tags and section_tag != TAG_LEADERS:
        raise LeadersParseError(
            f"Leaders tag {section_tag:#04x} != {TAG_LEADERS:#04x} at {offset:#x}"
        )
    prod_script_path = reader.wstr("Leaders.prod_script_path")
    records: list[LeaderRecord] = []

    for index in range(LEADER_COUNT):
        start = reader.pos
        tag = reader.u8(f"LeaderData[{index}] tag")
        if require_tags and tag != TAG_LEADER_DATA:
            raise LeadersParseError(
                f"LeaderData[{index}] tag {tag:#04x} != {TAG_LEADER_DATA:#04x} "
                f"at {start:#x}"
            )
        leader_flags = reader.i32(f"LeaderData[{index}].leader_flags")
        leader_flags2 = reader.i32(f"LeaderData[{index}].leader_flags2")
        active = bool(leader_flags & 1)

        who: int | None = None
        tribe: int | None = None
        fixed_body: Span | None = None
        diplomacy: Span | None = None
        personality: Span | None = None
        prefix_buffers: tuple[VariableBuffer, ...] = ()
        arrays: tuple[ArrayImage, ...] = ()
        prod_script: str | None = None
        suffix_buffers: tuple[VariableBuffer, ...] = ()
        encrypted_offset: int | None = None
        encrypted_end: int | None = None
        encrypted_values: tuple[tuple[str, int], ...] = ()

        if active:
            body_start = reader.pos
            body = reader._take(LEADER_FIXED_BODY_SIZE, f"LeaderData[{index}] fixed body")
            who, tribe = struct.unpack_from("<ii", body, 0)
            fixed_body = reader.span(body_start)

            diplomacy_start = reader.pos
            reader._take(DIPLOMACY_COUNT * DIPLOMACY_SIZE, f"LeaderData[{index}].dip[8]")
            diplomacy = reader.span(diplomacy_start)

            personality_start = reader.pos
            reader._take(PERSONALITY_SIZE, f"LeaderData[{index}].pers")
            personality = reader.span(personality_start)

            prefix_buffers = tuple(_parse_buffer(reader, name) for name in PREFIX_BUFFERS)

            # The instruction order is significant: make_list is laid out after
            # prod_script in memory, but its Array walk immediately follows sites.
            arrays = (
                _parse_array(reader, "sites", 0x18),
                _parse_array(reader, "make_list", 0x28),
                _parse_array(reader, "mil_trainers", 4),
                _parse_array(reader, "new_rares", 4),
                _parse_array(reader, "oil_patches", 4),
            )
            prod_script = reader.wstr(f"LeaderData[{index}].prod_script")
            suffix_buffers = tuple(_parse_buffer(reader, name) for name in SUFFIX_BUFFERS)

            # data_encrypted is a pointer in LeaderData.  Its pointee is the final
            # deferred child and writes 62 already-deobfuscated u32 values.
            encrypted_offset = reader.pos
            values = tuple(
                (name, reader.i32(f"LeaderData[{index}].data_encrypted.{name}"))
                for name in ENCRYPTED_WORD_NAMES
            )
            encrypted_end = reader.pos
            encrypted_values = values

        end = reader.pos
        records.append(
            LeaderRecord(
                index=index,
                offset=start,
                end=end,
                tag=tag,
                leader_flags=leader_flags,
                leader_flags2=leader_flags2,
                active=active,
                who=who,
                tribe=tribe,
                fixed_body=fixed_body,
                diplomacy=diplomacy,
                personality=personality,
                prefix_buffers=prefix_buffers,
                arrays=arrays,
                prod_script=prod_script,
                suffix_buffers=suffix_buffers,
                encrypted_offset=encrypted_offset,
                encrypted_end=encrypted_end,
                encrypted_values=encrypted_values,
                sha256=_sha(reader.data[start:end]),
            )
        )

    end = reader.pos
    return LeadersSection(
        offset=offset,
        end=end,
        tag=section_tag,
        prod_script_path=prod_script_path,
        records=tuple(records),
        sha256=_sha(reader.data[offset:end]),
    )


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes()
    return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def _jsonable(value: object) -> object:
    if dataclasses.is_dataclass(value):
        result = {}
        for field in dataclasses.fields(value):
            field_value = getattr(value, field.name)
            if field.name in ("offset", "end", "encrypted_offset", "encrypted_end"):
                result[field.name] = None if field_value is None else f"0x{field_value:x}"
            else:
                result[field.name] = _jsonable(field_value)
        return result
    if isinstance(value, tuple):
        return [_jsonable(item) for item in value]
    return value


def _summary(section: LeadersSection, path: pathlib.Path) -> str:
    lines = [
        f"{path}: Leaders {section.offset:#x}..{section.end:#x} "
        f"({section.size} bytes) sha256={section.sha256}",
        f"  tag={section.tag:#04x} prod_script_path={section.prod_script_path!r}",
    ]
    for row in section.records:
        line = (
            f"  LeaderData[{row.index}] {row.offset:#x}..{row.end:#x} "
            f"flags={row.leader_flags & 0xffffffff:#010x} "
            f"flags2={row.leader_flags2 & 0xffffffff:#010x}"
        )
        if row.active:
            arrays = ", ".join(f"{item.name}={item.length}/{item.capacity}" for item in row.arrays)
            line += (
                f" who={row.who} tribe={row.tribe} prod_script={row.prod_script!r} "
                f"arrays[{arrays}]"
            )
        lines.append(line)
    lines.append(f"  next owner begins at {section.end:#x} (Types::walk_data 0x00669780)")
    return "\n".join(lines)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument(
        "--offset",
        required=True,
        type=lambda text: int(text, 0),
        help="decompressed stream offset of the Leaders tag (for example 0x9a5a)",
    )
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    section = parse_leaders_section(_load(args.file), args.offset)
    if args.json:
        print(json.dumps(_jsonable(section), indent=2))
    else:
        print(_summary(section, args.file))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
