# `Leader::set_diplo` deterministic transaction

This is the convergence contract for `Leader::set_diplo(int j, int state)` at
`0x006EC6A0` (783 bytes). The executable model lives in
`crates/don-sim/src/systems/leader_set_diplo.rs`; its standalone contract tests live in
`crates/don-sim/tests/leader_set_diplo_transaction.rs`.

The important correction to the previous state-only summaries is that `set_diplo` writes
**both directional declaration cells**. At `0x006EC869` it writes
`self.diplos[target]`, then at `0x006EC870` it writes
`leaders[target].diplos[self.who]`. Calling it once is therefore a symmetric raw-state
mutation even though `LeaderData::get_diplo` remains a mutual-minimum predicate.

## Retail evidence

| body | VA | relevant effect |
|---|---:|---|
| `Leader::set_diplo` | `0x006EC6A0` | relation, vision, ejection, victory, army, UI tail |
| `Leader::eject_my_shit_from_his_ass` | `0x006D0220` | eject/kill contained units and restore air strafe order |
| `Leader::ally_diplo` | `0x006D0120` | `set_diplo` plus local-only ally notification |
| `Armies::diplo_change` | `0x006F30F0` | forced `Army::process(1)` in valid slot order |
| `Leader::action_declare` | `0x006DAB50` | DOW payment, statistics, `set_diplo`, team fan-out |
| `Leader::pay_dow` | `0x006D2B10` | six-good debit of both resource images |
| `Leader::action_respond` | `0x006D03C0` | accept/reject deal, transfer, treaty and callbacks |
| accepted resource loop | `0x006D0E36..0x006D0FFA` | declaration refund and two-direction transfer |

## Exact `set_diplo` order

The retail order is observable and must not be split across ticks or receipts.

1. Read `self.diplos[target]`. If it already equals `state`, return before every other
   read or write.
2. If the old raw value is `ALLY`:
   1. clear target's bit in `self.shared_vision`;
   2. clear self's bit in `target.shared_vision`;
   3. walk all objects owned by self and eject units contained by target;
   4. walk all objects owned by target and eject units contained by self.
3. If neither party is `Console.local_who`, query the scary-console leader's
   `has_treaty(self, 1)` and then, only if true, `has_treaty(target, 1)`. When both are
   true, enqueue the typed foreign-relation announcement and sound `0x135`. The text kind
   is war for state 0, alliance-broken for old 2 -> state 1, peace for other state 1, and
   alliance for state 2. This presentation occurs **before** either declaration write.
4. Write `self.diplos[target] = state`.
5. Write `target.diplos[self] = state`.
6. If the new state is `ALLY`:
   1. set self's vision bit for target when `self.has_preq(0x2b0)` or the global shared-
      vision byte is enabled;
   2. apply the same short-circuit query for target's vision bit for self;
   3. scan leader slots 0..7, excluding both parties. A slot participates only when
      `(leader_flags & 3) == 3`. Count it only when it is allied to neither party;
   4. if the count is zero, call `self.victory(0, 0)`, then set victory-mask bit 22.
7. Call `Armies::diplo_change(self.who)`. Its owner gate is
   `flags&1 != 0`, `flags&0x0c != 4`, and `flags2&0x0a == 0`; every valid preallocated
   army is then processed as `Army::process(1)` in slot order.
8. Set `IFaceData+0x22a = 1`.

Only step 3 is presentation. Ejection, victory and forced army processing are simulation
authority calls and are mandatory parts of an applied receipt.

### Contained-unit subtransaction

`eject_my_shit_from_his_ass` walks the owner's live object array in its existing order.
For every unit whose `ObjectData::get_inside` reports the revoked ally:

1. call `Unit::come_out(0)`;
2. on a nonzero return, call the unit's `vt+0x158` (`Object::kill`) with reason 0;
3. on a zero return, read `UnitTypeData::domain`;
4. for domain 2, query order `0x136`; when absent, enqueue
   `add_strafe_order(-1,-1,-1,-1,1,QueuePos(2),0)`.

The planner uses stable object ids and preserves the host-provided object-list order. It
does not substitute a set or sort.

## Exact fail-closed facts

Facts are required only after retail's own short-circuit reaches them.

| reached branch | facts which must be present |
|---|---|
| entry | actor and target identity, actor's raw target relation |
| old relation `ALLY` | both vision bytes; both complete ordered owner object lists |
| matching contained unit | `come_out(0)` return; after success, domain; for domain 2, order-`0x136` result |
| changed relation | local-player id |
| both parties non-local | scary-console treaty result for actor; target result only if actor result is true |
| new relation `ALLY` | each party's `has_preq(0x2b0)` result; global vision result only after a false preq |
| each third leader in ally scan | flags; identity and the two mutual relation pairs only when `(flags&3)==3` |
| army tail | actor flags; flags2 only when the first flags gate passes; complete 16-slot valid image only when owner is enabled |

