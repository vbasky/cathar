# What digital sound actually is

## Sound is wiggling air

When something makes a sound — a voice, a guitar string, a slammed door — it
pushes the air next to it, which pushes the air next to *that*, and so on, until
a little wave of pressure reaches your eardrum and wiggles it. Your brain reads
that wiggle as sound. That's all sound is: **changing air pressure over time.**

If you could draw the pressure at your ear from one moment to the next, you'd get
a wavy line: up when the air is squeezed, down when it's thinned out. That wavy
line is called a **waveform**, and it's the single most important picture in this
whole book. Loud sounds make tall wiggles; quiet sounds make small ones. Fast
wiggles are high-pitched; slow wiggles are low-pitched.

## Turning the wiggle into numbers

A computer can't store a smooth wiggly line directly. Instead it does something
clever and slightly brutal: many thousands of times per second, it measures how
high the wave is *right now* and writes that height down as a number. Then it
throws away everything in between.

Each measurement is called a **sample**. Think of it like a flipbook: a cartoon
isn't really moving, it's just a stack of still drawings shown fast enough to
fool your eye. Digital audio is the same trick for sound — a stack of still
"heights," played back fast enough to fool your ear.

Two numbers describe how finely the computer captured the sound:

- **Sample rate** — how many measurements per second. CD audio uses **44,100**
  per second (written *44.1 kHz*); video and pro audio often use **48,000**. The
  more samples per second, the higher the pitches you can capture. (There's a
  famous rule: to capture a pitch, you need at least twice as many samples per
  second as the pitch's frequency. We'll meet it again in the resampling
  chapter.)
- **Bit depth** — how finely each single measurement is written down: 16 bits
  per sample for CDs, 24 bits for studios. More bits means a quieter "noise
  floor" — the faint background fuzz that any digital measurement carries.

Inside cathar (and most modern tools), every sample is stored as a **floating-
point number between −1.0 and +1.0**. −1.0 is the lowest the wave can go, +1.0
the highest, and 0.0 is silence (no pressure change). A whole second of mono CD
audio is therefore just a list of 44,100 such numbers. A stereo recording is two
such lists, one for the left ear and one for the right.

## Why this matters for cleaning up sound

Every restoration tool in this book is, underneath, just **arithmetic on that
list of numbers.** Removing hiss means nudging the numbers; removing a click
means replacing a few of them; making something louder means multiplying them
all. There is nothing else in the file. When cathar "denoises an interview," it
reads the list, does sums on it, and writes a new list. The art is entirely in
*which* sums, and *why*.

There is a catch, though, and it sends us straight to the next chapter. Looking
at the raw list of heights — the waveform — is a great way to see *how loud*
something is from moment to moment, but a terrible way to see *what's in it*. A
hiss and a voice and a hum are all jumbled together in the same wiggly line, like
three colours of paint stirred into one bucket. To pull them apart, we need a
second way of looking at sound.
