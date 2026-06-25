//! Golden-file tests: each transform is run against a fixed input signal and
//! the output is compared byte-for-byte to a pre-computed reference. This
//! guarantees "same input, same flags, same bytes out" — the determinism
//! promise in the ROADMAP.
//!
//! Golden files are platform-specific (float rounding differs across arches),
//! so these only run on macOS where they were generated. To add Linux/Win
//! golden files, generate them on each platform and gate with per-OS cfg.
//!
//! ## Regenerating golden files
//!
//! Run with `--ignored` to regenerate all golden files after an intentional
//! change:
//!
//!     cargo test --test golden -- --ignored
//!
//! Golden files live in `tests/golden/` relative to this crate root.

#![cfg(target_os = "macos")]

use std::path::PathBuf;

use cathar::*;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden")
}

fn test_signal() -> AudioData {
    generate_wave(48_000, 440.0, 1.0, 0.05)
}

fn regen(name: &str, f: fn(&AudioData) -> AudioData) {
    let input = test_signal();
    let out = f(&input);
    let dir = golden_dir();
    std::fs::create_dir_all(&dir).ok();
    out.to_file(dir.join(name).to_str().unwrap()).unwrap();
}

fn check(name: &str, f: fn(&AudioData) -> AudioData) {
    let input = test_signal();
    let out = f(&input);
    let gold_path = golden_dir().join(name);
    assert!(
        gold_path.exists(),
        "golden file missing: {} — run `cargo test --test golden -- --ignored` to regenerate",
        gold_path.display()
    );
    let expected = std::fs::read(&gold_path).unwrap();
    let tmp = std::env::temp_dir().join(format!("cathar_golden_{name}"));
    out.to_file(tmp.to_str().unwrap()).unwrap();
    let actual = std::fs::read(&tmp).unwrap();
    assert_eq!(
        actual, expected,
        "{} drifted from golden — regenerate with `cargo test --test golden -- --ignored`",
        name
    );
}

// ── regeneration (ignored tests) ───────────────────────────────────────────

#[ignore]
#[test]
fn regen_all() {
    regen("resample_48000_to_44100.wav", |a| a.resample(44_100));
    regen("normalize_r128.wav", |a| a.normalize_r128(-23.0, -1.0));
    regen("denoise_default.wav", |a| {
        let d = SpectralDenoiser::default();
        d.denoise(a).unwrap()
    });
    regen("dehum_60hz.wav", |a| a.map_channels(|c| dehum(c, a.sample_rate, 60.0, 5)));
    regen("declick.wav", |a| a.map_channels(|c| declick(c, 10.0, 64)));
    regen("dewind.wav", |a| a.map_channels(|c| dewind(c, a.sample_rate, 80.0)));
    regen("repair.wav", |a| a.map_channels(|c| spectral_repair(c, 4.0)));
    regen("enhance.wav", |a| {
        let ch: Vec<Vec<f32>> =
            a.channels.iter().map(|c| bandwidth_extend(c, a.sample_rate, 48_000)).collect();
        AudioData { sample_rate: 48_000, channels: ch }
    });
    // declip needs a clipping signal
    let fs = 44_100u32;
    let n = fs as usize;
    let clip = 0.7f32;
    let truth: Vec<f32> =
        (0..n).map(|i| (2.0 * std::f32::consts::PI * 220.0 * i as f32 / fs as f32).sin()).collect();
    let clipped: Vec<f32> = truth.iter().map(|&v| v.clamp(-clip, clip)).collect();
    let out =
        AudioData { sample_rate: fs, channels: vec![clipped] }.map_channels(|c| declip(c, clip));
    let dir = golden_dir();
    std::fs::create_dir_all(&dir).ok();
    out.to_file(dir.join("declip.wav").to_str().unwrap()).unwrap();
}

// ── verification (always-run tests) ────────────────────────────────────────

#[test]
fn golden_resample() {
    check("resample_48000_to_44100.wav", |a| a.resample(44_100));
}
#[test]
fn golden_normalize_r128() {
    check("normalize_r128.wav", |a| a.normalize_r128(-23.0, -1.0));
}
#[test]
fn golden_denoise() {
    check("denoise_default.wav", |a| {
        let d = SpectralDenoiser::default();
        d.denoise(a).unwrap()
    });
}
#[test]
fn golden_dehum() {
    check("dehum_60hz.wav", |a| a.map_channels(|c| dehum(c, a.sample_rate, 60.0, 5)));
}
#[test]
fn golden_declick() {
    check("declick.wav", |a| a.map_channels(|c| declick(c, 10.0, 64)));
}
#[test]
fn golden_dewind() {
    check("dewind.wav", |a| a.map_channels(|c| dewind(c, a.sample_rate, 80.0)));
}
#[test]
fn golden_repair() {
    check("repair.wav", |a| a.map_channels(|c| spectral_repair(c, 4.0)));
}
#[test]
fn golden_enhance() {
    check("enhance.wav", |a| {
        let ch: Vec<Vec<f32>> =
            a.channels.iter().map(|c| bandwidth_extend(c, a.sample_rate, 48_000)).collect();
        AudioData { sample_rate: 48_000, channels: ch }
    });
}
#[test]
fn golden_declib() {
    let fs = 44_100u32;
    let n = fs as usize;
    let clip = 0.7f32;
    let truth: Vec<f32> =
        (0..n).map(|i| (2.0 * std::f32::consts::PI * 220.0 * i as f32 / fs as f32).sin()).collect();
    let clipped: Vec<f32> = truth.iter().map(|&v| v.clamp(-clip, clip)).collect();
    let input = AudioData { sample_rate: fs, channels: vec![clipped] };
    let out = input.map_channels(|c| declip(c, clip));
    let gold_path = golden_dir().join("declip.wav");
    assert!(gold_path.exists());
    let expected = std::fs::read(&gold_path).unwrap();
    let tmp = std::env::temp_dir().join("cathar_golden_declip.wav");
    out.to_file(tmp.to_str().unwrap()).unwrap();
    let actual = std::fs::read(&tmp).unwrap();
    assert_eq!(actual, expected);
}

// ── format round-trip tests ────────────────────────────────────────────────

#[test]
fn golden_flac_round_trip() {
    let input = test_signal();
    let path = std::env::temp_dir().join("cathar_golden.flac");
    let p = path.to_str().unwrap();
    input.to_file(p).unwrap();
    let back = AudioData::from_file(p).unwrap();
    std::fs::remove_file(p).ok();
    let err = input.channels[0]
        .iter()
        .zip(&back.channels[0])
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(err < 1e-4, "FLAC round-trip error {err}");
}

#[test]
fn golden_aiff_round_trip() {
    let input = test_signal();
    let path = std::env::temp_dir().join("cathar_golden.aiff");
    let p = path.to_str().unwrap();
    input.to_file(p).unwrap();
    let back = AudioData::from_file(p).unwrap();
    std::fs::remove_file(p).ok();
    let err = input.channels[0]
        .iter()
        .zip(&back.channels[0])
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(err < 1e-4, "AIFF round-trip error {err}");
}

#[test]
fn golden_wav_is_deterministic() {
    let input = test_signal();
    let path = std::env::temp_dir().join("cathar_golden_det.wav");
    let p = path.to_str().unwrap();
    input.to_file(p).unwrap();
    let first = std::fs::read(p).unwrap();
    input.to_file(p).unwrap();
    let second = std::fs::read(p).unwrap();
    std::fs::remove_file(p).ok();
    assert_eq!(first, second, "WAV output is not deterministic");
}
