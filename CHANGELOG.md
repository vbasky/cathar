# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/), and this project adheres to
[Semantic Versioning](https://semver.org/).

The release workflow extracts the notes for a version from the matching
`## [x.y.z]` section below, so keep these headings intact.

## [Unreleased]

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
