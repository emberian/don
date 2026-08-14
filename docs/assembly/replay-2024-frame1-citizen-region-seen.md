# 2024 frame-1 Citizen regional visibility

Supported executable SHA-256:
`30478a44d612d386c1ebb6b552d09c5e731e78e808102db6633ceb1a4a71fd6e`.

This continuation consumes the exact `Frame1CitizenRegionSeenRequest` emitted by the detached
golden `Unit::find_goody_box` scan. The request is reached only after an allied WData territory
owner has no active City in the queried region, so retail next reads
`LeaderData::reg_forts[region]` at `+0x12DE`.

## Source join

`derive_frame_zero_build_registry_census` owns all 64 `reg_cities`, 64 `reg_forts`, and 64
`reg_docks` values for every ordinary-setup Leader. Its exhaustive constructor census proves
that setup creates one Village and no Fort or Dock. The compact registry arrays are not carried
in canonical `Sim`, so the historical zero is admitted at frame one only after the exact
post-command and post-SetAnim Build owners prove they have not crossed a relevant lifecycle:

- replay payload and owner-zero starting Village identity/region match the golden setup entry;
- sparse and dense Build registries remain equivalent in both exact images;
- owner zero's Build mark is exactly 2002 and both retained slots are live;
- object 2000 is the admitted Village with its captured row, UID, and type;
- object 2001 is the exact Dutch starting Market with its captured row, UID, and type;
- no third allocation, tombstone, replacement, or type drift can hide a Fort lifecycle.

For the requested region the setup census must agree with the already-proved zero City count and
must report zero Forts. The authority digest binds the request, replay payload, complete
post-command and post-SetAnim Sim hashes, regional values, and both live Builds.

## Resumption and item producer state

The continuation replays the read-only scan from its exact parent, records the regional authority
on the resolved `was_seen` journal entry, and then evaluates the normal `seen2 & player_mask`
fallback. It may return locally, reach `Unit::get_goody_box`, or stop at a different typed region
request; it never publishes the detached transaction.

Before any scan, the complete post-SetAnim Sim is serialized. Save validation distinguishes an
absent item producer from an initialized-empty registry: absence is accepted only when the whole
WData plane has neither `WFLAG_ITEM` nor an item-chain sentinel. Consequently, reaching
`ObjectsData::find_goody_at` from an item-marked cell proves the synchronized item registry is
present. The plan records either `AbsentAndMapMarkerFree` or `PresentSaveValidated`, including
the present registry length and map shape. There is no caller-supplied “empty registry” seam.

## Atomicity

The operation mutates no Unit, Guys, order, path, item, WData, fog, RNG, Leader, Group, or Groups
owner. The staged Leader pending write and outer `unit_masks2 |= 0x8000` restoration remain armed.
All failures return without a canonical write, and validation re-derives the census, Build join,
regional authority, resumed scan, and both composition digests.
