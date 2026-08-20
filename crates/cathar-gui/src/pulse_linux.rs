//! Linux playback via PulseAudio's simple API (`libpulse-simple.so.0`).
//!
//! Loaded with `dlopen` at runtime — no pkg-config, no ALSA, no `-dev` headers.
//! PipeWire's Pulse compatibility layer provides the same soname.

use std::ffi::{CString, c_char, c_int, c_void};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Result, anyhow};
use libloading::Library;

use crate::engine::PlayState;

const PA_STREAM_PLAYBACK: c_int = 1;
const PA_SAMPLE_FLOAT32LE: c_int = 5;
const FRAMES: usize = 1024;
const CHANNELS: usize = 2;

#[repr(C)]
struct PaSampleSpec {
    format: c_int,
    rate: u32,
    channels: u8,
    _pad: [u8; 3],
}

type PaSimpleNew = unsafe extern "C" fn(
    *const c_char,
    *const c_char,
    c_int,
    *const c_char,
    *const c_char,
    *const PaSampleSpec,
    *const c_void,
    *const c_void,
    *mut c_int,
) -> *mut c_void;
type PaSimpleWrite = unsafe extern "C" fn(*mut c_void, *const c_void, usize, *mut c_int) -> c_int;
type PaSimpleFree = unsafe extern "C" fn(*mut c_void);

pub(crate) struct PulseOut {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl PulseOut {
    pub(crate) fn start(shared: Arc<PlayState>) -> Result<Self> {
        // SAFETY: `libpulse-simple.so.0` is a well-known soname; we only call
        // documented PulseAudio simple-API symbols after `get` succeeds.
        let lib = unsafe { Library::new("libpulse-simple.so.0") }
            .map_err(|e| anyhow!("PulseAudio not available ({e})"))?;
        let pa_simple_new: libloading::Symbol<PaSimpleNew> =
            unsafe { lib.get(b"pa_simple_new\0") }.map_err(|e| anyhow!("pa_simple_new: {e}"))?;
        let pa_simple_write: libloading::Symbol<PaSimpleWrite> =
            unsafe { lib.get(b"pa_simple_write\0") }
                .map_err(|e| anyhow!("pa_simple_write: {e}"))?;
        let pa_simple_free: libloading::Symbol<PaSimpleFree> =
            unsafe { lib.get(b"pa_simple_free\0") }.map_err(|e| anyhow!("pa_simple_free: {e}"))?;

        let pa_simple_new = *pa_simple_new;
        let pa_simple_write = *pa_simple_write;
        let pa_simple_free = *pa_simple_free;

        let stop = Arc::new(AtomicBool::new(false));
        let stop_t = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("cathar-pulse".into())
            .spawn(move || {
                let _lib = lib;
                run(shared, stop_t, pa_simple_new, pa_simple_write, pa_simple_free);
            })
            .map_err(|e| anyhow!("pulse thread: {e}"))?;

        Ok(Self { stop, thread: Some(thread) })
    }
}

impl Drop for PulseOut {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.thread.take() {
            let _ = h.join();
        }
    }
}

fn run(
    shared: Arc<PlayState>,
    stop: Arc<AtomicBool>,
    pa_simple_new: PaSimpleNew,
    pa_simple_write: PaSimpleWrite,
    pa_simple_free: PaSimpleFree,
) {
    let name = CString::new("Cathar").unwrap();
    let stream_name = CString::new("playback").unwrap();
    let mut buf = vec![0.0f32; FRAMES * CHANNELS];
    let mut stream: *mut c_void = std::ptr::null_mut();
    let mut open_rate = 0u32;

    while !stop.load(Ordering::Relaxed) {
        let rate = shared.sample_rate();
        if rate == 0 {
            thread::sleep(Duration::from_millis(10));
            continue;
        }
        if stream.is_null() || open_rate != rate {
            if !stream.is_null() {
                // SAFETY: `stream` came from `pa_simple_new` and is not freed yet.
                unsafe { pa_simple_free(stream) };
            }
            let spec = PaSampleSpec {
                format: PA_SAMPLE_FLOAT32LE,
                rate,
                channels: CHANNELS as u8,
                _pad: [0; 3],
            };
            let mut err = 0;
            // SAFETY: spec/name pointers live for this call; null server/dev
            // means the default Pulse/PipeWire sink.
            stream = unsafe {
                pa_simple_new(
                    std::ptr::null(),
                    name.as_ptr(),
                    PA_STREAM_PLAYBACK,
                    std::ptr::null(),
                    stream_name.as_ptr(),
                    &spec,
                    std::ptr::null(),
                    std::ptr::null(),
                    &mut err,
                )
            };
            if stream.is_null() {
                thread::sleep(Duration::from_millis(200));
                continue;
            }
            open_rate = rate;
        }

        shared.fill(&mut buf);
        let mut err = 0;
        // SAFETY: `stream` is a live `pa_simple`; `buf` is valid for `len*4` bytes.
        let rc = unsafe { pa_simple_write(stream, buf.as_ptr().cast(), buf.len() * 4, &mut err) };
        if rc < 0 {
            unsafe { pa_simple_free(stream) };
            stream = std::ptr::null_mut();
            thread::sleep(Duration::from_millis(50));
        }
    }

    if !stream.is_null() {
        unsafe { pa_simple_free(stream) };
    }
}
