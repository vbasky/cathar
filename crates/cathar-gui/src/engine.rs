//! System audio playback — rodio player with seek, dezippered volume, and L/R monitor.

use anyhow::{Result, anyhow};
use cathar::AudioData;
use std::num::NonZero;
use std::time::{Duration, Instant};

/// How the engine routes channels to the stereo output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Monitor {
    /// True multichannel as authored (mono → both speakers via OS/device).
    #[default]
    Stereo,
    /// Left channel only (right silent).
    Left,
    /// Right channel only (left silent).
    Right,
    /// Mid (L+R)/2 on both speakers.
    Mid,
}

/// Time constant for volume ramps (seconds). Long enough to kill zipper noise
/// when the UI slider jumps, short enough to still feel responsive.
const VOLUME_TAU_SEC: f32 = 0.035;

/// Owns the output device and the current player.
pub(crate) struct Engine {
    stream: rodio::MixerDeviceSink,
    player: rodio::Player,
    monitor: Monitor,
    /// User-requested gain (1.0 = unity).
    volume_target: f32,
    /// Gain currently sent to rodio (smoothed toward target).
    volume_actual: f32,
    last_volume_tick: Instant,
    /// Duration of the currently loaded buffer (seconds).
    duration: f32,
    /// Sample rate of the loaded buffer.
    sample_rate: u32,
    /// True after a successful load with samples.
    loaded: bool,
    /// Authoritative transport intent. rodio's pause flag alone is unreliable
    /// around buffer reloads / seeks (live EQ): we always re-apply this after
    /// those operations so Pause really means silence.
    want_playing: bool,
    /// True while the UI is dragging the playhead. Output is hard-muted and
    /// seeks must not resume audible playback (avoids scrub screech).
    scrubbing: bool,
}

impl Engine {
    /// Open the default output device.
    pub(crate) fn new() -> Result<Self> {
        let mut stream = rodio::DeviceSinkBuilder::open_default_sink()
            .map_err(|e| anyhow!("no audio output device: {e}"))?;
        stream.log_on_drop(false);
        let player = rodio::Player::connect_new(stream.mixer());
        player.pause();
        player.set_volume(1.0);
        Ok(Self {
            stream,
            player,
            monitor: Monitor::Stereo,
            volume_target: 1.0,
            volume_actual: 1.0,
            last_volume_tick: Instant::now(),
            duration: 0.0,
            sample_rate: 0,
            loaded: false,
            want_playing: false,
            scrubbing: false,
        })
    }

    pub(crate) fn set_monitor(&mut self, m: Monitor) {
        self.monitor = m;
    }

    /// Request a new playback volume. Applied smoothly by [`Self::tick_volume`]
    /// so slider moves do not create zipper crackle.
    pub(crate) fn set_volume(&mut self, v: f32) {
        self.volume_target = v.clamp(0.0, 2.0);
    }

    /// Advance the volume smoother. Call once per UI frame (or more often).
    ///
    /// Returns `true` while still approaching the target (caller may repaint).
    pub(crate) fn tick_volume(&mut self) -> bool {
        // Hard mute for the entire scrub gesture — never ramp volume back in.
        if self.scrubbing {
            if self.volume_actual != 0.0 {
                self.volume_actual = 0.0;
                self.player.set_volume(0.0);
            }
            return false;
        }

        let now = Instant::now();
        let dt = now.duration_since(self.last_volume_tick).as_secs_f32().clamp(0.0, 0.1);
        self.last_volume_tick = now;
        if dt <= 0.0 {
            return (self.volume_actual - self.volume_target).abs() > 1e-4;
        }

        let err = self.volume_target - self.volume_actual;
        if err.abs() < 1e-4 {
            if self.volume_actual != self.volume_target {
                self.volume_actual = self.volume_target;
                self.player.set_volume(self.volume_actual);
            }
            return false;
        }

        // One-pole toward target: y += (x - y) * (1 - e^{-dt/τ})
        let alpha = 1.0 - (-dt / VOLUME_TAU_SEC).exp();
        self.volume_actual += err * alpha;

        // Snap when close enough to avoid endless tiny updates.
        if (self.volume_actual - self.volume_target).abs() < 1e-4 {
            self.volume_actual = self.volume_target;
        }

        // Rodio multiplies samples by this each buffer — keep steps small via α.
        self.player.set_volume(self.volume_actual);
        true
    }

