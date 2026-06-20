# Harsh "S" sounds — de-essing

Some voices, some microphones, and a *lot* of close-up podcast and voiceover
recording produce a piercing, splashy hiss on every "s," "sh," "ch," and "t."
It's called **sibilance**, and once you notice it you can't un-notice it. Taming
it is **de-essing** — and it's a nice example of a tool that has to act *only at
certain moments*, not all the time.

## Why "s" sounds are special

Speech is mostly made down in the low and middle pitches — the body and warmth of
a voice. But the sibilant consonants are different: an "s" is essentially a
short burst of **high-pitched noise**, concentrated up near the top of the
spectrum (very roughly 4,000–10,000 cycles per second). On a spectrogram, every
"s" is a bright little cloud up high, separate from the vocal bands below.

That separation is the key. A de-esser is really just a **volume control that
only listens to the high end, and only turns down when that high end gets too
loud.** When you say a vowel, there's little energy up top, so the de-esser does
nothing. When you hit an "s," the high end spikes, the de-esser notices, and it
ducks *just that burst* by *just enough* — then lets go. The warmth of the voice
below is never touched.

Two controls run the show, and they're the same in every tool:

- A **crossover frequency** — the pitch above which the de-esser pays attention
  (cathar's `--freq`, default 4,000). Set it where the harsh "ss" lives.
- A **threshold** — how loud the high end has to get before the tool reacts. Too
  sensitive and it dulls every consonant; too lax and the worst "s" sounds still
  cut through.

## Going multiband and adaptive

There are two refinements that separate a crude de-esser from a good one, and
cathar offers both:

- **Multiband.** Sibilance isn't one pitch — a sharp "s" and a softer "sh" peak
  in different places up top. A *multiband* de-esser splits the high end into
  several sub-bands and watches each one independently, so it can duck the exact
  region that's offending without dulling the rest. (Cathar's `--bands 4` turns
  this on.)
- **Adaptive.** People get louder and quieter as they talk, so a *fixed*
  threshold is wrong half the time. An *adaptive* de-esser keeps a running sense
  of how loud each band normally is and reacts to sudden *jumps above its own
  recent average* — so it follows the speaker instead of needing constant
  babysitting.

## How the big tools do it

- Every DAW — **Logic, Pro Tools, Ableton, Cubase** — ships a de-esser plug-in,
  because sibilance is the single most common vocal-mixing problem. They all work
  on the crossover-plus-threshold principle above; the better ones are multiband.
- **FabFilter Pro-DS** and **Waves Sibilance** are the plug-ins mixing engineers
  reach for; Pro-DS in particular is prized for sounding transparent because it's
  cleverly adaptive and only touches the sibilant energy.
- **iZotope RX's** "De-ess" adds spectral precision — it can attenuate the
  offending high-frequency cloud *only where and when it occurs* on the
  spectrogram, which is gentler than turning down a whole band.

De-essing is a place where cathar's multiband, adaptive approach is genuinely
*competitive with the mainstream*, because the problem is well-bounded and
doesn't need a trained model — it needs to listen to the right pitches at the
right moments, which classical DSP does perfectly well.
