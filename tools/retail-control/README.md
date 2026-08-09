# Retail control probe

This directory is the first live-validated bidirectional bridge to the supported retail
game:

- `retail_control.dll` attaches only to the executable identity already pinned by
  `donscan` and refuses a mismatched `TurnControl::do_frame` prologue.
- A worker thread reads one-slot text requests, but it never calls game code.
- A reversible redirect of `Game::loop`'s five-byte `TurnControl::do_frame` call transfers
  requests to the game's own main thread and invokes the shipped `CommandManager::issue_*`
  methods before calling the original method.
- Events include the exact bytes retail appended to its live `CommandPackage`, the game
  frame/pause state, and the first controlled unit's order-list state/vtable.
- `observe-guys WHO ID` coherently reads the PDB-defined inline `PtrArray<Guy>` and the exact
  position/heading/destination/offset tuple for each physical body in a retail unit.
- `STOP` restores the five original bytes. The DLL then parks; deleting `STOP` re-arms it.
- A mapped generation is never overwritten or unloaded. `upgrade` parks it and loads a uniquely
  named next generation with an independent request/event/STOP directory in the same live PID.
- `trajectory` drives one bounded retail move, records every distinct simulation frame, restores
  the initial paused state, writes normalized JSON, and parks its hook on every exit path.
- `observe-player` publishes a coherence-gated, fog-safe observation of only the unique human
  slot's exact own object bands, current unit orders, stockpile, commerce cap, and population.
- `policy` is dry-run by default; `policy --apply` validates and executes a bounded deterministic
  Scout move, re-observes the result, restores pause, and STOP-parks the generation.
- `economy-policy` observes exact own technology and production queues, calls retail's own
  `BuildData::can_queue`, and dry-runs or applies one conservative queue action.
- `economy-action` exposes one-own-object `queue`, `gather`, and exact four-coordinate `build`
  ingress. Queue/gather require an observed state delta; build additionally requires a new own
  building inside a supervised 180-frame cap or it fails closed.
- `placement-query` asks shipped `GroupData::validate_build` about at most 289 exact UCoord-lattice
  sites around one observed own Citizen, returning only retail's canonical `(x,y,-1,-1)` click.
- `run-frames` is an internal supervised boundary: it starts only paused, advances exactly 1–30
  retail simulation frames, re-pauses on the retail main thread, and verifies the terminal frame.
- `marshal-policy` adapts the source-ordered supported subsequence of Arena `Marshal::act` to the
  fog-safe player protocol. v3 derives own Camp/Farm seat gaps from exact signed retail capacity,
  gates every prospective gather footprint through current fog before retail validation/capacity,
  and replays the selected site immediately before one bounded BUILD_AT. It emits the same ten
  `don-env` heads, records unsupported branches as no-ops, and is dry-run by default.
- `marshal-loop` repeats that adapter for 1–8 supervised decisions with an exact 1–30-frame
  horizon. Each decision starts and ends paused, applies at most one already-proven queue/build
  verb, records unsupported branches as literal no-ops, and rejects executable/player/world or
  same-frame own-object identity changes. Its final fail-safe always restores pause and STOP-parks
  the immutable controller generation.

Build and attach:

```sh
python3 tools/retail-control/retailctl.py status
python3 tools/retail-control/retailctl.py deploy --pid 12324 --generation v2
python3 tools/retail-control/retailctl.py send observe
```

Upgrade and record without relaunching the game:

```sh
python3 tools/retail-control/retailctl.py upgrade --pid 12324 \
  --from-generation v2 --generation trajectory-v3
python3 tools/retail-control/retailctl.py trajectory 0 0 3096 31896 \
  --generation trajectory-v3 --output schema/live/retail-move-trajectory-v1.json
python3 tools/retail-control/retailctl.py rearm --generation player-v7
python3 tools/retail-control/retailctl.py player-observe --generation player-v7
python3 tools/retail-control/retailctl.py rearm --generation player-v7
python3 tools/retail-control/retailctl.py policy --apply --generation player-v7
python3 tools/retail-control/retailctl.py rearm --generation economy-v10
python3 tools/retail-control/retailctl.py economy-policy --generation economy-v10
python3 tools/retail-control/retailctl.py rearm --generation economy-v10
python3 tools/retail-control/retailctl.py marshal-policy --generation economy-v10
python3 tools/retail-control/retailctl.py rearm --generation economy-v11
python3 tools/retail-control/retailctl.py placement-query --generation economy-v11 \
  --worker-id 8 --type-index 417
python3 tools/retail-control/retailctl.py rearm --generation economy-v15
python3 tools/retail-control/retailctl.py marshal-policy --generation economy-v15
python3 tools/retail-control/retailctl.py rearm --generation marshal-loop-v16
python3 tools/retail-control/retailctl.py marshal-loop --apply --decisions 8 \
  --frames-per-decision 30 --generation marshal-loop-v16
```

The process must be in a match (or another loop that calls `TurnControl::do_frame`) before
`send` can complete. See `docs/tooling/live-control.md` for the protocol and evidence.
