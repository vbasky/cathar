# The two ways to look at sound

This is the most important chapter in the book. Once it clicks, almost every
tool in every audio program will suddenly make sense.

## The problem with the waveform

The waveform — that wiggly line of heights — tells you *when* things are loud,
but not *what* they are. A hiss, a hum, a voice and a cymbal are all stirred into
the same line. Trying to remove the hiss by editing the waveform is like trying
to remove the salt from a soup with a fork.

What you really want is to separate the sound by **pitch**: put all the low
rumble in one pile, the mid-range voice in another, the high hiss in a third.
Then you could lower the hiss pile without touching the voice pile. That second
view exists, and it's called the **frequency view**, or the **spectrum**.

## Splitting sound into pure tones

Here's the deep idea, discovered by a mathematician named Fourier two centuries
ago: **any sound, however complicated, can be rebuilt by adding together a bunch
of simple, pure tones** — like the steady note of a tuning fork — each at its own
pitch and its own loudness.

So a voice isn't one thing; it's a recipe: "a little bit of this low tone, a lot
of this mid tone, a touch of that high tone…" Hiss is its own recipe: "a tiny,
even sprinkle of *every* high tone at once." A 60-cycle hum is the simplest
recipe of all: "one specific low tone, and nothing else."

The machine that takes a chunk of sound and reads off its recipe — how much of
each pitch is present — is the **Fourier transform**, and the fast version every
program uses is the **FFT** (Fast Fourier Transform). You will see "FFT" in the
settings of every serious audio tool. Now you know what it means: *split this
sound into its ingredient pitches.*

## The spectrogram: the picture you'll actually see

A single FFT reads the recipe of *one short moment*. But sounds change — a voice
moves from word to word. So tools chop the audio into many short, overlapping
slices (a few hundredths of a second each), take the FFT of every slice, and
stack the results side by side. This sliding-window approach has a name —
the **Short-Time Fourier Transform, or STFT** — and its picture is the
**spectrogram**.

A spectrogram is a heat-map of sound: time runs left-to-right, pitch runs
bottom (low) to top (high), and brightness shows how much of each pitch is
present at each moment. On a spectrogram:

- A **hum** is a steady horizontal line low down — one pitch, always there.
- A **voice** is a shifting stack of bands in the middle that wobble as words
  change.
- **Hiss** is a faint, even haze across the *entire* top.
- A **click** is a thin vertical streak — a single instant where *every* pitch
  flares at once.

Suddenly the soup is unstirred. The hiss, the hum, the voice and the click are
in visibly different places. *That* is why nearly every restoration tool works in
the frequency view: in the spectrogram, the problem and the wanted sound usually
sit in different spots, so you can lower one without harming the other.

## The loop almost everything uses

Most of cathar's "reduce" tools, and the equivalent tools in every professional
program, run the same three-step loop, over and over, on each short slice:

1. **Analyse** — FFT the slice into its recipe of pitches.
2. **Modify** — turn down (or rebuild) the parts you don't want.
3. **Resynthesise** — add the pitches back together into a cleaned slice, and
   glue the slices back into a waveform (a careful blend called *overlap-add*).

Cathar uses a 2,048-sample slice with a 75% overlap between neighbours and a
gentle taper (a *Hann window*) so the joins are seamless. Those exact numbers
don't matter to you; the *shape* of the idea does. Analyse → modify →
resynthesise. Hold onto it. Every "de-noise / de-hum / de-reverb / de-ess" tool
in this book is a different idea for that middle **modify** step. The rest is
plumbing.

## How the big tools do it

Every audio program you've heard of lives in this same two-view world. The
spectrogram in iZotope **RX** — the industry-standard restoration suite — is the
centrepiece of its interface; you literally paint on it to fix problems. Adobe
**Audition** has a "Spectral Frequency Display" that's the same idea. Free
**Audacity** shows a spectrogram view too. They all rely on the FFT/STFT loop
above. The differences between cheap and expensive tools are almost never about
*this* foundation — they're about how cleverly the **modify** step decides what's
noise and what's signal, which is exactly what the next chapters are about.
