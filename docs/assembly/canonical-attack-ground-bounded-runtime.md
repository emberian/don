# Canonical bounded ATTACK_GROUND runtime

This tranche mounts a real opcode-9 subdomain without promoting the whole command row. The
canonical packet host accepts a strict `[Group][ATTACK_GROUND]` pair only when every effective
member is an ordinary on-map Unit authorized for the ground receiver, the queue byte is retail
`QueuePos::New`, the clamped target cell is unowned, and the scenario/leader side-effect tails
are empty. The prior OrderList must also be empty: retail's `close_orders(0)` can run per-order
epilogues, so a nonempty queue remains refused until those are transaction-owned. It installs the
exact `AttackGroundOrder` words `(x,y,accuracy=0,attack_unit=0)`, sets
`ORDER_GROUP`, closes the prior queue, clears the partial path and `unit_masks & 0x04000000`, and
publishes the fixed Groups/backlink/cache transaction atomically.

The non-scenario retail fixture is
`playback___2014.08.13_19_26_39__wed_.rcx`, package 23836 / turn 23837 / play 0 /
frame 190162: Group `0007019f0548064b064e06540662068806`, action
`0952410000a5e3000002`. It selects seven owner-1 Units and targets `(16722,58277)` with
Queue-New. A second fixture proves persisted play-keyed cache reuse: package 14660 of
`playback___2014.04.26_16_17_03__sat_.rcx` carries Group `000003` and action
`09970a01005e3e000002`; its eight-member explicit cache origin is package 14654.

The 64-path corpus contains 239 strict opcode-9 pairs: 154 explicit and 85 zero-member cached;
all use Queue-New. Effective selection is Build-only in 225 packets and Unit-only in 14. There
is no singleton Unit witness. Consequently the canonical Unit host admits the observed
multi-member loop and does not claim coverage of the dominant Build receiver.

DoNSave tag 9 now preserves all four ground-order words and rejects a foreign-kind or
coordinate-mismatched typed payload. Legacy payload-free nodes still round-trip, but the frame
adapter refuses them rather than inventing `accuracy` or `attack_unit`. After reload,
`Sim::do_frame` reaches one exact bounded
`Unit::do_attack_ground` activation through its compact scheduler adapter: `ORDER_GROUP` bypasses
the recharge prelude, `recharging > 0`, target in range,
facing flag clear, and current animation in `0..=3`. Retail and the existing PE-derived planner
both perform no mutation, callback or RNG draw in this recharge-hold frame.

This is not a claim that the surrounding retail `Unit::process`/`Unit::work` frame is complete:
the compact scheduler still names its omitted Unit prelude and Guy/animation work as gaps.

The opcode row remains red. Owned terrain/diplomacy, Build receivers, clamping evidence,
containment and aircraft delegation, Queue-First/Last, reposition search, local presentation,
special-cast redirect, angle/animation writes, ammo firing and recharge publication are all
explicitly refused.
