# Supported 2024 golden replay: exact timeline and authority deadlines

This is the composition ledger for the supported 2024 replay from procedural setup through the
first Group checksum change. It records the earliest native body which can change a later input;
it is not a promise that the current tick executes the row. Recorded replay checksums are used
only for measurement, never as state inputs or fitted values.

The required frame-one entry is an adjacent supported-retail capture. The older
`Frame379SetupReceipt` is insufficient: it contains the center Village and seven owner-0 Units,
but omits the Dutch Market created before those Units and it does not execute frame zero.

## Canonical inventory and setup order

1. Complete Great Lakes style-14 `place_all`, final height plane, World and RNG.
2. Center Village, owner 0 / object 2000 / type 414, with its City and terrain links.
3. Dutch `Leader::produce_building(436, 2000, 0)` Market. Successful allocation is expected at
   object 2001, but identity, fine-site probes, 0..4 RNG draws, `init_build`, City linkage and
   activation must come from the Market transaction/capture.
4. Seven setup Units, in order: Scout69 o0, Merchant62 o1/o2, Citizen50 o3..o6. Every row is
   generation-bound and retains complete Unit/Guy/World/RNG receiver state.
5. Exact frame-zero simulation, then both serial-1/frame-1 LeaderOptions commands, then the
   post-command frame-one entry capture consumed by `setup_idle_prefix`.

The instance `UnitData::unit_masks` field is never derived from replay `obj_masks`. Native
`Unit::init` zeroes it and installs only reached derived bits. Every timeline mount reads the
live captured mask.

## Earliest deadlines

| frame | native boundary | exact consequence / open authority |
|---:|---|---|
| setup | Great Lakes mountain group 2 | installed effects XML plus 16 displacement TGAs are still required; no texture content is synthesized |
| setup | Dutch Market436 before Units | mandatory Build/City/World/RNG transaction omitted by the old setup receipt |
| 0 | Merchant62 `think_merchant -> unpack_merchant(3)` | can install order/path/mask state through `find_merchant_spot`; setup-empty orders do not prove frame-one emptiness |
| 0 | Scout69 `think_spellcaster` | golden human flags bit 4 takes the direct Counterintel arm and consumes no main RNG; it may install a CastOrder but never writes the Caster active-spell queue |
| 1 | two LeaderOptions commands | Citizens become worker stance 1; owner-0 `peasants_wait` becomes 0; synchronized writes precede `do_frame` |
| 1 | Scout `Unit::process` | `Caster::process_spells` occurs before healing/work; adjacent empty-queue authority is `frame1_caster_process` |
| 1 | Citizen/Merchant `Unit::do_idle` | after local mask/work prefix, `Unit::set_anim(0,0,1)` is the first unclosed child; it owns complete Guys and possibly main RNG |
| 1 | Citizen continuation | `collide=0`, `check_idle`, type50 mask clear, dual-mirror Leader pending OR, then possibly `find_build_spot -> Objects::find_builds` |
| 1 | Village/Market Build rows | `Build::process -> Wall::process`; they are separate from the Unit idle transaction |
| 1 | step 15, per Unit | `Unit::inc_time` then `execute_events`, in object traversal order; clocks cannot be advanced for all seven as a batch |
| 5 | step 11, owner 1 | first later `plan_strategy`/MakeList authority gap in the current driver; frame zero already entered both human phase-zero strategy passes |
| 26..29 | Citizens o6..o3 | phased attrition clears Unit masks/attrition; no-op needs exact current position and WData owner, not setup geometry |
| 30..31 | Merchants o2..o1 | same phased attrition boundary; Merchant AI may already have moved them |
| 32 | Scout o0 + wildlife | Scout attrition coincides with the first unconditional wildlife cadence; wildlife owns dynamic World/RNG/spawn outcomes |
| 33 | visibility refresh | current preflight refusal skips the entire step-12 tail and therefore incorrectly skips one empty-pool `Groups::process` cursor advance |
| 64 | first recorded matched Group horizon | measured Groups survival remains 64 commands; this is an observation, not a state source |
| 133/200/233/333 | visibility/danger cadence | together with frame33, four full visibility-refresh refusals before frame379 lose cursor advances; frame200 danger may lose another tail unless typed authority is present |
| 379 | serial64 Group+Move | exact pair executes after local checksum check against the canonical live Unit state |
| 384 | Group slot0 normalization | installed nonempty Group must process here; it is not clear merely because frame385's embedded checksum is delayed |
| 391 | first changed embedded Group checksum | visibility of the earlier command, not proof of its scheduling frame |

## Frame-one Citizen prefix contract

`plan_frame1_citizen_idle_prefix` is detached and atomic. It requires:

- nonzero revision/digest and supported executable/replay/capture identities;
- center414 o2000, Market436 o2001 and the exact seven-Unit inventory;
- the live post-frame-zero/post-command Unit generation, orders, path, instance masks, countdowns,
  Guys and RNG;
- replay-derived type50 facts, while keeping those type facts separate from live masks.

The plan journals the on-map `unit_masks2 &= ~0x10`, orderless work writes and the temporary
`unit_masks2 & 0x8000` clear. No `Sim` is mutated. A typed child request is returned for healing,
boat collision, animation-gate, SetAnim, SetAngle, entrenchment or the exact ten-argument
FindBuilds call. The temporary bit remains explicitly armed across child continuations and must
be restored only after the whole `check_idle -> think` call returns. Any missing or stale child
therefore rolls back the complete journal rather than leaking a partial Unit write.

## Current measured result

This prefix improves the authority graph but cannot advance corpus survival until the installed
Great Lakes assets, Market, frame-zero Unit branches and adjacent frame-one child captures are
materialized. The last measured target result remains 64 consecutive Group checksum matches;
the first residual is upstream canonical chronology, not Group normalization arithmetic.
