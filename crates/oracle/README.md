# oracle

Maps `riseofnations.exe` into a live process and calls retail functions directly, so our
Rust reimplementation can be differentially tested against the actual shipped code.

**Builds only for 32-bit x86.** It executes the retail image in-process, so the host
process must be i686. It is deliberately excluded from the workspace: an arm64 Mac cannot
build or run it (Rosetta is x86-64 only), so **`cargo test` at the repo root never touches
it — a green `cargo test` is evidence about the Rust crates only, never about a fidelity
claim.**

## The regression suite

`regress` runs **every** case in `src/registry.rs` and reports pass/fail/skip with sample
counts. It is what turns a Tier-B claim from a recollection of one manual run into a
measurement with a date on it.

```sh
tools/oracle-regress.sh              # from the Mac: sync to hbox, build, run, fetch JSON
tools/oracle-regress.sh --status     # report on the last record locally, run nothing
```

or directly, on an x86_64 Linux host with `data/riseofnations.exe` present:

```sh
rustup target add i686-unknown-linux-musl     # musl ships self-contained CRT objects,
                                              # so no 32-bit system dev packages needed
nice -n 15 taskset -c 0-3 cargo build --target i686-unknown-linux-musl --bin regress
./target/i686-unknown-linux-musl/debug/regress --json out.json
./target/i686-unknown-linux-musl/debug/regress --list          # the registry, run nothing
./target/i686-unknown-linux-musl/debug/regress --only flank_level --scale 0.01
```

| exit | meaning |
|---|---|
| 0 | every registered case ran **and** agreed with its Rust model |
| 1 | a mismatch, a crash, or unreadable output from a case |
| 2 | a case was SKIPPED — it produced no evidence |
| 3 | the harness could not start |

There is no exit code for "mostly fine". A case that cannot run reports zero mismatches,
which is indistinguishable from agreement unless the runner refuses to call it green.

## Adding a case

Edit `REGISTRY` in `src/registry.rs`. A case is data: target VA, calling convention, input
plan, and a **reference to the Rust function we ship**. If the target's ABI matches an
existing `Plan` variant, no new code is needed at all.

The one rule: `model` must name the shipped function (`don_sim::…`, `don_rules::…`), not a
copy transcribed into the oracle. A differential test against a copy proves the copy. Where
no shipped implementation exists yet the model lives in `src/models.rs` and the `model`
string says so out loud, so the JSON carries the gap rather than hiding it.

## Layout

| file | what |
|---|---|
| `src/registry.rs` | the case list, plus `KNOWN_GAPS` — Tier-B claims this suite *cannot* re-run |
| `src/run.rs` | executors, fork isolation, the JSON record |
| `src/image.rs` | mapping, relocating, calling, SHA-256, the fork helper, the selftest, the fake TEB |
| `src/models.rs` | models for targets with no `don-sim`/`don-rules` implementation yet |
| `src/damage_env.rs` | the fabricated world `ObjectData::get_damage` needs |
| `src/damage_test.rs` | scenario generation and comparison for the damage pipeline |
| `src/main.rs` | probes: `selftest`, `info`, `call`, `vectors`, `sweep` |
| `src/bin/rng.rs` | the standalone RNG probe |

## Other commands

- `oracle selftest` — executes hand-written machine code, so a failure is unambiguously the
  harness rather than the retail image.
- `oracle info` — maps the image and prints section geometry.
- `oracle call <va|rva> [args…]` — calls a function cdecl-style, in a forked child.
- `oracle vectors` / `oracle damage-vectors` — capture retail outputs as Rust assertions.
  **Capture, do not calculate.**
- `oracle sweep` — characterise every ISLAND by probing it.

`oracle difftest` and `oracle combat` are **superseded** by `regress`: they compare retail
against models copied into `main.rs` rather than against the shipped functions, so they can
only tell you the copy is right.

## Safety

Every case, and every ad-hoc probe, runs in a forked child, so a function that turns out to
need live globals faults the child and reports a signal instead of killing the harness.
Environment surgery — the fake `%fs` base for SEH prologues, the IAT patches the tokenizer
needs, writes into `.data` — is confined to the child that needed it, so it cannot silently
change what another case measured.

`data/riseofnations.exe` must be present (copyrighted; not committed — see
`docs/binary-ground-truth.md` for extraction). `data/rules.xml` is the tokenizer case's
shipped corpus; without it that case **skips** rather than running a shortened version,
because "exhaustive over the shipped corpus" and "some of the shipped corpus" are different
claims.
