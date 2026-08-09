# Release proof pack

This directory is the machine-checkable boundary between release facts already supported by
repository evidence and release work that is still missing. It does not pronounce legal advice
or infer a license. It records exactly what the repository itself declares, binds stable evidence
by SHA-256, and refuses a readiness claim when the required proof artifact is absent.

The current result is intentionally red for both release-evidence scopes. Neither scope means
whole-game/product readiness; gameplay completion remains governed by `GOAL.md` and
`tools/product-readiness.sh`.

- **source release:** `tools/owned-peer/Cargo.toml` declares MIT and the oracle's remote workspace
  template declares `MIT OR Apache-2.0`, but no applicable license texts are tracked. The remote
  template also disagrees with the repository workspace's GPL declaration. A whole-source license
  inventory and a reviewed classification of tracked binary/PDB/live-derived fact extracts are
  absent.
- **distribution proof:** in addition to that source blocker, the seven exact Cargo lock
  snapshots contain 179 package records without an audited product dependency notice inventory.
  One component-only precursor now binds the checked-in browser Wasm to its five-package lock graph
  and mechanically captures both registry packages' archive-carried declarations/texts, but every
  record remains pending review and the graph is not a linked-code SBOM. There is no standalone
  product payload/installer and no human-cleared manifest for independently licensed art, audio,
  fonts, and presentation data; the recorded retail-controller incident retained no dump; and the
  five-cycle STOP/rearm result lacks a compact per-cycle machine record.

These findings do not revoke or reinterpret any declaration. They prevent the project from
claiming that its release proof is complete until the missing evidence is supplied with authority.

## What is proved now

At the 2026-08-09 snapshot, the manifest accounts for every discovered repository Cargo manifest
or workspace template (23 total) and every `Cargo.lock` (7 total). Twenty package manifests
inherit or declare `GPL-3.0-or-later`; `tools/owned-peer` declares `MIT`; the workspace root
supplies the default GPL expression and text; and the oracle's remote workspace template declares
`MIT OR Apache-2.0`. The checker derives those values from TOML instead of trusting the JSON copy.

The pack also preserves narrow positive conclusions already supported elsewhere:

- the assembled-source-tree release audit and `git archive` exclusions refuse their enumerated
  proprietary/local payload classes and signatures;
- the owned-data bootstrap verifies one executable identity and 51 exact inputs and writes only
  create-only local copies under ignored `ron-data`;
- the documentation carries measured active-solo five-cycle STOP/rearm results, while the pack
  keeps distribution proof red until those results have a compact hash-bound machine record;
- the scoped WER workflow has setup, exact-owned removal, dump preservation, and stable in-place
  MDMP verification logic plus synthetic regressions.
- the checked-in browser Wasm component and exact five-package lock graph are hash-bound, while
  checksum-matching registry archives supply retained declaration and license-text evidence for
  `memchr 2.8.3` and `quick-xml 0.38.4`; all package obligation decisions remain explicitly pending.

None of those claims a finished installer, independent content, a retained live minidump, or an
assembled release. Those remain separate red gates in `evidence-manifest.json`.

## Commands

Validate that the proof pack still matches the repository exactly:

```sh
python3 tools/release-proof/check.py
```

Require a claim. These commands currently exit 3 and list the blocking gate IDs:

```sh
python3 tools/release-proof/check.py --require-ready source
python3 tools/release-proof/check.py --require-ready distribution
```

Run the text-only regressions:

```sh
python3 -m unittest tools/release-proof/test_check.py
python3 -m unittest tools/release-proof/test_component_provenance.py
python3 tools/release-proof/component_provenance.py verify \
  --artifact release/web-wasm-component-provenance.json
```

The checker does not compile, download, access a retail installation, copy content, or modify the
tree. A changed hash, newly added Cargo manifest/workspace template/lockfile, changed license
declaration, missing evidence path, inconsistent gate, or false readiness bit fails closed. Merely
adding a file with an expected top-level schema is also refused: each completion artifact needs a
schema-specific semantic validator in the checker before its gate can be promoted.

Before a source archive is published, run the existing source-payload audit independently against
the exact assembled source directory:

```sh
tools/release-audit.sh --staging-dir /absolute/path/to/exact-source-release-tree
```

That source staging audit answers “did known prohibited payload leak into this source tree?” The proof pack
answers the different question “is every named release obligation backed by an exact artifact?” A
source release needs both answers. A binary product additionally needs a product-specific,
allowlisted payload/provenance audit; `tools/release-audit.sh` deliberately rejects executables and
cannot certify such a product tree.

## Closing the red gates

Do not fill a field from memory or by copying a convenient license from elsewhere.

1. For `source-license-coverage` and `remote-workspace-license-consistency`, obtain applicable
   authoritative license texts and resolve the oracle template's inconsistent workspace expression
   with explicit authority; do not choose an expression merely to make the gate green.
2. For `whole-source-license-provenance` and `derived-research-release-review`, inventory non-Cargo
   scopes and classify every tracked derived research artifact before publishing a source archive.
3. For `binary-third-party-notices`, start from the exact assembled product/SBOM, select the
   actually conveyed subset from the repository lock snapshots, retain authoritative license
   evidence and required text, and bind source treatment. Merely listing SPDX guesses is
   insufficient.
4. For `independent-presentation-content`, record every shipped item, origin, stated
   author/rightsholder, exact license or owned-extraction rule, source URL or local source identity,
   content SHA-256, required attribution, destination, and human clearance. The auditor must not
   certify legal title, and retail content is never assigned a DoN license.
5. For `standalone-product-installer` and `assembled-product-packaging`, bind an exact payload and
   prove install/configure/repair/remove behavior before marking either gate proved.
6. For `retail-controller-byte-restoration`, add the redacted compact per-cycle record named by the
   manifest; prose describing the measurement is not a substitute for that artifact.
7. For `controller-stop-incident-closure`, add candidate-bound active soak/reversibility evidence.
   Do not intentionally induce another crash; if one occurs naturally, retain the stable MDMP in
   its protected local location and publish only a redacted identity/result and stack-supported
   conclusion.

Update hashes only after reviewing why the underlying evidence changed. A mechanical hash refresh
without reviewing the corresponding claim defeats the purpose of this pack.
