# Prior-art survey: RoN reverse engineering & batch RL simulators

*Gathered 2026-08-08 by a web-research agent; URLs were reported by the agent and
spot-checking is still worthwhile (Fandom/PCGamingWiki were read via Wayback due to
Cloudflare). Two halves: (A) RoN-specific RE landscape, (B) batch RL simulator prior art.*

Companion context: the complete mechanics XML set was extracted from the Steam EE
install (Parallels VM "Windows 11") into `ron-data/` via `prlctl exec … type` streaming.
`ron-data/` is copyrighted game data — never commit it to a public repo; ship the
extractor instead (OpenRA/openage convention).

---

## A. Rise of Nations: Extended Edition — RE survey

Bottom line: **RoN is unusually data-driven for its era — most mechanics constants live
in plain-text XML shipped uncompressed in the install dir, and the community (chiefly
Vanshilar and MHLoppy) has empirically documented the core formulas to near-spec
precision.** The Ghidra budget is real but bounded: it's needed for the *glue* (exact
rounding/order of operations, target selection, pathfinding, RNG call sites, replay
format), not for discovering what the mechanics are. No prior reimplementation exists;
nobody has parsed the replay format.

### 1. How data-driven RoN is: the XML layer

All gameplay data is plain XML in `<install>\data\` (plus `tribes\` for per-nation
files), editable with any text editor; EE keeps the same layout and adds Steam Workshop
distribution. Multiplayer aborts with "loss of synchronization" if players' data files
differ — i.e. **XML values are simulation inputs to a lockstep sim, not client-side
cosmetics** ([Unitrules.xml wiki](https://riseofnations.fandom.com/wiki/Unitrules.xml),
[Lord Cirone's modding guide](https://ron.heavengames.com/library/articles/nationsmodding/)).

Key files (each has a Fandom wiki page):

- **`rules.xml`** ([wiki](https://riseofnations.fandom.com/wiki/Rules.xml)) — first ~175
  entries are general game parameters: **flanking damage bonuses, the age-advantage
  damage table, base gather rates of structures, plunder calculation,
  `UNIT_COST_FACTOR`, `UNIT_MOVE_SPEED`, the focus-fire "overkill" penalty,
  height-advantage thresholds, population caps** — then Wonders, nation powers, rare
  resources, governments, Conquer-the-World, Patriots/Generals, Armageddon clock, then
  tooltips.
- **`unitrules.xml`** ([wiki](https://riseofnations.fandom.com/wiki/Unitrules.xml)) —
  every unit: `ATTACK`, `HITS`, `MOVES`, `COST` (×10 via `UNIT_COST_FACTOR`), `SUPPORT`
  (per-unit ramp increment, *not* ×10), `PROGRESSION` (ramp mode 0–3: 0 = linear per
  same type, 3 = progressive per group), `RECHARGE` (attack delay **in frames**, 15 =
  1 s), `ARMOR`, `ATTENUATE` (accuracy delta per tile of distance), `FLY_HIGH`/`FLY_LOW`
  (**percent chance-to-hit** vs high/low-flying air — explicit RNG), `AMMO_PER_ATT`
  (damage split across projectiles; armor applied per projectile), `PROJ_SPEED`,
  `JOB_EXTRA_TIME` (build-time ramp fraction), `RESEARCH_PREMIUM_COST`, `OBJ_MASK`,
  `FLAGS`. The file's own header documents the fields.
- **`balance.xml`** ([wiki](https://riseofnations.fandom.com/wiki/Balance.xml)) — the
  damage-modifier matrix: 243–244 per-unit entries + 7 type entries (FORT, TOWER,
  SIEGE…) + 8 age entries + 32 object-mask entries; values are percentages (100 =
  neutral, 50 = half, 200 = double). This is the counter system. Crucially **not the
  only modifier source — each mask also has hidden hardcoded base modifiers in the exe**
  (see §2).
- **`techrules.xml`** ([wiki](https://riseofnations.fandom.com/wiki/Techrules.xml)),
  **`buildingrules.xml`** ([wiki](https://riseofnations.fandom.com/wiki/Buildingrules.xml))
  — research costs/durations/prereqs; building stats, wonder points.
- **`tribes/*.xml`** (nation powers wiring), `typenames.xml`, `help.xml`,
  `unit_graphics.xml` (animations — these *do* gate effective attack rate below
  `RECHARGE`), `mapstyles\default.xml` (rare-resource placement),
  `scriptfunctions.xml` (in-game script-editor function list).

**Established as hardcoded in the exe:**
- **Entity counts**: `num_tribes`/units/buildings/techs are baked in — adding (vs.
  replacing) entries fails with "num_*** does not match"; hex-editing attempts failed,
  and a 2004 claim says the original exes were obfuscated after the first patch
  ([Hardcoded frustrations thread](https://ron.heavengames.com/cgi-bin/forums/display.cgi?action=ct&f=10,5034,1560,all)).
- **Per-mask base damage modifiers** (the "innate table"): e.g. Foot Archers 253% vs
  Heavy Infantry, machine guns ×0.5 vs Armored — documented exhaustively with numbers on
  the wiki [Damage page](https://riseofnations.fandom.com/wiki/Damage) but *absent from
  balance.xml*.
- The *algorithms* (rounding, ordering, target selection): "I suspect some of the
  algorithms we're seeking are not in the open game files"
  ([damage-calculation thread](https://ron.heavengames.com/cgi-bin/forums/display.cgi?action=ct&f=10,1081,2490,all)).

### 2. Known engine formulas (community black-box RE)

The wiki Damage page (largely by Vanshilar, who also wrote the pathing analysis below)
is effectively a spec document:

- **Damage**: `damage = attack × mod₁ × mod₂ × … − armor`, **minimum 1**; 3-sub-unit
  squads deal `(…)/3` per sub-unit; multi-projectile attacks apply armor per projectile
  ([Damage](https://riseofnations.fandom.com/wiki/Damage),
  [Armor](https://riseofnations.fandom.com/wiki/Armor)). Modifier sources: age table in
  rules.xml (**115/120/150/160/170%** for 1–5+ ages ahead; units only, not buildings),
  innate mask table (hardcoded), balance.xml, terrain (light infantry in rocks ×2/3,
  units in rivers ×2), height (+10% per increment, thresholds in rules.xml), flanking
  (rules.xml; ~50% rear / 100% side base, reduced factors for cavalry ~40% and vehicles
  ~33%), and focus-fire "overkill" (**×1/3 damage for 30 frames**, in rules.xml)
  ([damage-calc thread](https://ron.heavengames.com/cgi-bin/forums/display.cgi?action=ct&f=10,1081,2490,all),
  [hidden modifiers thread](https://ron.heavengames.com/cgi-bin/forums/display.cgi?action=st&fn=1&tn=5962)).
- **Buildings**: under-construction secondary damage =
  `damage × (building_HP/2) / current_job_time` (frames); build rate drops to 1/4 while
  attacked; buildings attacking constructions deal ×4; enemy-territory buildings take ×4
  damage with 0 effective armor; wonders start construction at half HP
  ([Damage](https://riseofnations.fandom.com/wiki/Damage)).
- **Sim tick**: engine time unit is the **frame = 1/15 s at normal speed** (20/s fast,
  8/s slow, 5/s very slow); all XML timings are in frames
  ([Frame](https://riseofnations.fandom.com/wiki/Frame)).
- **Attrition**: base **1 HP per 48 frames (3.2 s)**, each tech tier doubles it;
  artillery ×½; militia line ×4 (and immune to mitigations); +25% per age ahead;
  buildings in enemy territory take a fixed **8 HP per 32 frames**, unmodifiable
  ([Attrition guide](https://ron.heavengames.com/cgi-bin/forums/display.cgi?action=st&fn=12&tn=2661),
  [Attrition wiki](https://riseofnations.fandom.com/wiki/Attrition),
  [official FAQ](https://ron.heavengames.com/faq/official/)).
- **Economy**: resource ticks every 30 s (20 s "turbo"); ~**10 per citizen per 30 s**
  base gather; cities auto-gather 10 Food/30 s; **commerce cap starts at 70**; ramp cost
  = `SUPPORT` per existing unit of type/group per `PROGRESSION`; build-time ramp caps at
  3× base + 1 frame; actual build times = 1.2× the file values; upgrade cost =
  `2×base + n×Δcost`, −10% per additional military tech; wonder ramp stepped ×1 / ×1.5
  (5th–7th) / ×2 (8th+), +0.5 for allied wonders
  ([MHLoppy economy guide](https://mhloppy.com/2018/09/ron-building-strong-economy-general-game-concepts/),
  [Ramping Cost](https://riseofnations.fandom.com/wiki/Ramping_Cost),
  [cost formulae thread](https://ron.heavengames.com/cgi-bin/forums/display.cgi?action=st&fn=1&tn=2490),
  [Gather rate](https://riseofnations.fandom.com/wiki/Gather_rate),
  [Commerce Limit](https://riseofnations.fandom.com/wiki/Commerce_Limit)).
- **Borders/territory**: mechanism documented qualitatively (cities/forts push borders;
  Civics/Fortification/Religion techs and support buildings increase push, Temple +2
  tiles; three wonders add border) but **no closed-form tile formula published** — push
  radii are rules.xml entries
  ([National borders](https://riseofnations.fandom.com/wiki/National_borders),
  [Territory](https://riseofnations.fandom.com/wiki/Territory)). Territory also drives
  supply: out-of-supply siege reloads at 66% (catapult/trebuchet) or 50% (gunpowder+).
- **RNG — real but narrow**: chance-to-hit vs air (`FLY_HIGH`/`FLY_LOW`), accuracy
  (`ATTENUATE`), random attack-animation selection (affects effective DPS timing). The
  2003 Gamasutra postmortem states "game code and **random number generators must run in
  virtual lockstep** across every machine"
  ([postmortem](https://www.gamedeveloper.com/game-platforms/postmortem-big-huge-games-i-rise-of-nations-i-)).
  Ground-vs-ground damage itself appears deterministic.
- **Pathfinding**: behavioral documentation only — units veer on open ground and **pause
  on a ~128-frame cycle** (some units idle ~20% of travel time)
  ([Vanshilar's blog](https://riseofnations.fandom.com/wiki/User_blog:Vanshilar/Rise_of_Nations_unit_pathing_issues)).
  No algorithmic RE exists — a Ghidra item if fidelity matters.

### 3. Replay / recorded-game format

- `.rcx` (T&P/EE; original RoN used `.rec`), in
  `Documents\My Games\Rise of Nations\Recorded Games\`. Community descriptions confirm
  **command-stream playback**: "a record of what each player did… the game actually
  replays by copying everything that every player did"
  ([HG thread](https://ron.heavengames.com/cgi-bin/forums/display.cgi?action=ct&f=1,5739,,60))
  — consistent with the postmortem's lockstep design. Replays are version- and
  data-file-sensitive (desync across patches/mods;
  [out-of-sync threads](https://ron.heavengames.com/cgi-bin/forums/display.cgi?action=st&fn=13&tn=1686)).
- **Nobody has documented or parsed .rcx** — no parser on GitHub or anywhere else
  (verified multiple ways). Genuine gap: bit-exact replay validation would require RE'ing
  both the container and achieving perfect sim determinism.
- Concrete head start: RoN's **scenario files are a 10-byte uncompressed header + a gzip
  stream** (terrain heightmap as floats inside), and the same thread says replays use
  the same compression
  ([HG decompression thread](https://ron.heavengames.com/cgi-bin/forums/display.cgi?action=ct&f=10,6474,,75)).
  Step one on `.rcx`: strip 10 bytes, gunzip — likely yielding header + initial state +
  timestamped order stream.

### 4. Prior art

**No open-source RoN reimplementation, clone, or decompilation exists** — verified via
direct searches, the GitHub [`rise-of-nations` topic](https://github.com/topics/rise-of-nations)
(8 repos, all mod tooling), and awesome-remake lists. What does exist:
[ptasev/Rise-of-Nations](https://github.com/ptasev/Rise-of-Nations) (BH3/BHA model↔glTF
converters), [mjn33/ron-objmask-workaround](https://github.com/mjn33/ron-objmask-workaround)
(balance.xml tooling), [MHLoppy/CBP-Launcher](https://github.com/MHLoppy/CBP-Launcher)
(C# patch launcher), [nicholasob/bhs_lsp](https://github.com/nicholasob/bhs_lsp) (BHS
language server, bundles the official
[scenario-editor docs zip](https://github.com/nicholasob/bhs_lsp/blob/HEAD/files/scenario_editor_docs.zip)).
Only public memory-layout RE: Cheat Engine tables
([fearlessrevolution](https://fearlessrevolution.com/viewtopic.php?t=7298)).

Analogue lessons: **openage** — fixed-point math, explicitly "no binary compatibility";
gameplay still non-functional after two decades (architecture ambition ate it).
**OpenRA** — faithful-not-exact, all-integer sim (`WDist` = 1024/cell), order-stream
lockstep; reads original game data. **OpenSAGE** — parses original .ini as ground truth
and used **replay parsing as a spec-discovery lens** for the order system
([dev diary](http://timjones.io/blog/archive/2017/12/10/opensage-dev-diary-2-2017-12-10)).
**OpenBW** — the only bit-exact success (plays real BW replays), and it inherited an
integer-math original with one frozen patch, still fighting per-mechanic desyncs.
Float-determinism background:
[Dawson](https://randomascii.wordpress.com/2013/07/16/floating-point-determinism/),
[Gaffer](https://gafferongames.com/post/floating_point_determinism/),
[Forrest Smith on desyncs](https://www.forrestthewoods.com/blog/synchronous_rts_engines_and_a_tale_of_desyncs/),
["1500 Archers"](https://www.gamedeveloper.com/programming/1500-archers-on-a-28-8-network-programming-in-age-of-empires-and-beyond).
Consensus: for a 2003 float-based RTS, **bit-exact replay compatibility is a research
project; faithful-mechanics + fixed-point/deterministic Rust sim is the proven path** —
which suits an RL env anyway.

### 5. RoN:EE specifics

- **32-bit x86, confirmed**: PCGamingWiki lists Executable = 32-bit native only (no
  64-bit), Direct3D 11, engine "Big Huge Engine", Windows Media Foundation cutscenes,
  PlayFab multiplayer as of the 2024 build
  ([PCGW](https://www.pcgamingwiki.com/wiki/Rise_of_Nations:_Extended_Edition), read via
  Wayback); corroborated by the community routinely LAA/4GB-patching
  `riseofnations.exe`. Trainers (WeMod/PLITCH/CE) work against it, so no aggressive
  protection; the obfuscation claim applies to the *2003-era* patched exes, and EE is a
  fresh SkyBox build.
- **EE ≠ T&P mechanically, by accident**: EE intends identical gameplay (SkyBox port:
  graphics, Steamworks, Twitch) but shipped a major regression — **object-mask
  entries/references in balance.xml are silently ignored (~28,677 damage modifiers
  wrong; e.g. War Elephants far too durable)**; the hardcoded per-mask base modifiers
  still work
  ([MHLoppy's writeup](https://steamcommunity.com/app/287450/discussions/0/3059616584927587379/),
  [CBP workshop page](https://steamcommunity.com/sharedfiles/filedetails/?id=2287791153),
  [EE wiki page](https://riseofnations.fandom.com/wiki/Rise_of_Nations:_Extended_Edition)).
  Plus an Iroquois unit-naming bug excluding three units from balance.xml. **Decide
  early whether the oracle is EE-as-shipped, EE+CBP, or T&P.** Versions: 2014 release,
  1.10, 1.20 (Nov 2017), server hotfix June 2024; Steam and MS Store variants crossplay
  each other only; no crossplay with 2003 versions
  ([Game versions](https://riseofnations.fandom.com/wiki/Game_versions)).
- **Scripting oracle: yes.** BHS (Big Huge Script) survives in EE — Script Editor via
  Tools & Extras / Edit Script / Ctrl+Alt+Z
  ([Workshop scenario guide](https://steamcommunity.com/sharedfiles/filedetails/?id=319580033)).
  ~800 functions cataloged
  ([Complete Script Functions List](https://ron.heavengames.com/cgi-bin/forums/display.cgi?action=st&fn=11&tn=5423)):
  state readers (`num_units`, `num_military_units`, `population`,
  `object_position_x/y`, `territory_owner`, `find_city_owner`, `have_war`…) and
  actuators (`create_unit`, `train_unit_with_cost`, `give_good`, `unit_move_order`,
  `group_attack_order`, `declare_war`, `pause_game`, `victory`/`defeat`…). Official
  docs: Script Manual + Script Functions Listing_2.doc + HelpTables.doc
  ([ModDB "RoN extras"](https://www.moddb.com/games/rise-of-nations/downloads/ron-extras),
  [GameFAQs Scripting FAQ](https://gamefaqs.gamespot.com/pc/919247-rise-of-nations-thrones-and-patriots/faqs/32654)).
  Combined with console cheats (`cheat resource`, `cheat age`, `cheat sandbox`,
  `cheat ai off`, `cheat diff`, `cheat finish` —
  [Steam cheats guide](https://steamcommunity.com/sharedfiles/filedetails/?id=1798468396)),
  you can build scripted micro-experiments in the real game to calibrate formulas. Main
  missing piece: **no confirmed file/log output channel from BHS** — a
  trainer/memory-reader may be the practical telemetry path.

**Ghidra actually required for**: exact arithmetic order/rounding of the damage
pipeline; the innate mask table's values (cross-check against the wiki's numbers);
pathfinding; target-selection AI; the RNG algorithm + call sites; the .rcx order
encoding; border-push geometry. Everything else is XML + published formulas.

### Explicitly NOT found
- Any .rcx/.rec format documentation or parser, anywhere.
- Any RoN engine reimplementation/decompilation project on any forge.
- Any Ghidra/IDA writeup of riseofnations.exe (only CE cheat tables).
- A closed-form border-size formula (mechanism + which file holds constants known; exact
  geometry isn't).
- Confirmation that BHS can write files/logs (needed for a fully scripted oracle).
- Direct Fandom/PCGamingWiki access (Cloudflare) — content verified via Wayback
  snapshots and search excerpts; spot-check live pages during scoping.

---

## B. Batch RL simulator prior art

**Madrona — concrete numbers and the actual architecture win**
([SIGGRAPH '23 paper](https://madrona-engine.github.io/shacklett_siggraph23.pdf),
i9-13900K + RTX 4090): Hide&Seek 1.9M steps/s @ 32K worlds, Overcooked 40M @ 64K,
Hanabi 21M @ 128K, Cartpole 3.4B @ 1024K. The stealable parts: archetype-as-global-table
SoA where one column-store table spans *all* worlds with an implicit WorldID column
(component columns *are* the exported obs tensors, zero-copy); dynamic entity delete =
`WorldID = -1` sentinel + periodic parallel radix sort on WorldID to compact; all
systems fused into one megakernel per batch step (needs ≥~8K worlds for >75% peak; a
naive one-env-per-CUDA-thread port is **42x slower** than their design). Still **no RTS
env on Madrona**; closest is [GPUDrive](https://github.com/Emerge-Lab/gpudrive) (ICLR
2025, >1M steps/s driving, hundreds of agents/world). Caveat: tensor export is
fixed-size columns — no ragged obs, everything pads.

**microRTS — deprecated, niche vacant**: both
[microrts](https://github.com/santiontanon/microrts) and
[MicroRTS-Py](https://github.com/Farama-Foundation/MicroRTS-Py) were **deprecated Aug
2025** ("lack of widespread community use"); no Rust or JAX successor exists. The
durable results: action composition (~301 logits vs ~50M flat on 16x16), and the masking
ablation — full parameter-level masking 0.82/0.73 win rate vs **0.00 with none**
([CoG 2021 paper](https://arxiv.org/abs/2105.13807)). First DRL competition winner was
RAISocketAI (CoG 2023, [arXiv:2402.08112](https://arxiv.org/abs/2402.08112)), whose
post-hoc finding — **behavior-clone scripted bots, then PPO fine-tune on win/loss** — is
the cheapest bootstrap recipe. (Also: no "10th microRTS competition" paper exists; only
9 editions.) Adjacent fast-core prior art: [PufferLib/Ocean](https://puffer.ai/blog.html)
pure-C envs at >1M steps/s *per CPU core*.

**Lux — the best proof RTS economies flatten to arrays**:
[JUX](https://github.com/RoboEden/jux), a JAX reimplementation of the full S2 engine,
hits **~307K steps/s @ 20K envs on one A100 (985x the Python engine)** via fixed-size
unit buffers (MAX_N_UNITS≈200) with parity guaranteed only for valid actions.
[S3](https://github.com/Lux-AI-Challenge/Lux-Design-S3) was JAX-native from day one and
deliberately shrunk (24x24, 16 units, hidden randomized params for meta-learning). S2's
power-taxed **action queues** (executing is free, *replacing* the queue costs power) are
a genuinely novel macro-encouraging mechanic worth considering.

**SMAX/JaxMARL** ([arXiv:2311.10090](https://arxiv.org/abs/2311.10090)): what they
deleted vs SC2 — no pathfinding/terrain (move-then-push-apart discs), heuristic
decentralized enemy AI that is itself a legal policy (self-play-ready, 50% achievable),
reward renormalized to not scale with agent count. 2.7M steps/s @ 10K envs (vs SMAC's
~27–83); note **a single JAX env is often slower than the CPU original** — batch +
co-located training is the entire win. Key negative result: **replay-based off-policy
methods scale badly in the GPU-resident regime; on-policy PPO is throughput-native**.

**Gigastep reality check**: the "1B steps/s" headline is an up-to extrapolation;
measured peaks are ~350M agent-steps/s (vector obs) / ~1.3M (RGB) on A100-class
([paper](https://proceedings.neurips.cc/paper_files/paper/2023/file/00ba06ba5c324efdfb068865ca44cf0b-Paper-Datasets_and_Benchmarks.pdf)).
Its stochastic distance-based visibility (fog-of-war as detection probability) was an
explicit fix for SMACv1 memorization.

**Action-space design, settled recipe**: AlphaStar's six autoregressive heads with
pointer-net unit selection and a 256x256 deconv location head
([architecture mirror](https://github.com/chengyu2/learning_alpha_star/blob/master/detailed-architecture.txt))
— including a learned **delay head** (agent chooses when it next acts) — plus the
entity-list-transformer + scatter-into-spatial-planes hybrid encoder. OpenAI Five: no
pixels, ~16K structured floats, factored heads masked to 8K–80K available actions
([arXiv:1912.06680](https://ar5iv.labs.arxiv.org/html/1912.06680)). Kanervisto et al.
([arXiv:2004.00980](https://arxiv.org/abs/2004.00980)): keep MultiDiscrete, never
flatten; autoregressive heads only pay off at full-game scale.

**EnvPool's CPU architecture** ([arXiv:2206.10558](https://arxiv.org/abs/2206.10558)):
ActionBufferQueue → core-pinned thread pool → preallocated StateBufferQueue, and
crucially **async batching with batch_size M < N envs** — `recv` returns whichever M
envs finish first, so slow instances (RTS step cost scales with unit count) never stall
the batch. 1.07M Atari FPS on 256 cores, ~3x subprocess even on a laptop.

**Rust ecosystem — confirmed empty**: no Rust EnvPool/Madrona equivalent. Closest:
[border](https://crates.io/crates/border) (training framework, no batched env engine),
[bevy_rl](https://github.com/stillonearth/bevy_rl) (REST, anti-throughput), and
[entity-gym-rs](https://docs.rs/entity-gym-rs) (Clemens Winter's typed-entity
obs/action abstraction — philosophically the best fit, low activity). Interface to
target: Gymnasium `VectorEnv` + PettingZoo parallel.

**Synthesis**: the two batching philosophies (fixed-shape SoA arrays end-to-end à la
JUX/Madrona vs. fast native cores + async M<N thread pool à la EnvPool/PufferLib) are
not mutually exclusive in Rust — SoA fixed-capacity world state stepped by a rayon pool
now, GPU-portable later. And the niche is confirmed wide open: no high-throughput env
anywhere covers the economy/production/tech-tree side of RTS that is RoN's core (SMAX =
combat only, gigastep = tag, Lux = closest but small and stylized).

---

## Net assessment

**Budget Ghidra as a targeted verification tool, not the primary mechanics source** —
the XML + Vanshilar/MHLoppy corpus is ~80% of a mechanics spec, and every analogous
project's experience says build a fixed-point deterministic Rust sim validated against
scripted in-game experiments rather than chase bit-exact replay playback.
