# Retail save boundary: OptionInfo

Status: **complete fixed 331-row localized-string table**. This lane begins
at `OptionInfo::walk_data`, consumes its outer tag and the exact save
projection of all 331 positional `OptionData` rows, and stops before the
caller's next direct 108-byte block. It is an exclusive parser/test/doc
tranche and does not touch shared Rust.

## Fresh-SVX splice

The fresh SVX compressed and decompressed SHA-256 values are
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
The exact OptionInfo stage is:

| range | owner | fresh value |
|---|---|---:|
| `0x2725a..0x2725b` | outer tag, StringTable[5073] | 0 |
| `0x2725b..0x27dfe` | 331 nine-byte empty-string rows | all 0 |
| next at `0x27dfe` | caller direct block `0x00e85e98..0x00e85f04` | excluded |

Each fresh row has a zero row tag followed by two zero u32 string lengths.
The exact 2,980-byte section SHA-256 is
`646ae72e39ac99003e8171cc17013d6da2017b6a477cff68c5f62aa9bdc65c0a`.
The test chains every landed helper through LeaderOptions to reach `0x2725a`,
then proves that mutating `0x27dfe` cannot affect OptionInfo. SVX seed
`0x014810ac` and RCX seed `0x007f93e0` remain distinct; RCX SHA-256 is
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`.

## Exact grammar

`OptionInfo::walk_data` `0x0072c1e0` emits the outer `walk_test` at
StringTable byte offset `0x18c54`, exact index 5073, then advances through
331 PDB `OptionData` objects at a fixed 68-byte stride with no saved count:

```text
u8 OptionInfo tag                    # StringTable[5073]
repeat exactly 331 times:
    u8 OptionData tag                # StringTable[5072]
    String name
    String desc

String:
    u32 code_unit_length
    u16 utf16le_code_units[code_unit_length]
```

The row `walk_test` uses StringTable byte offset `0x18c40`, exact index 5072.
Within the global array the executable starts at row +40, calls
`String::walk_data` at `cursor-20` (row +20, `name`) and `cursor` (row +40,
`desc`), then adds 68. Independent `OptionData::walk_data` `0x0072bf70`
proves the same tag and +20/+40 calls.

## Walked and excluded PDB state

PDB `OptionInfo` is exactly `OptionData[331]`, 22,508 bytes. Each
`OptionData` is:

| PDB range | field | save behavior |
|---|---|---|
| +0..+20 | `english_name` String | excluded |
| +20..+40 | `name` String | walked dynamically |
| +40..+60 | `desc` String | walked dynamically |
| +60..+64 | `tex_x`, `tex_y` | excluded |
| +64..+68 | `tex_col`, `tex_row`, `tex_clip`, `tex_id` | excluded |

Only each walked String's current logical length and addressed UTF-16 payload
are serialized. String pointers/const aliases, offsets, flags, module id, and
cached hashes are excluded. The parser caps the stream length at the PDB
`curr_len` u16 width and preserves arbitrary UTF-16 code units—including
embedded zeroes and unpaired surrogates—without decoding or normalization.
Row tags are receipts, not inferred delimiters.

## Next owner and frozen evidence

After OptionInfo and profiling, caller `0x005a2c17` directly walks the 108
bytes at PE globals `0x00e85e98..0x00e85f04`; after that it invokes
`Array<Group>::walk_data` `0x0047ea30`. The first byte of that direct block
defines the exact OptionInfo boundary `0x27dfe`.

The matching PDB and schema-export SHA-256 values are
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`
and `399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The three-class layout receipt is
`2a78db2e1b061c5335b953d0c59bf4fa80b7ae3bac7c796ff29b00adfecad90c`.
The matched PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Tests freeze OptionInfo, OptionData, String, the caller/direct-block handoff,
and the following Array<Group> walker.

The synthetic fixture has all 331 positional rows, distinct wrapping tag
bytes, independently empty/nonempty `name` and `desc` strings, ordinary and
non-ASCII units, embedded zeroes, and unpaired surrogates. Every owned-byte
mutation fails or changes the receipt, every truncation is rejected, and
dedicated tests cover both string-length constraints and exact next-owner
exclusion.

## Reproduction

```sh
python3 re/scripts/test_savegame_option_info.py

python3 re/scripts/savegame_option_info.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x2725a
```

The returned `end` is the exact start of the caller's direct 108-byte block.
