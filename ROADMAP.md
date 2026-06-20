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

## Where we are — `0.4.x`

The restoration chain is implemented and unit-tested: `denoise` (spectral
subtraction / Wiener, noiseprint- or minimum-statistics-driven), `dehum`,
`dereverb`, `voiceisolate`, `deesser`, `breath`, `declick`, `declip`, `enhance`
(bandwidth extension), `normalize` (true EBU R128), `resample`, plus the `wave`
generator and parallel `batch`. Decode is via Symphonia
(MP4/M4A/MKV/MP3/FLAC/WAV/OGG); **encode is WAV (32-bit float), FLAC (24-bit
lossless), or AIFF (24-bit), chosen by the output extension.**

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

- **Spectral repair** — interpolate/paint out transient artifacts (RX's
  signature capability).
- **De-rustle, de-wind, de-plosive** — round out the `de-*` family.
- **Multiband / adaptive** denoise and de-ess.
- **Phase-coherent stereo** processing (today each channel is independent;
  joint-stereo matters for imaging).

### `0.6` — Learned denoise (make the `ml` feature real)

- Wire an actual `candle` model behind `cfg(feature = "ml")` — today the feature
  pulls in `candle` but **no code references it**.
- Port or run a DeepFilterNet / DNS-Challenge model; ship or fetch weights.
- Optional ML-based VAD and dialogue isolation.

---

## Phase 2 — Swiss-army expansion (`0.7`–`0.10`)

Restoration is the spine; now add the everyday audio toolkit so Cathar can
replace SoX for routine work. Target: **SoX effect/format parity** by `0.11`.

### `0.7` — Core utilities & editing

- `convert` (any decode → any encode), `trim`, `pad`, `fade`, `silence`/`vad`.
- `gain`/`vol`, `remix` (channel mixing), `channels`, `reverse`, `dither`.
- `speed`/`tempo`/`pitch` (built on the shipped `resample` + time-stretch).

### `0.8` — Filters & dynamics

- Biquad EQ: `highpass`, `lowpass`, `bandpass`, `equalizer`, `bass`, `treble`.
- Dynamics: `compand`/compressor, limiter, gate, `contrast`.

### `0.9` — Creative effects

- `reverb`, `echo`/`delay`, `chorus`, `flanger`, `phaser`, `tremolo`,
  `overdrive`. (Restoration removes these; a swiss-army tool also adds them.)

### `0.10` — Analysis & batch power

- `stat`/`stats`, `spectrogram`, loudness/true-peak reporting.
- Chain DSL + preset files — declarative multi-stage pipelines, reusable across
  `batch`.

---

## Phase 3 — Performance, integration, `1.0`

### `0.11` — Performance & reach

- SIMD STFT; per-file frame parallelism (batch is already rayon-parallel).
- **Streaming / real-time** block processing with bounded latency → live `cpal`
  use.
- Benchmark suite vs. SoX and FFmpeg so every claim is measured.
- SoX parity audit (see checklist below) — fill remaining gaps.

### `1.0` — Comprehensive & stable

- Stable, semver-guaranteed library API.
- Comprehensive format coverage (pure-Rust default; C-backed codecs opt-in).
- Plugin formats — CLAP (via `nih-plug`) and/or VST3/LV2 — so Cathar runs inside
  a DAW.
- Optional TUI spectral viewer (`ratatui`) as a lightweight nod to RX.

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
| Trim / pad / fade / silence | ✅ | ⬜ `0.7` |
| Gain / remix / channels / reverse | ✅ | ⬜ `0.7` |
| Speed / tempo / pitch | ✅ | ⬜ `0.7` |
| EQ / filters | ✅ | ⬜ `0.8` |
| Compander / dynamics | ✅ | ⬜ `0.8` |
| Reverb / echo / chorus / modulation | ✅ | ⬜ `0.9` |
| Stats / spectrogram | ✅ | ⬜ `0.10` |
| De-click / de-clip / de-hum / de-reverb | partial | ✅ (Cathar leads here) |
| Learned denoise | ⬜ | ⬜ `0.6` |

> Restoration depth (`declick`, `declip`, `dehum`, `dereverb`, `deesser`,
> learned denoise) is where Cathar already exceeds SoX — that lead is the point,
> and Phase 1 widens it.

---

*Milestone numbers signal ordering and intent, not commitments. Restoration
correctness (Phase 1) takes priority over breadth (Phase 2) whenever they
compete.*
