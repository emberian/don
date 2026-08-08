# Binary ground truth: riseofnations.exe

Everything here was derived **directly from the shipped binary or the shipped data
files**, not from community documentation. Each claim states how it was obtained so it
can be re-derived or falsified. Community formulas are used *only* as cross-checks and
are never the source of a value we implement.

Artifacts (hash-verified against the VM, see "Provenance"):
- `ron-bin/riseofnations.exe` — sha256 `30478a44…625079`, 9,925,120 bytes
- `ron-bin/patriots.exe` — sha256 `833fa691…eb8156`, 2,317,824 bytes
- `ron-data/*.xml|dtd|sps` — 45 files, all sha256-verified identical to the VM copies
- `ron-data/ai-scripts/*.bhs` — the three shipped AI scripts

## Provenance / extraction method

No Parallels shared folder was configured. Files were streamed out of the running VM:

```sh
# text
prlctl exec "Windows 11" cmd.exe /c 'type "C:\...\Rise of Nations\data\rules.xml"' > rules.xml
# binary (certutil base64; MUST use a fresh temp name per file — certutil does not
# reliably overwrite, which silently produced a duplicate file on the first attempt)
prlctl exec "Windows 11" cmd.exe /c 'certutil -encode "C:\...\riseofnations.exe" C:\Users\Public\enc.b64'
prlctl exec "Windows 11" cmd.exe /c 'type C:\Users\Public\enc.b64' | tr -d '\r' | grep -v CERTIFICATE | base64 -D > riseofnations.exe
```

Verification: `certutil -hashfile <f> SHA256` in the guest vs `shasum -a 256` on the
host. **Always hash-verify; the first binary copy was wrong and only the hash caught
it.** All 45 data files verified byte-identical.

## PE facts (via `pefile`, derived not quoted)

| property | value |
|---|---|
| machine | `0x14c` IMAGE_FILE_MACHINE_I386 — **32-bit x86**, PE32 |
| image base / entry | `0x400000` / `0x15d699` |
| linker | **14.0 (MSVC 2015/2017 toolchain)** |
| build timestamp | **2024-06-20 19:42:55 UTC** |
| ASLR / NX / LAA | on / on / **off** (not large-address-aware) |
| PDB path (CodeView) | `E:\agent\_work\2\s\main\game\rise.pdb` (Azure DevOps agent) |
| `.text` | 7,090,736 bytes, entropy 6.48 — **not packed** |
| exports | none |
| notable imports | `MSVCP140`, `VCRUNTIME140`, `api-ms-win-crt-*`, `steam_api`, `dbghelp` |

**The game is a 2024 recompile of the 2003 codebase, not a 2003 binary.** This matters:
a modern MSVC toolchain decompiles far better than a 2003 one, and it changes the
floating-point story below. The "exes were obfuscated after the first patch" claim from
2004 forums does not apply to this binary — it is a clean, unpacked, RTTI-rich build.

`patriots.exe` is **only the MFC launcher** (`launcher.pdb`, 2017-07-24, 2.3MB of which
2.28MB is `.rsrc`). It contains no simulation logic. All game logic is in
`riseofnations.exe`.

