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
`publish_frame379_setup_sim` is the direct Sim-owner boundary: it validates through an isolated
deterministic save/load copy, then moves the candidate into the published slot only on complete
success. Refusal returns the candidate and leaves any existing published Sim unchanged.

`bind_captured_frame379_completed_init` is now the source-capture constructor for each of those
seven receiver authorities. It admits the supported retail executable plus exact entry/after
DoNSave hashes and a versioned detailed-receipt hash, re-executes the placement probes, and derives
the composition digest itself. The binder requires exactly one appended Unit, validates its whole
native detailed receipt and stable Handle/type/position image, requires a complete canonical
`UnitGuys` side store (including every 155-byte Guy walk image), and proves every prior Unit row,
order, path, pathfinder input, and Guy image unchanged. Thus the Great Lakes lane can feed captured
Scout, Merchant, and Citizen calls directly into `produce_frame379_setup`; it no longer needs a
2018-specific complete-initializer adapter or a caller-chosen digest.

Mutation gates reject missing revisions/digests, replay drift, wrong dynamic bonuses/upgrades,
non-adjacent World/RNG states, invalid detailed receipts, stale identities, wrong types or
positions, nonempty orders/paths, wrong Guy marks, and allocation gaps.

## Exact residual

No real `Frame379SetupReceipt` can be emitted yet. Mountain mode 5 and its exact player-group
owner adapter are implemented, but the required effects-graphics/displacement inputs are not
installed as canonical runtime authority. A cold Great Lakes replay therefore still stops at
`place_all_mountains_add_mountain`; its placement result also has not yet been joined to the exact
CoordInfo/scalar cone and post-mountain height plane. Consequently there is no lawful
postworldgen World/RNG/Village seam from which to bind the seven adjacent receiver Sims.

After that seam lands, the receiver binder needs seven coherent retail capture tranches containing
the complete graphics, height, collision, visibility, Leader-accounting, RNG, Unit, and Guy
after-images for the Scout, both Merchants, and four Citizens. Full frame-379 replay also needs
every other player's setup chronology and exact intervening frame simulation; this owner-0 receipt
alone advances no corpus checksum.

The downstream command mount is `setup_2024_frame379_group_move`. It requires an explicit
source-backed frame-zero-to-379 chronology authority, rebinds all seven owner-0 setup members at
the canonical frame-379 Sim, installs complete live Group authority from the exact replay Rules,
and issues serial 64. It does not relabel frame zero. Playback checks before processing the due
record, so the delayed frame-385 clear checksum is not evidence that the Group was absent. The
next exact boundary is installed-Group processing at frame 384 before its first recorded
checksum-visible value at frame 391.

That handoff is now composed by `setup_2024_frame384_groups_process`. It accepts only an
independently canonical Sim immediately before the frame-384 Groups callback, proves its Groups
pool is still the serial-64 after-image, regenerates current-state land speed/Unit authority, and
commits the exact slot-zero normalization transaction. It deliberately does not run or skip the
intervening frames. Until the real Great Lakes/setup and frame-0-to-384 chronology authorities
exist, both mounts refuse and the replay scoreboard remains unchanged.
