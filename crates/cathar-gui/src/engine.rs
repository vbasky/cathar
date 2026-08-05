//! System audio playback — rodio player with seek, dezippered volume, and L/R monitor.
//!
//! # Seeking (no UI freezes)
//! rodio [`Player::try_seek`] blocks the calling thread until the audio device
//! callback processes the order. That handshake can stall forever — we never
//! call it.
//!
//! Instead we keep one interleaved stereo cache ([`Arc<[f32]>`]) and on each
//! seek attach a new [`CachedSamples`] source that **starts** at the target
//! sample index. Long files stay snappy: interleave once per load/EQ/monitor
//! change; seeks only swap the player and set an offset (O(1) work).

use anyhow::{Result, anyhow};
use cathar::AudioData;
use rodio::Source;
use std::num::NonZero;
use std::sync::Arc;
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

/// In-memory interleaved stereo source starting at an arbitrary sample offset.
///
/// Holds a shared cache so seeks never re-copy the full buffer.
#[derive(Clone)]
struct CachedSamples {
    data: Arc<[f32]>,
    /// Next sample index into `data` (interleaved).
    pos: usize,
    channels: u16,
    sample_rate: u32,
}

impl CachedSamples {
    fn new(data: Arc<[f32]>, start_frame: usize, channels: u16, sample_rate: u32) -> Self {
        let ch = channels.max(1) as usize;
        let start = (start_frame * ch).min(data.len());
        // Frame-align.
        let start = start - (start % ch);
        Self { data, pos: start, channels: channels.max(1), sample_rate: sample_rate.max(1) }
    }
}

impl Iterator for CachedSamples {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.data.len() {
            return None;
        }
        let s = self.data[self.pos];
        self.pos += 1;
        Some(s)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = self.data.len().saturating_sub(self.pos);
        (left, Some(left))
    }
}

impl Source for CachedSamples {
    #[inline]
    fn current_span_len(&self) -> Option<usize> {
        let left = self.data.len().saturating_sub(self.pos);
        Some(left)
    }

    #[inline]
    fn channels(&self) -> rodio::ChannelCount {
        NonZero::new(self.channels).unwrap_or(NonZero::new(1).unwrap())
    }

    #[inline]
    fn sample_rate(&self) -> rodio::SampleRate {
        NonZero::new(self.sample_rate).unwrap_or(NonZero::new(44_100).unwrap())
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        let frames = self.data.len() as u64 / self.channels.max(1) as u64;
        let nanos = frames.saturating_mul(1_000_000_000) / self.sample_rate.max(1) as u64;
        Some(Duration::from_nanos(nanos))
    }
}

/// Owns the output device and the current player.
pub(crate) struct Engine {
    stream: rodio::MixerDeviceSink,
    player: rodio::Player,
    monitor: Monitor,
    /// Interleaved stereo cache for the current monitor/EQ rendering.
    /// Built once per load; seeks reuse it via [`CachedSamples`].
    cache: Option<Arc<[f32]>>,
    /// User-requested gain (1.0 = unity).
    volume_target: f32,
    /// Gain currently sent to rodio (smoothed toward target).
    volume_actual: f32,
    last_volume_tick: Instant,
    /// Full-file duration (seconds) — scrubber / UI range.
    duration: f32,
    /// Absolute time (seconds) where the current source starts.
    pos_base: f32,
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
    /// Wall-clock of last play/pause/seek — used to re-attach after long idle.
    last_transport_at: Instant,
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
            cache: None,
            volume_target: 1.0,
            volume_actual: 1.0,
            last_volume_tick: Instant::now(),
            duration: 0.0,
            pos_base: 0.0,
            sample_rate: 0,
            loaded: false,
            want_playing: false,
            scrubbing: false,
            last_transport_at: Instant::now(),
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

