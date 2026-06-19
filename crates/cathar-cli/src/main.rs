//! CLI for cathar — audio restoration toolbox.

use anyhow::Result;
use cathar::Denoiser;
use clap::{Parser, Subcommand};
use rayon::prelude::*;

/// Audio restoration toolbox — denoise, de-hum, de-click, de-clip, normalise.
#[derive(Debug, Parser)]
#[command(name = "cathar", version, about)]
struct Cli {
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
        /// Threshold in dB
        #[arg(short, long, default_value_t = -24.0)]
        threshold: f32,
    },
    /// Attenuate breath sounds between speech segments.
    Breath {
        /// Input file
        input: String,
        /// Output WAV file
        #[arg(short, long, default_value = "breathless.wav")]
        out: String,
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
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Denoise { input, out, alpha, beta, noiseprint, wiener } => {
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
                denoiser.denoise(&audio)?
            } else if wiener {
                let np = cathar::learn_noise_print(&audio)?;
                let output = cathar::wiener_denoise(&audio.channels[0], &np, alpha)?;
                cathar::AudioData { sample_rate: audio.sample_rate, channels: vec![output] }
            } else {
                let denoiser = cathar::SpectralDenoiser { alpha, beta, ..Default::default() };
                denoiser.denoise(&audio)?
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
        Command::Deesser { input, out, freq, threshold } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let cleaned =
                audio.map_channels(|c| cathar::deesser(c, audio.sample_rate, freq, threshold, 3.0));
            cleaned.to_file(&out)?;
            eprintln!("de-essed  crossover={freq} Hz  threshold={threshold} dB  →  {out}");
        }
        Command::Breath { input, out } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let cleaned = audio.map_channels(|c| cathar::breath_remove(c, audio.sample_rate));
            cleaned.to_file(&out)?;
            eprintln!("breath-removed  →  {out}");
        }
        Command::Enhance { input, out, rate } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let channels: Vec<Vec<f32>> = audio
                .channels
                .iter()
                .map(|c| cathar::bandwidth_extend(c, audio.sample_rate, rate))
                .collect();
            let result = cathar::AudioData { sample_rate: rate, channels };
            result.to_file(&out)?;
            eprintln!("enhanced  {} Hz → {rate} Hz  →  {out}", audio.sample_rate);
        }

        Command::Normalize { input, out, target, peak } => {
            let audio = cathar::AudioData::from_file(&input)?;
            let cleaned = if peak {
                audio.map_channels(|c| cathar::normalize_peak(c, target))
            } else {
                audio.map_channels(|c| cathar::normalize_loudness(c, target))
            };
            cleaned.to_file(&out)?;
            let label = if peak { "peak" } else { "LUFS" };
            eprintln!("normalized  {target} {label}  →  {out}");
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
                        clean = clean.map_channels(|c| cathar::normalize_loudness(c, lu));
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
