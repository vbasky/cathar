# Pulling a sound apart — separation, modeling, and gap-filling

The tools so far mostly *subtract* a problem — hum, hiss, a click. This chapter is
about three deeper moves: **splitting** a sound into layers, **rebuilding** it
from a model, and **filling in** pieces that are missing entirely.

## Harmonic vs. percussive — the HPSS split

Look at a spectrogram (the time × frequency picture from the "two views"
chapter). Two kinds of things live there, and they look completely different:

- **Tonal** sounds — a held note, a hum, a vowel — draw **horizontal lines**.
  They sit at one pitch and last a while.
- **Percussive** sounds — a drum hit, a click, a consonant — draw **vertical
  lines**. They're brief but splash across every frequency at once.

That visual difference is a handle you can grab. Run a filter *along time* and you
keep the horizontal streaks (the tonal part). Run one *along frequency* and you
keep the vertical streaks (the percussive part). That's **HPSS** — harmonic /
percussive source separation — and it needs no AI, just two median filters:

```bash
cathar hpss song.wav --harmonic tonal.wav --percussive hits.wav
```

The two outputs add back up to the original exactly. It's a fast way to pull the
drums out from under a melody, or to treat the "attack" of a recording separately
from its "sustain".

## Rebuilding from a model — sinusoidal synthesis

Here's a stronger idea: instead of *filtering* a sound, **describe** it and build
a fresh one from the description.

Most musical, voiced sound is a stack of steady tones — **partials**. `cathar sms`
(sinusoidal modeling synthesis) finds the peaks in each moment's spectrum, tracks
each one as it glides through time, and then **re-synthesises the whole recording
from those tracked partials alone**:

```bash
cathar sms noisy-flute.wav        # keep the tones, drop everything else
```

Because hiss, breath, and crackle *don't* form steady tracked partials, they
simply aren't rebuilt — they fall away. The result is a "tonal purify": the
musical skeleton of the sound, with the stochastic fuzz left behind. Push it and
it sounds synthetic (it *is* synthetic — you rebuilt it); used gently it's a
striking way to isolate the pure tone from a noisy capture.

## Filling holes — audio inpainting

Sometimes samples aren't just damaged, they're **gone**: a drop-out on a bad
transfer, a splice in a tape, a mute where a CD skipped. There's nothing to
subtract — you have to **invent** the missing stretch so it can't be heard.

The trick is the same one your ear uses to finish a sentence someone mumbled:
**predict from context**. Cathar fits a short mathematical model to the audio on
*both* sides of the hole — one that captures the local pitch and shape — and then
solves for the samples that continue smoothly in from the left and out to the
right at the same time:

```bash
cathar inpaint dropout.wav --start-ms 1240 --len-ms 8   # patch a known gap
cathar inpaint transfer.wav                              # auto-find & fill mutes
```

This is the classic **Janssen / autoregressive** method, and it's genuinely
good for short gaps — up to a few milliseconds. Ask it to invent a whole word and
it can't; the model only knows what the surrounding audio implies.

## The dense cousin — de-crackle

Vinyl has a special kind of damage: not the occasional loud *pop* (that's a job
for `declick`) but a constant **field of tiny crackles**, thousands of them, like
frying bacon under the music. `cathar decrackle` hunts for those little spikes —
each one a sample or two that jumps away from its neighbours — and smooths each
back into place, without touching the music around it:

```bash
cathar decrackle old-lp.wav --sensitivity 5
```

Between `declick` for the big pops, `decrackle` for the fine sizzle, and
`inpaint` for the outright holes, the whole spectrum of "missing or broken
samples" is covered.
