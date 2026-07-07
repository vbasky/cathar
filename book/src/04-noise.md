# Hiss and noise — denoising

The steady *shhhhh* behind a recording — tape hiss, a noisy microphone preamp, an
air-conditioner, the electrical fuzz of a cheap interface — is the most common
complaint in all of audio. Removing it is called **denoising**, and it's the
clearest example of the "analyse → modify → resynthesise" loop from chapter 2.

## The core idea: subtract the haze

Recall that on a spectrogram, hiss looks like a faint, even haze sitting under
everything, across many pitches at once, *all the time*. The voice, by contrast,
is bright bands that come and go.

So denoising asks a simple question, pitch by pitch: **"how much faint, ever-
present haze is at this pitch?"** That amount is the **noise profile** — a
measurement of the hiss's recipe. Once you know it, you go through the sound and,
for each pitch in each moment, **subtract the haze amount.** Where the voice is
loud, subtracting a little haze barely changes it. Where there's only haze (the
silences between words), subtracting the haze amount leaves… almost nothing.
Silence. That's the trick, and it has a name: **spectral subtraction.**

Two questions remain: *how do you measure the haze*, and *how hard do you
subtract*.

### Measuring the haze (the noise profile)

There are two ways, and cathar offers both:

- **Learn it from silence.** If your recording has a moment of "room tone" — a
  patch with no voice, just the background — you can point the tool at it and say
  "*this* is the noise; memorise its recipe." Cathar calls this a **noiseprint**.
  It's by far the more accurate method, because you're showing the tool a clean
  example of exactly what to remove.
- **Guess it automatically.** If there's no clean silence, the tool assumes the
  *quietest* moments at each pitch are mostly haze, and builds the profile from
  those. Cathar does this with a method called *minimum statistics*. It's
  convenient and needs no setup, but it's a guess, so it's a little gentler and
  less surgical than a real noiseprint.

> **In practice:** if you can spare even half a second of "just the room," learn
> a noiseprint from it. It is the single biggest quality lever in denoising, in
> *any* program.

### How hard to subtract (the aggressiveness knob)

Subtract too little and hiss remains. Subtract too much and you start eating into
the voice — and you create a very recognisable artefact: a twinkly, watery,
"underwater" or "musical noise" sound. (It happens because subtraction can leave
isolated little flecks of pitch that warble.) So every denoiser has an
**aggressiveness** control. In cathar it's `--alpha` (gentle around 1.5, strong
around 4–6) plus a *floor* (`--beta`) that refuses to ever fully silence a pitch,
which keeps the result natural instead of glassy.

The whole game of denoising is this trade-off: **hiss versus artefacts.** There
is no setting that removes all hiss and adds nothing; the skill is finding the
spot where what's left is less distracting than what you've added.

## A gentler cousin: the Wiener filter

Instead of bluntly subtracting the haze, you can *scale each pitch* by how likely
it is to be real sound versus noise: pitches that tower over the haze are kept
almost fully, pitches barely above it are turned right down. This is the **Wiener
filter** (cathar's `--wiener` option). On steady, gentle hiss it often sounds
smoother and less twinkly than plain subtraction. Same goal, slightly different
maths for the "modify" step.

## How the big tools do it

This is mature, well-understood territory, and the *concepts* are identical
everywhere:

- **Audacity** (free) has "Noise Reduction," which works exactly like cathar's
  noiseprint method: you select a quiet bit, click "Get Noise Profile," then
  apply. Same idea, same trade-off knobs.
- **Adobe Audition** offers both a learned-profile "Noise Reduction" and an
  adaptive "DeNoise" that guesses, like cathar's two modes.
- **iZotope RX** is where the money shows. Its "Spectral De-noise" does classic
  profile-based subtraction *very* well, but its flagship "Voice De-noise" and
  the newer AI-powered modes use **machine learning** — models trained on
  thousands of hours of speech — to tell voice from noise far more cleverly than
  any "subtract the haze" rule. That's the real frontier: the *concept* is the
  same, but a trained model makes a smarter decision in the **modify** step,
  pulling clean speech out of noise that classical subtraction would smear.

Cathar sits firmly in the classical camp: transparent, predictable, no weights,
genuinely good on steady hiss — and honestly outclassed by RX's ML on the hardest
cases (heavy, non-steady background noise like a busy café). Knowing *which*
problem you have tells you which tool you need.

## Optional learned denoise (`ml-denoise`)

If you build cathar with the optional **`ml`** feature, **`ml-denoise`** adds a
third path: a small recurrent network that predicts a **per-pitch gain mask** from
the log-magnitude spectrum — the same broad idea as DNS-Challenge / DeepFilterNet
style speech denoisers, but with open weights you can inspect and replace. It
still works in the frequency view (phase preserved, overlap-add out), so the
story from this chapter still applies; only the **modify** step is learned instead
of rule-based.

```bash
cathar ml-denoise noisy.wav --out cleaner.wav
```

A bundled checkpoint ships for light broadband cleanup; for serious speech work
you'd retrain on data like the DNS Challenge. Classical `denoise` remains the
default transparent path — `ml-denoise` is explicitly opt-in for when subtraction
and Wiener aren't enough.
