# Wow and flutter — when the pitch won't sit still

Play an old cassette or a warped record and listen to a held note — a piano
chord, a sustained vowel. It doesn't sit still. It drifts and wavers, sagging
flat and creeping sharp. That seasick wobble is **wow and flutter**, and it's the
signature flaw of anything with a spinning part.

- **Wow** is the slow drift — under a few times per second. A slightly eccentric
  record, a belt that's stretched, a motor that surges.
- **Flutter** is the fast wobble — tens of times per second. A pinch roller with a
  flat spot, a bearing with grit in it.

Both come from the same cause: the medium didn't move past the head at a
**constant speed**. And because speed and pitch are welded together on analog
gear (see the previous chapter), a speed wobble *is* a pitch wobble.

## The key insight: everything wobbles together

Here's what makes it fixable. A speed error doesn't shift one note — it scales
**every** frequency by the same amount at that instant. When the tape runs 1%
fast, the whole recording is 1% sharp: the bass, the voice, the cymbals, all of
it, together.

So if you can measure how the pitch of **any one steady thing** moves over time,
you've measured the speed error for the *entire* recording.

```text
   ideal:     a steady 440 Hz note ────────────────────────────
   wowed:     440 → 448 → 435 → 443 → …   (the note "breathes")
              └─ that breathing IS the speed curve ─┘
```

## How `cathar dewow` does it

```bash
cathar dewow warped-tape.wav
```

Cathar finds a strong, sustained tone in the recording and watches its
**instantaneous frequency** — literally, how fast its waveform is turning,
moment to moment. That traces out a *speed curve*: 1.0 where the tape ran true,
1.01 where it ran fast, 0.99 where it dragged.

Then it **time-warps** the audio to undo that curve — stretching the fast bits
back out and squeezing the slow bits back in — so the note stops breathing and
sits at a constant pitch. The overall length is preserved; only the wobble is
removed.

The catch: Cathar needs *something steady to lock onto*. On a solo piano note or a
sustained vocal it works beautifully. On a dense, constantly-changing mix with no
stable pitch, there's nothing to measure, and `dewow` sensibly leaves the audio
alone rather than guessing.

## A cousin problem: channels out of step

Tape has a second timing flaw. If the playback head is tilted a hair — its
**azimuth** is off — the left and right channels arrive **slightly out of step**,
smearing the stereo image and thinning the sound when you sum to mono.

`cathar azimuth` measures the tiny delay between the two channels (by sliding one
against the other until they line up best) and nudges the right channel back into
alignment — down to a fraction of a sample.

The same trick aligns *separate* recordings of the same moment — two mics, or a
reference track — with `cathar align --reference good-take.wav`. Line up the
takes first, and everything you do afterwards (mixing, comparing, noise
profiling) gets easier.
