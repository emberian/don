# Tick step 11 frontier — `Leader::diplomacy` opening cone

This source-only proof attacks the larger of step 11's two still-red bodies:
`Leader::diplomacy` at retail `0x006BC950`. The shipped PDB identifies
`leaders.cpp:23630-26409`, size 20,348 bytes. That is too large and too product-entangled to
claim as one reconstruction. The owned boundary is instead the opening blocks beginning at
`0x006BC96E` and ending at the first unowned instruction `0x006BCB2C`, plus the physically
late target-loop continuation `0x006BE98A..0x006BE99A`. The boundary ends exactly after the
first local agenda store and before the resource/city/map/chat/command policy remainder.

The isolated artifacts are:

- `crates/don-sim/src/systems/leaders_diplomacy_opening_frontier.rs`;
- `crates/don-sim/tests/leaders_diplomacy_opening_frontier.rs`.

They are intentionally path-imported by the focused test. There is no shared `mod.rs`, tick,
save, or runtime edit, and therefore no integration or closure credit in this tranche.

## Retail evidence and ownership

Ground truth is the shipped `ron-bin/riseofnations.exe` image plus its GUID-matched
`ron-bin/sbl/rise.pdb`. Direct `objdump -d -Mintel` reads cover the owned instructions and
`LeaderData::is_ally` at `0x006EDB50`. `schema/pdb-types.json` names the load-bearing layout:

| offset | PDB field |
|---:|---|
| `+0x00` | `leader_flags` |
| `+0x08` | `who` |
| `+0x18` | `score` |
| `+0x74` | `diplos[8]` |
| `+0x94` | `treaties[8]` |
| `+0xB4` | `agendas[8]` |
| `+0x314` | `counteroffer[8]` |
| `+0x334` | `tribute_demanded[8]` |
| `+0x692C` | `dip[8]`, stride `sizeof(Diplomacy)==0x5C` |
| `dip+0x00` | `Diplomacy::agree` |

The entry's first ownership detail is easy to flatten incorrectly: it reads `this->who`, then
tests the **global** `leaders[this->who].leader_flags & 4` human bit. It next exits for game
semaphore bit 9 and only then reads `GameAccess::ai_off`. The typed state keeps the `this`
record separate from the global array so tests can pin this addressing and gate order.

An invalid owner index or an invalid active leader identity is a typed error. Every fallible
identity read completes before the first possible agenda store; errors therefore leave the
state byte-for-byte unchanged.

## Ally and score scan

The first loop walks the eight global records in address order and enters only when
`(leader_flags & 3) == 3`. Each entered record calls
`LeaderData::is_ally(this->who)`. The 55-byte callee returns true for the same `who`; otherwise
it requires exactly:

```text
subject.diplos[owner_who] == 2
    && leaders[owner_who].diplos[subject.who] == 2
```

The receipt stream records every reached call, its two directed values, and its result. The
opening also counts non-allies, counts scores strictly greater than the global owner score,
and retains the greatest non-negative score. Because the machine replaces on greater **or
equal**, a later record wins a tie. Since the accumulator begins at zero, an all-negative
active set leaves strongest slot `-1`.

The forward relation is a real short circuit. If it is not 2, retail never indexes the
reciprocal relation by `subject.who`; the fail-closed implementation validates that identity
only when the reciprocal load is reached.

## Candidate cadence and first mutation

The second loop visits target slots zero through seven. A target must have flag bit 1, differ
from the owner slot, have treaty bit 0 set, and have treaty bit 2 clear. Agenda bit 2 is an
immediate pending trigger. Without it, retail selects a power-of-two period:

```text
period = 0x800
if dip[target].agree != 1:
    if tribute_demanded[target] != 0: period = 0x2000
    if counteroffer[target] != 0:     period *= 2
if agendas[target] & 8 == 0:         period >>= 2
```

The due test is a wrapping dword add and mask, not a signed remainder:

```text
(period - 1)
    & (game.frame + (target + 8 * owner_who) * (period >> 6)) == 0
```

On either pending or due entry, instruction `0x006BCB2A` stores
`agendas[target] & ~4`. The module emits a typed before/after mutation receipt and then stops
at `0x006BCB2C` with `DownstreamPolicy`. Later targets cannot run ahead of that residual,
because the unrecovered body may mutate the facts they observe.

The focused tests mutation-pin both directed ally values, strict versus tied score
comparisons, every target gate, every period source, wrapping phase arithmetic, the exact
agenda bit clear, stop position, and no-mutation failure behavior.

## Honest residual and remote gate

This does not claim the remaining `Leader::diplomacy` policy is complete. Product-owned
resource evaluation, city/map facts, random/chat behavior, command emission, and the cold
tail remain red. Integration delta is zero until the shared step-11 owner adopts this cone
and supplies an authoritative continuation.

Root convergence formatted the isolated files and validated all nine tests in persvati batch
`gen7-five-pack-20260809T231109Z-3866-5144-5f896c0277b5`. Retail was not run. The focused
reproduction is:

```text
cargo test -p don-sim --test leaders_diplomacy_opening_frontier
```
