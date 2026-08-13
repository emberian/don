# Canonical bounded STANCE runtime

Opcode 2 now has an executable saved/resumed subdomain without promoting the whole command row.
The canonical host decodes one strict `[Group][STANCE]` pair, validates the play-to-owner map,
reuses the fixed Groups selector and persisted play-keyed selection cache, and publishes one
stale-checked transaction over Groups, Unit backlinks, `UnitData::stance`, Object flags, and the
command cache. The packet consumes no RNG.

The admitted cone is deliberately exact and small:

- the requested dword is `-1` or `-2`, the only two values in the corpus;
- every effective selection member is an active, on-map, non-aircraft Unit;
- the retail Object virtual and Unit type query agree on one stance type in `1..=3`;
- an on-map captain supplies the representative selected by formation category; and
- the recovered planner emits only `WriteUnitStance` plus Object flag `0x10` steps.

Within that cone this is the whole reached action, not a no-op approximation. Retail computes the
modal current option, cycles forward or backward with the type-specific cycle length, clears Group
`disband`, writes the resolved stance to every eligible Unit, and sets flag `0x10`. Those fields and
the selection cache already belong to the core save image. The external stance/type projection is
revision/digest and Handle bound, deliberately not saved, and must be reinstalled after load.

## Why STANCE was selected

The post-ATTACK_GROUND strict-corpus audit found 74 STANCE, 21 FORM, and 169 ground PATROL pairs.
Frequency alone would favor PATROL. It is not the largest whole executable saved/resumed branch:
the first installed PATROL frame immediately reaches `Group::action_move_to`, and FORM likewise
reaches halt plus formation movement. Both require the wider movement/path transaction. Stance
types `1..=3` terminate in already canonical scalar writes, so STANCE is the honest next branch.

Across the 64 local paths, every explicit STANCE selection is Unit-band. The exact distribution is
56 requests for `-1` (44 cached, 12 explicit) and 18 for `-2` (15 cached, 3 explicit): 59
zero-member cache uses and 15 explicit selections. Explicit
sizes are 1, 2, 5, 7, 28, 30, 31, 43, 58, 60, 63, 65, and 66 members.

The save/resume witness is
`Playback___2024.03.18_18_18_49__Mon_.rcx`, SHA-256
`d27e34aa6ac40fbab3948a3f0f2bfbf35058fe463e42604e1e375b26467360f9`:

```text
package 2818 / turn 2819 / play 0 / frame 11281
0005011b00200027002a002d00  Group(owner=1, objects=[27,32,39,42,45])
02ffffffff                  Stance(request=-1)

package 2820 / turn 2821 / play 0 / frame 11289
000001                      Group(owner=1, saved cache reuse)
02ffffffff                  Stance(request=-1)
```

The replay fixture test binds the file hash, turn/frame metadata, and both wire pairs. The Sim test
uses those exact bytes against a synthetic but fully bound stance-type-1 authority: the explicit
packet changes option 0 to 1, core save/load byte-round-trips the resulting state and cache, and
the exact empty-Group packet changes option 1 to 2 identically in direct and resumed simulations.
This does not claim the recording's unavailable type table was reconstructed from RCX bytes.

Type zero stays outside this host because stances 0/3/4 clear mandatory orders and other options
can reach repeated update, repath, kill-current-order, or clear-orders tails. Build groups,
aircraft, mixed stance types, positive requests not observed in this corpus, absent authority, and
every stale before-image fail before mutation. The opcode row therefore remains red.

## Gates

```sh
cargo test -p don-sim --test canonical_stance_save_resume
cargo test -p don-replay --test retail_simple_group_package_fixtures \
  retail_replay_binds_stance_explicit_origin_and_immediate_cache_reuse -- --exact
cargo test -p don-sim --lib
cargo test -p don-replay --lib
cargo test -p don-sim --test save_load_groups
cargo test -p don-sim --test movement_order_save_resume
git diff --check
```
