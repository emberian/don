# 2024 golden capture contract

## Minimal oracle bundle

`setup_2024_golden_capture::GoldenCaptureManifest` is the source-of-truth **oracle snapshot**
topology for the supported 2024 golden replay. Schema version 2 requires the unconditional Dutch
Market, Build `o=2001` type 436, in the independently captured `Setup::build_units` entry image.
That image is explicitly a supported-retail post-Market boundary. It proves the state at entry; it
does not prove the preceding `Leader::produce_building` lifecycle.

The minimal oracle bundle therefore has 11 unique canonical DoNSave images:

| Image | Retail boundary | Why it is independent |
|---:|---|---|
| 1 | `Setup::build_units` entry | Completed `place_all`, Village `o=2000`, Market `o=2001`, City/World links, activation and post-Market RNG are already present. |
| 2--8 | after setup receiver ordinals 0--6 | Seven adjacent `Objects::init_unit` after-images for Scout, two Dutch Merchants and four Citizens. |
| 9 | frame 1 before serial 1 | The frame-zero scheduler may have changed Scout/Merchant orders, masks, paths and object-search scratch. It is not the frame-zero setup image with a new frame number. |
| 10 | frame 1 after serial 1 | Exact return after both LeaderOptions commands. |
| 11 | frame 2 after `do_frame` | Earliest mandatory post-command scheduler checkpoint. |

The seven receiver entries also carry independently decoded detailed native-call receipts. The
manifest validates replay/executable identities, nonzero revisions and evidence digests, exact
receiver ordinals and every image-to-image edge. Content admission remains fail-closed in the
specialized binders. No API may describe this 11-image contract as Market lifecycle authority.

## Separate Market transaction bundle

`GoldenStartingMarketTransactionManifest` is the stronger, separate contract for a caller which
does need to consume `GoldenStartingMarketCityReceipt`. It has exactly two unique images:

| Image | Retail boundary | Required proof |
|---:|---|---|
| 1 | immediately before `Leader::produce_building(436,2000,0)` | Village-only Build/City/World/RNG state. |
| 2 | complete Market return | Exact Build/City/World/RNG after-image; its digest must equal oracle image 1. |

The transaction manifest additionally carries the native-trace digest, generated-World footprint
receipt digest, and ordered fine-grid RNG trace. `bind_golden_starting_market_setup_entry` joins
it to both `GoldenStartingMarketCityReceipt` and the schema-v2 setup-entry receipt. The resulting
digest-bound authority is accepted only by `produce_frame379_setup_with_starting_market`.

## Authority adapters

`bind_captured_frame379_setup_entry` admits oracle image 1 only after the completed `place_all` and
post-mountain terrain receipts agree with the supported retail capture. In addition to the
starting Village and City/World link, it now requires active owner-0 Market `o=2001`, current type
436, the same City slot, the Village-to-Market intrusive link, and no center/CITY flag on the
Market. The whole Sim digest retains its footprint, World mutations and post-Market RNG without
guessing an unresolved placement site. The receipt keeps the earlier post-`place_all` RNG state
separate from the later post-Market entry state; only the latter seeds the seven setup receivers.

`bind_captured_frame379_completed_init` admits each of oracle images 2--8. The manifest requires exact
adjacency, so a receiver cannot be spliced onto a different Market or World after-image.

`bind_captured_frame1_command_entry` admits images 8 and 9 and preserves all seven stable Unit
identities across the real frame-zero tick. Image 9 remains mandatory because Merchant scheduler
state and the Scout's Unit order/path and spatial-search after-images are not derivable from setup
receipts. The Scout source path is now represented by the detached
`frame0_scout_spellcaster` transaction: static castability, mana, and range are source-owned; its
sole dynamic child is an adjacent, composition-bound `ObjectsData::find` traversal receipt. The
golden human arm has no RNG draw, and it can stage Objects search scratch or request a Unit
CastOrder but cannot mutate
`CasterData::active_spells`. Its setup-empty active-spell array is therefore derivable and is not
an independent oracle field. `Frame0ScoutCasterInvariantAuthority` publishes only that narrow
owner invariant after the bounded human transaction; it explicitly does not claim Unit
order/path or Objects search-scratch invariance. `setup_2024_frame1_scout_caster` joins it to
setup ordinal zero and the post-command image before completing the frame-one empty Caster
child.

`bind_captured_frame1_post_command` admits oracle images 9 and 10. It independently mounts the recovered
five LeaderOptions stance writes onto a save/load copy of image 9 and requires byte-for-byte
equality with image 10. Its authority exposes the seven stable Unit identities, full Sim digest,
World checksum/RNG, and stable Village/Market row, object, UID, type and City facts.
`validate_frame1_post_command_authority` is the shared read-only gate for frame-1 process lanes.

`bind_captured_frame2_post_tick` admits oracle images 10 and 11. It verifies that the replay contains no
intervening simulation command, preserves the setup and Village/Market identities, and publishes
the first post-command tick oracle. It does not claim that the full tick has been reconstructed
offline. `validate_frame2_post_tick_authority` lets downstream lanes consume the immutable
checkpoint without duplicating capture logic.

## Optional second-stage captures

Frame 32 wildlife and frames 379, 384 and 391 are intentionally not members of the minimal
manifest. They require independent manifests so missing later state cannot invalidate or inflate
the earliest replay prefix. The supported frame-32 contract already lives in
`wildlife_frame32::Frame32WildlifeCapture`; see
[`replay-frame32-wildlife-frontier.md`](replay-frame32-wildlife-frontier.md). It must link to a
real frame-32 Sim in its own authority chain rather than being inserted into the 11-image bundle.

The frame-379 Group+Move, frame-384 Groups normalization and frame-391 observation captures are
similarly downstream products. None may relabel the frame-2 oracle or use a recorded replay
checksum as state.

## Current residual

Neither contract creates production capture bytes. The exact Great Lakes run still needs the
installed displacement assets and completed mode-5 world-generation owner before either the
post-Market oracle or the pre/post Market transaction can be captured lawfully. The Market native
trace, seven native Unit receiver traces, and frame-zero/frame-one tick images are capture debt.
Until the relevant manifest and receipts exist, no lifecycle or corpus advancement is claimed.
