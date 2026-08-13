# Retail save boundary: `Camera::walk_data`

Status: **measured complete owner**. The isolated helper
`re/scripts/savegame_camera.py` consumes Camera's tag and both direct memory
images in their retail stream order, then stops before `SelectGroups`.
Exhaustive gates live in `re/scripts/test_savegame_camera.py`; no shared parser
is changed.

## Exact installed boundary

The fresh SVX compressed/decompressed SHA-256 values are
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.

| owner | stream range | bytes | SHA-256 |
|---|---:|---:|---|
| Camera tag, StringTable[413] | `0x2bf7b..0x2bf7c` | 1 | `6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d` |
| Camera `[+0x28c,+0x370)` | `0x2bf7c..0x2c060` | 228 | `fb678f67aea5293efa9930a41b828fdfb475dc2b427628588640e080884f0e45` |
| BaseCamera `[+0xb4,+0x288)` | `0x2c060..0x2c234` | 468 | `fad0291d5a68a5bcfa50634082d9313e1636346fe46371801de06576f8d1f4a6` |
| complete Camera owner | `0x2bf7b..0x2c234` | 697 | `4d5e25a4acedc3660f8ad38b1629fa719ea6fca82cf58bbed13c00b8c7c82996` |
| next SelectGroups tag, StringTable[6009] | begins `0x2c234` | 1 | excluded |

Every byte in the fresh Camera image is zero. The parser nevertheless exposes
all 174 direct 32-bit words without floating-point conversion, so signed zero,
NaN payloads, integer fields, transforms, and matrices round-trip bit-exactly.

## Exact PE grammar

`Camera::walk_data` is VA `0x00844090`, 84 bytes, SHA-256
`12c6751e3794eaf2300c5714da80b62e931694ac7d95c9b591281aeb3437ea69`.
It performs exactly three visitor operations:

```text
walk_test(StringTable[413])
DataWalk(Camera + 0x28c, Camera + 0x370)       # 228 bytes
DataWalk(Camera + 0x0b4, Camera + 0x288)       # 468 bytes
```

The inlined caller copy at VA `0x005a2f0a`, 67 bytes, SHA-256
`8acbf09832cc9ae0c02660c8a76255f569a3915c05da47309799be2cfcb2747f`,
uses the same order and exact endpoints. It then calls the nonserializing
`NetDaemon::process_all` before beginning the next owner.

The next owner is independently frozen by `SelectGroups::walk_data` at VA
`0x00717230`, 57 bytes, SHA-256
`3187e207ece6a4fdc580107bfdd4255e1b6e228295e89b817a40e23b4069d526`.
It emits StringTable[6009] and walks the two `Array<SelectGroup>` globals at
`0x00e8d434` and `0x00e8d450`. The array walker at VA `0x00480900`, 545
bytes, has SHA-256
`752388b8cbff6cbe86bbb73d62050fa43707d626b9262177e0e4f11dfdb95346`.

The matched PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.

## PDB ownership and exclusions

The matched PDB/schema SHA-256 values are
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`
and
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
PDB layout fixes `Camera` at 880 bytes with `BaseCamera` at `+0` (648 bytes)
and the empty `GameAccessConst` base at `+652`. `BaseCamera` itself contains
the 180-byte `HierObj` base followed by fields through `+648`.

The first serialized range contains every Camera-declared field from
`camera_changed` at `+652` through `distance` at `+876`. The second contains
every BaseCamera-declared field from `axis_transform` at `+180` through
`parallel_distance` at `+644`. Consequently the owner deliberately excludes:

- inherited `HierObj` bytes `[+0,+0xb4)`;
- the BaseCamera/empty-base boundary `[+0x288,+0x28c)`;
- every host pointer or vftable contained by those excluded bases.

The deterministic four-class layout receipt is
`a709d3a452dc8e08a3e539189e2ea36de5b594cf88dbe34941044f0ae40fa11d`.

## Gates

The synthetic fixture fills all 57 Camera words and 117 BaseCamera words with
nontrivial bit patterns. Tests mutate every one of the 697 owned bytes, reject
every truncation and the wrong tag, prove every following SelectGroups byte is
excluded, and kill a mutated PDB Camera size. The installed gate checks the
fresh image, the exact PE bodies, the detail-threshold predecessor when that
isolated helper is present, and independent SVX/RCX seeds.

## Reproduction

```sh
python3 re/scripts/test_savegame_camera.py
python3 re/scripts/savegame_camera.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x2bf7b
```

The returned end, `0x2c234`, is the exact first byte owned by SelectGroups.
