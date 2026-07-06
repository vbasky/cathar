# Dequantization — grain from low bit depth

In chapter 1 we met **bit depth** — how finely each sample is written down. A
16-bit file has about 65,000 possible levels; an 8-bit file only 256. When audio
is stored with too few levels, you do not always hear a clean tone plus silence;
you hear a faint **grain** or **zipper noise**, especially on quiet passages or
after heavy editing. **Dequantization** tries to relax that grain without
pretending the lost information magically returns.

## Where the grain comes from

Quantization happens whenever the continuous wave is **rounded to the nearest
allowed step**:

- Old digital recorders (8-bit, 12-bit).
- Exporting to a lower bit depth without dither.
- Lossy codecs that leave a "stair-step" residue on decoded audio.
- Chains that repeatedly round the same file.

On a waveform, quiet music on a coarse grid looks like a signal **stuck on tiny
stairs** instead of a smooth curve. On a spectrogram it can show up as a brittle,
buzzy haze.

```text
  fine grid (24-bit):     smooth curve  ∿∿∿∿∿∿
  coarse grid (8-bit):    stair-steps   ┌┐┌┐┌┐┌┐
```

## What cathar does today

`cathar dequantize` assumes a source **bit depth** (`--bits`, default 16) and a
**strength** from 0 to 1. It uses **neighbour prediction**: each sample is nudged
toward what its neighbours suggest, but only within one quantisation step of its
current lattice position. The result is inspectable DSP — no neural model, no
hidden weights.

```bash
cathar dequantize grainy.wav --bits 16 --strength 0.7 --out smoother.wav
```

Strength `0` is a bypass. Higher strength corrects more aggressively; if it starts
to sound soft or wobbly, back off.

This is a **foundation**, not the last word. Research tools (e.g. Záviška et
al., co-sparse methods for audio dequantization) go much further with iterative
sparse recovery. Cathar may adopt those methods later; today's command is the
deterministic first step.

## Dequantization vs other tools

| Tool | Problem | Approach |
| --- | --- | --- |
| **dither** (cathar `dither`) | Prevents *new* grain when reducing bit depth | Add tiny noise *before* rounding |
| **dequantize** | Reduces *existing* grain on already-quantized audio | Neighbour-guided relaxation on the lattice |
| **denoise** | Steady or random hiss | Spectral subtraction / learned masks |
| **enhance** | Missing high frequencies | Band replication or spectral extrapolation |

If the file is genuinely noisy (room tone, hiss), try `denoise` first. If it
sounds **gritty on quiet lines** but not broadly hissy, `dequantize` is the better
fit.

## Honest limits

Dequantization cannot recover information that was never stored. A 8-bit recording
will not become a 24-bit studio master. The goal is to make the grain **less
annoying**, not to invent new detail. For the worst cases, re-capture or find a
higher-quality source if one exists.

For delivery, prefer **24-bit FLAC** or **32-bit float WAV** after cleanup so you
do not re-introduce grain at export.