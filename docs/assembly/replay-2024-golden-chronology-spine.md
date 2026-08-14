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
water-count-zero reads. It then executes the exact type and World-bounds prefix of
`BuildTypeData::find_friends`. The spine retains and digests both complete receipts and derives its
first missing child from `find_friends.first_child`. On the current receipt this is `0x00639335 ->
ObjectsData::find_building_placed_at 0x00658C80`. The opaque native-trace SHA-256 remains evidence
identity only. It cannot close that child or authorize a friend result, candidate scoring, fine
RNG, allocation, activation, or any downstream Unit, frame-zero, frame-one, frame-2, or frame-379
execution.

When the Market placement owner gains another decoded continuation, the receipt can move its open
child and the spine will derive the new earliest boundary. The spine deliberately does not
hardcode a later checkpoint or issue a downstream execution authority.

## Detached frame-zero step-11 projection

`compose_golden_frame0_strategy_spine` adds a deeper, deliberately detached diagnostic cone. It
joins the final seven-Unit setup composition and schema-v2 final setup image to the independent
native capture immediately before owner zero's frame-zero `Leader::plan_strategy`. The call-entry
image remains `CaptureOnly`: its nonzero preceding-chronology and native-trace digests identify
evidence but do not prove that the Market or seven Unit receivers executed.

On that captured entry, the decoded prefix derives the exact escrow, active-City scratch, and 26
Leader planning-counter stores without mutating a Sim. Its typed local residual is
`LeaderData::get_team_terr` at `0x006B98D0 -> 0x006D62E0`. This is the deepest local strategy
frontier, not the globally earliest replay gap; the parent's Market
`find_building_placed_at` residual is retained.

The extension records all of the following as false and refuses any receipt that changes them:
owner-zero strategy completion, owner-one strategy start/completion, Scout reachability, and
Merchant reachability. Those actors cannot become chronological claims until both owner-zero and
owner-one strategy calls have completed through their exact children.
