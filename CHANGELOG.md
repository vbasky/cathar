# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/), and this project adheres to
[Semantic Versioning](https://semver.org/).

The release workflow extracts the notes for a version from the matching
`## [x.y.z]` section below, so keep these headings intact.

## [Unreleased]

## [0.1.0] - 2026-06-18

### Added

- Audio-restoration library (`cathar`) and CLI: decode any
  [`symphonia`](https://crates.io/crates/symphonia)-supported media file
  (MP4, M4A, MKV, MP3, FLAC, WAV, OGG) to `f32` PCM and write 32-bit float WAV
  — no ffmpeg.
- Denoising: spectral subtraction and Wiener filter, driven by learned noise
  prints (`noiseprint`) or minimum-statistics noise estimation.
- Repair & reduction: `dehum`, `declick`, `declip`, `dereverb`,
  `voiceisolate`, `deesser`, `breath`.
- Enhancement & levelling: `enhance` (bandwidth extension) and `normalize`
  (LUFS / peak).
- Utilities: `wave` test-tone generator and `batch` directory processing.
- Optional `ml` feature scaffolding (candle) for a future learned denoiser.
