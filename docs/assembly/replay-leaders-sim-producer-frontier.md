# Replay Leaders same-frame producer frontier

This tranche joins the largest current fixed-body `LeaderData` cohort to real state in one
`don_sim::tick::Sim` frame. It adds a fail-closed ownership receipt; it does **not** install
checksum channel 7 (`leaders`) or claim retail equality.

## Contract correction

The generated `WalkSpec::walked_bytes = 27,182` is a static lower bound, not the total byte
count of an executed active-Leader traversal. `LeaderData::walk_data` contains unresolved
variable payload visits and calls child walkers. The complete conditional frontier executes
28,428 bytes for an active row whose five arrays and UTF-16 string are empty:

```text
fixed prefix [0,+0x692a)                              26,922
eight Diplomacy::walk_data calls                 8 * 92 = 736
default dynamic-child suffix                                 770
executed default active-row transcript                     28,428
```

Positive array lengths or a non-empty production script extend that transcript. An inactive
row visits only its eight-byte header. Therefore 27,182 must not be used as a fixed runtime
denominator or presented as a complete per-row checksum length.

## Newly joined owner cohort

For each active checksum Leader slot, `bind_sim_owner_frontier` observes these current owners:

| `LeaderData` bytes | Same-frame owner | Source bytes | Already owned | Newly canonical |
|---|---|---:|---:|---:|
| `city_mark +0x408` | `Sim::cities.city_mark` | 4 | 0 | 4 |
| `cities_captured/lost +0x824/+0x828` | `Sim::vic_leaders` | 8 | 0 | 8 |
| `blacken`, tribute sent/received | `Sim::diplomacy` | 12 | 0 | 12 |
| population and `reg_pop[64]` | production runtime | 132 | 0 | 132 |
| `age_stamp[7]` | production runtime | 28 | 0 | 28 |
| six training-queue counters | production runtime | 24 | 0 | 24 |
| ages/epochs queued | production runtime | 2 | 0 | 2 |
| `last_unit_finished[352]` | production runtime | 1,408 | 0 | 1,408 |
| `num_units[352]` | production runtime, checked against victory | 704 | 704 | 0 |
| `num_queued[806]` | production runtime, checked against victory | 1,612 | 1,612 | 0 |
| six resource buckets | production runtime, checked against decoded economy | 24 | 24 | 0 |
| `control` | production runtime, checked against step 8 | 4 | 4 | 0 |
| **total** | | **3,962** | **2,344** | **1,618** |

Composed with the established victory/step-8 frontier (4,852 bytes per active row) and the
same-frame production-tech join (24 additional bytes), the conservative current-owner lower
bound is 6,498 visited bytes per default active row. It deliberately does not count a byte
twice merely because retail revisits the same object field in a child call. Against the
28,428-byte empty-child transcript, at least 21,930 visited bytes per active row remain
unsourced. Dynamic container contents can make the residual larger.

## `last_unit_finished` port correction

The PDB declares `LeaderData::last_unit_finished[352]` at `+0x6274`. Retail completion code at
`0x0062fa35` subtracts the first regular-unit TypeIndex (50) before storing into the array.
The port previously allocated 806 entries and indexed it by raw TypeIndex. The runtime now
allocates exactly 352 entries, applies the subtraction, and refuses an out-of-range type. Unit
and carrier completion tests pin both the length and translated index. Preflight also rejects an
impossible Unit classification outside `50..402` before allocation, queue, or Leader mutation.

## Agreement gates and red boundary

The join first rebuilds the victory/step-8 base from the exact supplied `Sim`. It then requires:

- every new fixed-body value to equal the independently assembled conditional transcript;
- all 352 unit counts and 806 queue counts to fit retail `u16` representation and equal the
  established runtime bytes;
- resource buckets and control to equal their existing owners;
- all source arrays to have their retail shape.

A canonical population mutation changes Adler-32 only after the independent generated column
is updated; a stale column refuses. The same mutation proof reaches the final dword of
`last_unit_finished`, and a generated-layout test pins every joined field's PDB offset, size,
element count, and walked status. Separate tests prove duplicate disagreement, negative-count,
shortened-history refusal, and pre-mutation rejection of an impossible Unit index. `checksum()`
always returns the complete frontier as an error and `installed_in_scoreboard()` is always false.

## Corpus result

The real replay evidence is intentionally unchanged because no producer was installed:

```text
leaders compares                 222,938 -> 222,938
leaders matches                       0 -> 0
leaders substantive compares          0 -> 0
leaders best survived turns            0 -> 0
installed                           false -> false
```

Those values come from `schema/replay-validation.json`; this patch cannot alter them because
`CheckAll` still has no Leaders source. Reporting conditional transcript checksums as retail
matches would fabricate evidence.

