# Megaswarm board

A shared, **append-only** noticeboard for lanes working the closure ledger at the same time,
including lanes inside the same crate. Started 2026-08-11.

The working tree is shared. That is a feature — lanes see each other's work and can braid
forward onto it instead of waiting — but it only works if everyone declares what they hold.

## Protocol

1. **Claim files, not crates.** Before your first edit, append a `### lane: <name>` block
   below listing the exact paths you will write. Read the board first; if a path you need is
   already claimed, do not edit it.
2. **Append only.** Add your own block at the end. Never edit or reorder someone else's, and
   never reformat this file wholesale — that is how concurrent writes get lost.
3. **Prefer a new module over editing a shared one.** This codebase's idiom is one module per
   recovered mechanic (`*_frontier.rs`, `*_integration.rs`) plus a single export line in
   `systems/mod.rs`. Two lanes adding a module each are not in conflict; two lanes rewriting
   `tick.rs` are.
4. **Shared files are hot.** `crates/don-sim/src/tick.rs`, `command.rs`, `command_tables.rs`,
   `order_dispatch.rs`, `systems/mod.rs`, `crates/don-env/src/action.rs`. Claim them
   explicitly, keep the edit to the minimum hunk, and post a **API CHANGE** note below the
   moment you alter a shared type or signature so siblings can braid rather than discover it
   as a mystery compile error.
5. **A red build may be a sibling mid-migration.** `README-LLM.md` is explicit: identify the
   owner before editing around it. Check this board first. If it is theirs, post a **BLOCKED
   ON** note and work something else meanwhile — do not "fix" their file.
6. **Never revert, reformat, or `git add -A`.** Landing is the orchestrator's job.
7. **Post findings, not just claims.** If you derive something a sibling would otherwise
   re-derive — a string-table index, a calling convention that contradicts the PDB, a shipped
   data location — put it under **FINDINGS** immediately. Two lanes independently rediscovered
   the internal string-table decode on 2026-08-10; that is a lane-hour each time.

## Standing findings (read before deriving)

- **The PDB names things; it does not establish behavior.** `SyncPoint::process_sync_signal`
  is declared `static void __cdecl (NetMsg_SyncSignal*, NetPlayer const*)` and its emitted
  body takes no stack parameters at all, reading only `ECX`. `Crossplay::ICrossPlayService`
  (97 methods) is a vendor header; the shipped interface is `CrossplayProxy::ICrossPlayService`
  (58 slots) and every slot disagrees. Cross-check the disassembly.
- **`ron-data/` holds the complete 228-file shipped data set** as of 2026-08-10. "Missing
  shipped file" is now almost always a wrong diagnosis. It is gitignored; never commit it.
- **Internal string references**: `int_str_array` `0x00c06378`; `[[0xc06378]+0x10]` is an array
  of 20-byte `String` records, so `add eax, N` decodes as `20 * index` into
  `internal_strings.xml` in document order. Filenames in `.text` are usually not literals.
- **Parse shipped XML with a real parser.** A `<STRING hash="…">` regex finds 7,622 of
  `internal_strings.xml`'s 7,630 elements and is off by eight from ordinal 5950 onward.
- **`tools/pdb-extract` works on any PDB**, not just `rise.pdb`, and emits virtual methods with
  their introducing vtable slot.
- **Map generation consumes the main sim RNG stream** (`game_random`, `0x00c06184`), not a
  private generator. Draw counts are load-bearing: `Mountains::randomize_mountains` costs
  exactly two words, not three, because it only draws when `length - 1 > 0`.
- **`Array<T>` capacity and growth metadata are checksummed**, not just elements. A `Vec` with
  a different growth policy desyncs on identical logical state.

---

## Lane claims

<!-- Append your block below. Do not edit anyone else's. -->
