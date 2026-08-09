# Independent presentation-content manifest

DoN does not currently ship an independently licensed art, audio, font, model, or video set.
The gameplay and browser integration can use locally extracted retail data during development,
but those bytes are not a distributable edition. This document defines the mechanical gate a
future independent presentation pack must pass; it does not turn the missing pack green.

The gate is [`tools/content-license-audit.py`](../tools/content-license-audit.py), and its
authoring schema is
[`tools/content-license-manifest.schema.json`](../tools/content-license-manifest.schema.json).
Run it against the exact assembled release tree, not a convenient source directory:

```sh
python3 tools/content-license-audit.py \
  --release-root /absolute/path/to/release-tree \
  --manifest /absolute/path/to/release-tree/independent-content.json
```

A pass proves all of these mechanical facts:

- every regular file beneath every declared asset root has exactly one manifest entry;
- no manifest entry points outside the release tree, through a symlink, or into an overlapping
  asset root;
- paths are portable to a case-insensitive target and are canonical relative POSIX paths;
- every asset and included license text matches its recorded byte size and SHA-256;
- every asset has a presentation kind, gameplay/UI role, author, HTTPS provenance URL, exact
  license identifier, rendered attribution text, and an explicit modification notice;
- every referenced license has one included, hash-bound text, and every declared license is
  actually used;
- the executable, script, archive, PDB, and locally generated retail-pack signatures enumerated
  by the policy cannot be renamed and admitted as presentation assets; executable mode bits also
  refuse.

The report deliberately emits `"legal_title_certified": false`. Its manifest digest identifies
which declarations and bytes were audited; it does not prove that anyone reviewed or approved
them. A release proof must separately pin that digest after a human visits each source URL,
confirms authorship and license scope, checks transitive references embedded in formats such as
SVG and glTF, renders the attribution text in the product and release notices, and decides
whether the intended combined use is compatible. Rewriting an asset and its manifest hash can
produce another mechanically valid report, but it cannot preserve that external approval. The
fixed SPDX allowlist is only project policy. It excludes proprietary, non-commercial,
no-derivatives, and unknown terms, but it is not legal advice.

## Manifest shape

One release can have several non-overlapping roots, such as `assets/art`, `assets/audio`, and
`assets/fonts`. License texts must live outside those roots so they cannot be mistaken for game
assets. Empty roots, empty assets, unmanifested files, unknown fields, duplicate JSON keys,
unreviewed license identifiers, suspiciously short license labels, and extensions inconsistent
with the declared kind all refuse.

```json
{
  "schema": "don.independent-content.v1",
  "package": {
    "name": "DoN independent presentation",
    "version": "1"
  },
  "asset_roots": ["assets/art", "assets/audio", "assets/fonts"],
  "licenses": [
    {
      "id": "CC0-1.0",
      "path": "LICENSES/CC0-1.0.txt",
      "size": 7069,
      "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
    }
  ],
  "assets": [
    {
      "path": "assets/art/ui/selection-ring.svg",
      "kind": "art",
      "role": "selected-unit ring",
      "size": 1234,
      "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
      "author": "Example Artist",
      "source_url": "https://example.invalid/assets/selection-ring",
      "license": "CC0-1.0",
      "attribution": "Selection ring by Example Artist, CC0-1.0",
      "modifications": "recolored and simplified from the source SVG"
    }
  ]
}
```

The zero digests are illustrative and will refuse. Record exact bytes, then have the release
review pin the resulting whole-manifest digest. Any later edit changes that digest and therefore
needs a new external approval record.

## Relationship to the release gate

This audit admits an independent content pack; it does not audit the rest of a source or binary
release. Run [`tools/release-audit.sh`](../tools/release-audit.sh) on the same assembled tree as
the separate proprietary-input and generated-output exclusion gate. A complete standalone
release needs both checks, a real non-empty manifest, renderer/audio consumers for the admitted
files, and in-product attribution. None of those broader conditions may be inferred from this
tool passing in isolation.

The JSON Schema is an editor/authoring aid. The executable audit is authoritative and applies
cross-entry, filesystem, Unicode, URL, executable-mode, and case-insensitive target rules that
JSON Schema cannot express over an assembled tree.

The synthetic regression suite contains no retail or third-party assets:

```sh
python3 -m unittest tools/content-license-fixtures/test_content_license_audit.py
```
