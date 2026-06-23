//! Error type for decode, encode, and processing.

use thiserror::Error;

/// Errors returned by cathar's decode, encode, and processing routines.
#[derive(Debug, Error)]
pub enum Error {
    /// An underlying I/O error (opening, reading, or writing a file).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// A WAV write error from the `hound` encoder.
    #[error("audio write error: {0}")]
    Hound(#[from] hound::Error),
    /// Encoding to the chosen output container (FLAC/AIFF) failed.
    #[error("encode error: {0}")]
    Encode(String),
    /// Decoding the input failed (unsupported codec, corrupt stream, …).
    #[error("decode error: {0}")]
    Decode(String),
    /// The container has no audio track to decode.
    #[error("no audio track found")]
    NoAudioTrack,
    /// The input format or codec parameters are not supported.
    #[error("unsupported format")]
    UnsupportedFormat,
    /// The signal is shorter than the analysis window the stage requires.
    #[error("signal too short")]
    TooShort,
    /// An FFT planning or processing error.
    #[error("FFT error: {0}")]
    Fft(String),
    /// A supplied noise print's FFT size does not match the denoiser's.
    #[error("noise print FFT size mismatch")]
    NoisePrintMismatch,
    /// A neural-inference error from the `ml` feature (candle tensor op,
    /// shape mismatch, or checkpoint load failure).
    #[cfg(feature = "ml")]
    #[error("ML error: {0}")]
    Ml(String),
}

#[cfg(feature = "ml")]
impl From<candle_core::Error> for Error {
    fn from(e: candle_core::Error) -> Self {
        Error::Ml(e.to_string())
    }
}

impl From<realfft::FftError> for Error {
    fn from(e: realfft::FftError) -> Self {
        Error::Fft(format!("{e:?}"))
    }
}
