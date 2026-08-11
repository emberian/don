# Retail save boundary: `Leaders::walk_data`

Status: **measured, source-only parser proof**. This lane decodes the complete
`Leaders` section of the fresh 2026-08-11 retail `.SVX`, beginning immediately after the
25-row `ObjectArray<Tribe>`. It deliberately stops at the first byte owned by
`Types::walk_data`; it does not join the save to the different-seed replay and does not
embed retail bytes.

The reusable proof is `re/scripts/savegame_leaders.py`. Its synthetic tests are
`re/scripts/test_savegame_leaders.py`.

## Exact splice

For the fresh save whose decompressed SHA-256 begins `fa31f89f`, Tribes ends at
`0x00009a5a`. The next complete owner is:

| owner | decompressed range | bytes | SHA-256 |
|---|---:|---:|---|
| `Leaders::walk_data` | `0x00009a5a..0x00026247` | 116,717 | `bfa6e7d9cdc7ce5f33a7272150e4083e37ee4953f38c1858f5fb522ca14020e7` |
| next owner, excluded | begins `0x00026247` | — | `Types::walk_data` `0x00669780` |

The opening byte is the `Leaders` tag `0x20`, followed by the global
`Leaders::prod_script_path` String `".\\ai\\scripts\\"`. Each of the eight rows begins
with the `LeaderData` tag `0xee` and the raw eight-byte
`leader_flags, leader_flags2` prefix. Rows 0–3 have bit zero set and therefore carry the
conditional body; rows 4–7 have `leader_flags=0x02000000` and end after those nine bytes.

The next byte happens also to be `0xee`; that observation is not used to find the
boundary. The end comes from consuming the exact PE traversal below. The following
function is independently pinned by the caller at `0x005a299d` and its PDB name.

## Instruction-derived grammar

`Leaders::walk_data` `0x006e38e0` emits:

1. one tag (`0x006e38fa`);
2. `String::walk_data` on global `0x00e7f8c8` (`0x006e3903`);
3. eight `LeaderData::walk_data` calls over `0x00e3a390`, stride `0x6eec`
   (`0x006e3910..0x006e3924`).

`LeaderData::walk_data` `0x006d6750` emits a tag and `+0x00..+0x08` for every row. If
`leader_flags & 1`, it then emits, in this exact order:

| stream item | bytes / grammar | PE site |
|---|---|---:|
| fixed `LeaderData` range | `+0x08..+0x692a`, 26,914 B | `0x006d6796` |
| eight `Diplomacy` rows | 8 × 92 B | `0x006d67a3..0x006d67b3` |
| `Personality` | 96 B | `0x006d67c7` |
| six bit buffers | each `{i32 bits, i32 size, u8 payload[size]}` | `0x006d67c9..0x006d68e0` |
| `Array<Site>` | container history + `length × 24` | `0x006d68f7` → `0x0047cee0` |
| `Array<MakeObject>` | container history + `length × 40` | `0x006d6904` → `0x0047d440` |
| three `SimpleArray<int>` | container history + `length × 4` | `0x006d6911..0x006d692b` |
| `prod_script` String | `u32 code_units`, UTF-16LE | `0x006d6937` |
| three bit buffers | same grammar as above | `0x006d693c..0x006d69c0` |
| deferred `data_encrypted` child | 62 deobfuscated i32 values, 248 B | `0x006d69d6` → `0x006d9900` |

The six prefix buffers are `tech`, `tech_at_start`, `obs_flags`,
`conquest_wonders`, `conquest_wonders_in_game`, and `conquest_racial_powers`. The three
suffix buffers are `rare`, `rare_owned`, and `rare_conquest`. A non-empty Array history
is `{i32 length, i32 capacity, i16 increment, u8 flags}`; a zero-length Array writes only
its length. The `make_list` stream precedes the three SimpleArrays even though its field
is later in the in-memory `LeaderData` layout. Treating memory order as stream order lands
in the middle of the first `MakeObject` payload.

`LeaderDataEncrypt::walk_data` writes nine resource words for each of six resources,
then `resource_cap[6]`, four `epoch` words, and `ages`, `epochs`, `discovered`: exactly
62 i32 values. It XOR-deobfuscates into stack temporaries before calling the visitor, so
the save stream contains semantic values, not the obfuscated in-memory words.

## Fresh specimen facts

The four active rows are:

| row | range | flags | who / tribe | sites | make list | mil trainers | new rares | oil patches | script |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| 0 | `0x09a79..0x10c56` | `0x00080007` | 0 / 22 | 10/10 | 11/11 | 1/10 | 0 | 2/10 | empty |
| 1 | `0x10c56..0x17e43` | `0x00000013` | 1 / 2 | 10/10 | 11/11 | 0 | 1/10 | 2/10 | `economic` |
| 2 | `0x17e43..0x1f032` | `0x00000013` | 2 / 13 | 10/10 | 11/11 | 0 | 1/10 | 2/10 | `defensive` |
| 3 | `0x1f032..0x26223` | `0x00000013` | 3 / 4 | 10/10 | 11/11 | 0 | 2/10 | 2/10 | `economic` |

Every non-empty `sites`/`make_list` history has `increment=-1, flags=0x80`. Every
non-empty SimpleArray has `increment=-1, flags=0`; their capacities are 10. The six
bit-buffer histories are `(806,101)` three times, `(17,3)` twice, and `(24,3)` once; the
three rare histories are all `(44,6)`.

Rows 4–7 occupy `0x26223..0x26247`, nine bytes each, and have no conditional children.

## Reproduction

No retail artifact is required by the committed test:

```sh
python3 re/scripts/test_savegame_leaders.py
```

Against the local fresh save:

```sh
python3 re/scripts/savegame_leaders.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x9a5a
```

The helper accepts gzip or plain input, checks both tag bytes, validates all dynamic
length/capacity/bit histories, and reports only derived fields and hashes. Its returned
`end` is the exact splice that the main save parser can adopt after its Tribes lane is
frozen.
