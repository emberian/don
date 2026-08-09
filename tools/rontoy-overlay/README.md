# rontoy-overlay

A topmost, click-through card that floats above the game window and shows the live
RoNtoy feed: six stockpiles with their commerce-clamp state, population, direct idle
counters, and the current advice.

```sh
tools/rontoy-overlay/build.sh                 # builds target/rontoy-overlay and audits it
target/rontoy-overlay --port 17360            # anchors to the Parallels window, top right
target/rontoy-overlay --corner bl --width 320 --opacity 0.85
target/rontoy-overlay --dump-windows          # owners and bounds of on-screen windows
```

It needs a RoNtoy host on loopback — start one with
`python3 tools/rontoy-host/rontoyctl.py up`.

The card fails closed on lifecycle changes. A proven paused capture is labeled
`PAUSED` and carries no advice. Once `/v1/status` marks the retained observation
stale, the card is labeled `STALE` and hides the old recommendation; if monotonic
host status itself becomes unreachable it says `SOURCE LOST`. Restarting the game
ends `donfeed` because the process identity changed; restart `rontoyctl` to bind a
fresh host slot to the new game session. Economy from the last immutable capture may
remain visible for orientation, but is never presented as current advice.

## Why a Mac-side window and not an in-game overlay

RoNtoy is read-only, so a Direct3D overlay is out: it would mean injecting a DLL into
`riseofnations.exe` and hooking `Present`. A separate OS window compositing above the
game touches the game not at all, and this one goes further — it is not even in the
same operating system as the game. It reads `http://127.0.0.1:17360/v1/latest` and
draws. That is the entire I/O surface.

The linked binary is audited on every build. `build.sh` fails if it references any
input-synthesis (`CGEventPost`, `CGEventTapCreate`), accessibility-control
(`AXUIElement*`), foreign-memory (`task_for_pid`, `mach_vm_*`), or screen-capture
(`CGWindowListCreateImage`, `SCStream*`, `CGDisplayStream`) symbol. What it does
reference is `NSURLSession` and `CGWindowListCopyWindowInfo` — the latter for window
owner names and bounds only, which needs no permission and yields no pixels.

## Two constraints, stated plainly

**Fullscreen-exclusive.** No window at any level can draw over a fullscreen-exclusive
Direct3D surface. On this host the point never arises: the guest game runs inside a
Parallels *window* on the Mac, so the overlay composites above it like any other Mac
window. If the VM is switched to Parallels full-screen mode, the overlay still shows
(the window carries `.canJoinAllSpaces` and `.fullScreenAuxiliary`); a guest-side
overlay would be the only option if the game itself ever took the guest display
exclusively, and that would still be a *second* window, never a hook.

**Click-through means no controls.** `ignoresMouseEvents = true` is what guarantees
the overlay can never be clicked, dragged, focused, or used to send anything anywhere,
and the app runs as `.accessory` so it has no Dock tile and never becomes active. The
cost is that every setting is a launch flag; there is nothing to click.

## Flags

| flag | meaning |
|---|---|
| `--port <n>` | loopback RoNtoy host port (default 17360) |
| `--corner <tl\|tr\|bl\|br>` | which corner of the target window to hug (default `tr`) |
| `--margin <px>` / `--width <px>` / `--opacity <0..1>` | card geometry |
| `--poll <seconds>` | host poll interval (default 0.5) |
| `--seconds <n>` | exit after n seconds |
| `--snapshot <path>` | render one PNG of the card itself (not the screen) and continue |
| `--no-follow` | anchor to the main screen rather than the Parallels window |
| `--anchor-owner <s>` | anchor to a different application's window by owner name |
| `--dump-windows` | print on-screen window owners, layers, and bounds; exit |

`--snapshot` renders our own view through `bitmapImageRepForCachingDisplay`, so it
captures the card and nothing else. It is not a screenshot API and needs no
Screen Recording permission.
