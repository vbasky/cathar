# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/), and this project adheres to
[Semantic Versioning](https://semver.org/).

The release workflow extracts the notes for a version from the matching
`## [x.y.z]` section below, so keep these headings intact.

## [Unreleased]

## [0.7.7] - 2026-09-21

VHS noise print no longer eats sibilance, the chain no longer gates pauses,
and a weak 50 Hz line is not mistaken for 60
([#27](https://github.com/vbasky/cathar/issues/27)).

### Fixed

- **`vhs` noise print** ([#27](https://github.com/vbasky/cathar/issues/27)) —
  `learn_noise_print_quietest` took one contiguous 4 s stretch. On dialogue
  that stretch sits well above the true pauses and the print carries
  sibilance, so subtraction takes down 8–16 kHz. The chain now stitches
  eight non-overlapping 0.75 s windows spread evenly by level over the
  quietest 20 % of the file, with 10 ms crossfades
  (`learn_noise_print_quiet_windows`). Digital-silent tiles are skipped so
  a dropout is not learned as the floor.
- **`vhs` alpha** ([#27](https://github.com/vbasky/cathar/issues/27)) —
  default over-subtraction is 2.0 (`VhsOptions.alpha`, `cathar vhs --alpha`).
  Alpha 3 with coherent subtraction gated pauses to near-silence.
- **`detect_mains_hz`** ([#27](https://github.com/vbasky/cathar/issues/27)) —
  50 vs 60 is taken from the mean spectrum of the whole recording, not the
  quietest 1 s. A weak 50 Hz PAL line was losing to a 60 Hz bump in one
  pause; harmonic placement still uses the quietest stretch.

## [0.7.6] - 2026-09-21

VHS multiband de-ess no longer takes down everything above 4 kHz
([#26](https://github.com/vbasky/cathar/issues/26)).

### Fixed

- **`vhs` de-ess** ([#26](https://github.com/vbasky/cathar/issues/26)) — the
  chain called `deess_multiband` with `threshold_db = -24`, the single-band
  HF/broadband-ratio default. In multiband mode that value is "dB above each
  band's running average", so every frame was over and the region above 4 kHz
  was taken down (~30 dB). Default is now 6 dB (`VhsOptions.deess_threshold`,
  `cathar vhs --deess-threshold`). Negative multiband thresholds are floored
  at 0 dB so the same mix-up cannot crush the highs.

## [0.7.5] - 2026-09-20

High-frequency enhance methods inspired by DSRE / HRAudioWizard
([#20](https://github.com/vbasky/cathar/issues/20)).

### Added

- **`enhance --method harmonic|dsre`** ([#20](https://github.com/vbasky/cathar/issues/20))
  — two inspectable high-frequency restorers on top of the shipped SBR /
  log-magnitude pair. `harmonic` extends detected overtone series into the
  empty band (HRAudioWizard HFP family, phase-locked, no Griffin-Lim).
  `dsre` waveshapes the existing highs and high-passes the new content above
  the original ceiling (DSRE / DSEE-like). Both fill a rolled-off top at the
  same sample rate; `replicate` / `interpolate` still require a rate increase.
  Library: `EnhanceMethod::{Harmonic, Dsre}`.

## [0.7.4] - 2026-09-20

Tape / VHS restoration chain and measured de-hum / de-plosive fixes
([#25](https://github.com/vbasky/cathar/issues/25)).

### Added

- **Tape / VHS restoration chain** ([#25](https://github.com/vbasky/cathar/issues/25))
  — `cathar vhs` runs a gated cascade: DC block, rumble high-pass, stereo
  azimuth + bass-mono, declip (only if flat-top runs are present), dropout
  inpaint, declick, decrackle, adaptive dehum, event-gated deplosive, coherent
  spectral subtraction from the quietest 4 s, multiband de-ess. Library:
  `vhs_restore`, `VhsOptions`. Inspired by the measured
  `auto_pure_linear` stages in
  [AI Hybrid VHS Audio Restorer](https://github.com/ventura8/AI-Hybrid-VHS-Audio-Restorer).
- **`detect_mains_hz`** — pick 50 vs 60 from the recording's quiet spectrum.
- **`learn_noise_print_quietest`** — noise print from the quietest N seconds.
- **`remove_dc`** — mean subtraction (DC blocking).

### Changed

- **`dehum --adaptive`** — each harmonic is cancelled at the frequency it
  actually sits (tape lines land 1–8 Hz off the exact series); 50 vs 60 is
  taken from the recording when the other series is clearly stronger;
  harmonics that do not stand out are skipped, so no-hum files pass through
  unchanged. `--freq 0` auto-detects. Envelope bandwidth widens with harmonic
  number; a 6 dB cap over the running envelope keeps a voiced partial parked
  on a mains line from being taken with it.
- **`deplosive`** — default is now event-gated (`--method events`): a blast
  under 150 Hz that stands over the low band's running level and leads the
  mid band is taken down, and nothing else is touched. Undamaged material is
  bit-identical. `--method transients` keeps the legacy whole-file STFT path,
  which was measured harmful on controls.

## [0.7.3] - 2026-09-01

### Added

- **`stats` restoration triage** ([#22](https://github.com/vbasky/cathar/issues/22))
  — noise-floor dBFS (quietest 15 % of frames), SNR, and clip-run counts
  (flat-topped peaks at `|x| ≥ 0.99`). Failing checks print one suggested
  command (`declip`, `denoise`, or `normalize --true-peak -1`). Library:
  `Stats::{noise_floor_dbfs, snr_db, clipped_samples, clipped_runs, suggestions}`,
  `Suggestion`. No mix-quality score.

## [0.7.2] - 2026-08-05

Stereo toolkit and phase-aware alignment / diagnostics
([#19](https://github.com/vbasky/cathar/issues/19)).

### Added

- **`stereo` command / mid-side toolkit** — exact M/S encode–decode, `--width`
  (scale Side), `--mono-below` (mono-maker via elliptical crossover), `--upmix`
  (Haas mono→stereo), `--haas-ms`, and `--ms` / `--from-ms`. Library:
  `ms_encode`, `ms_decode`, `stereo_width`, `mono_below`, `upmix_mono`,
  `haas_delay`, `phase_correlation`.
- **GCC-PHAT alignment** — `align` / `azimuth` accept
  `--method correlation|gcc-phat` for lag estimation on level-mismatched or
  mildly reverberant pairs. Library: `LagMethod`, `estimate_lag_with_method`,
  `align_with_method`, `azimuth_correct_with_method`.
- **`stats` phase correlation** — stereo files report zero-lag L/R correlation
  in `[-1, +1]` (mono compatibility / out-of-phase check).

## [0.7.1] - 2026-08-02

De-click / de-clip method selection and survey-family reconstruction algorithms
([#17](https://github.com/vbasky/cathar/issues/17)).

### Added

- **`declick --method ar|cubic`** — reconstruction is selectable. Default is
  autoregressive Janssen interpolation (same family as `inpaint`); `cubic` keeps
  the legacy Hermite fill. Library: `DeclickMethod`, `declick_with_method`.
- **`declip --method spade|cubic|social|omp|nmf|neural`** — A-SPADE remains
  the default (survey-preferred sparse Gabor reconstruction). Additional pure-
  Rust methods from the Rajmic et al. / Adler / Bilen lineage:
  - `social` — Persistent Empirical Wiener (PEW) social sparsity on a
    time-frequency neighbourhood + consistency projection
  - `omp` — constrained matching pursuit on a per-frame DFT dictionary
  - `nmf` — low-rank NMF of the STFT magnitude, phase retained
  - `neural` — deep-unfolded soft-threshold ISTA (LISTA-style, weight-free;
    not a supervised DeclipNet — those need trained checkpoints)
  - `cubic` — fast shoulder-Hermite fill for light clips / previews
  Library: `DeclipMethod`, `declip_with_method` (module `declip`).

### Changed

- **`declick` default reconstruction** — now AR (Janssen) instead of cubic-
  Hermite; detection is unchanged (sliding-window local RMS).
- **README** — algorithm table corrected: `declip` documents A-SPADE (was stale
  cubic text), `declick` documents AR + method flags.
- **Book** — chapter 6 (clicks/clipping) updated for AR de-click and method flags;
  cites the Rajmic et al. de-clipping survey for A-SPADE.
- **Book** — brought chapters in line with `v0.6.1`–`v0.7.0`: fixed stale vinyl
  workflow text; expanded toolbox table; added adaptive de-hum, WPE de-reverb,
  `enhance --method`, optional `ml-denoise`, and [broadcast/CD de-emphasis](book/src/20-playback-deemphasis.md);
  updated industry comparison, glossary, and `book/README.md` contents.

## [0.7.0] - 2026-07-07

Restoration-depth & transform release: time-stretch/pitch, and a broad
`0.7.x` sweep of research-backed DSP — pitch detection, separation, inpainting,
adaptive de-hum, pre-emphasis decode, alignment, wow/flutter, WPE de-reverb, and
sinusoidal modeling. All deterministic, pure Rust.

### Added

- **`sms` command** — sinusoidal-modeling tonal purify (peak tracking + additive
  resynthesis). Library: `analyze_sms`, `synthesize_sms`, `SinusoidalModel`.
- **`dereverb --wpe`** — Weighted Prediction Error dereverberation (per-bin
  weighted linear prediction in the STFT domain). Library: `wpe`.
- **`dewow` command** — correct wow & flutter (pitch drift) by tracking a
  dominant tone's instantaneous frequency and time-warping to flatten it.
  Library: `dewow`.
- **`azimuth` command** — correct stereo azimuth skew (align R to L by
  sub-sample cross-correlation). Library: `azimuth_correct`.
- **`align` command** — time-align a recording to a reference track (multi-mic).
  Library: `align`, `estimate_lag`.
- **`dehum --adaptive`** — track a drifting mains fundamental + per-harmonic
  amplitude via an I/Q heterodyne canceller. Library: `dehum_adaptive`.
- **`deemphasis` command** — analog playback de-emphasis (FM 50/75 µs, CD/IEC
  50/15 µs). Library: `deemphasis`, `Emphasis`.
- **Constant-Q transform** — log-frequency analysis primitive. Library: `cqt`,
  `CqtSpec`.
- **`hpss` command** — harmonic/percussive separation (Fitzgerald median
  filtering); writes both layers, exact reconstruction. Library: `hpss`.
- **`inpaint` command** — reconstruct dropouts/mutes by autoregressive
  (Janssen) gap interpolation; explicit `--start-ms/--len-ms` or auto zero/NaN
  detection. Library: `inpaint_gap`, `inpaint_auto`.
- **`decrackle` command** — suppress dense vinyl surface crackle via a
  Laplacian detector over a running noise floor + cubic-Hermite repair.
  Library: `decrackle`.
- **YIN pitch detection** — `detect_pitch`, `fundamental_hz`; `stats` now
  reports an f0 line.
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
