#!/usr/bin/env python3
"""Audit and extract the repository's static Rules-channel input assets.

The extractor never reads a process.  It consumes the existing repository-relative
``DONTYPE1`` capture, selects the one most-derived object for each of the 806 global
type ids, and rebuilds ObjectType's two pointed-to u16 caches from the captured rule
fields.  Emitted images are privacy-safe normalized walker inputs: bytes the walker
does not visit are zero, including vtables, heap pointers, and String state.

``--check-types`` is the independently useful green gate.  ``--check`` is the full P0
gate and deliberately remains red until a narrow ``donject peek`` of the 24 contiguous
Tribe records is supplied with ``--tribes-capture``.  The raw dump is only an input:
the parser retains the two ranges visited by ``Tribe::walk_rules_data`` and zeroes every
other byte before it returns a record.
"""

from __future__ import annotations

import argparse
import base64
import csv
import hashlib
import json
import re
import struct
import sys
import xml.etree.ElementTree as ET
import zlib
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


TYPE_SLOTS = 806
RECORD_SIZE = 1792
TYPE_CHECKPOINT = 0x72E0C3B6
TYPE_WALKED_BYTES = 473_984
ARRAY_OFFSETS = (0x27C, 0x298)

RULES_BLOCK_BYTES = 0x0D40
RULES_DUPLICATE_OFFSET = 0x0804
CONSTANTS_CHECKPOINT = 0x50625668
CONSTANTS_WALKED_BYTES = RULES_BLOCK_BYTES + 4
BALANCE_SIDE = 493
BALANCE_BYTES = BALANCE_SIDE * BALANCE_SIDE * 2
BALANCE_CHECKPOINT = 0x56DAABC1
TRIBE_COUNT = 24
TRIBE_SIZE = 0x05F0
TRIBE_CAPTURE_BYTES = TRIBE_COUNT * TRIBE_SIZE
TRIBE_RANGES = ((0x54, 0x6C), (0x70, 0x5F0))
TRIBE_WALKED_BYTES = TRIBE_COUNT * sum(end - begin for begin, end in TRIBE_RANGES)
TRIBE_POINTER_RVA = 0x00A7FA34
RULES_CHECKPOINT = 0x12BA3104
RULES_WALKED_BYTES = (
    TYPE_WALKED_BYTES
    + CONSTANTS_WALKED_BYTES
    + BALANCE_BYTES
    + TRIBE_WALKED_BYTES
)
SUPPORTED_EXE_SHA256 = "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079"

# Most-derived registry and retail walker kind in global TypeIndex order.
TYPE_PLAN = (
    (range(0, 50), "GoodType", "Good", 760),
    (range(50, 414), "UnitType", "Unit", 1492),
    (range(414, 543), "BuildType", "Build", 741),
    (range(543, 544), "ItemType", "Object", 636),
    (range(544, 629), "TechType", "Tech", 483),
    (range(629, 684), "SpellType", "Spell", 504),
    (range(684, 806), "BonusType", "Type", 94),
)

# ObjectType::finalize_init_all (0x0065f4a0) calls init_is_list only for ordinary
# UnitTypes 50..401 and BuildTypes 414..542. Goods, 12 Gaia units, and ItemType keep
# their constructed empty arrays, despite also inheriting ObjectType.
ARRAY_INITIALIZED = frozenset((*range(50, 402), *range(414, 543)))


class AssetError(RuntimeError):
    pass


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


@dataclass(frozen=True)
class Block:
    pointers: tuple[int, ...]
    records: tuple[bytes, ...]


@dataclass(frozen=True)
class TypeInput:
    slot: int
    registry: str
    kind: str
    image: bytes


@dataclass(frozen=True)
class U16Array:
    capacity: int
    grow: int
    flags: int
    elements: tuple[int, ...]


@dataclass(frozen=True)
class TribeCapture:
    module_base: int
    address: int
    records: tuple[bytes, ...]
    header: dict[str, str]


def expected_type(slot: int) -> tuple[str, str, int]:
    for slots, registry, kind, minimum in TYPE_PLAN:
        if slot in slots:
            return registry, kind, minimum
    raise AssertionError(slot)


