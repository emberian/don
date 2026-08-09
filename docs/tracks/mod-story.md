# mod-story — how content and mods load

Lane: `mod-story`. Crate: `crates/don-content` (new, in the workspace).
Everything marked **[measured]** was read out of `ron-bin/riseofnations.exe` at a VA resolved
through `ron-bin/sbl/rise.pdb`, or captured from the live retail install in the Parallels
guest. Nothing here comes from community documentation. Nothing here is *verified*.

---

## What a human can now do that they could not before

1. **Point a tool at a folder of Rise of Nations mods and inspect the recovered loader
   model** — which files each mod claims, which categories, which Steam Workshop tags the
   recovered table assigns, and, per file, whether Descent of Nations can consume it. The
   strict command rejects unresolved runtime behavior instead of presenting a synthetic scan
   as live-retail certification:

   ```sh
   cargo run -p don-content --bin don-content -- scan  '/path/to/My Games/Rise of Nations/mods'
   cargo run -p don-content --bin don-content -- check '/path/to/My Games/Rise of Nations/mods'
   cargo run -p don-content --bin don-content -- probe '/path/to/mods' data/rules.xml mapstyles/greatlakes.xml
   cargo run -p don-content --bin don-content -- rules
   ```

   `probe` answers the question that actually matters when a mod misbehaves — *which file
   won?* — with retail's own precedence rule, not a guess.

2. **Know, precisely and citably, what a mod is allowed to replace.** The full 12-category
   table, the per-category recursion flags, the extension whitelist per category, and the
   21-name veto list are now data in the repo, generated from the binary by
   `crates/don-content/gen/gen_tables.py` rather than typed in.

3. **Change one rule constant without shipping a whole `rules.xml`.** Retail cannot express
   that; `don_content::overlay` can, with the field name checked against `don-rules`' 717
   binder sites, the array arity checked, the value produced by the derived tokenizer rather
   than by hand, and a per-write audit trail. A fidelity-mode stack refuses every deviating
   layer by construction and can assert byte-identity to the shipped block.

4. **Answer Ember's compatibility question without pretending that path resolution is
   execution.** For any mod, `don-content` reports `N of M files consumed`, names the missing
   subsystem for every inert file, and has a strict `check` command that exits unsuccessfully
   unless every declared file has an end-to-end consumer. Today even `data/rules.xml` is
   rejected: the shipped `Rules` block is modeled, but an external XML-to-`Rules` loader is
   not wired yet.

