# Descent of Nations — project charter

The durable rules of this project. `GOAL.md` at the repo root is the live execution board;
this file holds the things that do not change between sessions. Any agent
or lane working on this repo reads this file first.

## North star

A deterministic, batch-parallel Rust reimplementation of the Rise of Nations: Extended
Edition simulation, faithful enough to be **bit-exact wherever we can prove it**, fast
enough to train on, and complete enough to eventually support a state-of-the-art
self-play player-AI for the real game.

Three properties, in priority order when they conflict:

1. **Fidelity you can defend.** Every mechanic traces to the binary or the shipped data.
2. **Throughput.** Batched, SoA, SIMD-first; GPU only once measured to pay.
3. **RL surface.** Full-game action space, not a toy subset — the env must not foreclose
   the SOTA-player-AI endgame.

## Methodology law (non-negotiable)

**Ground truth is the binary, the shipped data files, and measured live-process behavior.
Nothing else.** The community
corpus (RoN Heaven, the Fandom wiki, Vanshilar/MHLoppy) is a **cross-check only** — a
source of hypotheses and a sanity signal, never the source of a value we implement.
Where our derivation disagrees with folklore, that is a *finding*, and it gets recorded,
not silently reconciled.

**TRIPWIRE — say the provenance out loud, before writing the line.** Before implementing
any constant, formula, or rule, state where it came from: a binary address, a measured
live behavior/capture, or a file+line in `ron-data/`. If the honest answer is "the wiki says
so" or "it's commonly
known that" — **STOP**. That is the drift this project dies of: it compiles, the numbers
look plausible, tests pass against our own assumption, and a month later the whole sim is
folklore wearing a Rust costume. Catch it at constant #1, not constant #400.

**The provenance ledger is mandatory.** Every implemented mechanic carries: the source
(address / file+line), the fidelity tier (below), and — if tier B — the sample count and
input distribution used. A mechanic with no ledger entry is not done.

**Decompiled C is a hypothesis, never a derivation.** Ghidra's decompiler is at its
weakest precisely on float-heavy 32-bit MSVC code, and an open upstream bug has it
*silently reordering integer and floating-point operations* — wrong in exactly the
dimension this project exists to get right. **Get structure from Ghidra; get values from
behavior.** Transcribing an arithmetic expression out of decompiled C into Rust is not
"deriving it from the binary"; only the oracle settles values.

