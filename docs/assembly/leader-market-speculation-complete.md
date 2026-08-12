# Complete `Leader::market_speculation` composition

`crates/don-sim/src/systems/leader_market_speculation_complete.rs` composes the recovered
opening/scarcity and quote/trade runtimes into the complete 707-byte parent at `0x006C8110`.
It remains path-imported evidence: no module registration, step-11 hook, or save-version edit.

## One canonical transaction

The adapter requires one host to provide both the mutable production-economy row and the live
planning-target reads used by the trade phase. It snapshots only immutable entry facts before
borrowing that row. Consequently:

1. unlock, complete `has_market`, complete nuclear-embargo, and starting-resource gates run in
   retail order;
2. the scarcity pass clamps `econ[6]` and derives scarcity from the same canonical stockpiles;
3. only `ReadyForSellPass` enters the exact sell then buy loops;
4. those loops re-read the clamped planning targets and live post-trade stockpiles/prices;
5. exact `calc_market_prices`, `do_sell`, and `do_buy` owners perform every mutation;
6. embargo presentation stays an ordered external effect.

Every entry early return omits the trade phase entirely. The opening can emit only scarcity
levels 0, 1, or 2, so the composed transaction cannot manufacture a detached threshold.

## Full retail child map

| child | VA | status in composition |
|---|---:|---|
| `LeaderData::get_nuke_embargo` | `0x006D52C0` | complete canonical owner/query |
| `LeaderData::has_market` | `0x006D5410` | complete Build-list query |
| `LeaderData::calc_market_prices` | `0x006DC2A0` | existing canonical economy owner |
| `Leader::do_sell` | `0x006CFC60` | existing canonical atomic mutation |
| `Leader::do_buy` | `0x006CFBD0` | existing canonical atomic mutation |
| `Leader::tell_embargo` | `0x006CFAF0` | ordered presentation effect only |

Tribe bonus, Commerce research, resource availability, wonder, relationship, and Build-list
answers remain explicit canonical facts. The adapter neither guesses them nor serializes them a
second time.

## Evidence and mount gate

Focused tests prove opening clamps feed the trade thresholds, exact buy mutation follows the
same row, complete embargo state prevents all scarcity/trade writes, and each entry gate omits
the second phase. Three deterministic four-seat worlds execute the full parent, then cross a
real current-format save/load for existing leader economy and shared market state plus the frozen
production and nuclear owner projections. Their next complete receipts and all canonical state
match uninterrupted execution.

No resources or build completions are injected. A step-11 mount still requires a coordinated
v15 Leader-row extension for rate, production economy, and nuclear embargo owners; this evidence
does not bypass that format gate.
