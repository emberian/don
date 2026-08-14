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
  -> current v18 root with typed AIR_PATROL tag 6
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

The shell also owns the exact multi-pair package chronology instead of requiring one transaction
per pair. A retail package at turn 8,853/frame 53,053 in replay SHA-256
`bc2c1f1a8bfb4b7e0d83a3f2ff69fb18ead1f2041507f6b3e864a1a069ee6089` contains
`[79,0,11,0,11,0,11,57,74,72]`: three cached Group+LaunchPatrol pairs at command indices
1/2, 3/4 and 5/6. The batch host replays them in order on detached World/Groups/paths/cache
images, then reruns the same three transactions on live owners under whole-command, authority and
canonical-state CAS. A late-pair refusal restores the package checkpoint, so an earlier install or
selection revision can never leak from a partly applied retail package. The shell receipts remain
read-only facts; this does not fabricate TurnControl or presentation state.

An answered containment chain with zero eligible aircraft is also a successful package, not an
adapter error. Opcode 0 has already advanced the canonical selection revision and may have
published a new fixed Group/cache even when Scramble installs no order. The transaction receipt
therefore binds the exact selection revision pair; empty explicit and cached Build Scrambles
commit that selection alone, retain every target order/path byte, and consume zero RNG.

The batch shell also admits one deliberately narrow opcode-28 no-action cone, without claiming
`Group::action_flight` generally complete. In `Group::action_launch_flight` (`0x006FBFB0`), the
ATTACK arm first evaluates the selected Group's non-strict AIRBASE count at
`0x006FC095` (`COUNT_TYPE = 0x11`, `AIRBASE = 0x1BF`). Its contained-object loop then applies
the non-strict `NUCLEARMISSILE = 0x13B` test at `0x006FC2D2`. For a wholly AIRBASE selection
whose complete containment chains contain no Nuclear Missile, every child exits at that test:
the Flight body installs no order and consumes no RNG, but the immediately preceding opcode-0
selection remains a real mutation. The canonical shell composes that selection-only Flight
result with the following Group+LaunchPatrol pair under the existing package-wide checkpoint.

The sole audited AIR-bearing package with this shape is package index 21,990 / serial 21,991 /
turn 21,991 / frame 131,771 / play 3 from retail replay SHA-256
`063bff8b293029c44cc14a4056eef130cf0efaf2e91632432653dbb1de477edc`. Its exact chronology is
`[0, 28, 79, 0, 11, 58, 74, 72]`. Flight
`1c36070000000000000000000000000000000000000a000000` targets owner 0 / Unit object 1,846 with
ATTACK (`OrderIndex = 10`) and all three modifier fields clear. Both Group packets are the cached
owner-3 selection `000003`; its latest explicit origin is package index 21,985, Group
`000403e808e908ea08eb08`, selecting Build addresses 2,280–2,283. This replay evidence fixes the
wire chronology and cache origin. The all-AIRBASE and no-Nuclear-Missile facts remain mandatory
host authority, not facts inferred from the recording.

Cycle 15 extends only the target band of that same proven no-action cone. Of the 2,164
no-modifier ATTACK Flight pairs, 658 select Build-band objects and target another Build-band
object. All 658 occur in package shells already decoded by the canonical batch host, across 21
replay files; 92 selections are explicit and 566 reuse the player cache, with effective selection
sizes `1:336, 2:80, 3:64, 4:92, 5:21, 6:56, 7:9`. None of those packages contains LaunchPatrol
or Scramble, so this cone adds standalone Flight package leverage rather than duplicating the AIR
pair coverage.

The executable's no-Nuclear-Missile exit precedes every order-install path and does not require a
Unit-band target. The host therefore admits a sparse-registry-bound valid Build target under the
same all-AIRBASE selection and complete no-Nuclear-Missile containment proof. Its BuildRow,
owner/object address, UID, and position are snapshotted; both direct commit and package rerun bind
the target image before publishing the opcode-0 selection. The exact witness is the finished
replay SHA-256 above at turn index 54,616 / serial 54,617 / frame 54,211 / play 0. Cached Group
`000000` reselects owner-0 Build 2,064 from explicit origin turn index 54,537, Group
`0001001008`; Flight `1c21080000030000000000000000000000000000000a000000` targets owner 3 /
Build 2,081. Replay bytes establish the package/cache chronology; AIRBASE and containment type
answers remain mandatory runtime authority.

Cycle 14 adds one second, separately bounded Flight arm. The full corpus contains 2,205 opcode-28
pairs, all immediately preceded by opcode 0; 2,164 are ATTACK with no modifiers, versus 18 order-1
requests and 23 shift-ATTACK requests. Among those 2,164 ATTACK pairs, selection/target bands are
942 Unit→Build, 263 Unit→Unit, 658 Build→Build, and 301 Build→Unit. The highest-leverage exact
Unit-selected cone is therefore the already-STRAFE retarget arm, not a speculative fresh-order
installer.

