# Vinyl digitization — RIAA and elliptical mono

If you digitize a vinyl record with a USB turntable or a phono preamp into your
computer, the file you get is **not** flat, neutral audio. The groove was cut with
a deliberate tilt baked in — the **RIAA curve** — and the low end of a stereo LP
often carries rumble that is **out of phase** between the left and right
channels. Cathar's `riaa` command fixes the first problem; optional **elliptical
mono** helps with the second.

## Why vinyl sounds wrong without RIAA

Vinyl has a physical problem: low frequencies need **wide groove wiggles**, which
eat up space and limit playing time. The recording industry agreed on a trick:
**boost the bass and cut the treble when cutting the master**, so the groove stays
narrow. Every home turntable and phono preamp is supposed to do the **opposite on
playback** — cut the bass back down and restore the highs — so what you hear is
flat again.

That playback correction is **RIAA de-emphasis**. If you skip it (or your capture
chain already applied it and you apply it *again*), the result is obviously
wrong: boomy bass, dull highs.

```text
  what was cut into the groove (pre-emphasis):   bass UP, treble DOWN
  what playback must do (de-emphasis):           bass DOWN, treble UP  ← cathar riaa
  what you want at the end:                      flat, natural balance
```

`cathar riaa recording.wav --out flat.wav` applies the standard playback curve.
Cathar normalises so **1 kHz stays at reference level** — the usual anchor point
for RIAA.

> **Already corrected?** If your chain includes a phono stage that already applies
> RIAA, running `riaa` again will over-correct. Listen first: if it already sounds
> balanced, skip this step.

## Elliptical mono — taming stereo rumble

On stereo LPs, very low frequencies (rumble, warp, groove noise) are often **mostly
out of phase** between left and right — they pull the stylus sideways without
carrying much musical stereo information. **Elliptical mono** sums only the **low
band** to mono while leaving everything above a crossover frequency in full
stereo.

```text
  below ~200 Hz:   L and R lows → one shared mono low  (rumble stops fighting the image)
  above crossover: untouched stereo
```

For stereo files:

```bash
cathar riaa stereo_capture.wav --out flat.wav --elliptical 200
```

Mono files get RIAA only; elliptical needs two channels.

## Where this sits in a vinyl chain

A typical cathar workflow after capture:

1. **`riaa`** — correct the playback curve (and optionally `--elliptical`).
2. **`dewow`** — speed drift / pitch wobble if the turntable or belt isn't steady
   (chapter 18).
3. **`azimuth`** — align the right channel to the left if the stereo image sounds
   thin or smeared (chapter 18).
4. **`declick`** — loud impulse pops and dust ticks (chapter 6).
5. **`decrackle`** — dense surface crackle between the big pops (chapter 19).
6. **`denoise` / `noiseprint`** — surface hiss (chapter 4).
7. **`dehum`** — mains buzz if the turntable motor leaks 50/60 Hz (chapter 5).
8. **`normalize`** — delivery loudness (chapter 10).

For outright drop-outs or mutes on a transfer, **`inpaint`** can fill short gaps
(chapter 19).

## How the big tools do it

- **Audacity** has no built-in RIAA; users rely on the turntable's phono stage or
  third-party EQ curves.
- **iZotope RX** and dedicated vinyl tools (e.g. open **Vinyl Restoration Suite**)
  bundle RIAA, declick, denoise, and sometimes wow/flutter in one GUI.
- **Cathar** keeps each stage separate and inspectable — one command per
  transform, scriptable and deterministic.

For archival work, RIAA is not optional maths; it's the step that turns "raw
groove data" into "music that sounds like the record you remember."