    #[allow(dead_code)]
    pub(crate) fn duration(&self) -> f32 {
        self.duration
    }

    pub(crate) fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Hard-stop the previous player so it cannot keep emitting on the mixer.
    fn retire_player(old: rodio::Player) {
        // Mute + pause + stop before drop. Never `detach()` — that leaves the
        // buffer playing (echo stack). stop() is non-blocking (atomic flag).
        old.set_volume(0.0);
        old.pause();
        old.stop();
        drop(old);
    }

    /// Tear down playback without blocking on audio-device Drop.
    ///
    /// On Windows, dropping cpal/rodio streams during process exit can hang
    /// forever; we stop the player then `mem::forget` the stream so the OS
    /// reclaims the device when the process actually ends.
    pub(crate) fn force_shutdown(self) {
        let Self { stream, player, .. } = self;
        player.set_volume(0.0);
        player.pause();
        player.stop();
        std::mem::forget(player);
        std::mem::forget(stream);
    }

    /// Apply [`Self::want_playing`] to the rodio player.
    fn apply_transport(&self) {
        if self.want_playing && self.loaded && !self.at_end() {
            self.player.play();
        } else {
            self.player.pause();
        }
    }

    /// Replace the currently-loaded audio, paused at position 0.
    ///
    /// Does **not** change [`Self::want_playing`] — callers that should stop
    /// transport (new file) clear it; live-EQ reload restores via [`Self::reload`].
    ///
    /// Always feeds a **stereo** interleaved buffer so L/R monitoring is
    /// predictable. Volume is applied by the player (dezippered), not baked in.
    pub(crate) fn load(&mut self, audio: &AudioData) -> Result<()> {
        // New buffer — never inherit a stuck scrub mute from the previous player.
        self.scrubbing = false;

        // Swap in a fresh Player on the shared mixer.
        // Replacing avoids rodio `stop`+`append` which can `sleep_until_end` and hang.
        let new_player = rodio::Player::connect_new(self.stream.mixer());
        // Pause *before* any samples are appended so the default unpaused
        // Player state cannot leak audio during the swap.
        new_player.pause();
        new_player.set_volume(self.volume_actual);

        let old = std::mem::replace(&mut self.player, new_player);
        Self::retire_player(old);

        self.last_volume_tick = Instant::now();

        let sr = audio.sample_rate;
        let n = audio.channels.iter().map(Vec::len).max().unwrap_or(0);
        self.sample_rate = sr;
        self.duration = if sr > 0 { n as f32 / sr as f32 } else { 0.0 };
        self.loaded = n > 0 && sr > 0;

        if !self.loaded {
            self.want_playing = false;
            return Ok(());
        }

        let left = audio.channels.first().map(Vec::as_slice).unwrap_or(&[]);
        let right = if audio.channels.len() >= 2 { audio.channels[1].as_slice() } else { left };

        let mut interleaved = vec![0.0f32; n * 2];
        for i in 0..n {
            let l = left.get(i).copied().unwrap_or(0.0);
            let r = right.get(i).copied().unwrap_or(0.0);
            let (ol, or) = match self.monitor {
                Monitor::Stereo => (l, r),
                Monitor::Left => (l, 0.0),
                Monitor::Right => (0.0, r),
                Monitor::Mid => {
                    let m = 0.5 * (l + r);
                    (m, m)
                }
            };
            interleaved[i * 2] = ol;
            interleaved[i * 2 + 1] = or;
        }

        let ch = NonZero::new(2u16).ok_or_else(|| anyhow!("zero channels"))?;
        let rate = NonZero::new(sr).ok_or_else(|| anyhow!("zero sample rate"))?;
        // Fresh SamplesBuffer starts at t=0 — do **not** try_seek here.
        self.player.append(rodio::buffer::SamplesBuffer::new(ch, rate, interleaved));
        // Append builds a source that starts unpaused until periodic_access;
        // re-assert pause. Transport is applied by play()/reload().
        self.player.pause();
        Ok(())
    }

