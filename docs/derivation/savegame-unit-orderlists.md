# Fresh SVX Unit OrderList localization

Status: **exclusive bounded parser/evidence tranche; no runtime or closure edit**.

The fresh retail v16 save contains a positive Unit order queue.  Starting from the exact
end of the already localized Groups owner, the retail walk reaches owner-zero object slot
0 and serializes one `EXPLORE_TO` node.  Its `OrderList` is
`0x5058c..0x505e2` (86 bytes), SHA-256
`2cea2ffb1e52ff2077982ee3004fcd02c74fda6bf512c3bf5e5b78c8c31fbfc7`.
This is the first direct retail-save witness for the node metric and a complete concrete
order payload in this engine.

The parser is `re/scripts/savegame_unit_orderlists.py`; its mutation and artifact tests are
`re/scripts/test_savegame_unit_orderlists.py`.  It does not scan for any tag or fit a byte
pattern.  The caller supplies the `Objects::walk_data` boundary, and the parser follows the
container and inherited-object grammar in program order.

## Structure-derived route

`WalkDataGame::walk_data` calls the Group array, writes the eight `last_group` words and
`proc_group`, then calls `Objects::walk_data` at `0x005a2c7e`.  The previous Groups tranche
ends at `0x4f21f`, so that byte is the independently supplied Objects boundary.

The exact route is:

```text
Groups end / Objects tag 0x4f21f
  -> Objects fixed ranges
  -> owner[0] MultiPtrArray<Object>
       -> length/capacity/increment/flags
       -> 3,000-byte presence plane
       -> one i32 concrete type for each present slot
       -> duplicated capacity/increment image
       -> first present body: Unit slot 0
  -> SubObject::walk_data
  -> Object::walk_data
  -> Unit::walk_data
       -> Stack<PathData>::walk_data
       -> OrderList::walk_data
       -> stop before PtrArray<Guy>::walk_data
```

The PE/PDB anchors, complete body sizes, and body hashes are:

| VA | body | bytes | SHA-256 |
|---:|---|---:|---|
| `0x006541e0` | `Objects::walk_data` | 469 | `f5c338b7e518b05f9c192545773cdf092d9d11764bdb26c5e00dec01710ff8e8` |
| `0x0045d550` | `MultiPtrArray<Object>::walk_data` | 907 | `675810f0a1ef4dbb3d7e44c50784dcf4df60699b19a19c05c07fd86eaf790672` |
| `0x006621d0` | `SubObject::walk_data` | 216 | `21da7fd9c4c463d308a7a8ec90cd867befb03d21920dd953e3c24cc8a74f51a1` |
| `0x00647830` | `Object::walk_data` | 256 | `49e892d0bf7bbe455339c55ba20bf6ed2be162e0958bdaffda644ac3526bb152` |
| `0x0060cf40` | `Unit::walk_data` | 248 | `a4b6e96d3c349dcd2b65f5256c1373ac8e16ddd544cf443d7f255f76ce0a12c7` |
| `0x0046d8b0` | `Stack<PathData>::walk_data` | 239 | `457efeb27bdd357c9e0b4d1f45168a220617944683a0d62b0782c1aaa306c243` |
| `0x00730270` | `OrderList::walk_data` | 391 | `34db4525635f25073be363792643145f6ece881b52bc3ade20ee6a669c176d2d` |
| `0x00482e70` | shared `MoveOrder::walk_data` | 56 | `3ba6e2fbcb0af227d2e63b4fac56739160e87cf8d1bcb7115d0fbd566e22a45b` |

The matched executable SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
The tests map every VA through the PE section table and hash each whole procedure span.

## Exact fresh-save boundaries

