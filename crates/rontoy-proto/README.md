# rontoy-proto

`rontoy-proto` is the dependency-free contract between the read-only Windows probe and the
host. The host may also use the same model for recordings and derived advisor events. The
canonical live/recording representation is binary `DONF`; JSON is a browser/debug projection.

The API surface is intentionally small:

```rust
use rontoy_proto::{decode_frame, encode_frame, Frame, Validate, WireLimits};

let frame: Frame = decode_frame(&bytes, WireLimits::default())?;
frame.validate(Default::default())?;
let same_bytes = encode_frame(&frame, WireLimits::default())?;
```

Important invariants:

- `decode_frame` rejects unsupported majors, CRC mismatch, truncation, missing required fields,
  wrong known wire types, malformed booleans, duplicate scalar tags, and integer narrowing.
- Unknown message kinds and unknown TLVs survive a decode/encode relay.
- Resource indices are the retail order `food=0, timber=1, wealth=2, knowledge=3, metal=4,
  oil=5`.
- Probe values remain in retail units. Conversion for display or analysis happens once on the
  host.
- `Snapshot::advice_allowed()` requires coherent single-player capture, one confirmed local
  human, own-player-only scope, and a complete process-memory identity.
- Direct engine income, stockpile-delta inference, and modeled income use distinct `RateBasis`
  values and per-record `Evidence`.

See [docs/protocol.md](docs/protocol.md), the JSON schema in `schema/`, and the generic
dependency-free JavaScript decoder in `examples/decode-donf.mjs`.
