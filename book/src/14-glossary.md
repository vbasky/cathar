# Glossary in plain language

Every term you met in this book, defined the way a friend would explain it.

**Aliasing** — The ghostly, gritty wrong-pitch tones you get when audio is
converted to a lower sample rate without first removing the pitches that are too
high for the new rate to hold. The audio version of wagon wheels spinning
backwards in films.

**Bit depth** — How finely each single sample is written down (16-bit for CDs,
24-bit for studios). More bits = a quieter background fuzz floor.

**Clipping** — Distortion caused by a signal trying to go louder than the maximum
the system can store, so its peaks get chopped flat. Sounds harsh and "broken."

**DAW (Digital Audio Workstation)** — A full audio-editing program like Pro
Tools, Logic, Ableton, Reaper, or Audition. The "Photoshop of sound."

**de- (prefix)** — Just means "remove": *de*-noise, *de*-hum, *de*-click.

**DSP (Digital Signal Processing)** — The umbrella term for doing maths on
digital audio (or any signal) to change it: filtering, denoising, all of it.

**EBU R128** — The European broadcast loudness standard built on BS.1770; the
reason broadcast audio targets −23 LUFS.

**FFT (Fast Fourier Transform)** — The fast machine that takes a chunk of sound
and reads off its "recipe" of pitches. The workhorse behind the frequency view.

**Filter** — A tool that turns some pitches up or down. A *high-pass* filter
keeps the highs and blocks the lows; a *low-pass* does the reverse; a *notch*
removes one narrow band.

**Frequency** — How fast the wave wiggles; what you hear as pitch. Measured in
cycles per second, or **hertz (Hz)**. 1,000 Hz = 1 kHz.

**Harmonics** — Faint copies of a tone at exact whole-number multiples of its
pitch. Why hum "buzzes" instead of being a pure tone.

**Hum** — Low, steady tone leaking in from the electrical mains (50 or 60 cycles
per second, plus harmonics).

**LUFS** — "Loudness Units relative to Full Scale." The modern unit for *perceived*
loudness — two files at the same LUFS sound equally loud. Targets: −23 broadcast,
−16 podcast, −14 streaming.

**Mono** — A single channel of audio; plays equally from both speakers.

**Noise / hiss** — Steady, random background energy spread across the high
frequencies — the *shhhh* behind a recording.

**Noiseprint / noise profile** — A measurement of the recipe of a recording's
background noise, learned from a quiet patch, so a denoiser knows exactly what to
subtract.

**Normalization** — Setting a recording to a target level. *Peak* normalization
aims at the tallest sample (crude); *loudness* (LUFS) normalization aims at how
loud it actually sounds (correct for delivery).

**Nyquist frequency** — The highest pitch a given sample rate can hold: exactly
half the sample rate. Go above it and you get aliasing.

**Overlap-add** — The careful blending technique that glues the processed short
slices of audio back into one seamless waveform.

**Phase coherence** — Keeping a stereo file's two channels "agreeing" when you
process them, so the stereo image stays stable instead of wandering.

**Plosive** — The low thump on "p" and "b" sounds when a puff of breath hits the
mic.

**Resampling** — Converting audio from one sample rate to another (e.g. 48,000 →
44,100). Done well, it's a smart filter, not a copy.

**Reverb** — The trail of fading echoes a room adds as sound bounces off its
surfaces. Makes recordings sound "roomy" or "boxy."

**Rustle** — Scratchy mid-range noise from clothing brushing a clip-on (lavalier)
microphone.

**Sample** — One single measurement of the wave's height. Audio is a long list of
these.

**Sample rate** — How many samples are taken per second (44,100 for CD, 48,000
for video/pro). Higher = can capture higher pitches.

**Sibilance** — Over-loud, piercing "s," "sh," and "ch" sounds; removed by
*de-essing*.

**Spectral subtraction** — The core denoising method: measure the background haze
at each pitch and subtract that amount.

**Spectrogram** — A heat-map picture of sound: time left-to-right, pitch
bottom-to-top, brightness = how much of each pitch is present. Where most
restoration tools "see."

**Stereo** — Two channels (left and right) whose *difference* creates a sense of
width and placement.

**STFT (Short-Time Fourier Transform)** — Taking an FFT of many short, overlapping
slices in a row, to track how a sound's pitches change over time. The engine
behind the spectrogram.

**Threshold** — A "how much counts" cutoff: how loud a spike must be to count as a
click, or how loud sibilance must get before a de-esser reacts.

**True peak (inter-sample peak)** — A hidden overshoot in the smooth curve drawn
*between* samples on playback, which can distort even when no stored sample looked
too loud. Why loudness tools keep a true-peak safety ceiling (e.g. −1 dBTP).

**Waveform** — The wiggly line of the wave's height over time. Great for seeing
*how loud*, poor for seeing *what's in it*.

**Wiener filter** — A gentler denoising method: instead of subtracting the haze,
scale each pitch by how likely it is to be real sound versus noise.

**Window (Hann window)** — The gentle taper applied to each short slice of audio
before its FFT, so the slices blend together without clicks at the seams.
