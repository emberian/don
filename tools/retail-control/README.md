# Retail control probe

This directory is the first bidirectional bridge to the supported retail game:

- `retail_control.dll` attaches only to the executable identity already pinned by
  `donscan` and refuses a mismatched `TurnControl::do_frame` prologue.
- A worker thread reads one-slot text requests, but it never calls game code.
- A reversible redirect of `Game::loop`'s five-byte `TurnControl::do_frame` call transfers
  requests to the game's own main thread and invokes the shipped `CommandManager::issue_*`
  methods before calling the original method.
- Events include the exact bytes retail appended to its live `CommandPackage`, the game
  frame/pause state, and the first controlled unit's order-list state/vtable.
- `STOP` restores the five original bytes. The DLL then parks; deleting `STOP` re-arms it.

Build and attach:

```sh
python3 tools/retail-control/retailctl.py status
python3 tools/retail-control/retailctl.py deploy --pid 5236
python3 tools/retail-control/retailctl.py send observe
```

The process must be in a match (or another loop that calls `TurnControl::do_frame`) before
`send` can complete. See `docs/tooling/live-control.md` for the protocol and evidence.
