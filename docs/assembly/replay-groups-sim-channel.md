# Same-frame Sim-owned Groups checksum transition

Status: exact bounded owner extension; deliberately **not installed** in the replay scoreboard.

## Why channel 6 is the next substantive seam

The 15-channel audit leaves one complete static channel (`rules`) and several exact but frozen
owners. `groups` is the smallest frozen owner with a newly canonical dynamic mutation:

- `groups_channel` derives the full 512-slot `Groups::clear` image and exact 36,896-byte retail
  traversal;
- `Sim.groups` is now the fixed save/checksum-owned pool;
- `Sim::process_command_package` atomically owns the real `GroupCommand 0x00` followed by
  `MoveToCommand 0x07`, including selection cache, stable object identity, Group allocation,
  backlinks, orders, paths, formation state and every one of MoveTo's nine fields; and
- the completed replay fixture establishes the cohort shape, while this tranche gates it again
  against the independent 21-recording checksum-bearing corpus.

Empty `walls`, pre-projectile `ammo`, pre-corpse `deaths`, and unconfigured `items` would be
smaller code changes but would add no substantive checksum evidence. Initial `units`, `builds`,
`guys`, `leaders`, `cities`, `goods`, and `world` still require incomplete setup joins.
`scenario_data::last_razed` is owned by a separate active lane.

The selection came from this complete current-owner audit, not from channel-number order:

| # | channel | current exact owner frontier | honest next gap |
|---:|---|---|---|
| 1 | units | exact sparse Unit walk and starting-Unit producer pieces | complete same-frame initial Unit/type/path/Guy authority |
| 2 | builds | exact canonical Build adapter and initialized-empty walk | complete setup City/Build constructor chronology |
| 3 | walls | exact typed walk/runtime | retail corpus is empty in all 222,938 comparisons; installing empty adds no substance |
| 4 | ammo | exact typed pool/walk and projectile mechanics | no exact replay initial/combat host reaches the first projectile |
| 5 | deaths | exact ring, slot policy and live-row walk | no exact kill/DeathObj host reaches the first corpse |
| 6 | groups | exact 512-slot initializer plus canonical Group→Move Sim owner | **this tranche**; other actions and pre-pair state remain red |
| 7 | guys | exact sparse recursive walk over `UnitGuys` | complete initial Unit/Guy identity and allocation history |
| 8 | leaders | exact setup-prefix, Sim-tech, Tribe and conditional-child frontiers | one complete same-frame join for all 27,182 walked bytes per row |
| 9 | cities | exact canonical City adapter | complete setup City/Build construction and live links |
| 10 | items | exact stable-slot runtime coupled to World WData | finish resource placement so replay setup installs the registry |
| 11 | goods | exact cold initializer and oil prefix | complete non-oil `Map::place_resources` schedule |
| 12 | world | exact thirteen-section walk and byte-owner ledger | finish map generation/resources/starts/fog/collision from replay setup |
| 13 | rules | complete replay-carried 997,846-byte producer | already 222,938/222,938 substantive matches |
| 14 | scenario | complete 8,453-byte initializer; `last_razed` writer active elsewhere | integrate all BHS/combat/city dynamic writers |
| 15 | script | exact empty count and typed program walk | complete shipped BHS execution and runtime state ownership |

## Ownership join

`ReplayGroupMoveSource::from_replay` accepts package coordinates only from a decoded Replay which
contains real opcode-`0x39` checksum packets and requires the selected package itself to carry one.
It retains the replay path and framing identity,
lockstep serial, package `play`, package `stamp`, exact Group and Move bytes, and every ignored
non-Sim opcode. Other Sim commands in the package are retained separately as unowned prefix/suffix
provenance; the caller must supply the exact state immediately before the admitted pair. It rejects:

- a recording with no checksums;
- a package without its own checksum command, even when another package in the recording has one;
- a checksum command which does not follow the admitted pair, or any unowned Sim command between
  the pair and that checksum boundary;
- a package without an immediately adjacent `0x00,0x07` pair;
- negative/out-of-range network seats; or
- a decoded command whose retained first byte disagrees with its opcode.

`issue_replay_group_move` then requires `Sim.world.frame == CommandPackage::stamp`, computes the
before image with the instruction-derived don-replay walker, calls the canonical Sim transaction
with the replay lockstep serial, and recomputes the post image. The post value must equal the
host's independently produced `groups_guys::Groups::check_groups` receipt. Frame and serial must
also round-trip unchanged, and the receipt proves the command consumed no RNG.

The adapter walks all 512 slots plus all eight `last_group` words. For every populated slot it
walks the 72-byte scalar header and the six `num`-length arrays in retail order. Focused mutation
kills cover all 21 header fields, all six member planes, every tail word, the package frame, and a
real selected object byte.

## Explicit red boundary

The transition is exact for the Sim state handed to it, but it does **not** prove that the Sim
matches retail before that package. `installed_in_scoreboard()` is therefore pinned false. The
remaining joins are:

1. replay setup must instantiate the retail initial Unit/content authority and every preceding
   Groups mutation;
2. Group action tails other than MoveTo need canonical hosts;
3. observed Camera/PlayerSpeed shell commands around Move packages need whole-package dispatch
   (their non-Sim opcodes are retained here but not dispatched); and
4. the replay harness must schedule the canonical package at the measured package frame and only
   issue channel 6 once every prior owner receipt is complete.

No recorded checksum word is an input, no residual is solved, and no empty channel is promoted.

The pinned Persvati witness is the checksum-bearing replay
`Playback___2018.11.17_13_21_42__Sat_.rcx` (SHA-256
`c006ecb860273605d2b48bf69f5dcb048596de5fc748aa664fa0a04452df2da0`), lockstep turn 48,
play 1, package frame 259, adjacent-pair command index 0. The hash is admitted by
`tools/swarm-remote-assets.sha256`; the remote gate therefore exercises retail bytes rather than
passing through the test's explicit `SKIPPED — NOT A PASS` branch.

## Gates

```sh
cargo test -p don-replay --test groups_sim_channel
tools/swarm-cargo-remote submit persvati checksum-groups-sim \
  --asset schema/live/final-balance-runtime.bin \
  --asset ron-data/replays/multi/Playback___2018.11.17_13_21_42__Sat_.rcx \
  -- test -p don-replay --test groups_sim_channel \
  real_packet_executes_at_its_package_frame_and_cross_checks_two_walkers -- --nocapture
```
