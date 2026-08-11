# Canonical economy/containment Group host contract

Status: **exclusive source contract only; zero runtime or closure delta**. The module is not
registered in `systems/mod.rs`, and the command bridge does not call it. It freezes the
production transaction boundary which can later close the `BOARD_SHIP`, `REPAIR`, and `TRADE`
Group actions and opcodes together after the shared Groups and order-payload prerequisites land.

## Why this cohort is first

The shipped PE and PDB make these three rows a bounded family:

| action | VA / PDB bytes | direct simulation writes |
|---|---:|---|
| `Group::action_board_ship` | `0x00700010` / 1,149 | Group form; passenger `BOARD_SHIP`; ship order clear; ship `AWAIT_BOARD` per passenger |
| `Group::action_repair` | `0x007020C0` / 999 | Group form; member `REPAIR`; optional typed `CAST_SPELL`; target mask/order/path/action retirement |
| `Group::action_trade` | `0x00701CC0` / 1,022 | Group form; typed two-endpoint `TRADE_ROUTE` installs |

Direct disassembly of the retail executable was compared with the existing recovered planners.
Board calls `Unit::add_board_order` `0x005E4D10`, `Unit::clear_orders` `0x005E3860`, and
`Unit::add_await_board_order` `0x005E4C80`. Repair calls `Unit::add_repair_order` `0x005E4FF0`,
`Unit::add_cast_order` `0x005E4A60`, and the target retirement sequence. Trade calls
`Unit::add_trade_order` `0x005E4DC0` with both object endpoints. Their common world reads are
the selected Unit identities, on-map/type predicates, `WorldData::get_tregion` `0x006B52E0`,
and `Region::is_coast` `0x00680F90` where reached.

This is six potential strict rows—three actions and their three opcodes—behind one owner. The
adjacent actions are not folded into this first boundary:

- `city_gather` and `gather_point` share `Build::{clear,add}_gather`, but `gather_point` can
  delegate the 3,398-byte `Group::action_flight` and both require live Build gather save state;
- `gather` reaches a 120-position world search, garrison/inside mutation, cast installation,
  `action_move_near`, and the still-red `GATHER` executor;
- `spell` has a 4,100-byte capability, mana, target, payment, cast, production, and order cone;
- `transport` creates and places a Unit before containment; `build` validates placement and
  delegates `action_swarm_around`; `queue_up` still has research, Library, aircraft, and
  producer/type compatibility branches.

Treating those as the same atomic implementation would merely hide independent open tails.

## Retail replay and v16 save evidence

The frozen wire/owner boundary was checked against the retail playback
`Playback - 2026.08.11 11'44'38 (Tue).rcx`
(`SHA-256 558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`).
The decoder reached exact EOF after 60,402 packages and 68,811 commands. In that playback,
every observed economy action had an opcode-0 command earlier in the same package:

| action | paired / observed | selected-count distribution |
|---|---:|---|
| `QUEUE_UP` | 311 / 311 | 205 cached, 101 one, 3 two, 1 three, 1 four |
| `BUILD` | 91 / 91 | 47 cached; explicit selections ranged from 1 to 20 |
| `GATHER_POINT` | 9 / 9 | 7 cached, 1 one, 1 six |
| `GATHER` | 3 / 3 | 3 one |
| `SPELL` | 3 / 3 | 1 three, 1 four, 1 thirteen |
| `GARRISON` | 2 / 2 | 1 cached, 1 five |
| `EJECT_ALL` | 1 / 1 | 1 one |

Here “cached” means opcode 0 has count zero and reselects its retained `(o, uid)` cache; it
does **not** mean an empty Group. Consequently the action request cannot be captured before
selection resolution. `CanonicalGroupSelectionReceipt` binds the package chronology, cache
revision and identities, fixed-slot allocation/removal after-images, member Group backlinks,
`last_group`, and before/after state digests. Production packet ingress must use
`snapshot_from_selection`; the lower-level `snapshot` remains only an isolated action-test seam.

The fresh retail v16 save `new save game 2026.08.11 15'42'57 (Tue).SVX`
(`SHA-256 161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7`)
contains one exact player-major `Array<Group>` at decompressed offset `0x4610b`: length and size
512, Group data `0x46116..0x4f1fa`, and `last_group = [4,65,130,192,256,320,384,448]`.
All 512 retained ids equal their absolute fixed slot, including touched empty slots. This is
save evidence for `groups_guys::Groups` as the canonical allocator/checksum owner and against
admitting `command::Groups` as a second authority.

