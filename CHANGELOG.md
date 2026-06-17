# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/), and this project adheres to
[Semantic Versioning](https://semver.org/).

The release workflow extracts the notes for a version from the matching
`## [x.y.z]` section below, so keep these headings intact.

## [Unreleased]

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