    /// True when a cached interleaved buffer is ready for O(1) seeks.
    pub(crate) fn has_cache(&self) -> bool {
        self.cache.is_some() && self.loaded
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

    /// Swap in a fresh Player and attach `source` from the current cache offset.
    fn attach_source(&mut self, source: CachedSamples) {
        let new_player = rodio::Player::connect_new(self.stream.mixer());
        new_player.pause();
        new_player.set_volume(if self.scrubbing { 0.0 } else { self.volume_actual });
        let old = std::mem::replace(&mut self.player, new_player);
        Self::retire_player(old);
        self.player.append(source);
        self.player.pause();
        self.last_transport_at = Instant::now();
    }

    /// Build interleaved stereo cache from `audio` with current monitor routing.
    fn rebuild_cache(&mut self, audio: &AudioData) {
        let sr = audio.sample_rate;
        let n = audio.channels.iter().map(Vec::len).max().unwrap_or(0);
        self.sample_rate = sr;
        self.duration = if sr > 0 { n as f32 / sr as f32 } else { 0.0 };
        self.loaded = n > 0 && sr > 0;

        if !self.loaded {
            self.cache = None;
            self.want_playing = false;
            self.pos_base = 0.0;
            return;
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
        self.cache = Some(Arc::from(interleaved));
    }

    /// Attach the cache starting at absolute `start_sec` (must have cache).
    fn attach_from_time(&mut self, start_sec: f32) {
        let Some(data) = self.cache.clone() else {
            return;
        };
        let sr = self.sample_rate.max(1);
        let n_frames = data.len() / 2;
        let start_sec = start_sec.clamp(0.0, self.duration.max(0.0));
        let start_i = ((start_sec * sr as f32).floor() as usize).min(n_frames.saturating_sub(1));
        self.pos_base = start_i as f32 / sr as f32;
        let source = CachedSamples::new(data, start_i, 2, sr);
        self.attach_source(source);
    }

    /// Replace the currently-loaded audio, paused at position 0.
    pub(crate) fn load(&mut self, audio: &AudioData) -> Result<()> {
        self.load_from(audio, 0.0)
    }

    /// Load audio starting at `start_sec` (absolute timeline).
    ///
    /// Rebuilds the interleaved cache once, then attaches from `start_sec`.
    pub(crate) fn load_from(&mut self, audio: &AudioData, start_sec: f32) -> Result<()> {
        let keep_scrub = self.scrubbing;
        if !keep_scrub {
            self.scrubbing = false;
        } else {
            self.volume_actual = 0.0;
        }

        self.last_volume_tick = Instant::now();
        self.rebuild_cache(audio);

        if !self.loaded {
            // Still swap player to silence any previous sound.
            let new_player = rodio::Player::connect_new(self.stream.mixer());
            new_player.pause();
            let old = std::mem::replace(&mut self.player, new_player);
            Self::retire_player(old);
            return Ok(());
        }

        self.attach_from_time(start_sec);
        Ok(())
    }

    /// Seek using the existing cache only (O(1)). Returns `false` if no cache —
    /// caller must supply audio via [`Self::reload`].
    pub(crate) fn seek_cached(&mut self, t: f32) -> bool {
        if !self.has_cache() {
            return false;
        }
        let t = t.clamp(0.0, self.duration.max(0.0));
        // Already there — avoid player churn for tiny moves.
        if (self.pos() - t).abs() < 0.02 && !self.at_end() {
            return true;
        }
        let was_scrub = self.scrubbing;
        self.attach_from_time(t);
        if was_scrub {
            self.player.set_volume(0.0);
            self.player.pause();
        } else {
            self.apply_transport();
        }
        true
    }

    /// Enter scrub mode: hard-mute + pause. Does not clear play intent.
    pub(crate) fn begin_scrub(&mut self) {
        self.scrubbing = true;
        self.volume_actual = 0.0;
        self.last_volume_tick = Instant::now();
        self.player.set_volume(0.0);
        self.player.pause();
        self.last_transport_at = Instant::now();
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
        self.last_transport_at = Instant::now();
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

    /// Rebuild cache from `audio` and attach at `resume_pos`.
    pub(crate) fn reload(
        &mut self,
        audio: &AudioData,
        resume_pos: f32,
        was_playing: bool,
    ) -> Result<()> {
        self.want_playing = was_playing;
        let scrub = self.scrubbing;
        if scrub {
            self.volume_actual = 0.0;
            self.player.set_volume(0.0);
            self.player.pause();
        }
        self.load_from(audio, resume_pos)?;
        if !self.loaded {
            self.want_playing = false;
            return Ok(());
        }
        if scrub {
            self.volume_actual = 0.0;
            self.player.set_volume(0.0);
            self.player.pause();
        } else {
            self.apply_transport();
        }
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

        // After a long pause the device/source can go stale. Re-attach from the
        // current playhead using the cache so resume is reliable and still O(1).
        let idle = self.last_transport_at.elapsed() > Duration::from_secs(30);
        if idle && self.has_cache() && !self.at_end() {
            let t = self.pos();
            self.attach_from_time(t);
        }

        self.apply_transport();
        self.last_transport_at = Instant::now();
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
        self.last_transport_at = Instant::now();
    }

    /// Toggle play/pause.
    pub(crate) fn toggle(&mut self) {
        if self.want_playing {
            self.pause();
        } else {
            self.play();
        }
    }

    /// Pause. Does not reseek — use app `seek_to(0)` to return to start.
    pub(crate) fn stop(&mut self) {
        if self.scrubbing {
            self.scrubbing = false;
            self.volume_actual = 0.0;
            self.last_volume_tick = Instant::now();
        }
        self.want_playing = false;
        self.player.pause();
        self.last_transport_at = Instant::now();
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
        let rel = self.player.get_pos().as_secs_f32();
        (self.pos_base + rel).clamp(0.0, self.duration.max(0.0))
    }

    /// True when the playhead is at (or past) the end of the file.
    pub(crate) fn at_end(&self) -> bool {
        if !self.loaded || self.duration <= 0.0 {
            return false;
        }
        self.pos() >= self.duration - 0.02
    }
}