`SetDiploPlanError` names every missing category and object identity. An error returns no
after-image. `SetDiploReceipt::validates` recomputes the plan and requires the complete,
ordered `SetDiploAuthority` list. It cannot treat relation writes alone as applied.

`SetDiploPresentation` is a separate type from both `SetDiploMutation` and
`SetDiploAuthority`. A product renderer can localize or suppress it without gaining access
to simulation mutation, and presentation is not accepted as proof that a simulation call
ran.

## Opcode 38: declaration payment and continuation

`action_declare(target, state, no_payment, override)` performs affordability checks before
mutation. On the normal paid path, ordering from `0x006DACF2` is:

1. `pay_dow(target, state)`;
2. when the old effective relation is ally and the requested effective state is war,
   `pay_dow(target, state)` a second time immediately;
3. update declaration statistics and, when the old relation is allied, the declaration
   frame;
4. call `set_diplo(target, state)`;
5. scan active third parties. If a third party teams with exactly one changed party and the
   new state is worse than its corresponding raw declaration, call
   `third.ally_diplo(other_party, state)`;
6. perform local presentation/callbacks.

One `pay_dow` walks goods 0..5. For every available good it computes the DOW cost, subtracts
and clamps the authoritative resource bucket, recomputes the same cost, then subtracts and
clamps the `LeaderData+0x468` mirror. `plan_declaration_payments` reproduces that order and
supports the retail double-payment arm.

The opcode-38 command boundary must stay unavailable unless affordability, payment,
statistics, the root `set_diplo`, every transitive `ally_diplo`, and presentation envelopes
commit under one receipt.

## Opcode 41: accepted resource transfer and treaty continuation

The accepted-deal resource loop at `0x006D0E36` runs once proposal validation and escrow
reservation have succeeded. For each good 0..5 it performs:

1. credit accepter's bucket with `accepter.proposal[proposer].declaration_cost`;
2. credit proposer's bucket with the reciprocal declaration cost;
3. clear accepter's declaration-cost field;
4. clear proposer's declaration-cost field;
5. query good availability for accepter; only when true query proposer;
6. transfer proposer -> accepter:
   1. scale raw positive offer with proposer's `scale_tribute` and credit accepter;
   2. debit raw amount from proposer's `LeaderData+0x498` escrow;
   3. add raw amount to proposer sent statistics;
   4. add scaled amount to accepter received statistics;
   5. call `accepter.consider_tribute(proposer, raw, good)`;
7. transfer accepter -> proposer in the same credit/debit/stat/callback order;
8. clamp accepter escrow to zero, then proposer escrow to zero.

`plan_accepted_deal_resources` owns this exact loop. Its tribute scaling uses the recovered
rounding bands: no bias below 20, `+50` below 100, and `+99` at 100 or above, after clamping
the percent to 1..100.

After all six goods, `action_respond` continues in this order:

1. if treaty is not `-1`, call `set_diplo` for accepter with
   `max(treaty, current_raw)` and then for proposer with the corresponding maximum;
2. for treaty 1, stamp both deal frames;
3. when either party is neutral, fan worse declarations through third-party team edges,
   including `notify_deal` presentation;
4. process attack proposals, which may recursively enter `action_declare`;
5. clear both 92-byte proposal records;
6. issue local accepted-deal presentation and `notify_deal` calls.

Opcode 41 therefore remains unavailable until the resource plan, both root `set_diplo`
plans, third-party fan-out, attack declarations, record clearing and typed callbacks share
one atomic host receipt.

## Hostile opcode 42

The non-pending reject arm enters `action_respond(target, 2)`. Its hostile branch can find
an attack counterproposal and recursively call `action_declare` before clearing the pair.
That recursion owns DOW payment, `set_diplo`, team fan-out and local callbacks described
above. The branch cannot be represented honestly by clearing proposal records first; opcode
42 remains unavailable until the recursive declaration receipt is nested in the reject
transaction.

## Integration map

No shared dispatcher file is changed by this pack. Convergence should:

1. expose `systems::leader_set_diplo` beside `diplomacy_command_plans`;
2. add a single diplomacy-host receipt capable of nesting declaration-payment,
   accepted-resource, `SetDiploPlan`, transitive `ally_diplo`, proposal-clear and callback
   plans;
3. resolve `DiplomacyBoundary::DeclarationResourceAndDiploChange` with the opcode-38 order
   above;
4. resolve `DiplomacyBoundary::AcceptTransferAndDiploChange` with the opcode-41 order above;
5. resolve `DiplomacyBoundary::RejectCounterproposal` only after its recursive declaration
   (if any) is included;
6. keep 38/41/42 state-wired red until receipt validation proves every mandatory authority
   call; presentation delivery is reported separately.

Per lane instruction, no build, test, formatter or remote job was run while producing this
pack.
