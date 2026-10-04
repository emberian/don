# Replay `ResourceDivvyPool` selector frontier

`crates/don-replay/src/resource_divvy_pool_selection_frontier.rs` owns the exact logical
pool mutation performed by the three shipped selectors. Its authority is
`ron-bin/riseofnations.exe` (SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`) and the
GUID-matched `ron-bin/sbl/rise.pdb` (SHA-256
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`).

| Lane | Procedure | RNG call | Return |
|---|---:|---:|---:|
| water | `0x0068a4a0` | `0x0068a4cd` | `0x0068a565` |
| late | `0x0068a570` | `0x0068a59d` | `0x0068a635` |
| early | `0x0068a640` | `0x0068a66c` | `0x0068a6f6` |

The PDB names the exact procedure ranges as water `[0x0068a4a0,0x0068a566)`
(198 bytes), late `[0x0068a570,0x0068a636)` (198 bytes), and early
`[0x0068a640,0x0068a6f7)` (183 bytes). `DynamicBitMask` is the native
`{ bits: +0, byte_count: +4, pointer: +8 }` layout. The corresponding pool lanes begin
at offsets `+0x50`, `+0x28`, and `+0x00`; their array counts/data pointers are
`+0x60/+0x6c`, `+0x38/+0x44`, and `+0x10/+0x1c`.

All three bodies implement the same transaction against different native offsets. If the
lane has one entry, index zero is selected without RNG. Otherwise each attempt calls the
shared `Random::get(0, 0xffff)` at `0x00a39d70` and applies signed remainder by the lane
count. An already-set index retries. An unused index is marked before its good ID is loaded;
therefore a `-1` entry remains marked and also retries. Once a non-`-1` good is selected,
the body scans every logical bit and clears every allocated mask byte if all are set.

The typed receipt carries the complete concrete six-field pool before and after, both stable
logical digests, the exact RNG transcript, selected index/good, and whether the exhaustion
clear ran. Validation re-executes the transaction. Malformed bit-count/byte projections and
states with no unused non-`-1` entry are rejected atomically instead of entering the native
infinite retry loop.

This owner proves an individual selector transaction. It does not infer how many times an
opaque placement body invoked a selector; the placement host must carry an ordered list of
these receipts, which integration binds to the placement RNG transcript before advancing the
public pool.

That binding is implemented for BONUS rows. Player placement dispatches selector 2 to early
at `0x006920b5` and every other nonzero selector to late at `0x006920bc`. Region placement
has calls at `0x006906ca/0x006906d6/0x006906dd`; its water call is unreachable for the
BONUS alias request because that request carries `good_id = -1`, so the admitted mapping is
again selector 2 to early and selectors 1/3 to late. No placement-body call targets the
separate mask-refresh helpers at `0x0068a440`, `0x0068a460`, or `0x0068a480`.
