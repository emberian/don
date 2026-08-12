# Canonical LaunchPatrol/Scramble runtime

This tranche mounts the recovered LaunchPatrol (opcode 11) and Scramble (opcode 36)
transaction on the real simulation owners for one bounded selection cone: opcode-0 selects
ordinary Unit-band carriers or sparse-registry-bound Build-band airbases, and the aircraft are
ordinary Unit objects in each selected container's `inside_down` chain.

The production route is:

```text
exact decoded retail package shell
  -> typed checksum / telemetry / speed / camera receipts
  -> exact [Group][11|36] command indices and bytes
  -> persisted CommandPackage selection cache
  -> fixed groups_guys::Groups slot
  -> typed Unit Handle / BuildRow+uid selection image
  -> reciprocal, acyclic Build-to-Unit containment snapshot
  -> exact Cast/SpecialAnim busy + Handle-bound aircraft type snapshot
  -> persisted ordered scenario-ignore prune image
  -> checkpointed World/order/path/Group publish
  -> current v17 root with typed AIR_PATROL tag 6
  -> canonical Unit::work row 17
  -> shared air physics + revision-bound Unit search
  -> typed queue-first STRAFE tag 8
  -> canonical Unit::work row 16 effect
```

Both package commands consume zero RNG. AIR_PATROL and STRAFE share one
`StrafeRuntimeAuthority`, including the type table, exact actor/frame search observations,
RNG epoch and external-effect epoch. The common air-physics adapter is therefore not copied
into a second runtime authority. AIR_PATROL prepares a detached order/path/Unit/RNG image,
revalidates the full World digest and authority, then publishes the after-image once.

LaunchPatrol's single-best arm retains the selected container as the AIR_PATROL home. A real
Build-carried launch therefore cannot be projected through `World::unit_row_at`: row 17 resolves
the sparse Build address, binds its `BuildRow`, owner/object id, uid, full 220-byte image,
position and containment head, and revalidates that image immediately before publishing the
detached AIR after-image. A changed Build home rejects as `StaleHome` without order, path, Unit,
RNG or authority mutation.

The package adapter no longer requires replay callers to discard every command outside the air
pair. `canonical_air_package_shell` admits the bounded retail shell opcodes 57, 58, 72, 74 and
79, checks every fixed wire size, retains the complete ordered byte image, and decodes typed
checksum, camera and player-speed facts plus exact TurnData payload bytes. It deliberately does
not copy TurnControl or presentation state into `command::Bridge`; only the canonical Group/AIR
transaction mutates Sim. Commit rechecks the complete command image, so a shell mutation between
prepare and commit publishes nothing.

An answered containment chain with zero eligible aircraft is also a successful package, not an
adapter error. Opcode 0 has already advanced the canonical selection revision and may have
published a new fixed Group/cache even when Scramble installs no order. The transaction receipt
therefore binds the exact selection revision pair; empty explicit and cached Build Scrambles
commit that selection alone, retain every target order/path byte, and consume zero RNG.

## Retail comparison and the closure boundary

The executable bodies are still the authority for the contained-object walk and the two
install branches. Scramble installs AIR_PATROL above the selected member position for every
eligible non-helicopter child. LaunchPatrol consumes all six unaligned dwords and retains
its filter/force-all/single-best planner. Helicopters receive the recovered MOVE_TO image.

The canonical selector resolves all explicit recorded Scramble Group packets in the audited
corpus: `000100df0724`, `0002002a082b0824`, and `0003002a082b08630824`. Build selection uses the
sparse `BuildRow`, owner/object address, `BuildData::uid`, full 220-byte image, position and
containment head. Commit revalidates that image, and Build members never receive a fabricated
`UnitData::group` backlink. The persisted retail `(o,uid)` cache also resumes the recorded empty
`00000024` reselection. DoNSave already retained both sides of the containment link; it now admits
only reciprocal, active, acyclic Build-to-Unit chains.

The strongest reached opcode-11 witness is package index 2,304 / serial 2,305 / frame 66,809 /
play 1 from retail replay SHA-256
`e8c0103f21dbdb97ecd083c1899065209bdb055581daaceff0c3ee547100ef8d`. Its exact opcode
chronology is `[79, 0, 11, 58, 74, 72]`; command indices 1/2 are Group
`0004022d082e082f083008` (owner 2, Build objects 2093–2096) and LaunchPatrol
`0bebb20000a77d000002000000000000000000000000000000`. The six unaligned dwords decode to
target `(45803,32167)`, queue `2`, force-all `0`, bombers-only `0`, fighters-only `0`, which
enters the executable's single-best scoring arm rather than launch-all.

