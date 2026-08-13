# Retail save boundary: `Options::walk_data`

Status: **measured complete owner**. The isolated helper
`re/scripts/savegame_options.py` consumes the inherited, untagged
`Array<Option>` first, then Options' own tag and both exact direct images. It
stops before `CommandManager::walk_data`. Exhaustive gates live in
`re/scripts/test_savegame_options.py`; no shared parser is changed.

## Exact installed boundary

The fresh SVX compressed/decompressed SHA-256 values are
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.

| owner | stream range | bytes | SHA-256 |
|---|---:|---:|---|
| inherited `Array<Option>.length` | `0x2c23d..0x2c241` | 4 | `df3f619804a92fdb4057192dc43dd748ea778adc52bc498ce80524c014b81119` |
| Options tag, StringTable[5077] | `0x2c241..0x2c242` | 1 | `6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d` |
| Options `[+0x20,+0x7c)` | `0x2c242..0x2c29e` | 92 | `62b14867e4e79d50673d2f7474335229f54c478f56d2a910235e1953c6d29206` |
| embedded `opt[+0,+0x12)` | `0x2c29e..0x2c2b0` | 18 | `60daa3a5f7dbfa200f8c82840ecf5b42640b70f3b7218a4c6bbd67db542e75a4` |
| complete Options owner | `0x2c23d..0x2c2b0` | 115 | `23cd67852af04fd6885d2763266f2765b5e03c6ae3a5c1c6c95f7e03e10ec10d` |
| next `CommandManager::walk_data` | begins `0x2c2b0` | variable | excluded |

The fresh inherited array is empty, so it emits only its signed zero length.
Every installed Options byte is zero. The nonempty array and signed field
grammar are independently exercised by the synthetic fixture rather than
inferred from that zero image.

## Complete grammar

Options inherits `Array<Option>` at object offset zero. Its array walker runs
before Options emits a tag:

```text
i32 length
if length != 0:
    i32 capacity
    i16 increment
    u8  flags                     # writer clears bit 0x40
    Option row[length]

Option row:
    i32 option
    i32 object
    i32 count
    i32 disable
    i8  grid_x
    i8  grid_y

u8  walk_test(StringTable[5077])
i32 Options[+0x20,+0x7c)         # 23 words
Option Options.opt[+0,+0x12)
```

The 23-word direct image contains, in order, `num`, `selecting_spot`,
`editor_unit_move_drag`, `editor_unit_rotate_drag`, all 16
`cycle_research` entries, `cycle_index`, `mode`, and `mode_data`. The helper
preserves all words as signed integers and both grid coordinates as signed
bytes.

## PE evidence and next owner

| body | VA | bytes | SHA-256 |
|---|---:|---:|---|
| `Options::walk_data` | `0x0072c240` | 80 | `5173806c509cd96f5f5b0a9f43a45e188c6d9974f23d6e8a5412bfb5a92d42ed` |
| `Array<Option>::walk_data` | `0x00480cc0` | 487 | `8af883a3db8cb42c004123fec9e4baac0171f00cb12bb1358695320b4d0b6f29` |
| `Option::walk_data` | `0x0072be20` | 23 | `c62c3eeac8657ca872cec51e3eb538377c795e0326ecc0fd1e0e8fa37e356ccc` |
| inlined Options caller owner | `0x005a2f8e` | 61 | `25f556c70a83a7a48a666dbc574ffe4ea408861dc674e3e42d3088cf22e5580f` |
| next `CommandManager::walk_data` | `0x00942d30` | 222 | `bfe3ff7f3e2bc9468aeb7187bf71bc2ee15281b73dae6e189a672d2478d2cb96` |

The standalone body and the inlined `WalkDataGame::walk_data` copy agree on
the inherited-array-first order and both direct endpoints. After Options, the
caller invokes nonserializing `NetDaemon::process_all`, then calls
`CommandManager::walk_data`; there is no intervening stream tag.

The matched executable SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.

## PDB ownership and exclusions

The matched PDB/schema SHA-256 values are
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`
and
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
PDB layout fixes Options at 232 bytes, its `Array<Option>` base at `+0`
(28 bytes), `GameAccessConst` at `+32`, and a virtual `MiscAccess` base through
the vbptr at `+28`. `Option` is 20 bytes, but its walker deliberately emits
only the six declared fields in `[+0,+0x12)`.

The owner deliberately excludes:

- the array vftable, allocation pointer, and `cur_index`;
- capacity, increment, and flags when the array length is zero;
- each Option row's two bytes of tail padding;
- the Options vbptr and virtual base machinery;
- every Options field at and after `rebuild` (`+0x90`), including
  `cycle_opt`, drop/place/draw state, `shortcut_string`, and
  `current_opt_obj`.

The deterministic layout receipt is
`5af595e0d291bf8abde2989141c0f57fd70ee323eaacf3999c7c45100f426ff6`.

## Gates

The synthetic image contains a three-row array with nondefault allocation
history, signed Option values, all 23 direct words, the embedded selected
Option, and a following CommandManager sentinel. Tests mutate every owned
byte, reject every truncation and malformed array history, check the tag,
kill a mutated PDB size, freeze the five PE bodies above, and prove every
following byte is excluded. The installed gate chains every owner from World
through SelectGroups, reaches Options at `0x2c23d`, lands on CommandManager at
`0x2c2b0`, and checks an independent RCX seed.

## Reproduction

```sh
python3 re/scripts/test_savegame_options.py
python3 re/scripts/savegame_options.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x2c23d
```

An isolated worktree without copied retail assets can point the installed
gate at a populated checkout with `DON_RETAIL_ROOT=/path/to/don`.

The returned end, `0x2c2b0`, is the exact first byte owned by CommandManager.
