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
| coverage audit | `a5c25cf2` | landed; generator rerun completed just before cutoff | `tools/coverage-ledger.py`, `schema/coverage.json`, `docs/mechanics/COVERAGE.md` | keep regenerated with code changes; correct stale prose when wiring changes |
| join real game | `a7372f3a` | title id `84214` recovered statically from `CrossplayProxy.dll`; the apparently timed-out DLL transfer completed; no report | scratch `jrg/xfer/steam_api.dll` (217,376 B, SHA-256 `dc204ea6ad73ae127a2f6977055f861dc9c850a3c14d66e0810b8650cc1340a0`); no named report | preserve the capture, obtain a real Steam session ticket, then test the unverified shim ABI/load path in retail |
| live attach | `a6611bcc` | quarantined orphan: not exported or in the workspace; forced harness compiles but 4/6 live tests fail; Windows binary predates it | `crates/donscan/src/live.rs`; no `docs/tracks/live-attach.md` | fix the 68-vs-64-byte header, Build/Wall HP, stockpile address, frame coherence and identity/wire gaps before a measured 1–2 Hz disposable-game smoke |
| replay viewer | `a8452ffd` | substantial coherent UI; syntax checks and 3/3 browser pages pass, but default audits exit nonzero and several coverage/security claims are wrong | six viewer files under `web/public/` and `web/tools/`; no report | fix the tautological round-trip metric, expected no-stream handling, Chrome cleanup, command claims and unescaped replay HTML; then add the report/link and rerun solo+MP+full smoke |
| casters + animals | `aaa5073c` | derivation reached “write the Rust module”; no write occurred | no named module or report | resume from transcript; do not re-derive from scratch |
| wonders + nations | `aca399d9` | 475 effect sites extracted (428 call, 47 inline); runtime and completeness audit not started | `schema/effects.json`; its only generator remains temporary scratch `wn/gen_effects.py`; no report | promote and make the generator reproducible, audit the 2 dynamic subjects and missing constants, then design typed dispatch |
| air | `ac73a0c7` | module written; 37/37 focused tests passed after correcting a local change-detector fixture | `crates/don-sim/src/systems/air.rs`, module declaration; no report | write `docs/mechanics/air.md`, add the currently uncalled anti-air gate to the live ammo path, replay-measure RNG impact |
| naval | `a273b7d8` | quarantined: isolated compile needs a missing `Dock: Default`; with a temporary shim 60/61 tests pass and the failing 29-unit expectation contradicts the 30-unit XML roster | `crates/don-sim/src/systems/naval.rs`; no module declaration or report | repair the sentinel/test, recover all 289 retail spiral offsets (currently 91), and close the pathing/RNG proxies before declaring it |
| items | `a740c2fd` | quarantined: isolated compile runs 42 tests, 41 pass; the failing empty-cell test contradicts the direct retail function's caller-gated contract | `crates/don-sim/src/systems/items.rs`; no module declaration or report | correct the test/contract and `mark_seen`, document the omitted full visibility/unlink/move behavior, then integrate the channel into World/SimBridge |
| walls | `a61232fa` | module written and declared; 41/41 focused tests and the full 643-test `don-sim` suite passed | `crates/don-sim/src/systems/walls.rs`; no report | write `docs/mechanics/walls.md`, connect the currently uncalled code to build/world state, replay-measure the channel |

“Focused tests passed” means only that the transcript shows the command succeeded. The current
umbrella suite independently compiles air and walls, but neither has a runtime caller. Naval
and items are not declared by `systems/mod.rs`; their isolated audits above are deliberately
outside the green workspace.

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

The live-attach transcript created a Windows scheduled task named `DONRoN`. Its reader is
state-safe but currently broken and prior broad scans moved roughly 0.84–0.92 GiB each, so no
fresh attach was attempted during Ember's valued solo match. Inspect the guest task after the
match; repair the targeted scanner and add a frame guard before measuring it at 1, 5, and 15 Hz
on a disposable skirmish.

## Landing checklist for recovered artifacts

1. Read the lane's final transcript span and its named brief.
2. Inspect the diff without formatting unrelated files.
3. Add missing module wiring only after the isolated file compiles.
4. Run focused tests, then `cargo test --workspace --all-targets`.
5. Run `tools/replay-validate.sh` if state, scheduling, orders, or checksum code changed.
6. Add the missing lane report with explicit provenance, tier, limits, and measurements.
7. Update `GOAL.md` and this table; only then is the lane landed.