def parse_dontype(path: Path) -> tuple[int, dict[str, Block]]:
    data = path.read_bytes()
    if data[:8] != b"DONTYPE1":
        raise AssetError(f"{path}: expected DONTYPE1 magic")
    module_base, block_count = struct.unpack_from("<II", data, 8)
    if block_count != 10:
        raise AssetError(f"{path}: expected 10 registries, got {block_count}")
    out: dict[str, Block] = {}
    at = 16
    for _ in range(block_count):
        if at + 32 > len(data):
            raise AssetError(f"{path}: truncated registry header at {at}")
        name = data[at : at + 16].split(b"\0", 1)[0].decode("ascii")
        _static_va, count, record_size, _list_ptr = struct.unpack_from("<IIII", data, at + 16)
        if count != TYPE_SLOTS or record_size != RECORD_SIZE:
            raise AssetError(
                f"{path}: {name} shape is {count}x{record_size}, expected 806x1792"
            )
        pointer_at = at + 32
        record_at = pointer_at + count * 4
        end = record_at + count * record_size
        if end > len(data):
            raise AssetError(f"{path}: truncated {name} record bank")
        pointers = struct.unpack_from(f"<{count}I", data, pointer_at)
        records = tuple(
            data[record_at + i * record_size : record_at + (i + 1) * record_size]
            for i in range(count)
        )
        if name in out:
            raise AssetError(f"{path}: duplicate registry {name}")
        out[name] = Block(pointers, records)
        at = end
    if at != len(data):
        raise AssetError(f"{path}: {len(data) - at} unexplained trailing bytes")
    return module_base, out


def select_types(blocks: dict[str, Block]) -> list[TypeInput]:
    selected = []
    for slot in range(TYPE_SLOTS):
        registry, kind, _minimum = expected_type(slot)
        if registry not in blocks:
            raise AssetError(f"missing registry {registry}")
        block = blocks[registry]
        if block.pointers[slot] == 0:
            raise AssetError(f"{registry}[{slot}] is null")
        selected.append(TypeInput(slot, registry, kind, block.records[slot]))
    return selected


def check_typeids(path: Path, selected: list[TypeInput]) -> None:
    with path.open(newline="", encoding="utf-8") as stream:
        rows = list(csv.DictReader(stream, delimiter="\t"))
    if len(rows) != TYPE_SLOTS:
        raise AssetError(f"{path}: expected 806 rows, got {len(rows)}")
    for ty, row in zip(selected, rows):
        if int(row["type_id"]) != ty.slot or row["class"] != ty.registry:
            raise AssetError(
                f"{path}: slot {ty.slot} says {row['type_id']}/{row['class']}, "
                f"expected {ty.slot}/{ty.registry}"
            )


def i32(image: bytes, offset: int) -> int:
    return struct.unpack_from("<i", image, offset)[0]


def u32(image: bytes, offset: int) -> int:
    return struct.unpack_from("<I", image, offset)[0]


def is_slow(types: list[TypeInput], owner: int, target: int, strict: bool) -> bool:
    """Instruction-equivalent ObjectTypeData::is_slow (0x00661ae0)."""
    if owner == target:
        return True
    image = types[owner].image
    if strict:
        if not 50 <= owner <= 413:
            return False
        graft = i32(image, 0x25C)
        return (
            graft == target
            and 50 <= target <= 413
            and u32(types[target].image, 0x2B4) & 0x0100_0000 == 0
        )
    if target < 0 or i32(image, 0x25C) == target:
        return target >= 0
    seen = {owner}
    parent = i32(image, 0x3C)
    while parent >= 0:
        if parent >= TYPE_SLOTS:
            raise AssetError(f"type {owner}: from={parent} is outside TypeIndex")
        if parent in seen:
            raise AssetError(f"type {owner}: cycle in from chain at {parent}")
        if parent == target or i32(types[parent].image, 0x25C) == target:
            return True
        seen.add(parent)
        parent = i32(types[parent].image, 0x3C)
    return False


def read_array_header(image: bytes, offset: int) -> tuple[int, int, int, int]:
    count, capacity = struct.unpack_from("<ii", image, offset + 4)
    grow = struct.unpack_from("<H", image, offset + 12)[0]
    flags = image[offset + 20]
    return count, capacity, grow, flags


