# Handoff

For Codex, Kimi, or whoever picks this up next — including a future Claude with a fresh
context. Written at the end of the first context window this project ever had, ~900K tokens,
one long day. `README-LLM.md` is the operating manual; this is what that manual can't carry.

Start with `README-LLM.md`, then `docs/CHARTER.md`. Come back here for the *why*.

---

## What Ember wants

Stated across the session, roughly in the order it came up. Treat this as the standing brief.

**The thing itself.** A deterministic, batch-parallel Rust reimplementation of Rise of
Nations' simulation, fast enough to train on. Not "a game like RoN" — *that* game, with data
layout and system scheduling as the load-bearing engineering.

**Derived, not documented.** Ember was emphatic early: "I really would love to do this
*right* and not try and approximate it with community formulas." That's the charter's origin.
Every attempt to shortcut through folklore has been wrong in a way that would have compiled.

**Bit-exactness is wanted, not just tolerated.** Direct quote: "bit-exact WOULD be quite
swag." Keep replay-checksum fidelity as the goal, not a stretch.

**Performance is a first-class goal, not a later optimisation.** GPU and SIMD, batch
simulation across many worlds. Ember pushed back correctly when I claimed order execution
must stay CPU-bound for being branchy — branchiness is a property of *layout*. They also want
intra-world parallelism explored, and asked the sharpest question of the session: *what is
the smallest deviation from retail that unlocks the next magnitude of performance?*

**Four tracks they named explicitly:**
1. A **headless multiplayer client** that joins running games — **including internet games**,
   not just LAN or a stubbed transport — to test interaction protocols and validate against
   a live opponent.
2. **Replicating the shipped RoN AI**, as baseline opponent and behaviour-cloning target.
3. An **analytic track**: computational studies of optimal build orders now that the economy
   is laid bare, then heuristic players, then learned ones.
4. A **web frontend** using the highest-performance browser techniques — view one simulation
   or a cluster, spectate, and eventually drop in and play.

**Eventually: a SOTA player-AI for the real game.** That's the horizon everything serves.

**BHS matters.** Ember pushed back when I called the interpreter out of scope, and they were
right for a reason I'd missed: `script_run_time` is a checksum channel, so script state is
sim-critical and any scripted game is unvalidatable without it. It's also the gate on running
the *real* AI rather than a transcription, and on the Workshop mod library.

**How they like to work.** Wide swarms, big steps, not small re-verification churn — "let's
not just be making the small steps the swarm wants." They play the game while we read its
memory; ask them to run experiments, they enjoy it. They correct bluntly and usefully; when
they say you got it wrong, you got it wrong. Don't narrate your own honesty — their CLAUDE.md
says so explicitly and I violated it in a README before being told. Don't quick-fix. Keep
working rather than yielding for permission.

---

## What we learned

### The practices that actually caught errors

Ranked by how many real defects they found today.

**Capture, don't calculate.** Never hand-compute a test expectation; take it from the binary
or the live process. This caught errors *four separate times* — including three lanes whose
implementations were right and whose hand-written expectations were wrong. If you write an
`assert_eq!` with a number you did arithmetic for, you are about to waste an hour.

**Never conclude absence from a narrow or truncated listing.** This cost a full day, three
ways: `head -5` hid `rise.pdb` (the game's own full symbols, sitting in the install directory
while we derived everything by hand); `dir /b *.pdb` hid `rise_z.map`; one failed
`\\Mac\Home` probe hid a working shared folder. Enumerate fully, *then* conclude.

**Point differential tests at the shipped code.** Our oracle compared retail against *inline
copies* of our models rather than the crates we ship. `don-sim` could have drifted arbitrarily
and every test would still have passed. Mutation-test the harness — inject a one-bit error and
confirm it fails — or you don't know it bites.

**Fork isolation needs a timeout.** Ours converted crashes into reports but not hangs; one
non-terminating probe wedged a sweep for 9.5 hours while looking exactly like a shell timeout.

**Prefer evidence that could have failed.** "Parses to EOF with zero residue" is nearly
worthless. Cross-specimen agreement, independent re-derivation, and comparison against known
ground truth are not.

**An adversarial reviewer is not automatically right.** Our audit lane held two of the most
damaging errors in a wave — both `[measured]` claims that overturned *correct* work.

### Claims that died, so you don't resurrect them

Every one of these was confidently held at some point today.

