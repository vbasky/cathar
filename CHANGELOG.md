# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/), and this project adheres to
[Semantic Versioning](https://semver.org/).

The release workflow extracts the notes for a version from the matching
`## [x.y.z]` section below, so keep these headings intact.

## [Unreleased]

## [0.5.2] - 2026-06-21

### Changed

- **De-clip now reconstructs clipped peaks with least-squares autoregressive
  interpolation (LSAR)** — the classical audio-restoration method (Janssen,
  Veldhuis & Vries, 1986) — instead of a cubic fill. An AR model is fit to the
  reliable audio either side of each clipped run (two-sided autocorrelation →
  Levinson-Durbin) and the gap samples that minimise its prediction error are
  solved for (banded normal equations via Cholesky), so a clipped peak is rebuilt
  toward its true amplitude rather than flattened to the shoulder level. A
  stability guard falls back to the previous smooth fill when a solve rings or
  overshoots, so badly-clipped material softens gracefully. On a +8 dB-clipped
  voice the old fill could only reach the 0.977 plateau; LSAR rebuilds the true
  peaks to ~1.53 (normalise afterwards).

### Fixed

- Mono WAV output played in the left speaker only. `hound` writes 32-bit float
  WAV as `WAVE_FORMAT_EXTENSIBLE` and tags a single channel as `FRONT_LEFT`, so
  layout-aware players (CoreAudio / `afplay`) routed it hard-left. Mono output is
  now tagged `FRONT_CENTER` and plays centred. Stereo and FLAC/AIFF were
  unaffected.

### Documentation

- Added **"Cleaning Up Sound"** (`book/`) — a from-first-principles book on the
  concepts this toolkit uses, for readers new to DAWs/DSP, with diagrams, a cover
  page, and a GitHub Pages build.
- Documented every remaining public item (`Error` + variants, `AudioData` +
  fields, the `Denoiser` trait, `NoisePrint`/`SpectralDenoiser` fields,
  `with_noise_print`, `generate_wave`, `variance`) and added
  `#![deny(missing_docs)]` to the `cathar` crate so public docs can't regress.

### Internal

- Split the ~2,400-line `cathar/src/lib.rs` into focused modules (`audio`,
  `denoise`, `restore`, `enhance`, `loudness`, `resample`, `error`, `util`),
  re-exported flat so the public API is unchanged. No behaviour change.

## [0.5.1] - 2026-06-20

Completes the `0.5` DSP-depth milestone (spectral repair shipped in 0.5.0).

### Added

- **De-wind** (`dewind` / `dewind` command) — 4th-order Butterworth high-pass
  (two cascaded biquads) to cut low-frequency wind rumble at a chosen cutoff.
- **De-plosive** (`deplosive`) and **de-rustle** (`derustle`) — band-limited
  transient suppression: per frame, energy in a band (plosive < 250 Hz, rustle
  1.5–6 kHz) that spikes above its temporal median is scaled back toward it with
  phase preserved, leaving sustained content untouched.
- **Multiband / adaptive de-ess** (`deess_multiband`, `deesser --bands N`) — the
  sibilant region is split into sub-bands, each compressed only when it rises
  `threshold` dB above its own EMA-tracked running level.
- **Phase-coherent stereo** (`SpectralDenoiser::denoise_coherent`,
  `denoise --coherent`) — one suppression gain mask is computed from the mid
  (L+R) signal and applied to every channel, so the stereo image stays stable
  instead of wandering as bins gate differently per channel.

## [0.5.0] - 2026-06-20

### Added

- **Spectral repair** — the `repair` command and `spectral_repair` function.
  Paints out isolated transient spectral artifacts (whistles, bursts, glitches):
  each STFT bin is compared to its temporal median across neighbouring frames and
  transient outliers are pulled back to the median with phase preserved, so
  sustained tones/formants/texture pass through transparently (overlap-add is
  window-normalised to unity gain). `--strength` (1–10) tunes aggressiveness.
  First item of the `0.5` DSP-depth milestone.

## [0.4.1] - 2026-06-20

### Fixed

- README links and images now render on crates.io. The crate README is a
  symlink under `crates/cathar/`, so crates.io resolved relative paths against
  that directory and 404'd the `ROADMAP.md`, license, and STFT-diagram links;
  every repo link/image is now an absolute `github.com/.../blob/main` (or
  `raw.githubusercontent.com`) URL, and the diagram is PNG (crates.io strips
  SVG).
- Refreshed stale docs: version `0.1.x` → `0.4.x`, roadmap phase numbers aligned
  with the renumbered `ROADMAP.md`, and the primary install is now
  `cargo install cathar-cli` (from crates.io).

## [0.4.0] - 2026-06-20

### Added

- **Encode beyond WAV.** `AudioData::to_file` now selects the container from the
  output extension: 24-bit lossless FLAC (`.flac`, via the pure-Rust `flacenc`)
  and 24-bit big-endian PCM AIFF (`.aif`/`.aiff`), in addition to 32-bit float
  WAV (the default). Every CLI command picks the format from its `--out`
  extension.

### Changed

- MSRV raised to 1.87 (required by a `flacenc` dependency).

### Fixed

- FLAC decoding: end-of-stream is now handled when symphonia signals it with an
  `UnexpectedEof` I/O error rather than a clean end, so FLAC inputs decode fully.
- FLAC encoding writes `min_block_size == max_block_size` in STREAMINFO for
  fixed-block-size streams, so strict decoders (including symphonia) don't
  misread cathar's FLAC output as variable-block-size.

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
