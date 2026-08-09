# Unit and guy `inc_time` — recovered step-15 evidence

Status: **research-only, Tier C, not wired**. The module declares the measured traversal and
animation-clock structure, but its composite drivers are compiled only for tests and named
`*_research_partial`; `RUNTIME_FIDELITY_READY` is `false`.

## Retail shape

`Objects::inc_time` at `0x0065DB70` is step 15 of `Game::do_frame`. Unlike step 14, it walks
owners in fixed order and does not visit the wall object band. Live units receive virtual
`inc_time` followed by `Unit::execute_events`; buildings receive their shared
`Wall::inc_time`; goods, ammo, deaths, farms, doobers, and surf then take their measured paths.

`Unit::inc_time` at `0x00610B40` dispatches `Guy::inc_time` over the live squad prefix and the
crew suffix while skipping the dead middle band. `Guy::inc_time` writes fields inside the
`guys` checksum channel. Its transitions call `Guy::set_anim`. Each function activation can
draw zero or one time from `game_random`: the three sites are in mutually exclusive
requested-class arms. A captain/uber path can recursively invoke `set_anim`, however, so a
root call can create additional activations and does not yet have a proven finite draw bound.
Animation timing is therefore simulation state, not harmless presentation.

| Entered `set_anim` arm | RNG call | Exact local gate and result |
|---|---:|---|
| default / class 0 | `0x005DAC75` | after the captain/group-idle fallback reaches this block, draw only when `UnitData::openlist` (`+0x104`) is null; otherwise use a synthetic percentage zero |
| attack / class 12 | `0x005DB22A` | third argument enables autoselection; `<30` selects attack 1, `30..=70` attack 2, `>70` attack 3 before packet validation |
| nature walk / class 8 | `0x005DB346` | owner 9 and type `0x192`, `0x193`, or `0x194`; `<50` walks and `>=50` jogs before unit-mask overrides and packet validation |

The third argument is the attack variation/autoselect gate. The second argument is the
separate same-class restart/override gate; the measured `Guy::inc_time` call sites pass the
second argument as zero. These local arms do not stand in for the rest of the 4.7 KiB
`set_anim` body: earlier returns, captain/uber coordination and recursion, hero/spell cases,
packet fallbacks, and the final state writes still have to be integrated.

`Unit::execute_events` at `0x0060EDC0` is now transcribed in full as an exact dispatcher. It
walks `[0, guy_mark)` and chooses either `Guy::execute_events` or
`GraphicEvents::verify_load(gpiece)` from the unit/on-map/freeze gate. The 131-byte wrapper
itself has no RNG or checksummed writes.

`Guy::execute_events` at `0x005D99C0` is also transcribed over explicit order, unit, graphic,
animation, and object-lookup inputs. The exact recovered slice includes:

- the PDB-exact 56-byte `GameDataPackage` and high-byte angle conversion;
- TargetOrder, GroupAttackOrder, SpecialAnimOrder, attack-ground, and `cavarch_*` identity;
- the SEA/type-`0x15F` carrier clock stored in checksummed `trench_angle +0x5C`;
- verify-load ordering and attack UID/stale-target rejection;
- the strict graphic-event interval `last_time < start_time <= cur_time`;
- linked EventGroup selection: unconditional root, then the last wildcard/exact-civilization
  group whose age is strictly below the current age.

The graphic-event sink is not approximated. Projectile RELEASE reaches `Ammo::init` (measured
at zero to four `game_random` draws per projectile); RELEASE_PLANE reaches `Unit::come_out`
(a measured late branch draws zero or one time, with deeper callees still unaudited). Both
paths write checksummed object or guy state. Shipped event/node tables are not present in the
repository, so an empty synthetic event table would be a fidelity bug rather than a fallback.

