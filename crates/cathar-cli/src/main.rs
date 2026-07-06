//! CLI for cathar — audio restoration toolbox.

mod banner;

use anyhow::Result;
use cathar::Denoiser;
use clap::{Parser, Subcommand};
use rayon::prelude::*;

#[cfg(feature = "tui")]
mod player;
#[cfg(feature = "tui")]
mod termcolor;
#[cfg(feature = "tui")]
mod tui;

#[derive(Debug, Clone, clap::ValueEnum)]
enum EnhanceMethodArg {
    Replicate,
    Interpolate,
}

impl From<EnhanceMethodArg> for cathar::EnhanceMethod {
    fn from(m: EnhanceMethodArg) -> Self {
        match m {
            EnhanceMethodArg::Replicate => cathar::EnhanceMethod::Replicate,
            EnhanceMethodArg::Interpolate => cathar::EnhanceMethod::Interpolate,
        }
    }
}

#[derive(Debug, Clone, clap::ValueEnum)]
enum StretchModeArg {
    /// Waveform-similarity overlap-add (robust default, no FFT).
    Wsola,
    /// Phase vocoder (smoother on tonal material).
    Pv,
}

impl From<StretchModeArg> for cathar::StretchMode {
    fn from(m: StretchModeArg) -> Self {
        match m {
            StretchModeArg::Wsola => cathar::StretchMode::Wsola,
            StretchModeArg::Pv => cathar::StretchMode::PhaseVocoder,
        }
    }
}

