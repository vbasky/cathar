# Implementation spec — restoration depth & toolkit beyond #12

Companion to `ROADMAP.md`. Specs the algorithms added under **`0.7.x` restoration
track**, **Transform & analysis toolkit**, and **Spatial & measurement** — the
set that goes *beyond* community issue [#12](https://github.com/vbasky/cathar/issues/12)
(which covers dequantization, RIAA/elliptical, DSRE/HRAudioWizard upsampling,
reference rebalance, premium resampling — already tracked elsewhere).

Every item here is **inspectable DSP, deterministic, pure Rust** — consistent
with the project vision. No new black boxes; ML stays behind `--features ml`.

---

## Conventions every new stage follows

Verified against the existing tree (`digitize.rs`, `dequant.rs`, `restore.rs`,
`enhance.rs`, `cathar-cli/src/main.rs`):

1. **Module** — one file `crates/cathar/src/<name>.rs`, `//!` header doc.
   `#![deny(missing_docs)]` is on, so **every `pub` item needs a doc comment**.
2. **Signatures** — mono DSP is `pub fn foo(signal: &[f32], sample_rate: u32,
   …params) -> Vec<f32>`. Stereo/cross-channel work takes slices and returns a
   tuple `(Vec<f32>, Vec<f32>)` (see `elliptical_mono`) or an `&AudioData`.
   Guard degenerate input (empty / `n < window`) with an identity early-return —
   see `dequantize` (`n < 3`) and `declick` short-signal regression test.
3. **Wiring** — add `mod <name>;` and `pub use <name>::{…};` to `lib.rs`.
4. **CLI** — add a `Command` enum variant (clap) with `input`, `#[arg(short,
   long, default_value = "<name>.wav")] out`, and typed params with
   `default_value_t`. Dispatch arm: load via `AudioData::from_file`, apply with
   `audio.map_channels(|c| …)` for per-channel stages or explicit indexing for
   stereo, `to_file(&out)`, then an `eprintln!` one-line summary.
5. **FFT** — reuse `realfft::RealFftPlanner::<f32>` (already a workspace dep);
   STFT is currently rolled inline per module (Hann window + overlap-add). See
   "Shared STFT helper" below — land it first; the transform items depend on it.
6. **Tests** — colocated `#[cfg(test)]` with **synthetic signals asserting a
   physical property** (energy ratio, one-bin DFT magnitude via `mag_at`,
   zero-crossing rate), plus a bypass/identity test. Add a golden case in
   `crates/cathar/tests/golden.rs` for any command with fixed output bytes.

### Shared STFT helper (prerequisite — do this first)

Four of the toolkit items (HPSS, SMS, CQT display, masking-aware denoise) each
need a forward/inverse STFT. Today every module replans its own. Extract one:

```rust
// crates/cathar/src/stft.rs  (crate-internal, not pub-exported)
pub(crate) struct Stft { /* planner, window, hop, fft_size */ }
impl Stft {
    pub(crate) fn new(fft_size: usize, hop: usize) -> Self { … }        // Hann, 75% overlap default
    pub(crate) fn forward(&mut self, x: &[f32]) -> Vec<Vec<Complex<f32>>>; // frames × (fft/2+1)
    pub(crate) fn inverse(&mut self, frames: &[Vec<Complex<f32>>], len: usize) -> Vec<f32>; // WOLA-normalised
}
```

Migrate `denoise.rs` / `enhance.rs` / `spectrum.rs` onto it opportunistically
(pure refactor, guarded by their existing golden tests). Not a public API.

---

## Tier 1 — Restoration depth

### 1. Audio inpainting / gap interpolation — `inpaint.rs`

**Purpose.** Reconstruct dropouts, tape splices, digital mutes — arbitrary
*known-location* gaps, unlike transient `repair` (which detects its own).

**Method.** Autoregressive **Janssen / Godsill–Rayner** interpolation:
1. Take a context window straddling the gap (e.g. gap length × 4 on each side).
2. Estimate an order-`p` AR model (`p ≈ 3 × gap_len`, capped) from the *known*
   samples by solving the Yule–Walker normal equations (Levinson–Durbin).
3. Solve the linear system for the missing samples that minimise AR prediction
   error given fixed known neighbours (a symmetric banded solve).
4. Optionally iterate (re-estimate AR from the filled signal 2–3×) — the
   classic Janssen refinement.

**Public API.**
```rust
/// Reconstruct a known missing span `[start, start+len)` via AR (Janssen) interpolation.
pub fn inpaint_gap(signal: &[f32], start: usize, len: usize, iterations: u32) -> Vec<f32>;
/// Detect flat/NaN/silent gaps and fill each. Returns filled signal + spans filled.
pub fn inpaint_auto(signal: &[f32], sample_rate: u32, max_gap_ms: f32) -> Vec<f32>;
```
**CLI.** `cathar inpaint <in> [--out] [--start-ms] [--len-ms] [--iterations 3]`;
with no explicit span, auto-detect (flat runs / NaNs / hard-zero mutes).

**Deps.** None beyond `std` — small dense linear algebra written inline
(Levinson–Durbin + banded solve, ~120 lines).

**Tests.** Take a clean tone/chord, zero out a 20 ms span, reconstruct; assert
interior RMS error vs. truth < threshold and peak restored (mirror the
`declip_restores_clipped_sine` style). Bypass test: `len == 0` → unchanged.

**Effort.** M (1–2 days). The AR solve is the only real work.

---

### 2. Harmonic–percussive separation (HPSS) — `hpss.rs`

**Purpose.** Deterministic, no-weights split into tonal vs. transient layers.
Enables drum/vocal-ish isolation, and improves de-ess / de-click by routing.

**Method.** Fitzgerald (2010) median filtering on the STFT magnitude `S`:
- `H = median_filter(S, horizontal, k_t)` — sustained/tonal energy.
- `P = median_filter(S, vertical, k_f)` — broadband/transient energy.
- Soft (Wiener) masks: `M_h = H^2 / (H^2 + P^2)`, `M_p = 1 - M_h`.
- Apply masks to the complex STFT (phase preserved), inverse-STFT each.

**Public API.**
```rust
/// Split into (harmonic, percussive) via median-filter masks on the STFT.
pub fn hpss(signal: &[f32], sample_rate: u32, kernel: HpssKernel) -> (Vec<f32>, Vec<f32>);
```
**CLI.** `cathar hpss <in> [--out harmonic.wav] [--percussive percussive.wav]
[--kernel 17]` — write whichever outputs are requested.

**Deps.** Shared STFT helper. Median filter is a small sliding-window selection.

**Tests.** Sum of a sustained sine + a click train → assert the harmonic output
retains the sine's bin magnitude (`mag_at`) while the percussive output holds
most of the click energy, and vice-versa. Reconstruction test: `H + P ≈ x`.

**Effort.** M (1–2 days).

---

### 3. De-crackle — `restore.rs` (extend) or `decrackle.rs`

**Purpose.** Dense, low-amplitude vinyl surface crackle — the continuous field
`declick` (isolated impulses) leaves behind.

**Method.** Detection over a running noise-floor estimate:
1. High-pass / whitening filter to emphasise crackle against programme.
2. Per-sample detector: |residual| vs. an EMA-tracked local median-absolute-
   deviation floor → crackle mask (many short events).
3. Repair each flagged micro-gap with the same AR interpolation as item 1
   (shared `inpaint_gap`), or short cubic-Hermite for 1–2 sample events.

**Public API.**
```rust
/// Suppress dense low-level surface crackle. `sensitivity` ~1–10.
pub fn decrackle(signal: &[f32], sample_rate: u32, sensitivity: f32) -> Vec<f32>;
```
**CLI.** `cathar decrackle <in> [--out] [--sensitivity 5]`.

**Deps.** Reuses `inpaint_gap` (item 1) — sequence item 1 first.

**Tests.** Clean tone + additive sparse impulse noise (seeded xorshift like the
`denoise_coherent` test) → assert variance reduced while tone `mag_at`
preserved > 0.8×.

**Effort.** S–M (1 day on top of item 1).

---

### 4. Analog NR / pre-emphasis decode — `deemphasis.rs`

**Purpose.** Decode pre-encoded analog sources and standard pre-emphasis curves.
Completes the digitization story next to RIAA.

**Method.**
- **FM 50/75 µs & CD pre-emphasis** — single first-order shelving de-curve;
  reuse the `FirstOrder` bilinear section already in `digitize.rs` (lift it into
  a shared `iir.rs`). Pure, exact.
- **Dolby B/C, dbx** — sliding-band companding *decoders*: level-detect a
  side-chain, apply the inverse compression law per band (B = 1 band ~1.5 kHz+;
  C = 2 bands; dbx = broadband 2:1 with pre/de-emphasis). Deterministic given
  the standard time constants; document tolerances (these are approximations of
  the analog originals, flagged as such — no black box, just published curves).

**Public API.**
```rust
pub enum Emphasis { Fm50, Fm75, CdIec, }        // fixed-curve
pub fn deemphasis(signal: &[f32], sample_rate: u32, curve: Emphasis) -> Vec<f32>;
pub fn dolby_b_decode(signal: &[f32], sample_rate: u32) -> Vec<f32>;
pub fn dbx_decode(signal: &[f32], sample_rate: u32) -> Vec<f32>;
```
**CLI.** `cathar deemphasis <in> --curve fm75|fm50|cd|dolby-b|dolby-c|dbx`.

**Deps.** Shared `iir.rs` first-order section (refactor out of `digitize.rs`).

**Tests.** Fixed curves: apply matching pre-emphasis then decode → round-trips
to flat (RIAA-style `riaa_is_unity_at_1khz` pattern). Companders: assert a
swept-level tone's dynamic range expands toward the expected ratio.

**Effort.** M for fixed curves + dbx; Dolby B/C companders are M–L (get the time
constants and band split right). Ship fixed-curve + dbx first.

> **Constrained de-clip** from the earlier discussion is already satisfied —
> `declip` uses A-SPADE sparse reconstruction (see `declip_restores_clipped_sine`).
> No new work; noted here so it isn't re-opened.

---

## Tier 2 — Transform & analysis toolkit

### 5. Phase-vocoder + WSOLA time-stretch — `timestretch.rs`

**Purpose.** The engine under the roadmapped `speed`/`tempo`/`pitch`; decouples
duration from sample rate (rate change = time-stretch ∘ `resample`).

**Method.** Two backends, chosen by content/flag:
- **WSOLA** — overlap-add with cross-correlation-aligned grains; best for
  percussive/transient material, cheap, no FFT.
- **Phase vocoder** — STFT, scale hop on synthesis, propagate phase with
  identity phase-locking (Laroche–Dolson) to reduce smearing on tonal material.

**Public API.**
```rust
pub fn time_stretch(signal: &[f32], sample_rate: u32, ratio: f32, mode: StretchMode) -> Vec<f32>;
pub fn pitch_shift(signal: &[f32], sample_rate: u32, semitones: f32, mode: StretchMode) -> Vec<f32>;
```
`pitch_shift = resample(time_stretch(x, 2^(st/12)), …)`.

**CLI.** `cathar tempo <in> --ratio 1.2`, `cathar pitch <in> --semitones -2`,
`cathar speed <in> --factor 1.1` (speed = resample only, changes pitch).

**Deps.** Shared STFT helper + existing `resample`.

**Tests.** Stretch a tone by 1.5× → output length ×1.5, tone frequency unchanged
(zero-crossing rate, cf. `resample_preserves_tone_frequency`). Pitch +12
semitones → zero-crossing rate doubles, length preserved.

**Effort.** L (2–3 days for both backends + phase-locking).

---

### 6. Pitch detection (YIN / pYIN) — `pitch.rs`

**Purpose.** Monophonic f0; surfaced in `stats`, foundation for pitch correction.

**Method.** YIN: difference function → cumulative mean normalised difference →
absolute-threshold pick → parabolic interpolation of the trough. pYIN adds an
HMM-smoothed probabilistic pick (optional second pass).

**Public API.**
```rust
/// Per-frame f0 in Hz (0.0 = unvoiced) at the given hop.
pub fn detect_pitch(signal: &[f32], sample_rate: u32, hop: usize) -> Vec<f32>;
/// Single dominant f0 over the whole clip (median of voiced frames).
pub fn fundamental_hz(signal: &[f32], sample_rate: u32) -> Option<f32>;
```
**CLI.** Extend `stats` output with an f0 line; optional `cathar pitch-detect
<in> --hop 512` dumping a per-frame track.

**Deps.** None (time-domain autocorrelation-style).

**Tests.** Synthesised 220 Hz / 440 Hz tones → `fundamental_hz` within ±1 Hz;
silence → `None`/0.

**Effort.** S–M (1 day for YIN; +1 for pYIN HMM).

---

### 7. Constant-Q transform (CQT) — `cqt.rs`

**Purpose.** Log-frequency spectral analysis (musically aligned); a better axis
for the TUI `view` and downstream pitch/harmonic work.

**Method.** Geometrically-spaced bins (constant Q = f/Δf), each a windowed
complex filter; implement via the efficient kernel method (sparse spectral
kernel × FFT) for determinism and speed.

**Public API.**
```rust
pub struct CqtSpec { pub bins: usize, pub frames: usize, /* … */ }
pub fn cqt(signal: &[f32], sample_rate: u32, bins_per_octave: usize, f_min: f32) -> CqtSpec;
```
Mirror the existing `Spectrogram` accessor shape (`get(frame, bin)`, `bin_hz`).

**CLI.** `cathar view --cqt` flag (log axis) rather than a new command.

**Deps.** Shared STFT/FFT.

**Tests.** A tone at `f_min · 2^(k/bpo)` peaks in CQT bin `k` (cf.
`spectrogram_peaks_at_tone_frequency`).

**Effort.** M (1–2 days).

---

### 8. Sinusoidal / spectral modeling (SMS) — `sms.rs`

**Purpose.** Peak-tracked analysis-resynthesis: high-quality bandwidth
extension, harmonic transformation, principled `enhance` successor.

**Method.** McAulay–Quatieri / Serra SMS: per-frame spectral-peak picking →
partial tracking across frames (birth/death/continuation) → sinusoidal
resynthesis (+ optional stochastic residual = original − sines).

**Public API.**
```rust
pub struct SinusoidalModel { /* tracks: freq/amp/phase envelopes */ }
pub fn analyze_sms(signal: &[f32], sample_rate: u32) -> SinusoidalModel;
pub fn synthesize_sms(model: &SinusoidalModel, sample_rate: u32) -> Vec<f32>;
```
**CLI.** Initially library-only; later back an `enhance --method sms`.

**Deps.** Shared STFT; item 6 (peak/pitch) helps track seeding.

**Tests.** Analyse→resynthesise a 2-partial signal → recovers both partials'
`mag_at` within tolerance; residual energy small.

**Effort.** L (2–4 days — partial tracking is the fiddly part).

---

## Tier 3 — Spatial & measurement

### 9. Mid-side / stereo toolkit — `stereo.rs`

**Purpose.** Everyday stereo utilities; cheap, high-utility.

**Method.** Exact arithmetic / simple filters:
- M/S encode `M=(L+R)/2, S=(L−R)/2` and decode (exact inverse).
- Width: scale `S` by a factor.
- Haas widening: sub-ms inter-channel delay.
- Mono-maker: sum to mono below a cutoff (reuses `elliptical_mono`'s crossover
  approach — factor the shared low-band split).
- Mono→stereo upmix: decorrelate via an allpass/short delay on one leg.

**Public API.**
```rust
pub fn ms_encode(l: &[f32], r: &[f32]) -> (Vec<f32>, Vec<f32>);
pub fn ms_decode(m: &[f32], s: &[f32]) -> (Vec<f32>, Vec<f32>);
pub fn stereo_width(l: &[f32], r: &[f32], width: f32) -> (Vec<f32>, Vec<f32>);
pub fn mono_below(l: &[f32], r: &[f32], sample_rate: u32, cutoff_hz: f32) -> (Vec<f32>, Vec<f32>);
pub fn upmix_mono(mono: &[f32], sample_rate: u32) -> (Vec<f32>, Vec<f32>);
```
**CLI.** `cathar stereo <in> --width 1.4`, `--mono-below 120`, `--upmix`, `--ms`.

**Deps.** Shares low-band split with `digitize::elliptical_mono` (refactor).

**Tests.** M/S encode∘decode round-trips to identity; `width 0` collapses to
mono (mean |L−R|≈0, cf. `elliptical_collapses_lows_to_mono`).

**Effort.** S (≤1 day).

---

### 10. Measured-IR deconvolution / room correction — `deconv.rs`

**Purpose.** Remove a *known* room/mic response (supplied IR) — targeted
complement to the *blind* `dereverb`/WPE.

**Method.** Regularised inverse filtering: `Y = X · conj(H)/(|H|² + λ)` in the
STFT/frequency domain (Kirkeby–Nelson regularisation), where `H` is the FFT of
the supplied impulse response. λ tames division near IR nulls.

**Public API.**
```rust
/// Deconvolve `signal` by an impulse response `ir` with Tikhonov regularisation `lambda`.
pub fn deconvolve(signal: &[f32], ir: &[f32], sample_rate: u32, lambda: f32) -> Vec<f32>;
```
**CLI.** `cathar deconv <in> --ir sweep.wav [--lambda 0.01]` (IR loaded via
`AudioData::from_file`).

**Deps.** Shared FFT (block convolution / overlap-save).

**Tests.** Convolve a tone with a synthetic short IR, then deconvolve → recovers
the tone (interior RMS error small). Guard: empty IR → passthrough.

**Effort.** M (1–2 days; overlap-save block processing is the bulk).

---

### 11. Masking-aware (psychoacoustic) denoise — `denoise.rs` (extend)

**Purpose.** Make residual noise *inaudible* rather than merely low: hold the
suppression floor beneath the signal's masking threshold instead of a fixed gate.

**Method.** Add a masking-threshold stage to the existing spectral-subtraction
path: per-frame, spread the signal spectrum across critical bands (Bark),
compute a masking curve (simplified MPEG psychoacoustic model I), and set the
per-bin gain floor to keep residual noise under that curve.

**Public API.**
```rust
impl SpectralDenoiser {
    /// Denoise with a psychoacoustic masking floor instead of a flat `beta` floor.
    pub fn denoise_masked(&self, audio: &AudioData) -> Result<AudioData, Error>;
}
```
**CLI.** `cathar denoise <in> --perceptual` (new flag on the existing command).

**Deps.** Reuses `SpectralDenoiser`'s STFT path; adds a Bark-band mapping + mask.

**Tests.** Assert noise-power reduction ≥ the flat-floor path on a tone+noise
input while the tone `mag_at` is preserved at least as well (relative comparison
to `denoise`, mirroring `spectral_denoiser_reduces_noise_power`).

**Effort.** M–L (2–3 days; the masking model needs care and calibration).

---

## Suggested sequencing

Ordered by dependency and impact-per-effort:

1. **Shared `stft.rs` + `iir.rs` refactors** — unblock everything, no behaviour
   change (golden-test guarded).
2. **Audio inpainting** (item 1) — high restoration value, self-contained; also
   a dependency for de-crackle.
3. **Mid-side toolkit** (item 9) — quick win, broad utility.
4. **HPSS** (item 2) — deterministic, distinctive, feeds routing.
5. **De-crackle** (item 3) — builds on inpainting.
6. **Pitch detection** (item 6) → **CQT** (item 7) — analysis foundations.
7. **Time-stretch/pitch** (item 5) — closes a named SoX-parity gap.
8. **Analog NR decode** (item 4, fixed-curve + dbx first).
9. **Measured-IR deconvolution** (item 10).
10. **Masking-aware denoise** (item 11) and **SMS** (item 8) — deepest, last.

Each lands as its own minor release with unit tests + a golden case, per the
existing cadence. Restoration items (1–4) take priority over toolkit/spatial
when they compete, per the roadmap's Phase-1-first rule.
