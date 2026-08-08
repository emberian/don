# Descent of Nations: Thrones & Agents

An RL env and high-performance batch resimulation of Rise of Nations (2003).

---

## What that means

Rise of Nations is a lockstep RTS: every client runs the same simulation and exchanges
only orders. That property is why this is possible at all. It also means "did we get it
right" has an answer that isn't a matter of taste — the engine computes a checksum over
its own simulation state every turn, and multiplayer recordings carry those checksums.
We can run our simulation against a real recorded match and find out, turn by turn,
channel by channel, exactly where we diverge.

The goal is an environment fast enough to train in and faithful enough that what an agent
learns is about *Rise of Nations* rather than about our approximation of it.

## The one rule

**Ground truth is the binary, the shipped data, or the live process. Nothing else.**

Twenty years of community documentation exists and we use none of it as a source. Not
because it's bad — much of it is careful, and where it's right we now know exactly *how*
right — but because a plausible number is indistinguishable from a correct one until
something independent disagrees, and tests written from folklore agree with folklore.

The corollaries were all paid for:

- Every claim is marked `[measured]` (verified here) or `[reported]` (read, unchecked).
- **Capture, don't calculate.** Test expectations come from the binary, never from
  arithmetic we did ourselves. This rule exists because we broke it and got caught.
- Decompiled C is a hypothesis. Ghidra silently reorders integer and floating-point
  operations on this profile — wrong in precisely the dimension that matters here.
- Differential testing is *testing*. We hold no formal semantics of Rust or x86, so
  nothing in this project is "verified" in the sense a proof assistant means it.

## Where it actually stands

The reverse engineering is substantially done. The game ships its own full PDB — 22,750
named functions, 19,914 types, field-level layouts, source filenames — which we found
late, having spent a day deriving by hand what it would have handed us. That day wasn't
wasted: it produced an independent derivation, and the agreement rate between the two
(86.5% of ~869 claims about the right function, 719/721 rule constants exact, 1,659/1,659
vtables) is a stronger statement about the method than either source alone.

The simulation is early. Mechanics are being ported subsystem by subsystem against the
engine's own 15 checksum channels, each with tests, most currently at "behaviourally
faithful, divergence unmeasured." The RL environment runs — around 98,000 env-steps/s at
1024 environments, a ~4,600× real-time factor — and reports that **34% of applied actions
currently produce no effect**, which is the honest measure of how much of it is still
surface.

## The character of the work

The findings we're proudest of are mostly corrections:

- The community damage formula is wrong in *structure*, not just constants.
- Borders aren't spline-based; that class only draws the ribbon.
- The pathfinder we first derived was the caravan-road pathfinder.
- The "3-sub-unit damage division" everyone documents does not exist in this build.
- Our own differential tests once compared retail against a *copy* of our code rather
  than our code, and would have passed forever while we drifted.

An adversarial reviewer reads every wave and has, more than once, been wrong about work
that was right. Nothing here is trustworthy because someone asserted it; things are
trustworthy in proportion to what would have had to fail for them to be false.
