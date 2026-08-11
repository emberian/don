# `Cities::capture_city` complete residual

Implementation tranche:
`crates/don-sim/src/systems/combat/cities_capture_residual.rs`.

Fidelity tier: **C**. This is a complete instruction-bounded recovery of the shipped
outer driver from the previous old-owner-refund seam through its return. It was not
executed against retail and has no differential promotion. Separately named callees remain
typed synchronous host transactions; this tranche does not claim their nested bodies.

## Provenance and exact boundary

- `ron-bin/riseofnations.exe` SHA-256:
  `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
- `ron-bin/sbl/rise.pdb` SHA-256:
  `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`.
- PDB procedure: `int Cities::capture_city(int new_owner, int old_city, int old_owner)`,
  VA `0x00733380`, size 7,998 (`0x1F3E`), exclusive end `0x007352BE`, source
  `cities.cpp:87..733`.
- This tranche: `0x0073432D..0x007352BE`, 3,985 (`0xF91`) bytes. SHA-256 of those exact
  executable bytes is
  `f95550a88be49a214f2970280c48067f414cab2074758453701f5511822f712e`.
- Capstone decodes the range continuously to the PDB end as 1,071 instructions: 116 calls
  (90 direct and 26 virtual), 63 conditional branches, and 14 unconditional jumps.
- Cumulative recovery is now `0x00733380..0x007352BE`: no byte residual remains in the
  outer function.

The Ghidra C at `re/decomp-all/00733380.c` supplied control-flow orientation only. Capstone
settled all call arguments, conditional read shapes, vtable slots, and cleanup order. The
PDB supplied identities including `CityData::{city_flags,o}`, `LeaderData::city_mark`,
`BuildData::city`, `ObjectData::damage`, `City::close`, and the capital methods.

## Exact control-flow map

| Retail range | Owned behavior |
|---|---|
| `0x0073432D..0x00734547` | old-owner capital refund across every mutually available non-knowledge bucket and optional local refund message |
| `0x00734547..0x007347E2` | ordinary-plunder Russian/Despot split, smallest eligible new-owner bucket, and optional local message |
| `0x007347E2..0x00734A32` | smallest eligible old-owner refund bucket and optional local message |
| `0x00734A32..0x00734E93` | notification suppression; local new/old capture messages; mutual-diplomacy, visibility-gated observer events |
| `0x00734E93..0x00734FFA` | old captured-object close/disband loop |
| `0x00734FFA..0x00735044` | new captured-object `Wall::mask_me(1, RegenRoads(1))` loop |
| `0x00735044..0x0073509A` | old-capital Forbidden City gate, `find_capital`, and `Leader::lost_capital` |
| `0x0073509A..0x007350E7` | new-capital `find_capital` and `Leader::recapture_capital` |
| `0x007350E7..0x00735141` | old city-center read, `City::close`, and trailing inactive city-row compaction |
| `0x00735141..0x0073522B` | old-center detach/close; new-center hits, damage, LOS, and pop-cap repair |
| `0x0073522B..0x007352BE` | new-array then old-array destruction and returned new-city index |

## Two distinct resource algorithms

The previous capital award cone may carry an old-owner refund into `0x0073432D`. Retail
walks TypeIndex 0 through 5 in order. For each type it queries new-owner availability and,
only on success, old-owner availability. It excludes TypeIndex 3 only after both queries.
Every remaining available bucket on the old owner receives the full refund via the
`LeaderDataEncrypt` XOR-`0x8221` transaction.

Ordinary non-capital plunder at `0x00734547` is deliberately different. The driver selects
one bucket, initialized as TypeIndex 0 with a signed minimum sentinel of 999,999. It uses
the same conditional availability order and excludes TypeIndex 3 after both calls, but
reads the candidate owner's decoded bucket balance and replaces the selection only on
strict signed `<`. If no eligible bucket exists, retail still credits bucket 0. The new
owner and Russian old-owner refund each perform their own selection against their own
balances.

The Russian/qualifying-general split is also recovered at this alternate entry:

- Russian: old owner gets the entire plunder; the new owner gets the entire amount only
  with the qualifying Despot/Spitamenes local, otherwise zero.
- non-Russian plus qualifying general: the new award is signed wrapping
  `plunder * Constants::thedespot_plunder / 100`.
- otherwise: the new owner gets the entire ordinary plunder and the refund is zero.

Every availability read, balance read, and encrypted bucket mutation is separately
receipted in driver order. A changed owner/type/mode, invalid raw availability outside
`0/2/4`, altered before image, or non-wrapping after image fails closed.

## Presentation and notification suppression

Resource templates are exact internal-string-array byte offsets, not guessed text:

- capital refund amount/city: `0x152C`, `0x1540`;
- minimum-bucket resource/city: `0x1554`, `0x1568`;
- local new-owner capture message/bubble: `0x157C`, `0x1590`;
- local old-owner capture message/bubble: `0x15A4`, `0x15B8`;
- allied new-owner bubble: `0x15CC`.

The local new-owner path uses notice offset `0x2828` and sound `0x7C`; the old-owner path
uses `0x283C` and `0x86`. Observer events require mutual diplomacy and
`CityData::is_seen(console)`. Both allied event arms use the new owner's neon color,
including the old-owner notification arm at `0x00734DFE`.

The alternate Russian old-owner refund has another measured attribution quirk: although
the old owner is the beneficiary and must be the local console player for the message to
appear, the bubble owner byte and neon color both use the new owner at
`0x007349CA/0x007349D6`. The receipt keeps beneficiary, bubble owner, and color owner as
three separate identities.

String allocation, parsing, recycler, color, MessageWin, optional IFace notice/sound, and
reverse destructor order are represented as one typed presentation receipt with mandatory
exact-order attestation. This keeps the outer driver exact without pretending that the
presentation allocator belongs to the simulation body.

A local resource notification suppresses the later generic capture notification. A local
old-owner refund presentation jumps directly to cleanup. A previously raised new-owner
capital-award notification also suppresses the common cone even when the refund is zero or
nonlocal.

## Captured-object cleanup

Retail walks the first `SimpleArray<int>` in original append order. Closed objects
(`SubObjectData::flags & 0x20`) skip every later query. Open non-buildings call
`Object::disband(0)`. Open buildings query TypeIndex `0x1A1` and then, only if that misses,
`0x1A7`. Either type queries new-owner tribe bonus `0x13` (Lakota): a hit closes with mode
0; every other building closes with mode 5. Both calls carry killer `-1` and float bits 0.

The second array then calls `Wall::mask_me(1, RegenRoads(1))` for every successfully
captured new-owner member in append order. Typed inspection receipts enforce the exact
short-circuit query shape, so an eager second type query or tribe query is rejected.

## Capital, score/diplomacy boundary, and final center repair

An old capital first calls `LeaderData::has_wonder(FORBIDDENCITY=0x213)`. Raw results 0
and 2 enter `find_capital`; other values skip it. A missing or differently owned result
calls `Leader::lost_capital(captor)`. A new capital independently calls `find_capital` and
calls `Leader::recapture_capital()` on the same missing/mismatched condition.

There is no direct leader score-field store in this 3,985-byte range. Capture/loss counters
and achievement events are in the already recovered prefix. The residual's score and
diplomacy consequences are inside the separately named synchronous calls `City::close`,
`Leader::lost_capital`, and `Leader::recapture_capital`; receipts require those transactions
to complete but do not claim their nested instruction bodies.

After reading `old CityData::o`, the valid typed path calls `City::close`. The machine code
pushes a volatile first stack word, but disassembly of shipped `City::close` `0x00737550`
shows that `[EBP+8]` is never read; only its second argument, the captor, is observable.
The model therefore records the city and captor and does not invent semantics for the
unread residue. The driver then decrements `LeaderData::city_mark` while trailing city rows
have `city_flags & 1 == 0`; the typed receipt freezes every descending flags read and the
exact before/after mark.

Finally retail:

1. writes old center `BuildData::city = -1`;
2. closes the old center with mode 5, killer -1, float zero;
3. calls new center `Wall::update_hits(0)`;
4. reads `BuildData::hits(0)` and writes
   `ObjectData::damage = wrapping(hits - 10)`;
5. calls `Wall::update_los()` and new-owner `Leader::calc_pop_cap()`;
6. destroys the new-object array before the old-object array; and
7. returns the signed new `CityKey::city` index.

## Validation and integration boundary

`crates/don-sim/tests/combat_cities_capture_residual.rs` privately path-imports the frozen
predecessor tranches and exercises the complete residual with canonical typed host receipts.
Five focused tests cover the exact byte/instruction/call census, skip-plunder through final
return, Russian minimum-bucket refund, impossible eager-query rejection, and predecessor
receipt corruption. Local `tools/swarm-cargo city-capture-residual test -p don-sim --test
combat_cities_capture_residual` passes 5/5.

This is not yet mounted in the shared combat module. The current authoritative Sim host does
not expose one coherent city/object/bucket/presentation transaction capable of implementing
all predecessor traits plus this residual without a parallel shadow state. A fake shared
adapter would create green isolation rather than a real path. Shared registration and a
single end-to-end Sim capture test should follow the host convergence, then be audited
against replay city/build/unit channels. No VM or live process was used.