def rebuild_arrays(types: list[TypeInput]) -> list[tuple[U16Array, U16Array] | None]:
    result: list[tuple[U16Array, U16Array] | None] = []
    for ty in types:
        if ty.kind not in {"Unit", "Build", "Object", "Good"}:
            result.append(None)
            continue
        arrays = []
        for array_index, strict in enumerate((False, True)):
            elements = (
                tuple(t for t in range(TYPE_SLOTS) if is_slow(types, ty.slot, t, strict))
                if ty.slot in ARRAY_INITIALIZED
                else ()
            )
            count, capacity, grow, flags = read_array_header(
                ty.image, ARRAY_OFFSETS[array_index]
            )
            if count != len(elements):
                raise AssetError(
                    f"type {ty.slot} array {array_index}: capture count {count}, "
                    f"rebuild count {len(elements)}"
                )
            if count and capacity < count:
                raise AssetError(
                    f"type {ty.slot} array {array_index}: capacity {capacity} < count {count}"
                )
            arrays.append(U16Array(capacity, grow, flags, elements))
        result.append((arrays[0], arrays[1]))
    return result


def walked_ranges(kind: str) -> tuple[tuple[int, int], ...]:
    base = ((4, 94),)
    obj = base + ((484, 636),)
    return {
        "Unit": obj + ((692, 716), (724, 732), (732, 736), (736, 1492)),
        "Build": obj + ((692, 741),),
        "Tech": base + ((456, 483),),
        "Object": obj,
        "Spell": base + ((456, 504),),
        "Type": base,
        "Good": obj + ((692, 760),),
    }[kind]


def walk_types(
    types: list[TypeInput], arrays: list[tuple[U16Array, U16Array] | None]
) -> tuple[int, int]:
    adler = 1
    walked = 0

    def update(payload: bytes) -> None:
        nonlocal adler, walked
        adler = zlib.adler32(payload, adler) & 0xFFFF_FFFF
        walked += len(payload)

    for ty, pair in zip(types, arrays):
        ranges = walked_ranges(ty.kind)
        # Object arrays are visited after ObjectType's direct [484,636) range.
        for index, (begin, end) in enumerate(ranges):
            update(ty.image[begin:end])
            if index == 1 and pair is not None:
                for value in pair:
                    update(struct.pack("<i", len(value.elements)))
                    if value.elements:
                        update(
                            struct.pack(
                                "<iHB", value.capacity, value.grow, value.flags & 0xBF
                            )
                        )
                        update(struct.pack(f"<{len(value.elements)}H", *value.elements))
    return adler, walked


def validate_pdb(path: Path) -> None:
    classes = json.loads(path.read_text(encoding="utf-8"))["classes"]

    def field(cls: str, name: str) -> tuple[int, int, str]:
        item = next(x for x in classes[cls]["fields"] if x["name"] == name)
        return item["offset"], item["size"], item["type"]

    expected = {
        ("ObjectTypeData", "is_list"): (636, 28, "SimpleArray<unsigned short>"),
        ("ObjectTypeData", "is_strict_list"): (664, 28, "SimpleArray<unsigned short>"),
        ("Tribe", "graft"): (112, 1408, "enum TypeIndex[352]"),
    }
    for key, value in expected.items():
        actual = field(*key)
        if actual != value:
            raise AssetError(f"{path}: {key} is {actual}, expected {value}")
    if classes["Tribe"]["size"] != 1520:
        raise AssetError(f"{path}: Tribe size is not 0x5f0")


def validate_state_schema(path: Path) -> None:
    classes = json.loads(path.read_text(encoding="utf-8"))["classes"]
    expected = {
        "Type": (460, 90),
        "ObjectType": (696, 242),
        "UnitType": (1496, 792),
        "BuildType": (844, 49),
        "TechType": (648, 117),
        "SpellType": (512, 138),
        "GoodType": (776, 68),
        "Tribe": (1520, 1432),
    }
    for name, shape in expected.items():
        actual = (classes[name]["sizeof"], classes[name]["walked_bytes"])
        if actual != shape:
            raise AssetError(f"{path}: {name} shape is {actual}, expected {shape}")


