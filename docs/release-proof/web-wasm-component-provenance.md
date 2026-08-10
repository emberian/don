# Web Wasm component provenance

`release/web-wasm-component-provenance.json` is a real, byte-bound precursor to the product
dependency notice gate. It covers the checked-in `web/public/wasm/don_web.wasm` component, not a
standalone Descent of Nations distribution.

The capture currently proves these narrow mechanical facts:

- the component is exactly 733,392 bytes at SHA-256
  `b908d6b86af7c3734477fe8af00899f9983da1db37cf420b099876f373570d5f`;
- its selected dependency candidate set is the five-package graph reachable from `don-web` in the
  exact `web/wasm/Cargo.lock` at SHA-256
  `7f311db3db512ef3d3b1b74c3455ec5513a3a1d6f2f8e12c3291f750bffe35d0`;
- the three repository-path packages resolve to the repository's declared
  `GPL-3.0-or-later` expression and tracked `LICENSE` bytes;
- the exact `memchr 2.8.3` and `quick-xml 0.38.4` archives matched their Cargo.lock checksums;
  their original package manifests and root license/notice files were mechanically extracted and
  retained under `release/notices/web-wasm/`;
- the verifier re-derives the exact lock graph and registry declarations, checks every retained
  evidence byte, and refuses any claim-bit promotion.

All five package records deliberately remain `pending-human-obligation-review`. A package's
declared expression and bundled text are evidence, not legal advice and not a decision that a
particular notice, source offer, or other treatment is sufficient.

## Capture and verification

Capture downloads only exact crates.io archives selected by the lock, verifies each archive
against its lock checksum before reading it, rejects links/path escapes, and writes the manifest
and root notice/license files carried by the archive:

```sh
python3 tools/release-proof/component_provenance.py capture \
  --output release/web-wasm-component-provenance.json \
  --evidence-root release/notices/web-wasm \
  --component don-web-wasm \
  --version 0.1.0 \
  --root-package don-web \
  --payload web/public/wasm/don_web.wasm \
  --lock web/wasm/Cargo.lock
```

Normal verification is offline:

```sh
python3 tools/release-proof/component_provenance.py verify \
  --artifact release/web-wasm-component-provenance.json
python3 -m unittest tools/release-proof/test_component_provenance.py
```

The JSON contract is `tools/release-proof/component-provenance.schema.json`. The executable
verifier is intentionally stricter than the structural schema: it re-hashes real files, resolves
the lock graph, re-parses license declarations, and requires the four negative boundary claims to
remain false.

## Still red

This artifact does **not** prove that every selected lock package contributes linked code to the
Wasm, or that an omitted package does not. It does not cover the surrounding HTML/JavaScript/data,
desktop binaries, retail inputs, presentation assets, an installer, or any other product payload.
It supplies no human notice/obligation determination and no source-provision decision. Therefore
it cannot be renamed to `release/product-dependency-notices.json`, cannot promote
`binary-third-party-notices`, and cannot satisfy `assembled-product-packaging`.

The next machine step is an exact assembled payload plus build-produced binary/SBOM linkage. The
next authority step is human review of the selected package obligations and source treatment.
