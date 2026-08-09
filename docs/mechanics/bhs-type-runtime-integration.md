# BHS type builtins: Gen-7 runtime integration

This tranche makes the already-recovered canonical type owner reachable from the persistent BHS
runtime without inventing a second rules table. It is a source-only, mutation-sensitive pack;
no compiler, formatter, Cargo command, retail process, or remote harness was run in this lane.

## Ground truth and admitted registrations

The declaration gate is fixed to `schema/bhs-builtins.json`, extracted from the shipped
`riseofnations.exe` registration body. It admits the global registration index only when its
name, arity, scalar parameter types, integer return type, and native handler VA all agree:

| index | declaration | handler VA |
|---:|---|---:|
| 284 | `disable_type(string)` | `0x009EA130` |
| 286 | `enable_type(string)` | `0x009EA3E0` |
| 288 | `rename_type(string,string)` | `0x009EA6C0` |
| 289 | `type_build_time(string)` | `0x009EA760` |
| 290 | `set_type_build_time(string,int)` | `0x009EA7D0` |
| 291 | `set_type_job_time(string,int)` | `0x009EA880` |
| 815 | `disable_type_by_tribe(string,string)` | `0x00A006A0` |
| 816 | `enable_type_by_tribe(string,string)` | `0x00A009A0` |
| 817 | `enable_type_by_tribe(string,string,string,int,int)` | `0x00A00CA0` |
| 818 | `enable_type_by_tribe_with_type_name(string,string)` | `0x00A00D90` |
| 819 | `enable_type_by_tribe_with_type_name(string,string,string,int,int)` | `0x00A01090` |

Dispatch is index-first. That preserves the overloaded 816/817 and 818/819 registrations and
the latter pair's `type_name` first lookup without confusing their building tail, which still
uses the ordinary internal type `name`.

`ScriptRuntime::install_type_builtins` accepts one synchronized `TypeBuiltinState`. A second
install returns the rejected owner intact. During `Sim::do_frame_with_scripts`, the VM consults
that adapter before the general Scenario host and publishes the last exact receipt/fault.
Uninstalled state and owner faults remain strict `HostError::Unimplemented`; neither path
silently substitutes a retail error return.

## Mutation receipt

Every admitted owner call records:

- global registration and shipped handler VA;
- an owned typed request, retaining name-vs-`type_name` lookup and optional placement payload;
- native integer return or the typed `TypeTableError`;
- mutation revision before and after the body;
- the resulting dirty admission bit.

The revision advances exactly once at each owner mutation commit and is evidence only: it is not
retail data and is excluded from save/checksum state. The receipt deliberately does not infer
mutation from the return value. Registration 290/291 can successfully return signed `-1`.
Registration 817/819 first mutates `tribe_mask`, `modified`, and matching Leader masks; a missing,
non-Build, or typed-failing building tail can then return/fail while retaining that prefix. The
receipt survives this typed-error path and proves the revision advanced.

## Save and checksum ownership

`save_sim_with_scripts` is the combined persistence entry point for callers that pass an
external `ScriptRuntime` into the tick. It rejects every installed owner with
`SaveOwnerUnowned`, including revision-zero pristine state. DoNSave v6 stores neither the owner
nor synchronized rules/mod provenance, and `load_sim` returns only a `Sim`; accepting a pristine
external owner would therefore produce a save that cannot resume the same session. Dirty state
additionally includes mutable rows, unrestricted display Strings, and the Leader
`tech`/`obs_flags` payloads and flags.

`ScriptRuntime::admitted_sim_channel_digest` fails whenever a type owner is installed, even
while pristine. The existing partial Sim digest does not walk the complete live channel-13
`Types::walk_rules_data` projection, so returning a digest would omit the canonical owner.
Leader `tech`/`obs_flags` are not added to channel 8; retail does not walk those fields there.

Both gates reject an installed owner until their complete projections exist. They are opt-in
combined APIs, not global enforcement: the older public `save_sim(&Sim)` and
`Sim::channel_digest()` cannot observe a `ScriptRuntime` passed separately to the tick. Moving
the owner/admission token into `Sim` or an opaque combined session is a remaining architectural
gate; this tranche does not overclaim that bypass as closed.

## Mutation-sensitive proof pack

`crates/don-sim/tests/bhs_type_runtime_integration.rs` freezes four boundaries for the next
independent harness run:

1. a real VM call to #290 whose successful native return is `-1`, with revision 1 and exact
   canonical `job_time` state;
2. #817's non-ASCII building-tail fault after the base mutation, including its error receipt;
3. rejection of mismatched PE declaration identity and wrong direct argument shapes without a
   revision advance;
4. pristine and dirty external-owner save rejection, unconditional channel-13 rejection, and
   single-owner install semantics.

Static validation in this source-only lane is limited to `git diff --check`, descriptor comparison
against `schema/bhs-builtins.json`, and shared-dirt/path review. The test pack is deliberately
unexecuted in the authoring lane under the mutation-sensitive no-build order.

Root convergence subsequently passed independent remote gates on 2026-08-09:

- hbox `bhs-type-runtime-20260809T215200Z-36698-16564-fec095d1b4d7`: 4/4 focused tests;
- persvati `bhs-type-runtime-release-20260809T215200Z-36697-5286-fec095d1b4d7`: 4/4
  focused release tests;
- hbox `bhs-type-runtime-script-20260809T215305Z-37717-19077-fec095d1b4d7`: 47/47
  `script_tick` tests;
- persvati `bhs-type-runtime-check-20260809T215306Z-37714-21029-fec095d1b4d7`: all-target
  compile check.

All four jobs exited 0. These receipts validate routing and the conservative refusal boundaries;
they do not change the zero immediate executed-call delta described below.

## Honest coverage delta and remaining red gates

The source dispatch path for 11 registrations is now connected to `ScriptRuntime`, covering the
1,614 shipped calls counted by the existing corpus ledger **when** synchronized setup installs
the exact 806-row/24-tribe/eight-Leader owner. No production setup path constructs that owner yet,
so the immediate executed shipped-call delta is **0**, not 1,614.

Remaining blockers are therefore explicit:

1. compose the exact String/relation rows, tribes, immutable backups, and Leader-mask state from
   one synchronized rules/mod source before the first script frame;
2. make the same owner feed every non-script rule consumer;
3. place the owner/admission token in `Sim` or an opaque combined session so legacy save/digest
   APIs cannot bypass the external-runtime gates;
4. implement the complete channel-13 projection and bind it to the mutation revision;
5. serialize/restore live mutable rows and Leader masks in DoNSave, with exact rules/mod
   provenance for immutable backups;
6. resolve registration 289's spell `leaders[-1]` dependency and Windows-compatible non-ASCII
   lookup folding before those typed faults can be called handled.

Frozen source paths for this pack are:

- `crates/don-sim/src/systems/bhs_type_table.rs`
- `crates/don-sim/src/systems/bhs_type_runtime.rs`
- `crates/don-sim/src/systems/mod.rs`
- `crates/don-sim/src/script_runtime.rs`
- `crates/don-sim/src/systems/save_load.rs`
- `crates/don-sim/tests/bhs_type_runtime_integration.rs`
- `docs/mechanics/bhs-type-table.md`
- `docs/mechanics/bhs-type-runtime-integration.md`
