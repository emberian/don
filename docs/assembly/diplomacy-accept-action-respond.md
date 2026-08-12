# Opcode 41 `Leader::action_respond` transaction

`crates/don-sim/src/systems/diplomacy_accept_host.rs` recovers the complete mode-one
body of `Leader::action_respond(target, 1)` at `0x006D03C0` (3,988 bytes, 1,102
instructions). It is an exclusive source-only transaction: it is deliberately not exported
through the shared command, tick, or save owners yet. A relation-only or resource-only handler
is not a valid opcode-41 closure.

The supported executable and shipped PDB identify the called bodies used by this recovery:

| VA | PDB name |
|---:|---|
| `0x0043ED10` | `LeaderData::bucket_add` |
| `0x0047DEC0` | `Diplomacy::num_attacks` |
| `0x00594670` | `Game::war_allowed` |
| `0x006D1360` | `Leader::consider_tribute` |
| `0x006D13F0` | `Leader::notify_deal` |
| `0x006D5240` | `Leader::scale_tribute` |
| `0x006D6250` | `LeaderData::num_allies` |
| `0x006EBAA0` | `LeaderData::is_enemy` |
| `0x006EBAE0` | `LeaderData::is_neutral` |
| `0x006EBD30` | `LeaderData::is_team` |
| `0x006EBF90` | `LeaderData::num_team_members` |
| `0x006EDB50` | `LeaderData::is_ally` |

## Exact ordered stages

| PE range | recovered transaction stage |
|---|---|
| `0x006D03D8..0x006D03E7` | Clear accepter `response_334[target]`, then `response_314[target]`. These writes survive every later early return. |
| `0x006D05A9..0x006D0893` | On first accept, preflight six goods before mutation. Each available-good offer is `max(forward.offer, 0)`. Each reached present, proposed, non-enemy attack overwrites—not sums—the good's DOW scratch with `base * (is_ally+1) * flags.bit4`. Debit the authoritative and `LeaderData+0x468` images with a zero clamp, credit `+0x498` escrow, retain nonzero DOW costs in the forward record, set pending, and OR the reciprocal `+0xB4` agenda cell with four. The local shortage UI gate records that a DOW-cost query was reached even when its result is zero. |
| `0x006D0894..0x006D0D05` | Scan forward attack candidates for an alliance with either party. A conflict refunds and clears both agreements and both complete proposal records. Otherwise inspect reciprocal pending. A nonreciprocal deal with treaty, attacks, or positive reciprocal offers waits; a completely empty deal may commit immediately. |
| `0x006D0D06..0x006D0DF8` | If the accepter's directional treaty is alliance (`2`), each party must satisfy `1 - num_team_members(1) + num_allies() == 0`. The first failed result clears both agreements and records before presentation. |
| `0x006D0E36..0x006D0FFA` | Reuse the accepted-resource authority: refund and clear both directional DOW reservations, transfer reciprocal then forward offers for each mutually available good, scale receiver credits, debit/clamp raw escrow, update sent/received statistics, and call `consider_tribute`. |
| `0x006D1000..0x006D10A6` | If the accepter's directional treaty is not `-1`, call `set_diplo` twice with `max(that same proposal, current_raw)`: accepter-to-proposer first, proposer-to-accepter second. Treaty one stamps both `LeaderData+0x2D0` peace frames. The duplicated proposal load at `0x006D100C` and `0x006D105A` matters when the two records disagree. |
| `0x006D10A7..0x006D11BB` | If either party is neutral, scan present third parties. A candidate on exactly one party's runtime team inherits that party's worse relation toward the other party through full `set_diplo`, followed by `notify_deal`. |
| `0x006D11BC..0x006D12AE` | Query `Game::war_allowed`. For every forward attack candidate, stamp both `+0x1B4` frames and `+0x1D4` peer identities. Each non-enemy party recursively declares war through the landed opcode-38 relation/fanout authority. The third `action_declare` argument is a **no-payment** flag: flags bit 4 clear skips affordability, payment, and declaration statistics; bit 4 set runs them. |
| `0x006D12AF..0x006D1340` | Clear the forward record, then the reciprocal record. A content-bearing deal presents acceptance only when the proposer is local, then calls `notify_deal` for accepter and proposer using the accepter's directional treaty. |

## Atomic authority and projection rules

`AcceptAuthorityImage` joins the already-landed `DeclareAuthorityImage`, the accepted-deal
resource owner, and the accept-only agenda/peace/attack fields. Before planning it requires
identity across all overlapping projections:

- authoritative, `LeaderData`, command, and accepted-resource bucket images;
- command and accepted-resource escrow;
- installed availability results;
- every directional offer and declaration-cost cell;
- the opcode-38 relation, flag, identity, local-player, and frame projections.

The receipt compares the complete before-image, recomputes the full plan, requires the exact
ordered mandatory authority list, and publishes only the complete after-image. Mandatory
children include `consider_tribute`, every root/fanout `set_diplo` authority, every nested
opcode-38 `set_diplo` authority, and `notify_deal`. Stale state, a projection disagreement,
or any missing/extra child leaves the image unchanged.

For a recursive no-payment declaration, `RecursiveDeclarePlan` retains a self-consistent
opcode-38 carrier plan for the complete relation/fanout child authority and an explicit
`effective_after` image. The latter restores payment and declaration statistics because the
general `action_declare(..., no_payment=1, ...)` body skips those opcode-38 mutations. This is
not a scalar relation stub: the nested plan still owns both relation rows, runtime-team
fanout, declaration frame behavior, and all mandatory army/object/vision consequences.

## Executable proof

`crates/don-sim/tests/diplomacy_accept_host.rs` path-includes the exclusive module so the
shared module table stays untouched. Its eight cases prove:

1. first-accept reservation and reciprocal wait;
2. the queried-but-zero DOW-cost shortage presentation gate;
3. bilateral scaled transfer, peace stamps, notifications, and record clearing;
4. asymmetric proposal records use the accepter's treaty for both root calls;
5. attack-conflict refund and complete rollback;
6. alliance-admissibility rollback;
7. paying and no-payment recursive declarations reuse self-consistent opcode-38 authority;
8. stale state, missing authority, and projection disagreement never partially commit.

The clean Linux overlay run
`diplomacy-accept-action-respond-20260812T004814Z-84770-18624-7c389597de5f`
finished with all eight tests passing. The local standalone `rustc --test` harness also passes
all eight.

The normalized replay cohort remains the external opcode identity gate: 101 opcode-38
commands in 24 files and 1,415 opcode-41 commands in 26 files, with manifest SHA-256
`92f255aa5e94bc503cfed8262ed47daeaa74d2819c654deefe5cd683151a97a9`.

## Integration boundary

This closes the missing body as source/test authority, not the shared executable row. Opcode
41 must remain red in the mounted command ledger until one canonical `Sim` aggregate owns and
saves all proposal, bucket, escrow, relation, statistic, agenda, and attack fields and a real
Bridge packet applies this receipt. No shared command, tick, save, schema, or module-export
hunk belongs to this exclusive recovery.