The same executable/order walk fixes the v12 typed suffix contracts:

| tag | payload | v12 suffix | complete retail walk |
|---:|---|---:|---:|
| 2 | Gather v1 | 20 bytes | 31 bytes |
| 3 | Cast v1 | 8 bytes | 27 bytes |
| 4 | Trade v1 | 18 bytes | 29 bytes |

The replay contains no Board, Repair, or Trade commands. It therefore corroborates the shared
packet-selection host and payload shapes but does not supply dynamic closure evidence for the
first six action/opcode rows.

## The owner split that must be removed

`Sim`, DoNSave, and `CheckSums::check_groups` own
`systems::groups_guys::Groups`: 8 player bands, 64 fixed slots each. `command.rs` currently owns
another private `Groups` pool and applies economy plans piecemeal through `Fleet`. A receipt from
that path is not canonical evidence. It also has known semantic losses:

- a Trade install drops the second endpoint because the flat order has no Trade payload;
- a Repair companion drops the spell index, and target retirement omits path/action columns;
- Board's queue-first path replays the saved selection to the wrong owners.

`EconomyGroupTransactionRequest` therefore accepts only `groups_guys::Groups`. The real packet
constructor first validates the typed selection receipt, then captures owner, fixed slot,
retained Group id, the complete Group before-image, scenario ignore-orders state, raw action,
and a complete-state digest. The host must revalidate the slot, id, image, and digest immediately
before its first write. A separately constructed `GroupData`—including one copied from
`command::Groups`—cannot create the request.

The future shared command callback should carry only the canonical locator and raw action into
the Sim owner. It must not pass a Bridge-planned Group after-image or mutate per-member queues
before the callback returns.

## Exact payload and queue prerequisites

The contract requires the common DoNSave v12 typed envelope. Capabilities are checked only when
the reached retail plan needs them, while closure evidence must cover every installing branch:

- Cast v1: generic primary target/x/y plus concrete `paid` and `spell` history;
- Trade v1: generic primary target/UID/Handle plus second `o/who/uid`, `started`, `loaded`, and
  a stable second canonical Handle;
- exact Group queue-first: save only the leader queue's `GROUP` nodes, issue the new action as
  raw `QUEUE_NEW`, then reissue supported cloned actions tail-to-front with raw `QUEUE_LAST`.
  Gather and Cast are copyable; Trade is deliberately not—retail `copy_order` returns null.

Board has its separate queue-first ship-selection choreography. Repair needs Cast v1 only on a
branch that actually installs its `0x293` companion. Trade needs Trade v1 on every accepted
member. Unknown facts never become false: every reached conditional must be known before the
plan can cross the preflight boundary.

## Atomic receipt and save proof

An applied receipt carries the original canonical request, stable target/member identities,
complete facts, capabilities, recomputed action plan, exact Group after-image, and one atomic
commit evidence record. Validation requires:

1. the plan recomputes from the request and facts;
2. the committed Group equals the computed after-image;
3. save/reload reproduces the complete after-state digest and Group image;
4. one direct next tick and one post-reload next tick produce equal complete-state digests.

The handler's invalid-object branch is represented as `action_reached=false` and must leave the
whole state unchanged. `ScenarioData::ignore_orders != 0` remains explicitly unavailable until
its pruning prelude receives its own recomputable canonical transaction.

The digest is evidence framing, not a substitute for typed state. Its future Sim implementation
must cover the fixed Group slot, Object registry generations and UIDs, Unit/order/path columns,
and every reached Build/World field. Mutation tests must tamper each identity, before-image,
payload capability, saved digest, and resumed-tick digest.

## Frozen integration order

1. Land the v12 order envelope with Gather, Cast, and Trade payloads plus exact Group queue-first.
2. Route opcode-0 selection through the fixed `groups_guys::Groups` owner and emit the typed
   allocation/cache receipt before creating an action request.
3. Register this module and add one shared canonical Group callback used by the movement and
   economy cohorts.
4. Implement the Sim adapter over World registry/orders/paths and fixed Groups; apply every
   planned write only after complete preflight.
5. Extend DoNSave only through the v12 payload owner, prove save/reload and resumed-tick equality,
   then run real 9/13/21-byte packets through the canonical adapter.
6. Only then promote the three action/opcode pairs and regenerate closure evidence.

Until those steps land, this contract earns no `complete` or `state_wired` row.
