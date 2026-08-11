# Leaders same-frame Sim-owner audit

The completed conditional `LeaderData::walk_data` transcript is a representation result, not
an ownership result. This audit identifies the first concrete cohort which is already retained
and mutated inside one canonical `don_sim::tick::Sim` frame, binds it to the transcript, and
keeps the Leaders channel red for every fact which remains caller-supplied.

## Result

`Sim::production_runtime.leaders[8].tech` is the first useful owner. Each
`LiveProductionLeader::tech` contains:

- the current 806-bit `LeaderData::tech` payload (101 bytes);
- decoded `LeaderDataEncrypt::ages`;
- decoded `epoch[4]`, aggregate `epochs`, and `discovered`.

This is real runtime state, not a checksum sidecar. `Build::finished` reaches
`execute_gain_tech_cohort`, whose mutation receipts update the `TechState`; low-level gain and
loss operations update the mask and counters together. The production runtime is a field of the
same `Sim` which owns step 8 and victory-score state, so a binder can require all three views to
agree at one frame boundary.

`leader_tech_sync` now preflights the two dynamically shaped destinations before the production
completion begins. After `execute_gain_tech_cohort` commits, one infallible publication copies
the current 806 tech answers into `vic_leaders.has_tech` and decoded `ages` into both the step-8
Leader and the compatibility `LeaderSlot` façade. The façade copy matters because the next
step-8 input sync would otherwise overwrite the freshly published age. A malformed destination
refuses before the queue, production tech, or any duplicate owner changes.

The cohort accounts for 129 source-produced bytes per active Leader:

```text
current tech payload                              101
seven decoded tech-counter dwords              7 * 4
total                                              129
```

Of those, 105 bytes were already established by the runtime checksum frontier: 101 current-tech
payload bytes and the four-byte `ages` value (`step8.econ.age_alt`). The six other counter dwords
were conditional and become canonical here:

```text
LeaderDataEncrypt visitor indices 55..58  epoch[4]      16 bytes
LeaderDataEncrypt visitor index 60        epochs         4 bytes
LeaderDataEncrypt visitor index 61        discovered     4 bytes
newly canonical                                           24 bytes
```

Index 59 (`ages`) must agree with step 8. Index 54 (`resource_cap[6]`) is not a tech counter and
is not promoted.

## Existing runtime population

The current repository already retains these Leader facts in executable runtime owners:

| Transcript area | Current owner | Status before this tranche |
|---|---|---|
| flags, identity, many score/count/timer fields | victory-score and step-8 Leader stores | duplicate-checked by the fixed frontier |
| resolved tribe | BHS type owner | live and duplicate-checked |
| `num_queued[806]` | victory-score Leader store | fills the generated deferred hole |
| `Diplomacy[8]`, `Personality.raid`, taunt/modifier fields | step-8 taunt store | live and duplicate-checked |
| current `tech` and `tech_at_start` payloads | victory-score tech runtime | live and duplicate-checked |
| three rare payloads | step-8 rare owner, cross-checked with victory state | live and duplicate-checked |
| 49 decoded economy dwords | step-8 `LeaderEcon`, overlapping stock/income checked against victory state | live and duplicate-checked |
| current `tech`, `ages`, `epoch[4]`, `epochs`, `discovered` | production `TechState` | same-frame Sim owner joined by this tranche |

The current-tech payload is intentionally checked again. A production completion which changes
the canonical `TechState` but leaves victory `has_tech` stale must refuse rather than letting two
current-tech views silently diverge.

## Facts which still lack producers

The following are still conditional or absent and therefore prevent channel installation:

- the caller-supplied generated `LeaderCols` fixed body has no canonical store inside `Sim`;
- `reg_buildings[64][129]`, both unnamed walked padding words, and many historical fixed arrays
  have no Sim producer (only `num_queued` has an established deferred owner);
- 23 of 24 `Personality` dwords have no canonical owner (`raid` is the exception);
- all nine bitmask headers, `obs_flags`, and the three conquest-mask payloads lack runtime
  producers; `tech_at_start` is retained history but not part of production `TechState`;
- `Sites`, `MakeList`, the three `SimpleArray<int>` histories/elements, and the production-script
  UTF-16 value lack canonical containers;
- six decoded `rate` dwords and `resource_cap[6]` remain absent from `LeaderEcon`;
- a completed transcript still depends on all earlier conditional fixed/deferred authorities.

The dynamic suffix had 350 conditional bytes per active Leader. Promoting six counter dwords
reduces that suffix-only number to 326. This is not a claim that the whole channel has only 326
bytes left: the generated and deferred fixed-body frontiers retain their independently reported
conditional coverage.

## Fail-closed join

`bind_sim_tech_frontier` takes the established deferred frontier, the complete child authority,
and `&Sim`. Before returning it requires:

1. the previous frontier, step-8 flags, and victory flags describe the same active roster;
2. all eight production-runtime Leader slots exist;
3. the production current-tech payload equals the conditional payload (which the inner frontier
   independently compares with victory `has_tech`);
4. production `ages` equals the step-8 decoded economy owner;
5. all seven production tech counters equal their visitor positions in the conditional authority;
6. the complete inner dynamic frontier still binds without any other disagreement.

The receipt reports 129 source-produced, 105 duplicate-checked, and 24 newly canonical bytes per
active Leader. It delegates the exact complete walk, but `checksum()` still returns
`Err(complete_frontier)` and `installed_in_scoreboard()` remains false.

## Mutation proof

Focused tests use `TechState::gain(100)`, an ordinary discovered tech, rather than directly
editing checksum bytes. The producer changes both its current-tech bit and decoded `discovered`
counter. After synchronizing the independently owned victory tech view and conditional receipt,
the complete Leader Adler checksum changes without changing its byte count. Leaving either the
authority, victory view, counter view, or step-8 age stale refuses at the agreement gate.

Production-runtime tests additionally prove the installed hook itself: research completion
publishes its tech bit, age completion publishes decoded ages to both consumers, and shortening
the victory tech vector refuses before either the queued Build or production `TechState` mutates.

This is the smallest real ownership step toward installation. The next useful cohort should be
chosen from a state already mutated by `Sim`; adding zero-initialized storage for an absent
container or generated plane would not be a producer.

## Verification

The local read-only replay corpus ran all seven focused dynamic-child tests: 7 passed, 0 failed,
0 skipped. Persvati job
`replay-leader-tech-20260811T192529Z-6950-32383-eacd5a5774c9` compiled that integration target
from clean pushed HEAD plus explicit overlays. Persvati job
`leader-tech-sync-20260811T192100Z-99972-26407-f80e3692fcd3` passed both atomic synchronization
tests, and warmed job `leader-tech-sync-20260811T192714Z-9363-16753-eacd5a5774c9` passed all 28
production-runtime tests, including research publication, age publication, malformed-view
refusal, explicit non-tech non-gating, and the unrelated unit/build completion paths.
