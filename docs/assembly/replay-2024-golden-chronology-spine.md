# 2024 golden oracle/chronology spine

`setup_2024_golden_chronology_spine` keeps capture identity separate from executed authority.
The strict starting-Market schema-v1 manifest and City receipt are joined to schema v2's
post-Market setup entry, including three separately named RNG boundaries:

1. `TerrainGroups::place_all` return;
2. post-Village/shuffle immediately before the Market call; and
3. post-Market `Setup::build_units` entry.

The schema-v2 manifest is then projected into exactly eleven whole-Sim nodes: post-Market entry,
seven adjacent Unit receiver after-images, post-frame-zero/pre-serial-1, post-LeaderOptions, and
the frame-2 post-tick image. Every node is explicitly `CaptureOnly`. A valid hash proves which
image was captured and how manifests link; it does not prove that Don executed the transition.

The source-derived Market placement prefix now consumes the typed `WorldData::get_tregion`
receipt and executes the exact dry/self-owned territory, Town, City Market-count-zero, and
water-count-zero reads. The spine retains the complete validated
`LeaderProduceBuildingMarketBlockedLocationReceipt` and derives its first missing child from that
receipt's `next_child`. On the current receipt this is `0x006E1F5F ->
BuildTypeData::find_friends 0x00639270`. The opaque native-trace SHA-256 remains evidence identity
only. It cannot close that child or authorize candidate scoring, fine RNG, allocation, activation,
or any downstream Unit, frame-zero, frame-one, frame-2, or frame-379 execution.

When the Market placement owner gains another decoded continuation, the receipt can move its open
child and the spine will derive the new earliest boundary. The spine deliberately does not
hardcode a later checkpoint or issue a downstream execution authority.
