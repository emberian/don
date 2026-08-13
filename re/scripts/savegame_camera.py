#!/usr/bin/env python3
"""Parse the complete retail ``Camera::walk_data`` save image."""

from __future__ import annotations

import argparse
import dataclasses
import functools
import gzip
import hashlib
import json
import pathlib
import struct
from typing import Sequence


TAG_CAMERA = 0
TAG_STRING_TABLE_INDEX = 413
NEXT_TAG_STRING_TABLE_INDEX = 6009
CAMERA_TAIL_OFFSET = 0x28C
CAMERA_TAIL_END = 0x370
BASE_CAMERA_TAIL_OFFSET = 0xB4
BASE_CAMERA_TAIL_END = 0x288
CAMERA_TAIL_SIZE = CAMERA_TAIL_END - CAMERA_TAIL_OFFSET
BASE_CAMERA_TAIL_SIZE = BASE_CAMERA_TAIL_END - BASE_CAMERA_TAIL_OFFSET
CAMERA_SECTION_SIZE = 1 + CAMERA_TAIL_SIZE + BASE_CAMERA_TAIL_SIZE
DEFAULT_SCHEMA_PATH = pathlib.Path(__file__).resolve().parents[2] / "schema/pdb-types.json"


class CameraParseError(ValueError):
    """The stream or PDB layout contradicts ``Camera::walk_data``."""


@dataclasses.dataclass(frozen=True)
class CameraSection:
    offset: int
    end: int
    tag: int
    camera_tail: bytes
    base_camera_tail: bytes
    camera_words: tuple[int, ...]
    base_camera_words: tuple[int, ...]
    raw: bytes
    sha256: str
    layout_sha256: str

    @property
    def size(self) -> int:
        return self.end - self.offset


@dataclasses.dataclass(frozen=True)
class _Layout:
    sha256: str


def _fields(record: dict[str, object]) -> tuple[tuple[object, ...], ...]:
    return tuple((field["name"], field["offset"], field["size"], field["type"]) for field in record["fields"])


BASE_CAMERA_FIELDS = (
    ("axis_transform", 180, 48, "Transform<float>"),
    ("origin_axis_transform", 228, 48, "Transform<float>"),
    ("x_cos", 276, 4, "float"), ("x_sin", 280, 4, "float"),
    ("y_cos", 284, 4, "float"), ("y_sin", 288, 4, "float"),
    ("x_tan", 292, 4, "float"), ("y_tan", 296, 4, "float"),
    ("angle", 300, 4, "float"), ("near_plane", 304, 4, "float"),
    ("far_plane", 308, 4, "float"), ("active", 312, 4, "int"),
    ("view_ratio", 316, 4, "float"),
    ("origin_render_matrix", 320, 64, "Matrix<float>"),
    ("render_matrix", 384, 64, "Matrix<float>"),
    ("projection_matrix", 448, 64, "Matrix<float>"),
    ("origin_to_screen_matrix", 512, 64, "Matrix<float>"),
    ("to_screen_matrix", 576, 64, "Matrix<float>"),
    ("parallel", 640, 4, "int"), ("parallel_distance", 644, 4, "float"),
)

CAMERA_FIELDS = (
    ("camera_changed", 652, 4, "int"), ("camera_changed_zoom", 656, 4, "int"),
    ("zoom_level", 660, 4, "int"), ("to_zoom_level", 664, 4, "int"),
    ("zooming", 668, 4, "int"), ("panning", 672, 4, "int"),
    ("pan_to_x", 676, 4, "int"), ("pan_to_y", 680, 4, "int"),
    ("pan_from_x", 684, 4, "int"), ("pan_from_y", 688, 4, "int"),
    ("follow", 692, 4, "int"), ("ox", 696, 4, "int"),
    ("whom", 700, 4, "int"), ("wheel_accum", 704, 4, "int"),
    ("scroll_speeds", 708, 28, "float[7]"), ("scroll_speed", 736, 4, "int"),
    ("scroll_edge", 740, 4, "int"), ("last_scroll", 744, 4, "int"),
    ("scroll_clock", 748, 4, "int"), ("zoom_distance", 752, 4, "float"),
    ("tilt_angle", 756, 4, "float"), ("dir_angle", 760, 4, "float"),
    ("dir_angle_cos", 764, 4, "float"), ("dir_angle_sin", 768, 4, "float"),
    ("loc_x", 772, 4, "float"), ("loc_y", 776, 4, "float"),
    ("loc_x_adjust", 780, 4, "float"), ("loc_y_adjust", 784, 4, "float"),
    ("loc_z_adjust", 788, 4, "float"), ("zoom_mode", 792, 4, "int"),
    ("zoom_factor", 796, 4, "float"), ("fixed_zooms", 800, 28, "float[7]"),
    ("fixed_angles", 828, 28, "float[7]"), ("tilt_onoff", 856, 4, "float"),
    ("scroll_lock_flags", 860, 4, "int"), ("zoom_lock_flags", 864, 4, "int"),
    ("fixed_zooms_inited", 868, 4, "int"), ("last_zoom_level", 872, 4, "int"),
    ("distance", 876, 4, "float"),
)


