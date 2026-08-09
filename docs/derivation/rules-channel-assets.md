# Rules-channel checked-in input audit

Status: **the existing local Type, Constants, and Balance inputs reproduce all three
retail prefix checkpoints. A fail-closed path is ready for the remaining narrow Tribe
capture, but that capture has not yet been made.** The full P0 gate therefore remains red.

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

## Tribe result and blocker

PDB layout is exact: `sizeof(Tribe) == 0x5f0`, with `graft: TypeIndex[352]` at `+0x70`.
The checksum visits six dwords at `[+0x54,+0x6c)` and the graft array at
`[+0x70,+0x5f0)`, for 34,368 bytes across 24 records.

`ron-data/rules.xml` names all 24 nation files, but those files are absent from the local
data set. `Tribes::init` establishes an identity graft baseline, while nation parsing and
substitution mutate the checksum-visible scalar/graft state. `unitrules.xml` contains
`GRAFT` and `TRIBE_MASK`, but neither the loader derivation nor a value-level comparison
establishes that those two columns alone reproduce the final 24 arrays. The tool therefore
does not manufacture them. No checked-in capture contains their values.

`--tribes-capture PATH` ingests the existing address-bearing `donject peek` text format.
It requires a `base`/`addr`/`len` header, exactly `0x8e80` bytes, contiguous addressed
hex rows, a plausible x86 image base, and a valid target address range. If a later header
also includes `rva`, `deref`, `off`, root-pointer address, module name, executable hash,
or root stability, the parser validates each field against the supported binary and the
measured Tribe pointer at preferred VA `0x00e7fa34` (RVA `0x00a7fa34`). The current
three-field `donject` header does not encode those request fields, so provenance also
depends on retaining the exact capture command below.

Immediately after parsing, every record is replaced by a zeroed `0x5f0` image containing
only `[+0x54,+0x6c)` and `[+0x70,+0x5f0)`. Vtables, pointers, strings, padding, and all
other unwalked process bytes do not survive ingestion. The raw capture remains an ignored
local input and must not be committed. A supplied specimen is admitted only if the full
prefix plus its normalized records reaches `after Tribes == 0x12ba3104` over 997,846
walked bytes.

The remaining concrete work is:

1. make the prepared narrow read-only capture, or supply the 24 shipped nation XML files
   named in `rules.xml` and implement their exact loader/substitution order;
2. compare every reconstructed value to that independent specimen;
3. store a lawful derived representation or a reproducible builder, not raw process
   memory;
4. require the final `after_tribes == 0x12ba3104` admission check.

## Commands

```sh
# Fast code sanity test.
python3 tools/rules-channel-assets.py --self-test

# Green today: exact Types selection, cache reconstruction, headers, bytes, and checkpoint.
# This also checks the exact Constants and Balance inputs and their cumulative checkpoints.
python3 tools/rules-channel-assets.py --check-types

# Intentionally exits 2 today and prints the explicit Tribe blockers.
python3 tools/rules-channel-assets.py --check

# In the Windows guest, using the supported retail PID. This requests 24 * 0x5f0 bytes
# through the pointer global at preferred VA 0x00e7fa34 (RVA 0x00a7fa34).
donject peek PID riseofnations.exe a7fa34 1 0 8e80 > tribes-pidPID.txt

# On the host, place the raw ignored file under schema/live/ or pass any local path.
# This exits 0 only when every cumulative retail checkpoint matches.
python3 tools/rules-channel-assets.py --check \
  --tribes-capture schema/live/tribes-pidPID.txt

# Focused generated-specimen parser/normalization tests.
python3 -m unittest tools/test_rules_channel_assets_tribes.py

# Optional ignored integration artifact; no raw pointers/unwalked bytes are emitted.
python3 tools/rules-channel-assets.py --check-types --emit-types var/rules-channel/types.json
```

`--check` exits 1 on malformed or checkpoint-drifted evidence and 2 when a verified input
remains incomplete. A local capture reaching the final checkpoint makes this specimen gate
green; it does not make the repository self-contained or license raw memory for check-in.
