# Retail save boundary: `ConquestGame::walk_data`

Status: **measured complete dynamic owner**. The isolated helper
`re/scripts/savegame_conquest_game.py` starts at the exact end returned by
UnbuiltForts, implements all 45 ordered traversal phases in the shipped body,
and stops at the caller's separate `detail_threshold` word. Its complete
dynamic, mutation, truncation, PE/PDB, fresh-SVX, and RCX gates are in
`re/scripts/test_savegame_conquest_game.py`; no shared parser is changed.

## Exact fresh splice

The fresh SVX compressed/decompressed SHA-256 values are
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.

| owner | decompressed range | bytes | SHA-256 |
|---|---:|---:|---|
| `ConquestGame::walk_data` | `0x2bcef..0x2bf77` | 648 | `f4bd841308415de6ed2727462cd66a7333ac8155b4e8e95de0220355189c785c` |
| next direct `detail_threshold` word | `0x2bf77..0x2bf7b` | 4 | `df3f619804a92fdb4057192dc43dd748ea778adc52bc498ce80524c014b81119` |

Every byte in the fresh Conquest owner is zero, but the 648-byte size is not a
zero-run inference. It is the exact sum of the executable's tag, 388-byte
direct prefix, all empty container/string/mask images, the 24 empty Conquest
piece pointer arrays, and its final array. The caller's following four-byte
walk explicitly addresses global float `detail_threshold` at `0x00c0623c`.
Mutating that first following byte leaves the Conquest result unchanged.

## Top-level executable order

The complete `ConquestGame::walk_data` body is VA `0x00798410`, 957 bytes,
SHA-256 `c8a3801f2ef2c4affb5014b2e400bff4f7b5ede2c34daba15a1903dfe6569c54`.
Its save order is:

```text
u8 tag                                  # StringTable[995]
bytes ConquestGame +4..+392             # 388 bytes, ends starting_round
Array<Color> global color history       # rows are Color[0..10), stride 12
u8 leaders tag                          # StringTable[1140]
ObjectArray<ConquestLeader>
u8 nodes tag                            # StringTable[1299]
ObjectArray<ConquestNode>
ObjectArray<ConquestColony>
ObjectArray<String> conquest_continents
ObjectArray<String> barbarian_files
String x10                              # +568 through +748
SimpleArray<float> map_size_scale
ConquestPieces                          # 24 PtrArray<ConquestPiece>
Array<ReinforcementArmy>
DynamicBitMask help_pointers_shown
ObjectArray<String> conquest_news strings
Array<ConquestNewsItem>
LinkList<int,short> valid_tribes
SimpleArray<int> overrun_armies
SimpleArray<int> continents_captured
ObjectArray<String> bonus_card_deck
ObjectArray<ObjectArray<ConquestStyle>> game_styles
NamedSimpleArray<int> stored_ints
NamedObjectArray<String> stored_strs
NamedSimpleArray<int> diplo_deals
u8 tribes tag                           # StringTable[6937]
ObjectArray<Tribe>
String x8                               # +1912 through +2052
SimpleArray<int> allied_punks            # last owned phase, +2072
```

Nothing at `ConquestGame+2100` or later is called by this walker. In
particular, the many country, category, build-out, and UI/runtime fields through
`sizeof(ConquestGame)==5308` do not enter this save owner.

## Container and child grammars

All non-pointer arrays write `i32 length`; live arrays then write `i32
capacity`, `i16 increment`, `u8 flags` with writer bit `0x40` cleared, followed
by logical rows. Object arrays dispatch each row's walker. Simple arrays copy
the element bytes. Named integer arrays write their integer plane followed by
one String name per row; named String arrays write the value Strings followed
by the name Strings.

Pointer arrays additionally write a Boolean presence byte per logical slot,
then repeat capacity/increment, then dispatch only present rows. This applies to
all 24 `PtrArray<ConquestPiece>` lists and to each ConquestStyle info-text list.
Each ConquestPiece excludes its virtual-base pointer and saves `+4..+48` as
32 direct state bytes followed by its 12-byte vector. StringListEntry saves two
Strings followed by its 12-byte integer tail.

The child parsers cover every reachable live-row branch:

- `ConquestLeader`: tag, 843-byte exact direct prefix, bonus-card object array,
  five integer arrays, String, three DynamicBitMasks, and three tail arrays;
- `ConquestNode`: tag, two direct ranges totaling 80 bytes, three Strings, and
  ConquestLink rows; each link owns a 24-byte prefix and five integer arrays;
- `ConquestColony`: 16 direct bytes, integer array, and two Strings;
- `ReinforcementArmy`: `+4..+36`, excluding its virtual-base pointer;
- `ConquestNewsItem`: all 24 bytes;
- `ConquestStyle`: tag, 140 direct bytes, ten Strings, four-byte change count,
  and pointer-array StringListEntry rows;
- `Tribe`: tag plus the exact 24-byte identity block and 1,408-byte graft
  table, totaling 1,432 direct bytes after the tag.

Strings are `i32` UTF-16 code-unit count followed by exactly `count*2` bytes.
DynamicBitMask is `i32 bits`, `i32 size`, then exactly `size` payload bytes.
The simple linked list is `i32 length`, then each node's two-byte metric and
four-byte integer data.

## PE/PDB receipts

The matched PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Besides the top body, the gates freeze the complete ConquestPieces, Leader,
Node, Colony, Link, Style, and Tribe bodies and every specialized container
body used by the traversal. Representative hashes include:

| body | VA | bytes | SHA-256 |
|---|---:|---:|---|
| `ConquestPieces::walk_data` | `0x007accb0` | 957 | `bd5cb0bad6c353f5dc8e515431ea9c52c297528ab9bcbb562f072e98dbbb1a93` |
| `ConquestLeader::walk_data` | `0x0079b6d0` | 349 | `003522c2e99e7ebe5d911b18a3443eb98b31fdbe968bf3ebf30ce8834d265eea` |
| `ConquestNode::walk_data` | `0x007a5520` | 103 | `15da7fd88b730cf867b2b905fe637e1e62ed8934b9c236b266459549e16be42a` |
| `ConquestStyle::walk_data` | `0x007a9170` | 205 | `9ae6a1be1f199ee8f1a5219467983d73e393949b24365438884bde9825047478` |
| `ObjectArray<ConquestNode>::walk_data` | `0x00493a40` | 541 | `0f330fac3f5ec6fa4e21985a60e098386a075971163bf6fe2cfc05cf2c7fb289` |
| `PtrArray` StringListEntry walker | `0x004940d0` | 832 | `441880d1b9b3ff58ebf708432e9ccf036bd5925dee4d671e47b4c17912c4479b` |

The matched PDB/schema SHA-256 values are
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5` and
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The helper validates 33 compiler types: the owner and every saved child and
specialized container. It explicitly freezes the first excluded
`conquest_countries` field at `+2100`. The deterministic layout receipt is
`f757a494435953871928f846c4e6c340579095e36cf40e08bf1f29ecabb8364a`.

## Exhaustive proof

The synthetic image activates every top-level dynamic family: two Color rows,
a complete Leader, Node with Link, Colony, Strings, float bit patterns,
present/absent ConquestPiece slots, ReinforcementArmy, nonempty masks, news,
linked-list rows, nested Style with present/absent info-text pointers, all three
named containers, a full Tribe, and the final array. It exercises allocation
history, signed increments, presence planes, duplicated pointer histories, and
variable-length payloads.

Every owned byte is mutated independently. Each mutation is rejected or changes
the immutable section and digest. Every truncation fails. Dedicated gates
reject invalid counts/capacities/flags, non-Boolean pointer presence, repeated
history disagreement, a wrong top tag, and a mutated PDB owner size. Mutating
`detail_threshold` does not affect the parse.

The installed gate chains World → GameDaemon → game_random → GraphicEvents →
Scene → Farms → UnbuiltWonders → UnbuiltCities → UnbuiltForts → ConquestGame
using only returned ends. It reaches `0x2bf77` and independently checks fresh
SVX seed `0x014810ac` and RCX seed `0x007f93e0`; no replay bytes infer save
structure.

## Reproduction

```sh
python3 re/scripts/test_savegame_conquest_game.py
python3 re/scripts/savegame_conquest_game.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x2bcef
```

The returned end is the exact first byte of the caller's separate
`detail_threshold` walk.
