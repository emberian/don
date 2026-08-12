# Canonical Board/Repair/Trade package host

Status: production packet installation and DoNSave v13 persistence are executable. One exact,
substantive `Unit::do_trade` rejection branch now runs through production `Unit::work`; the
remaining TradeRoute executor cone and the other economy executors remain open, so opcodes
15/16/17 and their group actions are not marked closed by this tranche.

## Integrated boundary

`Sim::process_economy_command_package` accepts exactly one opcode-0 Group selection followed by
one of the recovered retail packet bodies:

- `BOARD_SHIP` (15): ship object and queue position;
- `REPAIR` (16): target object, target owner, and queue position;
- `TRADE` (17): both object endpoints, both owners, and queue position.

The host uses the same canonical selector as Group→Move. Explicit selection refreshes the
play-keyed `(o, uid)` cache; a zero-count Group packet reuses it. Fixed `Groups` allocation,
old-group removal, Unit backlinks, action-specific masks, order lists, and partial paths are
prepared as detached after-images. Commit revalidates the command revision, entire Groups image,
both external authority snapshots, RNG state, and every affected Unit/order/path before one
assignment-only publication. A refusal after selection planning therefore publishes neither the
selection nor the action.

The save/resume gate also sends a second zero-count Board packet after load. That packet resolves
the persisted play-0 cache to the same generational passenger identity and reuses the same fixed
Group slot; no renderer or process-local object id participates.

The external economy adapter is deliberately not saved. It binds retail addresses to the live
sparse object registry and supplies the facts absent from generated columns, including scenario
`ignore_orders`. A loaded simulation must reinstall the same revision/digest-bound adapter before
processing another package.

## v13 order authority

DoNSave v13 owns the retail `RecycledOrderNode::metric` byte and the typed economy leaves already
reserved by the v12 envelope:

| tag | concrete payload | installed by this cohort |
|---:|---|---|
| 0 | target-only | `BOARD_SHIP`, `AWAIT_BOARD`, `REPAIR` |
| 2 | Gather suffix | codec authority only in this tranche |
| 3 | Cast suffix | repair companion `CAST_SPELL` |
| 4 | two-endpoint Trade suffix | `TRADE_ROUTE` |

Unit-band targets require a live generational `Handle`. Build/Wall targets use their save-owned
sparse registry row and retain the retail address and UID without inventing a Unit handle. Metrics,
typed payloads, both Trade endpoints, and optional handles round-trip exactly; foreign tags,
versions, kind/payload crosses, malformed identities, and pre-v13 economy payloads fail closed.

## Executable evidence

`canonical_economy_group_save_resume.rs` drives real packets through production `Sim` owners for
all three actions:

- Board installs `BOARD_SHIP` on the passenger and `AWAIT_BOARD` on the ship;
- Repair applies the measured cast-then-repair call chronology through retail's
  insert-before-current list primitive, yielding current `REPAIR`, then `CAST_SPELL`, then the
  pre-existing order, and retires the target;
- Trade consumes the full 1,022-byte-derived frontier and saves both sparse Build endpoints.

Each accepted path passes fixed Groups/World publication, v13 save/load/resave, external-authority
reinstallation, and a direct-versus-loaded `Game::do_frame`. The same test also forces the
scenario `ignore_orders` refusal after detached selection planning and proves byte-identical state,
unchanged cache revision, Groups checksum, and RNG.

The Queue-Last Trade fixture retains a pre-existing `THINK` node. After packet installation and
save/load, `Unit::work` reaches `Unit::do_trade` (`0x005ED270`) through the production order-15
jump-table arm. A revision/digest-bound snapshot supplies the still-external caravan, endpoint,
transport, prerequisite, road, and recovery facts. The admitted branch is exact and deliberately
narrow: both endpoints are foreign, prerequisite `0x2AC` is absent, feedback is false, and a
queued successor makes `think_caravan` unreachable. The PE-derived frontier consequently proves
the mutation list is renderer `set_anim(0,0,1)` followed only by `kill_current_order(0)`. The
runtime CAS revalidates the generational actor, complete typed TradeOrder, order list, path, Unit
columns, authority snapshot, and RNG, then exposes the saved `THINK` node. Direct and resumed
ticks produce equal receipts and canonical after-images without consuming RNG.

The retail comparisons come from the landed Capstone/PDB authorities for
`Group::action_board_ship` (`0x00700010`), `Group::action_trade` (`0x00701CC0`),
`Group::action_repair` (`0x007020C0`), and their concrete `Unit::add_*_order` calls. This tranche
does not upgrade that disassembly evidence to retail differential evidence.

## Honest remaining gates

- `Sim::unit_work` still does not dispatch `BOARD_SHIP`, `AWAIT_BOARD`, `REPAIR`, or `CAST_SPELL`.
  `TRADE_ROUTE` dispatches only the establishment-refused/no-feedback/queued-successor subpath;
  destination scan, route establishment, city/caravan links and income, roads/A*, arrival,
  movement insertion, contact, teardown, and `think_caravan` remain fail-closed.
- Queue-First depends on the unrecovered `set_up_insert` / recursive action / `finish_insert`
  chronology and is rejected before mutation.
- Gather tag 2 is lossless in v13 but no Gather group action was integrated here.
- No megaswarm closure row should flip until the concrete production executor mutates canonical
  containment/economy owners and the same save/resume gate observes that mutation.
