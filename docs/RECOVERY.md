# Recovery ledger — Claude session `62b78482`

Source: top-level Claude session `62b78482-846c-4ffd-a44c-2199d3744a8e`, inspected with
`cv` on 2026-08-08. This is the only top-level Claude session for the project.

The session launched six earlier persisted workflows and one final workflow whose original
state did not land cleanly. All seven now resolve through `cv workflow`; the final run's useful
state survives as ten agent transcripts plus files in the shared worktree.

## Final-wave incident

- Workflow: `don-coverage-wave`
- Run id: `wf_f9ba6b8d-f3e`
- Outcome: recovered workflow record says completed with ten agents, ten errors, and an empty
  lane result; there are no successful journaled lane returns.
- Cause visible in all ten transcripts: Claude session usage limit around 18:51–18:58
  America/New_York. The referenced 22:30 time was the quota reset, not the incident.
- Safety rule: do not delete or overwrite an artifact below until its transcript has been
  checked and the artifact has passed an integration gate.

## Lane-by-lane recovery state

| Lane | Agent | Recovered state | Artifacts on disk | Next action |
|---|---|---|---|---|
| coverage audit | `a5c25cf2` | landed and re-audited; generator now labels seeded reachability and textual references honestly | `tools/coverage-ledger.py`, `schema/coverage.json`, `docs/mechanics/COVERAGE.md` | keep regenerated with code changes; runtime execution still needs a real call/path analysis |
| join real game | `a7372f3a` | title id `84214` recovered statically from `CrossplayProxy.dll`; the apparently timed-out DLL transfer completed; no report | scratch `jrg/xfer/steam_api.dll` (217,376 B, SHA-256 `dc204ea6ad73ae127a2f6977055f861dc9c850a3c14d66e0810b8650cc1340a0`); no named report | preserve the capture, obtain a real Steam session ticket, then test the unverified shim ABI/load path in retail |
| live attach | `a6611bcc` | recovered into a targeted leader-only `donfeed` path: exact image allowlist, read-only imports, coherence/mode/pause/human guards, 15/15 tests and Windows cross-build green | `crates/donscan/{src/live.rs,src/bin/donfeed.rs,fixtures/,examples/}`, `docs/tracks/rontoy.md`, `tools/rontoy-host/` | deploy at 1 Hz in a controlled solo match; run HUD, MP suppression, pause/restart/fault, and observer-effect acceptance gates |
| replay viewer | `a8452ffd` | landed and audited: default corpus checker exits zero, hostile strings are escaped, browser cleanup/seek checks pass, and scope is labelled partial | viewer files under `web/public/` and `web/tools/`; `docs/tracks/replay-viewer.md` | decode state blobs and execute sim/WASM only as a later separately measured lane |
| casters + animals | `aaa5073c` | recovered as a bounded Tier-C module; 23/23 focused tests green | `crates/don-sim/src/systems/casters_animals.rs`, `docs/mechanics/casters-animals.md` | integrate casting/animal orders, allocation, hunting/food, checksum state, and a retail oracle before runtime use |
| wonders + nations | `aca399d9` | reproducible 475-site effect ledger landed; corrected register provenance and named all constants | `tools/effects-ledger.py`, `schema/effects.json`, `docs/mechanics/effects-ledger.md` | typed runtime semantics and differential tests remain separate work |
| air | `ac73a0c7` | module and Tier-C report landed; 37/37 focused tests green | `crates/don-sim/src/systems/air.rs`, `docs/mechanics/air.md` | connect the still-uncalled anti-air gate to the ammo/world path and replay-measure RNG impact |
| naval | `a273b7d8` | recovered, declared, and documented; 30-unit roster and all 289 offsets fixed; 65 focused tests green | `crates/don-sim/src/systems/naval.rs`, `docs/mechanics/naval.md` | keep pathing, boarding, gull objects, dock queues, and supply quarantined until full retail behavior/oracle work lands |
| items | `a740c2fd` | recovered, declared, and documented; caller contract, visibility, bit semantics, and object unlink repaired; 49 focused tests green | `crates/don-sim/src/systems/items.rs`, `docs/mechanics/items.md` | integrate World/SimBridge state and the deferred terrain transaction before claiming replay impact |
| walls | `a61232fa` | module and Tier-C report landed; two checksum/bit defects repaired; 43/43 focused tests green | `crates/don-sim/src/systems/walls.rs`, `docs/mechanics/walls.md` | connect the still-uncalled code to build/world state and replay-measure the channel |

“Focused tests passed” remains narrower than end-to-end fidelity. The recovered mechanics are
now compiled by their parent crates and documented, but air/walls/naval/items/casters still
lack the complete runtime state and retail-ordered callers described in their reports.

The join lane also validated the title endpoint with
`POST https://84214.playfabapi.com/...LoginWithSteam`: a dummy ticket reached PlayFab and
returned `InvalidSteamTicket` (1010). This proves the binary-derived title id, not the shim or
authentication path.

## Useful `cv` commands

```sh
# Whole sub-agent forest and persisted workflows
cv show 62b78482-846c-4ffd-a44c-2199d3744a8e --subagents
cv workflow 62b78482-846c-4ffd-a44c-2199d3744a8e

# Read one debris transcript (replace the agent id)
cv show 62b78482-846c-4ffd-a44c-2199d3744a8e --agent a273b7d8

# Last portion without resolving the whole large transcript
cv show 62b78482-846c-4ffd-a44c-2199d3744a8e --agent a273b7d8 --range 150-

# File-level provenance across the session
cv events 62b78482-846c-4ffd-a44c-2199d3744a8e --subagents
cv blame crates/don-sim/src/systems/naval.rs
```

The handoff narrative from the top-level session is in `CODEX.md`. `.revive/` contains the
older reconstructed lane prompts used to resume several pre-coverage-wave tasks; preserve it
until the dirty worktree is checkpointed and every named artifact is classified.

At recovery time, several `prlctl exec ... ReadProcessMemory` jobs from the Claude session and
the scratch `jrg/srv.py` upload server were still alive roughly five hours after their nominal
timeouts. After Ember explicitly authorized attach and cleanup, the exact inventoried host
PIDs were terminated; the game process was not targeted. Audit guest processes afresh before
starting another attach rather than assuming a timed-out host client left no detached child.
The replay-viewer audit also found and terminated its orphaned loopback server on port 8791.

The live-attach transcript created a Windows scheduled task named `DONRoN`. The new reader is
a foreground targeted process and does not depend on that task. Inspect/remove the old guest
task before the next deployment, then start the new feed at 1 Hz and measure 1/5/15 Hz only in
controlled skirmishes; the earlier broad scanner moved roughly 0.84–0.92 GiB per scan and is
not part of RoNtoy R1.

## Landing checklist for recovered artifacts

1. Read the lane's final transcript span and its named brief.
2. Inspect the diff without formatting unrelated files.
3. Add missing module wiring only after the isolated file compiles.
4. Run focused tests, then `cargo test --workspace --all-targets`.
5. Run `tools/replay-validate.sh` if state, scheduling, orders, or checksum code changed.
6. Add the missing lane report with explicit provenance, tier, limits, and measurements.
7. Update `GOAL.md` and this table; only then is the lane landed.