The simulation arms of `GraphicEvents::execute_game_events` `0x008E48E0` are now transcribed
behind a fail-loud world/resource sink. RELEASE applies the exact target and low-node-bit
gates, temporarily translates the package to the extracted node, calls
`Objects::add_ammo -> Ammo::init`, then restores its scratch package. RELEASE_PLANE uses the
upper node bits, calls the first queued child's `Unit::come_out(0)` before removing that value,
places guy 0, and intentionally leaves the package translation accumulated for later events.
The bridge copies/sign-extends shooter `(who,o)`, target `(whom,ox)`, projectile gpiece, and
muzzle coordinates synchronously; it does not retain the package pointer.

These operations are checksummed. A successful projectile contributes the fixed live-ammo
walk plus optional spline in pool-slot order, including ballistic floats bit-for-bit. An
early `Ammo::init` abort leaves the first-free slot reusable but still advances
`Objects::ammo_index`. Plane launching-array removal changes the object/unit walk; placement,
angle, `last_z/z`, `des_angle`, and `last_pitch/pitch` change the unit and guy channels.
`AmmoInitReceipt` and `ComeOutReceipt` therefore require complete transitive RNG accounting;
the existing inexact tick launcher is not accepted as an implementation of this sink.

`GraphicEvents::verify_load` at `0x008E4780` is transcribed as an exact resource dispatcher:
before graphics initialization it queues the gpiece; afterwards it conditionally calls
`init_unit_events`, installs animations 7 through 11 with the measured `(0,0,3)` arguments,
and marks the gpiece loaded.

The supported install provides `Data/unit_graphics.xml` as a loose 3,440,809-byte file
(SHA-256 `f01b091f1df8c79207683f54daa417c2fb9861fbbb6595e33e4a0d74d108e54d`).
Retail resolves that XML through its live RData, animation, graphic-piece, object-type, and
sound registries. The resolved global is `graphic_events` at preferred VA `0x00C0B010`:
`events +0x04` points to per-gpiece 0x430-byte roots, each containing 38 event pointer arrays,
`civ/age` at `+0x428/+0x429`, and `next` at `+0x42C`. Normal XML loading creates one
unconditional `(-1,-1)` root. Within an animation bucket it preserves XML order in three
phases: RELEASEEVENT, PLANERELEASE, then SOUNDEVENT.

Because those resolved tables are proprietary installed data, this repository carries no
copied event table. `init_unit_events_from_extractor` instead admits a coherent extracted
result only when both the pinned executable hash and the exact installed XML hash match. It
requires all 38 buckets and a root, validates every event's animation bucket, and fails on
missing/malformed data. A live read-only extractor can walk the global layout above; an
offline extractor must run the same name resolution. Neither path has an empty default.

## Runtime blockers

- full `Guy::set_anim` integration, including captain/uber recursion and state writes
- a coherent extracted `GraphicEvents` / `EventGroup` / graphic-node pack wired at runtime
- complete `Objects::add_ammo` / `Ammo::init` RELEASE adapter, including target abort,
  anti-air-before-scatter RNG, slot reuse, unconditional graph index, and ammo checksum
- complete `Unit::come_out`, launching-array, and Guy-storage RELEASE_PLANE adapter, including
  all transitive RNG and checksum mutations
- shipped `AnimationPacket` / `.anm` durations
- `Wall::inc_time` `0x0063FB60`
- `DeathObj::inc_time` `0x008D5240`
- `Farms::inc_time` `0x008D8600`, including its two `game_random` sites
- the recursive squad path of `Guy::set_new_location` `0x005D86F0`
- `Doober::inc_time` `0x00846770`
- `Surf::inc_time` `0x008A1A00`

`MissingAnimData` reproduces retail's missing-asset fallback only. It is a research fixture,
not a model of the shipped game.

## Admission rule

Do not wire the partial drivers into the tick, replay validation, the RL environment, or the
playable edition. Admission requires shipped animation data, exact RNG consumption, all named
step-15 children, and retail differential evidence. Green local tests establish control-flow
consistency, not retail fidelity.