def validate_manifest(
    path: Path, capture: Path, constants: bytes, balance_path: Path
) -> None:
    manifest = json.loads(path.read_text(encoding="utf-8"))
    if manifest.get("schema_version") != 2:
        raise AssetError(
            f"{path}: schema_version={manifest.get('schema_version')!r}, expected 2"
        )
    types = manifest["types"]
    expected = {
        "slots": TYPE_SLOTS,
        "source_sha256": sha256(capture),
        "pointed_arrays": 1088,
        "pointed_u16_values": 2363,
        "after_types": f"0x{TYPE_CHECKPOINT:08x}",
        "bytes_walked": TYPE_WALKED_BYTES,
    }
    for key, value in expected.items():
        if types.get(key) != value:
            raise AssetError(f"{path}: types.{key}={types.get(key)!r}, expected {value!r}")

    static_expected = {
        "constants": {
            "source_checked_in": True,
            "bytes": RULES_BLOCK_BYTES,
            "block_sha256": hashlib.sha256(constants).hexdigest(),
            "after_constants": f"0x{CONSTANTS_CHECKPOINT:08x}",
            "bytes_walked": CONSTANTS_WALKED_BYTES,
        },
        "balance": {
            "source_checked_in": False,
            "bytes": BALANCE_BYTES,
            "source_sha256": sha256(balance_path),
            "after_balance": f"0x{BALANCE_CHECKPOINT:08x}",
            "bytes_walked": BALANCE_BYTES,
        },
        "tribes": {
            "records_required": TRIBE_COUNT,
            "record_size": TRIBE_SIZE,
            "capture_bytes": TRIBE_CAPTURE_BYTES,
            "pointer_rva": f"0x{TRIBE_POINTER_RVA:08x}",
            "walked_ranges": ["0x54..0x6c", "0x70..0x5f0"],
            "bytes_walked": TRIBE_WALKED_BYTES,
            "after_tribes": f"0x{RULES_CHECKPOINT:08x}",
        },
    }
    for section, values in static_expected.items():
        actual = manifest.get(section, {})
        for key, value in values.items():
            if actual.get(key) != value:
                raise AssetError(
                    f"{path}: {section}.{key}={actual.get(key)!r}, expected {value!r}"
                )


def repository_constants(path: Path) -> bytes:
    text = path.read_text(encoding="ascii")
    encoded = re.findall(r"^BLK=(\S+)$", text, re.MULTILINE)
    if len(encoded) != 1:
        raise AssetError(f"{path}: expected exactly one BLK= payload, got {len(encoded)}")
    try:
        decoded = base64.b64decode(encoded[0], validate=True)
    except ValueError as error:
        raise AssetError(f"{path}: invalid BLK base64: {error}") from error
    if len(decoded) < RULES_BLOCK_BYTES:
        raise AssetError(
            f"{path}: decoded BLK is {len(decoded)} bytes, expected at least {RULES_BLOCK_BYTES}"
        )
    return decoded[:RULES_BLOCK_BYTES]


def repository_balance(path: Path) -> bytes:
    payload = path.read_bytes()
    if len(payload) != BALANCE_BYTES:
        raise AssetError(f"{path}: balance is {len(payload)} bytes, expected {BALANCE_BYTES}")
    return payload


def _header_hex(header: dict[str, str], name: str) -> int:
    try:
        value = header[name]
    except KeyError as error:
        raise AssetError(f"Tribe capture header is missing {name}=") from error
    if value.casefold().startswith("0x"):
        value = value[2:]
    if not value or not re.fullmatch(r"[0-9a-fA-F]+", value):
        raise AssetError(f"Tribe capture header has invalid {name}={header[name]!r}")
    return int(value, 16)


def _optional_header_hex(header: dict[str, str], *names: str) -> int | None:
    for name in names:
        if name in header:
            return _header_hex(header, name)
    return None


def normalize_tribe_records(payload: bytes) -> tuple[bytes, ...]:
    """Retain only bytes visited by Tribe::walk_rules_data (0x006f1270)."""
    if len(payload) != TRIBE_CAPTURE_BYTES:
        raise AssetError(
            f"Tribe capture is {len(payload)} bytes, expected "
            f"{TRIBE_COUNT} * 0x{TRIBE_SIZE:x} = {TRIBE_CAPTURE_BYTES}"
        )
    records = []
    for index in range(TRIBE_COUNT):
        raw = payload[index * TRIBE_SIZE : (index + 1) * TRIBE_SIZE]
        image = bytearray(TRIBE_SIZE)
        for begin, end in TRIBE_RANGES:
            image[begin:end] = raw[begin:end]
        records.append(bytes(image))
    return tuple(records)


