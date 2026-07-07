# Broadcast and CD playback de-emphasis

Vinyl has the **RIAA** curve (chapter 15). FM radio and early compact discs use
*different* pre-emphasis schemes — treble boosted on record, treble cut on
playback — and if you digitize the **wrong side** of that bargain, the file
sounds dull (de-emphasis never applied) or harsh and thin (de-emphasis applied
twice).

## FM — 50 µs and 75 µs

FM broadcasters boost highs before transmission so hiss is less audible; your
receiver cuts them back on playback. Archives of **off-air FM** captures, or
files from tuners that output "flat" RF-discriminator audio, often still carry
that treble tilt.

The time constant names the curve:

- **50 µs** — common in Europe and much of the world.
- **75 µs** — common in the Americas.

```bash
cathar deemphasis fm_capture.wav --curve fm50 --out flat.wav
cathar deemphasis us_fm.wav --curve fm75 --out flat.wav
```

Listen after one pass. If it gets brighter instead of more natural, you may have
started from already-flat audio — don't de-emphasize twice.

## Early CD — 50/15 µs optional pre-emphasis

Most CDs you meet are **flat** — no pre-emphasis. A minority of early titles
(roughly the Red Book optional pre-emphasis flag era) were mastered with a
**50/15 µs** emphasis curve. If a CD rip sounds oddly dull or muffled compared
to a reference, this is one thing to check.

```bash
cathar deemphasis odd_cd.wav --curve cd --out flat.wav
```

## RIAA vs `deemphasis` vs `riaa`

| Curve | Typical source | Cathar command |
| --- | --- | --- |
| RIAA | Vinyl digitized without a phono stage | `riaa` |
| FM 50 / 75 µs | Off-air FM, some tuner captures | `deemphasis --curve fm50` / `fm75` |
| CD 50/15 µs | Rare pre-emphasized CD masters | `deemphasis --curve cd` |

**`riaa`** is vinyl-specific (and supports `--elliptical` for stereo rumble).
**`deemphasis`** is the broadcast/CD family. Pick the curve that matches how the
recording was *emphasized* before capture, then apply the matching playback
de-emphasis once.

## How the big tools do it

- **Audacity** and most DAWs expose generic EQ; you can dial an FM de-emphasis
  shelf by hand, but there's no one standard menu item.
- **Dedicated archival tools** sometimes bundle FM and tape NR curves alongside
  vinyl workflows.
- **Cathar** keeps each curve as a named, inspectable first-order filter — one
  command, one job, scriptable like everything else in the toolkit.