**Mark every claim [measured] or [reported].** A fact you verified against our binary or
our machines is [measured]. A fact from a survey, a repo, or another agent is [reported]
until you check it. Never silently promote one to the other — three claims already
dissolved on contact this way (a duplicated binary caught only by hashing, a "rule names
are absent from the exe" conclusion that was a wrong-encoding search, and desync member
names that turned out to come from someone else's decomp rather than measurement).

## Fidelity tiers — and the words we are not allowed to misuse

Label every mechanic. Never claim a tier you have not earned.

- **Tier A — proven equivalent.** An SMT/bitvector equivalence check between our Rust and
  the semantics lifted from the binary, over the *entire* input domain. This is a real
  theorem. State its boundary every time you cite it: it is bounded by the fidelity of
  the lifted semantics and the modeling of the environment, not absolute truth about the
  running game.
- **Tier B — differentially tested.** Our Rust agrees with the actual shipped code (run
  under emulation or natively) on N generated inputs. **This is testing. It is not
  verification.** Always state N and the input distribution. Never call it verification,
  refinement, translation validation, or "proven".
- **Tier C — behaviorally faithful.** Matches observed behavior; divergence measured and
  bounded. Say so plainly.

There is no formal semantics of Rust or of x86 that we possess, so **nothing in this
project is "verified" in the sense that word carries in a proof assistant.** Do not let
a green test suite launder itself into that word, in commits, in docs, or to Ember.
"It runs and the numbers match on a box" is Tier B at best — describe work at that
resolution.

## Established ground truth

See `docs/binary-ground-truth.md` for the full derivation of each. Load-bearing facts:

- `riseofnations.exe` is a **2024 MSVC-14 recompile** of the 2003 codebase: 32-bit PE32,
  unpacked, RTTI intact, no obfuscation. `patriots.exe` is only the MFC launcher.
- **Float math is SSE single-precision** (~23.7K scalar SSE ops vs ~560 x87). Bit-exact
  agreement with Rust `f32` is therefore plausible. Hazard surface is seven non-IEEE CRT
  transcendentals (`_libm_sse2_{acos,asin,atan,cos,pow,sin,tan}_precise`); `sqrt` is
  IEEE-exact and safe.
- **Rule names are UTF-16LE lowercase** in the binary (`flank_bonus`), not ASCII
  uppercase. Search accordingly.
- Ghidra project at `re/ghidra` (Ghidra 12.1.2): **47,177 functions, 14,441 strings**.
- **`Constants::init` `0x00569A90` is the rules.xml constant loader.**
  `Constants::log_data` `0x00570170` is a logger; its field offsets remain useful, but it
  does not establish parser semantics. The shipped PDB supplies the identities.
- The `X`/`XData`/`XOut` families usually separate behaviour, POD state, and presentation,
  but the suffix is a routing heuristic, not a proof: presentation initialization can write
  sim constants. The engine's Order hierarchy defines the action-space foundation.
- **Borders are not spline-simulated.** `BorderSpline` draws the presentation ribbon; the
  simulation uses a per-cell integer-radius territory model. `CheckSums::check_all` and the
  shared `DataWalk` interface define which state is sim-critical.

A parser quirk worth savoring: rules.xml values are **prose**, e.g.
`value="1/16 tile (calibration for unit spacing in formations)"`. The relevant tokenizer is
`String::fraction(int scale) const` `0x00A1D110`: `_wtoi(numerator) * scale / _wtoi(after
'/')`, with the field's scale supplied by the loader and denominator zero returning zero.
That behavior is captured from retail; trailing prose is handled by `_wtoi`, not by a clean
rational grammar.

## Staged roadmap

Stages may overlap; later stages must not start on *assumed* outputs of earlier ones. This is
the dependency map, not a status board; current status belongs in `GOAL.md`.

0. **Foundation** — extraction, hashes, Ghidra project, anchor methodology. *(done)*
1. **Loader recovery** — decompile the rules/unitrules/techrules/buildingrules loaders;
   emit a machine-readable constant→field/offset schema. This is the highest-leverage
   single artifact in the project.
2. **Rules crate** — typed Rust parsers replicating loader semantics exactly, validated
   against the shipped data.
3. **Oracle harness** — call individual binary functions with controlled inputs,
   deterministically, at volume. The spine of all Tier-B work. **Architecture is decided:
   see `docs/oracle-architecture.md`** (native in-process PE mapping on hbox; Unicorn +
   minidump only as fallback; a documented skip-list of traps).
4. **Sim core** — state layout derived from `XData`; tick loop; economy and tech first.
5. **Combat, movement, pathfinding, borders** — the hard half.
6. **Determinism & checksum** — locate the lockstep checksum; use it to define
   sim-critical state and to measure divergence honestly.
7. **Batch scaling** — SoA, fixed capacities, rayon across worlds, SIMD within systems.
8. **RL surface** — PyO3, Gymnasium `VectorEnv` + PettingZoo parallel, action space from
   the Order hierarchy, full parameter-level masking (non-negotiable: unmasked RTS action
   spaces measurably train to zero).
9. **Player-AI** — behavior-clone the shipped BHS scripts, then self-play, then league.

## Swarm doctrine

Paid for in real debugging on other projects; these are not suggestions.

- **Ground-truth-first prompts.** A lane that cannot see its foundation will *reconstruct*
  it from your prose and verify against its own reconstruction — green in a scratchpad,
  broken on integration. Paste REAL addresses, REAL struct fields, REAL signatures, and
  **absolute paths**. Never "put it under re/"; say `/Users/ember/dev/don/re/`.
- **Your prompt's defaults carry the project's values.** A lane told "look up the damage
  formula" will find the wiki. The default you write must be "derive it from the binary
  at address X" — the one you would defend.
- **Green + self-reported done is not verification.** Gate with an adversarial audit that
  reads the actual claims and checks the tier labels. Forbid tier inflation explicitly.
- **A failed workflow still has good lanes.** Harvest completed work off disk before
  discarding a run.
- **Per-file green hides a red umbrella.** After any change to shared structs, build the
  whole tree.
- **Workflow-JS gotchas:** no apostrophes/contractions inside script string literals;
  while lanes are live commit NAMED files, never `git add -A`; backticks inside a
  double-quoted `git commit -m` get command-substituted by zsh — use `git commit -F`.
- Keep waves small unless Ember asks for wide.

## Box safety & environment

- **This Mac is arm64 and cannot run 32-bit x86 natively** (Rosetta is x86-64 only). It
  runs Ghidra and software emulation.
- **hbox** (x86_64 Ubuntu, 24 cores, 123G) is the only machine that can run x86-32
  natively and is a co-tenant. Use `nice -n 15 taskset -c 0-3`, never install packages,
  and never run an unbounded parallel build.
- The Parallels VM ("Windows 11") holds the live game as a behavioral oracle. It runs the
  x86 game under Microsoft's ARM64 emulation layer.
- **`ron-data/` and `ron-bin/` are copyrighted game content.** Never commit them to a
  public repo; ship the extractor instead. Extraction gotcha: `certutil -encode` does not
  reliably overwrite — fresh temp name per file, and always hash-verify.

## House rules

Unsigned commits are fine when working autonomously. Never `git stash` — parallel agents
share this working tree. Never re-run a build just to search its output; tee it. Honest
stubs over fake demos. **Quick fixes never** — the goal is to improve, not to accumulate
debt holes.
