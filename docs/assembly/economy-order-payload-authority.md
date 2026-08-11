# Economy order payload authority

Status: merge-ready exclusive authority and v13 leaf codec; not yet registered in the shared
`Order` / `OrderList` / DoNSave owners. The source is
`crates/don-sim/src/systems/economy_order_payload_authority.rs` and its 11 focused tests are
`crates/don-sim/tests/economy_order_payload_authority.rs`.

This tranche closes one ownership ambiguity shared by six executable rows. It does not promote
the rows to production-complete until the common `Order` and `save_load` integration lands.

| order row | exact authority | walked bytes | node bytes | v13 node pieces |
|---|---|---:|---:|---|
| `BOARD_SHIP` (8) | flag + primary `(o,who,uid)` + stable Handle | 11 | 16 | metric prelude; `None/0` typed leaf |
| `AWAIT_BOARD` (9) | flag + primary `(o,who,uid)` + stable Handle | 11 | 16 | metric prelude; `None/0` typed leaf |
| `REPAIR` (13) | flag + primary `(o,who,uid)` + stable Handle | 11 | 16 | metric prelude; `None/0` typed leaf |
| `GATHER` (7) | primary identity + exact 20-byte mutable suffix | 31 | 36 | metric prelude; tag 2/v1 + 20-byte leaf |
| `CAST_SPELL` (14) | primary identity + `x/y/paid/spell` | 27 | 32 | metric prelude; tag 3/v1 + `paid/spell` leaf |
| `TRADE_ROUTE` (15) | two identities + `started/loaded` + second Handle | 29 | 34 | metric prelude; tag 4/v1 + 18 bytes + Handle leaf |

“Node bytes” means the bytes after the surrounding four-byte list count: four-byte order type,
one-byte `RecycledOrderNode::metric`, then the concrete virtual walk. The metric is state, not
padding. The current flattened `OrderList` drops it; the shared integration must add ownership
before claiming save/checksum fidelity.

## Exact concrete images

All virtual walks begin with the `UnitOrder::flags` byte. The three target-only orders then walk
the ten bytes `ox:i32, whom:i32, uid:u16`.

Gather appends the full twenty-byte history, in retail field order:

```
tx:i32, ty:i32, build_type:i32, wait:i32,
goto_build:u8, non_flat_gather:u8, dist_mod:u8, been_there:u8
```

Cast appends `x:i32, y:i32, paid:i32, spell:i32`. `x/y` are already present in the generic
flattened order header, so its typed DoNSave leaf owns only the eight bytes `paid/spell`. The
retail checksum image still contains all sixteen bytes.

Trade's second region is deliberately not encoded as a normal contiguous target identity. Its
retail order is:

```
oxx:i32, whose:i32, started:i32, loaded:i32, uid2:u16
```

That 18-byte order is pinned by byte-exact tests. Reordering `uid2` beside `oxx/whose` would be a
plausible-looking but checksum-divergent representation.

## Stable identities and the second Trade Handle

Every live scalar `(o,who,uid)` must carry a compaction-stable `Handle`. A fully absent identity
must be exactly `(-1,-1,0xffff,None)`. Half-live addresses, live identities without a Handle, and
sentinels with invented Handles are rejected rather than normalized.

The generic order header already has storage for the primary Handle. Trade is the first row in
this cohort which requires a second independent Handle. The v13 leaf therefore writes the exact
18 retail bytes, then `handle_present:u8`, followed by `id:u32,generation:u32` when present. The
extra identity bytes are DoNSave-only. Tests prove that changing only the Handle changes the save
leaf and does not change the retail 29-byte walk.

An unresolved Trade destination remains representable as retail's exact no-target sentinel and
has no second Handle. This is required by the executor's destination-scan phase. Once `oxx/whose`
become live, absence of the second Handle is an error.

## DoNSave-v13 merge contract

The codec is deliberately strict and accepts format 13 only. Its two returned pieces have
separate anchors in the complete order record:

```
node_metric:u8,
generic_order_record...,
payload_tag:u8, payload_version:u8, payload...
```

The metric prelude is new in v13. The typed payload remains at the existing v12 envelope tail,
so v7-v12 streams stay byte-exact. Until `OrderListNode` owns nonzero metrics, the v13 writer may
emit zero only and the reader must reject a nonzero byte rather than drop it. Once node ownership
lands, the same v13 location round-trips every `u8` value without another format change.

The surrounding generic order header retains kind, flags, `x/y`, primary scalar identity and
primary Handle. The kind selects exactly one tag:

| kind | tag/version |
|---|---|
| Board / AwaitBoard / Repair | `0 / 0` |
| Gather | `2 / 1` |
| CastSpell | `3 / 1` |
| TradeRoute | `4 / 1` |

The decoder rejects all of the following before publication:

- unknown tags or payload versions;
- a known payload attached to a foreign order kind;
- unsupported order kinds;
- every truncation and every trailing byte;
- invalid Handle presence markers;
- malformed live/sentinel identity histories;
- DoNSave versions other than the currently proven v13 envelope.

This is intentionally stricter than retaining unknown bytes. Replay continuation cannot execute
an opaque concrete order, so accepting an unknown history would only defer divergence until the
next tick.

## Shared integration sequence

The source module stays unregistered while the movement lane owns the shared v13 package-state
tranche. Once that owner freezes, the conflict-free merge is:

1. add a node metric field to the authoritative `OrderList` node representation;
2. add exclusive Gather, Cast and Trade payload variants to `Order`, including Trade's second
   Handle;
3. make constructors for Board, AwaitBoard, Repair, Gather, Cast and Trade publish only exact
   target identities and typed payloads;
4. write/read the v13 metric immediately outside `write_order/read_order`, and transplant the
   typed leaf at the existing envelope tail without changing the Move tag-1 body;
5. widen `order_dispatch::adopt/publish` without dropping metric or either Handle;
6. prove direct-next-tick and save/reload-next-tick equality through the canonical Group action
   transactions.

Only after those steps can Board/Repair/Trade group packets and Gather/Spell order creation earn
production ownership. This tranche supplies their lossless common state boundary; it does not
pretend the Sim adapter has already committed those writes.

## Validation

The focused suite passes 11/11 both in an isolated local target and the current shared target,
and on the independent Persvati Linux executor. Final remote job
`economy-payload-v13-final-20260811T212614Z-2781-7706-a7bf776e8e5b` completed with exit code 0.
The suite covers all six supported order kinds, exact walked/node lengths and byte offsets,
nonzero metrics, every payload truncation, trailing bytes, kind/tag/version crosses, live and
sentinel Handle invariants, and the additive second Trade Handle boundary.
