# Rules-channel checked-in input audit

Status: **the existing local Type, Constants, and Balance inputs plus the tracked
walked-only Tribe asset reproduce the complete retail static Rules channel.** The local
specimen gate is green at `0x12ba3104`; fresh-clone self-containment remains red because
the exact Type and Balance inputs are still gitignored.

The deterministic audit/extractor is `tools/rules-channel-assets.py`; its small,
non-memory manifest is `schema/rules-channel-assets.json`. It does not attach to or read a
running process.

## Exact Types result

The local `schema/live/types-runtime.bin` is a `DONTYPE1` capture: ten 806-slot registries,
each with a pointer table and fixed 1,792-byte record bank. The script selects the
most-derived record in global TypeIndex order:

| TypeIndex | registry | retail walker | count |
|---|---|---|---:|
| 0–49 | `GoodType` | `GoodType` | 50 |
| 50–413 | `UnitType` | `UnitType` | 364 |
| 414–542 | `BuildType` | `BuildType` | 129 |
| 543 | `ItemType` | inherited `ObjectType` | 1 |
| 544–628 | `TechType` | `TechType` | 85 |
| 629–683 | `SpellType` | `SpellType` | 55 |
| 684–805 | `BonusType` | inherited `Type` | 122 |

This selection is checked slot-for-slot against the tracked
`schema/live/live-tables-typeids.tsv`.

The two `SimpleArray<unsigned short>` values at ObjectType `+0x27c` and `+0x298` are not
independent source data. `ObjectType::init_is_list` (`0x00661dd0`) builds them by calling
`ObjectTypeData::is_slow` (`0x00661ae0`) for all 806 target IDs, once non-strict and once
strict. The exact rules are:

- identity always matches;
- non-strict matching follows `graft +0x25c`, then the recursive `from +0x3c` chain;
- strict matching admits identity and a unit's graft target unless that target has
  `unit_flags & 0x01000000`;
- `ObjectType::finalize_init_all` (`0x0065f4a0`) initializes these caches only for ordinary
  units 50–401 and buildings 414–542. Goods, Gaia units 402–413, and ItemType retain empty
  arrays.

The extractor rebuilds all **1,088 array instances containing 2,363 u16 values**. Every
rebuilt count matches the corresponding captured header. Feeding the selected images and
rebuilt payloads through an independent Python implementation of the landed traversal
produces exactly:

| checkpoint | Adler-32 | bytes |
|---|---:|---:|
| after 806 Types | **`0x72e0c3b6`** | **473,984** |

Those values equal the narrow live walker checkpoint in
`docs/mechanics/rules-channel.md`. `--emit-types PATH` can write a deterministic JSON
integration asset. It zeroes all bytes the checksum walker does not visit—including
vtables, heap pointers, and String state—rather than republishing a raw memory dump.

Important provenance limit: `schema/live/types-runtime.bin` is gitignored, not checked in.
The tracked decoded TSVs cover many scalars but are not lossless for the complete walked
images. This script converts the existing specimen reproducibly; it does not yet make a
fresh clone self-contained.

## Exact local prefix inputs

The script now consumes the same local repository paths as the Rust walker:

- the first 3,392 decoded bytes of `schema/live/rules-block-pid14644.txt`, followed by
  the duplicate four-byte visit at `+0x804`;
- the exact 486,098-byte `schema/live/final-balance-runtime.bin` table.

Together with the Types result above, the independent Python walk reaches the measured
cumulative `after Constants == 0x50625668` and `after Balance == 0x56daabc1`
checkpoints. Consequently there is no hidden Constants or Balance asset blocker behind
the missing Tribe data: one valid Tribe capture is sufficient to exercise the final
checksum admission gate.

The Constants text is tracked. The Balance binary, like `types-runtime.bin`, is
gitignored proprietary input. Thus the local prefix calculation is exact, but a fresh
clone is still missing both the Type images and Balance table; the manifest records both
dependencies instead of treating the local green prefix as a distributable builder.

## Exact Tribe result

PDB layout is exact: `sizeof(Tribe) == 0x5f0`, with `graft: TypeIndex[352]` at `+0x70`.
The checksum visits six dwords at `[+0x54,+0x6c)` and the graft array at
`[+0x70,+0x5f0)`, for 34,368 bytes across 24 records.

`ron-data/rules.xml` names all 24 nation files, but those files are absent from the local
data set. `Tribes::init` establishes an identity graft baseline, while nation parsing and
substitution mutate the checksum-visible scalar/graft state. `unitrules.xml` contains
`GRAFT` and `TRIBE_MASK`, but neither the loader derivation nor a value-level comparison
establishes that those two columns alone reproduce the final 24 arrays. The tool does not
manufacture them.

