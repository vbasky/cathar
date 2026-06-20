# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/), and this project adheres to
[Semantic Versioning](https://semver.org/).

The release workflow extracts the notes for a version from the matching
`## [x.y.z]` section below, so keep these headings intact.

## [Unreleased]

## [0.3.1] - 2026-06-21

Backport of the LSAR de-clip improvement from 0.5.2 to the 0.3 line.

### Changed

- **De-clip now reconstructs clipped peaks with least-squares autoregressive
  interpolation (LSAR)** — the classical audio-restoration method (Janssen,
  Veldhuis & Vries, 1986) — instead of a cubic fill. An AR model is fit to the
  reliable audio either side of each clipped run (two-sided autocorrelation →
  Levinson-Durbin) and the gap samples that minimise its prediction error are
  solved for (banded normal equations via Cholesky), so a clipped peak is rebuilt
  toward its true amplitude rather than flattened to the shoulder level. A
  stability guard falls back to the previous smooth fill when a solve rings or
  overshoots, so badly-clipped material softens gracefully.

## [0.3.0] - 2026-06-20

### Added

- **Main-path resampling.** A shared Kaiser-windowed sinc resampler (16 lobes,
  β = 9, arbitrary ratio) with cutoff tracking the lower Nyquist, so
  downsampling is anti-aliased and upsampling adds no imaging. Exposed as the
  `resample` free function, `AudioData::resample(target_rate)`, and a new
  `resample` CLI command. Any stage can now bring mixed-rate inputs to a common
  rate.

### Changed

- `bandwidth_extend` (`enhance`) now uses the shared resampler instead of its
  own inline windowed-sinc loop.

## [0.2.0] - 2026-06-20

### Added

- `integrated_loudness` and `true_peak_dbtp` measurement functions, and
  `AudioData::normalize_r128(target_lufs, true_peak_ceiling_db)`.

### Changed

- **True EBU R128 loudness.** `normalize` now measures integrated LUFS with
  K-weighting and gating (ITU-R BS.1770-4) jointly across channels and applies a
  single broadband gain, held back to a `--true-peak` dBTP ceiling (4×
  oversampled), replacing the previous RMS-based LUFS approximation. `batch`
  `--normalize` uses the same path.

### Removed

- `normalize_loudness` (per-channel RMS) — superseded by `normalize_r128`.

## [0.1.1] - 2026-06-20

### Changed

- `batch` now processes files in parallel across the rayon thread pool instead
  of sequentially. Per-file errors are reported and skipped rather than aborting
  the run.

## [0.1.0] - 2026-06-18

Initial release.

### Added

- Audio-restoration toolkit — `cathar` library plus the `cathar` CLI. Decodes
  any [`symphonia`](https://crates.io/crates/symphonia) 0.6 input (MP4, M4A,
  MKV, MP3, FLAC, WAV, OGG) to `f32` PCM and writes 32-bit float WAV — no
  ffmpeg, no C/C++, pure Rust.
- **Reduce:** `denoise` (spectral subtraction or Wiener filter, driven by
  learned `noiseprint`s or minimum-statistics noise estimation), `dehum`,
  `dereverb`, `voiceisolate`, `deesser`, `breath`.
- **Repair:** `declick`, `declip`.
- **Enhance & level:** `enhance` (bandwidth extension), `normalize` (LUFS /
  peak).
- **Utilities:** `wave` test-tone generator and `batch` directory processing.
- Optional `ml` feature scaffolding (candle) for a future learned denoiser.
