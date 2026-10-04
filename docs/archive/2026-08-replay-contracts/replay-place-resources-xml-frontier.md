# Replay `Map::place_resources` XML frontier

`crates/don-replay/src/place_resources_xml_frontier.rs` reconstructs the largest
immediately contiguous source-only tranche after the divvy-pool residual:

```text
ResourceDivvyPool::done_adding_goods returns
  -> previous frontier residual 0x0068f597
  -> normalize Map::map_filename and append ".xml"
  -> open selected map-style XML and mapstyles/default.xml
  -> dispatch resource category 0 (BONUSES)
  -> prefer a valid selected BONUSES node, otherwise use default BONUSES
  -> enumerate ordered BONUS children at call 0x0068fb98
  -> exact residual 0x0068fb9d
```

The reversal is pinned to shipped `riseofnations.exe` SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`,
shipped `rise.pdb` SHA-256
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`,
`Map::place_resources` (`0x0068f4f0`, PDB lines 6405..6674), and the shipped
`ron-data/mapstyles/*.xml` structure.

## Path and document chronology

Retail copies `Map::map_filename` (`Map +0x114`) after looking backward for `'.'`.
The search starts one UTF-16 code unit before the final code unit, includes index zero,
and calls `String::Del` at `0x0068f68c` when it finds a dot. (A one-code-unit string has
no searched position.) It then
appends the internal string-table entry at `+0x173f4` (`.xml`) and initializes:

- the selected XML document at call `0x0068f6d1`;
- `mapstyles/default.xml` (internal string-table entry `+0x173b8`) at call
  `0x0068f6f9`.

Both document hosts remain live at this frontier's residual. Even an empty native style
name initializes the selected path `.xml`; the original string length, not load success,
controls whether retail attempts the selected category before falling back.

## Exact first-category fallback

The three-category loop starts at `0x0068f754`. This tranche owns only its first
dispatch, category `BONUSES` (internal string-table offset `+0x17a98`):

1. when the original style name is nonempty, call `XMLNode::get_element` at
   `0x0068fa51` on the selected document and assign it to `global_cur_element`;
2. accept it when its native tail pointer or inline tail word is nonzero;
3. otherwise call the same lookup on the default document at `0x0068fb0b` and assign
   that result instead;
4. at convergence `0x0068fb7f`, call `XMLNode::get_elements` at `0x0068fb98` for
   ordered `BONUS` children (internal string-table offset `+0x17ae8`).

A present but empty selected `BONUSES` section remains selected and produces zero rows.
It is not confused with an absent section. Capture-local row ordinals preserve native
order for the next typed row-body owner.

## Host references and deterministic state

Assignment to `global_cur_element` releases the old tail then old head, copies the new
head/tail, and add-references each non-null new handle. The receipt freezes that operation
order without pretending these host-only pointers belong in deterministic replay state.

The references deliberately remain live at `0x0068fb9d`. Their exact future tails are:

- category current tail/head and row current tail/head: calls `0x00690232`,
  `0x0069024c`, `0x00690266`, and `0x00690280`;
- selected document tail/head and default document tail/head: calls `0x006902ad`,
  `0x006902c7`, `0x006902e1`, and `0x006902fb` after all three categories.

There is no call to `Random::get` (`0x00a39d70`) in this span. The entry RNG state is
forwarded unchanged. The span performs zero World writes, leaves the World checksum and
sourced-walked byte count unchanged, and leaves the complete six-field
`Map::resource_pool` digest unchanged.

## Typed residual

`PlaceResourcesBonusRowsHandoff` owns the exact residual `0x0068fb9d`, the first
nonempty-row body `0x0068fbb3`, player-count argument, selected/default source identity,
live category/document host state, ordered rows, RNG state, World checksum/sourced bytes,
and resource-pool digest.

Shared integration derives that pool digest from the actual staged six-field
`ResourceDivvyPoolState`; XML facts cannot bypass the pool owner. The validated retail or
fixture evidence is retained in `PlaceResourcesXmlReceipt`, so the residual does not lose
the selected XML capture's EXE/PDB, string-table, RNG, World, sourced-byte, or pool binding.

The next red tranche begins at `0x0068fb9d`: it iterates each `BONUS`, parses its
attributes, performs the first direct chance draw at `0x0068fd64` when the native chance
key changes, and eventually dispatches `Map::place_resource` (`0x00691f70`) or
`Map::place_resource_special` (`0x00690480`). Categories `GOODIES` and `FISH`, their
callee RNG/World mutations, remaining category tails, return value, and the caller's
post-resource checkpoint at `0x0068c72d` remain red.

`crates/don-replay/tests/place_resources_xml_frontier.rs` freezes selected/default
fallback, present-empty semantics, UTF-16 suffix behavior, row order, host ref-operation
order, pending cleanup addresses, atomic rejection, and unchanged RNG/World/pool state.
`crates/don-replay/tests/map_make_resource_schedule_integration.rs` additionally proves the
typed pool-to-XML seam, exact `0x0068fb9d` schedule boundary, source evidence retention, and
joint rollback of pool and host state. The frontier is wired through `don_replay::lib` and
`execute_map_make_resource_schedule_with_xml`.