/// Audio restoration toolbox — denoise, de-hum, de-click, de-clip, normalise.
#[derive(Debug, Parser)]
#[command(
    name = "cathar",
    version,
    about = "Restore, enhance & level any recording — in pure Rust",
    long_about = None
)]
struct Cli {
    /// Suppress the startup banner.
    #[arg(long, global = true)]
    no_banner: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Denoise audio with spectral subtraction.
    Denoise {
        /// Input file (any format: WAV, MP3, MP4, M4A, MKV, FLAC, OGG)
        input: String,
        /// Output WAV file
        #[arg(short, long, default_value = "clean.wav")]
        out: String,
        /// Aggressiveness (1.0 = gentle, 6.0 = aggressive)
        #[arg(short, long, default_value_t = 3.0)]
        alpha: f32,
        /// Spectral floor (0.0–0.1, higher = less artifacts)
        #[arg(short = 'b', long, default_value_t = 0.01)]
        beta: f32,
        /// Pre-computed noise print (from `noiseprint` command)
        #[arg(long)]
        noiseprint: Option<String>,
        /// Use Wiener filter instead of spectral subtraction
        #[arg(long)]
        wiener: bool,
        /// Phase-coherent stereo: one shared gain mask keeps the stereo image stable
        #[arg(long)]
        coherent: bool,
    },
    /// Learn a noise profile from a silence/noise-only recording.
    Noiseprint {
        /// Input file containing only noise/silence
        input: String,
        /// Output noise print file (JSON)
        #[arg(short, long, default_value = "noise.np.json")]
        out: String,
    },
    /// Remove mains hum (50/60 Hz + harmonics).
    Dehum {
        /// Input file
        input: String,
        /// Output WAV file
        #[arg(short, long, default_value = "dehummed.wav")]
        out: String,
        /// Base frequency (50 or 60 Hz)
        #[arg(short = 'f', long, default_value_t = 60.0)]
        freq: f32,
        /// Number of harmonics to notch
        #[arg(short = 'n', long, default_value_t = 5)]
        harmonics: usize,
    },
    /// Detect and remove impulse clicks (vinyl rips, bad edits).
    Declick {
        /// Input file
        input: String,
        /// Output WAV file
        #[arg(short, long, default_value = "declicked.wav")]
        out: String,
        /// Detection threshold (higher = less sensitive)
        #[arg(short, long, default_value_t = 10.0)]
        threshold: f32,
    },
    /// Reconstruct clipped peaks (overdriven recordings).
    Declip {
        /// Input file
        input: String,
        /// Output WAV file
        #[arg(short, long, default_value = "declipped.wav")]
        out: String,
        /// Clipping threshold (0.0–1.0)
        #[arg(short, long, default_value_t = 0.95)]
        threshold: f32,
    },
    /// Remove room echo and reverb from recordings.
    Dereverb {
        /// Input file
        input: String,
        /// Output WAV file
        #[arg(short, long, default_value = "dereverbed.wav")]
        out: String,
        /// Strength (1.0 = gentle, 5.0 = aggressive)
        #[arg(short, long, default_value_t = 2.0)]
        strength: f32,
    },
    /// Isolate speech from background noise.
    Voiceisolate {
        /// Input file
        input: String,
        /// Output WAV file
        #[arg(short, long, default_value = "isolated.wav")]
        out: String,
        /// Optional noise print for cleaner separation
        #[arg(long)]
        noiseprint: Option<String>,
    },
    /// Reduce harsh sibilance (s, sh, ch sounds).
    Deesser {
        /// Input file
        input: String,
        /// Output WAV file
        #[arg(short, long, default_value = "deessed.wav")]
        out: String,
        /// Crossover frequency in Hz
        #[arg(short, long, default_value_t = 4000.0)]
        freq: f32,
        /// Threshold in dB (single-band: vs HF/broadband ratio; multiband: dB above each band's running average — try 6)
        #[arg(short, long, default_value_t = -24.0)]
        threshold: f32,
        /// Split the sibilant region into N adaptive sub-bands (1 = classic single-band)
        #[arg(long, default_value_t = 1)]
        bands: usize,
    },
    /// Remove low-frequency wind rumble (high-pass).
    Dewind {
        /// Input file
        input: String,
        /// Output WAV file
        #[arg(short, long, default_value = "dewinded.wav")]
        out: String,
        /// High-pass cutoff in Hz
        #[arg(short, long, default_value_t = 80.0)]
        cutoff: f32,
    },
    /// Tame plosive pops ("p"/"b" low-frequency bursts).
    Deplosive {
        /// Input file
        input: String,
        /// Output WAV file
        #[arg(short, long, default_value = "deplosived.wav")]
        out: String,
        /// Aggressiveness 1–10
        #[arg(short, long, default_value_t = 4.0)]
        strength: f32,
    },
    /// Suppress lavalier / clothing rustle (mid-band transient bursts).
    Derustle {
        /// Input file
        input: String,
        /// Output WAV file
        #[arg(short, long, default_value = "derustled.wav")]
        out: String,
        /// Aggressiveness 1–10
        #[arg(short, long, default_value_t = 4.0)]
        strength: f32,
    },
    /// Attenuate breath sounds between speech segments.
    Breath {
        /// Input file
        input: String,
        /// Output WAV file
        #[arg(short, long, default_value = "breathless.wav")]
        out: String,
    },
    /// Split into harmonic (tonal) and percussive (transient) layers (HPSS).
    Hpss {
        /// Input file
        input: String,
        /// Harmonic output WAV file
        #[arg(long, default_value = "harmonic.wav")]
        harmonic: String,
        /// Percussive output WAV file
        #[arg(long, default_value = "percussive.wav")]
        percussive: String,
        /// Median-filter length (odd, ≥ 3)
        #[arg(short, long, default_value_t = 17)]
        kernel: usize,
    },
    /// Apply RIAA playback de-emphasis to a digitized vinyl recording.
    Riaa {
        /// Input file
        input: String,
        /// Output WAV file
        #[arg(short, long, default_value = "riaa.wav")]
        out: String,
        /// Elliptical mono crossover (Hz); sums lows to mono below this frequency
        #[arg(long)]
        elliptical: Option<f32>,
    },
    /// Reduce quantization grain from low-bit-depth sources.
    Dequantize {
        /// Input file
        input: String,
        /// Output WAV file
        #[arg(short, long, default_value = "dequantized.wav")]
        out: String,
        /// Assumed source bit depth (4–24)
        #[arg(short, long, default_value_t = 16)]
        bits: u32,
        /// Correction strength 0–1
        #[arg(short, long, default_value_t = 0.7)]
        strength: f32,
    },
    /// Restore high frequencies lost to compression or low sample rates.
    Enhance {
        /// Input file
        input: String,
        /// Output WAV file
        #[arg(short, long, default_value = "enhanced.wav")]
        out: String,
        /// Target sample rate (Hz)
        #[arg(short, long, default_value_t = 48000)]
        rate: u32,
        /// Upsampling strategy: replicate (SBR) or interpolate (log-magnitude extrapolation)
        #[arg(long, default_value = "replicate")]
        method: EnhanceMethodArg,
    },
    /// Repair isolated transient spectral artifacts (whistles, bursts, glitches).
    Repair {
        /// Input file
        input: String,
        /// Output WAV file
        #[arg(short, long, default_value = "repaired.wav")]
        out: String,
        /// Aggressiveness 1–10 (higher removes more)
        #[arg(short, long, default_value_t = 4.0)]
        strength: f32,
    },
    /// Resample to a different sample rate (anti-aliased, any ratio).
    Resample {
        /// Input file
        input: String,
        /// Output WAV file
        #[arg(short, long, default_value = "resampled.wav")]
        out: String,
        /// Target sample rate (Hz)
        #[arg(short, long, default_value_t = 48000)]
        rate: u32,
    },
    /// Change tempo (duration) without changing pitch.
    Tempo {
        /// Input file
        input: String,
        /// Output WAV file
        #[arg(short, long, default_value = "tempo.wav")]
        out: String,
        /// Speed factor: >1 faster/shorter, <1 slower/longer
        #[arg(short, long, default_value_t = 1.5)]
        factor: f32,
        /// Stretch engine
        #[arg(long, default_value = "wsola")]
        mode: StretchModeArg,
    },
    /// Shift pitch (semitones) without changing duration.
    Pitch {
        /// Input file
        input: String,
        /// Output WAV file
        #[arg(short, long, default_value = "pitched.wav")]
        out: String,
        /// Semitones to shift (negative = down)
        #[arg(short, long, default_value_t = 2.0, allow_hyphen_values = true)]
        semitones: f32,
        /// Stretch engine
        #[arg(long, default_value = "wsola")]
        mode: StretchModeArg,
    },
    /// Change speed (resample): alters both pitch and duration, like tape.
    Speed {
        /// Input file
        input: String,
        /// Output WAV file
        #[arg(short, long, default_value = "speed.wav")]
        out: String,
        /// Speed factor: >1 faster/higher, <1 slower/lower
        #[arg(short, long, default_value_t = 1.5)]
        factor: f32,
    },
    /// Normalize loudness or peak level.
    Normalize {
        /// Input file
        input: String,
        /// Output WAV file
        #[arg(short, long, default_value = "normalized.wav")]
        out: String,
        /// Target level in LUFS/dBFS (-23 = broadcast, -16 = podcast, -14 = streaming)
        #[arg(short, long, default_value_t = -16.0, allow_hyphen_values = true)]
        target: f32,
        /// Use peak normalization instead of loudness
        #[arg(long)]
        peak: bool,
        /// True-peak ceiling in dBTP for loudness mode (gain is held back to respect it)
        #[arg(long, default_value_t = -1.0, allow_hyphen_values = true)]
        true_peak: f32,
    },
    /// Generate a synthetic waveform for testing.
    Wave {
        #[arg(short, long, default_value = "test.wav")]
        out: String,
        #[arg(short, long, default_value = "44100")]
        sample_rate: u32,
        #[arg(short, long, default_value_t = 440.0)]
        freq: f32,
        #[arg(short, long, default_value_t = 3.0)]
        duration: f32,
        #[arg(short, long, default_value_t = 0.1)]
        noise: f32,
    },
    /// Batch process all audio files in a directory.
    Batch {
        /// Input directory
        #[arg(short, long, default_value = ".")]
        indir: String,
        /// Output directory
        #[arg(short, long, default_value = "clean")]
        outdir: String,
        /// Aggressiveness
        #[arg(short, long, default_value_t = 3.0)]
        alpha: f32,
        /// Spectral floor
        #[arg(short = 'b', long, default_value_t = 0.01)]
        beta: f32,
        /// Also de-hum (specify base frequency, e.g. 60)
        #[arg(long)]
        dehum: Option<f32>,
        /// Also normalize to this LUFS level
        #[arg(long)]
        normalize: Option<f32>,
        /// Extensions to process (comma-separated)
        #[arg(long, default_value = "wav,mp3,mp4,m4a,mkv,flac,ogg,aac")]
        exts: String,
    },
    /// View a spectrogram in the terminal (interactive heatmap).
    #[cfg(feature = "tui")]
    View {
        /// Input file (any supported format)
        input: String,
        /// FFT size (frequency resolution = sample_rate / fft)
        #[arg(long, default_value_t = 2048)]
        fft: usize,
        /// Hop between frames in samples (smaller = more time detail)
        #[arg(long, default_value_t = 512)]
        hop: usize,
    },
    /// Trim audio to a time range.
    Trim {
        /// Input file
        input: String,
        /// Output file
        #[arg(short, long, default_value = "trimmed.wav")]
        out: String,
        /// Start time in seconds
        #[arg(short, long, default_value_t = 0.0)]
        start: f32,
        /// Duration in seconds
        #[arg(short, long)]
        duration: f32,
    },
    /// Pad audio with silence at start and/or end.
    Pad {
        /// Input file
        input: String,
        /// Output file
        #[arg(short, long, default_value = "padded.wav")]
        out: String,
        /// Seconds of silence to prepend
        #[arg(long, default_value_t = 0.0)]
        pre: f32,
        /// Seconds of silence to append
        #[arg(long, default_value_t = 0.0)]
        post: f32,
    },
    /// Apply a linear fade-in and/or fade-out.
    Fade {
        /// Input file
        input: String,
        /// Output file
        #[arg(short, long, default_value = "faded.wav")]
        out: String,
        /// Fade-in duration in seconds
        #[arg(long, default_value_t = 0.05)]
        fade_in: f32,
        /// Fade-out duration in seconds
        #[arg(long, default_value_t = 0.1)]
        fade_out: f32,
    },
    /// Strip silence from the start and end of audio.
    Silence {
        /// Input file
        input: String,
        /// Output file
        #[arg(short, long, default_value = "silenced.wav")]
        out: String,
        /// Amplitude threshold below which is considered silence (0.0-1.0)
        #[arg(long, default_value_t = 0.01)]
        threshold: f32,
        /// Minimum silent duration in seconds before trimming
        #[arg(long, default_value_t = 0.1)]
        min_duration: f32,
    },
    /// Apply gain in dB.
    Gain {
        /// Input file
        input: String,
        /// Output file
        #[arg(short, long, default_value = "gained.wav")]
        out: String,
        /// Gain in dB (positive = boost, negative = cut)
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        db: f32,
    },
    /// Remix channels (stereo → mono, swap L/R, custom mapping).
    Remix {
        /// Input file
        input: String,
        /// Output file
        #[arg(short, long, default_value = "remixed.wav")]
        out: String,
        /// How to remix: 'mono', 'swap', or comma-separated channel indices
        #[arg(long, default_value = "mono")]
        layout: String,
    },
    /// Select a subset of channels.
    Channels {
        /// Input file
        input: String,
        /// Output file
        #[arg(short, long, default_value = "channeled.wav")]
        out: String,
        /// Comma-separated 0-based channel indices (e.g. "0" for left, "1" for right)
        #[arg(long, default_value = "0")]
        indices: String,
    },
    /// Reverse audio in time.
    Reverse {
        /// Input file
        input: String,
        /// Output file
        #[arg(short, long, default_value = "reversed.wav")]
        out: String,
    },
    /// Apply a low-pass filter (remove high frequencies).
    Lowpass {
        input: String,
        #[arg(short, long, default_value = "lowpassed.wav")]
        out: String,
        /// Cutoff frequency in Hz
        #[arg(short, long)]
        freq: f32,
    },
    /// Apply a high-pass filter (remove low frequencies).
    Highpass {
        input: String,
        #[arg(short, long, default_value = "highpassed.wav")]
        out: String,
        /// Cutoff frequency in Hz
        #[arg(short, long)]
        freq: f32,
    },
    /// Apply a band-pass filter.
    Bandpass {
        input: String,
        #[arg(short, long, default_value = "bandpassed.wav")]
        out: String,
        /// Center frequency in Hz
        #[arg(short, long)]
        freq: f32,
        /// Q factor (bandwidth = freq / Q)
        #[arg(long, default_value_t = 1.0)]
        q: f32,
    },
    /// Apply a peaking (bell) EQ filter.
    Equalizer {
        input: String,
        #[arg(short, long, default_value = "eqd.wav")]
        out: String,
        /// Center frequency in Hz
        #[arg(short, long)]
        freq: f32,
        /// Q factor
        #[arg(long, default_value_t = 1.0)]
        q: f32,
        /// Gain in dB (positive = boost, negative = cut)
        #[arg(short, long, allow_hyphen_values = true)]
        gain: f32,
    },
    /// Apply a low-shelf filter (boost/cut bass).
    Bass {
        input: String,
        #[arg(short, long, default_value = "bassed.wav")]
        out: String,
        /// Cutoff frequency in Hz
        #[arg(short, long)]
        freq: f32,
        /// Gain in dB
        #[arg(short, long, allow_hyphen_values = true, default_value_t = 3.0)]
        gain: f32,
    },
    /// Apply a high-shelf filter (boost/cut treble).
    Treble {
        input: String,
        #[arg(short, long, default_value = "trebled.wav")]
        out: String,
        /// Cutoff frequency in Hz
        #[arg(short, long)]
        freq: f32,
        /// Gain in dB
        #[arg(short, long, allow_hyphen_values = true, default_value_t = 3.0)]
        gain: f32,
    },
    /// Apply a dynamic range compressor.
    Compress {
        input: String,
        #[arg(short, long, default_value = "compressed.wav")]
        out: String,
        /// Threshold in dBFS (e.g. -20)
        #[arg(short, long, default_value_t = -20.0, allow_hyphen_values = true)]
        threshold: f32,
        /// Compression ratio (e.g. 2 for 2:1)
        #[arg(long, default_value_t = 2.0)]
        ratio: f32,
        /// Attack time in seconds
        #[arg(long, default_value_t = 0.01)]
        attack: f32,
        /// Release time in seconds
        #[arg(long, default_value_t = 0.1)]
        release: f32,
    },
    /// Apply a brickwall limiter.
    Limit {
        input: String,
        #[arg(short, long, default_value = "limited.wav")]
        out: String,
        /// Ceiling in dBFS
        #[arg(short, long, default_value_t = -1.0, allow_hyphen_values = true)]
        ceiling: f32,
    },
    /// Apply a noise gate.
    Gate {
        input: String,
        #[arg(short, long, default_value = "gated.wav")]
        out: String,
        /// Threshold in dBFS (e.g. -40)
        #[arg(short, long, default_value_t = -40.0, allow_hyphen_values = true)]
        threshold: f32,
        /// Attack time in seconds
        #[arg(long, default_value_t = 0.001)]
        attack: f32,
        /// Release time in seconds
        #[arg(long, default_value_t = 0.1)]
        release: f32,
    },
    /// Apply TPDF dither (for bit-depth reduction).
    Dither {
        /// Input file
        input: String,
        /// Output file
        #[arg(short, long, default_value = "dithered.wav")]
        out: String,
        /// Target bit depth (e.g. 16)
        #[arg(long, default_value_t = 16)]
        bits: u32,
    },
    /// Print audio statistics (peak, RMS, LUFS, true-peak, crest factor, …).
    Stats {
        /// Input file
        input: String,
    },
    /// Convert between audio formats (WAV, FLAC, AIFF, MP3, …) without processing.
    Convert {
        /// Input file (any format: WAV, MP3, MP4, M4A, MKV, FLAC, OGG, AIFF)
        input: String,
        /// Output file (format chosen by extension: .wav, .flac, .aiff)
        #[arg(short, long)]
        out: String,
    },
    /// Neural spectral-gain denoise (candle GRU). Requires `--features ml`.
    #[cfg(feature = "ml")]
    MlDenoise {
        /// Input file (any format: WAV, MP3, MP4, M4A, MKV, FLAC, OGG)
        input: String,
        /// Output WAV file
        #[arg(short, long, default_value = "clean.wav")]
        out: String,
        /// Path to a custom `.safetensors` checkpoint. Omit to use the bundled
        /// pretrained model (synthetic tones; retrain on DNS-Challenge for speech).
        #[arg(long)]
        weights: Option<String>,
        /// Use the deterministic passthrough model instead of the pretrained one.
        #[arg(long, conflicts_with = "weights")]
        passthrough: bool,
    },
    /// Play a file with a live spectrum-analyzer visualizer (Winamp-style).
    #[cfg(feature = "tui")]
    Play {
        /// Input file (any supported format)
        input: String,
        /// FFT size for the analyzer (larger = finer frequency bands)
        #[arg(long, default_value_t = 2048)]
        fft: usize,
    },
}

