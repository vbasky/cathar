# Sample rate and resampling

Back in chapter 1 we met the **sample rate** — how many times per second the
computer measured the wave. Sometimes you need to *change* it: a podcast host
records at 48,000 samples per second but the platform wants 44,100; an old clip is
at 22,050 and you're mixing it into a 48,000 project. Converting from one rate to
another is **resampling**, and doing it well is subtler than it looks.

## Why you can't just drop or copy samples

The lazy way to go from 48,000 to 24,000 would be to throw away every other
sample. The lazy way up would be to duplicate samples. Both wreck the sound, and
the reason why is one of the most important rules in all of digital audio.

Remember the rule from chapter 1: to capture a pitch correctly, you need at least
**twice** as many samples per second as that pitch's frequency. The highest pitch
a given rate can hold is therefore *half* the sample rate — a limit called the
**Nyquist frequency**. At 48,000, you can hold pitches up to 24,000; at 24,000,
only up to 12,000.

Now the trap. If you crudely halve the rate to 24,000 but the original still
contained, say, a 15,000-cycle pitch — above the new 12,000 ceiling — that pitch
doesn't just disappear. It **folds back down** and reappears as a *wrong, lower
pitch*, a ghostly tone that was never in the music. This folding is called
**aliasing**, and it sounds like metallic, gritty, "digital" nastiness. (It's the
audio version of why wagon wheels seem to spin backwards in old films — too few
"samples" per rotation.)

```text
  A fast wiggle, measured too rarely, masquerades as a SLOW one:

  the real (fast) wave:   /\  /\  /\  /\  /\  /\
  we only sample here:    ●           ●           ●
                           ╲         ╱ ╲         ╱
  so we "see" this:         ╲_______╱   ╲_______╱     ← a wrong, low tone
                                                        that was never there
```

## The fix: filter, then convert

So correct downsampling has two parts: **first remove every pitch above the new
ceiling** (so there's nothing left to fold), **then** drop to the new rate. And to
*invent* the new in-between sample values smoothly — whether converting up or down
— you don't copy the nearest old sample; you draw the ideal smooth curve through
the existing samples and read the new values off it.

The "ideal smooth curve" has a known best shape (mathematicians call the perfect
one a *sinc* function), and a good resampler uses a careful, tapered approximation
of it — cathar uses a *Kaiser-windowed sinc* — that both interpolates cleanly and
kills the aliasing in a single pass. You don't need the maths; you need the moral:
**good resampling is a smart filter, not a copy-paste**, which is why "just change
the number" in cheap software can sound worse than the original.

Cathar's `resample --rate 44100` does this properly in both directions, with the
anti-alias filter tracking whichever rate is lower.

## A cousin: bandwidth extension

The same family of ideas powers cathar's `enhance` tool, which tackles the
opposite problem — sound that's *missing* its highs (muffled, "telephone-y,"
because heavy MP3 compression or a low recording rate threw the top away). You
can't recover what was deleted, but you can **synthesize plausible new highs** by
taking the texture of the existing upper range and extending it upward, so the
result sounds brighter and more open. It's an educated fabrication, not a
recovery — useful for rescuing dull material, but it's adding an informed guess,
not restoring lost detail.

## How the big tools do it

- **SoX** ("Sound eXchange"), the venerable command-line audio swiss-army knife,
  has a famously high-quality `rate` effect — its resampler is a reference others
  are measured against.
- **libsamplerate** (a.k.a. "Secret Rabbit Code") is the open-source resampling
  library quietly embedded in countless audio apps; **r8brain** and **iZotope's**
  resamplers are studio-grade options.
- Every DAW resamples automatically when you drop a 44,100 file into a 48,000
  project — usually invisibly and well.

Resampling, like loudness, is *settled science*: there's a known-best approach,
and the difference between tools is how closely they approximate it and how fast.
Cathar's Kaiser-windowed sinc is a solid, standard implementation in the same
family as the references above — no model, no magic, just a well-built filter.