**CORRECTION [measured, 2026-08-08]: the game ships its own full PDB.** An earlier pass
here concluded that `sbl\` held PDBs only for SkyBox's support libraries
(`CrossplayNetLib.pdb`, `CrossplayProxy.pdb`, `d3dgl.pdb`, `dssl.pdb`, `PartyWin.pdb`,
`PlayFabMultiplayerWin.pdb`) and that `rise.pdb` was not shipped. That conclusion came from
a recon command that truncated its own directory listing; `sbl\rise.pdb` was there all
along. It is 57,290,752 bytes and it is the PDB for **this exact binary**:

| | |
|---|---|
| EXE CodeView `PdbFileName` | `E:\agent\_work\2\s\main\game\rise.pdb` |
| EXE CodeView GUID / age | `{51D4F219-61C6-4F84-9D5B-C3361B0D291F}` / 1 |
| `rise.pdb` GUID / age | `{51D4F219-61C6-4F84-9D5B-C3361B0D291F}` / 1 |

37,138 public symbols, 22,752 procedure records with undecorated names, code sizes and
full C++ signatures, plus complete type information. Extracted to
`schema/rise-symbols.tsv` (publics) and `schema/rise-procs.tsv` (procedures, with sizes —
this is the one that lets you resolve an arbitrary VA to its containing function).

**What this does and does not change.** It gives *names, types, sizes and line info*. It
does **not** give semantics, and it does not raise any fidelity tier: a symbol is a name,
not a behaviour, and values still come from the oracle. See
`docs/derivation/PDB-RECONCILIATION.md` for the claim-by-claim audit of everything this
project derived before the PDB was found.

## Floating point — the determinism crux

Full-coverage linear disassembly of `.text` with capstone (`skipdata=True`,
2,126,018 instructions decoded):

| class | count |
|---|---|
| **SSE scalar float** | **23,758** (`movss` 12,511, `mulss` 3,900, `addss` 2,430, `subss` 1,283, `cvttss2si` 1,049, `divss` 711, `comiss` 605) |
| **x87** | **560** (residue: `fld` 149, `fstp` 88, `fldcw` 15) |
| integer | `imul` 12,443, `idiv` 1,029, `sar` 5,541, `shl` 4,131 |

*(Caveat: a linear sweep over a 7MB section includes some data misdecoded as
instructions. The ratio is decisive at 42:1 but individual counts are approximate. A
precise count should come from the Ghidra function graph.)*

Conclusions:

1. **The sim is single-precision IEEE-754 via SSE, not x87.** MSVC 14 targeting x86
   defaults to `/arch:SSE2`. The x87 residue is almost certainly static-linked library
   code, not sim math.
2. **Bit-exactness is therefore plausible**, which it would *not* be for a genuine 2003
   x87 build. SSE scalar ops are exactly-rounded per IEEE-754 with no 80-bit
   intermediates and no FPU-control-word sensitivity. Rust `f32` arithmetic has
   identical semantics, and Rust does not enable fast-math reassociation by default.
   x86-32 SSE2 has no FMA, so there are no fused-op discrepancies either.
3. **The enumerated bit-exactness hazard is small and known.** The only non-IEEE-mandated
   math imported from the CRT is:
   `_libm_sse2_acos_precise`, `_libm_sse2_asin_precise`, `_libm_sse2_atan_precise`,
   `_libm_sse2_cos_precise`, `_libm_sse2_pow_precise`, `_libm_sse2_sin_precise`,
   `_libm_sse2_tan_precise` (plus `_libm_sse2_sqrt_precise`, which *is* IEEE-exact and
   therefore safe).
   These seven are where Rust's libm may differ in the last ulp. Open question for
   Ghidra: are any of them reachable from sim code, or only from rendering/camera? If
   sim-reachable, we either replicate them or accept bounded divergence there.

**Correction to an earlier working assumption:** the `rules.xml` header comment
("distances are fractions of a tile, largest denominator 192") describes the *authoring
and parsing* convention, not necessarily the runtime representation. The runtime is
float-heavy. Whether parsed rationals are converted to `f32` or to a fixed-point integer
is an open Ghidra question, not a settled fact.

## Rule-name string anchors — they exist, as UTF-16 lowercase

The XML tag names are **not** present as ASCII, which briefly looked like the loader was
positional/index-based. That was wrong. They are present as **UTF-16LE, lowercased**:

```
'flank_bonus'  'cavalry_flank_bonus'  'vehicle_flank_bonus'  'accel_train'
'progression'  'unit_rate_progression'  'recharge'  'air_unit_mana_recharge'
'siege_attrition'  'militia_attrition'  'colosseum_attrition'
'city_plunder_per_level'  'capital_plunder'  'caravan_plunder'
```

8,363 UTF-16 strings ≥6 chars total. So **the string-anchor RE strategy works**: xref
each lowercase wide string → the loader that consumes it → the struct field it writes →
every reader of that field. Search UTF-16LE lowercase, not ASCII uppercase.

Note some names carry a `[scan]` suffix (`attrition_upgrade[scan]`,
`attrition_improved[scan]`) — an unexplained convention worth resolving.

## C++ class architecture (RTTI)

620 RTTI class descriptors (`.?AV…@@`) are present, so Ghidra can recover class names
and vtable layouts automatically. Recovered names reveal a consistent triple pattern —
`X` / `XData` / `XOut` — across `Unit`, `Build`, `City`, `Terrain`, `TechType`,
`PathFinder`, `Ammo`, `Animal`, `BonusType`, `Caster`, `MountainRange`, `UnitType`,
`BuildType`. Working hypothesis: `XData` is the POD state struct and `XOut` a
serialization/output helper. **`XData` layouts are, if so, exactly the sim state we need
to mirror** — and they are recoverable rather than guessable.

The order hierarchy is **the game's own action space, enumerated by the engine**:

```
MoveOrder  GroupMoveOrder  AttackOrder  AttackToOrder  AttackGroundOrder
GroupAttackOrder  GroupAttackToOrder  AirAttackGroundOrder  GatherOrder
GuardOrder  PatrolOrder  GroupPatrolOrder  AirPatrolOrder  FollowOrder
FormOrder  BoardOrder  AwaitBoardOrder  GarrisonOrder  RepairOrder  CastOrder
StrafeOrder  ExploreToOrder  FleeToOrder  SpecialAnimOrder  AirOrder
GroupOrder  OrderList
```

This is a far better basis for an RL action space than anything inferred from the UI.

Other load-bearing classes: **`BorderSpline`** (borders are spline-based — which explains
why no closed-form radius formula was ever published), **`SyncDisplay`** (lockstep
desync tooling; likely reachable from a state-checksum routine that would tell us
exactly which fields are sim-critical — a high-value early target), `GameLog`,
`ReplayWin`, `PathFinder`, `Tribes`, `Map*` (per-map-type classes).

## Shipped AI scripts

`ai\scripts\` contains three BHS scripts totalling 2,400 lines — `economic.bhs`,
`defensive.bhs`, `aibestbuildlibrary.bhs` — authored by Mark Sobota and Mike Engle. They
are plain source, and they exercise the real script API (`num_cities`,
`num_type_with_queued`, `place_building_with_cost`, `find_city_with_num`,
`was_city_attacked`, `have_tech`, `get_territory_owner`). Two uses:
1. Ground truth on the BHS API surface for building a scripted differential oracle.
2. Scripted opponents to behavior-clone as an RL bootstrap.

## The descriptor/visitor framework (how rules bind to fields)

Ghidra project `re/ghidra` (project `ron`): **47,177 functions, 14,441 defined strings**.
Rule-name anchors resolve as `unicode` data with clean xrefs — the methodology works:

| anchor | xref'd from | **real symbol [measured, rise.pdb]** |
|---|---|---|
| `flank_bonus`, `cavalry_flank_bonus`, `vehicle_flank_bonus`, `siege_attrition`, `accel_train` | `FUN_00570170` | `Constants::log_data(Log*) const` |
| `progression` | `FUN_0061c490` | `UnitType::log_data` |
| `recharge` | `FUN_0065fc00` | `ObjectType::log_data` |

⚠ **CORRECTION [measured, 2026-08-08, rise.pdb]. These three functions are LOGGERS, not
loaders.** This section originally called `FUN_00570170` "the rules.xml constants loader".
It is `Constants::log_data`. The real loader is **`Constants::init` at `0x00569A90`**
(26,336 bytes, ending exactly where `log_data` begins at `0x00570170`); likewise
`UnitType::init` at `0x0061AB50`. The mistake was structural, not careless: the rule-name
UTF-16 literals live in `.rdata` and are referenced **only** from the `log_data` functions
(one xref for `flank_bonus`, in `Constants::log_data+2764`). `Constants::init` never touches
them — it fetches each name from the runtime `StringTable` at `[0x00C06378]`
(`int_str_array`) by fixed offset. Following the name strings therefore leads to the logger
every time. Full detail and consequences in `docs/derivation/PDB-RECONCILIATION.md` §2.

The good news is that the *binding* this section extracts survives, because `log_data`
reads each field at its true offset in order to print it. See below.

Decompiling `FUN_0061c490` (456 lines) and `FUN_0065fc00` (698 lines) reveals a uniform
**descriptor + visitor** pattern. Each named field is bound by building a small
stack descriptor and making one virtual call:

```c
local_88 = L"recharge";      // rule name, wide string
local_84 = 8;                // type tag
local_82 = 0x80000;          // flags/precision
(**(code **)(*param_1 + 0x1c))
    (&local_88, &DAT_00eb437c, *(undefined4 *)(in_ECX + 500), 0,0,0,0,0,0,1);
