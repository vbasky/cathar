# How loud is loud? — loudness and LUFS

You've cleaned up the recording. Now you have to make it the *right loudness* to
publish — for a podcast, a video, a broadcast, a music stream. This sounds
trivial ("just turn it up") and is secretly one of the most misunderstood topics
in audio. Getting it right is **normalization**, and getting it right *the modern
way* means understanding a unit called **LUFS**.

## "Loud" is not "tall"

The naïve way to set level is to look at the **peak** — the single tallest sample
in the file — and turn everything up until that peak just touches the ceiling.
This is **peak normalization**, and it has a fatal flaw: it tells you nothing
about how loud something *sounds*.

A sudden snare hit and a sustained shout can have the *same* peak height, yet the
shout sounds far louder, because loudness is about *how much energy there is over
time*, not how tall one instant is.

```text
   a brief tick                a sustained tone
   (same PEAK height, but much quieter to the ear)

  +1 ┤   █                    ████████████████████
   0 ┼───█──────────          ████████████████████
  -1 ┤   █                    ████████████████████
       one tall spike          loud the whole time
       PEAK: maxed             PEAK: identical
       LOUDNESS: low      ◄──►  LOUDNESS: high
```

Peak-normalize a quiet, even podcast and a punchy one to the same peak and the
punchy one will sound much louder. That's why,
for decades, some adverts felt like they were screaming at you between TV shows:
everyone was peak-normalizing and then squashing their audio to be as dense as
possible.

## LUFS: measuring perceived loudness

The fix was an international standard (its name is **ITU-R BS.1770**, adopted for
broadcast as **EBU R128**) that measures loudness the way *ears* experience it,
not the way a ruler does. The unit is the **LUFS** — "Loudness Units relative to
Full Scale." Bigger negative number = quieter. Three ideas make it match
hearing:

1. **It averages energy over time**, so a sustained sound reads louder than a
   brief spike of the same height — exactly as you hear it.
2. **It weights pitches like your ear does.** Your hearing is most sensitive in
   the upper-mid range and less so at the extremes, so the meter gives those
   mid-high pitches more say. (This pitch-weighting is called *K-weighting*.)
3. **It ignores the silences.** Long gaps shouldn't drag the average down, so the
   measurement "gates out" the quiet bits and only averages the parts that are
   actually playing.

The upshot: two pieces of audio at the same **LUFS** *sound* equally loud, even
if one is a whisper-and-shout drama and the other a steady narrator. That's why
the whole delivery world now specifies loudness in LUFS. Common targets:

| Where it's going | Target |
| --- | --- |
| Broadcast TV / radio (EBU R128) | **−23 LUFS** |
| Podcasts (Apple/Spotify spoken) | **−16 LUFS** |
| Music streaming (Spotify, YouTube) | **−14 LUFS** |

Cathar measures true, gated, K-weighted LUFS and turns the whole file up or down
by one amount to hit your target: `normalize --target -16`.

## The true-peak safety net

There's one last trap. When digital audio is turned back into sound, the player
draws a smooth curve *through* the samples — and that curve can briefly poke
*higher* than any actual sample, between the dots. These hidden overshoots are
**true peaks** ("inter-sample peaks"), and if they cross the ceiling they cause
nasty distortion on some devices even though no stored sample looked too loud.

So a proper loudness normalizer doesn't just hit the LUFS target — it also keeps
the *true* peak under a safe ceiling (commonly −1 dBTP). Cathar holds the gain
back if pushing for the loudness target would breach that ceiling
(`--true-peak -1`), trading a hair of loudness for a guarantee it never clips on
playback.

## How the big tools do it

- Every broadcast and streaming workflow on earth now runs on LUFS — it's the
  law for TV in much of the world. **Loudness meters** are everywhere: the free
  **Youlean Loudness Meter**, **Waves WLM**, **Nugen VisLM**, and the meters
  built into **Pro Tools, Logic, and Audition**.
- **iZotope RX** and **Ozone** include a "Loudness" module that does exactly what
  cathar does — measure integrated LUFS, normalize to a target, respect a
  true-peak ceiling — with presets for every platform.
- **FFmpeg's** `loudnorm` filter is the command-line workhorse the whole web uses
  for batch-normalizing video and podcast audio; it implements the same BS.1770
  standard.

This is a corner where cathar is doing the *exact same standardized maths* as the
professional tools — there's no ML and no secret sauce in loudness, just a
well-defined international measurement. If cathar says −16 LUFS, it means the same
−16 that RX, FFmpeg, and a broadcast meter mean.