5. **Stop trusting one thing that was never true.** Cross-checking the binary's protection
   list against the shipped `mapstyles\` directory found two shipped defects (below). Both are
   pinned by a test so they cannot be silently "fixed" into invisibility.

---

## 1. How retail actually loads mods

### 1.1 It resolves paths. It does not merge.

Every content open funnels through `String::prepend_content_dir` `0x00A1D690` (60 call sites
across 36 functions), whose only callee of interest is `ModManager::prepend_content_dir`
`0x00A22800` — which in turn has exactly one caller. The generic openers are
all among the callers — `File::open`, `XML::init` `0x00A279E0`, `Text::open`, `Filemap::open`,
`Lexer::open_file`, `Compiler::compile` — alongside 54 specific loaders [measured, via
`tools/pdb/callers.py`].

`ModManager::prepend_content_dir` is two steps and nothing else:

1. `ModManager::calcFileNameAndCategoryFromPath` `0x00A21E40` — normalise separators
   (`SkyString::ReplaceAll` `0x00A20CE0`), strip a leading `.\`, then scan categories `0..=11`
   for the first whose `relativeDirectory` is a prefix. Returns `(category, filename-within)`.
2. `ModManager::calcFilePath` `0x00A22910` — walk the mod list and return the first installed
   path that owns that filename in that category.

**The consequence is the single most important fact about RoN modding: a mod replaces whole
files, never fields.** A data mod that ships `data\rules.xml` supplies *all* of the rules; the
shipped file is not consulted. There is no retail mechanism for "change one constant". This is
why two single-constant mods are mutually exclusive, and why every Workshop rebalance silently
reverts later patches.

`Constants::init` `0x00569A90` calls `XML::init` once, and `XML::init` calls
`String::prepend_content_dir` [measured, call edges]. So the rules loader *is* mod-aware, as
are `Types::init` (9 XML loads), `Balance::init`, `Game::init_rules_data` and `Tribes::init`.

### 1.2 The twelve categories

`s_ModCategoryInfo` `0x00C07AD0`, 12 × `sizeof(ModCategoryInfo)` = 120 [measured]. The enum is
PDB `LF_ENUM 0x58AB`. `recursive` is what `ModPackage::buildCategory` passes as
`FindFile::RecurseSubDirs`.

| # | `ModCategoryType` | name | relative dir | recursive |
|--:|---|---|---|---|
| 0 | `CAT_AI` | `AI` | `ai\scripts\` | no |
| 1 | `CAT_ART` | `ART` | `art\` | **yes** |
| 2 | `CAT_CONQUEST` | `CONQUEST` | `conquest\` | **yes** |
| 3 | `CAT_DATA` | `DATA` | `data\` | no |
| 4 | `CAT_MAPSTYLES` | `MAPSTYLES` | `mapstyles\` | no |
| 5 | `CAT_SCENARIO` | `SCENARIO` | `scenario\` | **yes** |
| 6 | `CAT_SOUNDS` | `SOUNDS` | `sounds\` | **yes** |
| 7 | `CAT_TERRAIN` | `TERRAIN` | `terrain art\` | **yes** |
| 8 | `CAT_TRIBES` | `TRIBES` | `tribes\` | no |
| 9 | `CAT_REPLAYS` | `REPLAYS` | `replays\` | no |
| 10 | `CAT_SAVES` | `SAVES` | `saves\` | no |
| 11 | `CAT_ROOT` | `ROOT` | *(empty)* | no |

Three things fall out and all three bite a naive reimplementation:

* **`CAT_ROOT`'s prefix is empty**, so it matches every path, and it is last in the scan. That
  is why classification never fails: an unrecognised path is a root-level file, not an error.
* **`ai\scripts\` is two segments.** A "first path component" classifier gets `ai\other.bhs`
  wrong — it is `CAT_ROOT`, not `CAT_AI`.
* **No category prefix is a prefix of a later one**, so first-hit scanning is unambiguous. That
  is a property of the shipped table and could have been false; `tests/shipped_layout.rs`
  asserts it.

These twelve directories are exactly what the retail install has at its root
(`ai art conquest Data mapstyles scenario sounds "terrain art" tribes` plus a `mods` folder)
[measured, guest listing 2026-08-08].

### 1.3 Discovery

`ModManager::buildModPackages` `0x00A221F0` calls
`TFileSystem::FindAllMatchingFiles(L"*", StoragePoint::MYMODS (7), results, nullptr,
FindFile::MatchDirectories (2))` — i.e. **every subdirectory of the MyMods storage point is a
candidate mod** [measured]. On this install MyMods resolves to
`C:\Users\ember\Documents\My Games\Rise of Nations\` [measured — the guest has
`Recorded Games`, `Saves`, `Scenarios`, `StandaloneScripts` there and no `mods` folder yet].

Each candidate gets a 360-byte (`0x168`) `ModPackage` and
`ModPackage::buildPackage` `0x00A358B0`, which calls `ModPackage::buildCategory` `0x00A34DB0`
for all twelve categories and then **drops the package if it found zero files**. `buildCategory`
searches `<installDir>\<relativeDir>*` with
`MatchFiles | MatchAnyAttributes | SkipForbiddenFiles | ReturnPathSpec` (`0x10029`) plus
`RecurseSubDirs` when the category says so, stores each name relative to the category
directory, and lowercases it (`SkyString::ConvertToLowerCase` `0x00A1F540`).

Workshop items arrive by a different door — `ModSteamWorkshop::SubscribeModNow` `0x005672C0`,
`OnRequestUGCDetails` `0x00566530`, then `ModManager::addModPackage` `0x00A23E40` with the
UGC install path and a non-`MYMODS` location — but **after that they are ordinary packages**.
Subscribing needs Steam; loading does not. That matters for us: an on-disk Workshop directory
can be scanned directly.

`ModPackage` field offsets, derived from code (the PDB emits only its ctor/dtor):

| offset | field | derived from |
|---|---|---|
| `+0x000` | `u64` owner SteamID | `readModStatus` `0x00A244E0` |
| `+0x008` | `u64` `PublishedFileId` | `addModPackage` dedup key |
| `+0x010` | `SkyStringList` for `CAT_ROOT` | `getCategoryFileList` `0x00A35B40` |
| `+0x024 … +0x0FC` | 11 more `SkyStringList`, stride `0x14` | same |
| `+0x114` | `SkyString` display name | `addModPackage` arg 4 |
| `+0x124` | `SkyString` install directory | `calcFilePath` reads it |
| `+0x138` | `u64` total bytes | `buildCategory` |
| `+0x140` | `StoragePoint::Location` | `calcFilePath` compares to 7 |
| `+0x144` | `int` priority | `sortViaPriority` `0x00A22CC0` |
| `+0x148` | `int` publishable-asset count | `buildCategory` |
| `+0x14C` | `int` total file count | `buildPackage` gate |
| `+0x150` | `bool` enabled | `calcFilePath` gate 1 |
| `+0x151` | `bool` dropdown active | `calcFilePath` gate 2 |
| `+0x160`, `+0x164` | `int` timestamps | `readModStatus` |

### 1.4 The precedence rule, stated exactly

`ModManager::calcFilePath` `0x00A22910` [measured, disassembled — Ghidra swaps the two exit
blocks, so the decompiled C reads backwards]:

```text
for mod in mods:                                     # std::list order == priority order
    if not mod.enabled:                              continue   # +0x150
    if mod.is_dropdown and not mod.dropdown_active:  continue   # +0x151
    if mod.files[category] is empty:                 continue
    if filename not in mod.files[category]:          continue   # SkyStringList::FindI, case-insensitive
    if category == CAT_MAPSTYLES and isMapForbidden(filename): continue
    *out_index = 1 + position_of(mod)                           # 0 means "shipped"
    if mod.location == MYMODS:  return "mods\" + mod.installDir + "\" + relDir(category) + filename
    else:                       return               mod.installDir + "\" + relDir(category) + filename
