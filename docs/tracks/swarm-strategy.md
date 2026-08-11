# Swarm strategy: how to close the ledger fast

Written 2026-08-11, after a day that landed real work but hit four avoidable walls. This
replaces ad-hoc lane dispatch. It is about **throughput without giving up the evidence
discipline** — the two are not in tension, and the day's near-misses show why.

## What actually walled us

Diagnosed from one day of four-lane operation, not from theory.

**1. The orchestrator was the bottleneck.** Every lane funnelled through one serial path:
verify → write a commit message → commit → push. Four lanes consumed most of the
orchestrator's turns. Adding lanes made this *worse*. Any strategy that leaves a single
agent on the critical path for coordination cannot scale past about four lanes.

**2. `persvati` sat idle all day.** 24 cores, 78 GB free, zero jobs. `README-LLM.md` says
"at most four compiling lanes concurrently" — that is a **Mac** constraint that had been
treated as a global one. `hbox` is a different case: it is memory-tight (a co-tenant HOL
build holds ~105 of 123 GB), so it should be reserved for the i686 oracle, which is the one
thing only it can run.

**3. Ownership was handed out by directory.** Giving a lane `crates/don-sim/**` means one
lane per crate, and `don-sim` is where most of the backlog lives. The ownership collisions
that followed were an artifact of the dispatch shape, not a real constraint.

**4. Lanes re-derived the same facts.** Two lanes independently rediscovered the internal
string-table decode on the same day. Every such rediscovery is a lane-hour spent on
something already written down.

## The four changes

### 1. Compile off the Mac

`tools/swarm-cargo-remote submit persvati <lane> -- <cargo args>` already exists. Use it.
Remote jobs start from a full tracked checkout of the pushed `HEAD` and overlay only files
named with repeated `--path`, so a lane must push or overlay explicitly.

| host | use for | constraint |
|---|---|---|
| `persvati` | general build/test lanes | 24 cores, ~78 GB free. The default remote. |
| `hbox` | the i686 oracle (`tools/oracle-regress.sh`) | memory-tight; co-tenant build. Be polite. |
| this Mac | orchestration, Ghidra/Capstone, web, live VM work | keep ~4 compiling lanes max |

This lifts the concurrent-lane ceiling from about four to ten-plus without touching the
co-tenant's memory.

### 2. The closure inventory is the backlog; `cv task` is the queue

`schema/simulation-closure.json` already carries, per red row: a name, a retail VA, a
status, and an evidence note. That **is** a work breakdown structure — it does not need to
be reinvented as prose.

`cv task` provides claim (first-writer-wins), note, propose, review pass/refute, and
**git-observed landing** — `cv task verify` resolves the branch tip and computes the range
patch-id from git itself, so landing is observed rather than reported. `cv task debt` is the
honest unlanded view; `cv task inbox <who>` is what a lane should check first.

Because claiming is first-writer-wins, **collisions resolve themselves**. The orchestrator
stops arbitrating ownership.

### 3. Assign by inventory row, not by crate — and let lanes share a crate

A lane claims a **row** and declares its **file set**, not a directory. One-lane-per-crate
was an early over-correction: it caps the swarm at about six lanes when the backlog has 36
red opcodes, 33 red group actions and 18 blockers that are mostly independent of each other.

Same-crate concurrency is safe because of an idiom the codebase already follows everywhere:
**one module per recovered mechanic** (`*_frontier.rs`, `*_integration.rs`) plus a single
export line in `systems/mod.rs`. Two lanes each adding a module are not in conflict. The
genuinely hot files are few — `tick.rs`, `command.rs`, `command_tables.rs`,
`order_dispatch.rs`, `systems/mod.rs`, `don-env/src/action.rs` — and those get claimed
explicitly and edited in minimal hunks.

Coordination runs through **`docs/tracks/megaswarm-board.md`**, an append-only noticeboard:
lanes post their file claims, post an **API CHANGE** note the moment they alter a shared type
so siblings braid forward instead of hitting a mystery compile error, and post **FINDINGS**
so nobody re-derives what a sibling already established. A red build may be a sibling
mid-migration — check the board and work something else rather than "fixing" their file.

### 4. Kill re-derivation

Every lane prompt must say: **grep `docs/assembly/` and `docs/mechanics/` before opening
Ghidra**, and `re/decomp-all/<EA>.c` before that.

Capabilities discovered on 2026-08-10 that no earlier lane knew about, and that belong in
every prompt until they are folklore:

- **`tools/pdb-extract` works on any PDB**, not just `rise.pdb`. It emits virtual methods
  with their introducing vtable slot. This is how the Crossplay ABI was recovered:
  `cargo run --release --manifest-path tools/pdb-extract/Cargo.toml -- <pdb> <base> <sym.json> <types.json>`
- **The complete shipped data set is local.** `ron-data/` now holds all 228 shipped data
  files, not the earlier 47. A "missing shipped file" diagnosis is now almost always wrong.
- **Internal string references decode as** `int_str_array` `0x00c06378` → `[[0xc06378]+0x10]`
  is an array of 20-byte `String` records, so `add eax, N` is `20 * index` into
  `internal_strings.xml` in document order. Filenames in `.text` are usually *not* literals.
- **Parse shipped XML as XML.** A `<STRING hash="…">` regex finds 7,622 of
  `internal_strings.xml`'s 7,630 elements, and the eight it drops precede ordinal 5950 — so
  a regex index is off by exactly eight where it matters.

## The discipline that does not get traded for speed

Three claims on 2026-08-10 were wrong and were caught **only** by independent cross-check:

- an orchestrator claim that `ICrossPlayService` has 97 slots — the shipped interface is a
  *different class* with 58, and every slot of the 97-record vendor table disagrees;
- an orchestrator claim that extracting `tilesets.xml` merely masked a boundary — it
  advanced the generator a full stage on all 21 checksum-bearing recordings;
- a lane trusting the PDB's `static void __cdecl (NetMsg_SyncSignal*, NetPlayer const*)`
  when the emitted body takes no stack parameters at all and reads only `ECX`.

Each would have compiled, passed its own tests, and been wrong. **Velocity without
adversarial verification would have landed all three.** `cv task`'s `pass`/`refute` with the
independence check is the mechanism, and it is *cheaper* than a manual orchestrator pass,
not more expensive.

Concretely, a reviewer's job is to check the claim against the binary, not to re-read the
diff: the tier stated, the sample count on any Tier-B claim, and whether a green test could
have failed at all.

## Burn-down

The metric is `python3 tools/simulation-closure.py --check`:

```
124 red  →  0
  checksums 1/15   ← the fidelity scoreboard underneath everything
```

Checksums are **downstream**: `units` cannot agree until units do the right things. So the
sprint is mechanics-first with the checksum row as the scoreboard, not as the work.

## Sequencing

1. Finish the in-flight lanes.
2. **Crossplay implementation** — needed for DoN-only hosting to work properly, and the
   58-slot ABI is already pinned in `crates/don-crossplay`.
3. The sustained ledger sprint on this substrate.

Live retail attachment is explicitly **parked**. The replay corpus — 61 recordings, 585,152
turns, 488,557 checksum packets, whole corpus in ~30 s — *is* the retail-compatibility
harness, and it is deterministic, offline, and not gated on a human at a keyboard. Live
attachment adds nothing to checksum agreement until the simulation can produce the fifteen
channels. Resuming it later costs one run: the sync-barrier work is landed and carries a
self-localising diagnostic.
