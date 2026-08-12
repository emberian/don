# Retail save boundary: Forms

Status: **complete caller tag plus concrete ObjectArray owner**.  This lane
begins at the caller-emitted Forms walk-test immediately after Cities, consumes
the complete `ObjectArray<Form>::walk_data` image, and stops at the next native
owner, `PtrArray<Good>::walk_data`.  It does not edit the shared parser or
normalize allocation history.

The exclusive helper is `re/scripts/savegame_forms.py`; exhaustive tests are
in `re/scripts/test_savegame_forms.py`.

## Fresh-SVX splice

The user-created SVX has compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
decompressed SHA-256
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
Chaining the exclusive parsers from Leaders through Cities reaches:

| stream range | exact owner | fresh value |
|---|---|---:|
| `0x27040..0x27041` | caller `walk_test(StringTable[2690])` | `0x00` |
| `0x27041..0x27045` | `ObjectArray<Form>.length` | 0 |
| next owner at `0x27045` | `PtrArray<Good>::walk_data` `0x0045cce0` | excluded |

The exact five-byte fresh image is all zero and has SHA-256
`8855508aade16ec573d21e6a485dfd0a7624085c1a14b5ecdd6485de0c6839a4`.
The empty branch owns no capacity, increment, flags, or rows.

The separate RCX SHA-256 is
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`.
The test independently confirms the fresh SVX seed `0x014810ac` and RCX seed
`0x007f93e0`; no state or identity is joined across those artifacts.

## Exact caller and container grammar

`WalkDataGame::walk_data` inlines the two operations otherwise performed by
`Forms::walk_data` `0x0072e970`:

1. at `0x005a2ade..0x005a2af3`, emit the walk-test whose StringTable byte
   offset is `0xd228`; `sizeof(String) == 20`, so this is index 2690;
2. call `ObjectArray<Form>::walk_data` `0x00481190` at `0x005a2af5`.

The concrete container grammar is:

```text
u8  Forms tag                       # StringTable[2690]
i32 length
if length != 0:
    i32 capacity
    i16 increment
    u8  flags                       # writer persistently clears bit 0x40
    Form rows[length]
```

`ObjectArray<Form>` is a contiguous value array, not a pointer array.  It has
no pointer-presence plane and no repeated capacity/increment pass.  The parser
rejects negative or unreasonable lengths, `capacity < length`, and the
writer-cleared flag bit, while preserving every valid capacity/increment value
exactly.  A synthetic `length=1, capacity=99, increment=-17` image remains 99
and -17; it is not rewritten to host-container defaults.

After the function returns, profiling owns no stream bytes.  The caller next
invokes `PtrArray<Good>::walk_data` at `0x005a2b0b`, defining the exact end.

## Complete Form row

For every logical row, `ObjectArray<Form>::walk_data` emits exactly the same
sequence as `Form::walk_data` `0x0072df10`:

```text
u8  Form tag                        # walk_test(StringTable[2694])
String name                         # u32 code-unit count + UTF-16LE units
String desc                         # same grammar
bytes Form[+0x28..+0xe90)           # 3,688 bytes
```

The row walk-test byte offset is `0xd278`, hence StringTable index 2694.  The
fresh specimen has no Form rows, so its numeric tag value is not guessed; the
helper preserves any byte at the PE-proven location.

`String::walk_data` at `0x00a1b2d0` widens the PDB's unsigned-short
`curr_len` to a four-byte stream count and then emits exactly that many
UTF-16LE code units, without a terminator.  The helper preserves raw code
units and rejects counts beyond `0xffff`.

The final direct range begins immediately after the two Strings and contains
all fixed Form state after them.  Its 922 signed 32-bit words are decoded into
the PDB fields:

| Form range | field | type / count |
|---|---|---|
| `+0x028..+0x03c` | `form`, `density`, `o`, `idx`, `who` | 5 × `int` |
| `+0x03c..+0x114` | `num_category`, `x_spacing`, `y_spacing` | 3 × `int[18]` |
| `+0x114..+0x314` | `cat_id` | `int[128]` |
| `+0x314..+0x514` | `category` | `FormCatIndex[128]` |
| `+0x514..+0xd14` | `to_x`, `to_y`, `off_x`, `off_y` | 4 × `Coord[128]` |
| `+0xd14..+0xd20` | `wedge`, `total`, `guarding` | 3 × `int` |
| `+0xd20..+0xd68` | `per` | `int[18]` |
| `+0xd68..+0xe88` | `space` | `int[4][18]` |
| `+0xe88..+0xe90` | `reverse`, `across` | 2 × `int` |

There are no gaps in `[+0x28,+0xe90)`.  The `GameAccess` virtual-base pointer
at `+0xe90` and trailing compiler bytes through `sizeof(Form)==0xe98` are not
walked.

## PDB and executable receipts

The matching PDB has SHA-256
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`,
GUID `51D4F219-61C6-4F84-9D5B-C3361B0D291F`, age 1.  Its JSON export has
SHA-256
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The helper loads it at runtime and requires:

- `sizeof(ObjectArray<Form>) == 24`, with exact length, size, increment,
  list, and flags fields;
- `sizeof(Form) == 3736` and `sizeof(FormData) == 3728`;
- the two 20-byte Strings at `+0` and `+20`;
- every named field covering `[+0x28,+0xe90)` without a gap; and
- `sizeof(String) == 20`, with unsigned-short `curr_len` at `+8`.

The canonical layout receipt has SHA-256
`c84580b8b69cc7606cb6a792a7c0ebc452e68601dd925d009d326004e98bc818`.

The matched PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Tests freeze complete spans for `ObjectArray<Form>::walk_data`,
`Form::walk_data`, `Forms::walk_data`, `Forms::init`, `String::walk_data`, the
main caller sequence, and the following Goods walker.

## Independent formation-authority comparison

`Forms::init` `0x0072e9a0` independently requires exactly ten rules formations,
reserves at least ten contiguous Form values, and calls `make_valid(9)`, giving
logical length ten.  Its constructor state supplies capacity ten, increment
-1, and flags zero for the canonical initial container.

The landed group authority independently defines the ten formation indices in
rules order: Line 0, Refused 1, Envelop 2, EchelonRight 3, EchelonLeft 4,
Sparse 5, Square 6, Wedge 7, Column 8, and Mob 9.  The comparison test builds a
ten-row container with `FormData::form` 0 through 9 and checks that shape and
index order against `groups_guys::Formation`.

That is deliberately only a shape/index cross-check.  The existing group
authority does not own a canonical serialized global Form pool, its two
Strings, or the complete 922-word per-row dynamic state.  Therefore it is not
used to invent fresh bytes, replace retail capacity history, or claim a save/
checksum projection that no checksum channel actually walks.

## Mutation and boundary proof

The nonempty fixture includes one arbitrary per-row tag, two nonempty UTF-16
Strings, and every one of the 3,688 fixed bytes.  Every owned-byte one-bit
mutation either violates a structural invariant or changes the parsed receipt
and digest.  Every truncation is rejected.  Dedicated tests cover invalid
outer tag/length/capacity/flags, impossible String counts, exact history/tag
preservation, and mutation exclusion at the first Goods byte.

## Reproduction

```sh
python3 re/scripts/test_savegame_forms.py

python3 re/scripts/savegame_forms.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x27040
```

The returned `end` is the exact start of `PtrArray<Good>::walk_data`.