//                             ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ field at this+0x1F4
```

`param_1` is a visitor object; `vtable+0x1c` is its "bind named field" method.

**What the visitor actually is [measured, rise.pdb]:** `param_1` is a `Log*`, and
`vtable+0x1c` is a `Log` method that records one named value. This is the signature
`void ObjectType::log_data(Log*) const`. It is *not* the save/checksum interface — that is
a separate method family, `walk_data(DataWalk*)` / `walk_rules_data(DataWalk*)`, on the
same classes (`Constants::walk_data` `0x0057F910`, `Balance::walk_rules_data` `0x00582CC0`,
`Game::walk_data` `0x00589600`, `LeaderData::walk_data` `0x006D6750`, …). Two visitors, two
interfaces.

**CORRECTION [measured, 2026-08-08]: the field we called a "type tag" is the LENGTH of the
wide rule name, not a type tag.** `recharge`=8, `crew_size`=9, `attack`=6, `hits`=4,
`armor`=5, `splash_percent`=14, `special_upgrade_cost`=20 — every one of the 34 bindings in
`FUN_0065fc00` equals `strlen(name)`. It is stored twice (u16 at +4 and in the high half of
the dword at +6): the classic `{const wchar_t*; size_t}` string-view shape. This is why the
hypothesis that it encoded the value's unit/parser was refuted — it never encoded a type at
all. Array-valued fields are bound in a loop (e.g. base `in_ECX + 0x314`, count 352).

**This is the schema, mechanically extractable**: every (rule name, type tag, struct
offset) triple in the game is recoverable by walking these call sites. `FUN_0065fc00` is
the combat-stats loader — its 35 names are `obj_masks`, `attack`, `to_hit`, `attenuate`,
`recharge`, `min_range`, `max_range`, `splash_area`, `splash_percent`, `ammo_per_att`,
`proj_speed`, `hits`, `armor`, `domain`, `science_los`, `guy_spacing`, `x_spacing`,
`y_spacing`, `abil`, `x_size`, `y_size`, `guy_radius`, `block_radius`, `big_radius`,
`new_block_radius`, … `FUN_0061c490` binds 23 names.

**Inference REFUTED as stated [measured, rise.pdb]:** this section guessed that "the same
descriptor tables are plausibly reused for save-game serialization and possibly the
lockstep checksum". They are not the same tables. `log_data(Log*)` and
`walk_data(DataWalk*)` are distinct virtual interfaces implemented separately on each
class. The *conclusion* that a single traversal defines the sim-state schema still holds —
but it is the `DataWalk` family that does it, and the enumeration must be of `walk_data` /
`walk_rules_data`, not of this pattern.

**Also resolved [measured, rise.pdb]:** the third argument decompiles as
`*(undefined4 *)(this + N)` — a *load*, not an address — because a logger passes the
field's **value**, not a pointer to it. There is no hidden indirection; `this+N` is the
storage. (Verified in the disassembly of `Constants::log_data+2764`, which loads
`[edi+0x4C]` and pushes it alongside `L"flank_bonus"`.)

**`FUN_00570170` exceeds the decompiler's 600 s budget** — expected for a 63,382-byte
function. Do not fight the decompiler here; extract at the instruction level instead. Two
extractors now exist and they answer different questions:

- *name → struct offset*, from `Constants::log_data` and its siblings: locate each
  `L"name"` store and read the offset operand pushed alongside it. This is where
  `docs/derivation/rules-constants.json` and `schema/bindings.json` came from, and the PDB
  vindicates the result even though the function was misidentified.
- *offset → parser + scale*, from `Constants::init` `0x00569A90`: every rules constant is
  loaded by one of exactly two calls — `Constants::get_item(const String&)` `0x0057FA60`
  (plain `_wtoi`, 661 sites) or `Constants::get_fraction(const String&, int scale)`
  `0x0057F950` (40 sites), with the scale as a `push imm32` immediately before the call.
  The complete scale universe is **{256 ×24, 192 ×11, 100 ×5}**.

## Open questions for Ghidra (the worklist)

1. Does sim code reach any of the seven non-IEEE CRT transcendentals?
2. Are parsed rule values stored as `f32` or fixed-point integers?
3. `XData` struct layouts for `Unit`, `Build`, `City`, `TechType`.
4. The damage pipeline: exact operation order, rounding, and where the hardcoded
   per-mask modifier table lives.
5. The lockstep state-checksum routine (via `SyncDisplay`) → the definition of
   sim-critical state.
6. `BorderSpline` geometry.
7. `PathFinder` algorithm.
8. RNG algorithm and its call sites.
9. The `[scan]` suffix convention on rule names.
