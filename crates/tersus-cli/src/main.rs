//! CLI for tersus — AI audio denoising for video.

use anyhow::Result;
use clap::{Parser, Subcommand};

/// AI audio denoising and cleanup — clean, neat, polished.
#[derive(Debug, Parser)]
#[command(name = "tersus", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Denoise a video or audio file.
    Denoise {
        /// Input file (WAV, MP4, M4A, etc.)
        input: String,
        /// Output file (WAV)
        #[arg(short, long, default_value = "clean.wav")]
        out: String,
    },
    /// Generate a synthetic waveform for testing (sine wave + optional noise).
    Wave {
        /// Output file (WAV)
        #[arg(short, long, default_value = "test.wav")]
        out: String,
        /// Sample rate in Hz
        #[arg(short, long, default_value = "44100")]
        sample_rate: u32,
        /// Frequency of the sine wave in Hz
        #[arg(short, long, default_value = "440")]
        freq: f32,
        /// Duration in seconds
        #[arg(short, long, default_value_t = 3.0)]
        duration: f32,
        /// Noise level (0.0 = pure sine, 0.5 = very noisy)
        #[arg(short, long, default_value_t = 0.1)]
        noise: f32,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Denoise { input, out } => {
            let audio = tersus::AudioData::from_file(&input)?;
            tracing::info!(
                "Loaded {}: {} Hz, {} channel(s), {} samples",
                input,
                audio.sample_rate,
                audio.channels.len(),
                audio.channels.first().map_or(0, |c| c.len())
            );
            let denoiser = tersus::MockDenoiser;
            let clean = denoiser.denoise(&audio)?;
            clean.to_file(&out)?;
            tracing::info!("Wrote cleaned audio to {}", out);
        }
        Command::Wave {
            out,
            sample_rate,
            freq,
            duration,
            noise,
        } => {
            let audio = tersus::generate_wave(sample_rate, freq, duration, noise);
            audio.to_file(&out)?;
            tracing::info!(
                "Generated {} ({} Hz, {:.1}s, f={} Hz, noise={:.2})",
                out,
                sample_rate,
                duration,
                freq,
                noise
            );
        }
    }

    Ok(())
}