@functools.lru_cache(maxsize=4)
def _load_layout(path_text: str) -> _Layout:
    path = pathlib.Path(path_text)
    try:
        classes = json.loads(path.read_bytes())["classes"]
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise CameraParseError(f"cannot load Camera PDB layout from {path}: {error}") from error
    sizes = {"Camera": 880, "BaseCamera": 648, "HierObj": 180, "GameAccessConst": 1}
    for name, size in sizes.items():
        record = classes.get(name)
        if not record or record.get("size") != size:
            raise CameraParseError(f"PDB {name} size disagrees: {record.get('size') if record else None}")
    camera_bases = tuple((base["name"], base["offset"], base["size"]) for base in classes["Camera"]["bases"])
    base_bases = tuple((base["name"], base["offset"], base["size"]) for base in classes["BaseCamera"]["bases"])
    if camera_bases != (("BaseCamera", 0, 648), ("GameAccessConst", 652, 1)):
        raise CameraParseError(f"PDB Camera bases disagree: {camera_bases!r}")
    if base_bases != (("HierObj", 0, 180),):
        raise CameraParseError(f"PDB BaseCamera bases disagree: {base_bases!r}")
    if _fields(classes["BaseCamera"]) != BASE_CAMERA_FIELDS:
        raise CameraParseError("PDB BaseCamera fields disagree")
    if _fields(classes["Camera"]) != CAMERA_FIELDS:
        raise CameraParseError("PDB Camera fields disagree")
    receipt = {
        "sizes": sizes,
        "camera_bases": camera_bases,
        "base_camera_bases": base_bases,
        "base_camera_fields": BASE_CAMERA_FIELDS,
        "camera_fields": CAMERA_FIELDS,
        "selectors": {
            "tag": TAG_STRING_TABLE_INDEX,
            "camera_tail": [CAMERA_TAIL_OFFSET, CAMERA_TAIL_END],
            "base_camera_tail": [BASE_CAMERA_TAIL_OFFSET, BASE_CAMERA_TAIL_END],
            "next_tag": NEXT_TAG_STRING_TABLE_INDEX,
        },
    }
    return _Layout(hashlib.sha256(json.dumps(receipt, sort_keys=True, separators=(",", ":")).encode()).hexdigest())


def parse_camera_section(
    data: bytes | bytearray | memoryview,
    offset: int,
    *,
    require_tag: bool = True,
    schema_path: pathlib.Path = DEFAULT_SCHEMA_PATH,
) -> CameraSection:
    """Parse Camera's tag and two direct images, stopping before SelectGroups."""
    layout = _load_layout(str(pathlib.Path(schema_path).resolve()))
    view = memoryview(data)
    end = offset + CAMERA_SECTION_SIZE
    if offset < 0 or end > len(view):
        raise CameraParseError(f"Camera range [{offset:#x},{end:#x}) exceeds {len(view):#x}-byte stream")
    tag = view[offset]
    if require_tag and tag != TAG_CAMERA:
        raise CameraParseError(f"Camera tag {tag:#04x} != {TAG_CAMERA:#04x}")
    camera_start = offset + 1
    base_start = camera_start + CAMERA_TAIL_SIZE
    camera_tail = bytes(view[camera_start:base_start])
    base_camera_tail = bytes(view[base_start:end])
    raw = bytes(view[offset:end])
    return CameraSection(
        offset,
        end,
        tag,
        camera_tail,
        base_camera_tail,
        struct.unpack(f"<{CAMERA_TAIL_SIZE // 4}I", camera_tail),
        struct.unpack(f"<{BASE_CAMERA_TAIL_SIZE // 4}I", base_camera_tail),
        raw,
        hashlib.sha256(raw).hexdigest(),
        layout.sha256,
    )


def _load(path: pathlib.Path) -> bytes:
    raw = path.read_bytes()
    return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=pathlib.Path)
    parser.add_argument("--offset", required=True, type=lambda text: int(text, 0))
    args = parser.parse_args(argv)
    section = parse_camera_section(_load(args.file), args.offset)
    print(
        f"{args.file}: Camera {section.offset:#x}..{section.end:#x} "
        f"({section.size} bytes) sha256={section.sha256}\n"
        f"  next owner begins at {section.end:#x}: SelectGroups tag"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
