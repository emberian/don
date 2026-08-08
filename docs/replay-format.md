# The `.rcx` recorded-game format

**No public documentation or parser for this format existed** (see
`docs/prior-art-survey.md` §3 — verified absent across GitHub and the modding community).
This is a first opening of it.

Specimen: a skirmish played 2026-08-08, Dutch, defeated at game time 00:11:40, score 766,
against two AI opponents. Known initial conditions and outcome, which makes it the ideal
first specimen.

## Container [measured]

**The whole file is a single gzip stream starting at offset 0.**

```
1f 8b 08 00 00 00 00 00 00 0b ...
```

This **refutes** the [reported] claim (carried in `docs/prior-art-survey.md`, sourced from
an RoN Heaven thread) that recorded games use a 10-byte uncompressed header followed by
gzip. That may hold for *scenario* files; it does not hold for `.rcx`. Decompress with
plain gzip, no offset:

```python
import zlib
raw = zlib.decompress(open(path,'rb').read(), 16 + zlib.MAX_WBITS)
```

| property | value |
|---|---|
| compressed | 108,241 bytes |
| decompressed | 1,332,849 bytes |
| ratio | 12.3x |

## Payload head [measured]

```
16 42 1a 00  00 00 28 00  then UTF-16LE "Version: 00.2024.06.20..."
```

The version string carries **the engine build date, 2024-06-20** — which matches the PE
build timestamp in `docs/binary-ground-truth.md` exactly. So a replay self-identifies the
build that produced it, and version-sensitivity of playback (long noted by the community)
has a concrete mechanism.

`0x28` immediately preceding the string is a plausible length field; not yet confirmed.

## Corpus available

`C:\Users\ember\Documents\My Games\Rise of Nations\Recorded Games\` on the VM holds
replays spanning **2014, 2020, 2024 and 2026**, including a `multi\` subdirectory of
multiplayer games. Multiple engine versions across twelve years is exactly the corpus
format RE wants: diffing across versions isolates which fields are structural and which
are build-specific.

## Extraction

`prlctl exec` runs as **SYSTEM**, so `%USERPROFILE%` resolves to
`C:\WINDOWS\system32\config\systemprofile` and user-profile paths must be given
explicitly as `C:\Users\<name>\...`. Copy out with the certutil base64 hop documented in
`docs/binary-ground-truth.md`.

Replays are the user's own gameplay but derive from the game; `ron-data/replays/` is
gitignored.

## Next

- Identify the order/command stream and its per-frame framing.
- Cross-check against `CheckSum`/`DataWalk` (`docs/derivation/checksum.md`): if replays
  embed periodic checksums, they are a **per-frame oracle for our simulation**, which is
  the single highest-value use of this format.
- Confirm whether initial state is stored, or only a seed plus the command stream.
