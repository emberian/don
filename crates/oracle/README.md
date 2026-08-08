# oracle

Maps `riseofnations.exe` into a live process and calls retail functions directly, so our
Rust reimplementation can be differentially tested against the actual shipped code.

**Builds only for 32-bit x86.** It executes the retail image in-process, so the host
process must be i686. It is deliberately excluded from the workspace: an arm64 Mac cannot
build or run it (Rosetta is x86-64 only).

Build and run on an x86_64 Linux host:

```sh
rustup target add i686-unknown-linux-musl     # musl ships self-contained CRT objects,
                                              # so no 32-bit system dev packages needed
cargo build --target i686-unknown-linux-musl
./target/i686-unknown-linux-musl/debug/oracle selftest
./target/i686-unknown-linux-musl/debug/oracle difftest 200000
```

`data/riseofnations.exe` must be present (copyrighted; not committed — see
`docs/binary-ground-truth.md` for extraction).

## Commands

- `selftest` — executes hand-written machine code, so a failure is unambiguously the
  harness rather than the retail image.
- `info` — maps the image and prints section geometry.
- `call <va|rva> [args…]` — calls a function cdecl-style, in a forked child.
- `difftest [N]` — runs the registered cases against Rust models over N inputs.

## Safety

Every call runs in a forked child, so probing a function that turns out to need live
globals faults the child and reports a signal instead of killing the harness. Targets
come from `schema/islands.jsonl` (see `re/scripts/FindIslands.java`): only `ISLAND` and
`DATA_ONLY` functions are callable with fabricated inputs.
