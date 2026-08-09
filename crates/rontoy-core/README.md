# rontoy-core

`rontoy-core` is the deterministic advice/lifecycle layer for RoNtoy. It takes
a versioned abstract telemetry snapshot and returns ranked coaching cards plus
raise/update/retraction events. It does not read process memory, own a clock,
render UI, or mutate the game.

Safety policy:

- partial, incoherent, stale, paused, or ambiguously attributed snapshots emit
  no cards;
- unknown/multiplayer modes emit no cards: the first live adapter targets only
  a confirmed human in a single-player game;
- legal population over-cap is not treated as a problem;
- population advice requires an evidenced live, already-paid queue;
- idle-worker advice requires evidenced free gather slots;
- labor rebalance requires capacity, path, and marginal-gain evidence;
- empty production advice requires a per-producer queue, explicit intent, and
  an observed affordable option (aggregate class counters are not accepted);
- build/technology notices require a player-tracked goal;
- military-pressure rules default off and require directly visible enemies;
- modeled projections say which fixed-income/no-spend assumptions they use.

Every card has a stable key, category, severity, confidence, evidence with
provenance, TTL, and cooldown. `AdviceEngine` applies sustain hysteresis,
resolution hysteresis, per-key cooldowns, and a global new-card rate limit.
