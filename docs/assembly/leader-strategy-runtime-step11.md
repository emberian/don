# leader strategy runtime — canonical step-11 transaction

Tick step 11 reaches `Leaders::strategy_all` at retail `0x006ED430`. The 93-byte
dispatcher, `Leader::check_explore`, the entry/phase of `Leader::plan_strategy`, and the
complete 628-byte `Leader::production_ai` step machine were already recovered. This tranche
closes the largest child of that machine whose complete inputs already have canonical Sim
owners: `Leader::queued_units` `0x006CE000`, 394 bytes.

Everything below is **[measured], Tier C** from `ron-bin/riseofnations.exe` (SHA-256
`30478a44…625079`), `re/decomp-all/006ce000.c`, and the PDB layouts in
`schema/pdb-types.json`. It has not been differentially executed against retail.

## Exact queue join

Retail starts at object index 2000 and walks the calling leader's Build band to its live
mark. A building contributes only when its folded virtual predicates `is_valid()` and
`is_active()` both return nonzero. For every logical queue entry it then:

1. reads the queued `TypeIndex` from `BuildQueueEntry +0x04` (20-byte stride);
2. accepts only Unit-shaped TypeData;
3. calls `LeaderData::type_avail(type, 1)` and requires the raw result to be greater than
   3 (the installed production table and canonical leader TechState represent this admitted
   state as `class == Unit && can_make && prerequisites held`);
4. adds `UnitTypeData::control` at `+0x2F0`, with 32-bit wrapping arithmetic.

The adapter uses `ObjectRegistry::slot(owner).band(Build)` rather than filtering the dense
`Sim::builds` vector. That preserves the retail object-list order and rejects stale band
bindings, wrong owners, malformed logical queue lengths, and missing Type rows before any
Leader byte changes.

`Leader::production_ai` consumes the result unconditionally after its human/AI-off/
production-disabled gates:

```text
effective_pop (+0x9E0) = queued_units() + control (+0x940) + 1
```

The queue answer is installed only during the dispatcher call and then restored. It is a
derived adapter value, not another persistent `LeaderData` field.

## Atomic dispatcher receipt

`leader_production_ai::strategy_runtime::execute_strategy_all` preflights every queue join,
temporarily projects the canonical `ai_off` and `starting_resources` values, invokes the
existing complete dispatcher, and returns:

- the exact `StrategyTrace`, retaining per-slot
  `check_explore -> plan_strategy -> compute_score(0) -> diplomacy` ordering and the
  independent victory tail;
- every visited Build object and every admitted queued Unit row;
- before/after snapshots of the retail-owned fields at their PDB offsets;
- Adler-32 of the sparse owned projection before and after.

The owned snapshot is versioned (`DoNAI11`, version 1). It includes leader flags/who,
`production_step`, `prod_script_run`, `script_step`, `control`, `explored`, and
`effective_pop`; it deliberately excludes host answers such as queued population, script
return values, and the MakeList head. A focused resume test executes stage 3, encodes and
decodes the snapshot, reinstalls external type/build inputs, then proves stage 4's trace,
after-image, and checksum match an uninterrupted run.

## Why `production_ai_setup` remains red

`Leader::production_ai_setup` `0x006C83E0` is 1,807 bytes, but its existing inputs are not
complete. The body writes `LeaderDataEncrypt::rate[6]` at encrypted-data offset `+0xAC`
(PDB name `rate`) and later calls `Leader::market_speculation` `0x006C8110` (707 bytes).
The current `LeaderEcon` owner has stockpile, accumulator, cap, capped flag, gross,
expense, displayed income, and breakdown, but not the distinct rate block. Substituting
`displayed` or stockpile would make a green test around an aliased PDB field. This tranche
therefore keeps `ProductionAiSetup` as the named stage boundary.

## Canonical-owner integration boundary

The current `step8.leaders[*].ai` structure is an execution adapter, not a valid save
owner. `victory_score::LeaderState` already owns and serializes the canonical LeaderData
record, including `leader_flags2`, `multi_diff`, per-type queue counts, and economy state.
Before the transaction is hooked into `Sim::leaders_strategy_all`, its six persistent AI
scalars must move to (or be mirrored from) that canonical owner and the `LEADER_MATCH`
DoNSave section must carry them. The tick hook should then only:

1. project `command::InlineState::ai_off` and
   `vic_match.options.starting_resources`;
2. run this transaction with the canonical ObjectRegistry/Build/type owners;
3. commit the owned after-image to `victory_score::LeaderState`;
4. preserve the existing compute-score and diplomacy interleaving.

No tick or DoNSave hook is claimed by this isolated tranche.