def parse_tribe_capture(path: Path) -> TribeCapture:
    """Parse the address-bearing text emitted by ``donject peek``.

    The current injector header supplies ``base``, ``addr``, and ``len``.  Newer
    headers may additionally supply the request metadata; every field that is present
    is checked against the measured global-pointer boundary.
    """
    try:
        lines = path.read_text(encoding="ascii").splitlines()
    except UnicodeDecodeError as error:
        raise AssetError(
            f"{path}: expected address-bearing donject peek text, not a raw memory blob"
        ) from error
    nonempty = [line.strip() for line in lines if line.strip()]
    if not nonempty or not nonempty[0].startswith("#"):
        raise AssetError(f"{path}: missing donject peek header")
    header_pairs = re.findall(r"\b([A-Za-z][A-Za-z0-9_]*)=([^\s]+)", nonempty[0])
    header = {name.casefold(): value for name, value in header_pairs}
    if len(header) != len(header_pairs):
        raise AssetError(f"{path}: duplicate field in donject peek header")

    module_base = _header_hex(header, "base")
    address = _header_hex(header, "addr")
    length = _header_hex(header, "len")
    if module_base < 0x00400000 or module_base & 0xFFFF:
        raise AssetError(f"{path}: implausible x86 image base 0x{module_base:08x}")
    if address < 0x10000 or address & 3 or address + length > 0x1_0000_0000:
        raise AssetError(f"{path}: invalid x86 read range 0x{address:08x}+0x{length:x}")
    if length != TRIBE_CAPTURE_BYTES:
        raise AssetError(
            f"{path}: header len=0x{length:x}, expected 24 * 0x5f0 = 0x{TRIBE_CAPTURE_BYTES:x}"
        )

    rva = _optional_header_hex(header, "rva", "root_rva")
    if rva is not None and rva != TRIBE_POINTER_RVA:
        raise AssetError(
            f"{path}: header rva=0x{rva:x}, expected Tribe pointer RVA 0x{TRIBE_POINTER_RVA:x}"
        )
    deref = _optional_header_hex(header, "deref", "nderef")
    if deref is not None and deref != 1:
        raise AssetError(f"{path}: header deref={deref}, expected 1")
    offset = _optional_header_hex(header, "off", "offset")
    if offset is not None and offset != 0:
        raise AssetError(f"{path}: header off=0x{offset:x}, expected 0")
    root = _optional_header_hex(header, "root", "source", "pointer_addr")
    if root is not None and root != module_base + TRIBE_POINTER_RVA:
        raise AssetError(
            f"{path}: header pointer address 0x{root:08x}, expected "
            f"base+rva = 0x{module_base + TRIBE_POINTER_RVA:08x}"
        )
    if "module" in header and header["module"].casefold() != "riseofnations.exe":
        raise AssetError(f"{path}: capture module is {header['module']!r}, not riseofnations.exe")
    for name in ("exe_sha256", "image_sha256", "target_sha256"):
        if name in header and header[name].casefold() != SUPPORTED_EXE_SHA256:
            raise AssetError(f"{path}: {name} does not identify the supported executable")
    if "stable" in header and header["stable"].casefold() not in {"1", "true", "yes"}:
        raise AssetError(f"{path}: capture reports unstable root pointer")

    payload = bytearray()
    expected_address = address
    row_pattern = re.compile(r"^([0-9a-fA-F]{8}):((?: [0-9a-fA-F]{2}){1,16})$")
    for line_number, line in enumerate(nonempty[1:], 2):
        match = row_pattern.fullmatch(line)
        if not match:
            raise AssetError(f"{path}:{line_number}: malformed donject peek row")
        row_address = int(match.group(1), 16)
        if row_address != expected_address:
            raise AssetError(
                f"{path}:{line_number}: row starts at 0x{row_address:08x}, "
                f"expected 0x{expected_address:08x}"
            )
        row = bytes.fromhex(match.group(2))
        if len(payload) + len(row) < length and len(row) != 16:
            raise AssetError(f"{path}:{line_number}: short row before the final payload row")
        payload.extend(row)
        expected_address += len(row)
        if len(payload) > length:
            raise AssetError(f"{path}:{line_number}: payload exceeds header len")
    if len(payload) != length:
        raise AssetError(f"{path}: decoded {len(payload)} bytes, header declares {length}")

    return TribeCapture(
        module_base=module_base,
        address=address,
        records=normalize_tribe_records(bytes(payload)),
        header=header,
    )


