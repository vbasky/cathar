# tersus

*tersus* — Latin for **"clean, neat, polished."** AI denoising and audio
cleanup for video. Extracts the audio track from a video file, runs a deep
learning denoiser, and writes a clean version — testing it in the CLI with
waveform visualisation so you can see and hear the difference.

Given a noisy recording (video or audio), tersus:
1. Demuxes the audio track (if video input).
2. Runs inference through a speech-denoising model (Demucs / DNS Challenge
   family, ONNX Runtime).
3. Writes the cleaned audio alongside an original-vs-cleaned spectrogram for
   visual comparison.

**Scope is deliberately bounded.** tersus owns the denoise loop and the model
inference — not a full NLE, not real-time processing, not a model zoo. The
default build ships a deterministic mock so the loop runs end-to-end with no
model weights.

[![CI](https://github.com/vbasky/tersus/actions/workflows/ci.yml/badge.svg)](https://github.com/vbasky/tersus/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

## Layout

```
crates/
  tersus/       # library crate
  tersus-cli/   # binary crate (clap), installs as `tersus`
```

## Quick start

```bash
just setup        # one-time: enable the auto-format pre-commit hook
just build        # build the workspace
just test         # run all tests

# Generate a test sine wave with noise:
just run -- wave --out test.wav --duration 5 --freq 440 --noise 0.1

# Denoise a file (mock backend until you wire real inference):
just run -- denoise test.wav --out clean.wav
```

If you don't have [`just`](https://github.com/casey/just):
`cargo install just`.

## The real thing: AI denoising

With the `ort` feature on, tersus runs a Demucs-style speech denoiser:

```bash
cargo run -p tersus-cli --features ort -- denoise \
  noisy_interview.m4a --model demucs.onnx --out clean.wav
```

Models are standard ONNX exports. See `docs/` for model setup.

## CLI subcommands

| Command | What it does |
|---------|-------------|
| `denoise` | Run the denoiser on a video or audio file |
| `wave` | Generate a synthetic waveform for testing |
| `spectrogram` | Render original vs. cleaned spectrograms |

## Development

`just check-all` runs the exact gate CI enforces — formatting, clippy
(`-D warnings`), tests, and docs — before you push.

| Task | Command |
| --- | --- |
| Format | `just fmt` |
| Lint | `just lint` |
| Test | `just test` |
| Docs | `just docs` |
| Dependency audit | `just deny` (needs `cargo install cargo-deny`) |

## Releasing

1. Update `CHANGELOG.md` under a new `## [x.y.z]` heading and commit.
2. `just release x.y.z` — bumps versions, tags, and pushes.
3. CI (`.github/workflows/release.yml`) builds binaries for macOS (arm64 +
   x86_64), Linux, and Windows, and publishes a GitHub Release with checksums
   and the changelog notes.
4. To also publish to crates.io: `PUBLISH=1 just release x.y.z` (needs
   `cargo login`).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
