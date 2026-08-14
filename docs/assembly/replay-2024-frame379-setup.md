# 2024 frame-379 setup chronology

## Scope

`setup_2024_frame379` closes the deterministic owner-0 starting-Unit chronology downstream of
one real Great Lakes world-generation after-image. It targets the shortest known clear-pool
Group-to-Move witness:

- replay `Playback___2024.02.23_20_49_35__Fri_.rcx`, file SHA-256
  `1690431a5ef19b38a3425d3dd7311e8e83ca0d27c56fabe49d776a9f1421b251`;
- seed `0x00bb97d3`, style 14, size 6, starting-town mode 1;
- owner/play 0, Dutch tribe 22, camera/Village center `(74592, 10080)`, tile `(97, 13)`;
- first clear-pool pair serial 64/frame 379, selecting Citizens 3 through 6.

This is an authority adapter, not a Unit generator. It never uses the recorded checksum as an
oracle and it does not claim that frame 379 is replayable yet.

## Exact schedule

Static sizes come from the replay-carried shipped Rules. Dynamic tribe/type/upgrade results come
from a revisioned `Frame379LeaderSetupAuthority`; they are not inferred from static upgrade rows.
The admitted owner-0 schedule is:

| Setup ordinal | Unit | Type | Unit members | Guys per Unit |
| --- | --- | ---: | ---: | ---: |
| 0 | Scout | 69 | 1 | 2 (`squad=1`, `crew=1`) |
| 1 | Dutch Merchant | 62 | 1 | 3 (`squad=1`, `crew=2`) |
| 2 | Dutch Merchant | 62 | 1 | 3 (`squad=1`, `crew=2`) |
| 3--6 | Citizen | 50 | 1 each | 1 each |

The Merchant shape is load-bearing. A one-Guy Merchant fixture is not an exact setup image.

## Composition contract

`produce_frame379_setup` takes eight immutable `Sim` snapshots: the completed
worldgen/starting-Village state and the state after each of seven complete retail
`Objects::init_unit` receivers. It also requires:

- `Frame379WorldgenAuthority`, binding the replay, whole World checksum, RNG, and source-owned
  completed Great Lakes/Village state;
- `Frame379LeaderSetupAuthority`, binding exact new-game Leader bonus/type selectors;
- seven `Frame379CompletedInitAuthority` values, each containing a valid source-model
  `DetailedInitUnitReceipt`, the generic `InitUnitAuthorityReceipt`, World/RNG before and after,
  and a nonzero complete-receiver composition digest.

For every call the adapter re-executes `Setup::place_unit` probes, verifies the exact RNG chain,
validates the 1,603-byte `Objects::init_unit` receipt, enforces sequential Unit marks and stable
identities, and binds the detailed Unit after-image to the canonical Sim row. It retains exact
World checksums without fitting them. The output includes a validated `BuildUnitsPrefixReceipt`,
so `bind_canonical_setup_citizens` can select ordinals 3--6 and downstream Groups can combine
those immutable Unit images with independently exact land-speed authority.
`Frame379SetupReceipt::canonical_snapshot_authority` projects the composed worldgen, Leader,
seven receiver, stable-identity, and final World/RNG digest directly into the generic selector;
no downstream caller supplies another attestation scalar.

Mutation gates reject missing revisions/digests, replay drift, wrong dynamic bonuses/upgrades,
non-adjacent World/RNG states, invalid detailed receipts, stale identities, wrong types or
positions, nonempty orders/paths, wrong Guy marks, and allocation gaps.

## Exact residual

No real `Frame379SetupReceipt` can be emitted yet. The currently known executable Great Lakes
worldgen chain stops in `place_all_mountains_add_mountain`: mountain mode 5 remains unsupported,
and the required effects-graphics/displacement inputs are not yet installed as canonical runtime
authority. Consequently there is no lawful post-worldgen World/RNG/Village seam and no real
complete receiver after-image for the seven owner-0 calls.

After that seam lands, the remaining direct prerequisites are the complete generic Unit/Guy
initializers (graphics, height, collision, visibility, Leader accounting, and RNG effects) for the
Scout, both Merchants, and four Citizens. Full frame-379 replay also needs every other player's
setup chronology and intervening frame simulation; this owner-0 receipt alone advances no corpus
checksum.
