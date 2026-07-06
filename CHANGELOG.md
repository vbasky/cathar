# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/), and this project adheres to
[Semantic Versioning](https://semver.org/).

The release workflow extracts the notes for a version from the matching
`## [x.y.z]` section below, so keep these headings intact.

## [Unreleased]

### Added

- **`tempo` / `pitch` / `speed` commands** — time-scale and pitch-scale audio.
  `tempo --factor` changes duration with pitch preserved; `pitch --semitones`
  shifts pitch with duration preserved; `speed --factor` resamples (both change,
  like tape). Two engines via `--mode wsola|pv`. Library: `time_stretch`,
  `pitch_shift`, `StretchMode` — WSOLA overlap-add (default, no FFT) and a
  phase-vocoder with instantaneous-frequency phase propagation. Closes the `0.7`
  SoX-parity gap for speed/tempo/pitch.

## [0.6.1] - 2026-07-06

### Added

- **`riaa` command** — RIAA playback (de-emphasis) for digitized vinyl, with
  optional `--elliptical <Hz>` to sum lows to mono on stereo captures. Library:
  `riaa_deemphasis`, `elliptical_mono`, `vinyl_restore`.
- **`dequantize` command** — reduce quantization grain from low-bit-depth
  sources via inspectable neighbour-prediction on the quantisation lattice.
  Library: `dequantize`. Foundation for deeper co-sparse methods (see ROADMAP).
- **`enhance --method replicate|interpolate`** — bandwidth extension now selects
  SBR band replication (default) or log-magnitude spectral extrapolation.
  Library: `EnhanceMethod`, `bandwidth_extend_with_method`.
- **Contributor algorithm specs** — `docs/algorithm-specs.md` documents
  conventions and planned restoration-depth implementations.
- **ROADMAP** — marks `0.6.x` digitization items shipped; adds research &
  project inspiration index and `0.7.x` restoration track.
- **Book** — new chapters on [vinyl digitization (RIAA)](book/src/15-vinyl-digitization.md)
  and [dequantization](book/src/16-dequantization.md); toolbox table and glossary
  updated.

## [0.6.0] - 2026-06-25

### Added

- **Neural spectral-gain denoiser (`ml-denoise`)** — the `ml` feature is real.
  A GRU network (log-magnitude → encoder → GRU → decoder → sigmoid) predicts a
  per-bin suppression mask, applied with phase preserved and window-normalised
  overlap-add. The DNS-Challenge / DeepFilterNet recipe, pure Rust via `candle`,
  deterministic. Weights load from open `.safetensors` checkpoints (PyTorch-
  compatible parameter names). A **bundled pretrained checkpoint** (2 MB,
  compiled into the binary) ships out of the box — `ml-denoise` denoises
  immediately with no download. The passthrough-initialised default remains
  available via `--passthrough`. `NeuralDenoiser::new()` and
  `NeuralDenoiser::pretrained()` are both public.
- **Training script (`scripts/train_denoiser.py`)** — PyTorch training loop
  matching the exact cathar architecture. Generates synthetic clean/noisy tone
  pairs, trains the GRU, and exports a `.safetensors` checkpoint. Retrain on
  [DNS-Challenge](https://github.com/microsoft/DNS-Challenge) speech data for
  production-quality speech denoising.
- **`convert` command** — zero-processing format conversion. Decode from any
  symphonia-supported container and encode to WAV (32-bit float), FLAC (24-bit
  lossless), or AIFF (24-bit) based on output extension.
- **Swiss-army editing utilities (`0.7` phase)** — `trim`, `pad`, `fade`,
  `silence` (strip), `gain`, `remix` (mono/swap), `channels` (select),
  `reverse`, and `dither` (TPDF). All available as library functions and CLI
  subcommands.
- **Golden-file integration tests (`crates/cathar/tests/golden.rs`)** — byte-
  exact regression tests for every restoration transform. Run `cargo test
  --test golden` to verify output matches the precomputed references; regenerate
  with `--ignored`. Also: deterministic WAV round-trip test.
- **SoX comparison script (`scripts/compare_sox.sh`)** — sanity-checks cathar
  resample, dehum, declip, and normalize against SoX equivalents.
- **CLI startup banner** — inline-image logo on supported terminals (iTerm2,
  WezTerm, ghostty, Warp, Rio, Konsole). Suppress with `--no-banner`.

### Changed

- `ml-denoise` now uses the bundled pretrained model by default (no more
  passthrough-no-op surprise). Pass `--passthrough` for the old behaviour or
  `--weights <checkpoint.safetensors>` for a custom model.

## [0.5.4] - 2026-06-21

### Added

- **Player + visualizer (`cathar play`, opt-in `tui` feature)** — a Winamp/cava-
  style terminal player: streams the file to the system audio device (`rodio`) and
  animates a live, colored spectrum analyzer (log-spaced bands, unicode eighth-
  blocks, gravity decay + peak-hold caps) synced to playback, plus an oscilloscope
  mode. `space` pause, `←/→` seek, `m` mode, `q` quit. On Linux the build needs
  ALSA headers (`libasound2-dev`).
- **`spectrogram` (library)** — `cathar::spectrogram(signal, sample_rate, fft_size,
  hop)` computes a Hann-windowed STFT magnitude spectrogram (dB), returned as a
  `Spectrogram` with `frames()`/`get()`/`bin_hz()`/`frame_time()` helpers.
- Both TUI tools use 24-bit truecolor when the terminal advertises it
  (`COLORTERM`) and otherwise downsample to the nearest xterm-256 palette colors,
  so gradients render correctly on 256-color terminals (e.g. macOS Terminal.app).
- **Terminal spectrogram viewer (`cathar view`, opt-in `tui` feature)** — an
  interactive truecolor heatmap of time × frequency × level built on `ratatui`, a
  lightweight nod to RX's spectral display. Unicode half-blocks pack two frequency
  bins per row; a movable crosshair reads out time/frequency/dB, `+`/`-` zoom time,
  `f` toggles log frequency. Behind `--features tui` so the default build and its
  dependency set are unchanged: `cargo install cathar-cli --features tui`.

### Fixed

- **Security (RUSTSEC-2026-0009)** — bumped the transitive `time` dependency
  (pulled in by the `tui` feature via `ratatui`) from 0.3.45 to 0.3.47, clearing
  a denial-of-service-via-stack-exhaustion advisory. `time` only enters the graph
  under the optional `tui` feature, so the default build's MSRV (1.87) is unchanged.
- `cathar play` no longer prints rodio's "Dropping DeviceSink…" warning over the
  restored terminal on exit — playback is stopped deliberately, so the sink's
  drop logging is disabled.

## [0.5.3] - 2026-06-21

### Changed

- **De-clip now uses A-SPADE sparse reconstruction** (Kitić, Bertin & Gribonval,
  2015) over a Hann-windowed, 4×-overlapping Gabor tight frame, replacing the
  LSAR fill shipped in 0.5.2. Each clipped run is recovered as the signal that is
  sparsest in the windowed-DFT domain while keeping reliable samples exact and
  clipped samples beyond the threshold; the iteration converges monotonically and
  rebuilds a clipped tone to within ~0.01 RMS of the original with the peak
  restored. It is iterative (≈2 s for a few-second clip) where LSAR was one-shot —
  the quality/robustness trade chosen deliberately. Public API unchanged.

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
