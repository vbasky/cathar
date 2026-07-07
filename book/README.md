# Cleaning Up Sound

### Audio restoration from first principles — for people who have never touched a DAW or DSP

You have a recording that sounds bad — hiss, hum, crackle, harsh "s" sounds — and
you'd like to understand how software fixes it, without going back to school.
This short book explains every idea behind audio cleanup in plain language, with
pictures and everyday analogies and **no maths to follow**. It's built around the
open-source toolkit [**cathar**](https://github.com/vbasky/cathar), but the ideas
are the same ones inside iZotope RX, Adobe Audition, Audacity, SoX, and FFmpeg —
and each chapter ends by comparing them honestly.

> **Read it rendered:** once GitHub Pages is enabled for this repo, the book is
> published as a browsable site at **<https://vbasky.github.io/cathar/>**. Or
> build it yourself: `cargo install mdbook mdbook-mermaid && mdbook serve book`.

## Contents

1. [What digital sound actually is](src/01-digital-sound.md) — samples, the waveform, the flipbook trick
2. [The two ways to look at sound](src/02-two-views.md) — the FFT, the spectrogram, and the one idea that unlocks everything
3. [The repair toolbox](src/03-the-toolbox.md) — what breaks, and *reducers* vs *repairers*
4. [Hiss and noise — denoising](src/04-noise.md)
5. [Hum — getting rid of the buzz](src/05-hum.md)
6. [Clicks and clipping — repairing damaged samples](src/06-clicks-clipping.md)
7. [Rooms and reverb](src/07-reverb.md)
8. [Harsh "S" sounds — de-essing](src/08-sibilance.md)
9. [Wind, pops, and rustle](src/09-wind-pops-rustle.md)
10. [How loud is loud? — loudness and LUFS](src/10-loudness.md)
11. [Sample rate and resampling](src/11-resampling.md)
12. [Stereo, channels, and phase](src/12-stereo.md)
13. [How cathar compares to the big tools](src/13-vs-industry.md)
14. [Vinyl digitization — RIAA and elliptical mono](src/15-vinyl-digitization.md)
15. [Dequantization — grain from low bit depth](src/16-dequantization.md)
16. [Time and pitch — stretching without the chipmunk effect](src/17-time-and-pitch.md)
17. [Wow and flutter — when the pitch won't sit still](src/18-wow-and-flutter.md)
18. [Separation, modeling, and gap-filling](src/19-separation-and-modeling.md)
19. [Broadcast and CD playback de-emphasis](src/20-playback-deemphasis.md)
20. [Glossary in plain language](src/14-glossary.md)

New here? **[Start with the preface →](src/preface.md)**