fn main() -> Result<()> {
    if !std::env::args().any(|a| a == "--no-banner") {
        banner::print();
    }

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Denoise { input, out, alpha, beta, noiseprint, wiener, coherent } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let orig_power = power(&audio.channels[0]);

            eprintln!(
                "{}  {} Hz  {} ch  {:.1}s",
                input,
                audio.sample_rate,
                audio.channels.len(),
                audio.channels[0].len() as f32 / audio.sample_rate as f32
            );

            let clean = if let Some(np_path) = noiseprint {
                let np_json = std::fs::read_to_string(&np_path)?;
                let np: Vec<f32> = serde_json::from_str(&np_json)?;
                let noise_print = cathar::NoisePrint { fft_size: 2048, spectrum: np };
                let denoiser = cathar::SpectralDenoiser::with_noise_print(noise_print, alpha, beta);
                if coherent {
                    denoiser.denoise_coherent(&audio)?
                } else {
                    denoiser.denoise(&audio)?
                }
            } else if wiener {
                let np = cathar::learn_noise_print(&audio)?;
                let output = cathar::wiener_denoise(&audio.channels[0], &np, alpha)?;
                cathar::AudioData { sample_rate: audio.sample_rate, channels: vec![output] }
            } else {
                let denoiser = cathar::SpectralDenoiser { alpha, beta, ..Default::default() };
                if coherent {
                    denoiser.denoise_coherent(&audio)?
                } else {
                    denoiser.denoise(&audio)?
                }
            };

            clean.to_file(&out)?;
            let clean_power = power(&clean.channels[0]);
            if let (Some(o), Some(c)) = (orig_power, clean_power) {
                if o > 0.0 {
                    let db = -10.0 * (c / o).log10();
                    eprintln!("reduction  {:.1}%  ({db:.1} dB)", (1.0 - c / o) * 100.0);
                }
            }
            eprintln!("wrote  {out}");
        }

        Command::Noiseprint { input, out } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let np = cathar::learn_noise_print(&audio)?;
            let json = serde_json::to_string(&np.spectrum)?;
            std::fs::write(&out, json)?;
            eprintln!(
                "{}  {} Hz  {:.1}s  →  {} ({} bins, FFT={})",
                input,
                audio.sample_rate,
                audio.channels[0].len() as f32 / audio.sample_rate as f32,
                out,
                np.spectrum.len(),
                np.fft_size
            );
        }

        Command::Dehum { input, out, freq, harmonics } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let cleaned =
                audio.map_channels(|c| cathar::dehum(c, audio.sample_rate, freq, harmonics));
            cleaned.to_file(&out)?;
            eprintln!("de-hummed  {freq} Hz + {harmonics} harmonics  →  {out}");
        }

        Command::Declick { input, out, threshold } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let cleaned = audio.map_channels(|c| cathar::declick(c, threshold, 64));
            cleaned.to_file(&out)?;
            eprintln!("de-clicked  threshold={threshold}  →  {out}");
        }

        Command::Declip { input, out, threshold } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let cleaned = audio.map_channels(|c| cathar::declip(c, threshold));
            cleaned.to_file(&out)?;
            eprintln!("de-clipped  threshold={threshold}  →  {out}");
        }
        Command::Dereverb { input, out, strength } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let cleaned = audio.map_channels(|c| cathar::dereverb(c, audio.sample_rate, strength));
            cleaned.to_file(&out)?;
            eprintln!("de-reverbed  strength={strength}  →  {out}");
        }
        Command::Voiceisolate { input, out, noiseprint } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let np = if let Some(ref path) = noiseprint {
                let json = std::fs::read_to_string(path)?;
                let spectrum: Vec<f32> = serde_json::from_str(&json)?;
                Some(cathar::NoisePrint { fft_size: 2048, spectrum })
            } else {
                None
            };
            let cleaned =
                audio.map_channels(|c| cathar::voice_isolate(c, audio.sample_rate, np.as_ref()));
            cleaned.to_file(&out)?;
            eprintln!("voice-isolated  →  {out}");
        }
        Command::Deesser { input, out, freq, threshold, bands } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let cleaned = audio.map_channels(|c| {
                if bands > 1 {
                    cathar::deess_multiband(c, audio.sample_rate, freq, threshold, 4.0, bands)
                } else {
                    cathar::deesser(c, audio.sample_rate, freq, threshold, 3.0)
                }
            });
            cleaned.to_file(&out)?;
            let mode =
                if bands > 1 { format!("{bands}-band adaptive") } else { "single-band".into() };
            eprintln!(
                "de-essed  crossover={freq} Hz  threshold={threshold} dB  ({mode})  →  {out}"
            );
        }
        Command::Dewind { input, out, cutoff } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let cleaned = audio.map_channels(|c| cathar::dewind(c, audio.sample_rate, cutoff));
            cleaned.to_file(&out)?;
            eprintln!("de-winded  high-pass {cutoff} Hz  →  {out}");
        }
        Command::Deplosive { input, out, strength } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let cleaned = audio.map_channels(|c| cathar::deplosive(c, audio.sample_rate, strength));
            cleaned.to_file(&out)?;
            eprintln!("de-plosived  strength={strength}  →  {out}");
        }
        Command::Derustle { input, out, strength } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let cleaned = audio.map_channels(|c| cathar::derustle(c, audio.sample_rate, strength));
            cleaned.to_file(&out)?;
            eprintln!("de-rustled  strength={strength}  →  {out}");
        }
        Command::Breath { input, out } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let cleaned = audio.map_channels(|c| cathar::breath_remove(c, audio.sample_rate));
            cleaned.to_file(&out)?;
            eprintln!("breath-removed  →  {out}");
        }
        Command::Hpss { input, harmonic, percussive, kernel } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let sr = audio.sample_rate;
            let (mut hs, mut ps) = (Vec::new(), Vec::new());
            for c in &audio.channels {
                let (h, p) = cathar::hpss(c, sr, kernel);
                hs.push(h);
                ps.push(p);
            }
            cathar::AudioData { sample_rate: sr, channels: hs }.to_file(&harmonic)?;
            cathar::AudioData { sample_rate: sr, channels: ps }.to_file(&percussive)?;
            eprintln!("HPSS (kernel {kernel})  →  {harmonic}  +  {percussive}");
        }
        Command::Riaa { input, out, elliptical } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let channels = if audio.channels.len() >= 2 {
                let (l, r) = cathar::vinyl_restore(
                    &audio.channels[0],
                    &audio.channels[1],
                    audio.sample_rate,
                    elliptical,
                );
                vec![l, r]
            } else {
                vec![cathar::riaa_deemphasis(&audio.channels[0], audio.sample_rate)]
            };
            cathar::AudioData { sample_rate: audio.sample_rate, channels }.to_file(&out)?;
            let ellip = elliptical.map(|f| format!("  elliptical {f} Hz")).unwrap_or_default();
            eprintln!("RIAA de-emphasis{ellip}  →  {out}");
        }

        Command::Dequantize { input, out, bits, strength } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let cleaned =
                audio.map_channels(|c| cathar::dequantize(c, audio.sample_rate, bits, strength));
            cleaned.to_file(&out)?;
            eprintln!("dequantized  {bits}-bit  strength={strength}  →  {out}");
        }

        Command::Enhance { input, out, rate, method } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let method: cathar::EnhanceMethod = method.into();
            let channels: Vec<Vec<f32>> = audio
                .channels
                .iter()
                .map(|c| cathar::bandwidth_extend_with_method(c, audio.sample_rate, rate, method))
                .collect();
            let result = cathar::AudioData { sample_rate: rate, channels };
            result.to_file(&out)?;
            let mode = match method {
                cathar::EnhanceMethod::Replicate => "replicate",
                cathar::EnhanceMethod::Interpolate => "interpolate",
            };
            eprintln!("enhanced  {} Hz → {rate} Hz  ({mode})  →  {out}", audio.sample_rate);
        }

        Command::Repair { input, out, strength } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let cleaned = audio.map_channels(|c| cathar::spectral_repair(c, strength));
            cleaned.to_file(&out)?;
            eprintln!("repaired  strength={strength}  →  {out}");
        }

        Command::Resample { input, out, rate } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let from = audio.sample_rate;
            let resampled = audio.resample(rate);
            resampled.to_file(&out)?;
            eprintln!("resampled  {from} Hz → {rate} Hz  →  {out}");
        }

        Command::Tempo { input, out, factor, mode } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let sr = audio.sample_rate;
            let mode: cathar::StretchMode = mode.into();
            let ratio = 1.0 / factor; // factor>1 = faster ⇒ shorter output
            let stretched = audio.map_channels(|c| cathar::time_stretch(c, sr, ratio, mode));
            stretched.to_file(&out)?;
            eprintln!("tempo ×{factor}  →  {out}");
        }

        Command::Pitch { input, out, semitones, mode } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let sr = audio.sample_rate;
            let mode: cathar::StretchMode = mode.into();
            let shifted = audio.map_channels(|c| cathar::pitch_shift(c, sr, semitones, mode));
            shifted.to_file(&out)?;
            eprintln!("pitch {semitones:+} st  →  {out}");
        }

        Command::Speed { input, out, factor } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let sr = audio.sample_rate;
            // Resample by 1/factor and keep the original rate: faster ⇒ higher.
            let from = (sr as f32 * factor).round().max(1.0) as u32;
            let channels: Vec<Vec<f32>> =
                audio.channels.iter().map(|c| cathar::resample(c, from, sr)).collect();
            cathar::AudioData { sample_rate: sr, channels }.to_file(&out)?;
            eprintln!("speed ×{factor}  →  {out}");
        }

        Command::Normalize { input, out, target, peak, true_peak } => {
            let audio = cathar::AudioData::from_file(&input)?;
            if peak {
                let cleaned = audio.map_channels(|c| cathar::normalize_peak(c, target));
                cleaned.to_file(&out)?;
                eprintln!("normalized  {target} dBFS peak  →  {out}");
            } else {
                let before = cathar::integrated_loudness(&audio.channels, audio.sample_rate);
                let cleaned = audio.normalize_r128(target, true_peak);
                let after = cathar::integrated_loudness(&cleaned.channels, cleaned.sample_rate);
                let tp = cathar::true_peak_dbtp(&cleaned.channels, cleaned.sample_rate);
                cleaned.to_file(&out)?;
                eprintln!(
                    "normalized  {before:.1} → {after:.1} LUFS  (target {target}, true peak {tp:.1} dBTP ≤ {true_peak})  →  {out}"
                );
            }
        }

        Command::Wave { out, sample_rate, freq, duration, noise } => {
            let audio = cathar::generate_wave(sample_rate, freq, duration, noise);
            audio.to_file(&out)?;
            eprintln!(
                "{}  {} Hz  {:.1}s  f={freq} Hz  noise={noise:.2}",
                out, sample_rate, duration
            );
        }

        Command::Batch { indir, outdir, alpha, beta, dehum, normalize, exts } => {
            std::fs::create_dir_all(&outdir)?;
            let extensions: Vec<&str> = exts.split(',').map(|s| s.trim()).collect();
            let mut files: Vec<_> = std::fs::read_dir(&indir)?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.extension()
                        .and_then(|e| e.to_str())
                        .map(|e| extensions.contains(&e.to_lowercase().as_str()))
                        .unwrap_or(false)
                })
                .collect();
            files.sort();

            let total = files.len();
            let done = std::sync::atomic::AtomicUsize::new(0);

            // Files are independent — each reads, processes, and writes its own
            // output — so fan out across the rayon thread pool. Per-file errors
            // are reported and skipped rather than aborting the whole batch.
            files.par_iter().for_each(|path| {
                let name = path.file_stem().unwrap().to_string_lossy();

                let process = || -> Result<()> {
                    let audio = cathar::AudioData::from_file(&path.to_string_lossy())?;
                    let denoiser = cathar::SpectralDenoiser { alpha, beta, ..Default::default() };
                    let mut clean = denoiser.denoise(&audio)?;

                    if let Some(freq) = dehum {
                        clean =
                            clean.map_channels(|c| cathar::dehum(c, clean.sample_rate, freq, 5));
                    }
                    if let Some(lu) = normalize {
                        clean = clean.normalize_r128(lu, -1.0);
                    }

                    let out_path = std::path::Path::new(&outdir).join(format!("{name}.wav"));
                    clean.to_file(&out_path.to_string_lossy())?;
                    Ok(())
                };

                let i = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                match process() {
                    Ok(()) => eprintln!("[{i}/{total}] {name}"),
                    Err(e) => eprintln!("[{i}/{total}] {name}  skip: {e}"),
                }
            });
            eprintln!("done  {total} files  →  {outdir}/");
        }
        #[cfg(feature = "tui")]
        Command::View { input, fft, hop } => {
            tui::run(&input, fft, hop)?;
        }
        Command::Trim { input, out, start, duration } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let trimmed = audio.trim(start, duration);
            trimmed.to_file(&out)?;
            eprintln!("trimmed  {start}s +{duration}s  →  {out}");
        }
        Command::Pad { input, out, pre, post } => {
            let audio = cathar::AudioData::from_file(&input)?;
            audio.pad_extend(pre, post).to_file(&out)?;
            eprintln!("padded  +{pre}s / +{post}s  →  {out}");
        }
        Command::Fade { input, out, fade_in, fade_out } => {
            let audio = cathar::AudioData::from_file(&input)?;
            audio.fade(fade_in, fade_out).to_file(&out)?;
            eprintln!("faded  in={fade_in}s out={fade_out}s  →  {out}");
        }
        Command::Silence { input, out, threshold, min_duration } => {
            let audio = cathar::AudioData::from_file(&input)?;
            audio.silence_strip(threshold, min_duration).to_file(&out)?;
            eprintln!("silence-stripped  threshold={threshold} min={min_duration}s  →  {out}");
        }
        Command::Gain { input, out, db } => {
            let audio = cathar::AudioData::from_file(&input)?;
            audio.gain_db(db).to_file(&out)?;
            eprintln!("gain  {db} dB  →  {out}");
        }
        Command::Remix { input, out, layout } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let spec: Vec<Vec<(usize, f32)>> = match layout.as_str() {
                "mono" => {
                    let n = audio.channels.len();
                    vec![(0..n).map(|i| (i, 1.0 / n as f32)).collect()]
                }
                "swap" if audio.channels.len() >= 2 => {
                    vec![vec![(1, 1.0)], vec![(0, 1.0)]]
                }
                "swap" => {
                    anyhow::bail!("swap needs at least 2 channels");
                }
                other => {
                    anyhow::bail!("unknown layout: {other}. Use 'mono' or 'swap'")
                }
            };
            audio.remix(&spec).to_file(&out)?;
            eprintln!("remixed  {layout}  →  {out}");
        }
        Command::Channels { input, out, indices } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let ids: Vec<usize> =
                indices.split(',').filter_map(|s| s.trim().parse().ok()).collect();
            if ids.is_empty() {
                anyhow::bail!("no valid channel indices in: {indices}");
            }
            let out_audio = cathar::AudioData {
                sample_rate: audio.sample_rate,
                channels: cathar::select_channels(&audio.channels, &ids),
            };
            out_audio.to_file(&out)?;
            eprintln!("channels  {:?}  →  {out}", ids);
        }
        Command::Reverse { input, out } => {
            let audio = cathar::AudioData::from_file(&input)?;
            audio.reverse().to_file(&out)?;
            eprintln!("reversed  →  {out}");
        }
        Command::Lowpass { input, out, freq } => {
            let audio = cathar::AudioData::from_file(&input)?;
            audio.map_channels(|c| cathar::lowpass(c, audio.sample_rate, freq)).to_file(&out)?;
            eprintln!("lowpass  {freq} Hz  →  {out}");
        }
        Command::Highpass { input, out, freq } => {
            let audio = cathar::AudioData::from_file(&input)?;
            audio.map_channels(|c| cathar::highpass(c, audio.sample_rate, freq)).to_file(&out)?;
            eprintln!("highpass  {freq} Hz  →  {out}");
        }
        Command::Bandpass { input, out, freq, q } => {
            let audio = cathar::AudioData::from_file(&input)?;
            audio
                .map_channels(|c| cathar::bandpass(c, audio.sample_rate, freq, q))
                .to_file(&out)?;
            eprintln!("bandpass  {freq} Hz  Q={q}  →  {out}");
        }
        Command::Equalizer { input, out, freq, q, gain } => {
            let audio = cathar::AudioData::from_file(&input)?;
            audio
                .map_channels(|c| cathar::equalizer(c, audio.sample_rate, freq, q, gain))
                .to_file(&out)?;
            eprintln!("eq  {freq} Hz  Q={q}  {gain} dB  →  {out}");
        }
        Command::Bass { input, out, freq, gain } => {
            let audio = cathar::AudioData::from_file(&input)?;
            audio.map_channels(|c| cathar::bass(c, audio.sample_rate, freq, gain)).to_file(&out)?;
            eprintln!("bass  {freq} Hz  {gain} dB  →  {out}");
        }
        Command::Treble { input, out, freq, gain } => {
            let audio = cathar::AudioData::from_file(&input)?;
            audio
                .map_channels(|c| cathar::treble(c, audio.sample_rate, freq, gain))
                .to_file(&out)?;
            eprintln!("treble  {freq} Hz  {gain} dB  →  {out}");
        }
        Command::Compress { input, out, threshold, ratio, attack, release } => {
            let audio = cathar::AudioData::from_file(&input)?;
            audio
                .map_channels(|c| {
                    cathar::compressor(c, audio.sample_rate, threshold, ratio, attack, release)
                })
                .to_file(&out)?;
            eprintln!("compressed  {threshold} dB  {ratio}:1  →  {out}");
        }
        Command::Limit { input, out, ceiling } => {
            let audio = cathar::AudioData::from_file(&input)?;
            audio.map_channels(|c| cathar::limiter(c, audio.sample_rate, ceiling)).to_file(&out)?;
            eprintln!("limited  ceiling={ceiling} dBFS  →  {out}");
        }
        Command::Gate { input, out, threshold, attack, release } => {
            let audio = cathar::AudioData::from_file(&input)?;
            audio
                .map_channels(|c| cathar::gate(c, audio.sample_rate, threshold, attack, release))
                .to_file(&out)?;
            eprintln!("gated  {threshold} dBFS  →  {out}");
        }
        Command::Dither { input, out, bits } => {
            let audio = cathar::AudioData::from_file(&input)?;
            audio.dither(bits).to_file(&out)?;
            eprintln!("dithered  {bits}-bit TPDF  →  {out}");
        }
        Command::Stats { input } => {
            let audio = cathar::AudioData::from_file(&input)?;
            if let Some(stats) = cathar::compute_stats(&audio.channels, audio.sample_rate) {
                println!("{input}");
                println!("  Sample rate  {:>8} Hz", stats.sample_rate);
                println!("  Channels     {:>8}", stats.channels);
                println!("  Duration     {:>8.2} s", stats.duration_sec);
                println!("  Samples      {:>8}", stats.samples);
                println!("  ────────────────────────");
                println!("  Peak         {:>8.1} dBFS", stats.peak_dbfs);
                println!("  RMS          {:>8.1} dBFS", stats.rms_dbfs);
                println!("  Crest factor {:>8.1} dB", stats.crest_factor_db);
                println!("  Integrated   {:>8.1} LUFS", stats.integrated_lufs);
                println!("  True peak    {:>8.1} dBTP", stats.true_peak_dbtp);
                println!("  DC offset    {:>8.4}", stats.dc_offset);
                // Monophonic pitch estimate over a mono mixdown.
                let n = audio.channels.iter().map(Vec::len).max().unwrap_or(0);
                let nch = audio.channels.len().max(1);
                let mut mono = vec![0.0f32; n];
                for ch in &audio.channels {
                    for (i, &s) in ch.iter().enumerate() {
                        mono[i] += s / nch as f32;
                    }
                }
                match cathar::fundamental_hz(&mono, audio.sample_rate) {
                    Some(f0) => println!("  Pitch (f0)   {f0:>8.1} Hz"),
                    None => println!("  Pitch (f0)        —    (unvoiced)"),
                }
                if stats.channels > 1 {
                    print!("  Ch peaks     ");
                    for (i, &p) in stats.channel_peaks.iter().enumerate() {
                        print!("ch{i}={p:.1}  ");
                    }
                    println!();
                }
            } else {
                eprintln!("{input}: empty or unreadable");
            }
        }
        Command::Convert { input, out } => {
            let audio = cathar::AudioData::from_file(&input)?;
            eprintln!(
                "{}  {} Hz  {} ch  {:.1}s  →  {}",
                input,
                audio.sample_rate,
                audio.channels.len(),
                audio.channels[0].len() as f32 / audio.sample_rate as f32,
                out
            );
            audio.to_file(&out)?;
            eprintln!("wrote  {out}");
        }
        #[cfg(feature = "ml")]
        Command::MlDenoise { input, out, weights, passthrough } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let orig_power = power(&audio.channels[0]);
            let denoiser = match (weights.as_deref(), passthrough) {
                (Some(path), _) => {
                    eprintln!("ml denoise  weights={path}");
                    cathar::NeuralDenoiser::from_safetensors(path, cathar::NeuralConfig::default())?
                }
                (_, true) => {
                    eprintln!("ml denoise  passthrough model (near no-op)");
                    cathar::NeuralDenoiser::new()?
                }
                (None, false) => {
                    eprintln!(
                        "ml denoise  pretrained model (synthetic tones — retrain on DNS-Challenge for speech)"
                    );
                    cathar::NeuralDenoiser::pretrained()?
                }
            };
            let clean = denoiser.denoise(&audio)?;
            clean.to_file(&out)?;
            if let (Some(o), Some(c)) = (orig_power, power(&clean.channels[0])) {
                if o > 0.0 {
                    eprintln!("reduction  {:.1}%", (1.0 - c / o) * 100.0);
                }
            }
            eprintln!("wrote  {out}");
        }
        #[cfg(feature = "tui")]
        Command::Play { input, fft } => {
            player::run(&input, fft)?;
        }
    }

    Ok(())
}

fn power(channel: &[f32]) -> Option<f32> {
    if channel.is_empty() {
        return None;
    }
    let mean = channel.iter().sum::<f32>() / channel.len() as f32;
    Some(channel.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / channel.len() as f32)
}
