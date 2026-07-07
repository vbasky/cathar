//! System audio playback — the same rodio path `cathar play` uses, wrapped so
//! the GUI can (re)load edited buffers, transport-control, and read the playhead.

use anyhow::{Result, anyhow};
use cathar::AudioData;
use std::num::NonZero;
use std::time::Duration;

/// Owns the output device and the current player.
pub(crate) struct Engine {
    stream: rodio::MixerDeviceSink,
    player: rodio::Player,
}

impl Engine {
    /// Open the default output device.
    pub(crate) fn new() -> Result<Self> {
        let mut stream = rodio::DeviceSinkBuilder::open_default_sink()
            .map_err(|e| anyhow!("no audio output device: {e}"))?;
        stream.log_on_drop(false);
        let player = rodio::Player::connect_new(stream.mixer());
        Ok(Self { stream, player })
    }

    /// Replace the currently-loaded audio, paused at position 0.
    pub(crate) fn load(&mut self, audio: &AudioData) -> Result<()> {
        self.player.stop();
        self.player = rodio::Player::connect_new(self.stream.mixer());

        let sr = audio.sample_rate;
        let nch = audio.channels.len().max(1);
        let n = audio.channels.iter().map(Vec::len).max().unwrap_or(0);
        let mut interleaved = vec![0.0f32; n * nch];
        for (c, ch) in audio.channels.iter().enumerate() {
            for (i, &s) in ch.iter().enumerate() {
                interleaved[i * nch + c] = s;
            }
        }
        let ch = NonZero::new(nch as u16).ok_or_else(|| anyhow!("zero channels"))?;
        let rate = NonZero::new(sr).ok_or_else(|| anyhow!("zero sample rate"))?;
        self.player.append(rodio::buffer::SamplesBuffer::new(ch, rate, interleaved));
        self.player.pause();
        Ok(())
    }

    /// Toggle play/pause.
    pub(crate) fn toggle(&self) {
        if self.player.is_paused() {
            self.player.play();
        } else {
            self.player.pause();
        }
    }

    /// True when paused (or nothing loaded).
    pub(crate) fn is_paused(&self) -> bool {
        self.player.is_paused()
    }

    /// Current playhead position, seconds.
    pub(crate) fn pos(&self) -> f32 {
        self.player.get_pos().as_secs_f32()
    }

    /// Seek to `t` seconds (best-effort).
    pub(crate) fn seek(&self, t: f32) {
        let _ = self.player.try_seek(Duration::from_secs_f32(t.max(0.0)));
    }
}