*out_index = 0
return relDir(category) + filename
```

Order comes from `ModManager::sortViaPriority` `0x00A22CC0`: a `std::list::sort` with the
comparator at `0x00A21E20` (a bare `a->priority < b->priority` on the `int` at `+0x144`),
followed by a renumber to `1..N` in list order. So **lower priority number wins**, ties are
impossible after a sort, and the sort is stable. Retail sorts **once**, at the end of
`readModStatus` — not on every insertion; `addModPackage` just appends and takes the new list
length as its priority, so discovery order is the default order.

Two details a reimplementation gets wrong by default:

* **First match wins and the scan stops.** A second mod owning the same file never sees it.
  There is no "later mod patches earlier mod" anywhere in this engine.
* **The mod must have *declared* the file.** `mod.files[category]` is `buildCategory`'s
  snapshot, not a live directory probe. Dropping a file into a mod folder while the game runs
  changes nothing.

### 1.5 What a mod is allowed to contain

`s_SteamWorkshopTagLinks` `0x00C068D0`, 64 × 72 bytes [measured]. Nominally this drives Workshop
tagging (`ModPackage::calculateSteamTags` `0x00A35300`), but it is also the closest thing
retail has to a *manifest schema* — it is BHG's own statement of what file types belong in
what category.

| tag | category | patterns |
|---|---|---|
| `TagAi` | `CAT_AI` | `.bho .bhs` |
| `TagArt` | `CAT_ART` | `.bh3 .bha .mot .tga .wmv` |
| `TagCursors` | `CAT_ART` | `.cur` |
| `TagConquest` | `CAT_CONQUEST` | `.png .bhs .xml .4 .7 .9 .10 .12 .16 .17 .18` |
| `TagData` | `CAT_ROOT` | `.txt .wmv .bhs .xml` |
| `TagData` | `CAT_DATA` | `.4 .7 .9 .10 .12 .16 .17 .18 .bhs .dtd .sps .xml .xsd` |
| `TagMods` | `CAT_ROOT` | `info.xml` |
| `TagMapStyles` | `CAT_MAPSTYLES` | `.xml` |
| `TagReplays` | `CAT_REPLAYS` | `.rcx` |
| `TagScenarios` | `CAT_SCENARIO` | `.4 .7 .9 .10 .12 .16 .17 .18 .bhs .sce .scx .wav .xml` |
| `TagSounds` | `CAT_SOUNDS` | `.wav` |
| `TagTerrain` | `CAT_TERRAIN` | `.bh3 .png .tga` |
| `TagTribe` | `CAT_TRIBES` | `.4 .7 .9 .10 .12 .16 .17 .18` |

The numeric extensions are the localised-text files. A pattern of `*` matches anything, a
leading `.` is an extension suffix test, otherwise it is an exact filename; a leading `!`
negates the row (`0x21` is tested at `0x00A35300`) but **no shipped row uses it**. Tag names
come from `s_SteamWorkshopTagNames` `0x00C06678` (13 × 44): `AI Art Conquest Cursors Data Mods
MapStyles Replays Scenarios Sound Terrain Tribes Other`.

> **Correction to `CODEX.md`.** The "balance table at `0x00C06AFC`" entry says that address is
> "Steam Workshop strings". Refined: `s_SteamWorkshopTagLinks` runs
> `0x00C068D0 … 0x00C07AD0` (4,608 bytes), and `0x00C06AFC` is **0x22C bytes into it**. The
> symbol immediately after is `s_ModCategoryInfo` `0x00C07AD0`. Both are file-static
> (`S_LDATA32`), which is why neither shows up in `schema/rise-symbols.tsv`.

### 1.6 The only veto — and two shipped defects

`calcFilePath` consults `ModManager::isMapForbidden` `0x00A21140` for `CAT_MAPSTYLES` and for
**no other category**. That function holds exactly 21 UTF-16 literals [measured, full
immediate scan of all 3,283 bytes, not just `push` operands].

The retail install's `mapstyles\` directory holds **22** files [measured, guest listing
2026-08-08]. The lists do not agree, and the disagreement is the engine's, not the capture's:

* **`GREATLAKES.xml` ships and is not protected.** `greatlakes_trial.xml` is in the list;
  `greatlakes.xml` is not.
* **`SOUTHWESTMESA.xml` ships; the protection list spells it `soutwestmesa.xml`** — missing the
  `h`. The comparison never matches, so that entry protects a file that does not exist and the
  real file is overridable.

Both are pinned by `tests/shipped_layout.rs::the_forbidden_map_list_does_not_cover_the_shipped_map_styles`,
which also drives them through the resolver and asserts those two — and only those two — resolve
into a mod. **These are candidates for the improved-mode list** owned by the fidelity/improved
sibling lane: fix the typo and add `greatlakes.xml`, or (better, since the veto exists to stop
multiplayer desyncs on shipped maps) replace the hard-coded list with "any map style the base
install ships".

### 1.7 `mod-status.txt`

`ModManager::readModStatus` `0x00A244E0` opens `mod-status.txt` at `MYMODS` with
`Read | ShareRead`; `writeModStatus` `0x00A240F0` writes it with
`CreateAsNeeded | OverwriteExisting`. A latch (`0x00ED68E4/E5`, `disableModStatus`
`0x00A24890`) makes the writer refuse to run if the reader never did, so a failed load cannot
truncate the user's mod order.

Fixed-width, two `.rdata` format strings. Header arg order taken from the push order in
`writeModStatus` [measured, disassembled]; the row's field order is the reader's parse order,
whose types match the row format specifier-for-specifier.

```
header  0x00B149D0  "%-10s%-50s%-10s%-10s%-10s%-12s%-12s%-24s%-24s"
row     0x00B14950  "%-10d%-49s %-10d%-10s%-10s%-12d%-12d%-24llu%-24llu"
```

| # | column | type | notes |
|--:|---|---|---|
| 1 | `ID` | `int` | list index |
| 2 | `MOD NAME` | quoted string | `"%s"` `0x00B149B8`, so names with spaces survive |
| 3 | `PRIORITY` | `int` | lower wins |
| 4 | `ENABLED` | `Yes`/`No` | |
| 5 | `LOCAL` | `Yes`/`No` | `Yes` = `MYMODS`, `No` = Workshop |
| 6 | `TIMESTAMP` | `int` | |
| 7 | `TIMESTAMP2` | `int` | |
| 8 | `AUTHOR` | `u64` | SteamID64 |
| 9 | `WORKSHOPID` | `u64` | `PublishedFileId` |

The reader validates every field and **skips the whole line** on any failure, then matches rows
to already-discovered packages by `(name, local)`. A row naming an uninstalled mod is dropped;
an installed mod with no row keeps its scan-order defaults. So **`mod-status.txt` is advisory
state, not a manifest — it can never introduce a mod.**

### 1.8 Dropdown mods, and the runtime reload

`ModPackage::isDropdownMod` `0x00A35C20` is literally *"does the root file list contain
`info.xml`"*. A dropdown mod is one the player picks per match from a combo box; a data mod is
one that is simply on or off. Only dropdown mods consult the `+0x151` "active" flag.

`GameMod::init(ModPackage*)` `0x005A9AD0` reads `info.xml` through `XML::init` for its
metadata, and `GameMod::load` `0x005A9A60` installs the package pointer into the game-settings
object at `[0x00C061EC] + 0x4B8`. Switching a dropdown mod tears down and rebuilds the whole
ruleset at runtime: `GameMod::closeExistingData` `0x005AA170` frees the type arrays and
`GameMod::initExistingData` `0x005AA3E0` re-runs the XML loads and the interface, guarded by
`GameMod::changing_assets` (`0x00CAB394`). That is a hot-swap path we do not need to reproduce
— we can restart the world — but it is the reason the engine's rule state is torn down as a
unit and not patched in place.

### 1.9 Multiplayer: the engine already treats mods as sim-critical

`GameMod::compute_checksum` `0x005A94A0` sums a per-file checksum over
`GameMod::generate_file_list` `0x005A9030`. `GameMod::sync` `0x005A9650` reads the chosen
mod's `PublishedFileId` out of the game settings and, if the local machine does not have it,
calls `ModSteamWorkshop::SubscribeModNow` `0x005672C0` — i.e. **the joiner is made to download
the mod rather than desync**. The wire types are `NetMsg_GameModSyncRequest` /
`NetMsg_GameModSyncResponse` (`GameMod::process_request` `0x005A9640`,
`process_response` `0x005A9630`).

This is the precedent for our own handshake: a mod set has an identity and it belongs in the
lockstep negotiation, not in a config file each peer reads independently.

---

## 2. Our content model

`crates/don-content` implements retail's layer as-is, then adds one retail cannot express.

### 2.1 The layer stack

Five layers, lowest to highest. **Higher wins.** Within layer 2, retail's own rule applies
verbatim (lowest `priority`, first match, scan stops).

| # | layer | source | fidelity mode |
|--:|---|---|---|
| 0 | `Shipped` | `don_rules::SHIPPED` — `ron-data/rules.xml` as the engine stores it | required |
| 1 | `Edition` | Descent of Nations' own corrections; the improved-mode lane owns the contents | **must be empty** |
| 2 | `Content` | retail-compatible file replacement via `ContentStack` | allowed |
| 3 | `Overlay` | named field-level patches (this crate) | **must be empty** |
| 4 | `Session` | per-match overrides from a scenario or BHS script | **must be empty** |

`Content` is allowed in fidelity mode because a retail client running the same mods holds the
same bytes. What it does mean is that a replay captured without a mod cannot be validated
against a stack with one, because `Game::walk_rules_data` `0x00589550` (checksum channel 13)
walks the loaded values. `RuleStack::content_digest` is the hook for carrying that identity
into a handshake, mirroring `GameMod::compute_checksum`.

### 2.2 Validation a mod actually gets

Retail's failure mode is silence: an unknown `<CONSTANTS>` element is never queried by
`Constants::init`, because the loader has 717 hard-coded binder sites that *pull* names out of
the parsed XML. A typo therefore produces a mod that loads, runs, and does nothing. We refuse
instead. An overlay patch is checked for:

* **unknown field** — against `don_rules::FIELDS`' 717 names;
* **array index out of range** — against each field's real arity (31 of the fields are arrays);
* **unclassifiable parser** — two fields (`unit_block_radius`, `americans_marine_entrench`) have
  a loader site the extractor could not classify, so setting them from *text* would require
  guessing a scale, which is folklore; setting them numerically is allowed;
* **fidelity violation** — any non-empty deviating layer under `Mode::Fidelity`.

All errors are reported in one pass, not first-error-wins. `Patch::from_text` runs the modder's
literal text through the field's own parser (`_wtoi` or `String::fraction` `0x00A1D110` at the
field's compile-time scale of 100/192/256), so the stored integer is produced by the derived
tokenizer and never hand-computed. `compose()` returns the block **plus one `Attribution` per
write that actually changed something** — layer, field, before, after, and the modder's reason
— so no deviation is anonymous. `is_byte_identical_to_base()` is the honest fidelity assertion:
it compares bytes rather than trusting the mode flag.

### 2.3 What layering does *not* solve

Layer 2 is still whole-file. If a mod ships `data\rules.xml` we take its file; we cannot merge
two such mods, and neither can retail. The overlay layer is the answer *for constants*, and it
is only as good as `don-rules`' coverage — which is the 717-name `Constants::init` block, not
`unitrules.xml`, `techrules.xml` or the 493×493 balance matrix. Extending the overlay to those
means the same treatment (a name→offset binding table) for `UnitType::init` `0x0061AB50` and
friends. That is the obvious next lane and it is not done.

---

## 3. Can a retail mod load into our engine unmodified?

**Core path resolution: reproduced. End-to-end mod loading: not certified.** The resolver rule
is small and is reproduced from the engine's tables and disassembly. Discovery still depends
on filesystem enumeration and on `SkipForbiddenFiles`, whose internal filter has not been
traced, and no real Workshop package has yet been exercised. Nothing about resolving an
already-present directory needs Steam; subscribing and certifying a real package are separate.

**Content consumption: per file, not per mod.** That is the honest framing, and `don-content`
reports it as a number. Current classification:

| what the mod ships | verdict | why |
|---|---|---|
| `data\rules.xml` | resolved-only, **rejected** | `don-rules` models the shipped Constants block, but no external XML-to-`Rules` loader is wired |
| other `data\*.xml` | resolved-only, **rejected** | unit/type/tech/building XML binders and consumers are not implemented |
| `replays\*.rcx` | **consumed** | `don-replay` decodes `.rcx` |
| `data\*.bhs`, `ai\scripts\*.bhs` | **parsed** | `don-bhs` / `don-bhs-cc` front end exists; `RunTimeEnv::run_script` `0x0043D0E0` is not driven by our tick |
| `mapstyles\*.xml` | resolved-only, **rejected** | neither the XML parser nor map-generation consumer is wired |
| `info.xml` | resolved-only, **rejected** | dropdown status is detected by filename; the metadata XML is not parsed |
| `tribes\*.{4,7,9,…}` | resolved-only | needs the `StringTable` at `[0x00C06378]` |
| `scenario\*` | resolved-only | `ScenarioData::walk_data` `0x00997AD0` has no runtime producer |
| `*.txt` at root, `.dtd`, `.sps`, `.xsd`, `.bho` | resolved-only | unread, schema-only, or a precompiled form we compile from source instead |
| `art\*`, `terrain art\*`, `sounds\*` | out of scope | no renderer, no audio |
| `conquest\*` | out of scope | CtW campaign layer, out of scope in `COVERAGE.md` |

**Two things that could have blocked compatibility outright, and neither does:**

* *No encryption or packing.* `buildCategory` enumerates plain files and every consumer opens
  them through `prepend_content_dir`. A mod is a directory tree.
* *No manifest format to reverse.* Data mods have none — the file list **is** the manifest.
  Only dropdown mods have `info.xml`, and that is metadata plus a reload trigger, not a schema.

**What a shim would need, in order of cost:**

1. **`.bhs` execution** (biggest, and already a live lane). Gates AI-script mods, scenario mods,
   and — because `script_run_time` is checksum channel 15 — any *validation* of a scripted game.
2. **The `StringTable`.** Everything with a numeric extension, plus the mod's own display names.
   Cheap, and it unlocks tribe/localisation mods, which are a large share of the Workshop.
3. **External XML loading plus `unitrules.xml` / `techrules.xml` / `buildingrules.xml` binding
   tables.** We do not currently parse those files, and even `rules.xml` lacks a file-to-block
   loader. Without both parser and bindings, a mod's changes resolve and then land nowhere.
4. **Steam UGC subscription**, only if we want to *download* mods rather than read a folder.
   Not on the critical path.

**The one structural caveat, and it is not a shim problem:** in fidelity mode a mod is part of
the simulation's identity. `Array<T>`'s capacity *and* growth hint are checksummed, so any mod
that changes how many of something exists perturbs the checksum through the container, not only
through the values. A mod set therefore belongs in the lockstep handshake — which is exactly
what retail concluded when it built `GameMod::sync`.

---

## 4. The extension surface beyond retail

### 4.1 New unit types: retail cannot, and here is the proof

`enum TypeIndex` is a **compile-time enum** running `0..=805`, and
`Balance::final_balance_table` `0x00C12BF4` is a static `short[493][493]` — 486,098 bytes,
matching `schema/live/balance-real.bin` exactly. There is no allocation and no count read from
data.

**So a retail mod cannot add a unit.** It can retune all 352 unit slots, rename them, repoint
their art and change what trains them, but the 353rd unit does not exist and cannot be made to.
Every "new unit" mod on the Workshop is a reskinned existing slot.

| family | ids | count |
|---|---|---:|
| goods (6 common + 44 rare) | `0..50` | 50 |
| units | `50..402` | 352 |
| gaia | `402..414` | 12 |
| buildings (wonders `526..543`) | `414..543` | 129 |
| items | `543..544` | 1 |
| techs (ages `544..551`, epochs `551..579`, finals `579..583`, govs `623..629`) | `544..629` | 85 |
| spells | `629..684` | 55 |
| bonuses | `684..806` | 122 |

Note the matrix is **narrower than the type space**: 493 < 806, stopping partway into the
building range. That is a shipped fact, not a reading error, and it is why a bias-folded base at
`0x00C06AFC` produced the "unexplained negatives" recorded in `CODEX.md`.

`don_content::extend::TypeSpace` keeps `0..806` reserved and byte-identical to retail and
allocates extension ids from `806` upward. That choice is deliberate: every retail id keeps its
numeric value so captured tables, `.rcx` command streams and disassembly listings still index
correctly; an extension id is trivially detectable (`id >= 806`) so fidelity mode refuses one by
construction rather than by convention; and `BalanceOverlay` becomes dense-capture + sparse
overlay, so the 486,098-byte capture stays ground truth and extensions cost only what they use.

### 4.2 Hooks

`HookPoint` is deliberately short, and **every variant names the retail step it sits at**,
because a hook that does not correspond to a real ordered position in `Game::do_frame` is a hook
whose determinism nobody can reason about.

| hook | retail position |
|---|---|
| `RulesComposed` | after `Constants::init` `0x00569A90`, before any world |
| `WorldInitialised` | after world build, before frame 0 |
| `PreLeaders` | `do_frame` step 8, `Leaders::process_all` `0x006ED2A0` |
| `PreObjects` | `do_frame` step 14, `Objects::process_all` `0x0065DCE0` (owner rotation `(frame + i) % 10`) |
| `PostFrame` | `do_frame` step 20, right after `Game::frame++` `0x005924BF` |
| `EndGame` | `do_frame` step 27, `Game::process_end_game` `0x00591CE0` |

`crates/don-sim/src/schedule.rs` owns `DO_FRAME` and belongs to another lane; this enum is the
contract a mod sees, not the schedule. `is_in_tick()` separates the two hooks that run before
the checksum stream starts (deviation visible only in the `rules` channel) from the four that
deviate every channel from the frame they fire.

### 4.3 Rule overlays as a registerable artifact

`RuleStack` is the object the fidelity/improved sibling lane should register into:
`Layer::Edition` is theirs, `Layer::Overlay` is a mod's, and `Layer::Session` is a scenario's.
The four known improved-mode candidates in the lane brief all express cleanly as `Edition`
patches or as code guarded by mode — the AI difficulty gather handicap and the `attack_dir`
semantics as patches, the `economic.bhs` step-20 infinite loop as a content replacement in
`ai\scripts\`, and Tikal's `TIKAL_TEMPLE_HP`/`TIKAL_TEMPLE_BORDERS` mix-up as a code fix since
it is a wrong *read*, not a wrong value. The two map-style defects in §1.6 are new additions to
that list.

---

## 5. Deliberate divergences from retail

Recorded so they are visible rather than discovered.

1. **`ContentStack::push` does not sort.** Faithful: retail sorts once, at the end of
   `readModStatus`. Callers must call `sort_via_priority()` explicitly.
2. **Path separators are `/` internally.** Cosmetic; the captured `relativeDirectory` strings
   are rewritten by the generator.
3. **The dead base-directory assignment is not reproduced.** `calcFilePath` assigns
   `getBaseDirectory()` into the static result and then unconditionally overwrites it in both
   exits [measured, `0x00A229B3`–`0x00A229BE`]. Harmless, and reproducing it would be noise.

---

## 6. What is *not* established

* **Argument threading through `prepend_content_dir` was not traced.** The call *edges*
  `Constants::init → XML::init → String::prepend_content_dir` are measured; that the rules
  filename is the argument that reaches `calcFilePath` is inferred from the shape of the two
  functions, not proven. A live probe on the guest would settle it.
* **`ModManager::isScenarioBuiltIn` `0x00A22DA0` (2,797 B) and `isScriptBuiltin` `0x00A23890`
  were not extracted.** They pull names from the `StringTable`, not from literals, so the
  push-scan trick does not work. They gate publishing, not load precedence, so this is low
  priority — but "a mod cannot replace a built-in scenario/script" is currently **unverified**.
* **`ModPackage::calculateSteamTags` `0x00A35300` is implemented from the table, not traced.**
  The `!` negation prefix and the `"\"`-in-name subdirectory test are read off the string
  constants; no shipped row exercises either. Tagging is cosmetic, so this is acceptable, but
  it is not the same tier as the resolution algorithm.