    /// Seek without permanently changing [`Self::want_playing`].
    ///
    /// rodio `try_seek` needs the audio thread to run; we may briefly unpause
    /// for the handshake. When [`Self::scrubbing`], volume is forced to 0 so
    /// that handshake never becomes audible.
    fn seek_internal(&self, pos: Duration, resume: bool) {
        if !self.loaded {
            return;
        }
        let target = pos.as_secs_f32().clamp(0.0, self.duration.max(0.0));
        let cur = self.player.get_pos().as_secs_f32();
        // Already there — skip (also avoids seek-to-end at EOS hangs).
        if (cur - target).abs() < 0.03 {
            if resume && !self.scrubbing {
                self.apply_transport();
            } else {
                self.player.pause();
            }
            return;
        }
        // Source has finished draining: try_seek can block forever. Caller must
        // re-load the buffer (see `transport_play_pause` / `reload`).
        if self.at_end() && target > 0.05 {
            if resume && !self.scrubbing {
                self.apply_transport();
            } else {
                self.player.pause();
            }
            return;
        }

        // Handshake: stream must pull samples. Volume is 0 while scrubbing.
        if self.scrubbing {
            self.player.set_volume(0.0);
        }
        self.player.play();
        let _ = self.player.try_seek(Duration::from_secs_f32(target));
        if resume && !self.scrubbing {
            self.apply_transport();
            self.player.set_volume(self.volume_actual);
        } else {
            self.player.pause();
            if self.scrubbing {
                self.player.set_volume(0.0);
            }
        }
    }

    /// Enter scrub mode: hard-mute + pause. Does not clear play intent.
    pub(crate) fn begin_scrub(&mut self) {
        self.scrubbing = true;
        self.volume_actual = 0.0;
        self.last_volume_tick = Instant::now();
        self.player.set_volume(0.0);
        self.player.pause();
    }

    /// Leave scrub mode after the final seek.
    ///
    /// Stays at volume 0 and resumes transport if needed, then
    /// [`Self::tick_volume`] ramps gain back in — avoids a full-level click
    /// on the first buffer after a discontinuous seek.
    pub(crate) fn end_scrub(&mut self) {
        self.scrubbing = false;
        self.volume_actual = 0.0;
        self.last_volume_tick = Instant::now();
        self.player.set_volume(0.0);
        self.apply_transport();
        // Keep actual at 0 so the dezipper fades in over ~35ms instead of
        // slamming the new position at full volume.
    }

    /// True while a scrub gesture has muted the engine.
    pub(crate) fn is_scrubbing(&self) -> bool {
        self.scrubbing
    }

    /// Force-clear scrub mute and restore transport (missed drag_stopped, etc.).
    ///
    /// Leaves volume at 0 so [`Self::tick_volume`] can fade in cleanly.
    pub(crate) fn cancel_scrub(&mut self) {
        if !self.scrubbing {
            return;
        }
        self.end_scrub();
    }

    /// Drop the scrub flag only — no transport change.
    ///
    /// Used before play/pause/toggle so we do not `apply_transport` and then
    /// immediately invert intent via toggle.
    pub(crate) fn clear_scrub_flag(&mut self) {
        if !self.scrubbing {
            return;
        }
        self.scrubbing = false;
        self.volume_actual = 0.0;
        self.last_volume_tick = Instant::now();
        self.player.set_volume(0.0);
    }

    /// One seek at scrub release (must be called while still muted / scrubbing).
    ///
    /// Returns `false` when the source is drained and `try_seek` would hang or
    /// no-op — caller must re-append the buffer via [`Self::reload`] instead.
    pub(crate) fn seek_scrub(&self, t: f32) -> bool {
        if !self.loaded {
            return true;
        }
        // Drained source: try_seek can hang; at_end path in seek_internal also
        // refuses non-zero seeks. Force a full reload from the UI.
        if self.at_end() {
            return false;
        }
        let t = t.clamp(0.0, self.duration.max(0.0));
        // Force silence around the handshake even if begin_scrub was skipped.
        self.player.set_volume(0.0);
        self.seek_internal(Duration::from_secs_f32(t), false);
        self.player.set_volume(0.0);
        self.player.pause();
        true
    }