`--tribes-capture PATH` ingests the existing address-bearing `donject peek` text format.
It requires a `base`/`addr`/`len` header, exactly `0x8e80` bytes, contiguous addressed
hex rows, a plausible x86 image base, and a valid target address range. The hardened
header also supplies `module`, `rva`, `deref`/`nderef`, `off`, both root-pointer address
fields, root value, and root stability; the parser validates every redundant field against
the measured Tribe pointer at preferred VA `0x00e7fa34` (RVA `0x00a7fa34`).

Immediately after parsing, every record is replaced by a zeroed `0x5f0` image containing
only `[+0x54,+0x6c)` and `[+0x70,+0x5f0)`. Vtables, pointers, strings, padding, and all
other unwalked process bytes do not survive ingestion. The raw capture remains an ignored
local input and must not be committed. A supplied specimen is admitted only if the full
prefix plus its normalized records reaches `after Tribes == 0x12ba3104` over 997,846
walked bytes.

On 2026-08-09 the read-only command ran against owned retail PID 13876 while it was idle
in the Friend Game staging screen and the controller was parked. The supported module base
was `0x00d60000`; `base + 0x00a7fa34` was `0x017dfa34`, whose value was
`0x0c61b9bc` both before and after the exact 36,480-byte read (`stable=1`). No retail
function was called and no lobby state was changed. The ignored address-bearing capture
has SHA-256 `39efa69914d0ce494d1d6824ac678004a673e01ccb5f8cad4f7b80e1a8a24a46`.

`schema/rules-channel-tribes.json` is the compact integration artifact emitted from that
admitted specimen. It contains 24-byte scalar slices and 1,408-byte graft slices for each
of 24 records—34,368 checksum-visible bytes total—and no vtables, pointers, strings,
padding, heap addresses, or other unwalked process memory. Its normalized-image SHA-256 is
`4a271dcca8a7c1223e61b9f58b4e5809f45f0fcfd43ce1b5f79a8c55dfde14bb`;
the concatenated walked payload SHA-256 is
`3110c6fb525ee25185c8dfb48d261be63bed386161f0350c345a015108b8f9bc`.

The independently recomputed cumulative result is:

| checkpoint | Adler-32 | cumulative bytes |
|---|---:|---:|
| after Types | `0x72e0c3b6` | 473,984 |
| after Constants | `0x50625668` | 477,380 |
| after Balance | `0x56daabc1` | 963,478 |
| after 24 Tribes | **`0x12ba3104`** | **997,846** |

The checked-in asset loader validates its exact schema, provenance boundary, record order,
base64 lengths, hashes, shape, and checkpoint declarations, reconstructs zeroed normalized
images, and reruns the complete checksum. `--check` now uses that asset by default.

The remaining concrete work is:

1. replace the gitignored Type and Balance specimens with lawful reproducible builders or
   similarly minimized derived inputs;
2. load the normalized Tribe asset from `don-replay` and set replay checksum channel 13
   from `after_tribes`;
3. retain the full `after_tribes == 0x12ba3104` admission check rather than accepting
   merely well-shaped assets.

## Commands

```sh
# Fast code sanity test.
python3 tools/rules-channel-assets.py --self-test

# Green today: exact Types selection, cache reconstruction, headers, bytes, and checkpoint.
# This also checks the exact Constants and Balance inputs and their cumulative checkpoints.
python3 tools/rules-channel-assets.py --check-types

# Green on this machine: recompute all four cumulative checkpoints from normalized inputs.
python3 tools/rules-channel-assets.py --check

# In the Windows guest, using the supported retail PID. This requests 24 * 0x5f0 bytes
# through the pointer global at preferred VA 0x00e7fa34 (RVA 0x00a7fa34).
donject peek PID riseofnations.exe a7fa34 1 0 8e80 > tribes-pidPID.txt

# On the host, place the raw ignored file under schema/live/ or pass any local path.
# This exits 0 only when every cumulative retail checkpoint matches.
python3 tools/rules-channel-assets.py --check \
  --tribes-capture schema/live/tribes-pidPID.txt \
  --emit-tribes schema/rules-channel-tribes.json

# Focused generated-specimen parser/normalization tests.
python3 -m unittest tools/test_rules_channel_assets_tribes.py

# Optional ignored integration artifact; no raw pointers/unwalked bytes are emitted.
python3 tools/rules-channel-assets.py --check-types --emit-types var/rules-channel/types.json
```

`--check` exits 1 on malformed or checkpoint-drifted evidence and 2 when a verified input
remains incomplete. The complete local specimen is green; this does not make the repository
self-contained because Type and Balance inputs remain local, and it does not license the raw
memory capture for check-in.