* **No real retail mod has been run through this.** The guest has no mods installed and no
  Workshop subscriptions [measured again, 2026-08-09: neither the local `mods` directory nor
  Steam app `287450`'s Workshop content directory exists]. Every test uses the shipped `Data\`
  tree or a synthetic tree. Asking Ember to subscribe to two or three popular Workshop mods
  would turn §3's table from a classification into a measurement, and is the single highest-value
  next step for this lane.
* **`GameMod::compute_checksum`'s per-file checksum function was not identified**, so our
  `content_digest` is a placeholder, not retail's value. Matching it exactly is only necessary
  if we want to appear on a retail lobby.

---

## 7. Files

| path | what |
|---|---|
| `crates/don-content/src/vfs.rs` | retail discovery, classification, precedence; `ContentStack::resolve` |
| `crates/don-content/src/scan.rs` | directory → `ModPackage`, honouring per-category recursion |
| `crates/don-content/src/status.rs` | `mod-status.txt` reader/writer in retail's fixed-width format |
| `crates/don-content/src/overlay.rs` | the five-layer stack, validation, fidelity lock, audit |
| `crates/don-content/src/extend.rs` | `TypeSpace`, `BalanceOverlay`, `HookPoint` |
| `crates/don-content/src/compat.rs` | per-file support table and report |
| `crates/don-content/src/generated.rs` | tables captured from the binary — do not edit |
| `crates/don-content/gen/gen_tables.py` | the generator; re-run it, do not hand-edit |
| `crates/don-content/src/bin/don-content.rs` | `scan` / `probe` / `rules` |
| `crates/don-content/tests/shipped_layout.rs` | cross-checks against the shipped tree and the retail install |

Regenerate the tables:

```sh
cd /Users/ember/dev/don/ron-bin
uv run --quiet --with pefile --with capstone python ../crates/don-content/gen/gen_tables.py
```

The reproducible gate is `cargo test -p don-content --all-targets`; do not copy a historical
test count here. A real-mod certification remains unavailable until a package is installed in
the guest and tested both through retail and through `don-content check`.