def walk_static_prefix(after_types: int, constants: bytes, balance: bytes) -> dict[str, int]:
    if len(constants) != RULES_BLOCK_BYTES:
        raise AssetError(f"Constants block is {len(constants)} bytes, expected {RULES_BLOCK_BYTES}")
    if len(balance) != BALANCE_BYTES:
        raise AssetError(f"balance table is {len(balance)} bytes, expected {BALANCE_BYTES}")
    adler = zlib.adler32(constants, after_types) & 0xFFFF_FFFF
    adler = zlib.adler32(
        constants[RULES_DUPLICATE_OFFSET : RULES_DUPLICATE_OFFSET + 4], adler
    ) & 0xFFFF_FFFF
    after_constants = adler
    adler = zlib.adler32(balance, adler) & 0xFFFF_FFFF
    return {
        "after_types": after_types,
        "after_constants": after_constants,
        "after_balance": adler,
        "bytes_walked": TYPE_WALKED_BYTES + CONSTANTS_WALKED_BYTES + BALANCE_BYTES,
    }


def walk_static_rules(
    after_types: int, constants: bytes, balance: bytes, tribes: tuple[bytes, ...]
) -> dict[str, int]:
    if len(tribes) != TRIBE_COUNT or any(len(image) != TRIBE_SIZE for image in tribes):
        raise AssetError("normalized Tribe set is not exactly 24 records of 0x5f0 bytes")
    prefix = walk_static_prefix(after_types, constants, balance)
    adler = prefix["after_balance"]
    for image in tribes:
        for begin, end in TRIBE_RANGES:
            adler = zlib.adler32(image[begin:end], adler) & 0xFFFF_FFFF
    return {
        "after_types": after_types,
        "after_constants": prefix["after_constants"],
        "after_balance": prefix["after_balance"],
        "after_tribes": adler,
        "bytes_walked": RULES_WALKED_BYTES,
    }


def check_walker_source(path: Path) -> None:
    text = path.read_text(encoding="utf-8")
    expected = {
        "TYPE_SLOTS": "806",
        "RETAIL_AFTER_TYPES": "0x72e0_c3b6",
        "RETAIL_TYPE_WALKED_BYTES": "473_984",
        "TRIBE_COUNT": "24",
        "TRIBE_SIZE": "0x05f0",
    }
    for name, literal in expected.items():
        if not re.search(rf"\b{name}\b[^;]*\b{re.escape(literal)}\b", text):
            raise AssetError(f"{path}: cannot confirm {name}={literal}")


def tribe_blockers(root: Path) -> tuple[list[str], list[str]]:
    rules = root / "ron-data/rules.xml"
    if not rules.exists():
        return [], ["ron-data/rules.xml is absent (gitignored shipped data)"]
    tree = ET.parse(rules)
    node = tree.getroot().find("TRIBES")
    names = (
        [x.findtext("FILE", "").strip() for x in node.findall("TRIBE")]
        if node is not None
        else []
    )
    blockers = []
    if len(names) != 24:
        blockers.append(f"rules.xml names {len(names)} tribes, expected 24")
    # RulesCategory::CAT_TRIBES resolves these names under the shipped `tribes\`
    # category. Same-named campaign/scenario XML files elsewhere in the extracted
    # corpus are not loader inputs and must not make this gate look less incomplete.
    tribe_dir = root / "ron-data/tribes"
    available = (
        {p.name.casefold() for p in tribe_dir.iterdir() if p.is_file()}
        if tribe_dir.is_dir()
        else set()
    )
    missing = [name for name in names if name.casefold() not in available]
    if missing:
        blockers.append(
            f"{len(missing)} of {len(names)} nation XML files in ron-data/tribes that set "
            "Tribe scalar/substitution state are absent: "
            + ", ".join(missing)
        )
    blockers.append(
        "no checked-in value-level capture exists for the 24 x TypeIndex[352] graft arrays"
    )
    return names, blockers


def audit_unitrules(path: Path) -> dict[str, int]:
    if not path.exists():
        raise AssetError(f"{path} is absent (gitignored shipped data)")
    units = ET.parse(path).getroot().findall("UNIT")
    grafts = sum((unit.findtext("GRAFT") or "").strip().casefold() != "none" for unit in units)
    bad_masks = sum(len((unit.findtext("TRIBE_MASK") or "").strip()) != 24 for unit in units)
    if len(units) != 364 or grafts != 194 or bad_masks:
        raise AssetError(
            f"{path}: units={len(units)}, non-none grafts={grafts}, bad 24-bit masks={bad_masks}"
        )
    return {"unit_rows": len(units), "non_none_grafts": grafts, "tribe_masks": len(units)}