At `Group::action_flight` `0x006FB6DB`, retail reads the actor's current order type. When it is
STRAFE (16), `0x006FB700..0x006FB87B` obtains the existing concrete payload and, for ATTACK,
writes the new target object/owner/UID, target position, `mandatory = 1`, `returning = 0`, and
the common group-order flag. The canonical host reproduces only that arm. Every selected member
must be an active Unit with Handle-bound air authority, clear missile mask, non-Nuclear-Missile
type answer, positive exact `mana_left`, and a coherent current typed STRAFE payload. The target
may be an active Unit or sparse-registry-bound active Build; its identity, UID, and position are
revalidated at commit. Fresh STRAFE insertion, non-STRAFE current orders, shift/control/alt,
order 1, scenario-ignore pruning, and every later Flight branch remain typed refusals.

Cycle 16 keeps that fresh-install boundary red while completing the exact skip/tail behavior
inside the existing-STRAFE arm. The 942 Unit→Build ATTACK packets span 28 replay files; 603 use
explicit selections and 339 use cache reuse, with effective selection sizes from 1 through 128.
941 already fit the canonical Flight batch shell. The sole shell residual is replay
`Playback___2019.03.24_11_56_19__Sun_.rcx`, turn index 10,869 / turn 10,870 / frame 65,215 /
play 2, whose `[0,10,0,28,57,74,72]` package still has an unsupported preceding Patrol pair.
None of the 942 packages contains LaunchPatrol or Scramble.

The bounded actor loop now follows four executable landmarks. ATTACK first records whether the
Group contains any non-strict Nuclear Missile at `0x006FB555`; when true,
`0x006FB67B..0x006FB6BE` skips each non-nuclear actor before reading its order. An actor which
does reach `0x006FB6DB` must still have a coherent current STRAFE—non-STRAFE actors retain the
fresh-install red boundary. `0x006FB73E..0x006FB748` skips a current STRAFE whose type carries
object mask `0x08000000`. Otherwise `0x006FB7A2..0x006FB7E7` rewrites the target fields before
`mana_left` is tested at `0x006FB801`: zero fuel retains that target-only rewrite, while positive
fuel reaches `0x006FB865..0x006FB87B` and additionally clears `returning`, sets `mandatory`, and
sets the common Group-order flag. Per-actor receipts distinguish all three current-STRAFE effects
plus the pre-order nuclear-group skip, and the package CAS still publishes them atomically with
opcode-0 selection. No branch in this cycle calls `Unit::add_strafe_order` at `0x006FBEB3`.

The exact fixture is finished replay SHA-256
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`, turn index 44,294 /
serial 44,295 / turn 44,295 / frame 43,963 / play 0. Its package is exactly `[0,28]`:
Group selects the 22 Unit objects
`[14,21,64,97,151,189,85,120,132,136,145,169,182,191,144,131,202,203,205,206,207,208]`,
and Flight `1c20080000000000000000000000000000000000000a000000` targets owner 0 / Build 2,080
with ATTACK and no modifiers. The test mounts the pre-existing STRAFE image explicitly; the
recording proves command bytes and chronology, while current-order/type/fuel facts remain
authoritative host inputs.

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
  preserves its Build home across current load/resave, inserts STRAFE on the first resumed row-17
  tick and executes the same STRAFE/RNG state on the next row-16 tick;
- a changed Build uid/image between AIR_PATROL prepare and commit rejects with no publication;
- the unique exact cached force-all package consumes its full six-command shell, launches all
  four Build-contained aircraft with zero package RNG, survives current load/resave, resumes four
  row-17 activations and executes the due aircraft's row-16 STRAFE identically; and
- a changed PlayerSpeed shell byte between prepare and commit rejects before Group, cache, order,
  path or RNG publication;
- the exact triple cached package applies all three AIR pairs atomically, survives current v18
  save/load, and resumes matching row-17 execution; a stale third carrier publishes none of the
  first two installs or cache revisions;
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

`canonical_air_package_shell_save_resume` additionally proves the exact mixed package above:

- the Flight receipt retains target owner/object, ATTACK, both exact packet slices, all four
  selected Airbases, and the full four-child non-missile containment walk;
- Flight changes only the canonical Group/cache selection, then the following single-best
  LaunchPatrol installs exactly one AIR_PATROL order with zero package RNG;
- current save/load/resave retains that combined after-image and resumes the next AIR_PATROL
  tick identically; and
- a changed contained aircraft after whole-package prepare rejects before either the Flight
  selection or the following LaunchPatrol can publish.

It also proves the exact 22-Unit Flight witness: all current STRAFE payloads retarget to Build
2,080, preserve their other walked state, consume zero package RNG, and round-trip through the
current save format byte-for-byte. A changed Build UID after prepare publishes none of the 22
retargets, while a fresh-install actor or shifted Flight packet is rejected without selection
publication. The same fixture now proves that a fuel-exhausted current STRAFE receives only the
native target-field rewrite and survives save/load, a missile-mask peer remains order-identical
while ordinary peers retarget, and a nuclear Group skips non-nuclear members before their empty
order queues are inspected.

No closure flag is changed by this tranche.