The largest residuals are the historical region/building planes, remaining score and AI
fields, most `Personality`, bitmask headers/payloads, `tech_at_start` history, live container
contents and representation metadata, the production script, and the decoded economy fields
which still lack a same-frame owner.

The frame-zero setup continuation now conditionally promotes the 16,512-byte
`reg_buildings[64][129]` plane from a complete canonical census of starting City-center
Builds. The adjacent setup chronology also owns the 516-byte `last_building_finished[129]`
initializer history because `Build::activate(0, 0, 0)` bypasses its only completion-id store.
The pre-strategy receipt also owns 256 zero bytes across the four regional strategy histories,
whose first writers are in `Leader::plan_strategy`. The setup activation chronology and
digest-bound replay Rules additionally own 258 bytes of `high_buildings` history and 384 bytes
of regional City/Fort/Dock registries. Together they lower the empty-child residual to 4,004
bytes per active row. The canonical BHS type owner further owns the eight-byte `tech` header and
all 109 walked `obs_flags` bytes, reaching a 3,887-byte residual per active row only while the
ordinary starting-town-one setup receipt is live. It does not change this module's general
same-frame residual: the Sim does not yet maintain the regional matrix through all later Build
lifecycle transitions. See
[`replay-leaders-frame-zero-reg-buildings.md`](replay-leaders-frame-zero-reg-buildings.md).

The exact `Leader::init` government store and contiguous zeroed activity/technology stamp block
add 48 more setup-surviving bytes per active row, reducing that conditional residual to 3,839.
They remain uninstalled with zero survival: the receipt proves only the constructor boundary,
not maintenance after the first replay turn.

An exact disjoint census of constructor-zero strategy scratch and regional arrays adds another
2,558 bytes per active row. Every admitted field is first cleared or recomputed by the first
`Leader::plan_strategy`, so this narrows the constructor-bound residual to 1,281 but cannot
survive that first pass. The general scoreboard therefore remains uninstalled and at zero
survived turns.

The constructor-zero `known_rares` and `rares_collected[44]` fixed history contributes another
180 bytes per active row, narrowing the setup residual to 1,101. Its first `calc_gather`
recomputation is outside the setup receipt, so this too remains uninstalled with zero survival.

Seven fixed-size BitMask visitor headers add 56 lifetime-stable bytes per active row, reducing
the composed residual to 1,045. Their shape survives arbitrary payload mutations, but their
current owner is not mounted independently on `Sim`; the enclosing setup receipt therefore
still reports zero survived turns and remains uninstalled.

Exact setup-surviving gather-high/score, best-stat/war, and garrison/action histories add 104
more fixed bytes per active row, reducing the composed residual to 941. The ordinary setup
transaction reaches none of their first mutators; all bytes remain at constructor zero except
the `attacked_by` and `gov_hero_frame` `-1` sentinels. The claim expires before the first
relevant plan/process/action writer and therefore remains uninstalled with zero survival.

The contiguous attrition/diplomacy, Conquer-the-World Hero, and repair stamp block contributes
another 24 constructor-zero bytes per active row, reducing the residual to 917. Ordinary setup
reaches none of those actions; the receipt expires before the first later writer and remains
uninstalled with zero survival.

The complete fresh-Village chronology supplies 16 more bytes per active row: exact one-valued
`city_mine` and `cities_built` counters plus adjacent constructor-zero Village counters. This
reduces the residual to 901, but later City lifecycle changes are not maintained here, so the
receipt remains uninstalled with zero survival.

The all-human setup gate additionally owns 92 previously conditional bytes in the raw
`Personality` child. `Personality::init` clears all 96 bytes and the active-human branch skips
the AI selection body; the existing runtime `raid` owner accounts for the other four bytes.
The composed residual is therefore 809 bytes per active row. This remains constructor-bound
and uninstalled because no live Personality maintainer is mounted.

The existing canonical `Game::init_teams` mutation seam then supplies all 32 bytes of each
active row's `chat_status[8]`. The starting setup retains its exact pre-state, post-state, and
ordered receipt; the Leader join re-executes that source and agreement-checks the result against
the independent conditional image. This reduces the residual to 777 bytes per active row.
Later team/diplomacy changes are not mounted here, so the channel remains uninstalled with zero
survival.

Finally, the ordinary non-scenario `Leader::init` tail supplies 50 new fixed bytes per active
row: zero bonus-card/rate arrays, walked padding, zero `defeat_stamp`, and exact
`team_color = who`. The already-owned conquest byte at `+0x6900` is agreement-checked and not
counted twice. The composed residual is 727 bytes per active row; later conquest/defeat state is
not mounted, so install and survival remain false/zero.

## Verification

The focused replay target covers the exact accounting, complete-but-red receipt, canonical
mutation sensitivity, stale conditional refusal, duplicate-owner refusal, malformed shape,
and representation-range refusal. Production tests cover both ordinary and carrier completion
stores into the translated 352-entry `last_unit_finished` array.