def normalized_image(ty: TypeInput) -> bytes:
    _registry, _kind, minimum = expected_type(ty.slot)
    image = bytearray(minimum)
    for begin, end in walked_ranges(ty.kind):
        image[begin:end] = ty.image[begin:end]
    return bytes(image)


def emit_types(
    path: Path,
    source: Path,
    types: list[TypeInput],
    arrays: list[tuple[U16Array, U16Array] | None],
) -> None:
    payload = {
        "format": "don-rules-type-assets-v1",
        "source_sha256": sha256(source),
        "after_types": f"0x{TYPE_CHECKPOINT:08x}",
        "bytes_walked": TYPE_WALKED_BYTES,
        "privacy": "unwalked bytes, vtables, pointers, and String state are zeroed",
        "types": [],
    }
    for ty, pair in zip(types, arrays):
        item = {
            "slot": ty.slot,
            "registry": ty.registry,
            "kind": ty.kind,
            "image_b64": base64.b64encode(normalized_image(ty)).decode("ascii"),
            "object_arrays": [],
        }
        if pair:
            item["object_arrays"] = [
                {
                    "capacity": value.capacity,
                    "grow": value.grow,
                    "flags": value.flags & 0xBF,
                    "elements": list(value.elements),
                }
                for value in pair
            ]
        payload["types"].append(item)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def self_test() -> None:
    assert zlib.adler32(b"Wikipedia") & 0xFFFF_FFFF == 0x11E60398
    assert expected_type(0) == ("GoodType", "Good", 760)
    assert expected_type(543) == ("ItemType", "Object", 636)
    assert expected_type(805) == ("BonusType", "Type", 94)
    assert len(ARRAY_INITIALIZED) == 481
    assert TRIBE_CAPTURE_BYTES == 0x8E80
    assert TRIBE_WALKED_BYTES == 34_368
    assert RULES_WALKED_BYTES == 997_846
    print("rules-channel-assets self-test: ok")


