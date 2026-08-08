# Wonders, nations, and rare-resource effect ledger

`tools/effects-ledger.py` is the reproducible generator for
`schema/effects.json`. It scans the shipped executable rather than treating XML
or community descriptions as an execution oracle.

## Reproduction

The tool discovers the repository from its own path; it does not depend on the
current working directory or an Ember-specific absolute path. Its Python
dependencies are `pefile` and `capstone`.

```sh
python3 -m pip install pefile capstone
python3 tools/effects-ledger.py /tmp/effects.json
python3 tools/effects-ledger.py --check /tmp/effects.json
```

With the pinned executable and recovered PDB inputs, the census is:

| kind | sites |
|---|---:|
| explicit predicate calls | 428 |
| inlined `has_rare` bit tests | 47 |
| **total** | **475** |

The predicate split is 101 `has_wonder`, 320 `has_tribe_bonus`, and 54
`has_rare` sites. There are no direct `has_rare_conquest` call sites in this
census. This is Tier C static evidence: the tool has not executed the retail
predicate bodies as an oracle.

## The two dynamic subjects

Both unresolved subjects are genuinely dynamic API boundaries, not failed
literal recovery.

| site | conclusion |
|---|---|
| `0x006DA479`, `LeaderData::has_unbuilt_wonder` | The subject is the wrapper's `int` parameter, with the wonder domain `526..542`. It has three direct call sites: `TypeData::get_cost` passes `SUPERCOLLIDER` (541) and `SPACEPROGRAM` (542); `LeaderData::team_has_unbuilt_wonder` forwards its own checked non-negative parameter. |
| `0x009EA093`, `ScenarioFuncSet::has_rare_resource` | The subject begins as a script `String`. `ScenarioFuncSet::get_type_index` resolves it to `eax`; failures return `-1`, resolved values `0..5` return true directly, and values `6..49` are passed to `LeaderData::has_rare`. No single static TypeIndex exists for this site. |

The generated records therefore keep `subject: null` and
`subject_confidence: "dynamic"`, but their notes now state the recovered domain
and dispatch behavior.

## Constants-read audit

The scratch generator kept a register marked as a Constants base after
instructions such as `mov eax,[eax+off]`. Any later object, vtable, or Setup
read through that register could then be mislabeled as another Constants slot.
It also treated alignment NOP operands and write-only memory operands as reads.

The promoted generator clears provenance on overwrite/dereference, requires a
Capstone read access, and excludes multi-byte alignment NOPs. Relative to the
currently checked-in `schema/effects.json`, regeneration:

- preserves all 475 effect sites;
- removes 103 false Constants-read records across 73 sites;
- reduces sites with at least one Constants read from 352 to 348;
- fills all six remaining unnamed records from the PDB `Constants` layout:
  `+0x680 spanish_extra_scout`, `+0x574 aztec_move_speed`,
  `+0x548 eiffel_siege_range` (two sites), and
  `+0x534 liberty_free_upgrades` (two sites);
- leaves zero unnamed Constants reads.

The false records included convincing-but-wrong names whenever an unrelated
object offset happened to equal a valid Constants offset. For example, the
`LeaderData +0x6EB8` pointer dereference in the Japanese cost/damage branches
made its obfuscated resource fields at `+0xDC/+0xE8` appear to be
`civic_upgrade_terr`; vtable reads such as `call [eax+0x60]` appeared to be
`overkill_damage`; and eight alignment NOPs appeared to read
`unit_formation_spacing`.

`schema/effects.json` is intentionally not rewritten by this recovery lane.
Until the corrected output is reviewed and promoted, checking that path will
report the expected ledger drift:

```sh
python3 tools/effects-ledger.py --check schema/effects.json
```
