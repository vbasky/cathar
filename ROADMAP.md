# Roadmap

## Vision

Cathar starts as an **audio-restoration toolkit** and grows, before `1.0`, into a
**general-purpose audio swiss-army knife** — the [SoX](https://sourceforge.net/projects/sox/)
of the Rust era: one CLI (and one library) that decodes anything, applies any
common transform or effect, and encodes to any common format.

What stays constant as the scope widens:

- **Pure Rust.** No ffmpeg, no C/C++ FFI, no `pkg-config` in the default build.
  Codecs or models that can only be reached through C bindings live behind
  explicit opt-in features — never on the default path.
- **No black boxes.** Every stage is inspectable DSP or an open, named model.
- **Deterministic per-file output.** Same input, same flags, same bytes out —
  protected by golden-file tests.
- **Restoration-first.** Breadth never comes at the cost of the restoration
  chain that is Cathar's reason to exist.

## Where we are — `0.6.x`

The restoration chain is implemented and unit-tested: `denoise` (spectral
subtraction / Wiener, noiseprint- or minimum-statistics-driven), `dehum`,
`dereverb`, `voiceisolate`, `deesser`, `breath`, `declick`, `declip` (A-SPADE
sparse reconstruction), `enhance` (bandwidth extension), `riaa` (vinyl
de-emphasis + elliptical mono), `dequantize`, `normalize` (true EBU R128),
`resample`, plus the `wave` generator and parallel `batch`. Decode is via
Symphonia (MP4/M4A/MKV/MP3/FLAC/WAV/OGG); **encode is WAV (32-bit float), FLAC
(24-bit lossless), or AIFF (24-bit), chosen by the output extension.**

The **Foundations** milestone is complete — it shipped as three releases,
`v0.2.0` (loudness), `v0.3.0` (resampling), `v0.4.0` (encode). Each roadmap
milestone item tends to land as its own minor release, so the version pins below
are indicative ordering, not exact promises (see the closing note).

---

## Phase 1 — Restoration, finished and credible (`0.2`–`0.6`)

Close the gaps the docs already admit, then deepen the restoration set until it
stands next to iZotope RX's core.

### Foundations — shipped (`v0.2.0`–`v0.4.0`)

- ✅ **True EBU R128 loudness** (`v0.2.0`) — K-weighting + gated integrated LUFS
  (ITU-R BS.1770-4), measured jointly across channels, with a 4×-oversampled
  true-peak (dBTP) ceiling. Replaces the RMS approximation in `normalize`.
- ✅ **Main-path resampling** (`v0.3.0`) — a shared Kaiser-windowed sinc
  resampler (anti-aliased, arbitrary ratio) exposed as `AudioData::resample` and
  the `resample` command; `enhance` now uses it too, so mixed-rate inputs can be
  brought to a common rate by any stage.
- ✅ **Encode beyond WAV** (`v0.4.0`) — 24-bit lossless FLAC (pure-Rust
  `flacenc`) and 24-bit AIFF, selected by the output extension. The first brick
  of the swiss-army goal: real format conversion. (Codecs needing C bindings —
  Opus, AAC — will live behind an opt-in `codecs` feature when added.)

### `0.5` — DSP depth

- ✅ **Spectral repair** (`v0.5.0`) — the `repair` command / `spectral_repair`:
  per time-frequency bin, transient outliers (vs the temporal median) are pulled
  back to the median with phase preserved, so brief whistles/bursts/glitches are
  painted out while sustained content passes through untouched.
- ✅ **De-wind, de-plosive, de-rustle** (`v0.5.1`) — round out the `de-*` family:
  a 4th-order high-pass for wind rumble, and band-limited transient suppression
  (low band for plosive pops, mid band for clothing/lav rustle).
- ✅ **Multiband / adaptive de-ess** (`v0.5.1`) — `deess_multiband` splits the
  sibilant region into sub-bands, each compressed when it rises above its own
  EMA-tracked running level (`deesser --bands N`).
- ✅ **Phase-coherent stereo** (`v0.5.1`) — `denoise_coherent` (`denoise
  --coherent`) derives one gain mask from the mid signal and applies it to every
  channel, so the stereo image no longer wanders.
- ✅ **Terminal player, visualizer & spectral viewer** (`v0.5.4`, opt-in `tui`
  feature) — `cathar play` streams to the system device (`rodio`) with a live
  colored spectrum analyzer and oscilloscope; `cathar view` renders an
  interactive truecolor time × frequency × level heatmap with a readout crosshair;
  both fall back to xterm-256 on non-truecolor terminals. Also exposes
  `cathar::spectrogram(...)` (Hann-windowed STFT magnitude, dB) in the library.
  This lands the **TUI spectral viewer (`ratatui`)** that Phase 3 had parked
  under `1.0` — ahead of schedule, and entirely behind `--features tui` so the
  default build and dependency set are unchanged.

### `0.6` — Learned denoise (make the `ml` feature real)

- ✅ **Real `candle` model behind `cfg(feature = "ml")`** (`v0.6.0`) — the
  `ml-denoise` command / `NeuralDenoiser`: a GRU spectral-gain network
  (log-magnitude → encoder → GRU → decoder → sigmoid) predicts a per-bin
  suppression mask, applied with phase preserved and window-normalised
  overlap-add — the DNS-Challenge / DeepFilterNet recipe, pure Rust and
  deterministic. Weights load from open `.safetensors` (PyTorch-compatible
  parameter names); `NeuralDenoiser::new()` is a passthrough-initialised default.
  This closes the long-standing gap where the `ml` feature pulled in `candle` but
  no code referenced it.
- ✅ **Bundled pretrained checkpoint** (`v0.6.0`) — a trained GRU checkpoint
  compiled into the binary (2 MB). `NeuralDenoiser::pretrained()` loads it;
  `ml-denoise` uses it by default. Retrain on DNS-Challenge speech data
  for production use.
- ⬜ Optional ML-based VAD and dialogue isolation.

### `0.6.x` — Digitization & fidelity (community [#12](https://github.com/vbasky/cathar/issues/12))

Algorithm-based treatments that deepen the restoration chain beyond noise and
transients — especially for vinyl captures and low-bit-depth sources.

- ✅ **RIAA de-emphasis + elliptical mono** (`v0.6.1`) — `riaa` command /
  `riaa_deemphasis`, `elliptical_mono`, `vinyl_restore`: standard playback curve
  (verified biquad at 44.1/48/88.2/96 kHz; bilinear fallback elsewhere) with
  optional `--elliptical` crossover for stereo rumble
  ([DrCuts](https://github.com/opcode66/DrCuts)).
- 🔶 **Dequantization** (`v0.6.1`) — `dequantize` command: inspectable
  neighbour-prediction on the quantisation lattice (foundation). **Next depth:**
  co-sparse / non-convex recovery from Záviška et al.
  ([audio_dequantization](https://github.com/zawi01/audio_dequantization),
  [ICASSP 2021 paper](https://arxiv.org/abs/2010.16386)).
- 🔶 **Spectral upsampling / resolution enhancement** (`v0.6.1`) — `enhance
  --method replicate|interpolate`: SBR plus log-magnitude extrapolation.
  **Next depth:** published HR interpolation kernels
  ([DSRE](https://github.com/x1aoqv/DSRE---Digital-Sound-Resolution-Enhancer),
  [HRAudioWizard](https://github.com/Super-YH/HRAudioWizard)).

**Planned depth (`0.7.x` restoration track)** — research-backed extensions before
the swiss-army `0.7` utilities milestone in Phase 2:

- ✅ **Wow & flutter** (`v0.7.0`) — `dewow` command / `dewow`: track a dominant
  sustained tone's instantaneous frequency by heterodyne demodulation, build a
  speed curve `s(t)`, and time-warp (resample at `φ⁻¹`, `φ=∫s`) to flatten pitch.
  Best on material with a stable reference pitch
  ([HENDRIX-ZT2/pyaudiorestoration](https://github.com/HENDRIX-ZT2/pyaudiorestoration),
  [Audio Restoration VST](https://github.com/flarkflarkflark/AudioRestorationVST)).
- ✅ **Azimuth / stereo skew correction** (`v0.7.0`) — `azimuth` command /
  `azimuth_correct`: sub-sample cross-correlation lag estimate + fractional
  shift aligns the right channel to the left.
- ⬜ **WPE de-reverb** — Weighted Prediction Error dereverberation in the STFT
  domain (Nakatani et al.) as a deeper alternative to today's energy-based
  `dereverb`; deterministic, no weights
  ([WPE paper](https://arxiv.org/abs/1807.03612)).
- ✅ **Adaptive de-hum** (`v0.7.0`) — `dehum --adaptive` / `dehum_adaptive`:
  locate the precise fundamental from a spectral peak, then cancel each harmonic
  with an I/Q heterodyne canceller (demodulate → zero-phase low-pass → subtract)
  that tracks slow amplitude and small frequency drift.
- ✅ **Audio inpainting / gap interpolation** (`v0.7.0`) — `inpaint` command /
  `inpaint_gap`, `inpaint_auto`: autoregressive **Janssen / Godsill–Rayner**
  interpolation (AR model from the samples around the gap via Levinson–Durbin,
  missing block solved by banded Cholesky, iterated). Order scales with gap
  length; explicit-span or auto zero/NaN-mute detection.
- ✅ **Multi-mic alignment** (`v0.7.0`) — `align` command / `align`,
  `estimate_lag`: sub-sample cross-correlation lag estimate aligns a recording
  to a reference track ([synaudio-cli](https://github.com/eshaz/synaudio-cli),
  [HyMPS alignment index](https://github.com/FORARTfe/HyMPS/blob/main/Audio/Treatments.md#alignmentsynch-)).
  **Next depth:** GCC-PHAT weighting for dissimilar/reverberant content.
- ✅ **Harmonic–percussive separation (HPSS)** (`v0.7.0`) — `hpss` command:
  Fitzgerald median filtering (horizontal median → harmonic, vertical →
  percussive) with a soft Wiener mask; percussive derived by subtraction so
  harmonic+percussive reconstructs exactly. Deterministic, no weights.
- ✅ **De-crackle** (`v0.7.0`) — `decrackle` command: second-difference
  (Laplacian) detector over a running EMA noise-floor flags dense impulsive
  crackle; each micro-run is repaired by cubic-Hermite interpolation. Distinct
  from `declick`'s isolated impulses.
- 🔶 **Analog NR / pre-emphasis decode** (`v0.7.0`) — `deemphasis` command /
  `deemphasis`, `Emphasis`: exact first-order FM 50/75 µs and CD/IEC 50/15 µs
  playback de-curves. **Next depth:** companding decoders (Dolby B/C, dbx).

**Transform & analysis toolkit (`0.7`–`0.8`)** — foundations that broaden the
swiss-army surface and feed later effects:

- ✅ **Phase-vocoder + WSOLA time-stretch** (`v0.7.0`) — `time_stretch` /
  `pitch_shift` (`StretchMode::{Wsola, PhaseVocoder}`): WSOLA overlap-add
  (default, no FFT) and a phase-vocoder engine with instantaneous-frequency
  phase propagation, decoupling duration from rate atop `resample`. Drives the
  `tempo`/`pitch`/`speed` commands below.
- ✅ **Pitch detection (YIN)** (`v0.7.0`) — `detect_pitch` / `fundamental_hz`:
  difference function → cumulative-mean-normalised difference → absolute-
  threshold trough → parabolic interpolation, with a silence gate. A `Pitch
  (f0)` line is exposed in `stats`. (pYIN HMM smoothing still open.)
- ✅ **Constant-Q transform (CQT)** (`v0.7.0`) — `cqt` / `CqtSpec`: direct
  time-domain log-frequency analysis (equal octaves per bin), a library
  primitive alongside `spectrogram`. (TUI-viewer `--cqt` wiring still open.)
- ⬜ **Sinusoidal / spectral modeling (SMS)** — peak-tracked analysis-resynthesis
  for high-quality bandwidth extension and transformation beyond band-replication.

**Spatial & measurement (`0.8`–`0.9`)**:

- ⬜ **Mid-side / stereo toolkit** — M/S encode-decode, Haas widening,
  mono-maker below a cutoff, and mono→stereo decorrelation upmix.
- ⬜ **Measured-IR deconvolution / room correction** — inverse-filter from a
  supplied impulse response; a targeted complement to the *blind* `dereverb`/WPE.
- ⬜ **Masking-aware (psychoacoustic) denoise** — shape the suppression floor by
  a simplified masking model so residual noise sits under the signal rather than
  at a fixed gate.

> Implementation details — module layout, public APIs, algorithm steps, test
> plans and sequencing — are specced in [`docs/algorithm-specs.md`](docs/algorithm-specs.md).

---

## Phase 2 — Swiss-army expansion (`0.7`–`0.10`)

Restoration is the spine; now add the everyday audio toolkit so Cathar can
replace SoX for routine work. Target: **SoX effect/format parity** by `0.11`.

### `0.7` — Core utilities & editing (ahead of schedule — shipped in `v0.6.0` as swiss-army foundation)

- ✅ `convert` (any decode → any encode), `trim`, `pad`, `fade`, `silence`/`vad`.
- ✅ `gain`/`vol`, `remix` (channel mixing), `channels`, `reverse`, `dither`.
- ✅ `speed`/`tempo`/`pitch` (`v0.7.0`) — `tempo` (duration, pitch preserved),
  `pitch` (semitones, duration preserved), `speed` (resample: both), on the
  shipped `resample` + WSOLA/phase-vocoder time-stretch.

### `0.8` — Filters & dynamics (ahead of schedule — shipped in `v0.6.0`)

- ✅ Biquad EQ: `highpass`, `lowpass`, `bandpass`, `equalizer`, `bass`, `treble`.
- ✅ Dynamics: compressor, limiter, gate.
- ⬜ `compand` (multi-band), `contrast`.

### `0.9` — Creative effects

- `reverb`, `echo`/`delay`, `chorus`, `flanger`, `phaser`, `tremolo`,
  `overdrive`. (Restoration removes these; a swiss-army tool also adds them.)

### `0.10` — Analysis & batch power

- ✅ `stat`/`stats` (peak, RMS, LUFS, true-peak, crest factor, DC offset).
- ✅ `spectrogram` lib + TUI viewer (shipped in `v0.5.4`).
- ⬜ Chain DSL + preset files — declarative multi-stage pipelines, reusable across
  `batch`.
- ⬜ **Reference spectral rebalance** — compare a noisy recording's long-term
  spectrum to a clean reference track and apply a corrective, inspectable gain
  curve; useful when a gold-standard take of the same material exists
  ([AssistedSpectralRebalancePlugin](https://github.com/joaomauricio5/AssistedSpectralRebalancePlugin)).

---

## Phase 3 — Performance, integration, `1.0`

### `0.11` — Performance & reach

- SIMD STFT; per-file frame parallelism (batch is already rayon-parallel).
- **Streaming / real-time** block processing with bounded latency → live `cpal`
  use.
- Benchmark suite vs. SoX and FFmpeg so every claim is measured.
- **High-quality resampling mode** — optional premium SRC (longer polyphase sinc,
  configurable passband/stopband) benchmarked against
  [libsamplerate](https://src.hydrogenaudio.org/) and SoX `rate`; the shipped
  Kaiser sinc remains the default for speed and determinism.
- SoX parity audit (see checklist below) — fill remaining gaps.

### `1.0` — Comprehensive & stable

- Stable, semver-guaranteed library API.
- Comprehensive format coverage (pure-Rust default; C-backed codecs opt-in).
- Plugin formats — CLAP (via `nih-plug`) and/or VST3/LV2 — so Cathar runs inside
  a DAW — the primary integration path for a graphical workflow (a standalone
  desktop GUI is out of scope for the core project; collaboration with external
  tools such as
  [SpectraMini](https://github.com/hamiltonbarber/SpectraMini) or
  [Vinyl Restoration Suite](https://github.com/flarkflarkflark/AudioRestorationVST)
  is encouraged — see [#12](https://github.com/vbasky/cathar/issues/12)).
- ✅ Optional TUI spectral viewer (`ratatui`) as a lightweight nod to RX —
  shipped early in `v0.5.4` (`cathar view`, behind `--features tui`).

---

## SoX-parity checklist (gate for `1.0`)

Tracks how close the swiss-army surface is. ✅ done · 🔶 partial · ⬜ planned.

| Capability | SoX | Cathar |
| --- | --- | --- |
| Decode common formats | ✅ | ✅ (Symphonia) |
| Encode common formats | ✅ | ✅ WAV + FLAC + AIFF (more behind `codecs`) |
| Resample (`rate`) | ✅ | ✅ `resample` command + `AudioData::resample` (anti-aliased) |
| Noise profile + reduction | ✅ | ✅ `noiseprint` + `denoise` |
| Normalize / loudness | ✅ | ✅ true EBU R128 (BS.1770-4) + true-peak ceiling |
| Tone/synth generation | ✅ | ✅ `wave` |
| Trim / pad / fade / silence | ✅ | ✅ `v0.6.0` |
| Gain / remix / channels / reverse | ✅ | ✅ `v0.6.0` |
| Speed / tempo / pitch | ✅ | ✅ `tempo`/`pitch`/`speed` (WSOLA + phase vocoder, `v0.7.0`) |
| EQ / filters | ✅ | ✅ biquad: `lowpass`, `highpass`, `bandpass`, `equalizer`, `bass`, `treble` (`v0.6.0`) |
| Compander / dynamics | ✅ | ✅ compressor, limiter, gate (`v0.6.0`) |
| Reverb / echo / chorus / modulation | ✅ | ⬜ `0.9` |
| Stats / spectrogram | ✅ | ✅ `stat`/`stats` + `spectrogram` lib + TUI viewer (`v0.6.0`) |
| De-click / de-clip / de-hum / de-reverb | partial | ✅ (Cathar leads here) |
| Learned denoise | ⬜ | ✅ `ml-denoise` + bundled pretrained checkpoint (`v0.6.0`) |
| Dither | ✅ | ✅ `v0.6.0` |
| Vinyl RIAA / elliptical EQ | ⬜ | ✅ `riaa` (`v0.6.1`) |
| Dequantization | ⬜ | 🔶 `dequantize` foundation (`v0.6.1`) · ⬜ sparse depth |
| Spectral upsampling / resolution enhance | partial | 🔶 `enhance --method` (`v0.6.1`) |
| Wow / flutter / azimuth (vinyl & tape) | ⬜ | ⬜ `0.7.x` restoration |
| Reference spectral rebalance | ⬜ | ⬜ `0.10` |
| Premium resample (libsamplerate-class) | partial | 🔶 Kaiser sinc today · ⬜ `0.11` |

> Restoration depth (`declick`, `declip`, `dehum`, `dereverb`, `deesser`,
> learned denoise) is where Cathar already exceeds SoX — that lead is the point,
> and Phase 1 widens it. Community-sourced algorithm ideas are tracked in
> [#12](https://github.com/vbasky/cathar/issues/12).

---

## Research & project inspiration

A living index of algorithms, papers, and open projects Cathar can draw from.
Not a commitment to implement everything — a map for prioritising **inspectable
DSP first**, with learned models behind explicit `ml` (or future) features when
classical methods plateau. See also the
[HyMPS Treatments catalogue](https://github.com/FORARTfe/HyMPS/blob/main/Audio/Treatments.md).

### Already in Cathar (for reference)

| Area | Cathar today | Primary inspiration |
| --- | --- | --- |
| De-click | Cubic-Hermite interpolation | Classical declicker; Audacity/RX family |
| De-clip | A-SPADE sparse Gabor frame | Kitić, Bertin & Gribonval; [SPADE](https://spade.inria.fr/) |
| De-noise | Spectral subtraction / Wiener; phase-coherent stereo | Boll (1979); Ephraim & Malah |
| Learned de-noise | GRU spectral-gain (`ml` feature) | DNS Challenge / DeepFilterNet recipe |
| De-reverb | Energy-based tail suppression | Classical; **WPE** is the upgrade path |
| De-hum | Cascaded notch harmonics | SoX `synth`/`noisered`; adaptive tracking TBD |
| Spectral repair | Temporal-median outlier pull | iZotope RX Spectral Repair (conceptual) |
| Voice isolate | Energy VAD + spectral gating | Classical; ML dialogue isolation TBD |
| Vinyl | RIAA + elliptical mono | [DrCuts](https://github.com/opcode66/DrCuts), [Vinyl Restoration Suite](https://github.com/flarkflarkflark/AudioRestorationVST) |
| Dequant | Lattice neighbour prediction | Záviška et al. co-sparse methods (depth TBD) |
| Enhance | SBR + log-magnitude interpolate | DSRE, HRAudioWizard |

### High-value additions (restoration toolkit scope)

| Area | Candidate approach | Sources |
| --- | --- | --- |
| **Dereverberation** | WPE / NMF-WPE in STFT domain | [arXiv:1807.03612](https://arxiv.org/abs/1807.03612); interspeech neural-WPE surveys |
| **Dialogue isolation** | ML mask estimation + classical fallback | DeepFilterNet, Demucs stems; DNS Challenge data |
| **De-click (vinyl)** | CED / matched filtering / wavelet | [pyaudiorestoration](https://github.com/HENDRIX-ZT2/pyaudiorestoration); ClickRepair lineage |
| **Wow & flutter** | Drift function → resample correction | Capstan-style; vinyl archival tools |
| **Azimuth & phase** | Cross-correlate L/R; rotate M/S | Tape/vinyl archival practice |
| **Hum tracking** | PLL on fundamental + harmonic comb | Broadcast field-recording tools |
| **Inpainting / gaps** | Sparse Gabor / convex relaxation | A-SPADE family; declipping literature |
| **Dequantization (deep)** | Co-sparse analysis operators | [Záviška ICASSP 2021](https://arxiv.org/abs/2010.16386) |
| **Spectral rebalance** | Long-term envelope match to reference | [AssistedSpectralRebalancePlugin](https://github.com/joaomauricio5/AssistedSpectralRebalancePlugin) |
| **HR upsampling** | Bandlimited interpolation kernels | [DSRE](https://github.com/x1aoqv/DSRE---Digital-Sound-Resolution-Enhancer), [libsamplerate](https://src.hydrogenaudio.org/) |
| **Alignment** | GCC-PHAT / chroma cross-correlation | [synaudio-cli](https://github.com/eshaz/synaudio-cli), [AudioAlign](https://github.com/protyposis/AudioAlign) |
| **DC offset / rumble** | Mean removal + subsonic high-pass | HyMPS DC-offsetting; `dewind` covers part |
| **Plosive / rustle** | Band-limited transient suppression | ✅ shipped; multiband variants from RX/Audition |
| **Sibilance** | Multiband adaptive compression | ✅ shipped `deess_multiband` |

### Open projects worth watching (GUI / integration, not ports)

These are collaboration or plugin-integration targets rather than code to
vendor — Cathar stays CLI/library-first:

- [SpectraMini](https://github.com/hamiltonbarber/SpectraMini) — lightweight spectral editor
- [Python Audio Restoration Suite](https://github.com/HENDRIX-ZT2/pyaudiorestoration) — broad vinyl/tape toolkit
- [Vinyl Restoration Suite](https://github.com/flarkflarkflark/AudioRestorationVST) — VST restoration chain
- [Audio Dequantization](https://github.com/zawi01/audio_dequantization) — MATLAB reference implementations

### Selection criteria (what gets roadmap slots)

1. **Inspectable** — algorithm name, published reference, tunable parameters.
2. **Deterministic** — same input + flags → same bytes (golden tests).
3. **Pure Rust by default** — C/FFI only behind explicit opt-in features.
4. **Restoration-first** — breadth (SoX effects) never crowds out de-* quality.

---

*Milestone numbers signal ordering and intent, not commitments. Restoration
correctness (Phase 1) takes priority over breadth (Phase 2) whenever they
compete.*