def report(root: Path, args: argparse.Namespace) -> tuple[dict, bool]:
    capture = root / args.capture
    if not capture.exists():
        raise AssetError(
            f"{capture} is absent; it is gitignored and the checked-in TSVs do not contain "
            "all checksum-visible image bytes"
        )
    module_base, blocks = parse_dontype(capture)
    types = select_types(blocks)
    check_typeids(root / "schema/live/live-tables-typeids.tsv", types)
    validate_pdb(root / "schema/pdb-types.json")
    validate_state_schema(root / "schema/state-schema.json")
    constants_path = root / "schema/live/rules-block-pid14644.txt"
    balance_path = root / "schema/live/final-balance-runtime.bin"
    constants = repository_constants(constants_path)
    balance = repository_balance(balance_path)
    validate_manifest(
        root / "schema/rules-channel-assets.json", capture, constants, balance_path
    )
    check_walker_source(root / "crates/don-replay/src/rules_channel.rs")
    arrays = rebuild_arrays(types)
    checkpoint, walked = walk_types(types, arrays)
    if checkpoint != TYPE_CHECKPOINT or walked != TYPE_WALKED_BYTES:
        raise AssetError(
            f"type walk produced 0x{checkpoint:08x}/{walked}, expected "
            f"0x{TYPE_CHECKPOINT:08x}/{TYPE_WALKED_BYTES}"
        )

    prefix = walk_static_prefix(checkpoint, constants, balance)
    prefix_expected = {
        "after_constants": CONSTANTS_CHECKPOINT,
        "after_balance": BALANCE_CHECKPOINT,
    }
    for name, expected in prefix_expected.items():
        if prefix[name] != expected:
            raise AssetError(
                f"checked-in inputs produce {name}=0x{prefix[name]:08x}, "
                f"expected measured retail checkpoint 0x{expected:08x}"
            )

    tribe_names, xml_blockers = tribe_blockers(root)
    unitrules = audit_unitrules(root / "ron-data/unitrules.xml")
    tribe_capture = None
    checkpoints = None
    if args.tribes_capture:
        tribe_capture_path = Path(args.tribes_capture)
        if not tribe_capture_path.is_absolute():
            tribe_capture_path = root / tribe_capture_path
        tribe_capture = parse_tribe_capture(tribe_capture_path)
        checkpoints = walk_static_rules(checkpoint, constants, balance, tribe_capture.records)
        final_expected = {
            "after_constants": CONSTANTS_CHECKPOINT,
            "after_balance": BALANCE_CHECKPOINT,
            "after_tribes": RULES_CHECKPOINT,
            "bytes_walked": RULES_WALKED_BYTES,
        }
        for name, expected in final_expected.items():
            if checkpoints[name] != expected:
                formatted = (
                    f"0x{checkpoints[name]:08x}" if name.startswith("after_") else checkpoints[name]
                )
                wanted = f"0x{expected:08x}" if name.startswith("after_") else expected
                raise AssetError(
                    f"capture and repository inputs produce {name}={formatted}, expected {wanted}"
                )
        blockers = []
    else:
        blockers = [
            "no --tribes-capture was supplied; capture exactly 0x8e80 bytes with "
            "`donject peek PID riseofnations.exe a7fa34 1 0 8e80`"
        ] + xml_blockers

    if args.emit_types:
        emit_types(Path(args.emit_types), capture, types, arrays)
    array_values = sum(len(value.elements) for pair in arrays if pair for value in pair)
    result = {
        "status": "incomplete" if blockers else "complete-local-specimen",
        "source": {
            "path": str(capture.relative_to(root)),
            "sha256": sha256(capture),
            "module_base": f"0x{module_base:08x}",
            "checked_in": False,
        },
        "types": {
            "slots": len(types),
            "after_types": f"0x{checkpoint:08x}",
            "bytes_walked": walked,
            "pointed_arrays": sum(2 for pair in arrays if pair),
            "pointed_u16_values": array_values,
            "capture_header_mismatches": 0,
        },
        "constants": {
            "source": str(constants_path.relative_to(root)),
            "source_checked_in": True,
            "bytes": len(constants),
            "after_constants": f"0x{prefix['after_constants']:08x}",
        },
        "balance": {
            "source": str(balance_path.relative_to(root)),
            "source_checked_in": False,
            "bytes": len(balance),
            "after_balance": f"0x{prefix['after_balance']:08x}",
        },
        "tribes": {
            "named": len(tribe_names),
            "complete_records": len(tribe_capture.records) if tribe_capture else 0,
            "raw_bytes_retained": 0,
            "normalized_bytes_per_record": TRIBE_SIZE if tribe_capture else 0,
            "walked_bytes": TRIBE_WALKED_BYTES if tribe_capture else 0,
            **unitrules,
        },
        "blockers": blockers,
    }
    if tribe_capture and checkpoints:
        normalized = b"".join(tribe_capture.records)
        result["tribes"]["capture_header"] = {
            "module_base": f"0x{tribe_capture.module_base:08x}",
            "address": f"0x{tribe_capture.address:08x}",
            "length": TRIBE_CAPTURE_BYTES,
            "normalized_sha256": hashlib.sha256(normalized).hexdigest(),
        }
        result["rules"] = {
            "after_types": f"0x{checkpoints['after_types']:08x}",
            "after_constants": f"0x{checkpoints['after_constants']:08x}",
            "after_balance": f"0x{checkpoints['after_balance']:08x}",
            "after_tribes": f"0x{checkpoints['after_tribes']:08x}",
            "bytes_walked": checkpoints["bytes_walked"],
            "matches_retail": True,
        }
        result["remaining_integration"] = [
            "derive a reproducible lawful Tribe builder or minimized value representation; "
            "the raw donject capture remains local and must not be checked in",
            "replace the gitignored Type and Balance specimen dependencies with lawful builders",
            "feed normalized complete assets into don-replay channel 13",
        ]
    return result, not blockers


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, help="repository root (auto-detected by script path)")
    parser.add_argument(
        "--capture", default="schema/live/types-runtime.bin", help="DONTYPE1 path under root"
    )
    parser.add_argument("--check", action="store_true", help="require the full Types+Tribes gate")
    parser.add_argument(
        "--check-types", action="store_true", help="require the independently complete Types gate"
    )
    parser.add_argument(
        "--tribes-capture",
        metavar="PATH",
        help="address-bearing donject peek text for 24 contiguous 0x5f0-byte Tribe records",
    )
    parser.add_argument("--emit-types", metavar="PATH", help="write normalized type assets")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)
    if args.self_test:
        self_test()
        return 0
    root = (args.root or Path(__file__).resolve().parents[1]).resolve()
    try:
        result, complete = report(root, args)
    except (AssetError, OSError, ValueError, ET.ParseError) as error:
        print(f"rules-channel-assets: ERROR: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, indent=2))
    if args.check and not complete:
        print("rules-channel-assets: full gate BLOCKED (see blockers above)", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
