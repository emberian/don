# Archived: August 2026 replay-contract tranches

These 92 pages (`replay-*.md`, `golden-*.md`; written 2026-08-09..14) are the per-lane
receipts of the "replay-first" programme: each specifies a checksum-channel contract, a
"golden" 2024 capture, or an exact-prefix producer in `crates/don-replay` that was supposed to
be validated against retail captures of a particular setup. Those captures were never taken,
so the contracts describe a frontier against an oracle that did not exist, and their status
lines ("fail closed", "conditional authority", "not installed") were the permanent state.
They were superseded on 2026-10-04 by the convergence order in `GOAL.md` ("Convergence
order"): the retail save game (`.svx`) is the primary oracle, `crates/don-state` loads and
re-emits it byte-identically and reproduces the fifteen live `check_all` words from the loaded
state, and consecutive frame-pair captures (`tools/retail-control/retailctl.py capture-pairs`,
`schema/live/frame-pairs/`) drive the per-system frame burn-down in `crates/don-state/src/systems/`.
The pages are kept with their history for the retail VAs, PDB offsets and chronology notes they
cite — read `docs/derivation/` and `docs/mechanics/` first; where a page here disagrees with
those, the live docs win. Nothing in `docs/assembly/` links here except as archived context.
