# Retail save boundary: direct `detail_threshold`

Status: **measured complete direct owner**. The isolated helper
`re/scripts/savegame_detail_threshold.py` consumes the caller's exact four-byte
global range and stops before the Camera tag. Exhaustive gates live in
`re/scripts/test_savegame_detail_threshold.py`; no shared parser is changed.

## Exact installed boundary

The fresh SVX compressed/decompressed SHA-256 values are
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.

| owner | range | bytes | SHA-256 |
|---|---:|---:|---|
| direct `detail_threshold` | `0x2bf77..0x2bf7b` | 4 | `df3f619804a92fdb4057192dc43dd748ea778adc52bc498ce80524c014b81119` |
| next Camera tag, StringTable[413] | begins `0x2bf7b` | 1 | excluded |

The installed word is zero. The helper deliberately reports its raw IEEE-754
bit pattern as `0x00000000`: it does not canonicalize NaNs or conflate signed
zero. Mutating the following Camera tag leaves the result identical.

## Exact PE caller grammar

The caller span at VA `0x005a2efa`, 43 bytes through the Camera tag call,
has SHA-256
`7403157b8e699cc491e164896f010852b9db3ef91b603899d67bead6a67d0368`:

```text
mov eax,[ebx]
mov ecx,ebx
push 0x00c06240                 # exclusive end: sGameSaveVersion
push 0x00c0623c                 # begin: detail_threshold
call [eax]                      # DataWalk(begin,end), exactly four bytes
...
mov esi,[0x00c06200]            # MiscAccess::camera (Camera&)
...
add eax,0x2044                  # StringTable[413]
push eax
call [edx+4]                    # next owner's tag
```

The following caller instructions prove the Camera owner independently: they
submit Camera `+0x28c..+0x370` (228 bytes), then `+0xb4..+0x288` (468 bytes).
Those ranges are not absorbed into this direct-global owner.

The matched PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.

## PDB/global boundary and gates

The matched PDB, schema, and symbol-export SHA-256 values are
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`,
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`,
and `8e0fdfc4a538c1dc51f615c38d2fb713a901efee70f80d65106b9fb5c52d623f`.
The PDB export fixes adjacent globals exactly:

| VA | symbol | type | size |
|---:|---|---|---:|
| `0x00c0623c` | `detail_threshold` | `float` (type index 64) | 4 |
| `0x00c06240` | `sGameSaveVersion` | `int` (type index 116) | 4 |

Their deterministic layout receipt is
`ffe89a9c7186460ad3e018116a788196a12c2c5a2b8833fdd7dce771dbc857b2`.
The next PDB class is the 880-byte `Camera`, agreeing with the reference loaded
through `MiscAccess::camera`.

The synthetic fixture preserves a noncanonical NaN payload, independently
mutates every owned byte, mutates every following-owner byte, and rejects every
truncation and invalid offset. A dedicated mutation changes the PDB-exported
global size and must fail closed. The installed gate chains World through
ConquestGame by returned boundaries, lands at `0x2bf77`, and independently
verifies SVX/RCX seeds `0x014810ac` and `0x007f93e0`.

## Reproduction

```sh
python3 re/scripts/test_savegame_detail_threshold.py
python3 re/scripts/savegame_detail_threshold.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x2bf77
```

The returned end, `0x2bf7b`, is the exact first byte owned by the caller's
Camera section.
