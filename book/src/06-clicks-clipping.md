# Clicks and clipping — repairing damaged samples

The last two chapters *reduced* unwanted sound that sat alongside the good stuff.
This chapter is different: here the good sound has been **destroyed** in places,
and the tool has to *rebuild* it. These are the "repairer" tools from chapter 3,
and they behave more like an art restorer repainting a damaged canvas than like a
filter.

## Clicks — tiny holes in the wave

A click is a *brief, violent spike*: a speck of dust on a vinyl record, a
scratch, or a sloppy digital edit that left a sudden jump. On the waveform it's a
single sample (or a few) shooting way out of line with its neighbours; on the
spectrogram it's a thin vertical streak, because an instantaneous spike contains a
flash of *every* pitch at once.

The fix, **de-click**, is wonderfully intuitive:

1. **Find the spike.** Compare each sample to the typical level of its
   neighbours. If one is wildly larger than the local average — say ten times —
   it's almost certainly a click, not real sound. (Cathar's `--threshold` is
   exactly this "how many times louder than normal counts as a click" number.)
2. **Cut it out and redraw.** Delete the offending samples, leaving a tiny gap,
   and **draw a smooth curve across the hole** that connects what came before to
   what comes after. Cathar uses a gentle curved line (a *cubic interpolation*)
   so the patch blends in.

Because a click is only a handful of samples — a fraction of a millisecond — the
gap is tiny and the redraw is almost always invisible. De-click is one of
restoration's reliable wins.

## Clipping — when the tops get chopped off

Clipping is nastier. Every recording system has a ceiling: the loudest it can
represent (that ±1.0 from chapter 1). Push a signal past the ceiling — record too
hot, overdrive a preamp — and the system can't go higher, so it just **flattens
the peak off**. The rounded tops of the wave become flat plateaus. You hear it as
a harsh, fuzzy, "broken speaker" distortion on the loud parts.

```text
  recorded fine:                 CLIPPED (overloaded):
                                 ceiling ┄┄┏━━━━━┓┄┄  ← the top is chopped flat;
        ╭───╮                            ┃     ┃        the real curve is GONE
       ╱     ╲                          ╱       ╲
     ─╯       ╰─                      ─╯         ╰─

  de-clip's job: guess the missing dotted peak from the slopes either side
                                 ┄┄┄╭╴╴╮┄┄┄  ← an invented, plausible curve
                                  ╱      ╲     (a guess, not a recovery)
                                ─╯        ╰─
```

Here's the cruel part: when the top is flattened, **the information about how high
the wave really wanted to go is gone.** Unlike a click (a brief spike you can
delete), clipping erases whole stretches of the true waveform and leaves a flat
line where a curve should be. De-clip has to *guess the missing peak* from the
shape of the wave on either side.

How do you guess a peak you can't see? You use the fact that real sound is
**predictable**: the wiggle approaching the flat top was on a clear trajectory,
and the part leaving it continues that trajectory, so you can extrapolate the
curve that "should" have been there — rising above the ceiling and coming back
down — instead of leaving a plateau. The better the prediction model, the more
natural the rebuilt peak.

## An honest word about de-clip

De-clip is the hardest tool in this book, and it's important to set expectations:

- A **lightly** clipped recording (a few peaks just kissing the ceiling) cleans
  up beautifully — there's lots of surrounding curve to predict from, and the
  gaps are short.
- A **badly** clipped recording (long flat stretches, a distorted scream) can be
  *softened* but never truly restored. The original is gone; the tool is
  inventing, and across a long flat run even a clever guess drifts. Expect "less
  harsh," not "as if it never happened."

The professional state of the art here is genuinely sophisticated — it treats the
missing samples as unknowns and **solves** for the values that best fit a model
of the surrounding sound (an "autoregressive" prediction, the classic method) or
that make the result as *simple as possible* in the frequency view (a modern
"sparse reconstruction" approach). These are real mathematics, not a smooth line
across the gap, and they're why a top declipper can rebuild a peak so convincingly.

## How the big tools do it

- **Audacity** has a "Clip Fix" effect that estimates the missing peaks from the
  surrounding slope — the same basic idea as a simple de-clip.
- **Adobe Audition's** "DeClipper" and the click-focused "Automatic Click
  Remover" handle both problems with adjustable thresholds.
- **iZotope RX** is, again, the benchmark. Its "De-clip" and "De-click"
  modules use the advanced prediction/reconstruction methods above and apply them
  automatically across a whole file; "De-crackle" extends de-click to the dense,
  continuous crackle of old records. For serious restoration of damaged vinyl or
  badly clipped masters, RX is the tool the pros reach for.

For the **dense crackle** of old vinyl — thousands of tiny spikes, not isolated
pops — see **`decrackle`** in the separation chapter (chapter 19). It hunts
micro-spikes the way de-click hunts big ones.

Cathar's de-click is solid and reliable, and its de-clip uses **the modern
"sparse reconstruction" method described above** — A-SPADE (Kitić, Bertin &
Gribonval, 2015), the same family iZotope-class tools use. It treats the clipped
samples as unknowns and solves for the signal that is *simplest in the frequency
view* (sparsest across a windowed, overlapping spectrum) while keeping every
reliable sample exact and every clipped sample beyond the threshold — so a peak
is rebuilt toward its true height rather than flattened to a plateau. It's an
iterative solve (a little slower than a one-shot fill, and worth it). Light-to-
moderate clipping cleans up convincingly; it's still not a substitute for RX on
*heavily* distorted material — across long flat runs any tool is guessing. As
always: knowing *how badly* something is damaged tells you whether any tool can
save it.