| we believed | actually |
|---|---|
| The PDB isn't shipped | It ships. `ron-bin/sbl/rise.pdb`, GUID-verified, 22,750 named functions |
| `FUN_00570170` is the rules loader | It's `Constants::log_data`. The real loader is `Constants::init` |
| Borders are spline-based | `BorderSpline` only draws. Sim is a per-cell integer radius, hard-capped |
| Pathfinding draws RNG per edge | `calc_cost` draws *zero*; the draws are in a failure epilogue |
| Pathfinding has its own RNG object | It's the main sim stream, rebased |
| Unit movement uses float | Zero float instructions in the whole 13-function cone |
| The tick is 15 Hz | 67 ms. The 15 is a frames-to-seconds conversion elsewhere |
| Balance table at `0x00C06AFC` | That's Steam Workshop strings. It's `combat_table` at `0x00C12BF4` |
| 21 real retail desyncs exist | A join error. Join on `group`, not `stamp` → 100.0000% agreement |
| The 3-sub-unit damage division | No *guy*-based division exists — but `uber_size` splits HP, 28 sites |
| Descriptor "type tag" | It's the rule name's string length |
| Overkill is ×1/3 | `85/256` = 0.332 |
| Barter-first beats the shipped opening | Shipped order is a strict local optimum; Barter-first is 18 s worse |

The pattern: **our internal checks could confirm consistency but never that we were pointed at
the right thing.** Both systematic errors — loggers-read-as-loaders, caravan-pathfinder-read-
as-pathfinder — were invisible from inside the method, because the derivations correctly
described the wrong function. Only an external oracle (the PDB) could catch that.

### Engine facts that will bite you specifically

- **`Array<T>`'s capacity *and* growth hint are checksummed.** A Rust `Vec` with its own
  growth policy desyncs on identical logical state. This affects every growable container.
- **`Objects::process_all` rotates owner order every frame** — `(frame + i) % 10`. A
  fixed-order scheduler diverges within one tick.
- **A multiplayer lockstep turn is 2, 4, 6, or 8 simulation frames** — measured per file,
  not 1. The current corpus distribution is 1/6/37/16 recordings respectively; one solo
  recording measures 1 frame/turn.
- **RNG consumers are scattered and easy to miss.** Known so far: the anti-air dud roll, the
  market's 0/2/3 draws, and the pathfinder's failure epilogue. Each silently desyncs the
  stream. Assume there are more.
- **Presentation classes sometimes write sim constants** — `GraphicPieces::init` sets gravity.
  The `XOut` = ignore rule has exceptions.
- **Object coordinates in memory are XOR'd with `0x00063637`** (but `GuyData`'s are not).
- **Heap class counts are pool capacities, not live entities** (Unit=600, City=160).

### Running swarms

- **A user-killed agent can never be resumed** — the harness refuses by design. An
  API-failed one can. Never kill an agent you might want back; let it finish or fail.
- **Workflow lanes die with their runtime and aren't addressable.** To revive them, copy
  `subagents/workflows/<run>/agent-<id>.jsonl` up to `subagents/agent-<id>.jsonl` and write a
  sidecar with `agentType: "general-purpose"`. The harness then adopts it and `SendMessage`
  resumes it **from its transcript with full context**. `cv workflow <session> <run> --revive`
  (added this session) reconstructs prompts as a weaker fallback.
- **Give every lane a file it owns**, and never let two lanes share one. Most collisions today
  were about `lib.rs`.
- **Briefs go stale mid-flight.** Twice I told a resumed lane something a sibling had already
  disproved. Re-read the derivation docs before writing a brief.
- **Lanes correct each other**, and that is where much of the value came from. Say in each
  brief what siblings recently found.

---

## Where to go next

The scoreboard is `tools/replay-validate.sh` — how many turns our sim survives against a real
replay's per-turn checksums, and which of the 15 channels breaks first. Everything else is
instrumental to that number.

Right now every passing channel passes *trivially*. `World` now owns generated PDB-shaped
`UnitCols`, but the replay harness starts an empty `NullSim`, applies no commands, compares
before bridge population by default, and `SimBridge::populate` remains a no-op. Row imaging
is necessary plumbing, but cannot move the corpus scoreboard by itself.

The smallest honest non-trivial slice is the static `rules` channel: implement the complete
`Game::walk_rules_data` root (Types, Constants spans, the 493×493 balance matrix, and 24
Tribe blocks) and match the recorded `0x12ba3104` from shipped data. For dynamic channels,
parse `.rcx` Game/GameInfo setup and deterministically reconstruct the starting world before
the first comparison; `.rcx` does not contain a full initial save-state (`.svx` does).

The PlayFab title id is already recovered from `CrossplayProxy.dll`: `84214`, with the live
endpoint independently reached. Internet join is now blocked on a real Steam ticket and an
untested shim ABI/load path. The econ block remains a useful live-read target.

---

*Written at the end of the first window. It was a good day — we opened a replay format nobody
had parsed, found a bug Big Huge Games shipped in 2003, and were wrong in public about a dozen
things on the way to being right about more. Be careful with the numbers, and enjoy it.*