The source artifact is
`new save game 2026.08.11 15'42'57 (Tue).SVX`, compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7`
and decompressed SHA-256
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.

| stream range | retail owner | observed fact |
|---|---|---|
| `0x4f21f..0x4f220` | Objects `walk_test` | tag `0x7c` |
| `0x4f220..0x4f2ae` | Objects fixed walks | 142 bytes |
| `0x4f2ae..0x4f2b9` | owner-0 MultiPtrArray header | length/capacity 3000, increment -1, flags 0 |
| `0x4f2b9..0x4fe71` | presence plane | 400 present, 2,600 null |
| `0x4fe71..0x504b1` | concrete types | 200 Unit type 0, then 200 Build type 1 |
| `0x504b1..0x504b7` | duplicated container history | capacity 3000, increment -1 |
| `0x504b7..0x504cd` | SubObject walk | Unit identity `(who=0,o=0)`, ptype index 69 |
| `0x504cd..0x504f2` | Object walk | launching pointer absent |
| `0x504f2..0x50563` | Unit tag/gate/fixed window | tag `0x12`, gate true, 111 fixed bytes |
| `0x50563..0x5058c` | PathData stack | capacity 30, length 2, increment 10 |
| `0x5058c..0x505e2` | OrderList | one complete node |
| `0x505e2` | next owner | `PtrArray<Guy>` begins with length 2 |

The bounded parser stops at `0x505e2`; it does not consume or infer Guys.  It also refuses
a present `Object::launching` array, a non-Unit first concrete body, invalid container
history, false inherited walk gates, and every concrete order class whose payload grammar
is not yet mounted.

## Exact OrderList and MoveOrder image

`OrderList::walk_data` writes a signed i32 count, then iterates the circular list in retail
execution order.  For each node it writes:

```text
concrete OrderIndex : i32
RecycledOrderNode.metric : u8
UnitOrder.flags : u8
concrete payload fields
```

The fresh node is:

| field | offset | value |
|---|---:|---:|
| count | `0x5058c` | 1 |
| concrete type | `0x50590` | 3 (`EXPLORE_TO`) |
| node metric | `0x50594` | 0 |
| inherited UnitOrder flags | `0x50595` | `0x01` (`ORDER_PATHED`) |
| MoveOrder fixed payload | `0x50596..0x505e2` | 76 bytes; SHA-256 below |

The fixed MoveOrder payload SHA-256 is
`ed0f47e809ea729dbc48679abaaad1df5b96d922898e43a7d3292c1ee4860c5f`.

There is no concrete payload tag in this retail node image.  `MoveOrder` virtually inherits
`UnitOrder`; the first call in `MoveOrder::walk_data` resolves that virtual base through the
vbtable and writes its `flags` byte.  The second call writes the PDB's 76-byte `MoveOrder`
field range at concrete offsets `+0x04..+0x50`: 18 i32 values followed by two i16 offsets.
In field order the specimen is:

```text
x=48888, y=30456, angle=-1550974976, dest=1, tolerance=0,
pause=0, retry=0, attempts=0, timer=0, facing=1,
dest_x=50424, dest_y=29688, last_x=-1, last_y=-1,
coll_x=0, coll_y=0, orig_x=48864, orig_y=30432,
off_x=504, off_y=504
```

The shared body at `0x00482e70` serves MOVE_TO, ATTACK_TO, EXPLORE_TO, and FLEE_TO.
The parser supports exactly that four-type payload family.  It preserves `metric` as the
PDB's unsigned byte and `flags` as the PDB's `char`; a synthetic witness pins metric `0xfe`
and flags `0xa5` so no signed conversion, zeroing, or invented tag check can hide either
value.

## Canonical save-owner implications

This evidence confirms the current model's decision that the node metric belongs outside
the concrete `Order`, immediately before its payload.  It also establishes three retail
format facts that the internal DoNSave v13 envelope must not be confused with:

- retail writes `OrderIndex` as i32, while internal DoNSave begins its order record with a
  compact u8 kind;
- retail writes `UnitOrder::flags` immediately after the node metric and has no payload tag
  at that boundary; DoNSave's later payload tag/version pair is an internal discriminator;
  and
- the shared retail MoveOrder body contains `x`, `y`, and `tolerance` alongside all retry,
  facing, origin, collision, and offset fields, while DoNSave splits those values between
  its generic order header and typed movement payload.

DoNSave v13 can remain the canonical internal owner, and its `node_metric: u8` placement is
semantically validated.  It is not byte-compatible with a retail `OrderList` and therefore
cannot import or emit SVX order streams without a dedicated retail codec.  Most
importantly, an initialized or early-game save cannot treat all Unit OrderLists as empty:
the first structurally reached neutral Unit already has a positive ExploreTo order.

## Mutation gates and next boundary

The six focused tests cover the exact executable spans, the PDB field ownership, the
SHA-pinned SVX witness, non-searching boundary behavior, synthetic nonzero metric/flags,
and mutations of:

- Objects, SubObject, Object, and Unit tags;
- both inherited walk gates;
- the MultiPtrArray presence plane and duplicated history;
- launching presence, path length, concrete order type, and truncation.

Run them with:

```sh
python3 re/scripts/test_savegame_unit_orderlists.py -v
```

The next independent extension begins at `0x505e2`: parse `PtrArray<Guy>` and its recursive
Guy payloads, then continue through the remaining Unit bodies, Build bodies, and owner arrays.
Only after all concrete order payload families are mounted should this bounded parser become
a general retail SVX importer or earn save-closure credit.