The remaining launch-all arm has one especially strong finished-replay witness: package 10,988 /
serial 10,989 / frame 65,929 / play 2 from SHA-256
`dab1c282556642300a5bc153f1f432f417fa039b265b4d72cb5876dd643ec055`. Its complete
chronology is `[0, 11, 79, 57, 74, 72]`. Group `000005` is the retail empty cached reselection;
the exact preceding explicit cache origin is package 10,981 Group
`0004052f08300831083208` (owner 5, Build objects 2095–2098). LaunchPatrol
`0be90d01007523000001000000010000000000000000000000` decodes to target `(69097,9077)`,
queue `1`, force-all `1`, and both type filters clear. The canonical saved cache resolves all
four Builds, force-all bypasses mana burn, and each installed AIR_PATROL retains its own Build
home.

Replay decoding no longer has to collapse every admitted pair to command indices 0/1. The
canonical pair entrypoint consumes the exact decoded Group and Scramble/LaunchPatrol slices plus
their `CommandPackagePosition`, retains the retail frame, serial, play and command indices in the
transaction receipt, and requires the packet frame to equal the canonical World frame before
selection. This matters for the recorded Build Scrambles at indices 1/2: presentation or lockstep
shell commands remain outside this bounded transaction, but their presence is no longer erased
from its provenance. Negative plays and stale frames fail before Group/cache/order publication.

`UnitData::is_busy` now follows the executable at `0x0060A370`: a typed current CastOrder reads
its exact spell id, then ORs the synchronized spell-type virtual answers at `+0x50` and `+0x54`;
when there is no CastOrder, typed SpecialAnim Enter/Exit supplies the recovered
`is_entering_or_exiting` tail. Missing spell rows or malformed typed orders fail before selection
or cache publication.

Armed `ScenarioData::ignore_orders` is no longer bypassed. The Sim owns and persists the scalar
plus all eight arrays in DoNSave v15; list order, duplicates and negative tombstones survive
load/resave. The action transaction recomputes `plan_ignore_order_kills`, including subordinate
to captain redirection, active `o_down` recursion, final leader-speed recomputation and the exact
last-member clear image. Its receipt binds full before/after Groups, ordered object facts and
Unit backlink clears. Selection, cache, Group, backlinks, target orders and paths still publish
through one checkpoint; a changed scenario list rolls all of them back.

No closure flag is changed in this isolated tranche; closure metadata remains a separate
integration decision after the patch lands with its save-version coordination.

## Focused evidence

`canonical_air_group_save_resume` proves:

- Scramble packet -> fixed Group/cache -> typed AIR_PATROL, with no RNG draw;
- current save/load/resave and reinstalled external type authority;
- the first resumed row-17 tick performs air physics and inserts typed STRAFE at queue first;
- the following row-16 tick executes the landed STRAFE runtime with identical loaded and
  uninterrupted World/order/path/RNG state;
- opcode 11 retains all six dwords and installs the expected relative patrol waypoint;
- the exact Build-carried retail opcode-11 witness selects one cheapest/closest aircraft,
  preserves its Build home across v17 load/resave, inserts STRAFE on the first resumed row-17
  tick and executes the same STRAFE/RNG state on the next row-16 tick;
- a changed Build uid/image between AIR_PATROL prepare and commit rejects with no publication;
- the unique exact cached force-all package consumes its full six-command shell, launches all
  four Build-contained aircraft with zero package RNG, survives v17 load/resave, resumes four
  row-17 activations and executes the due aircraft's row-16 STRAFE identically; and
- a changed PlayerSpeed shell byte between prepare and commit rejects before Group, cache, order,
  path or RNG publication;
- exact Cast predicate and SpecialAnim busy vetoes, including malformed-state gates;
- v15 duplicate/tombstone scenario persistence, captain/down recursion and stale-list rollback;
- an armed partial prune followed by AIR_PATROL save/load and identical resumed row-17 STRAFE
  insertion; and
- a changed canonical World between prepare and commit rejects without publishing the
  detached after-image.

`canonical_air_build_selection_save_resume` additionally proves:

- all three exact explicit Build-band replay packets select the recorded airbases;
- their exact recorded frame/serial/play/index positions survive canonical execution, including
  the 1/2 pairs, and stale position mutations publish nothing;
- an empty recorded Build selection and its saved cached reselection advance only the canonical
  Group/cache revision, with zero aircraft/order/path/RNG effects;
- contained Unit aircraft receive AIR_PATROL while retaining `group == -1`;
- current v15 save/load/resave retains the v13-origin Build selection cache and empty cached
  Scramble;
- Build uid or containment mutation between prepare and commit publishes no Group, cache, order
  or RNG after-image; and
- malformed nonreciprocal or cyclic Build garrisons remain unsavable.

No closure flag is changed by this tranche.
