# `market_speculation` quote and trade transaction

The standalone continuation in
`crates/don-sim/src/systems/leader_market_speculation_transaction.rs` executes the two loops
after the previously recovered entry/scarcity pass. It adds no module registration, tick hook,
or save-format edge.

## Ownership boundary

The continuation does not reimplement market arithmetic. Its host delegates to the existing
complete canonical children:

| PE child | VA | canonical owner |
|---|---:|---|
| `LeaderData::calc_market_prices` | `0x006DC2A0` | `systems::economy::calc_market_prices` |
| `Leader::do_sell` | `0x006CFC60` | `systems::economy::do_sell` |
| `Leader::do_buy` | `0x006CFBD0` | `systems::economy::do_buy` |

Thus the shared `MarketState`, decoded `LeaderEcon::stockpile`, and the production owner's
plain `escrow[6]` are mutated once by their established owners. `tell_embargo` remains an
ordered presentation effect rather than a fake Sim mutation.

## Sell pass

For each resource retail calls `type_avail(resource, 1)` before skipping wealth 2 and knowledge
3. It requires the signed wrapping surplus `bucket[r] - econ[r]` to reach
`2000 / 2^scarcity`, quotes the resource, and requires sell price at least
`75 / (scarcity + 1)`. Only then does it recheck Nubian-or-Commerce unlock, market presence,
and embargo in that order. A clear embargo executes exactly one `do_sell` attempt.

## Buy pass

The same type and 2/3 exclusions precede an unconditional quote for each remaining available
resource. Admission requires all retail comparisons:

```text
buy <= bucket[wealth] - econ[wealth]
bucket[r] < 2000
buy < 201
bucket[r] < 500 || buy < 26
bucket[r] < 200 || buy < 51
bucket[r] < 100 || buy < 101
```

The unlock, market, and embargo rechecks then occur in the same order as the sell pass. A clear
embargo executes exactly one `do_buy` attempt. Earlier buys can change the wealth balance and
market base price observed by later resources; the host reads canonical state at each retail
read rather than snapshotting the loop.

## Evidence

Focused tests pin the quote-before-gates call order, all scarcity thresholds, signed wrapping
surplus, unavailable/non-trade skips, embargo effect ordering, and actual canonical quote/sell/
buy mutations. Three deterministic four-seat worlds run one natural transaction generation,
cross a real current-format `save_sim`/`load_sim` boundary for the existing leader economy and
shared market owners plus the frozen 132-byte production-owner projection, then compare the next
generation's ordered calls and complete canonical state with uninterrupted execution.

The fixtures begin with ordinary stockpiles and market prices. The runtime performs only retail
buy/sell children; no test or adapter grants resources, completes builds, or overwrites an
outcome to force convergence.