    /// Reload buffer, keep playhead, restore transport intent.
    pub(crate) fn reload(
        &mut self,
        audio: &AudioData,
        resume_pos: f32,
        was_playing: bool,
    ) -> Result<()> {
        // Capture intent before load (load does not clear want_playing unless empty).
        self.want_playing = was_playing;
        self.load(audio)?;
        if !self.loaded {
            self.want_playing = false;
            return Ok(());
        }
        if resume_pos > 0.05 {
            self.seek_internal(
                Duration::from_secs_f32(resume_pos.clamp(0.0, self.duration.max(0.0))),
                true,
            );
        }
        self.apply_transport();
        Ok(())
    }

    pub(crate) fn play(&mut self) {
        if !self.loaded {
            return;
        }
        // Never stay hard-muted after the user hits play.
        if self.scrubbing {
            self.scrubbing = false;
            self.volume_actual = 0.0;
            self.last_volume_tick = Instant::now();
            self.player.set_volume(0.0);
        }
        self.want_playing = true;
        // Restart from 0 only if the buffer is still seekable (not drained).
        if self.at_end() {
            // Need a full reload — seek on drained source hangs. Caller uses
            // needs_reload_to_restart; still try soft seek for partial cases.
            self.seek_internal(Duration::ZERO, true);
        }
        self.apply_transport();
    }

    pub(crate) fn pause(&mut self) {
        // Clear scrub mute so pause does not leave volume stuck at 0 forever.
        if self.scrubbing {
            self.scrubbing = false;
            self.volume_actual = 0.0;
            self.last_volume_tick = Instant::now();
        }
        self.want_playing = false;
        self.player.pause();
    }

    /// Toggle play/pause.
    pub(crate) fn toggle(&mut self) {
        if self.want_playing {
            self.pause();
        } else {
            self.play();
        }
    }

    /// Pause and return to the start (no-op seek if already finished — reload instead).
    pub(crate) fn stop(&mut self) {
        if self.scrubbing {
            self.scrubbing = false;
            self.volume_actual = 0.0;
            self.last_volume_tick = Instant::now();
        }
        self.want_playing = false;
        self.player.pause();
        if !self.at_end() {
            self.seek_internal(Duration::ZERO, false);
        }
        self.apply_transport();
    }

    /// True when a restart needs a full buffer reload (source finished).
    pub(crate) fn needs_reload_to_restart(&self) -> bool {
        self.loaded && self.at_end()
    }

    /// True when transport is paused (authoritative — not only rodio's flag).
    pub(crate) fn is_paused(&self) -> bool {
        !self.want_playing
    }

    pub(crate) fn is_playing(&self) -> bool {
        self.loaded && self.want_playing && !self.at_end()
    }

    /// Current playhead position, seconds (clamped to [0, duration]).
    pub(crate) fn pos(&self) -> f32 {
        if !self.loaded {
            return 0.0;
        }
        self.player.get_pos().as_secs_f32().clamp(0.0, self.duration.max(0.0))
    }

    /// True when the playhead is at (or past) the end of the buffer.
    pub(crate) fn at_end(&self) -> bool {
        if !self.loaded || self.duration <= 0.0 {
            return false;
        }
        self.player.get_pos().as_secs_f32() >= self.duration - 0.02
    }

    /// Seek to `t` seconds (clamped). Resumes if transport wants to play.
    pub(crate) fn seek(&self, t: f32) -> f32 {
        if !self.loaded {
            return 0.0;
        }
        let t = t.clamp(0.0, self.duration.max(0.0));
        self.seek_internal(Duration::from_secs_f32(t), true);
        t
    }

    /// Skip by `delta` seconds (negative = rewind).
    pub(crate) fn skip(&self, delta: f32) -> f32 {
        self.seek(self.pos() + delta)
    }
}
