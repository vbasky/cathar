//! System audio playback — seek, dezippered volume, and L/R monitor.
//!
//! Linux talks to PulseAudio (`libpulse-simple`) via `dlopen`. macOS/Windows
//! keep rodio/cpal (CoreAudio / WASAPI). Neither path uses ALSA or pkg-config.
//!
//! Seeking is an atomic playhead into an interleaved stereo cache — no device
//! `try_seek`.

use anyhow::Result;
use cathar::AudioData;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
#[cfg(not(target_os = "linux"))]
use std::time::Duration;
use std::time::Instant;

#[cfg(target_os = "linux")]
use crate::pulse_linux::PulseOut;

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

/// Time constant for volume ramps (seconds).
const VOLUME_TAU_SEC: f32 = 0.035;

/// Shared with the audio callback / Pulse write thread.
pub(crate) struct PlayState {
    cache: Mutex<Option<Arc<[f32]>>>,
    /// Next interleaved sample index.
    pos: AtomicUsize,
    playing: AtomicBool,
    volume: AtomicU32,
    sample_rate: AtomicU32,
}

impl PlayState {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            cache: Mutex::new(None),
            pos: AtomicUsize::new(0),
            playing: AtomicBool::new(false),
            volume: AtomicU32::new(1.0f32.to_bits()),
            sample_rate: AtomicU32::new(0),
        })
    }

    pub(crate) fn sample_rate(&self) -> u32 {
        self.sample_rate.load(Ordering::Relaxed)
    }

    pub(crate) fn fill(&self, out: &mut [f32]) {
        let vol = f32::from_bits(self.volume.load(Ordering::Relaxed));
        let playing = self.playing.load(Ordering::Relaxed);
        let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let Some(data) = cache.as_ref() else {
            out.fill(0.0);
            return;
        };
        let mut i = self.pos.load(Ordering::Relaxed);
        for s in out.iter_mut() {
            if !playing || i >= data.len() {
                *s = 0.0;
            } else {
                *s = data[i] * vol;
                i += 1;
            }
        }
        if playing {
            self.pos.store(i, Ordering::Relaxed);
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn next_sample(&self) -> f32 {
        let mut one = [0.0f32];
        self.fill(&mut one);
        one[0]
    }
}

enum Output {
    #[cfg(target_os = "linux")]
    Pulse(PulseOut),
    #[cfg(not(target_os = "linux"))]
    Rodio { stream: rodio::MixerDeviceSink, player: rodio::Player },
}

/// Owns the output device and the current playhead.
pub(crate) struct Engine {
    shared: Arc<PlayState>,
    output: Output,
    monitor: Monitor,
    volume_target: f32,
    volume_actual: f32,
    last_volume_tick: Instant,
    duration: f32,
    loaded: bool,
    want_playing: bool,
    scrubbing: bool,
}

impl Engine {
    pub(crate) fn new() -> Result<Self> {
        let shared = PlayState::new();
        let output = start_output(Arc::clone(&shared))?;
        Ok(Self {
            shared,
            output,
            monitor: Monitor::Stereo,
            volume_target: 1.0,
            volume_actual: 1.0,
            last_volume_tick: Instant::now(),
            duration: 0.0,
            loaded: false,
            want_playing: false,
            scrubbing: false,
        })
    }

    pub(crate) fn set_monitor(&mut self, m: Monitor) {
        self.monitor = m;
    }

    pub(crate) fn set_volume(&mut self, v: f32) {
        self.volume_target = v.clamp(0.0, 2.0);
    }

    pub(crate) fn tick_volume(&mut self) -> bool {
        if self.scrubbing {
            if self.volume_actual != 0.0 {
                self.volume_actual = 0.0;
                self.store_volume(0.0);
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
                self.store_volume(self.volume_actual);
            }
            return false;
        }

        let alpha = 1.0 - (-dt / VOLUME_TAU_SEC).exp();
        self.volume_actual += err * alpha;
        if (self.volume_actual - self.volume_target).abs() < 1e-4 {
            self.volume_actual = self.volume_target;
        }
        self.store_volume(self.volume_actual);
        true
    }

    fn store_volume(&self, v: f32) {
        self.shared.volume.store(v.to_bits(), Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub(crate) fn duration(&self) -> f32 {
        self.duration
    }

    pub(crate) fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub(crate) fn has_cache(&self) -> bool {
        self.loaded && self.shared.cache.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    pub(crate) fn force_shutdown(self) {
        self.shared.playing.store(false, Ordering::Relaxed);
        #[cfg(not(target_os = "linux"))]
        {
            let Output::Rodio { stream, player } = self.output else {
                return;
            };
            player.set_volume(0.0);
            player.pause();
            player.stop();
            std::mem::forget(player);
            std::mem::forget(stream);
        }
        #[cfg(target_os = "linux")]
        {
            let Output::Pulse(_out) = self.output;
        }
    }

    fn sync_output(&self) {
        let run = self.want_playing && self.loaded && !self.at_end() && !self.scrubbing;
        self.shared.playing.store(run, Ordering::Relaxed);
    }

    fn rebuild_cache(&mut self, audio: &AudioData) {
        let sr = audio.sample_rate;
        let n = audio.channels.iter().map(Vec::len).max().unwrap_or(0);
        self.duration = if sr > 0 { n as f32 / sr as f32 } else { 0.0 };
        self.loaded = n > 0 && sr > 0;
        self.shared.sample_rate.store(if self.loaded { sr } else { 0 }, Ordering::Relaxed);

        if !self.loaded {
            *self.shared.cache.lock().unwrap_or_else(|e| e.into_inner()) = None;
            self.want_playing = false;
            self.shared.pos.store(0, Ordering::Relaxed);
            self.sync_output();
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
        *self.shared.cache.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::from(interleaved));
    }

    fn seek_index(&self, start_sec: f32) {
        let sr = self.shared.sample_rate().max(1);
        let start_sec = start_sec.clamp(0.0, self.duration.max(0.0));
        let frame = (start_sec * sr as f32).floor() as usize;
        self.shared.pos.store(frame.saturating_mul(2), Ordering::Relaxed);
    }

    pub(crate) fn load(&mut self, audio: &AudioData) -> Result<()> {
        self.load_from(audio, 0.0)
    }

    pub(crate) fn load_from(&mut self, audio: &AudioData, start_sec: f32) -> Result<()> {
        if !self.scrubbing {
            self.scrubbing = false;
        } else {
            self.volume_actual = 0.0;
            self.store_volume(0.0);
        }
        self.last_volume_tick = Instant::now();
        self.rebuild_cache(audio);
        if self.loaded {
            self.seek_index(start_sec);
            #[cfg(not(target_os = "linux"))]
            self.rebind_rodio();
        }
        self.sync_output();
        Ok(())
    }

    pub(crate) fn seek_cached(&mut self, t: f32) -> bool {
        if !self.has_cache() {
            return false;
        }
        let t = t.clamp(0.0, self.duration.max(0.0));
        if (self.pos() - t).abs() < 0.02 && !self.at_end() {
            return true;
        }
        self.seek_index(t);
        self.sync_output();
        true
    }

    pub(crate) fn begin_scrub(&mut self) {
        self.scrubbing = true;
        self.volume_actual = 0.0;
        self.last_volume_tick = Instant::now();
        self.store_volume(0.0);
        self.sync_output();
    }

    pub(crate) fn end_scrub(&mut self) {
        self.scrubbing = false;
        self.volume_actual = 0.0;
        self.last_volume_tick = Instant::now();
        self.store_volume(0.0);
        self.sync_output();
    }

    pub(crate) fn is_scrubbing(&self) -> bool {
        self.scrubbing
    }

    pub(crate) fn cancel_scrub(&mut self) {
        if self.scrubbing {
            self.end_scrub();
        }
    }

    pub(crate) fn clear_scrub_flag(&mut self) {
        if !self.scrubbing {
            return;
        }
        self.scrubbing = false;
        self.volume_actual = 0.0;
        self.last_volume_tick = Instant::now();
        self.store_volume(0.0);
    }

    pub(crate) fn reload(
        &mut self,
        audio: &AudioData,
        resume_pos: f32,
        was_playing: bool,
    ) -> Result<()> {
        self.want_playing = was_playing;
        if self.scrubbing {
            self.volume_actual = 0.0;
            self.store_volume(0.0);
        }
        self.load_from(audio, resume_pos)?;
        if !self.loaded {
            self.want_playing = false;
        }
        self.sync_output();
        Ok(())
    }

    pub(crate) fn play(&mut self) {
        if !self.loaded {
            return;
        }
        if self.scrubbing {
            self.scrubbing = false;
            self.volume_actual = 0.0;
            self.last_volume_tick = Instant::now();
            self.store_volume(0.0);
        }
        self.want_playing = true;
        self.sync_output();
    }

    pub(crate) fn pause(&mut self) {
        if self.scrubbing {
            self.scrubbing = false;
            self.volume_actual = 0.0;
            self.last_volume_tick = Instant::now();
        }
        self.want_playing = false;
        self.sync_output();
    }

    pub(crate) fn toggle(&mut self) {
        if self.want_playing {
            self.pause();
        } else {
            self.play();
        }
    }

    pub(crate) fn stop(&mut self) {
        if self.scrubbing {
            self.scrubbing = false;
            self.volume_actual = 0.0;
            self.last_volume_tick = Instant::now();
        }
        self.want_playing = false;
        self.sync_output();
    }

    pub(crate) fn needs_reload_to_restart(&self) -> bool {
        self.loaded && self.at_end()
    }

    pub(crate) fn is_paused(&self) -> bool {
        !self.want_playing
    }

    pub(crate) fn is_playing(&self) -> bool {
        self.loaded && self.want_playing && !self.at_end()
    }

    pub(crate) fn pos(&self) -> f32 {
        if !self.loaded {
            return 0.0;
        }
        let sr = self.shared.sample_rate().max(1);
        let frame = self.shared.pos.load(Ordering::Relaxed) / 2;
        (frame as f32 / sr as f32).clamp(0.0, self.duration.max(0.0))
    }

    pub(crate) fn at_end(&self) -> bool {
        if !self.loaded || self.duration <= 0.0 {
            return false;
        }
        self.pos() >= self.duration - 0.02
    }

    #[cfg(not(target_os = "linux"))]
    fn rebind_rodio(&mut self) {
        let Output::Rodio { stream, player } = &mut self.output else {
            return;
        };
        let new_player = rodio::Player::connect_new(stream.mixer());
        new_player.pause();
        new_player.set_volume(1.0);
        let old = std::mem::replace(player, new_player);
        old.set_volume(0.0);
        old.pause();
        old.stop();
        drop(old);
        player.append(StateSource { shared: Arc::clone(&self.shared) });
        player.play();
        self.sync_output();
    }
}

fn start_output(shared: Arc<PlayState>) -> Result<Output> {
    #[cfg(target_os = "linux")]
    {
        Ok(Output::Pulse(PulseOut::start(shared)?))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let mut stream = rodio::DeviceSinkBuilder::open_default_sink()
            .map_err(|e| anyhow::anyhow!("no audio output device: {e}"))?;
        stream.log_on_drop(false);
        let player = rodio::Player::connect_new(stream.mixer());
        player.set_volume(1.0);
        player.append(StateSource { shared });
        player.play();
        Ok(Output::Rodio { stream, player })
    }
}

#[cfg(not(target_os = "linux"))]
struct StateSource {
    shared: Arc<PlayState>,
}

#[cfg(not(target_os = "linux"))]
impl Iterator for StateSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        Some(self.shared.next_sample())
    }
}

#[cfg(not(target_os = "linux"))]
impl rodio::Source for StateSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> rodio::ChannelCount {
        std::num::NonZero::new(2).unwrap()
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        std::num::NonZero::new(self.shared.sample_rate().max(1)).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}
